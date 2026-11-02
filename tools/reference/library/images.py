"""Export source images and fonts without material channel conversions."""
import hashlib
import io
import struct
from dataclasses import asdict
from pathlib import Path

from PIL import Image, ImageFont, DdsImagePlugin
from UnityPy.enums import TextureFormat
from UnityPy.export.Texture2DConverter import get_image_from_texture2d, parse_image_data

from .records import _file, _json
from .types import AssetExport, Catalog
from ..source import Source

FACE_LABELS = ('+X', '-X', '+Y', '-Y', '+Z', '-Z')
FACE_FILES = ('positive-x', 'negative-x', 'positive-y', 'negative-y', 'positive-z', 'negative-z')


def typed_record(identity):
    return f'records/{identity.file}/{identity.path_id}.json'


def _png(output, name, image):
    content = io.BytesIO()
    image.save(content, format='PNG')
    return _file(output, name, content.getvalue())


def _dds(payload, width, height, mips):
    """Wrap unchanged BC6H mip bytes in a DX10 2D DDS container."""
    flags = 0x1 | 0x2 | 0x4 | 0x1000 | 0x80000
    caps = 0x1000
    if mips > 1:
        flags |= 0x20000
        caps |= 0x8 | 0x400000
    top_size = max(1, (width + 3) // 4) * max(1, (height + 3) // 4) * 16
    header = [124, flags, height, width, top_size, 0, mips, *([0] * 11),
              32, int(DdsImagePlugin.DDPF.FOURCC), int(DdsImagePlugin.D3DFMT.DX10),
              0, 0, 0, 0, 0, caps, 0, 0, 0, 0]
    return b'DDS ' + struct.pack('<31I', *header) + struct.pack('<5I',
        int(DdsImagePlugin.DXGI_FORMAT.BC6H_UF16), 3, 0, 1, 0) + payload


def _cubemap(source, reader, data, output, prefix):
    obj = source.object(reader)
    payload = obj.get_image_data()
    width, height = data['m_Width'], data['m_Height']
    mips = data['m_MipCount']
    if data['m_TextureFormat'] != int(TextureFormat.BC6H) or data['m_ImageCount'] != 6:
        raise ValueError('Expected six BC6H cubemap faces')
    mip_sizes = [max(1, (max(1, width >> mip) + 3) // 4) *
                 max(1, (max(1, height >> mip) + 3) // 4) * 16 for mip in range(mips)]
    face_size = sum(mip_sizes)
    if len(payload) != 6 * face_size or data['m_CompleteImageSize'] != face_size:
        raise ValueError('Cubemap mip payload size mismatch')
    files, faces = [], []
    for index, (label, name) in enumerate(zip(FACE_LABELS, FACE_FILES)):
        face = payload[index * face_size:(index + 1) * face_size]
        file = _file(output, f'{prefix}/{name}.dds', _dds(face, width, height, mips))
        files.append(file)
        with Image.open(output / file.file) as image:
            if image.size != (width, height) or image.pixel_format != 'BC6H':
                raise ValueError('DDS face metadata mismatch')
        preview = parse_image_data(face, width, height, TextureFormat.BC6H,
                                   getattr(reader, 'version', (2021, 3, 18, 1)),
                                   getattr(reader, 'platform', 0), flip=True)
        preview_file = _png(output, f'{prefix}/{name}.preview.png', preview)
        files.append(preview_file)
        offset, mip_records = 0, []
        for mip, size in enumerate(mip_sizes):
            mip_records.append({'level': mip, 'offset': offset, 'bytes': size,
                                'sha256': hashlib.sha256(face[offset:offset + size]).hexdigest()})
            offset += size
        faces.append({'label': label, 'index': index, 'file': file.file,
                      'preview': preview_file.file, 'preview_dimensions': list(preview.size),
                      'payload_sha256': hashlib.sha256(face).hexdigest(), 'mips': mip_records})
    return tuple(files), {'faces': faces, 'payload_sha256': hashlib.sha256(payload).hexdigest(),
                          'conversion': {'format': 'DDS_DX10_BC6H_UF16', 'hdr_preserved': True,
                                         'face_order': list(FACE_LABELS), 'dds_orientation': 'source',
                                         'preview_orientation': 'flip_y', 'preview_role': 'clipped_8bit_preview'}}


def _font_names(raw):
    """Read attribution fields from the embedded sfnt name table."""
    count = struct.unpack_from('>H', raw, 4)[0]
    for index in range(count):
        tag, _, offset, length = struct.unpack_from('>4sIII', raw, 12 + index * 16)
        if tag != b'name':
            continue
        table = raw[offset:offset + length]
        _, names, storage = struct.unpack_from('>3H', table)
        result = {}
        for item in range(names):
            platform, encoding, language, name, size, start = struct.unpack_from('>6H', table, 6 + item * 12)
            if name not in (0, 7, 8, 9, 13, 14):
                continue
            text = table[storage + start:storage + start + size].decode(
                'utf-16-be' if platform in (0, 3) else 'mac_roman', errors='replace')
            result.setdefault(str(name), set()).add(text)
        return {key: sorted(values) for key, values in sorted(result.items())}
    return {}


def export_images(source: Source, catalog: Catalog, output: Path) -> tuple[AssetExport, ...]:
    exports = []
    for identity, record in sorted(catalog.objects.items()):
        if record.type not in ('Texture2D', 'Sprite', 'Cubemap', 'Font'):
            continue
        reader = source.get(identity.file, identity.path_id)
        data = source.read_record(reader)
        prefix = f'images/{identity.file}/{identity.path_id}'
        details = {'name': data['m_Name'], 'type': record.type, 'typed_record': typed_record(identity)}
        files, status, runtime, reason = (), 'converted', 'unverified', ''
        if record.type in ('Texture2D', 'Cubemap'):
            details.update({'dimensions': [data['m_Width'], data['m_Height']],
                            'source_settings': {name: value for name, value in data.items()
                                                if name.startswith('m_') and name != 'm_Name'}})
            # Texture source settings contain no image payloads. Source pointers stay typed.
            if record.type == 'Cubemap':
                files, extra = _cubemap(source, reader, data, output, prefix)
                details.update(extra)
            elif not data.get('image data') and not data.get('m_StreamData', {}).get('size', 0) and not all(details['dimensions']):
                status, runtime, reason = 'source_empty', 'engine_dependent', 'Source texture has no stored pixels'
            else:
                obj = source.object(reader)
                if not obj.get_image_data():
                    raise ValueError(f'Nonempty texture has no pixels: {identity}')
                image = get_image_from_texture2d(obj)
                files = (_png(output, f'{prefix}/source.png', image),)
                details['conversion'] = {'role': 'source_pixels', 'format': 'PNG', 'orientation': 'flip_y',
                                         'dimensions': list(image.size), 'mips': 'base_level_only',
                                         'original_mips': 'source_archive'}
        elif record.type == 'Sprite':
            image = source.object(reader).image
            files = (_png(output, f'{prefix}/sprite.png', image),)
            settings = data['m_RD']['settingsRaw']
            details.update({'border': data['m_Border'], 'pivot': data['m_Pivot'],
                            'packed': bool(settings & 1), 'packing_rotation': (settings >> 2) & 15,
                            'geometry_record': typed_record(identity),
                            'atlas_pointer': data.get('m_SpriteAtlas'),
                            'conversion': {'role': 'sprite_crop', 'dimensions': list(image.size),
                                           'orientation': 'UnityPy_unpacked_flip_y'}})
        else:
            raw = bytes(data['m_FontData'])
            if not raw:
                status, runtime, reason = 'native_record', 'engine_dependent', 'Source font has no embedded bytes'
            else:
                if raw[:4] not in (b'\x00\x01\x00\x00', b'OTTO', b'true'):
                    raise ValueError(f'Unsupported embedded font signature: {identity}')
                face = ImageFont.truetype(io.BytesIO(raw), 16).getname()
                extension = 'otf' if raw[:4] == b'OTTO' else 'ttf'
                files = (_file(output, f'fonts/{identity.file}/{identity.path_id}/font.{extension}', raw),)
                details.update({'face': list(face), 'signature': raw[:4].hex(),
                                'attribution': {'source': 'embedded_sfnt_name_table', 'fields': _font_names(raw)},
                                'conversion': {'role': 'embedded_font', 'transcoded': False}})
        exports.append(AssetExport(identity, 'images', status, runtime, reason, files,
                                   catalog.dependencies[identity], details))
    _file(output, 'images/index.json', _json([asdict(item) for item in exports]))
    return tuple(exports)
