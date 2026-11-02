"""Write typed records and validate the complete source graph."""
import hashlib
import json
import math
import stat
from dataclasses import asdict
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path, PurePosixPath

from .inventory import inventory_source
from .types import AssetExport, Catalog, DATA_DIRECTORY, FileRecord, ObjectId, SCHEMA_VERSION
from ..source import Source


def _json(data):
    return (json.dumps(data, ensure_ascii=True, sort_keys=True, separators=(',', ':'), allow_nan=False) + '\n').encode('utf-8')


def _file(root, name, content):
    path = root / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(content)
    return FileRecord(name, len(content), hashlib.sha256(content).hexdigest())


def _tree_data(node):
    return {'type': node.m_Type, 'name': node.m_Name,
            'size': getattr(node, 'm_ByteSize', None), 'flags': getattr(node, 'm_MetaFlag', None),
            'children': [_tree_data(child) for child in node.m_Children]}


def _write_decoded(source, reader, records_root, data):
    identity = ObjectId(source.container_path(reader), reader.path_id)
    name = f'{identity.file}/{identity.path_id}.json'
    sidecar_index = 0
    def encode(value, node=None):
        nonlocal sidecar_index
        if isinstance(value, (bytes, bytearray, memoryview)):
            sidecar_name = f'{identity.file}/{identity.path_id}.{sidecar_index}.bin'
            sidecar_index += 1
            return {'$binary': asdict(_file(records_root, sidecar_name, bytes(value)))}
        if isinstance(value, float) and not math.isfinite(value):
            return {'$float': 'nan' if math.isnan(value) else '+inf' if value > 0 else '-inf'}
        if node is not None and node.m_Type == 'pair':
            return {'$pair': [encode(item, child) for item, child in zip(value, node.m_Children)]}
        if node is not None and node.m_Type != 'string' and node.m_Children and node.m_Children[0].m_Type == 'Array':
            element = node.m_Children[0].m_Children[1]
            if element.m_Type in ('UInt8', 'char') and len(value) > 1024:
                return {'$byte_array': {'element_type': element.m_Type, **encode(bytes(value))['$binary']}}
            return [encode(item, element) for item in value]
        if isinstance(value, tuple):
            raise TypeError(f'Untyped tuple: {identity}')
        if isinstance(value, list):
            return [encode(item) for item in value]
        if isinstance(value, dict):
            fields = {child.m_Name: child for child in node.m_Children} if node is not None else {}
            return {key: encode(item, fields.get(key)) for key, item in value.items()}
        if value is None or isinstance(value, (str, int, float, bool)):
            return value
        raise TypeError(f'Unsupported typed value: {identity}: {type(value).__name__}')
    tree = source.record_tree(reader)
    # Type trees are small shared schemas. The cache contains no decoded payloads.
    schemas = getattr(source, 'record_schema_cache', None)
    if schemas is None:
        schemas = source.record_schema_cache = {}
    schemas = schemas.setdefault(str(records_root.resolve()), {})
    tree_key = id(tree)
    if tree_key not in schemas:
        content = _json(_tree_data(tree))
        digest = hashlib.sha256(content).hexdigest()
        schemas[tree_key] = _file(records_root, f'types/{digest}.json', content)
    payload = {'schema_version': SCHEMA_VERSION, 'source': asdict(identity), 'type': reader.type.name,
               'class_id': reader.class_id, 'bytes': reader.byte_size,
               'complete_byte_consumption': True, 'type_tree': asdict(schemas[tree_key]), 'data': encode(data, tree)}
    return _file(records_root, name, _json(payload))


def write_record(source, reader, records_root: Path) -> FileRecord:
    return _write_decoded(source, reader, records_root, source.read_record(reader))


def _walk(node, value, path='', relevant=None):
    if relevant is not None and id(node) not in relevant:
        return
    kind = node.m_Type
    if kind.startswith('PPtr<') or kind in ('StreamingInfo', 'StreamedResource'):
        yield kind, value, path
        return
    if not node.m_Children or kind in ('string', 'TypelessData'):
        return
    if kind == 'pair':
        for child, item in zip(node.m_Children, value):
            yield from _walk(child, item, f'{path}.{child.m_Name}', relevant)
    elif node.m_Children[0].m_Type == 'Array':
        element = node.m_Children[0].m_Children[1]
        for index, item in enumerate(value):
            yield from _walk(element, item, f'{path}[{index}]', relevant)
    else:
        for child in node.m_Children:
            if child.m_Name in value:
                yield from _walk(child, value[child.m_Name], f'{path}.{child.m_Name}', relevant)


