"""Validate category closure and publish one complete development library."""
import hashlib
import json
import os
import shutil
from collections import Counter, defaultdict
from concurrent.futures import ThreadPoolExecutor
from dataclasses import asdict
from importlib.metadata import version
from pathlib import Path
from urllib.parse import unquote, urlsplit

from .archive import load_snapshot, sha256, validate_paths, _check_path
from .records import _file, _json, validate_graph, verify_catalog
from .types import (AssetExport, Catalog, FileRecord, LibraryReceipt, ObjectId,
                    ObjectRecord, SCHEMA_VERSION, Snapshot)

CATEGORIES = ('images', 'ui', 'audio', 'models', 'animations', 'native')
REQUIRED_CATEGORY = {'Texture2D': 'images', 'Sprite': 'images', 'Cubemap': 'images',
                     'Font': 'images', 'Mesh': 'models', 'AudioClip': 'audio',
                     'AnimationClip': 'animations', 'Avatar': 'animations',
                     'AnimatorController': 'animations', 'Canvas': 'ui',
                     'RectTransform': 'ui', 'CanvasRenderer': 'ui'}
EXPECTED_COUNTS = {'Texture2D': 1035, 'Mesh': 2934, 'Material': 897, 'AudioClip': 474,
                   'Sprite': 155, 'Font': 33, 'AnimationClip': 42, 'Avatar': 13,
                   'AnimatorController': 10, 'Cubemap': 2, 'Shader': 118,
                   'ComputeShader': 16, 'TextAsset': 2}


def load_exports(path: Path) -> tuple[AssetExport, ...]:
    data = json.loads(path.read_bytes())
    category = path.parent.name
    if category in ('images', 'ui', 'audio'):
        if not isinstance(data, list):
            raise ValueError(f'Unknown category schema: {path}')
    else:
        fields = {'schema_version', 'exports'} | ({'ownership'} if category == 'animations' else {'managed_links'} if category == 'native' else set())
        if not isinstance(data, dict) or set(data) != fields or data.get('schema_version') != SCHEMA_VERSION:
            raise ValueError(f'Unknown category schema: {path}')
        data = data['exports']
    return tuple(AssetExport(ObjectId(**e['source']), e['category'], e['status'], e['runtime'],
                             e['reason'], tuple(FileRecord(**f) for f in e['files']),
                             tuple(ObjectId(**d) for d in e['dependencies']), e['details']) for e in data)


def load_catalog(path: Path) -> Catalog:
    data = json.loads(path.read_bytes())
    if data.get('schema_version') != SCHEMA_VERSION:
        raise ValueError('Unknown catalog schema')
    objects, dependencies = {}, {}
    for entry in data['objects']:
        identity = ObjectId(**entry['source'])
        if identity in objects:
            raise ValueError(f'Duplicate catalog identity: {identity}')
        objects[identity] = ObjectRecord(identity, entry['type'], entry['class_id'],
                                         entry['offset'], entry['bytes'], entry['container_sha256'])
        dependencies[identity] = tuple(ObjectId(**d) for d in entry['dependencies'])
    return Catalog(objects, data['counts'], dependencies)


def check_installed_counts(catalog: Catalog) -> None:
    if sum(catalog.counts.values()) != 262784:
        raise ValueError('Installed object total differs from 262784')
    for name, count in EXPECTED_COUNTS.items():
        if catalog.counts.get(name) != count:
            raise ValueError(f'Installed {name} count differs from {count}')


