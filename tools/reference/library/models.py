"""Export shared geometry and complete authored model hierarchies."""
import copy
import hashlib
import os
from dataclasses import asdict, dataclass, field
from pathlib import Path
from urllib.parse import quote

import pygltflib as gl
from UnityPy.helpers.MeshHelper import MeshHandler

from .images import typed_record
from .records import _file, _json
from .types import AssetExport, Catalog, FileRecord, ObjectId, SCHEMA_VERSION
from ..gltf import GltfBuilder, convert_point, convert_triangles, converted_transform, convert_bind_poses, validate_gltf
from ..source import Source, key, source_id, vector


@dataclass
class ModelNode(gl.Node):
    weights: list[float] = field(default_factory=list)


def _identity(reader):
    return ObjectId(*key(reader))


def _reader(source, identity):
    return source.get(identity.file, identity.path_id)


def _prefix(kind, identity):
    return f'models/{kind}/{identity.file}/{identity.path_id}'


def _receipt(output, path):
    content = path.read_bytes()
    return FileRecord(path.relative_to(output).as_posix(), len(content), hashlib.sha256(content).hexdigest())


def _save(writer):
    writer.output.parent.mkdir(parents=True, exist_ok=True)
    if writer.binary:
        writer.document.buffers.append(gl.Buffer(uri=writer.output.with_suffix('.bin').name,byteLength=len(writer.binary)))
        writer.output.with_suffix('.bin').write_bytes(writer.binary)
    elif writer.output.with_suffix('.bin').is_file():
        writer.output.with_suffix('.bin').unlink()
    writer.document.save_json(str(writer.output))
    validate_gltf(writer.output)


def _indices(handler, submesh):
    size = 2 if handler.m_Use16BitIndices else 4
    if submesh.firstByte % size:
        raise ValueError('Submesh index offset is unaligned')
    start = submesh.firstByte // size
    raw = handler.m_IndexBuffer or []
    values = raw[start:start + submesh.indexCount]
    if len(values) != submesh.indexCount:
        raise ValueError('Submesh index range exceeds source buffer')
    values = [value + submesh.baseVertex for value in values]
    topology = int(submesh.topology)
    if topology == 0:
        if len(values) % 3: raise ValueError('Incomplete source triangle')
        values = [v for triangle in convert_triangles(zip(values[::3], values[1::3], values[2::3])) for v in triangle]
        return 4, values
    if topology == 1:
        triangles = []
        for i in range(len(values)-2):
            a,b,c = values[i:i+3]
            if len({a,b,c}) < 3: continue
            triangles.append((b,a,c) if i % 2 else (a,b,c))
        return 4, [v for triangle in convert_triangles(triangles) for v in triangle]
    if topology == 2:
        if len(values) % 4: raise ValueError('Incomplete source quad')
        triangles = []
        for i in range(0,len(values),4):
            a,b,c,d = values[i:i+4]
            triangles.extend(((a,b,c),(a,c,d)))
        return 4, [v for triangle in convert_triangles(triangles) for v in triangle]
    if topology == 3:
        if len(values) % 2: raise ValueError('Incomplete source line')
        return 1, values
    if topology == 4: return 3, values
    if topology == 5: return 0, values
    return None, values


def _morphs(writer, data, count):
    shapes = data.get('m_Shapes') or {}
    vertices, frames = shapes.get('vertices', []), shapes.get('shapes', [])
    channels, full_weights = shapes.get('channels', []), shapes.get('fullWeights', [])
    if len(frames) != len(full_weights):
        raise ValueError('Morph frame weight count mismatch')
    targets = []
    for frame in frames:
        start, length = frame['firstVertex'], frame['vertexCount']
        if start < 0 or start + length > len(vertices): raise ValueError('Morph vertex range exceeds source')
        attributes = {'POSITION': [(0.,0.,0.)]*count}
        if frame['hasNormals']: attributes['NORMAL'] = [(0.,0.,0.)]*count
        if frame['hasTangents']: attributes['TANGENT'] = [(0.,0.,0.)]*count
        seen = set()
        for vertex in vertices[start:start+length]:
            index = vertex['index']
            if not 0 <= index < count or index in seen: raise ValueError('Invalid morph vertex index')
            seen.add(index)
            for attribute, field in (('POSITION','vertex'),('NORMAL','normal'),('TANGENT','tangent')):
                if attribute in attributes: attributes[attribute][index] = convert_point(vector(vertex[field]))
        targets.append({name:writer.accessor(values,'VEC3') for name,values in attributes.items()})
    for channel in channels:
        start, length = channel['frameIndex'], channel['frameCount']
        if start < 0 or length <= 0 or start + length > len(frames): raise ValueError('Invalid morph channel frame range')
    return targets, {'channels': channels, 'frame_weights': full_weights, 'frame_count': len(frames),
                     'source_to_target':[{'channel':index,'source_frame':frame,'gltf_target':frame,'weight':full_weights[frame]}
                         for index,channel in enumerate(channels) for frame in range(channel['frameIndex'],channel['frameIndex']+channel['frameCount'])],
                     'vertex_delta_count': len(vertices)}


