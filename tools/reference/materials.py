"""Decode source normal channels before writing glTF textures."""
import math

from PIL import Image, ImageMath

from .source import source_id


def decode_normal_image(image: Image.Image, encoding: str) -> Image.Image:
    if encoding not in {'ag', 'rg', 'rag', 'rgb'}:
        raise ValueError(f'Unknown normal encoding: {encoding}')
    red, green, blue, alpha = image.convert('RGBA').split()
    if encoding == 'rgb':
        return Image.merge('RGB', (red, green, blue))
    red = alpha if encoding == 'ag' else red
    if encoding == 'rag':
        red = ImageMath.lambda_eval(lambda p: p['r'] * p['a'] / 255, r=red.convert('F'), a=alpha.convert('F'))
    x = red.point(lambda value: value * 2 / 255 - 1, mode='F')
    y = green.point(lambda value: value * 2 / 255 - 1, mode='F')
    z = ImageMath.lambda_eval(lambda p: p['max'](1 - p['x']**2 - p['y']**2, 0)**0.5, x=x, y=y)
    blue = z.point(lambda value: (value + 1) * 127.5 + 0.5).convert('L')
    if encoding == 'rag': red = red.point(lambda value: value + .5).convert('L')
    return Image.merge('RGB', (red, green, blue))


def normal_encoding(texture, shader, shader_name: str, identity: dict) -> str:
    # Unity 2021.3.18f1 uses UnpackNormal/UnpackScaleNormal on D3D11.
    # DXT5nm stores X in alpha; BC5 stores X in red. Both store Y in green.
    platforms = shader['platforms'] if shader else []
    if shader_name in {'Standard', 'FX/Glass/Stained BumpDistort', 'Custom/playermodelshader'} and set(platforms) == {4}:
        from UnityPy.enums import TextureFormat
        formats = {TextureFormat.DXT5: 'ag', TextureFormat.DXT5Crunched: 'ag', TextureFormat.BC5: 'rg'}
        if texture['m_TextureFormat'] in formats:
            return 'rag' if shader_name == 'Custom/playermodelshader' else formats[texture['m_TextureFormat']]
    raise ValueError(f'Unresolved normal encoding: {identity} _BumpMap; '
                     f'shader={shader_name}, platforms={platforms}, format={texture['m_TextureFormat']}')

# Defaults come from the installed shader property table and Unity 2021.3.18f1 Standard.shader.
STANDARD_FLOATS = {'_Cutoff': .5, '_Glossiness': .5, '_GlossMapScale': 1.,
    '_SmoothnessTextureChannel': 0., '_Metallic': 0., '_SpecularHighlights': 1.,
    '_GlossyReflections': 1., '_BumpScale': 1., '_Parallax': .02,
    '_OcclusionStrength': 1., '_DetailNormalMapScale': 1., '_UVSec': 0.,
    '_Mode': 0., '_SrcBlend': 1., '_DstBlend': 0., '_ZWrite': 1.}
TEXTURE_DEFAULTS = {'_MainTex': 'white', '_BumpMap': 'bump',
    '_DetailAlbedoMap': 'grey', '_DetailMask': 'white', '_DetailNormalMap': 'bump',
    '_MetallicGlossMap': 'white', '_OcclusionMap': 'white', '_EmissionMap': 'white',
    '_ParallaxMap': 'black'}
NORMAL_SLOTS = {'_BumpMap', '_DetailNormalMap'}
FAMILIES = {'Standard': 'standard', 'Unlit/Color': 'unlit_color',
            'FX/Glass/Stained BumpDistort': 'stained_glass', 'AmplifyRope': 'amplify_rope',
            'Custom/playermodelshader': 'player_model'}
KEYWORDS = {'_NORMALMAP', '_DETAIL_MULX2', '_METALLICGLOSSMAP', '_EMISSION',
    '_PARALLAXMAP', '_SMOOTHNESS_TEXTURE_ALBEDO_CHANNEL_A', '_SPECULARHIGHLIGHTS_OFF',
    '_GLOSSYREFLECTIONS_OFF', '_ALPHATEST_ON', '_ALPHABLEND_ON', '_ALPHAPREMULTIPLY_ON'}