def _verify_inputs(snapshot, catalog, exports, output):
    from ..source import Source
    source = Source(snapshot.root)
    verified = verify_catalog(source, snapshot, snapshot.manifest_path.parent / 'catalog.json',
                              snapshot.manifest_path.parent / 'records')
    if verified != catalog:
        raise ValueError('Catalog differs from accepted source records')
    root = snapshot.manifest_path.parent / 'records'
    receipt = json.loads((root / 'manifest.json').read_bytes())
    typed = {f['file']: f for f in receipt['records']}
    for e in exports:
        if e.category == 'native':
            record = catalog.objects[e.source]
            expected = {'file': f'TheLongDrive_Data/{e.source.file}', 'offset': record.offset,
                        'bytes': record.bytes, 'container_sha256': record.container_sha256}
            if e.details.get('native_range') != expected or e.details.get('record') != typed[f'{e.source.file}/{e.source.path_id}.json']:
                raise ValueError('Native record evidence differs from accepted source')
            if record.type == 'TextAsset':
                import struct
                raw = source.get(e.source.file, e.source.path_id).get_raw_data()
                label_size = struct.unpack_from('<i', raw)[0]
                offset = (4 + label_size + 3) & ~3
                size = struct.unpack_from('<i', raw, offset)[0]
                payload = raw[offset + 4:offset + 4 + size]
                if len(e.files) != 1 or len(payload) != size or (output / e.files[0].file).read_bytes() != payload:
                    raise ValueError('Missing required TextAsset source payload')
        if catalog.objects[e.source].type == 'AnimationClip':
            from .animations import decode_clip, _native
            reader = source.get(e.source.file, e.source.path_id)
            decoded = decode_clip(source, reader, catalog)
            expected = _native(asdict(decoded), source, reader, catalog)
            name = e.details.get('tracks_file')
            if not name or name not in {f.file for f in e.files}:
                raise ValueError('Missing required normalized tracks')
            actual = json.loads((output / name).read_bytes())
            for track in actual.get('tracks', []):
                track['binding'].pop('targets', None)
            if actual != expected:
                raise ValueError(f'Normalized tracks differ from source decode: {e.source}')


def _receipt(root, name):
    validate_paths([name])
    path = root / name
    _check_path(path, root.resolve())
    return FileRecord(name, path.stat().st_size, sha256(path))


def _add(files, record):
    validate_paths([record.file])
    if record.file in files and files[record.file] != record:
        raise ValueError(f'Conflicting file hash or size: {record.file}')
    files[record.file] = record


def _check_files(root, files):
    folded = {}
    for name in files:
        if name.casefold() in folded and folded[name.casefold()] != name:
            raise ValueError(f'Case-colliding output: {name}')
        folded[name.casefold()] = name
    resolved = root.resolve()
    checked_parents = set()
    paths = []
    for record in files.values():
        path = root / record.file
        if path.parent not in checked_parents:
            _check_path(path.parent, resolved)
            checked_parents.add(path.parent)
        _check_path(path, resolved)
        paths.append((record, path))
    def check(pair):
        record, path = pair
        if path.stat().st_size != record.bytes or sha256(path) != record.sha256:
            raise ValueError(f'Output hash mismatch: {record.file}')
    with ThreadPoolExecutor(max_workers=8) as pool:
        for _ in pool.map(check, paths):
            pass


def _category_files(snapshot, exports, output):
    grouped = defaultdict(list)
    for e in exports:
        grouped[e.category].append(e)
    files, indexes, measured = {}, [], set()
    def measure(name):
        record = _receipt(output, name)
        measured.add(name)
        return record
    for category in sorted(grouped):
        name = f'{category}/index.json'
        saved = load_exports(output / name)
        if tuple(sorted(grouped[category], key=lambda e: e.source)) != saved:
            raise ValueError(f'Category index differs from owners: {category}')
        record = measure(name)
        _add(files, record); indexes.append(record)
        envelope = json.loads((output / name).read_bytes())
        key = {'animations': 'ownership', 'native': 'managed_links'}.get(category)
        if key:
            _add(files, FileRecord(**envelope[key]))
    source_files = {f.file: f for f in snapshot.files}
    standalone = []
    if 'ui' in grouped:
        name = 'ui/languages.json'
        _add(files, measure(name))
        for entry in json.loads((output / name).read_bytes()):
            source = source_files.get(entry['source_file'])
            result = FileRecord(entry['file'], entry['bytes'], entry['sha256'])
            if source is None or (source.bytes, source.sha256) != (result.bytes, result.sha256):
                raise ValueError('Language source identity or hash mismatch')
            _add(files, result)
            standalone.append({'source': asdict(source), 'output': asdict(result)})
        expected = {f.file for f in snapshot.files if f.file.startswith('TheLongDrive_Data/Languages/')}
        if expected != {e['source']['file'] for e in standalone}:
            raise ValueError('Missing standalone language files')
    if 'native' in grouped:
        name = 'native/source-text.json'
        _add(files, measure(name))
        data = json.loads((output / name).read_bytes())
        if data.get('schema_version') != SCHEMA_VERSION:
            raise ValueError('Unknown source text schema')
        for entry in data['files']:
            source, result = FileRecord(**entry['source']), FileRecord(**entry['output'])
            if source_files.get(source.file) != source or (source.bytes, source.sha256) != (result.bytes, result.sha256):
                raise ValueError('Standalone text source identity or hash mismatch')
            _add(files, result); standalone.append(entry)
        expected = {f.file for f in snapshot.files if f.file.lower().endswith('.txt') and not f.file.startswith('TheLongDrive_Data/Languages/')}
        if expected != {e['source']['file'] for e in data['files']}:
            raise ValueError('Missing standalone source text files')
    for e in exports:
        for record in e.files:
            _add(files, record)
    return files, indexes, standalone, measured