def _write_mesh(source, identity, catalog, output):
    reader = _reader(source, identity)
    data = source.read_record(reader)
    handler = MeshHandler(source.object(reader))
    handler.process()
    points = handler.m_Vertices or []
    details = {'name':data['m_Name'], 'type':'Mesh', 'typed_record':typed_record(identity),
               'vertex_count':len(points), 'source_bounds':data.get('m_LocalAABB'),
               'bind_pose_count':len(data.get('m_BindPose',[])), 'skin_weights':bool(handler.m_BoneWeights),
               'morph_channel_count':len((data.get('m_Shapes') or {}).get('channels',[])),
               'morph_frame_count':len((data.get('m_Shapes') or {}).get('shapes',[]))}
    if not points:
        return AssetExport(identity,'models','source_empty','unverified','Source mesh contains zero vertices.',(),
                           catalog.dependencies[identity], details)
    path = output / f'{_prefix("meshes",identity)}/geometry.gltf'
    writer = GltfBuilder(source,path)
    details['skin_attribute_widths'] = {'indices':sorted({len(v) for v in (handler.m_BoneIndices or [])}),
                                       'weights':sorted({len(v) for v in (handler.m_BoneWeights or [])})}
    def influences(values):
        if not values: return values
        if any(not 1 <= len(v) <= 4 for v in values): raise ValueError(f'Invalid skin influence width: {identity}')
        return [(*v,*([0]*(4-len(v)))) for v in values]
    skin_weights = handler.m_BoneWeights
    details['skin_weight_encoding'] = 'explicit' if skin_weights else 'none'
    if handler.m_BoneIndices and not skin_weights and all(len(v) == 1 for v in handler.m_BoneIndices):
        alternate = data.get('m_VariableBoneCountWeights',{}).get('m_Data',[]) or data.get('m_Skin',[]) or data.get('m_CompressedMesh',{}).get('m_Weights',{}).get('m_NumItems',0)
        if not data.get('m_BindPose') or alternate:
            raise ValueError(f'Invalid implicit OneBone source layout: {identity}')
        skin_weights = [(1.,)]*len(points)
        details['skin_weight_encoding'] = 'implicit_one_bone'
        details['skin_weight_conversion'] = 'Infer weight 1 from Unity single-index/no-weight layout; no weight values are stored.'
    if handler.m_BoneIndices and any(index < 0 or index >= len(data.get('m_BindPose',[])) for row in handler.m_BoneIndices for index in row):
        raise ValueError(f'Mesh joint index exceeds source bind poses: {identity}')
    if bool(handler.m_BoneIndices) != bool(skin_weights):
        raise ValueError(f'Incomplete skin attributes: {identity}')
    attributes = {'POSITION':writer.accessor([convert_point(v) for v in points],'VEC3')}
    for name, values, shape, component in (
        ('NORMAL',[convert_point(v[:3]) for v in (handler.m_Normals or [])],'VEC3',5126),
        ('TANGENT',[(*convert_point(v[:3]),-v[3]) for v in (handler.m_Tangents or [])],'VEC4',5126),
        ('COLOR_0',handler.m_Colors,'VEC4',5126),
        ('JOINTS_0',influences(handler.m_BoneIndices),'VEC4',5123),
        ('WEIGHTS_0',influences(skin_weights),'VEC4',5126),
        *[(f'TEXCOORD_{i}',getattr(handler,f'm_UV{i}'), 'VEC2',5126) for i in range(8)]):
        if values:
            if len(values) != len(points): raise ValueError(f'Attribute count mismatch: {identity}: {name}')
            try:
                attributes[name] = writer.accessor(values,shape,component)
            except ValueError as error:
                raise ValueError(f'{identity} {name} row widths={sorted({len(v) for v in values})}: {error}') from error
    targets, morph = _morphs(writer,data,len(points))
    primitives, unsupported = [], []
    for index,submesh in enumerate(handler.src.m_SubMeshes):
        mode, indices = _indices(handler,submesh)
        if any(i < 0 or i >= len(points) for i in indices): raise ValueError(f'Mesh index outside vertices: {identity}')
        if mode is None:
            unsupported.append({'submesh':index,'topology':int(submesh.topology),
                                'reason':'Unity topology has no supported glTF mapping.'})
        elif indices:
            primitives.append(gl.Primitive(attributes=gl.Attributes(**attributes),
                indices=writer.accessor(indices,'SCALAR',5125,34963), mode=mode, targets=targets,
                extras={'submesh':index,'source':asdict(identity),'source_topology':int(submesh.topology)}))
    details.update({'attributes':sorted(attributes),'submesh_count':len(handler.src.m_SubMeshes),
                    'primitive_count':len(primitives),'unsupported_topology':unsupported,'morph':morph})
    inverse_bind = writer.accessor(convert_bind_poses(data['m_BindPose']),'MAT4',target=None) if data.get('m_BindPose') else None
    metadata = {'source':asdict(identity),'typed_record':typed_record(identity),
                'source_bounds':data.get('m_LocalAABB'),'morph':morph,
                'inverse_bind_accessor':inverse_bind,'bind_pose_count':len(data.get('m_BindPose',[]))}
    if primitives:
        writer.document.meshes.append(gl.Mesh(name=data['m_Name'],primitives=primitives,
            weights=[0.]*len(targets) if targets else [],extras=metadata))
    else:
        writer.document.extras = metadata
    _save(writer)
    files = (_receipt(output,path),_receipt(output,path.with_suffix('.bin')))
    reason = 'Unsupported topology retained in typed source record.' if unsupported else 'Source mesh has no drawable submesh.' if not primitives else ''
    status = 'native_record' if unsupported or not primitives else 'converted'
    return AssetExport(identity,'models',status,'unverified',reason,files,catalog.dependencies[identity],details)


