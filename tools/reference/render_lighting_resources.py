"""Hashed source resources for sky, lighting, and active effects."""
import hashlib
import math
from pathlib import Path
from PIL import Image
from .source import source_id, vector
from .materials import source_mip_images
from .render_lighting import NATIVE_DLL_SHA256

DITHER_SHA256 = '609ff1d7287e09600b6aff7ce87202b6207cb66b644e7be9f1ea158c4cd0a2c6'


def caster_dither_bytes():
    # GenerateDitherTextures RVA0x372b80. A8 storage; x, then y, then z.
    thresholds = (0,9,3,9,9,4,9,7,2,9,1,9,9,6,9,5)
    pixels = bytearray(256)
    for z in range(8):
        for index, threshold in enumerate(thresholds):
            value = 255 if threshold < z else 0
            pixels[z * 16 + index] = value
            pixels[(15-z) * 16 + index] = value ^ 255
    result = bytes(pixels)
    if hashlib.sha256(result).hexdigest() != DITHER_SHA256:
        raise ValueError('Source caster dither generation differs from audited native bytes')
    return result


def dependency(path, output):
    return {'file': 'reference/' + path.relative_to(output).as_posix(),
            'sha256': hashlib.sha256(path.read_bytes()).hexdigest()}


def export_texture(source, reader, output):
    """Export stored channels and every authored mip. There is no slot color-space rule."""
    if reader.type.name != 'Texture2D':
        raise ValueError(f'{source_id(reader)} expected a source Texture2D')
    texture = source.texture_object(reader)
    settings = texture.m_TextureSettings
    if texture.m_ColorSpace not in (0, 1):
        raise ValueError(f'{source_id(reader)} invalid stored texture color space')
    if settings.m_FilterMode not in (0,1,2) or settings.m_WrapU not in (0,1,2,3) or settings.m_WrapV not in (0,1,2,3):
        raise ValueError(f'{source_id(reader)} unsupported texture sampler')
    if not math.isfinite(settings.m_MipBias) or settings.m_Aniso < 0:
        raise ValueError(f'{source_id(reader)} invalid texture sampler')
    if min(texture.m_Width,texture.m_Height,texture.m_MipCount) < 1:
        raise ValueError(f'{source_id(reader)} missing source texture mip')
    color_space = 'srgb' if texture.m_ColorSpace else 'linear'
    stem = f'texture-{reader.assets_file.name}-{reader.path_id}-{color_space}'
    levels=[]; dependencies=[]
    for level,image in enumerate(source_mip_images(texture)):
        if level >= texture.m_MipCount:
            raise ValueError(f'{source_id(reader)} excess source texture mip')
        extent = (max(1,texture.m_Width>>level),max(1,texture.m_Height>>level))
        if image.size != extent:
            raise ValueError(f'{source_id(reader)} wrong source texture mip extent')
        name = stem + (f'-mip-{level}' if level else '') + '.png'
        path = output/name
        image.convert('RGBA').transpose(Image.Transpose.FLIP_TOP_BOTTOM).save(path)
        levels.append({'file':'reference/'+name,'width':extent[0],'height':extent[1]})
        dependencies.append(dependency(path,output))
    if len(levels) != texture.m_MipCount:
        raise ValueError(f'{source_id(reader)} missing source texture mip')
    return {'source':source_id(reader),'color_space':color_space,
            'sampler':{'filter':settings.m_FilterMode,'wrap_u':settings.m_WrapU,'wrap_v':settings.m_WrapV,
                       'aniso':settings.m_Aniso,'mip_bias':settings.m_MipBias,'mip_count':texture.m_MipCount},
            'mips':levels}, dependencies