def _reference_fields(source, reader, data):
    tree = source.record_tree(reader)
    cache = getattr(source, 'reference_tree_cache', None)
    if cache is None:
        cache = source.reference_tree_cache = {}
    if id(tree) not in cache:
        relevant = set()
        def visit(node):
            found = node.m_Type.startswith('PPtr<') or node.m_Type in ('StreamingInfo', 'StreamedResource')
            for child in node.m_Children:
                found = visit(child) or found
            if found:
                relevant.add(id(node))
            return found
        visit(tree)
        cache[id(tree)] = relevant
    return _walk(tree, data, relevant=cache[id(tree)])


def _external_file(source, reader, file_id):
    if file_id == 0:
        return source.container_path(reader)
    if file_id < 1 or file_id > len(reader.assets_file.externals):
        raise ValueError(f'Invalid owner file ID: {file_id}')
    path = reader.assets_file.externals[file_id - 1].path.replace('\\', '/')
    # Unity declares its installed Resources file with a Library virtual path.
    if path == 'Library/unity default resources':
        path = 'Resources/unity default resources'
    if path not in source.serialized_files:
        raise ValueError(f'Missing external container: {path}')
    return path


def _pointer(source, reader, value):
    if value['m_PathID'] == 0:
        return None
    target = ObjectId(_external_file(source, reader, value['m_FileID']), value['m_PathID'])
    if (target.file, target.path_id) not in source.objects:
        raise ValueError(f'Missing pointer target: {target}')
    return target


def _stream(source, kind, value):
    if kind == 'StreamingInfo':
        path, offset, size = value['path'], value['offset'], value['size']
    else:
        path, offset, size = value['m_Source'], value['m_Offset'], value['m_Size']
    if size == 0 and path == '':
        if offset < 0:
            raise ValueError('Negative empty stream offset')
        return None
    name = PurePosixPath(path.replace('\\', '/')).as_posix()
    if name not in source.resource_streams:
        raise ValueError(f'Missing stream: {name}')
    file = source.root / DATA_DIRECTORY / name
    if offset < 0 or size < 0 or offset + size > file.stat().st_size:
        raise ValueError(f'Stream range escapes source file: {name}: {offset}+{size}')
    return {'file': name, 'offset': offset, 'bytes': size}


def resolve_dependencies(source, reader, data: dict) -> tuple[ObjectId, ...]:
    targets = set()
    for kind, value, field in _reference_fields(source, reader, data):
        try:
            if kind.startswith('PPtr<'):
                target = _pointer(source, reader, value)
                if target is not None:
                    targets.add(target)
            else:
                _stream(source, kind, value)
        except Exception as error:
            raise ValueError(f'{source.container_path(reader)}:{reader.path_id} {reader.type.name} {field}: {error}') from error
    return tuple(sorted(targets))


def validate_transform_parents(parents):
    done = set()
    for identity in parents:
        path = set()
        current = identity
        while current is not None and current not in done:
            if current in path:
                raise ValueError(f'Transform parent cycle: {current}')
            if current not in parents:
                raise ValueError(f'Missing Transform parent: {current}')
            path.add(current)
            current = parents[current]
        done.update(path)


def validate_graph(catalog: Catalog) -> None:
    if catalog.objects.keys() != catalog.dependencies.keys():
        raise ValueError('Missing dependency graph keys')
    for identity, record in catalog.objects.items():
        if record.source != identity or record.offset < 0 or record.bytes < 0:
            raise ValueError(f'Invalid object range or identity: {identity}')
        for target in catalog.dependencies[identity]:
            if target not in catalog.objects:
                raise ValueError(f'Missing dependency target: {identity} -> {target}')
        if record.type == 'GameObject':
            transforms = [target for target in catalog.dependencies[identity] if catalog.objects[target].type in ('Transform', 'RectTransform')]
            if len(transforms) != 1:
                raise ValueError(f'GameObject Transform ownership mismatch: {identity}')
        if record.type in ('Transform', 'RectTransform'):
            owners = [target for target in catalog.dependencies[identity] if catalog.objects[target].type == 'GameObject']
            if len(owners) != 1 or identity not in catalog.dependencies[owners[0]]:
                raise ValueError(f'Transform ownership mismatch: {identity}')