PLAYER_TEXTURES = {54:(155,None),55:(169,None),57:(346,168),58:(169,None),59:(284,168),
    60:(346,168),61:(219,168),62:(169,None),63:(357,None),64:(355,None),65:(175,168),
    66:(233,168),67:(169,None),68:(290,168),69:(270,168),70:(362,236),71:(281,350),
    72:(281,350),73:(329,236),74:(212,225),75:(212,225)}
PLAYER_FLOATS = {'_BumpStrength': 5., '_Glossiness': 1., '_Metallic': 1., '_Dist': 10., '_Cutoff': 1.}
PLAYER_PROPERTIES = {'_Color': 0, '_MainTex': 4, '_BumpMap': 4,
    **{name: 3 for name in PLAYER_FLOATS}}


def validate_player_source(reader, shader_reader, data, shader):
    identity = source_id(reader)
    if identity['file'] != 'sharedassets0.assets' or identity['path_id'] not in PLAYER_TEXTURES:
        raise ValueError(f'{identity}: unsupported player material')
    if source_id(shader_reader) != {'file': 'sharedassets0.assets', 'path_id': 719}:
        raise ValueError(f'{identity}: player shader source')
    parsed = shader.get('m_ParsedForm', {})
    properties = parsed.get('m_PropInfo', {}).get('m_Props', [])
    if {p['m_Name']: p['m_Type'] for p in properties} != PLAYER_PROPERTIES or len(properties) != 8:
        raise ValueError(f'{identity}: unsupported player shader properties')
    saved = data['m_SavedProperties']
    floats = dict(saved['m_Floats'])
    expected = (set() if identity['path_id'] in (63,64) else set(STANDARD_FLOATS)) | set(PLAYER_FLOATS) | ({'_Red'} if identity['path_id'] in (61,69) else set())
    if set(floats) - expected:
        raise ValueError(f'{identity}: unsupported player properties')
    for field in expected:
        if field not in floats:
            raise ValueError(f'{identity}: missing {field}')
    for field, maximum in PLAYER_FLOATS.items():
        if not 0 <= floats[field] <= maximum:
            raise ValueError(f'{identity}: {field} outside player range')
    colors = dict(saved['m_Colors'])
    expected_colors = {'_Color'} if identity['path_id'] in (63,64) else {'_Color','_EmissionColor'}
    for field in expected_colors:
        if field not in colors:
            raise ValueError(f'{identity}: missing {field}')
    if set(colors) != expected_colors:
        raise ValueError(f'{identity}: unsupported player colors')
    if data['m_EnableInstancingVariants'] or data['m_CustomRenderQueue'] != -1:
        raise ValueError(f'{identity}: unsupported player state')
    if data['m_ValidKeywords'] or data['disabledShaderPasses']:
        raise ValueError(f'{identity}: unsupported player keyword or disabled pass')
    subshaders = parsed.get('m_SubShaders', [])
    tags = [('IGNOREPROJECTOR', 'true'), ('QUEUE', 'AlphaTest'), ('RenderType', 'TransparentCutout')]
    if len(subshaders) != 1 or subshaders[0].get('m_LOD') != 200 or subshaders[0].get('m_Tags') != {'tags': tags}:
        raise ValueError(f'{identity}: unsupported player subshader')
    passes = subshaders[0].get('m_Passes', [])
    expected_passes = [('FORWARDBASE', 'FORWARD', 2, 1, 0), ('FORWARDADD', 'FORWARD', 2, 0, 1),
        ('DEFERRED', 'DEFERRED', 2, 1, 0), ('SHADOWCASTER', 'ShadowCaster', 2, 1, 0), ('META', 'Meta', 0, 1, 0)]
    if len(passes) != len(expected_passes):
        raise ValueError(f'{identity}: unsupported player pass count')
    for render_pass, (mode, name, cull, z_write, dst_blend) in zip(passes, expected_passes):
        state = render_pass.get('m_State', {})
        pass_tags = [tags[0], ('LIGHTMODE', mode), *tags[1:]]
        if mode in ('FORWARDBASE', 'FORWARDADD', 'SHADOWCASTER'):
            pass_tags.append(('SHADOWSUPPORT', 'true'))
        if (render_pass.get('m_Type') != 0 or render_pass.get('m_Name') != ''
                or render_pass.get('m_Tags') != {'tags': []} or state.get('m_Name') != name
                or state.get('m_LOD') != 200 or state.get('m_Tags') != {'tags': pass_tags}):
            raise ValueError(f'{identity}: unsupported player {mode} pass identity')
        for owner, values in ((state, {'culling': cull, 'zWrite': z_write, 'zTest': 4}),
                (state.get('rtBlend0', {}), {'srcBlend': 1, 'destBlend': dst_blend, 'colMask': 15})):
            for field, expected_value in values.items():
                if owner.get(field) != {'name': '<noninit>', 'val': expected_value}:
                    raise ValueError(f'{identity}: unsupported player {mode} {field} state')



