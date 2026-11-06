"""Source BC primary storage and checked private-package conversion."""
import hashlib


def mip_sizes(width, height, count, block):
    if min(width, height) <= 0 or not 0 < count <= max(width, height).bit_length() or block not in (8, 16):
        raise ValueError('BC extent or mip count')
    return [((max(1, width >> level) + 3) // 4) * ((max(1, height >> level) + 3) // 4) * block
            for level in range(count)]


NORMAL_SLOTS = {'_BumpMap', '_DetailNormalMap'}


def stored_encoding(entry, family):
    """Encoding of native blocks: color as stored, or a Standard DXT5nm normal in AG."""
    if entry['slot'] not in NORMAL_SLOTS:
        return 'rgba' if entry['source_encoding'] == 'rgba' else None
    # Standard samples x = r * a and y = g, as Unity UnpackNormal does.
    return 'ag' if family == 'standard' and entry['source_encoding'] == 'ag' else None


def eligible(entry, data, family):
    encoding = stored_encoding(entry, family)
    formats = (12, 29) if encoding == 'ag' else (10, 12, 28, 29)
    width, height = data['m_Width'], data['m_Height']
    return (encoding is not None and data['m_TextureFormat'] in formats
            and data['m_ImageCount'] == 1 and data['m_TextureDimension'] == 2
            and data['m_MipsStripped'] == 0 and min(width, height) > 0
            and width % 4 == 0 and height % 4 == 0
            and 0 < data['m_MipCount'] <= max(width, height).bit_length())


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


def split_blocks(texture):
    """The top mip and the mips 1+ of a texture. A one-level texture has no top file."""
    levels = list(source_blocks(texture))
    return (levels[0], b''.join(levels[1:])) if len(levels) > 1 else (None, levels[0])


def top_file(name):
    return name.removesuffix('.bc') + '.top.bc'


def receipt(name, data):
    return {'file': name, 'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest(), 'image_size': None}


def storage(source, reader, entry, family):
    """Select one required representation from checked source metadata.
    Native storage sets the entry encoding to the stored channels."""
    data = source.read(reader)
    if not eligible(entry, data, family):
        return {'kind': 'png_chain', 'mips': entry['mips']}
    settings = data['m_TextureSettings']
    expected_sampler = dict(filter=settings['m_FilterMode'], wrap_u=settings['m_WrapU'], wrap_v=settings['m_WrapV'],
        aniso=settings['m_Aniso'], mip_bias=settings['m_MipBias'], mip_count=data['m_MipCount'])
    expected_mips = [(max(1, data['m_Width'] >> i), max(1, data['m_Height'] >> i)) for i in range(data['m_MipCount'])]
    if (entry['sampler'] != expected_sampler or entry['color_space'] != ('srgb' if data['m_ColorSpace'] == 1 else 'linear')
            or [(m['width'], m['height']) for m in entry['mips']] != expected_mips):
        raise ValueError('Primary metadata differs from source')
    top, rest = split_blocks(source.texture_object(reader))
    entry['encoding'] = stored_encoding(entry, family)
    identity = entry['source']
    name = f"reference/texture-{identity['file']}-{identity['path_id']}-{entry['color_space']}.bc"
    return {'kind': 'native_bc', 'format': 'bc1' if data['m_TextureFormat'] in (10, 28) else 'bc3',
        'source_format': data['m_TextureFormat'], 'width': data['m_Width'], 'height': data['m_Height'],
        'mip_count': data['m_MipCount'], 'image_count': data['m_ImageCount'],
        'dimension': data['m_TextureDimension'], 'mips_stripped': data['m_MipsStripped'],
        # The game loads mips 1+ at start and the top mip later.
        'input': receipt(name, rest), 'top': top and receipt(top_file(name), top)}


def texture_inputs(entry):
    value = entry['storage']
    if value['kind'] == 'png_chain':
        return [m['file'] for m in value['mips']]
    if value['kind'] == 'native_bc':
        return [value['input']['file']] + ([value['top']['file']] if value['top'] else [])
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
              'dimension', 'mips_stripped', 'input', 'top'}
    if set(value) != fields or value['kind'] != 'native_bc':
        raise ValueError('Required primary storage fields')
    normal = entry['encoding'] == entry['source_encoding'] == 'ag' and entry['slot'] in NORMAL_SLOTS
    if not ((entry['encoding'] == entry['source_encoding'] == 'rgba' or normal and value['format'] == 'bc3')
            and (value['format'], value['source_format']) in (('bc1', 10), ('bc1', 28), ('bc3', 12), ('bc3', 29))
            and value['image_count'] == 1 and value['dimension'] == 2 and value['mips_stripped'] == 0
            and value['width'] % 4 == 0 and value['height'] % 4 == 0
            and value['mip_count'] == entry['sampler']['mip_count']
            and 0 < value['mip_count'] <= max(value['width'], value['height']).bit_length()
            and entry['source']['file'] and entry['source']['path_id']):
        raise ValueError('Native BC source gate')
    identity = entry['source']
    receipt = value['input']
    if receipt['file'] != f"reference/texture-{identity['file']}-{identity['path_id']}-{entry['color_space']}.bc":
        raise ValueError('Native BC source identity')
    sizes = mip_sizes(value['width'], value['height'], value['mip_count'], 8 if value['format'] == 'bc1' else 16)
    top = value['top']
    if (top is None) != (len(sizes) == 1) or top and (
            top['file'] != top_file(receipt['file']) or top['image_size'] is not None or top['bytes'] != sizes[0]
            or files[top['file']] != top):
        raise ValueError('Native BC top mip receipt')
    if receipt['image_size'] is not None or receipt['bytes'] != sum(sizes[1:] if top else sizes):
        raise ValueError('Native BC receipt bytes')
    if files[receipt['file']] != receipt:
        raise ValueError('Native BC catalog receipt')