def _size(value):
    if isinstance(value, dict):
        return value.get('$binary', value.get('$byte_array', {})).get('bytes', -1)
    return len(value)


def _required(e, kind, records_root, output):
    expected = REQUIRED_CATEGORY.get(kind)
    if expected is not None and e.category != expected:
        raise ValueError(f'Missing required conversion category: {e.source}')
    if e.status == 'failed':
        raise ValueError(f'Failed required conversion: {e.source}')
    if e.category not in CATEGORIES:
        raise ValueError(f'Unknown category: {e.category}')
    typed = f'records/{e.source.file}/{e.source.path_id}.json'
    if e.details.get('typed_record') != typed:
        raise ValueError(f'Missing typed source record: {e.source}')
    if e.runtime == 'engine_dependent' and not e.reason:
        raise ValueError(f'Missing engine-dependent evidence: {e.source}')
    if e.runtime == 'verified':
        raise ValueError('Library extraction has no browser verification evidence')
    # Native metadata is sufficient only for classes without a required decode.
    required = kind in ('Texture2D', 'Sprite', 'Cubemap', 'Font', 'Mesh', 'AudioClip', 'AnimationClip')
    if required and e.status != 'converted':
        if kind == 'AnimationClip' and e.status == 'native_record':
            pass  # Normalized tracks below remain mandatory for native curves.
        elif kind == 'Font' and e.status == 'native_record':
            data = json.loads((records_root / typed.removeprefix('records/')).read_bytes())['data']
            if _size(data['m_FontData']) != 0 or e.files or e.runtime != 'engine_dependent':
                raise ValueError('Embedded font required conversion is missing')
        elif e.status == 'source_empty' and kind in ('Texture2D', 'Mesh'):
            data = json.loads((records_root / typed.removeprefix('records/')).read_bytes())['data']
            if kind == 'Texture2D':
                empty = not (data['m_Width'] and data['m_Height']) and _size(data['image data']) == 0 and data['m_StreamData']['size'] == 0
            else:
                empty = data['m_VertexData']['m_VertexCount'] == 0 and data.get('m_CompressedMesh', {}).get('m_Vertices', {}).get('m_NumItems', 0) == 0
            if not empty or e.files or not e.reason:
                raise ValueError(f'Invalid source-empty evidence: {e.source}')
        else:
            raise ValueError(f'Missing required conversion: {e.source}')
    elif e.status == 'source_empty':
        raise ValueError(f'Invalid source-empty class: {kind}')
    if e.status == 'converted' and not e.files:
        raise ValueError(f'Missing required conversion files: {e.source}')
    if kind == 'AudioClip':
        if not e.details.get('samples') or not all(f.file.endswith('.wav') for f in e.files):
            raise ValueError('Audio required conversion is missing')
    if kind == 'AnimationClip':
        name = e.details.get('tracks_file')
        if not name or name not in {f.file for f in e.files}:
            raise ValueError(f'Missing required normalized tracks: {e.source}')
        data = json.loads((output / name).read_bytes())
        tracks = data.get('tracks', [])
        if data.get('source') != asdict(e.source) or len(tracks) != e.details.get('track_count'):
            raise ValueError(f'Incomplete normalized tracks: {e.source}')
        if data.get('settings', {}).get('curve_counts') != e.details.get('curve_counts'):
            raise ValueError('Normalized curve count mismatch')
        if sum(e.details['curve_counts'].values()) and not tracks:
            raise ValueError('Missing required normalized tracks')
        for track in tracks:
            if not track['times'] or len(track['times']) != len(track['values']) or any(b < a for a, b in zip(track['times'], track['times'][1:])):
                raise ValueError('Invalid normalized track samples')
        if e.status == 'native_record' and e.details.get('native_track_count') != len(tracks):
            raise ValueError('Native reason hides required generic tracks')
    if e.category == 'native':
        if kind == 'TextAsset' and len(e.files) != 1:
            raise ValueError('Missing required TextAsset source payload')
        if e.status != 'native_record' or e.runtime != 'engine_dependent' or e.details.get('type') != kind:
            raise ValueError('Invalid native metadata classification')
    if e.category == 'models' and e.details.get('type') == 'Hierarchy' and e.runtime == 'engine_dependent':
        unbound = [r for r in e.details['renderers'] if r.get('rig_status') == 'source_unbound_skin']
        if not unbound or any(not r['bones'] or any(r['bones']) or r['root_bone']['m_PathID'] or r['bind_pose_count'] != len(r['bones']) for r in unbound):
            raise ValueError('Invalid source-unbound rig evidence')