def collect_model_hierarchy(source: Source, root: ObjectId, catalog: Catalog) -> tuple[ObjectId, ...]:
    """Collect descendants and each authored skeleton's ancestor chain."""
    reader = _reader(source,root)
    if reader.type.name == 'GameObject':
        reader = next(r for r in (source.resolve(reader,p['component']) for p in source.read(reader)['m_Component'])
                      if r.type.name in ('Transform','RectTransform'))
    found = {}
    pending = [reader]
    while pending:
        node = pending.pop()
        identity = _identity(node)
        if identity in found: raise ValueError(f'Duplicate hierarchy reference: {identity}')
        found[identity] = node
        pending.extend(source.resolve(node,p) for p in reversed(source.read(node)['m_Children']))
    for node in tuple(found.values()):
        _, components = source.components(node)
        for renderer in components:
            if renderer.type.name != 'SkinnedMeshRenderer': continue
            data = source.read(renderer)
            for pointer in [*data['m_Bones'],data['m_RootBone']]:
                bone = source.resolve(renderer,pointer)
                unbound = not data['m_RootBone']['m_PathID'] and not any(p['m_PathID'] for p in data['m_Bones'])
                if bone is None and pointer in data['m_Bones'] and data['m_Mesh']['m_PathID'] and not unbound:
                    raise ValueError(f'Missing authored bone: {key(renderer)}: {pointer}')
                seen = set()
                while bone is not None:
                    identity = _identity(bone)
                    if identity in seen: raise ValueError(f'Skeleton parent cycle: {identity}')
                    if identity not in catalog.objects: raise ValueError(f'Missing authored bone: {identity}')
                    seen.add(identity)
                    found[identity] = bone
                    bone = source.resolve(bone,source.read(bone)['m_Father'])
    first = _identity(reader)
    return (first,*sorted(identity for identity in found if identity != first))