def required_pointer(source, owner, pointer, property_name):
    try:
        reader = source.resolve(owner, pointer)
    except Exception as error:
        raise ValueError(f'{source_id(owner)} {property_name}: unresolved source pointer') from error
    if pointer['m_PathID'] and reader is None:
        raise ValueError(f'{source_id(owner)} {property_name}: unresolved source pointer')
    return reader


def shader_defaults(shader, family):
    floats = dict(STANDARD_FLOATS) if family == 'standard' else {}
    colors = {} if family == 'amplify_rope' else {'_Color': [1., 1., 1., 1.]}
    textures = dict(TEXTURE_DEFAULTS)
    parsed = shader.get('m_ParsedForm')
    if parsed:
        for prop in parsed['m_PropInfo']['m_Props']:
            values = [prop[f'm_DefValue[{i}]'] for i in range(4)]
            if prop['m_Type'] in (2, 3): floats[prop['m_Name']] = values[0]
            elif prop['m_Type'] in (0, 1): colors[prop['m_Name']] = values
            elif prop['m_Type'] == 4: textures[prop['m_Name']] = prop['m_DefTexture']['m_DefaultName']
    if family == 'standard': colors.setdefault('_EmissionColor', [0., 0., 0., 0.])
    return floats, colors, textures


def recover_material(source, reader, color_override=None, condition=None, texture_overrides=None):
    data = source.read(reader)
    shader_reader = required_pointer(source, reader, data['m_Shader'], 'm_Shader')
    if shader_reader is None: raise ValueError(f'{source_id(reader)} m_Shader: missing shader')
    shader = source.read(shader_reader)
    shader_name = shader['m_Name'] or shader['m_ParsedForm']['m_Name']
    family = FAMILIES.get(shader_name)
    if family is None: raise ValueError(f'{source_id(reader)} required shader: {shader_name}')
    if source_id(reader)['file'] == 'sharedassets0.assets' and reader.path_id in PLAYER_TEXTURES and family != 'player_model':
        raise ValueError(f'{source_id(reader)}: player shader family')
    if family == 'player_model': validate_player_source(reader, shader_reader, data, shader)
    keywords = sorted(data.get('m_ValidKeywords', data.get('m_ShaderKeywords', '').split()))
    if family == 'standard' and set(keywords) - KEYWORDS:
        raise ValueError(f'{source_id(reader)} {shader_name}: unsupported keywords {set(keywords) - KEYWORDS}')
    floats, colors, defaults = shader_defaults(shader, family)
    saved = data['m_SavedProperties']
    floats.update(dict(saved.get('m_Floats', [])))
    colors.update({name: [value[c] for c in ('r', 'g', 'b', 'a')]
                   for name, value in saved.get('m_Colors', [])})
    if not all(math.isfinite(v) for v in floats.values()) or not all(
            math.isfinite(v) for color in colors.values() for v in color):
        raise ValueError(f'{source_id(reader)} nonfinite shader property')
    color = [color_override[c] for c in ('r', 'g', 'b', 'a')] if color_override is not None else colors.get('_Color', [1.,1.,1.,1.])
    if not all(math.isfinite(value) for value in color):
        raise ValueError(f'{source_id(reader)} _Color: nonfinite override')
    active = []
    if family == 'standard':
        active = ['_MainTex', '_OcclusionMap']
        if '_NORMALMAP' in keywords: active.append('_BumpMap')
        if '_DETAIL_MULX2' in keywords:
            active += ['_DetailMask', '_DetailAlbedoMap']
            if '_NORMALMAP' in keywords: active.append('_DetailNormalMap')
        for keyword, slot in [('_METALLICGLOSSMAP', '_MetallicGlossMap'),
                              ('_EMISSION', '_EmissionMap'), ('_PARALLAXMAP', '_ParallaxMap')]:
            if keyword in keywords: active.append(slot)
    elif family in {'stained_glass', 'player_model'}: active = ['_MainTex', '_BumpMap']
    environments = dict(saved.get('m_TexEnvs', []))
    identity_uv = {'m_Scale': {'x': 1., 'y': 1.}, 'm_Offset': {'x': 0., 'y': 0.}}
    textures = []; active_defaults = {}
    for slot in active:
        env = environments.get(slot)
        pointer = env['m_Texture'] if env else {'m_PathID': 0}
        texture_reader = (texture_overrides or {}).get(slot) or required_pointer(source, reader, pointer, slot)
        if texture_reader is None:
            if family == 'player_model' and not (slot == '_BumpMap' and PLAYER_TEXTURES[reader.path_id][1] is None and env is not None and defaults[slot] == 'bump'):
                raise ValueError(f'{source_id(reader)} {slot}: required source texture')
            active_defaults[slot] = defaults[slot]; continue
        if texture_reader.type.name == 'RenderTexture':
            raise ValueError(f'{source_id(reader)} {slot}: unsupported RenderTexture {source_id(texture_reader)}')
        if texture_reader.type.name != 'Texture2D':
            raise ValueError(f'{source_id(reader)} {slot}: required Texture2D')
        texture = source.read(texture_reader); settings = texture['m_TextureSettings']
        encoding = normal_encoding(texture, shader, shader_name, source_id(texture_reader)) if slot in NORMAL_SLOTS else None
        # Native texture upload uses stored TextureColorSpace: Linear=0, SRGB=1.
        stored_space = texture['m_ColorSpace']
        if stored_space not in (0, 1):
            raise ValueError(f'{source_id(texture_reader)} unsupported stored color space')
        color_space = 'srgb' if stored_space == 1 else 'linear'
        if encoding and color_space != 'linear':
            raise ValueError(f'{source_id(texture_reader)} unresolved normal color space')
        uv_env = env if family in {'stained_glass', 'player_model'} else environments.get(
            '_DetailAlbedoMap' if slot in {'_DetailAlbedoMap', '_DetailNormalMap'} else '_MainTex', identity_uv)
        uv_channel = int(floats['_UVSec'] != 0) if family == 'standard' and slot in {
            '_DetailAlbedoMap', '_DetailNormalMap'} else 0
        scale = [uv_env['m_Scale'][c] for c in ('x', 'y')]
        # UnityPy flips source images; export reverses that flip. UV coordinates stay authored.
        offset = [uv_env['m_Offset'][c] for c in ('x', 'y')]
        if not all(math.isfinite(v) for v in scale + offset):
            raise ValueError(f'{source_id(reader)} {slot}: nonfinite UV transform')
        if settings['m_FilterMode'] not in (0, 1, 2) or settings['m_WrapU'] not in (0, 1, 2, 3) or settings['m_WrapV'] not in (0, 1, 2, 3):
            raise ValueError(f'{source_id(reader)} {slot}: unsupported sampler')
        suffix = f'-normal-{encoding}' if encoding else ''
        name = f'texture-{texture_reader.assets_file.name}-{texture_reader.path_id}-{color_space}{suffix}.png'
        width, height = texture['m_Width'], texture['m_Height']
        if family == 'player_model':
            main, bump = PLAYER_TEXTURES[reader.path_id]
            expected = {'file': 'sharedassets0.assets', 'path_id': bump if slot == '_BumpMap' else main}
            if source_id(texture_reader) != expected or color_space != ('linear' if encoding else 'srgb'):
                raise ValueError(f'{source_id(reader)} {slot}: player texture source or color space')
            if (not 0 <= settings['m_Aniso'] <= 16 or not math.isfinite(settings['m_MipBias'])
                    or min(width, height) <= 0 or not 0 < texture['m_MipCount'] <= max(width, height).bit_length()):
                raise ValueError(f'{source_id(reader)} {slot}: player texture sampler or mips')
        mips = [{'file': f'reference/{name}' if level == 0 else f'reference/{name[:-4]}-mip-{level}.png',
                 'width': max(1, width >> level), 'height': max(1, height >> level)}
                for level in range(texture['m_MipCount'])]
        textures.append({'slot': slot, 'source': source_id(texture_reader), 'file': f'reference/{name}',
            'color_space': color_space, 'encoding': 'rgb' if encoding else 'rgba',
            'source_encoding': encoding or 'rgba', 'uv_channel': uv_channel, 'scale': scale, 'offset': offset,
            'sampler': {'filter': settings['m_FilterMode'], 'wrap_u': settings['m_WrapU'],
                'wrap_v': settings['m_WrapV'], 'aniso': settings['m_Aniso'], 'mip_bias': settings['m_MipBias'],
                'mip_count': texture['m_MipCount']}, 'mips': mips})
    alpha = 'opaque'
    if family == 'standard':
        alpha = ('premultiply' if '_ALPHAPREMULTIPLY_ON' in keywords else
                 'blend' if '_ALPHABLEND_ON' in keywords else 'mask' if '_ALPHATEST_ON' in keywords else 'opaque')
    elif family == 'stained_glass': alpha = 'refract'
    elif family == 'player_model': alpha = 'mask'
    state = {'alpha_mode': alpha, 'cull': 2, 'z_write': bool(floats.get('_ZWrite', 1)),
        'z_test': 4, 'src_blend': int(floats.get('_SrcBlend', 1)),
        'dst_blend': int(floats.get('_DstBlend', 0)), 'render_queue': data.get('m_CustomRenderQueue', -1)}
    parsed = shader.get('m_ParsedForm')
    if parsed:
        render_pass = next(p for p in parsed['m_SubShaders'][0]['m_Passes'] if p['m_Type'] == 0)
        def value(field): return floats.get(field['name'], field['val'])
        pass_state = render_pass['m_State']
        pass_values = dict(cull=value(pass_state['culling']), z_write=value(pass_state['zWrite']),
            z_test=value(pass_state['zTest']), src_blend=value(pass_state['rtBlend0']['srcBlend']),
            dst_blend=value(pass_state['rtBlend0']['destBlend']))
        state.update({key: bool(value) if key == 'z_write' else int(value) for key, value in pass_values.items()})
    if family == 'player_model' and condition is not None:
        raise ValueError(f'{source_id(reader)}: unsupported player condition')
    from .native_bc import storage
    for entry in textures:
        tr = source.get(entry['source']['file'], entry['source']['path_id'])
        entry['storage'] = storage(source, tr, entry)
        entry.pop('file'); entry.pop('mips')
    return {'source': source_id(reader), 'shader_family': family, 'color': color,
        'enable_instancing': bool(data['m_EnableInstancingVariants']),
        'condition': condition, 'keywords': keywords, 'state': state,
        'properties': {'floats': floats, 'colors': colors, 'textures': textures,
                       'defaults': active_defaults}}


