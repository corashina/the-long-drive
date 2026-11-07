"""Receipts and selected scene plans for the immutable runtime package."""
import gzip
import hashlib
import json
import re
import shutil
import struct
from pathlib import Path, PurePosixPath

MAX_U32 = 2**32 - 1
MAX_STAGING = 32 * 1024 * 1024
# The exporter manifest JSON. The tools read it. Gzip at rest.
MANIFEST = 'manifest.json.gz'
# `content_base` and `file_catalog` of the manifest. Deploy reads them here.
CATALOG = 'catalog.json'
# A start pack holds at most this many entry bytes. Its gzip file stays under the Pages 25 MiB limit.
PACK_BYTES = 16 * 1024 * 1024
# Usages that load as fixed scene roots at startup. Same list as `scene_root` in src/presentation/fixed_assets.rs.
SCENE_ROOTS = {'unique_scene', 'road_lamp_prototype', 'sky_prototype', 'food_prototype', 'item_prototype',
               'player_prototype', 'ragdoll_prototype', 'sky_traffic', 'creature_prototype'}
# The game manifest file: the manifest as postcard. The game contract makes it from the exporter JSON.
RUNTIME_MANIFEST = 'manifest.bin.gz'


def write_manifest(output: Path, manifest: dict) -> None:
    from .export import encode_runtime_manifest
    content = json.dumps(manifest, separators=(',', ':'), allow_nan=False).encode()
    # mtime 0 keeps the file bytes the same for the same manifest.
    (output / MANIFEST).write_bytes(gzip.compress(content, compresslevel=9, mtime=0))
    runtime = encode_runtime_manifest(content)
    (output / RUNTIME_MANIFEST).write_bytes(gzip.compress(runtime, compresslevel=9, mtime=0))
    catalog = {'content_base': manifest['content_base'], 'file_catalog': manifest['file_catalog']}
    (output / CATALOG).write_text(json.dumps(catalog, separators=(',', ':'), allow_nan=False), encoding='utf-8')


def runtime_manifest_bytes(output: Path) -> bytes:
    return gzip.decompress((output / RUNTIME_MANIFEST).read_bytes())


def manifest_bytes(output: Path) -> bytes:
    return gzip.decompress((output / MANIFEST).read_bytes())


def read_catalog(output: Path) -> dict:
    return json.loads((output / CATALOG).read_bytes())


def checked(value):
    if type(value) is not int or not 0 <= value <= MAX_U32:
        raise ValueError('Prototype plan arithmetic overflow')
    return value


def canonical(value):
    if not isinstance(value, str) or not re.fullmatch(r'[a-zA-Z0-9_./ -]+', value) or any(
            not part or part in ('.', '..') or part.startswith(' ') or part.endswith(' ')
            for part in value.split('/')):
        raise ValueError('Invalid package path')
    return value


def file_receipt(path: Path, name: str) -> dict:
    data = path.read_bytes()
    result = {'file': canonical(name), 'bytes': checked(len(data)),
              'sha256': hashlib.sha256(data).hexdigest(), 'image_size': None}
    if path.suffix == '.png':
        if len(data) < 33 or data[:16] != b'\x89PNG\r\n\x1a\n\0\0\0\rIHDR':
            raise ValueError('Invalid PNG header')
        width, height = struct.unpack('>II', data[16:24])
        if not width or not height:
            raise ValueError('Invalid PNG dimensions')
        checked(width * height * 4)
        result['image_size'] = [width, height]
    return result