def build_catalog(source, snapshot, records_root: Path) -> Catalog:
    inventory = inventory_source(source, snapshot)
    dependencies, parents, children, owners, components, receipts = {}, {}, {}, {}, {}, []
    stream_ranges = []
    for index, (identity, record) in enumerate(sorted(inventory.objects.items()), 1):
        reader = source.get(identity.file, identity.path_id)
        try:
            data = source.read_record(reader)
            dependencies[identity] = resolve_dependencies(source, reader, data)
            if record.type in ('Transform', 'RectTransform'):
                parents[identity] = _pointer(source, reader, data['m_Father'])
                children[identity] = tuple(_pointer(source, reader, value) for value in data['m_Children'])
            if 'm_GameObject' in data:
                # Ownership fields are verified only when their tree declares a PPtr.
                fields = {child.m_Name: child for child in source.record_tree(reader).m_Children}
                if 'm_GameObject' in fields and fields['m_GameObject'].m_Type.startswith('PPtr<'):
                    owners[identity] = _pointer(source, reader, data['m_GameObject'])
            if record.type == 'GameObject':
                components[identity] = tuple(_pointer(source, reader, item['component']) for item in data['m_Component'])
            for kind, value, field in _reference_fields(source, reader, data):
                if kind in ('StreamingInfo', 'StreamedResource'):
                    byte_range = _stream(source, kind, value)
                    if byte_range:
                        stream_ranges.append({'source': asdict(identity), 'field': field, **byte_range})
            receipts.append(_write_decoded(source, reader, records_root, data))
            del data
        except Exception as error:
            raise ValueError(f'Record decode failed: {identity.file}:{identity.path_id} {record.type}: {error}') from error
        if index % 10000 == 0:
            print(f'Wrote records: {index:,}/{len(inventory.objects):,}', flush=True)
    catalog = Catalog(inventory.objects, inventory.counts, dependencies)
    validate_graph(catalog)
    validate_transform_parents(parents)
    for identity, parent in parents.items():
        if parent is not None and identity not in children[parent]:
            raise ValueError(f'Transform parent omits child: {identity}')
        for child in children[identity]:
            if child is None or child not in parents or parents[child] != identity:
                raise ValueError(f'Transform child has different parent: {identity}: {child}')
    for game_object, parts in components.items():
        for part in parts:
            if part is None or owners.get(part) != game_object:
                raise ValueError(f'GameObject component ownership mismatch: {game_object}: {part}')
    for component, owner in owners.items():
        if owner is not None and component not in components.get(owner, ()):
            raise ValueError(f'Component owner omits component: {component}: {owner}')
    schemas = tuple(sorted(set(source.record_schema_cache[str(records_root.resolve())].values()), key=lambda item: item.file))
    manifest = {'schema_version': SCHEMA_VERSION, 'source_manifest_sha256': snapshot.manifest_sha256,
                'record_count': len(receipts), 'records': [asdict(item) for item in receipts],
                'type_trees': [asdict(item) for item in schemas], 'stream_ranges': stream_ranges,
                'checks': {'complete_byte_consumption': True, 'graph_closed': True,
                           'transform_parent_cycles': 0, 'reciprocal_hierarchy': True,
                           'reciprocal_component_ownership': True}}
    _file(records_root, 'manifest.json', _json(manifest))
    return catalog