def _payloads(e, kind, records_root, output):
    """Decode required payloads and compare their measured source metadata."""
    if e.status != 'converted' or kind not in ('Texture2D', 'Sprite', 'Cubemap', 'Font', 'AudioClip'):
        return
    typed = records_root / e.details['typed_record'].removeprefix('records/')
    data = json.loads(typed.read_bytes())['data']
    def blob(value):
        if isinstance(value, dict):
            record = value.get('$binary', value.get('$byte_array'))
            return (records_root / record['file']).read_bytes()
        return bytes(value)
    if kind == 'AudioClip':
        from .audio import validate_wav
        samples = {item['file']: item for item in e.details['samples']}
        if set(samples) != {f.file for f in e.files}:
            raise ValueError('Missing WAV sample receipts')
        for record in e.files:
            measured = validate_wav(output / record.file, data)
            if any(samples[record.file].get(k) != v for k, v in measured.items()):
                raise ValueError('WAV source sample metadata mismatch')
    elif kind == 'Font':
        from PIL import ImageFont
        if len(e.files) != 1 or (output / e.files[0].file).read_bytes() != blob(data['m_FontData']):
            raise ValueError('Embedded font bytes differ from source')
        ImageFont.truetype(str(output / e.files[0].file), 16)
    elif kind == 'Cubemap':
        from PIL import Image
        if len(e.details.get('faces', [])) != 6 or len(e.files) != 12:
            raise ValueError('Cubemap is missing required faces')
        digest = hashlib.sha256()
        faces = e.details['faces']
        if [f['label'] for f in faces] != ['+X', '-X', '+Y', '-Y', '+Z', '-Z']:
            raise ValueError('Cubemap face order mismatch')
        for face in faces:
            raw = (output / face['file']).read_bytes()
            payload = raw[148:]
            digest.update(payload)
            if hashlib.sha256(payload).hexdigest() != face['payload_sha256']:
                raise ValueError('Cubemap face payload mismatch')
            if len(face['mips']) != data['m_MipCount'] or sum(m['bytes'] for m in face['mips']) != len(payload):
                raise ValueError('Cubemap mip coverage mismatch')
            cursor = 0
            for level, mip in enumerate(face['mips']):
                if mip['level'] != level or mip['offset'] != cursor:
                    raise ValueError('Cubemap mip range mismatch')
                part = payload[cursor:cursor + mip['bytes']]
                if hashlib.sha256(part).hexdigest() != mip['sha256']:
                    raise ValueError('Cubemap mip hash mismatch')
                cursor += mip['bytes']
            with Image.open(output / face['file']) as image:
                if image.size != (data['m_Width'], data['m_Height']) or image.pixel_format != 'BC6H':
                    raise ValueError('Cubemap DDS source format mismatch')
                image.load()
            with Image.open(output / face['preview']) as image:
                image.load()
                if list(image.size) != face['preview_dimensions']:
                    raise ValueError('Cubemap preview dimensions mismatch')
        if digest.hexdigest() != e.details['payload_sha256']:
            raise ValueError('Cubemap complete payload mismatch')
    else:
        from PIL import Image
        if len(e.files) != 1:
            raise ValueError('Missing required source image')
        with Image.open(output / e.files[0].file) as image:
            image.load()
            dimensions = [data['m_Width'], data['m_Height']] if kind == 'Texture2D' else e.details['conversion']['dimensions']
            if image.format != 'PNG' or list(image.size) != dimensions:
                raise ValueError('Source image format or dimensions mismatch')