def build_prototype_plan(document: dict, receipts: dict) -> dict:
    """Use the final document receipt and the package file lookup."""
    files = receipts['files']
    current = receipts['document']
    buffer_bytes = 0
    for buffer in document.get('buffers', []):
        uri = buffer['uri']
        if PurePosixPath(uri).name != uri or uri in ('.', '..'):
            raise ValueError('Invalid prototype buffer URI')
        name = str(PurePosixPath(current['file']).parent / uri)
        size = checked(buffer['byteLength'])
        if name not in files or files[name]['bytes'] != size:
            raise ValueError('Prototype buffer length differs from receipt')
        buffer_bytes = checked(buffer_bytes + size)
    accessors = document.get('accessors', [])
    primitives = []
    for mesh_index, mesh in enumerate(document.get('meshes', [])):
        for primitive_index, primitive in enumerate(mesh['primitives']):
            if primitive.get('mode', 4) != 4:
                raise ValueError('Unsupported source primitive topology')
            attributes = primitive['attributes']
            vertices = checked(accessors[attributes['POSITION']]['count'])
            indices = checked(accessors[primitive['indices']]['count']) if 'indices' in primitive else vertices
            stride = 0
            for index in attributes.values():
                width = {'SCALAR': 1, 'VEC2': 2, 'VEC3': 3, 'VEC4': 4, 'MAT4': 16}.get(accessors[index]['type'])
                if not width:
                    raise ValueError('Unsupported source vertex attribute')
                stride = checked(stride + width * 4)
            extras = primitive.get('extras', {})
            material = document['materials'][primitive['material']].get('extras') if 'material' in primitive else None
            # Preserve authored null slots. Runtime admission checks their source assignments.
            primitives.append({'mesh': mesh_index, 'primitive': primitive_index,
                'submesh': checked(extras['submesh']), 'vertices': vertices, 'indices': indices,
                'bytes': checked(vertices * stride + indices * 4), 'attributes': list(attributes),
                'source_material': extras.get('world_source_material'),
                'has_source_material': 'world_source_material' in extras, 'material': material})
    nodes = [{'index': index, 'mesh': node.get('mesh'), 'skin': node.get('skin'),
              'children': node.get('children', []), 'matrix': node.get('matrix'),
              'translation': node.get('translation', [0, 0, 0]),
              'rotation': node.get('rotation', [0, 0, 0, 1]),
              'scale': node.get('scale', [1, 1, 1]), 'extras': node.get('extras')}
             for index, node in enumerate(document.get('nodes', []))]
    mesh_bytes = checked(sum(p['bytes'] + p['vertices'] * 32 for node in nodes
                             for p in primitives if node['mesh'] == p['mesh']))
    scene_bytes = checked(len(nodes) * 2048 + len(primitives) * 2048
                          + sum(len(s['joints']) * 128 for s in document.get('skins', [])))
    count = sum(1 for node in nodes for p in primitives if node['mesh'] == p['mesh'])
    checked(current['bytes'] * 18 + buffer_bytes * 2 + mesh_bytes * 2 + scene_bytes + count * 4096)
    checked(mesh_bytes + count * 8)
    return {'json_bytes': current['bytes'], 'buffer_bytes': buffer_bytes,
            'mesh_bytes': mesh_bytes, 'scene_bytes': scene_bytes, 'nodes': nodes, 'primitives': primitives}


def build_file_catalog(manifest: dict, output: Path) -> dict:
    excluded = {MANIFEST, CATALOG, RUNTIME_MANIFEST, 'library-selection.json'}
    files = {f'reference/{path.name}': file_receipt(path, f'reference/{path.name}')
             for path in sorted(output.iterdir()) if path.is_file()
             and path.name not in excluded and not path.name.endswith('-evidence.json')
             and not path.name.endswith('.plan.json')}
    plans = []
    asset_paths = {asset['path'] for asset in manifest['assets']}
    reserve = 5 * (1024 * 1024 + 16)
    for name, receipt in list(files.items()):
        if name in asset_paths:
            document = json.loads((output / Path(name).name).read_bytes())
            plan = build_prototype_plan(document, {'document': receipt, 'files': files})
            path = output / (Path(name).stem + '.plan.json')
            path.write_text(json.dumps(plan, separators=(',', ':'), allow_nan=False), encoding='utf-8')
            plan_receipt = file_receipt(path, f'reference/{path.name}')
            files[plan_receipt['file']] = plan_receipt
            plans.append({'asset_file': name, 'input': plan_receipt})
            reserve = max(reserve, checked(2 * (plan['json_bytes'] + plan['buffer_bytes'])),
                          checked(2 * plan_receipt['bytes']))
        # The game reads a primary texture input whole. Only crunch input is decoded (primary_staging).
        if name.endswith(('.rgba', '.bc')):
            reserve = max(reserve, receipt['bytes'])
    from .native_bc import crunch_staging, validate_storage
    tops = set()
    for material in manifest.get('rendering', {}).get('materials', []):
        for texture in material['properties']['textures']:
            validate_storage(texture, files)
            if texture['storage']['kind'] == 'native_bc' and texture['storage']['top']:
                tops.add(texture['storage']['top']['file'])
            reserve = max(reserve, crunch_staging(texture['storage']))
    if reserve >= MAX_STAGING:
        raise ValueError('World input overlap leaves no generation capacity')
    for asset in manifest['assets']:
        asset['dependencies'] = [files[entry['file']] for entry in asset['dependencies']]
    if 'render_lighting' in manifest:
        manifest['render_lighting']['dependencies'] = [files[entry['file']] for entry in manifest['render_lighting']['dependencies']]
    names = start_files(manifest, {p['asset_file']: p['input']['file'] for p in plans})
    shutil.rmtree(output / 'packs', ignore_errors=True)
    # Top mips of start textures load after the game starts, from their own packs.
    packs = build_packs([n for n in names if n not in tops], files, output, 'start')
    top_packs = build_packs([n for n in names if n in tops], files, output, 'top')
    return {'files': [files[name] for name in sorted(files)], 'plans': plans,
            'dependency_staging_reserve': reserve, 'packs': packs, 'top_packs': top_packs}


