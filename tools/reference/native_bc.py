"""Source BC primary storage and checked private-package conversion."""
import hashlib
from pathlib import Path


def mip_sizes(width, height, count, block):
    if min(width, height) <= 0 or not 0 < count <= max(width, height).bit_length() or block not in (8, 16):
        raise ValueError('BC extent or mip count')
    return [((max(1, width >> level) + 3) // 4) * ((max(1, height >> level) + 3) // 4) * block
            for level in range(count)]


def eligible(entry, data):
    return (entry['encoding'] == entry['source_encoding'] == 'rgba'
            and data['m_TextureFormat'] in (10, 12, 28, 29)
            and data['m_ImageCount'] == 1 and data['m_TextureDimension'] == 2
            and data['m_MipsStripped'] == 0 and min(data['m_Width'], data['m_Height']) > 0
            and data['m_MipCount'] == max(data['m_Width'], data['m_Height']).bit_length())


def source_blocks(texture):
    """Unpack crunch levels, or slice plain BC. Preserve source block order."""
    import texture2ddecoder
    fmt = texture.m_TextureFormat
    if fmt not in (10, 12, 28, 29):
        raise ValueError('Unsupported native BC source format')
    raw = texture.get_image_data()
    sizes = mip_sizes(texture.m_Width, texture.m_Height, texture.m_MipCount, 8 if fmt in (10, 28) else 16)
    offset = 0
    for level, size in enumerate(sizes):
        blocks = texture2ddecoder.unpack_unity_crunch(raw, level) if fmt in (28, 29) else raw[offset:offset + size]
        if len(blocks) != size:
            raise ValueError(f'Native BC mip {level}: source byte count differs')
        yield blocks
        offset += size
    if fmt in (10, 12) and offset != len(raw):
        raise ValueError('Native BC source has trailing data')


def storage(source, reader, entry):
    """Select one required representation from checked source metadata."""
    data = source.read(reader)
    if not eligible(entry, data):
        return {'kind': 'png_chain', 'mips': entry['mips']}
    settings = data['m_TextureSettings']
    expected_sampler = dict(filter=settings['m_FilterMode'], wrap_u=settings['m_WrapU'], wrap_v=settings['m_WrapV'],
        aniso=settings['m_Aniso'], mip_bias=settings['m_MipBias'], mip_count=data['m_MipCount'])
    expected_mips = [(max(1, data['m_Width'] >> i), max(1, data['m_Height'] >> i)) for i in range(data['m_MipCount'])]
    if (entry['sampler'] != expected_sampler or entry['color_space'] != ('srgb' if data['m_ColorSpace'] == 1 else 'linear')
            or [(m['width'], m['height']) for m in entry['mips']] != expected_mips):
        raise ValueError('Primary metadata differs from source')
    blocks = b''.join(source_blocks(source.texture_object(reader)))
    identity = entry['source']
    name = f"reference/texture-{identity['file']}-{identity['path_id']}-{entry['color_space']}.bc"
    return {'kind': 'native_bc', 'format': 'bc1' if data['m_TextureFormat'] in (10, 28) else 'bc3',
        'source_format': data['m_TextureFormat'], 'width': data['m_Width'], 'height': data['m_Height'],
        'mip_count': data['m_MipCount'], 'image_count': data['m_ImageCount'],
        'dimension': data['m_TextureDimension'], 'mips_stripped': data['m_MipsStripped'],
        'input': {'file': name, 'bytes': len(blocks), 'sha256': hashlib.sha256(blocks).hexdigest(), 'image_size': None}}


def texture_inputs(entry):
    value = entry['storage']
    if value['kind'] == 'png_chain':
        return [m['file'] for m in value['mips']]
    if value['kind'] == 'native_bc':
        return [value['input']['file']]
    raise ValueError('Unknown primary storage')


def validate_storage(entry, files):
    """Check the same native admission contract as the Rust reservation boundary."""
    value = entry['storage']
    if value['kind'] == 'png_chain':
        if set(value) != {'kind', 'mips'} or len(value['mips']) != entry['sampler']['mip_count'] or not value['mips']:
            raise ValueError('PNG chain mip count')
        previous = None
        for mip in value['mips']:
            w, h = mip['width'], mip['height']
            if min(w, h) <= 0 or files[mip['file']]['image_size'] != [w, h]:
                raise ValueError('PNG chain receipt dimensions')
            if previous and (previous == (1, 1) or (w, h) != tuple(max(1, n // 2) for n in previous)):
                raise ValueError('PNG chain mip progression')
            previous = w, h
        return
    fields = {'kind', 'format', 'source_format', 'width', 'height', 'mip_count', 'image_count',
              'dimension', 'mips_stripped', 'input'}
    if set(value) != fields or value['kind'] != 'native_bc':
        raise ValueError('Required primary storage fields')
    if not (entry['encoding'] == entry['source_encoding'] == 'rgba'
            and (value['format'], value['source_format']) in (('bc1', 10), ('bc1', 28), ('bc3', 12), ('bc3', 29))
            and value['image_count'] == 1 and value['dimension'] == 2 and value['mips_stripped'] == 0
            and value['mip_count'] == entry['sampler']['mip_count'] == max(value['width'], value['height']).bit_length()
            and entry['source']['file'] and entry['source']['path_id']):
        raise ValueError('Native BC source gate')
    identity = entry['source']
    receipt = value['input']
    if receipt['file'] != f"reference/texture-{identity['file']}-{identity['path_id']}-{entry['color_space']}.bc":
        raise ValueError('Native BC source identity')
    sizes = mip_sizes(value['width'], value['height'], value['mip_count'], 8 if value['format'] == 'bc1' else 16)
    if receipt['image_size'] is not None or receipt['bytes'] != sum(sizes):
        raise ValueError('Native BC receipt bytes')
    if files[receipt['file']] != receipt:
        raise ValueError('Native BC catalog receipt')


def convert_package(accepted, destination, library_path):
    """Convert an accepted PNG package with checked exporter logic and prove every mip."""
    import copy
    import json
    import shutil
    from PIL import Image
    from UnityPy.export.Texture2DConverter import parse_image_data
    from .library.reader import LibraryReader
    from .library.types import LibraryReceipt
    from .source import LibrarySource
    from .runtime_package import file_receipt, prepare_package, publish_package, verify_package, package_id
    raw = library_path.read_bytes()
    metadata = json.loads(raw)
    library = LibraryReceipt(library_path, hashlib.sha256(raw).hexdigest(), metadata['catalog_sha256'], metadata['source_manifest_sha256'], ())
    source = LibrarySource(LibraryReader(library))
    manifest = json.loads((accepted / 'reference/manifest.json').read_bytes())
    from .export import provenance
    if manifest['library'] != provenance(library):
        raise ValueError('Accepted package source provenance differs')
    verify_package(manifest, accepted / 'runtime' / package_id(manifest))
    output = destination / 'reference'
    if destination.exists():
        raise ValueError('Private package destination already exists')
    output.parent.mkdir(parents=True)
    shutil.copytree(accepted / 'reference', output)
    receipts = {r['file']: r for r in manifest['file_catalog']['files']}
    old = {}
    replacements = {}
    unique = {}
    compared = 0
    old_gpu = new_gpu = fixed_old = fixed_new = 0
    for material in manifest['rendering']['materials']:
        for entry in material['properties']['textures']:
            name = entry['file']
            if name not in unique:
                reader = source.get(entry['source']['file'], entry['source']['path_id'])
                value = storage(source, reader, entry)
                unique[name] = value
                png_bytes = sum(m['width'] * m['height'] * 4 for m in entry['mips'])
                payload = value['input']['bytes'] if value['kind'] == 'native_bc' else png_bytes
                old_gpu += png_bytes
                new_gpu += payload
                if value['kind'] == 'native_bc':
                    blocks = b''.join(source_blocks(source.texture_object(reader)))
                    path = output / Path(value['input']['file']).name
                    path.write_bytes(blocks)
                    if file_receipt(path, value['input']['file']) != value['input']:
                        raise ValueError('Exported native receipt differs')
                    offset = 0
                    for level, mip in enumerate(entry['mips']):
                        png = accepted / 'reference' / Path(mip['file']).name
                        if file_receipt(png, mip['file']) != receipts[mip['file']]:
                            raise ValueError('Accepted PNG receipt differs')
                        size = mip_sizes(mip['width'], mip['height'], 1, 8 if value['format'] == 'bc1' else 16)[0]
                        actual = parse_image_data(blocks[offset:offset + size], mip['width'], mip['height'],
                            10 if value['format'] == 'bc1' else 12, (2021, 3, 18, 1), 0, flip=False).convert('RGBA')
                        with Image.open(png) as image:
                            if actual.tobytes() != image.convert('RGBA').tobytes():
                                raise ValueError(f'Native BC pixel/orientation mismatch: {name} mip {level}')
                        offset += size
                        compared += 1
                    if offset != len(blocks):
                        raise ValueError('Native BC mip coverage differs')
                    old[name] = [m['file'] for m in entry['mips']]
            entry['storage'] = copy.deepcopy(unique[name])
            entry.pop('file'); entry.pop('mips')
            if name in old:
                replacements.setdefault(material['asset'], {})[name] = unique[name]['input']
    for asset in manifest['assets']:
        removed = {path for name in replacements.get(asset['id'], {}) for path in old[name]}
        document = json.loads((output / Path(asset['path']).name).read_bytes())
        # glTF owns its authored image references independently of primary material storage.
        from pathlib import PurePosixPath
        gltf_inputs = {str(PurePosixPath(asset['path']).parent / r['uri'])
            for r in [*document.get('images', []), *document.get('buffers', [])] if 'uri' in r}
        asset['dependencies'] = [r for r in asset['dependencies'] if r['file'] not in removed or r['file'] in gltf_inputs]
        current = {r['file'] for r in asset['dependencies']}
        asset['dependencies'].extend(r for r in replacements.get(asset['id'], {}).values() if r['file'] not in current)
    # Plates deliberately retain their existing PNG representation.
    for entry in manifest['plates']['material']['textures']:
        entry['storage'] = {'kind': 'png_chain', 'mips': entry.pop('mips')}
        entry.pop('file')
    # Keep a PNG only when another declared runtime consumer still uses it.
    def references(value):
        if isinstance(value, str):
            yield value
        elif isinstance(value, list):
            for child in value: yield from references(child)
        elif isinstance(value, dict):
            for child in value.values(): yield from references(child)
    consumers = set(references({k: v for k, v in manifest.items() if k != 'file_catalog'}))
    for paths in old.values():
        for name in paths:
            if name not in consumers:
                (output / Path(name).name).unlink()
    evidence_path = output / 'material-evidence.json'
    evidence = json.loads(evidence_path.read_bytes())
    def update_evidence(value):
        if isinstance(value, dict):
            if 'source_encoding' in value and 'file' in value and value['file'] in unique:
                value['storage'] = copy.deepcopy(unique[value.pop('file')]); value.pop('mips')
            for child in value.values(): update_evidence(child)
        elif isinstance(value, list):
            for child in value: update_evidence(child)
    update_evidence(evidence)
    evidence_path.write_text(json.dumps(evidence, separators=(',', ':'), allow_nan=False), encoding='utf-8')
    prepare_package(manifest, output)
    files = {r['file']: r for r in manifest['file_catalog']['files']}
    for material in manifest['rendering']['materials']:
        asset = next(a for a in manifest['assets'] if a['id'] == material['asset'])
        deps = {r['file']: r for r in asset['dependencies']}
        for entry in material['properties']['textures']:
            validate_storage(entry, files)
            if any(deps[name] != files[name] for name in texture_inputs(entry)):
                raise ValueError('Primary dependency closure differs')
    for entry in manifest['plates']['material']['textures']: validate_storage(entry, files)
    unchanged = sum(r == files.get(name) for name, r in receipts.items() if name in files)
    (output / 'manifest.json').write_text(json.dumps(manifest, separators=(',', ':'), allow_nan=False), encoding='utf-8')
    publish_package(output, destination)
    native = [v for v in unique.values() if v['kind'] == 'native_bc']
    fixed = {}
    for material in manifest['rendering']['materials']:
        if 1 <= material['asset'] <= 7:
            for t in material['properties']['textures']: fixed[texture_inputs(t)[0]] = t
    for t in fixed.values():
        v = t['storage']
        if v['kind'] == 'native_bc':
            fixed_old += sum(max(1,v['width'] >> i)*max(1,v['height'] >> i)*4 for i in range(v['mip_count']))
            fixed_new += v['input']['bytes']
        else:
            n = sum(m['width']*m['height']*4 for m in v['mips']); fixed_old += n; fixed_new += n
    result = dict(package_id=package_id(manifest), native_chains=len(native), compared_mips=compared,
        png_chains=len(unique)-len(native), old_gpu=old_gpu, new_gpu=new_gpu, fixed_old_gpu=fixed_old,
        fixed_new_gpu=fixed_new, max_bc_bytes=max(v['input']['bytes'] for v in native),
        max_staging=max(3*v['input']['bytes']+2*1024*1024+128*v['width'] for v in native),
        unchanged_retained_receipts=unchanged)
    print(json.dumps(result, indent=2))
    return result


def verify_native_package(package, accepted, library_path):
    """Read final exported blobs and prove source bytes, PNG pixels, receipts, and closure."""
    import json
    from PIL import Image
    from UnityPy.export.Texture2DConverter import parse_image_data
    from .library.reader import LibraryReader
    from .library.types import LibraryReceipt
    from .source import LibrarySource
    from .runtime_package import file_receipt, verify_package, package_id
    from .export import provenance
    raw = library_path.read_bytes(); metadata = json.loads(raw)
    library = LibraryReceipt(library_path, hashlib.sha256(raw).hexdigest(), metadata['catalog_sha256'], metadata['source_manifest_sha256'], ())
    source = LibrarySource(LibraryReader(library))
    current = json.loads((package / 'reference/manifest.json').read_bytes())
    baseline = json.loads((accepted / 'reference/manifest.json').read_bytes())
    if current['library'] != baseline['library'] or current['library'] != provenance(library):
        raise ValueError('Native package source provenance differs')
    root = package / 'runtime' / package_id(current)
    verify_package(current, root)
    files = {r['file']: r for r in current['file_catalog']['files']}
    old_files = {r['file']: r for r in baseline['file_catalog']['files']}
    changed = {'rendering', 'plates', 'assets', 'file_catalog', 'content_base'}
    if {k:v for k,v in current.items() if k not in changed} != {k:v for k,v in baseline.items() if k not in changed}:
        raise ValueError('Unrelated manifest contract changed')
    for old_entry, entry in zip(baseline['plates']['material']['textures'], current['plates']['material']['textures'], strict=True):
        expected = dict(old_entry)
        expected['storage'] = {'kind': 'png_chain', 'mips': expected.pop('mips')}; expected.pop('file')
        if entry != expected: raise ValueError('Plate PNG contract changed')
    checked = set(); native = levels = png = gltfs = retained_png = 0
    for old_material, material in zip(baseline['rendering']['materials'], current['rendering']['materials'], strict=True):
        if {k:v for k,v in old_material.items() if k != 'properties'} != {k:v for k,v in material.items() if k != 'properties'}:
            raise ValueError('Primary material source contract changed')
        if {k:v for k,v in old_material['properties'].items() if k != 'textures'} != {k:v for k,v in material['properties'].items() if k != 'textures'}:
            raise ValueError('Primary shader properties changed')
        for old_entry, entry in zip(old_material['properties']['textures'], material['properties']['textures'], strict=True):
            if {k:v for k,v in old_entry.items() if k not in ('file','mips')} != {k:v for k,v in entry.items() if k != 'storage'}:
                raise ValueError('Primary texture source contract changed')
            validate_storage(entry, files)
            if old_entry['file'] in checked: continue
            checked.add(old_entry['file'])
            expected = storage(source, source.get(entry['source']['file'], entry['source']['path_id']), old_entry)
            if entry['storage'] != expected:
                raise ValueError('Native package storage differs from source')
            if expected['kind'] == 'png_chain':
                png += 1
                for name in texture_inputs(entry):
                    if files[name] != old_files[name]: raise ValueError('Retained PNG receipt changed')
                continue
            native += 1
            blocks = (root / expected['input']['file']).read_bytes()
            offset = 0
            for level, mip in enumerate(old_entry['mips']):
                size = mip_sizes(mip['width'],mip['height'],1,8 if expected['format']=='bc1' else 16)[0]
                actual = parse_image_data(blocks[offset:offset+size],mip['width'],mip['height'],
                    10 if expected['format']=='bc1' else 12,(2021,3,18,1),0,flip=False).convert('RGBA')
                path = accepted / 'reference' / Path(mip['file']).name
                if file_receipt(path,mip['file']) != old_files[mip['file']]: raise ValueError('Accepted PNG receipt differs')
                with Image.open(path) as image:
                    if actual.tobytes() != image.convert('RGBA').tobytes(): raise ValueError('Exported native pixel/orientation mismatch')
                offset += size; levels += 1
            if offset != len(blocks): raise ValueError('Exported native byte coverage differs')
    for asset in current['assets']:
        document = json.loads((root / asset['path']).read_bytes())
        deps = {r['file']:r for r in asset['dependencies']}
        from pathlib import PurePosixPath
        for image in document.get('images', []):
            name = str(PurePosixPath(asset['path']).parent / image['uri'])
            if deps[name] != files[name]: raise ValueError('glTF image dependency removed')
            retained_png += name.endswith('.png')
        for material in current['rendering']['materials']:
            if material['asset'] == asset['id']:
                for entry in material['properties']['textures']:
                    if any(deps[name] != files[name] for name in texture_inputs(entry)):
                        raise ValueError('Primary storage dependency closure differs')
        gltfs += 1
    for name, receipt in files.items():
        if name in old_files and old_files[name] != receipt:
            raise ValueError(f'Unrelated retained receipt changed: {name}')
    result = dict(package_id=package_id(current), native_chains=native, compared_mips=levels,
        png_chains=png, checked_gltf_documents=gltfs, retained_gltf_png_references=retained_png)
    print(json.dumps(result, indent=2))
    return result


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--accepted', required=True, type=Path)
    parser.add_argument('--destination', required=True, type=Path)
    parser.add_argument('--library', required=True, type=Path)
    parser.add_argument('--verify', action='store_true')
    args = parser.parse_args()
    if args.verify:
        verify_native_package(args.destination, args.accepted, args.library)
    else:
        convert_package(args.accepted, args.destination, args.library)