def _gltf_references(output, files):
    root = output.resolve()
    for name in files:
        if not name.endswith('.gltf'):
            continue
        path = output / name
        data = json.loads(path.read_bytes())
        if data.get('asset', {}).get('version') != '2.0':
            raise ValueError('Unknown glTF schema')
        for resource in [*data.get('buffers', []), *data.get('images', [])]:
            uri = resource.get('uri')
            if not uri or urlsplit(uri).scheme or urlsplit(uri).netloc:
                raise ValueError(f'Invalid library glTF URI: {uri}')
            target = (path.parent / unquote(uri)).resolve()
            if not target.is_relative_to(root):
                raise ValueError(f'glTF URI escapes library: {uri}')
            key = target.relative_to(root).as_posix()
            if key not in files:
                raise ValueError(f'Unowned glTF resource: {uri}')
            if 'byteLength' in resource and files[key].bytes != resource['byteLength']:
                raise ValueError(f'glTF buffer size mismatch: {uri}')
        from ..gltf import validate_gltf
        validate_gltf(path)


def validate_library(snapshot: Snapshot, catalog: Catalog, exports: tuple[AssetExport, ...], output: Path) -> dict:
    validate_graph(catalog)
    if dict(Counter(r.type for r in catalog.objects.values())) != catalog.counts:
        raise ValueError('Catalog category counts mismatch')
    owners = set()
    records_root = snapshot.manifest_path.parent / 'records'
    for e in exports:
        if e.source in owners or e.source not in catalog.objects:
            raise ValueError(f'Duplicate or unknown category owner: {e.source}')
        owners.add(e.source)
        if not set(catalog.dependencies[e.source]).issubset(e.dependencies):
            raise ValueError(f'Missing required dependency: {e.source}')
        if any(d not in catalog.objects for d in e.dependencies):
            raise ValueError(f'Missing non-null dependency: {e.source}')
        # Required-class classification is checked before trusting category receipts.
        kind = catalog.objects[e.source].type
        if REQUIRED_CATEGORY.get(kind, e.category) != e.category:
            raise ValueError(f'Missing required conversion category: {e.source}')
    if owners != catalog.objects.keys():
        raise ValueError('Incomplete category ownership')
    files, indexes, standalone, measured = _category_files(snapshot, exports, output)
    _check_files(output, {name: f for name, f in files.items() if name not in measured})
    _verify_inputs(snapshot, catalog, exports, output)
    totals = defaultdict(Counter)
    for e in exports:
        _required(e, catalog.objects[e.source].type, records_root, output)
        _payloads(e, catalog.objects[e.source].type, records_root, output)
        totals[e.category][e.status] += 1
        totals[e.category]['total'] += 1
    _gltf_references(output, files)
    return {'schema_version': SCHEMA_VERSION, 'source_preservation': {
        'files': len(snapshot.files), 'bytes': sum(f.bytes for f in snapshot.files),
        'objects': len(catalog.objects), 'counts': catalog.counts,
        'dependency_edges': sum(map(len, catalog.dependencies.values()))},
        'development_availability': {k: dict(sorted(v.items())) for k, v in sorted(totals.items())},
        'browser_support': dict(sorted(Counter(e.runtime for e in exports).items())),
        'limits': {'source_empty': dict(sorted(Counter(catalog.objects[e.source].type for e in exports if e.status == 'source_empty').items())),
                   'native_record': dict(sorted(Counter(catalog.objects[e.source].type for e in exports if e.status == 'native_record').items()))},
        'managed_analysis': _verify_managed(snapshot, records_root) if 'native' in totals else None,
        'category_indexes': [asdict(f) for f in indexes],
        'standalone_files': standalone, 'files': [asdict(f) for f in sorted(files.values(), key=lambda f: f.file)]}