def start_files(manifest, plans):
    """Files of the interface and of "Load garage assets", in about the order the game reads them."""
    names = [sprite['path'] for sprite in manifest.get('ui', {}).get('sprites', [])]
    for asset in manifest['assets']:
        if asset['usage'] in SCENE_ROOTS:
            names += [plans[asset['path']], asset['path'], *(entry['file'] for entry in asset['dependencies'])]
    names += [chunk['data']['path'] for texture in manifest['lighting']['textures'] for chunk in texture['chunks']]
    names += [entry['file'] for entry in manifest.get('render_lighting', {}).get('dependencies', [])]
    return list(dict.fromkeys(names))


def build_packs(names, files, output, kind):
    """Write gzip packs `packs/{kind}-N.gz` of the files. A pack is the plain concatenation of its entries."""
    root = output / 'packs'
    groups, size = [], PACK_BYTES
    for name in names:
        bytes_ = files[name]['bytes']
        if bytes_ > PACK_BYTES:
            continue
        if size + bytes_ > PACK_BYTES:
            groups.append([])
            size = 0
        groups[-1].append(name)
        size += bytes_
    packs = []
    for index, group in enumerate(groups):
        root.mkdir(exist_ok=True)
        path = root / f'{kind}-{index}.gz'
        content = b''.join((output / name.removeprefix('reference/')).read_bytes() for name in group)
        # mtime 0 keeps the file bytes the same for the same entries.
        path.write_bytes(gzip.compress(content, compresslevel=9, mtime=0))
        receipt = file_receipt(path, f'packs/{path.name}')
        packs.append({'file': receipt['file'], 'bytes': receipt['bytes'], 'sha256': receipt['sha256'], 'files': group})
    return packs


def package_dir(catalog):
    """The package directory name in `content_base`."""
    return catalog['content_base'].removeprefix('assets/runtime/').removesuffix('/')


def package_id(manifest):
    core = {key: value for key, value in manifest.items() if key != 'content_base'}
    return hashlib.sha256(json.dumps(core, sort_keys=True, separators=(',', ':'), allow_nan=False).encode()).hexdigest()


def prepare_package(manifest, output):
    from .lighting import export_manifest_texture_batches
    export_manifest_texture_batches(manifest, output)
    manifest['file_catalog'] = build_file_catalog(manifest, output)
    from .runtime_encoding import encode_runtime
    encode_runtime(manifest)
    manifest['content_base'] = f"assets/runtime/{package_id(manifest)}/"


def publish_package(output, assets_root):
    """Publish a complete package before replacing the mutable root manifest."""
    catalog = read_catalog(output)
    destination = assets_root / 'runtime' / package_dir(catalog)
    destination.parent.mkdir(exist_ok=True)
    if destination.exists():
        verify_package(catalog, destination)
    else:
        _write_package(catalog, output, destination)
    prune_packages(destination)


def prune_packages(current):
    """Keep the current package and the newest older one, which a running page may still read."""
    older = sorted((path for path in current.parent.iterdir()
                    if path.is_dir() and path != current and re.fullmatch(r'[0-9a-f]{64}', path.name)),
                   key=lambda path: path.stat().st_mtime, reverse=True)
    for path in older[1:]:
        shutil.rmtree(path)


def _write_package(manifest, output, destination):
    import tempfile
    pending = Path(tempfile.mkdtemp(prefix='.package-', dir=destination.parent))
    try:
        for receipt in manifest['file_catalog']['files']:
            name = receipt['file']
            source = output / name.removeprefix('reference/') if name.startswith('reference/') else output / name
            target = pending / name
            target.parent.mkdir(parents=True, exist_ok=True)
            # Staging and the package share one volume. Exports never change a written file.
            target.hardlink_to(source)
        for pack in manifest['file_catalog']['packs'] + manifest['file_catalog']['top_packs']:
            target = pending / pack['file']
            target.parent.mkdir(parents=True, exist_ok=True)
            target.hardlink_to(output / pack['file'])
        verify_package(manifest, pending)
        pending.rename(destination)
    finally:
        if pending.exists():
            shutil.rmtree(pending)


def verify_package(manifest, root):
    """Check the package files against the `file_catalog` of `manifest` (the manifest or its catalog)."""
    packs = manifest['file_catalog']['packs'] + manifest['file_catalog']['top_packs']
    expected = {receipt['file'] for receipt in manifest['file_catalog']['files']} | {pack['file'] for pack in packs}
    if {path.relative_to(root).as_posix() for path in root.rglob('*') if path.is_file()} != expected:
        raise ValueError('Runtime package file set differs')
    for receipt in manifest['file_catalog']['files']:
        if file_receipt(root / receipt['file'], receipt['file']) != receipt:
            raise ValueError(f"Runtime package receipt differs: {receipt['file']}")
    for pack in packs:
        receipt = file_receipt(root / pack['file'], pack['file'])
        if (receipt['bytes'], receipt['sha256']) != (pack['bytes'], pack['sha256']):
            raise ValueError(f"Runtime package receipt differs: {pack['file']}")