def _import_geometry(writer, output, identity):
    path = output / f'{_prefix("meshes",identity)}/geometry.gltf'
    if not path.is_file(): return None
    geometry = gl.GLTF2().load_json(str(path))
    if not geometry.meshes: return None
    accessors, views, buffers = len(writer.document.accessors),len(writer.document.bufferViews),len(writer.document.buffers)
    for buffer in geometry.buffers:
        buffer.uri = quote(Path(os.path.relpath(path.parent / buffer.uri,writer.output.parent)).as_posix(),safe='/')
        writer.document.buffers.append(buffer)
    for view in geometry.bufferViews:
        view.buffer += buffers
        writer.document.bufferViews.append(view)
    for accessor in geometry.accessors:
        accessor.bufferView += views
        writer.document.accessors.append(accessor)
    mesh = geometry.meshes[0]
    if mesh.extras.get('inverse_bind_accessor') is not None:
        mesh.extras['inverse_bind_accessor'] += accessors
    for primitive in mesh.primitives:
        primitive.indices += accessors
        for name,value in vars(primitive.attributes).items():
            if isinstance(value,int): setattr(primitive.attributes,name,value+accessors)
        primitive.targets = [{name:value+accessors for name,value in target.items()} for target in primitive.targets]
    return mesh


def _normal_rgb(image):
    from math import sqrt
    from PIL import Image
    result = Image.new('RGB', image.size)
    result.putdata([(round(r*a/255), g,
                     round((sqrt(max(0., 1.-(2*r*a/65025-1)**2-(2*g/255-1)**2))+1)*127.5))
                    for r,g,_,a in image.convert('RGBA').get_flattened_data()])
    return result


def _texture(writer, owner, environment, role, floats, standard, coordinates):
    from PIL import Image
    reader = writer.source.resolve(owner, environment['m_Texture'])
    if reader is None: return None
    identity = _identity(reader)
    data = writer.source.read(reader)
    if reader.type.name != 'Texture2D':
        writer.texture_limits.append({'source':asdict(identity),'role':role,
            'reason':f'{reader.type.name} needs Unity runtime texture support.'})
        return None
    if not data.get('m_CompleteImageSize',0):
        writer.texture_limits.append({'source':asdict(identity),'role':role,'reason':'Source texture contains no stored pixels.'})
        return None
    if role == 'normal' and not standard:
        writer.texture_limits.append({'source':asdict(identity),'role':role,
            'reason':'Normal packing needs a supported Unity shader adapter.'})
        return None
    settings = data['m_TextureSettings']
    color_space = 'linear' if role in ('normal','metallic_roughness','occlusion') else 'srgb'
    adaptation = floats.get('_GlossMapScale',1.) if role == 'metallic_roughness' else None
    conversion = 'rg-or-ag' if standard and role == 'normal' else 'unity-g' if standard and role == 'occlusion' else ''
    texture_key = (identity,role,adaptation,conversion)
    if texture_key not in writer.textures:
        original = writer.library_output / f'images/{identity.file}/{identity.path_id}/source.png'
        name = role + (f'-{conversion}' if conversion else '') + (f'-{adaptation.hex()}' if isinstance(adaptation,float) else '')
        derived = writer.library_output / f'models/textures/{identity.file}/{identity.path_id}/{name}.png'
        if not original.is_file() or not derived.is_file():
            if not original.is_file():
                original.parent.mkdir(parents=True,exist_ok=True)
                writer.source.object(reader).image.save(original)
            with Image.open(original) as stored:
                image = stored.transpose(Image.Transpose.FLIP_TOP_BOTTOM)
            if conversion == 'rg-or-ag': image = _normal_rgb(image)
            elif conversion == 'unity-g': image = image.convert('RGB').getchannel('G')
            if role == 'metallic_roughness':
                red,_,_,alpha = image.convert('RGBA').split()
                roughness = alpha.point(lambda value:round(255-value*adaptation))
                image = Image.merge('RGB',(Image.new('L',image.size,0),roughness,red))
            derived.parent.mkdir(parents=True,exist_ok=True)
            if not derived.is_file(): image.save(derived)
        writer.library_files.update((original,derived))
        wrapping = {0:10497,1:33071,2:33648}
        filters = {0:(9728,9984),1:(9729,9985),2:(9729,9987)}
        if settings['m_WrapU'] not in wrapping or settings['m_WrapV'] not in wrapping or settings['m_FilterMode'] not in filters:
            raise ValueError(f'Unsupported authored sampler: {identity}')
        mag,minimum = filters[settings['m_FilterMode']]
        if data['m_MipCount'] <= 1: minimum = mag
        sampler = len(writer.document.samplers)
        writer.document.samplers.append(gl.Sampler(wrapS=wrapping[settings['m_WrapU']],wrapT=wrapping[settings['m_WrapV']],
            magFilter=mag,minFilter=minimum,extras={'unity_aniso':settings['m_Aniso'],'unity_mip_bias':settings['m_MipBias']}))
        image_index = len(writer.document.images)
        writer.document.images.append(gl.Image(uri=quote(Path(os.path.relpath(derived,writer.output.parent)).as_posix(),safe='/'),
            extras={'source':asdict(identity),'source_image':original.relative_to(writer.library_output).as_posix(),
                    'role':role,'color_space':color_space,'orientation':'flip_y',
                    'adaptation':'Unity RG/AG packed normal to glTF RGB XYZ' if conversion == 'rg-or-ag' else
                                 'Unity occlusion G to glTF R' if conversion == 'unity-g' else
                                 'Unity metallic R and smoothness A to glTF roughness G and metallic B' if role == 'metallic_roughness' else 'source pixels'}))
        writer.textures[texture_key] = len(writer.document.textures)
        writer.document.textures.append(gl.Texture(source=image_index,sampler=sampler))
        getattr(writer.source,'object_cache',{}).pop(key(reader),None)
    if 'KHR_texture_transform' not in writer.document.extensionsUsed: writer.document.extensionsUsed.append('KHR_texture_transform')
    return {'index':writer.textures[texture_key], 'extensions':{'KHR_texture_transform':{
        'offset':[coordinates['m_Offset']['x'],coordinates['m_Offset']['y']],
        'scale':[coordinates['m_Scale']['x'],coordinates['m_Scale']['y']]}}}