def export_caster_dither(source, output):
    engine = source.root/'UnityPlayer.dll'
    with engine.open('rb') as stream:
        engine_hash = hashlib.file_digest(stream,'sha256').hexdigest()
    if engine_hash != NATIVE_DLL_SHA256:
        raise ValueError('Source engine differs from the recovered caster dither generator')
    path=output/'source-caster-dither-r8.bin'
    path.write_bytes(caster_dither_bytes())
    return {'file':'reference/'+path.name,'extent':[4,4,16],
            'source_engine_sha256':engine_hash,'source_format':'a8','target_format':'r8_unorm',
            'filter':'point','wrap':'repeat','mip_count':1}, dependency(path,output)


def export_cloud_mesh(source, reader, output):
    """Use the existing Unity mesh decoder and handedness conversion."""
    from .gltf import GltfExporter, convert_point
    exporter=GltfExporter(source,output/'source-cloud.gltf',source.library,0)
    _,points,groups=exporter.geometry(reader)
    document,directory,index=exporter.accepted_mesh(reader)
    primitives=document.meshes[index].primitives
    uv_accessors={p.attributes.TEXCOORD_0 for p in primitives}
    if len(uv_accessors)!=1 or None in uv_accessors:
        raise ValueError('Source cloud mesh requires one shared primary UV accessor')
    uvs=exporter.rows(document,directory,next(iter(uv_accessors)))
    indices=[index for group in groups for triangle in group for index in triangle]
    if len(points)!=829 or len(indices)!=4596 or len(uvs)!=len(points):
        raise ValueError('Source cloud mesh differs from the recovered Ball geometry')
    gl=exporter.gl
    positions=exporter.accessor(points,'VEC3')
    uv=exporter.accessor(uvs,'VEC2')
    index=exporter.accessor(indices,'SCALAR',5125,34963)
    exporter.document.meshes=[gl.Mesh(primitives=[gl.Primitive(attributes=gl.Attributes(POSITION=positions,TEXCOORD_0=uv),indices=index,mode=4)])]
    exporter.document.nodes=[gl.Node(mesh=0)]
    exporter.document.scenes=[gl.Scene(nodes=[0])];exporter.document.scene=0
    binary=output/'source-cloud.bin';binary.write_bytes(exporter.binary)
    exporter.document.buffers=[gl.Buffer(uri=binary.name,byteLength=len(exporter.binary))]
    path=output/'source-cloud.gltf';exporter.document.save_json(str(path))
    bounds=source.read(reader)['m_LocalAABB']
    return {'source':source_id(reader),'file':'reference/'+path.name,'buffer':'reference/'+binary.name,'vertex_count':len(points),'index_count':len(indices),
            'bounds_center':list(convert_point(vector(bounds['m_Center']))),'bounds_extents':list(vector(bounds['m_Extent']))}, [dependency(path,output),dependency(binary,output)]