def _verify_managed(snapshot, root):
    from .managed import verify_managed_export

    manifest = json.loads((root / 'managed/manifest.json').read_bytes())
    if manifest.get('schema_version') != SCHEMA_VERSION:
        raise ValueError('Unknown managed manifest schema')
    files = {}
    for item in manifest['files']:
        if item['file'] in files:
            raise ValueError('Duplicate managed analysis receipt')
        _add(files, FileRecord(**item))
    assemblies = tuple(item for item in snapshot.files if item.file.startswith('TheLongDrive_Data/Managed/')
                       and item.file.lower().endswith('.dll'))
    report, managed_files = verify_managed_export(snapshot.root, assemblies, root, snapshot.manifest_sha256)
    expected = {item.file: item for item in managed_files}
    expected['managed/analysis.json'] = _receipt(root, 'managed/analysis.json')
    if files != expected:
        raise ValueError('Incomplete managed analysis receipts')
    _check_files(root, files)
    analysis = json.loads((root / 'managed/analysis.json').read_bytes())
    if analysis.get('schema_version') != SCHEMA_VERSION:
        raise ValueError('Unknown managed analysis schema')
    if analysis['managed_files'] != [asdict(item) for item in managed_files]:
        raise ValueError('Managed analysis report receipts mismatch')
    actual = {path.relative_to(root).as_posix() for path in (root / 'managed').rglob('*') if path.is_file()}
    if actual != set(files) | {'managed/manifest.json'}:
        raise ValueError('Untracked managed analysis files')
    return {'root': 'records', 'manifest': asdict(_receipt(root, 'managed/manifest.json')),
            'files': [asdict(f) for f in sorted(files.values(), key=lambda f: f.file)]}


def _publish_directory(work, output):
    import logging
    from uuid import uuid4

    output.parent.mkdir(parents=True, exist_ok=True)
    backup = work.parent / f'previous-{output.name}-{uuid4().hex}'
    had_output = output.exists()
    if had_output:
        output.replace(backup)
    try:
        work.replace(output)
    except BaseException:
        if had_output:
            backup.replace(output)
        raise
    if had_output:
        # The swap committed. Cleanup cannot roll back a partially deleted backup.
        try:
            shutil.rmtree(backup)
        except OSError as error:
            logging.getLogger(__name__).warning(
                'Published %s; backup cleanup failed at %s: %s', output, backup, error)


def publish_library(snapshot: Snapshot, catalog: Catalog, exports: tuple[AssetExport, ...], work: Path, output: Path) -> LibraryReceipt:
    exports = tuple(sorted(exports, key=lambda e: e.source))
    work, output = work.resolve(), output.resolve()
    pending_root = (snapshot.manifest_path.parent / 'work').resolve()
    if not work.is_relative_to(pending_root) or work == pending_root:
        raise ValueError('Library work must remain under the source stage work directory')
    if (output.is_relative_to(snapshot.manifest_path.parent.resolve())
            or snapshot.manifest_path.parent.resolve().is_relative_to(output) or work == output):
        raise ValueError('Library output overlaps source staging')
    coverage = validate_library(snapshot, catalog, exports, work)
    actual = {p.relative_to(work).as_posix() for p in work.rglob('*') if p.is_file()}
    if actual - {'coverage.json', 'manifest.json'} != {f['file'] for f in coverage['files']}:
        raise ValueError('Untracked staging output files')
    catalog_hash = sha256(snapshot.manifest_path.parent / 'catalog.json')
    record_manifest = _receipt(snapshot.manifest_path.parent / 'records', 'manifest.json')
    coverage_file = _file(work, 'coverage.json', _json(coverage))
    manifest = {'schema_version': SCHEMA_VERSION, 'source_manifest_sha256': snapshot.manifest_sha256,
        'catalog_sha256': catalog_hash, 'records_root': Path(os.path.relpath(snapshot.manifest_path.parent / 'records', output)).as_posix(),
        'records_manifest': asdict(record_manifest), 'managed_analysis': coverage['managed_analysis'], 'coverage': asdict(coverage_file),
        'category_indexes': coverage['category_indexes'], 'standalone_files': coverage['standalone_files'],
        'files': sorted([*coverage['files'], asdict(coverage_file)], key=lambda f: f['file']),
        'converter_versions': {name: version(name) for name in ('UnityPy', 'Pillow', 'pygltflib')},
        'coverage_totals': {key: coverage[key] for key in ('source_preservation', 'development_availability', 'browser_support', 'limits')}}
    # This is the final write. A rejected stage never changes accepted output.
    receipt = _file(work, 'manifest.json', _json(manifest))
    _publish_directory(work, output)
    return LibraryReceipt(output / 'manifest.json', receipt.sha256, catalog_hash, snapshot.manifest_sha256, exports)