def _material(writer, reader, cache):
    if reader is None: return None
    identity = _identity(reader)
    if identity in cache: return cache[identity]
    data = writer.source.read(reader)
    properties = data['m_SavedProperties']
    colors, floats = dict(properties.get('m_Colors',[])),dict(properties.get('m_Floats',[]))
    environments = dict(properties.get('m_TexEnvs',[]))
    color = colors.get('_Color',colors.get('_BaseColor',{'r':1.,'g':1.,'b':1.,'a':1.}))
    mode = int(floats.get('_Mode',0))
    if mode == 0 and data.get('m_CustomRenderQueue',-1) >= 3000: mode = 2
    links = []
    for name, environment in properties.get('m_TexEnvs',[]):
        texture = writer.source.resolve(reader,environment['m_Texture'])
        links.append({'property':name,'source':source_id(texture) if texture else None,
                      'scale':environment['m_Scale'],'offset':environment['m_Offset']})
    shader = writer.source.resolve(reader,data['m_Shader'])
    standard = shader is not None and writer.source.read(shader).get('m_ParsedForm',{}).get('m_Name') in ('Standard','Standard (Specular setup)')
    if standard and '_MainTex' not in environments:
        raise ValueError(f'Standard material lacks primary UV coordinates: {identity}')
    def unit(value): return min(1.,max(0.,value))
    factors = [color[n] for n in ('r','g','b','a')]
    adapted = any(unit(v) != v for v in factors) or unit(floats.get('_Metallic',0.)) != floats.get('_Metallic',0.) or unit(1.-floats.get('_Glossiness',.5)) != 1.-floats.get('_Glossiness',.5)
    material = gl.Material(name=data['m_Name'],
        alphaMode={0:'OPAQUE',1:'MASK',2:'BLEND',3:'BLEND'}.get(mode,'OPAQUE'),
        alphaCutoff=floats.get('_Cutoff',.5) if mode == 1 else None,
        doubleSided=floats.get('_Cull',2)==0,
        pbrMetallicRoughness=gl.PbrMetallicRoughness(baseColorFactor=[unit(v) for v in factors],
            metallicFactor=unit(floats.get('_Metallic',0.)),roughnessFactor=unit(1.-floats.get('_Glossiness',.5))),
        extras={'source':asdict(identity),'typed_record':typed_record(identity),
                'shader':source_id(shader) if shader else None,'textures':links,
                'texture_adapter':'unity_standard_primary' if standard else 'authored_map_transforms',
                'factor_adaptation':'Clamp out-of-range Unity factors to glTF bounds; native values retain typed links.' if adapted else 'none'})
    for name in ('_MainTex','_BaseMap'):
        if name in environments:
            info = _texture(writer,reader,environments[name],'base_color',floats,standard,environments['_MainTex'] if standard else environments[name])
            if info: material.pbrMetallicRoughness.baseColorTexture = gl.TextureInfo(**info)
            break
    for name,role in (('_BumpMap','normal'),('_MetallicGlossMap','metallic_roughness'),('_OcclusionMap','occlusion'),('_EmissionMap','emission')):
        if name not in environments: continue
        info = _texture(writer,reader,environments[name],role,floats,standard,environments['_MainTex'] if standard else environments[name])
        if not info: continue
        if role == 'normal': material.normalTexture = gl.NormalMaterialTexture(scale=floats.get('_BumpScale',1.),**info)
        elif role == 'metallic_roughness':
            material.pbrMetallicRoughness.metallicRoughnessTexture = gl.TextureInfo(**info)
            material.pbrMetallicRoughness.metallicFactor = 1.
            material.pbrMetallicRoughness.roughnessFactor = 1.
        elif role == 'occlusion': material.occlusionTexture = gl.OcclusionTextureInfo(strength=floats.get('_OcclusionStrength',1.),**info)
        elif role == 'emission': material.emissiveTexture = gl.TextureInfo(**info)
    if '_EmissionColor' in colors:
        material.emissiveFactor = [min(1.,max(0.,colors['_EmissionColor'][c])) for c in ('r','g','b')]
    cache[identity] = len(writer.document.materials)
    writer.document.materials.append(material)
    return cache[identity]