def verify_catalog(source, snapshot, catalog_path: Path, records_root: Path) -> Catalog:
    """Check accepted record hashes without changing accepted files."""
    from .inventory import catalog_data
    from .archive import validate_paths, _check_path
    resolved_root = records_root.resolve()
    _check_path(records_root, resolved_root)
    verified_parents = {records_root}
    receipt = json.loads((records_root / 'manifest.json').read_bytes())
    raw = catalog_path.read_bytes()
    if receipt['schema_version'] != SCHEMA_VERSION or receipt['source_manifest_sha256'] != snapshot.manifest_sha256:
        raise ValueError('Catalog receipt schema or source mismatch')
    if hashlib.sha256(raw).hexdigest() != receipt['catalog_sha256']:
        raise ValueError('Catalog hash mismatch')
    data = json.loads(raw)
    inventory = inventory_source(source, snapshot)
    dependencies = {ObjectId(**item['source']): tuple(ObjectId(**value) for value in item['dependencies']) for item in data['objects']}
    catalog = Catalog(inventory.objects, inventory.counts, dependencies)
    if catalog_data(catalog, source, snapshot) != data:
        raise ValueError('Catalog source inventory mismatch')
    validate_graph(catalog)
    expected_checks = {'complete_byte_consumption': True, 'graph_closed': True, 'transform_parent_cycles': 0,
                       'reciprocal_hierarchy': True, 'reciprocal_component_ownership': True}
    if receipt['checks'] != expected_checks or receipt['record_count'] != len(catalog.objects):
        raise ValueError('Missing accepted graph checks')
    def checked_path(record):
        validate_paths([record['file']])
        path = records_root / record['file']
        if path.parent not in verified_parents:
            _check_path(path.parent, resolved_root)
            verified_parents.add(path.parent)
        attributes = path.lstat()
        if stat.S_ISLNK(attributes.st_mode) or getattr(attributes, 'st_file_attributes', 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT:
            raise ValueError(f'Record file is a filesystem link: {path}')
        return path
    def read_file(record, path):
        content = path.read_bytes()
        if len(content) != record['bytes'] or hashlib.sha256(content).hexdigest() != record['sha256']:
            raise ValueError(f'Record hash mismatch: {record["file"]}')
        return content
    def check_file(record):
        return read_file(record, checked_path(record))
    verified_trees = {}
    for item in receipt['type_trees']:
        check_file(item)
        verified_trees[item['file']] = item
    def binaries(value):
        if isinstance(value, dict):
            if '$binary' in value:
                check_file(value['$binary'])
            elif '$byte_array' in value:
                check_file(value['$byte_array'])
            else:
                for item in value.values():
                    binaries(item)
        elif isinstance(value, list):
            for item in value:
                binaries(item)
    seen = set()
    with ThreadPoolExecutor(max_workers=8) as pool:
        for start in range(0, len(receipt['records']), 64):
            batch = receipt['records'][start:start + 64]
            # Independent file path checks, reads, and hashes run in bounded workers.
            # Typed decoding and graph checks remain on this thread.
            for item, content in zip(batch, pool.map(check_file, batch)):
                payload = json.loads(content)
                identity = ObjectId(**payload['source'])
                if identity in seen or identity not in catalog.objects:
                    raise ValueError(f'Duplicate or unknown typed record: {identity}')
                seen.add(identity)
                record = catalog.objects[identity]
                if payload['type'] != record.type or payload['class_id'] != record.class_id or payload['bytes'] != record.bytes:
                    raise ValueError(f'Typed record identity mismatch: {identity}')
                if payload['schema_version'] != SCHEMA_VERSION or not payload['complete_byte_consumption']:
                    raise ValueError(f'Incomplete typed record: {identity}')
                if verified_trees.get(payload['type_tree']['file']) != payload['type_tree']:
                    raise ValueError(f'Unknown type tree: {identity}')
                binaries(payload['data'])
                if len(seen) % 10000 == 0:
                    print(f'Verified records: {len(seen):,}/{len(catalog.objects):,}', flush=True)
    if seen != catalog.objects.keys():
        raise ValueError('Missing typed records')
    for byte_range in receipt['stream_ranges']:
        file = source.root / DATA_DIRECTORY / byte_range['file']
        if byte_range['file'] not in source.resource_streams or byte_range['offset'] < 0 or byte_range['bytes'] < 0 or byte_range['offset'] + byte_range['bytes'] > file.stat().st_size:
            raise ValueError(f'Invalid stream range: {byte_range}')
    return catalog


# UnityPy diagnostic text fails for these fixed-build programs. Native records remain complete.
SHADER_TEXT_FAILURES = frozenset((ObjectId('globalgamemanagers.assets', 3),
    ObjectId('globalgamemanagers.assets', 14), ObjectId('globalgamemanagers.assets', 16),
    ObjectId('sharedassets0.assets', 714)))


def _shader_text(source, identity):
    diagnostic = {'status': 'not_requested', 'original_source_recovered': False,
                  'reason': 'Shipped GPU programs retain native bytes and parsed metadata; diagnostic text is optional.'}
    if identity in SHADER_TEXT_FAILURES:
        try:
            source.get(identity.file, identity.path_id).parse_as_object(check_read=True).export()
        except Exception as error:
            diagnostic = {'status': 'failed_optional', 'original_source_recovered': False,
                          'reason': f'{type(error).__name__}: {error}'}
        else:
            diagnostic['reason'] = 'Previously failed diagnostic text export now succeeds; original source remains unavailable.'
    return diagnostic


def export_native_assets(source: Source, catalog: Catalog, records_root: Path,
                         claimed: frozenset[ObjectId]) -> tuple[AssetExport, ...]:
    """Index the exact remaining owners without changing accepted typed records."""
    unknown = claimed - catalog.objects.keys()
    if unknown:
        raise ValueError(f'Unknown claimed identities: {sorted(unknown)}')
    if catalog.objects.keys() != catalog.dependencies.keys():
        raise ValueError('Missing dependency graph keys')
    for identity, targets in catalog.dependencies.items():
        for target in targets:
            if target not in catalog.objects:
                raise ValueError(f'Missing dependency target: {identity} -> {target}')
    manifest = json.loads((records_root / 'manifest.json').read_bytes())
    if manifest['schema_version'] != SCHEMA_VERSION:
        raise ValueError('Obsolete typed record schema')
    receipts = {item['file']: item for item in manifest['records']}
    exports = []
    for identity in sorted(catalog.objects.keys() - claimed):
        entry = catalog.objects[identity]
        name = f'{identity.file}/{identity.path_id}.json'
        if name not in receipts:
            raise ValueError(f'Missing accepted typed record: {identity}')
        details = {'type': entry.type, 'typed_record': f'records/{name}',
                   'record': receipts[name], 'native_range': {
                       'file': f'{DATA_DIRECTORY}/{identity.file}', 'offset': entry.offset,
                       'bytes': entry.bytes, 'container_sha256': entry.container_sha256}}
        files = ()
        if entry.type in ('Shader', 'ComputeShader'):
            details['representation'] = 'shipped_programs_and_settings'
            details['shader_text'] = _shader_text(source, identity) if entry.type == 'Shader' else {
                'status': 'not_requested', 'original_source_recovered': False,
                'reason': 'The typed record retains shipped compute programs and settings.'}
        elif entry.type == 'RenderTexture':
            details['representation'] = 'runtime_descriptor'
            details['pixels'] = 'created_by_engine'
        elif entry.type == 'TextAsset':
            import struct
            reader = source.get(identity.file, identity.path_id)
            raw = reader.get_raw_data()
            label_size = struct.unpack_from('<i', raw)[0]
            offset = (4 + label_size + 3) & ~3
            size = struct.unpack_from('<i', raw, offset)[0]
            payload = raw[offset + 4:offset + 4 + size]
            if label_size < 0 or size < 0 or len(payload) != size:
                raise ValueError(f'Invalid TextAsset payload: {identity}')
            receipt = _file(records_root, f'native/text/{identity.file}/{identity.path_id}.bin', payload)
            files = (receipt,)
            try:
                payload.decode('utf-8')
                encoding = 'utf-8'
            except UnicodeDecodeError:
                encoding = 'binary'
            details['text'] = {'encoding': encoding, 'bytes': size,
                               'source_payload_offset': entry.offset + offset + 4,
                               'payload_sha256': receipt.sha256}
        exports.append(AssetExport(identity, 'native', 'native_record', 'engine_dependent',
                       'Complete typed metadata and original source bytes; engine execution requires an adapter.',
                       files, catalog.dependencies[identity], details))
    from .managed import native_managed_links
    links = native_managed_links(source, catalog, records_root)
    _file(records_root, 'native/index.json', _json({'schema_version': SCHEMA_VERSION,
          'exports': [asdict(item) for item in exports], 'managed_links': asdict(links)}))
    return tuple(exports)