def verify_library(output: Path, source_root: Path) -> LibraryReceipt:
    output = output.resolve()
    snapshot = load_snapshot(source_root.resolve().parent)
    if snapshot.root.resolve() != source_root.resolve():
        raise ValueError('Library source root differs from the accepted source stage')
    manifest_path = output / 'manifest.json'
    manifest = json.loads(manifest_path.read_bytes())
    if manifest.get('schema_version') != SCHEMA_VERSION:
        raise ValueError('Unknown library schema')
    records = snapshot.manifest_path.parent / 'records'
    expected_root = Path(os.path.relpath(records, output)).as_posix()
    if manifest['records_root'] != expected_root:
        raise ValueError('Library records root differs from verified source storage')
    if manifest['source_manifest_sha256'] != snapshot.manifest_sha256 or manifest['catalog_sha256'] != sha256(snapshot.manifest_path.parent / 'catalog.json'):
        raise ValueError('Library source or catalog hash mismatch')
    _check_files(records, {'manifest.json': FileRecord(**manifest['records_manifest'])})
    files = {}
    for item in manifest['files']:
        _add(files, FileRecord(**item))
    _check_files(output, {manifest['coverage']['file']: FileRecord(**manifest['coverage'])})
    for item in manifest['category_indexes']:
        validate_paths([item['file']])
    if manifest.get('converter_versions') != {name: version(name) for name in ('UnityPy', 'Pillow', 'pygltflib')}:
        raise ValueError('Unknown converter versions')
    exports = tuple(sorted((e for item in manifest['category_indexes'] for e in load_exports(output / item['file'])), key=lambda e: e.source))
    catalog = load_catalog(snapshot.manifest_path.parent / 'catalog.json')
    coverage = validate_library(snapshot, catalog, exports, output)
    if coverage != json.loads((output / manifest['coverage']['file']).read_bytes()):
        raise ValueError('Library coverage differs from verified assets')
    expected_files = sorted([*coverage['files'], manifest['coverage']], key=lambda f: f['file'])
    if manifest['files'] != expected_files or manifest['category_indexes'] != coverage['category_indexes'] or manifest['standalone_files'] != coverage['standalone_files']:
        raise ValueError('Incomplete library file receipts')
    if manifest.get('managed_analysis') != coverage['managed_analysis']:
        raise ValueError('Managed analysis receipts mismatch')
    if manifest['coverage_totals'] != {key: coverage[key] for key in ('source_preservation', 'development_availability', 'browser_support', 'limits')}:
        raise ValueError('Library coverage totals mismatch')
    actual = {p.relative_to(output).as_posix() for p in output.rglob('*') if p.is_file()}
    if actual != set(files) | {'manifest.json'}:
        raise ValueError('Untracked library output files')
    return LibraryReceipt(manifest_path, sha256(manifest_path), manifest['catalog_sha256'], snapshot.manifest_sha256, exports)


def assemble_library(snapshot, exports_by_root, work):
    """Copy only receipted category files into one canonical staging root."""
    files, exports = {}, []
    def copy(root, record):
        old = files.get(record.file)
        _add(files, record)
        if old is not None:
            return
        path = root / record.file
        validate_paths([record.file])
        _check_path(path, root.resolve())
        target = work / record.file
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, target)
    for category, root, items in exports_by_root:
        name = f'{category}/index.json'
        copy(root, _receipt(root, name))
        exports.extend(items)
        for e in items:
            if e.category != category:
                raise ValueError('Category input contains another owner category')
            for record in e.files:
                copy(root, record)
        if category in ('animations', 'native'):
            envelope = json.loads((root / name).read_bytes())
            key = 'ownership' if category == 'animations' else 'managed_links'
            copy(root, FileRecord(**envelope[key]))
        if category == 'ui':
            name = 'ui/languages.json'
            copy(root, _receipt(root, name))
            for entry in json.loads((root / name).read_bytes()):
                copy(root, FileRecord(entry['file'], entry['bytes'], entry['sha256']))
        if category == 'native':
            name = 'native/source-text.json'
            copy(root, _receipt(root, name))
            for entry in json.loads((root / name).read_bytes())['files']:
                copy(root, FileRecord(**entry['output']))
    return tuple(sorted(exports, key=lambda e: e.source))