def _morph_weights(mesh, renderer):
    morph = mesh.extras['morph']
    authored = renderer.get('m_BlendShapeWeights',[])
    channels = morph['channels']
    if authored and len(authored) != len(channels): raise ValueError('Renderer morph channel count mismatch')
    weights = [0.]*morph['frame_count']
    for index,channel in enumerate(channels):
        value = authored[index] if authored else 0.
        first, count = channel['frameIndex'],channel['frameCount']
        frames = morph['frame_weights'][first:first+count]
        if any(frames[i] <= frames[i-1] for i in range(1,len(frames))): raise ValueError('Morph frame weights are not ordered')
        # Unity interpolates adjacent authored frames, including extrapolation.
        if value <= frames[0]:
            if frames[0] == 0:
                if value: raise ValueError('Cannot extrapolate a zero-weight first morph frame')
            else: weights[first] = value / frames[0]
        elif count == 1:
            weights[first] = value / frames[0]
        else:
            high = next((i for i,w in enumerate(frames) if w >= value),count-1)
            low = high-1
            fraction = (value-frames[low])/(frames[high]-frames[low])
            weights[first+low],weights[first+high] = 1.-fraction,fraction
    return weights


def write_library_model(source: Source, nodes: tuple[ObjectId, ...], output: Path) -> AssetExport:
    """Write placement and rig data against shared mesh buffers."""
    root = _reader(source,nodes[0])
    owner = _identity(source.components(root)[0])
    path = output / f'{_prefix("hierarchies",owner)}/model.gltf'
    writer = GltfBuilder(source,path)
    writer.library_output = output
    writer.library_files = set()
    writer.texture_limits = []
    mapping = {identity:index for index,identity in enumerate(nodes)}
    geometry, materials, variants = {},{},{}
    renderers, dependencies = [],set()
    for identity in nodes:
        reader = _reader(source,identity)
        data = source.read(reader)
        go, components = source.components(reader)
        go_data = source.read(go)
        dependencies.update((_identity(go),identity))
        node = ModelNode(name=go_data['m_Name'],**converted_transform(data),extras={
            'source':asdict(identity),'game_object':source_id(go),'typed_record':typed_record(identity),
            'active':bool(go_data['m_IsActive']),'layer':go_data['m_Layer'],'renderers':[],'lod_groups':[]})
        for component in components:
            dependencies.add(_identity(component))
            if component.type.name == 'LODGroup':
                node.extras['lod_groups'].append({'source':source_id(component),'lods':source.read(component)['m_LODs']})
            if component.type.name not in ('MeshRenderer','SkinnedMeshRenderer'): continue
            renderer_data = source.read(component)
            info = {'source':source_id(component),'enabled':bool(renderer_data['m_Enabled']),
                    'typed_record':typed_record(_identity(component))}
            node.extras['renderers'].append(info)
            renderers.append(info)
            mesh_owner = component if component.type.name == 'SkinnedMeshRenderer' else next(
                (c for c in components if c.type.name == 'MeshFilter'),None)
            mesh_reader = source.resolve(mesh_owner,source.read(mesh_owner)['m_Mesh']) if mesh_owner else None
            info['mesh'] = source_id(mesh_reader) if mesh_reader else None
            if not mesh_reader:
                if component.type.name == 'SkinnedMeshRenderer':
                    info.update({'bones':[source_id(b) if (b:=source.resolve(component,p)) else None for p in renderer_data['m_Bones']],
                                 'root_bone':renderer_data['m_RootBone'],'morph_weights':renderer_data['m_BlendShapeWeights'],
                                 'rig_status':'source_mesh_null','bind_pose_count':None})
                continue
            mesh_id = _identity(mesh_reader)
            dependencies.add(mesh_id)
            if mesh_id not in geometry: geometry[mesh_id] = _import_geometry(writer,output,mesh_id)
            mesh = geometry[mesh_id]
            bones = []
            if component.type.name == 'SkinnedMeshRenderer':
                bones = [source.resolve(component,p) for p in renderer_data['m_Bones']]
                unbound = bool(bones) and all(b is None for b in bones) and not renderer_data['m_RootBone']['m_PathID']
                if not unbound and any(b is None or _identity(b) not in mapping for b in bones):
                    raise ValueError(f'Missing authored bone: {key(component)}')
                bind_count = mesh.extras['bind_pose_count'] if mesh else len(source.read_record(mesh_reader)['m_BindPose'])
                info.update({'bones':[source_id(b) if b else None for b in bones], 'bind_pose_count':bind_count,
                             'root_bone':renderer_data['m_RootBone'], 'morph_weights':renderer_data['m_BlendShapeWeights']})
                if bones and bind_count != len(bones): raise ValueError(f'Skin bind pose count mismatch: {key(component)} mesh={key(mesh_reader)} bones={len(bones)} bindposes={bind_count}')
                if bones and mesh is not None and not unbound:
                    root_bone = source.resolve(component,renderer_data['m_RootBone'])
                    node.skin = len(writer.document.skins)
                    writer.document.skins.append(gl.Skin(joints=[mapping[_identity(b)] for b in bones],
                        skeleton=mapping[_identity(root_bone)] if root_bone else None,
                        inverseBindMatrices=mesh.extras['inverse_bind_accessor'],
                        extras={'source':source_id(component),'mesh':source_id(mesh_reader)}))
                info['rig_status'] = 'source_unbound_skin' if unbound else 'authored' if bones else 'orphan_no_authored_bones'
                if unbound:
                    info['unbound_bone_slots'] = len(bones)
                    info['reason'] = 'Source bone slots are null; Unity runtime binding is required.'
            if mesh is None: continue
            material_readers = [source.resolve(component,p) for p in renderer_data['m_Materials']]
            for material_reader in material_readers:
                if material_reader: dependencies.add(_identity(material_reader))
            variant = (mesh_id,tuple(_identity(r) if r else None for r in material_readers))
            if variant not in variants:
                instance = copy.deepcopy(mesh)
                for primitive in instance.primitives:
                    index = primitive.extras['submesh']
                    primitive.material = _material(writer,material_readers[index] if index < len(material_readers) else None,materials)
                    primitive.extras['material_source'] = source_id(material_readers[index]) if index < len(material_readers) and material_readers[index] else None
                variants[variant] = len(writer.document.meshes)
                writer.document.meshes.append(instance)
            if node.mesh is not None: raise ValueError(f'Multiple mesh renderers on node: {identity}')
            node.mesh = variants[variant]
            if mesh.extras['morph']['frame_count']:
                node.weights = _morph_weights(mesh,renderer_data)
        writer.document.nodes.append(node)
    roots = []
    for identity in nodes:
        reader = _reader(source,identity)
        father = source.resolve(reader,source.read(reader)['m_Father'])
        parent = mapping.get(_identity(father)) if father else None
        if parent is None: roots.append(mapping[identity])
        else: writer.document.nodes[parent].children.append(mapping[identity])
    writer.document.scenes.append(gl.Scene(nodes=roots))
    writer.document.scene = 0
    writer.document.extras = {'source':asdict(owner),'authored_root':asdict(nodes[0]),
                             'geometry_storage':'shared_external_buffers','visibility':'authored_states_all_lods'}
    for material in writer.document.materials:
        if material.extras['shader']: dependencies.add(ObjectId(**material.extras['shader']))
        dependencies.update(ObjectId(**t['source']) for t in material.extras['textures'] if t['source'])
    _save(writer)
    unbound_skin = any(r.get('rig_status') == 'source_unbound_skin' for r in renderers)
    return AssetExport(owner,'models','converted','engine_dependent' if unbound_skin else 'unverified',
        'Source skin needs Unity runtime bone binding.' if unbound_skin else '',(_receipt(output,path),
        *(_receipt(output,p) for p in sorted(writer.library_files))),
        tuple(sorted(dependencies)),{'name':source.read(source.components(root)[0])['m_Name'],
        'type':'Hierarchy','typed_record':typed_record(owner),'root_transform':asdict(nodes[0]),
        'nodes':[asdict(n) for n in nodes],'node_count':len(nodes),'renderers':renderers,
        'skin_count':len(writer.document.skins),'mesh_count':len(writer.document.meshes),
        'material_adaptation':'PBR factors and textures; Unity shader properties retain typed source links.',
        'texture_limits':writer.texture_limits})


