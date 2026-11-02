"""Receipts and selected scene plans for the immutable runtime package."""
import hashlib
import json
import re
import shutil
import struct
from pathlib import Path, PurePosixPath

MAX_U32 = 2**32 - 1
MAX_STAGING = 32 * 1024 * 1024


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
    excluded = {'manifest.json', 'library-selection.json'}
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
        if receipt['image_size']:
            reserve = max(reserve, checked(3 * receipt['bytes'] + 2 * 1024 * 1024
                                          + 128 * receipt['image_size'][0]))
    from .native_bc import validate_storage
    for material in manifest.get('rendering', {}).get('materials', []):
        for texture in material['properties']['textures']:
            validate_storage(texture, files)
            storage = texture['storage']
            if storage['kind'] == 'native_bc':
                reserve = max(reserve, checked(3 * storage['input']['bytes'] + 2 * 1024 * 1024 + 128 * storage['width']))
    if reserve >= MAX_STAGING:
        raise ValueError('World input overlap leaves no generation capacity')
    for asset in manifest['assets']:
        asset['dependencies'] = [files[entry['file']] for entry in asset['dependencies']]
    if 'render_lighting' in manifest:
        manifest['render_lighting']['dependencies'] = [files[entry['file']] for entry in manifest['render_lighting']['dependencies']]
    return {'files': [files[name] for name in sorted(files)], 'plans': plans,
            'dependency_staging_reserve': reserve}


def package_id(manifest):
    core = {key: value for key, value in manifest.items() if key != 'content_base'}
    return hashlib.sha256(json.dumps(core, sort_keys=True, separators=(',', ':'), allow_nan=False).encode()).hexdigest()


def prepare_package(manifest, output):
    from .lighting import export_manifest_texture_batches
    export_manifest_texture_batches(manifest, output)
    manifest['file_catalog'] = build_file_catalog(manifest, output)
    manifest['content_base'] = f"assets/runtime/{package_id(manifest)}/"


def publish_package(output, assets_root):
    """Publish a complete package before replacing the mutable root manifest."""
    import tempfile
    manifest = json.loads((output / 'manifest.json').read_bytes())
    destination = assets_root / 'runtime' / package_id(manifest)
    destination.parent.mkdir(exist_ok=True)
    if destination.exists():
        verify_package(manifest, destination)
        return
    pending = Path(tempfile.mkdtemp(prefix='.package-', dir=destination.parent))
    try:
        for receipt in manifest['file_catalog']['files']:
            name = receipt['file']
            source = output / name.removeprefix('reference/') if name.startswith('reference/') else output / name
            target = pending / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, target)
        verify_package(manifest, pending)
        pending.rename(destination)
    finally:
        if pending.exists():
            shutil.rmtree(pending)


def verify_package(manifest, root):
    if manifest['content_base'] != f'assets/runtime/{package_id(manifest)}/':
        raise ValueError('Runtime package identity differs')
    expected = {receipt['file'] for receipt in manifest['file_catalog']['files']}
    if {path.relative_to(root).as_posix() for path in root.rglob('*') if path.is_file()} != expected:
        raise ValueError('Runtime package file set differs')
    for receipt in manifest['file_catalog']['files']:
        if file_receipt(root / receipt['file'], receipt['file']) != receipt:
            raise ValueError(f"Runtime package receipt differs: {receipt['file']}")