def source_mip_images(texture):
    """Decode the authored levels. Never resize level zero to invent source mips."""
    from UnityPy.enums import TextureFormat as TF
    from UnityPy.export.Texture2DConverter import parse_image_data
    import texture2ddecoder
    if texture.m_MipCount == 1:
        yield texture.image; return
    fmt = TF(texture.m_TextureFormat); raw = texture.get_image_data(); offset = 0
    plain = {TF.DXT1Crunched: TF.DXT1, TF.DXT5Crunched: TF.DXT5}.get(fmt, fmt)
    block = {TF.DXT1: 8, TF.DXT5: 16, TF.BC5: 16, TF.BC7: 16}
    texel = {TF.RGBA32: 4, TF.RGB24: 3, TF.Alpha8: 1}
    if plain not in block and plain not in texel:
        raise ValueError(f'{source_id(texture.object_reader)} required source mips: {fmt.name}')
    for level in range(texture.m_MipCount):
        width, height = max(1, texture.m_Width >> level), max(1, texture.m_Height >> level)
        size = max(1, (width + 3) // 4) * max(1, (height + 3) // 4) * block[plain] if plain in block else width * height * texel[plain]
        pixels = texture2ddecoder.unpack_unity_crunch(raw, level) if fmt != plain else raw[offset:offset + size]
        if len(pixels) != size: raise ValueError(f'{source_id(texture.object_reader)} source mip {level}: invalid bytes')
        yield parse_image_data(pixels, width, height, plain, texture.object_reader.version,
                               texture.object_reader.platform)
        offset += size


def export_material_textures(source, owner, record, output):
    environments = dict(source.read(owner)['m_SavedProperties'].get('m_TexEnvs', []))
    dependencies = {}
    cache = getattr(source, 'material_texture_cache', None)
    if cache is None: source.material_texture_cache = cache = set()
    for entry in record['properties']['textures']:
        reader = source.get(entry['source']['file'], entry['source']['path_id'])
        from .native_bc import source_blocks, texture_inputs
        paths = texture_inputs(entry)
        identity = str((output / paths[0].removeprefix('reference/')).resolve())
        if identity not in cache:
            texture = source.texture_object(reader)
            if entry['storage']['kind'] == 'native_bc':
                (output / paths[0].removeprefix('reference/')).write_bytes(b''.join(source_blocks(texture)))
            else:
                for mip, image in zip(entry['storage']['mips'], source_mip_images(texture), strict=True):
                    pixels = decode_normal_image(image, entry['source_encoding']) if entry['slot'] in NORMAL_SLOTS else image.convert('RGBA')
                    pixels.transpose(Image.Transpose.FLIP_TOP_BOTTOM).save(output / mip['file'].removeprefix('reference/'))
            cache.add(identity)
        from .runtime_package import file_receipt
        from .native_bc import validate_storage
        for name in paths:
            path = output / name.removeprefix('reference/')
            dependencies[name] = file_receipt(path, name)
        validate_storage(entry, dependencies)
    return list(dependencies.values())


def material_source_evidence(source, reader, record):
    import hashlib
    data = source.read(reader)
    shader_reader = required_pointer(source, reader, data['m_Shader'], 'm_Shader')
    shader = source.read(shader_reader)
    if not hasattr(source, 'material_evidence'): source.material_evidence = {}
    identity = f'{reader.assets_file.name}:{reader.path_id}'
    if identity in source.material_evidence: return
    channels = {'_MainTex': 'RGB albedo; A alpha or smoothness by keyword',
        '_BumpMap': 'source R*A,G unpack; exported RGB; scale XY then reconstruct Z',
        '_DetailMask': 'A mask', '_DetailAlbedoMap': 'RGB multiply x unity_ColorSpaceDouble',
        '_DetailNormalMap': 'source R*A,G unpack; whiteout BlendNormals; mask lerp',
        '_MetallicGlossMap': 'R metallic; A smoothness unless albedo-alpha keyword; GlossMapScale',
        '_OcclusionMap': 'G occlusion; lerp(1,G,OcclusionStrength)',
        '_EmissionMap': 'RGB times EmissionColor only with EMISSION',
        '_ParallaxMap': 'G height; ParallaxOffset1Step offsets main and detail UV'}
    if record['shader_family'] == 'stained_glass':
        channels = {
            '_MainTex': 'RGBA times grab sample and Color; independent MainTex ST',
            '_BumpMap': 'source R*A,G unpack; XY times BumpAmt and GrabTexelSize; depth scale; project by clip W',
        }
    if record['shader_family'] == 'player_model':
        channels = {
            '_MainTex': 'RGB times _Color.rgb; A times _Color.a; independent _MainTex_ST',
            '_BumpMap': 'R*A,G unpack XY; reconstruct Z; lerp((0.5,0.5,1),normal,_BumpStrength); TBN then normalize; independent _BumpMap_ST',
        }
    source.material_evidence[identity] = {
        'source': source_id(reader), 'sha256': hashlib.sha256(source.raw_data(reader)).hexdigest(),
        'shader': source_id(shader_reader), 'shader_sha256': hashlib.sha256(source.raw_data(shader_reader)).hexdigest(),
        'shader_name': shader['m_Name'] or shader['m_ParsedForm']['m_Name'], 'platforms': shader['platforms'],
        'official_unity_version': '2021.3.18f1',
        'official_shader_archive_sha256': '8fd9157efb775fc395e0e26e48c46b2195a3c66d234d20922caf06769b74794b',
        'keywords': record['keywords'], 'disabled_passes': data['disabledShaderPasses'],
        'passes': [{'subshader': index, 'active': index == 0, 'type': p['m_Type'],
                    'state': p['m_State'], 'tags': p['m_Tags']}
                   for index, sub in enumerate(shader['m_ParsedForm']['m_SubShaders']) for p in sub['m_Passes']],
        'channel_samples': [] if record['shader_family'] == 'player_model' else SOURCE_CHANNEL_SAMPLES,
        'state': record['state'], 'channels': {slot: channels[slot] for slot in
            [*record['properties']['defaults'], *[t['slot'] for t in record['properties']['textures']]]},
        'textures': record['properties']['textures'], 'defaults': record['properties']['defaults'],
        'transport_limits': (['Imported glTF retains the existing white PBR rope material.',
            'Source rope sag and sway are not implemented.'] if record['shader_family'] == 'amplify_rope' else
            ['Base glTF does not apply layered shader operations.',
             'Final binding must upload authored mips, source anisotropy and mip bias.']),
    }
    if record['shader_family'] == 'player_model':
        evidence = source.material_evidence[identity]
        evidence['required_operations'] = [
            {'operation': 'camera_distance_clip', 'discard': 'length(world_position - _WorldSpaceCameraPos) < _Dist',
             'passes': ['FORWARDBASE', 'FORWARDADD', 'DEFERRED', 'SHADOWCASTER', 'META']},
            {'operation': 'alpha_clip', 'discard': '_Cutoff >= sample(_MainTex).a * _Color.a',
             'after': 'camera_distance_clip'},
            {'operation': 'normal', 'equation': 'q.xy=2*(R*A,G)-1; q.z=sqrt(1-min(dot(q.xy,q.xy),1)); normalize(TBN*lerp((0.5,0.5,1),q,_BumpStrength))'},
            {'operation': 'surface', 'albedo': 'sample(_MainTex).rgb * _Color.rgb',
             'metallic': '_Metallic', 'smoothness': '_Glossiness', 'occlusion': 1, 'emission': [0,0,0]},
            {'operation': 'camera_distance_override', 'source': {'file':'level1','path_id':70544},
             'before_render': 0.3499999940395355, 'after_render': 0,
             'methods': ['playermodeloutfitscript.SetDistance', 'playermodeloutfitscript.ReSetDistance',
                         'setsubshaderplayercamscript.OnPreRender', 'setsubshaderplayercamscript.OnPostRender']},
        ]
        from .library.reader import checked
        evidence['compiled_data'] = shader['compressedBlob']['$byte_array']
        checked(source.library.records_root, evidence['compiled_data'])
        evidence['programs'] = dict(zip(['FORWARDBASE','FORWARDADD','DEFERRED','SHADOWCASTER','META'], [
            '3fc44d0503ebfefe950bbbf1421d05dd64635c58ba85250e409c50a94bf93138',
            'c416560da4519a8f1efd5c2a76729c4718f4cbada13081fb481f4ff4c333877a',
            '3a6dc70fae785e22a66269ba660c7960e87975db890f97b08e2bfaedd6108768',
            'a873c329305fc3f24007c8c1708b58a26652825b2e8c5a10b68113510b6d2432',
            'fbc1da7ea386966e3bff8eb51930af5135fe88313aebb3f729b7262a8b9df30e']))
        evidence['authored_null_textures'] = [slot for slot, env in data['m_SavedProperties']['m_TexEnvs'] if slot in ('_MainTex','_BumpMap') and not env['m_Texture']['m_PathID']]
        evidence['unused_saved_properties'] = sorted(set(record['properties']['floats']) - set(PLAYER_FLOATS))
        evidence['transport_limits'] = ['Inactive source contract. Active player material preparation is rejected.',
            'Camera distance clipping, custom normal blending, and matched game views are not implemented or measured.']



# UnityStandardInput.cginc Albedo and MetallicGloss; retain inputs for GPU probes.
SOURCE_CHANNEL_SAMPLES = [
    dict(operation='detail_albedo', color_space='gamma', detail=.2, mask_alpha=.5, albedo_multiplier=.7),
    dict(operation='smoothness', keyword='_SMOOTHNESS_TEXTURE_ALBEDO_CHANNEL_A',
         main_alpha=.75, gloss_map_scale=.4, smoothness=.3),
    dict(operation='detail_albedo', color_space='linear', detail=.2, mask_alpha=.5,
         unity_color_space_double=4.59479380, albedo_multiplier=.95947938),
]