def recover_sky_resources(source, output):
    """Recover linked sky geometry and stored image resources for the fixed source profile."""
    from .gltf import converted_transform
    from .render_lighting import startup_sample
    from .postprocessing import recover_postprocessing
    from .source import key
    settings_reader=source.get('level1',56810)
    settings=source.read(settings_reader)
    if settings['m_ReflectionBounces'] != 1 or settings['m_AmbientMode'] != 3:
        raise ValueError('Unrecovered source reflection or ambient mode')
    control_reader=source.get('level1',64920)
    controls=source.read(control_reader)
    sky_reader=source.resolve(settings_reader,settings['m_SkyboxMaterial'])
    sky=source.read(sky_reader)
    sky_shader=source.resolve(sky_reader,sky['m_Shader'])
    if key(sky_shader) != ('globalgamemanagers.assets',4):
        raise ValueError('Unrecovered source procedural sky shader')
    floats=dict(sky['m_SavedProperties']['m_Floats'])
    colors=dict(sky['m_SavedProperties']['m_Colors'])
    if floats['_SunDisk']!=2 or controls['sandStorms'] or controls['starColorDay']['a']!=0:
        raise ValueError('Unrecovered source startup sky variant')
    def rgba(color):return [color[channel] for channel in ('r','g','b','a')]
    post=recover_postprocessing(source)
    readers={}
    def texture(owner,pointer):
        reader=source.resolve(owner,pointer)
        if reader is None:raise ValueError('Missing required source lighting texture')
        readers[key(reader)]=reader
        return source_id(reader)
    cookie=texture(control_reader,controls['cloudShadowCookie'])
    for identity in [post['bloom']['dirt'],*post['blue_noise'],post['binoculars']['mask'],*post['luts'].values(),post['heat_haze']['noise']]:
        reader=source.get(identity['file'],identity['path_id']);readers[key(reader)]=reader
    clouds=[]
    mesh_reader=None
    for name,pid,material_field in [('low',27215,'materialCloudLow'),('high',27216,'materialCloudHigh')]:
        transform=source.get('level1',pid);tr=source.read(transform)
        if key(source.resolve(transform,tr['m_Father'])) != ('level1',27217):
            raise ValueError('Source cloud parent changed')
        go=source.resolve(transform,tr['m_GameObject'])
        components=[source.resolve(go,part['component']) for part in source.read(go)['m_Component']]
        renderer=next(part for part in components if part.type.name=='MeshRenderer')
        mesh_filter=next(part for part in components if part.type.name=='MeshFilter')
        mesh=source.resolve(mesh_filter,source.read(mesh_filter)['m_Mesh'])
        if key(mesh)!=('sharedassets0.assets',508):raise ValueError('Unexpected cloud geometry')
        mesh_reader=mesh
        material=source.resolve(control_reader,controls[material_field])
        rd=source.read(renderer)
        if source.read(go)['m_Layer']!=16 or not rd['m_Enabled']:
            raise ValueError('Unrecovered source cloud layer or renderer state')
        if len(rd['m_Materials'])!=1 or key(source.resolve(renderer,rd['m_Materials'][0]))!=key(material):
            raise ValueError('Unexpected cloud material binding')
        data=source.read(material)
        shader=source.resolve(material,data['m_Shader'])
        if key(shader)!=('sharedassets0.assets',721):raise ValueError('Unrecovered cloud shader')
        bindings=dict(data['m_SavedProperties']['m_TexEnvs'])
        slots=[]
        for slot in ['_ScatterMap0','_ScatterMap1','_DensityMap','_TextureMap']:
            binding=bindings[slot]
            slots.append({'slot':slot,'texture':texture(material,binding['m_Texture']),
                          'scale':[binding['m_Scale'][c] for c in ('x','y')],
                          'offset':[binding['m_Offset'][c] for c in ('x','y')]})
        title=name.title()
        clouds.append({'source':source_id(renderer),'material':source_id(material),
                       'transform':converted_transform(tr),
                       'color':rgba(controls[f'Cloud{title}DayColor']),
                       'speed':controls[f'Cloud{title}MidSpeed'],
                       'density':startup_sample()['cloud_density'],'textures':slots})
    output.mkdir(parents=True,exist_ok=True)
    textures=[];dependencies=[]
    for reader in readers.values():
        record,deps=export_texture(source,reader,output);textures.append(record);dependencies.extend(deps)
    mesh,deps=export_cloud_mesh(source,mesh_reader,output);dependencies.extend(deps)
    dither,dep=export_caster_dither(source,output);dependencies.append(dep)
    return {'sky':{'source':source_id(sky_reader),'shader':source_id(sky_shader),
                   'sun_size':floats['_SunSize'],'sun_convergence':floats['_SunSizeConvergence'],
                   'sun_disk':2,'capture_sun_disk':0,'exposure':controls['maxExposure'],
                   'atmosphere_thickness':1.,'tint':rgba(colors['_SkyTint']),
                   'ground':rgba(controls['daySkyGroundColors'][2]),
                   'cloud_root':converted_transform(source.read(source.get('level1',27217))),'cloud_mesh':mesh,'clouds':clouds},
            'cookie':cookie,'textures':textures,'caster_dither':dither,
            'postprocessing':post,'dependencies':dependencies}