def export_models(source: Source, catalog: Catalog, output: Path) -> tuple[AssetExport, ...]:
    exports = []
    roots = []
    for identity,record in sorted(catalog.objects.items()):
        if record.type == 'Mesh':
            exports.append(_write_mesh(source,identity,catalog,output))
            # Keep one decoded mesh at a time.
            getattr(source,'object_cache',{}).pop((identity.file,identity.path_id),None)
            getattr(source,'data_cache',{}).pop((identity.file,identity.path_id),None)
        elif record.type in ('Transform','RectTransform'):
            reader = _reader(source,identity)
            if source.resolve(reader,source.read(reader)['m_Father']) is None: roots.append(identity)
    for root in roots:
        nodes = collect_model_hierarchy(source,root,catalog)
        exports.append(write_library_model(source,nodes,output))
    exports = tuple(sorted(exports,key=lambda e:e.source))
    _file(output,'models/index.json',_json({'schema_version':SCHEMA_VERSION,'exports':[asdict(e) for e in exports]}))
    return exports


def animation_model_document(model: AssetExport, output: Path, destination: Path):
    """Load separate animation nodes with canonical shared geometry and image URIs."""
    import pygltflib as gl
    from urllib.parse import unquote, quote
    import os
    path = output / next(f.file for f in model.files if f.file.endswith('/model.gltf'))
    document = gl.GLTF2.load(str(path))
    # pygltflib's Node omits morph weights; preserve the existing extended node.
    import json
    document.nodes = [ModelNode.from_dict(node) for node in json.loads(path.read_text())['nodes']]
    root = output.resolve()
    for item in (*document.buffers, *document.images):
        if not item.uri: raise ValueError('Expected external model resource')
        resource = (path.parent / unquote(item.uri)).resolve()
        if not resource.is_relative_to(root) or not resource.is_file():
            raise ValueError(f'Invalid animation model resource: {item.uri}')
        item.uri = quote(Path(os.path.relpath(resource,destination)).as_posix(),safe='/')
    return document
