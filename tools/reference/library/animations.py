"""Decode Unity 2021 optimized clips and link source rig instances.

Format analysis: AssetRipper fd2e17582114c6bca373f598eb4a4d915fdf5281,
AnimationClipConverter.cs and Editor/Streamed{CurveKey,Frame}.cs (GPL-3.0).
No source text is copied. Polynomial derivatives and Hermite evaluation below
implement the stored format through UnityPy's existing binary reader.
"""
import bisect
import math
import struct
from dataclasses import asdict
from pathlib import Path

import attrs
from UnityPy.streams import EndianBinaryReader

from ..gltf import convert_point, convert_rotation
from ..source import Source, source_id
from .types import AnimationRecord, AnimationTrack, AssetExport, Catalog, ObjectId
from .records import _file, _json
from .images import typed_record

FORMAT_REFERENCE = {
    'revision': 'fd2e17582114c6bca373f598eb4a4d915fdf5281', 'license': 'GPL-3.0',
    'url': 'https://github.com/AssetRipper/AssetRipper/blob/fd2e17582114c6bca373f598eb4a4d915fdf5281/Source/AssetRipper.Processing/AnimationClips/AnimationClipConverter.cs',
    'copied_source': False, 'decoder': 'UnityPy Clip and EndianBinaryReader',
    'negative_infinity_slopes': False,
}


def _fields(value):
    if attrs.has(type(value)):
        return {f.name: getattr(value, f.name) for f in attrs.fields(type(value))}
    return vars(value)


def _native(value, source, owner, catalog):
    if hasattr(value, 'm_FileID') and hasattr(value, 'm_PathID'):
        pointer = {'m_FileID': value.m_FileID, 'm_PathID': value.m_PathID}
        target = source.resolve(owner, pointer)
        if target is None: return None
        identity = ObjectId(**source_id(target))
        if identity not in catalog.objects: raise ValueError(f'Missing animation pointer: {identity}')
        return asdict(identity)
    if isinstance(value, float) and not math.isfinite(value):
        return {'$float': 'nan' if math.isnan(value) else '+inf' if value > 0 else '-inf'}
    if value is None or isinstance(value, (str, bool, int, float)): return value
    if isinstance(value, (list, tuple)):
        return [_native(v, source, owner, catalog) for v in value]
    fields = value if isinstance(value, dict) else _fields(value)
    return {k: _native(v, source, owner, catalog) for k, v in fields.items()}


def _dimension(binding):
    if binding.typeID == 4:
        if binding.attribute not in (1, 2, 3, 4):
            raise ValueError(f'Unknown Transform attribute: {binding.attribute}')
        return 4 if binding.attribute == 2 else 3
    return 1


def _stream_frames(words, endian):
    payload = struct.pack(endian + str(len(words)) + 'I', *words)
    reader = EndianBinaryReader(payload, endian=endian)
    frames = []
    while reader.Position < len(payload):
        time, count = reader.read_float(), reader.read_int()
        if count < 0 or count * 20 > len(payload) - reader.Position:
            raise ValueError('Invalid streamed frame size')
        keys = {}
        previous = None
        for _ in range(count):
            index = reader.read_int()
            coefficients = tuple(reader.read_float() for _ in range(3))
            value = reader.read_float()
            if index in keys and index != previous: raise ValueError('Unordered duplicate streamed key')
            keys[index] = (coefficients, value)
            previous = index
        reader.align_stream()
        frames.append((time, keys))
    if frames and (frames[-1][0] != math.inf or (len(frames) > 1 and frames[0][0] >= 0)):
        raise ValueError('Missing streamed sentinel frames')
    return frames


def _scalar_streams(frames, bindings):
    curves = [[] for _ in bindings]
    for frame_index, (time, keys) in enumerate(frames[:-1]):
        for index, (coefficients, value) in keys.items():
            if not 0 <= index < len(bindings): raise ValueError('Stream curve exceeds binding count')
            binding = bindings[index]
            if frame_index == 0:
                if not binding.isPPtrCurve: continue
                time = 0.
            if not math.isfinite(time) or time < 0 or not all(map(math.isfinite, (*coefficients, value))):
                raise ValueError('Invalid streamed numeric value')
            key = [time, value, 0., 0., coefficients]
            if curves[index] and curves[index][-1][0] == time:
                curves[index][-1] = key
            else:
                curves[index].append(key)
    for binding, keys in zip(bindings, curves):
        for left, right in zip(keys, keys[1:]):
            dt = right[0] - left[0]
            if dt <= 0: raise ValueError('Non-monotonic streamed times')
            a, b, c = left[4]
            if binding.isPPtrCurve or (a == b == c == 0 and left[1] != right[1]):
                left[3] = math.inf
            else:
                left[3] = c
                right[2] = (3*a*dt + 2*b)*dt + c
    return curves


def decode_clip(source: Source, reader, catalog: Catalog) -> AnimationRecord:
    # The single-argument AssetRipper comparison tests the major version only.
    # Unity 2021.3 therefore uses positive-infinity steps, without negative slopes.
    if str(reader.version).split('.')[0] != '2021': raise ValueError('Unsupported animation version')
    obj = reader.parse_as_object(check_read=True)
    muscle = obj.m_MuscleClip
    clip = muscle.m_Clip.data
    bindings = obj.m_ClipBindingConstant.genericBindings
    expanded = [b for b in bindings for _ in range(_dimension(b))]
    streamed, dense, constant = clip.m_StreamedClip, clip.m_DenseClip, clip.m_ConstantClip
    constants = constant.data if constant else []
    count = streamed.curveCount + dense.m_CurveCount + len(constants)
    if count != len(expanded): raise ValueError('Optimized curve and binding counts differ')
    if not math.isfinite(muscle.m_StopTime) or muscle.m_StopTime < muscle.m_StartTime:
        raise ValueError('Invalid animation duration')
    if obj.m_SampleRate <= 0: raise ValueError('Invalid clip sample rate')
    frames = _stream_frames(streamed.data, reader.assets_file.reader.endian)
    scalars = _scalar_streams(frames, expanded[:streamed.curveCount])
    if len(dense.m_SampleArray) != dense.m_CurveCount * dense.m_FrameCount:
        raise ValueError('Dense frame size differs from curve count')
    if dense.m_CurveCount and (dense.m_FrameCount < 1 or dense.m_SampleRate <= 0):
        raise ValueError('Invalid dense sample rate or frame count')
    for column in range(dense.m_CurveCount):
        scalars.append([[dense.m_BeginTime + frame/dense.m_SampleRate,
                         dense.m_SampleArray[frame*dense.m_CurveCount+column], 0., 0., None]
                        for frame in range(dense.m_FrameCount)])
    stop = muscle.m_StopTime
    if len(frames) > 1:
        last_time, last_keys = frames[-2]
        if round(stop*obj.m_SampleRate) - round(max(last_time, 0)*obj.m_SampleRate) == 1:
            if any(expanded[i].isPPtrCurve for i in last_keys): stop = max(last_time, 0.)
    for value in constants:
        scalars.append([[t, value, 0., 0., None] for t in ((0., stop) if stop else (0.,))])
    tracks, offset = [], 0
    mapping = _native(obj.m_ClipBindingConstant.pptrCurveMapping, source, reader, catalog)
    for binding in bindings:
        width = _dimension(binding)
        components = scalars[offset:offset+width]
        if any(not keys for keys in components): raise ValueError(f'Empty optimized curve at {offset}')
        times = tuple(k[0] for k in components[0])
        if any(tuple(k[0] for k in keys) != times for keys in components):
            raise ValueError('Transform component key times differ')
        if any(b <= a for a, b in zip(times,times[1:])) or not all(map(math.isfinite,times)):
            raise ValueError('Non-monotonic animation times')
        values = tuple(tuple(keys[i][1] for keys in components) for i in range(len(times)))
        if not all(math.isfinite(v) for row in values for v in row): raise ValueError('Non-finite animation value')
        incoming = tuple(tuple(keys[i][2] for keys in components) for i in range(len(times)))
        outgoing = tuple(tuple(keys[i][3] for keys in components) for i in range(len(times)))
        storage = 'streamed' if offset < streamed.curveCount else 'dense' if offset < streamed.curveCount+dense.m_CurveCount else 'constant'
        info = {'type_id': binding.typeID, 'attribute': binding.attribute, 'path_hash': binding.path,
                'custom_type': binding.customType, 'is_pptr': bool(binding.isPPtrCurve),
                'script': _native(binding.script,source,reader,catalog), 'curve_offset':offset,
                'dimension':width, 'storage':storage, 'native':_native(binding,source,reader,catalog),
                'property': {1:'translation',2:'rotation',3:'scale',4:'euler_degrees'}.get(binding.attribute) if binding.typeID == 4 else 'native'}
        if binding.isPPtrCurve:
            refs=[]
            for row in values:
                index=int(row[0])
                if row[0] != index or index >= len(mapping): raise ValueError('Invalid object curve mapping index')
                refs.append(mapping[index] if index >= 0 else None)
            info['object_references']=refs
        mode = 'STEP' if binding.isPPtrCurve else 'CUBIC' if storage == 'streamed' else 'LINEAR' if storage == 'dense' else 'CONSTANT'
        coefficients = tuple(tuple(keys[i][4] for keys in components) for i in range(len(times))) if storage == 'streamed' else ()
        tracks.append(AnimationTrack(info,times,values,mode,incoming,outgoing,coefficients))
        offset += width
    if not tracks: raise ValueError('Stored clip contains no optimized tracks')
    settings = {k: _native(v,source,reader,catalog) for k,v in _fields(obj).items()
                if k not in ('m_MuscleClip','m_ClipBindingConstant','m_Events') and not (k.endswith('Curves') and isinstance(v,list))}
    settings['muscle'] = {k:_native(v,source,reader,catalog) for k,v in _fields(muscle).items() if k != 'm_Clip'}
    settings['duration'] = muscle.m_StopTime-muscle.m_StartTime
    settings['curve_counts'] = {'streamed':streamed.curveCount,'dense':dense.m_CurveCount,'constant':len(constants)}
    settings['format_reference'] = FORMAT_REFERENCE
    return AnimationRecord(ObjectId(source.container_path(reader),reader.path_id),tuple(tracks),
                           tuple(_native(e,source,reader,catalog) for e in obj.m_Events),settings)


def sample_track(track: AnimationTrack, time: float) -> tuple[float, ...]:
    if time <= track.times[0]: return track.values[0]
    if time >= track.times[-1]: return track.values[-1]
    i = bisect.bisect_right(track.times,time)-1
    if track.interpolation in ('STEP','CONSTANT'): return track.values[i]
    dt=track.times[i+1]-track.times[i]
    u=(time-track.times[i])/dt
    if track.interpolation == 'LINEAR':
        return tuple(a+(b-a)*u for a,b in zip(track.values[i],track.values[i+1]))
    if track.interpolation != 'CUBIC': raise ValueError(f'Unknown interpolation: {track.interpolation}')
    if track.coefficients:
        elapsed=time-track.times[i]
        return tuple(value if math.isinf(outgoing) else ((a*elapsed+b)*elapsed+c)*elapsed+value
                     for value,outgoing,(a,b,c) in zip(track.values[i],track.out_tangents[i],track.coefficients[i]))
    h00=(2*u-3)*u*u+1;h10=((u-2)*u+1)*u;h01=(-2*u+3)*u*u;h11=(u-1)*u*u
    return tuple(a if math.isinf(outgoing) else h00*a+h10*dt*outgoing+h01*b+h11*dt*incoming
                 for a,b,outgoing,incoming in zip(track.values[i],track.values[i+1],track.out_tangents[i],track.in_tangents[i+1]))


def resolve_path(path_hash, paths):
    matches=set(paths.get(path_hash,()))
    if not matches: raise ValueError(f'Missing animation target hash: {path_hash}')
    if len(matches) != 1: raise ValueError(f'Ambiguous animation target hash: {path_hash}: {sorted(matches)}')
    return next(iter(matches))


def _unit(q):
    length=math.sqrt(sum(v*v for v in q))
    if not math.isfinite(length) or length < 1e-12: raise ValueError('Zero or invalid animation quaternion')
    return tuple(v/length for v in q)


def _rotation(euler):
    x,y,z=(math.radians(v)*.5 for v in euler)
    sx,sy,sz=math.sin(x),math.sin(y),math.sin(z)
    cx,cy,cz=math.cos(x),math.cos(y),math.cos(z)
    # qY * qX * qZ: Unity applies Z, X, then Y.
    return (cy*sx*cz+sy*cx*sz,sy*cx*cz-cy*sx*sz,cy*cx*sz-sy*sx*cz,cy*cx*cz+sy*sx*sz)


def _slerp(a,b,u):
    a,b=_unit(a),_unit(b)
    dot=sum(x*y for x,y in zip(a,b))
    if dot < 0: b=tuple(-v for v in b);dot=-dot
    if dot > .9995: return _unit(tuple(x+(y-x)*u for x,y in zip(a,b)))
    angle=math.acos(min(1.,dot));den=math.sin(angle)
    return tuple((math.sin((1-u)*angle)*x+math.sin(u*angle)*y)/den for x,y in zip(a,b))


def _error(a,b,rotation):
    if rotation:
        dot=abs(sum(x*y for x,y in zip(_unit(a),_unit(b))))
        return math.degrees(2*math.acos(min(1.,dot)))
    return max(abs(x-y) for x,y in zip(a,b))


def _f32(value): return struct.unpack('<f',struct.pack('<f',value))[0]


def bake_track(track: AnimationTrack, transform=None, rotation=None, evaluate=None):
    """Bake finite glTF samples, testing the actual float32 values and times."""
    attribute=track.binding.get('attribute')
    if rotation is None: rotation=attribute in (2,4)
    if transform is None:
        transform=(lambda v: convert_rotation(_unit(_rotation(v) if attribute == 4 else v))) if rotation else convert_point if attribute == 1 else lambda v:v
    def native(t): return evaluate(t) if evaluate else transform(sample_track(track,t))
    def value(t): return tuple(_f32(v) for v in native(t))
    limit=.01 if rotation else .00001
    step=track.interpolation in ('STEP','CONSTANT')
    if track.interpolation == 'CUBIC':
        steps=[any(math.isinf(v) for v in row) for row in track.out_tangents[:-1]]
        if any(steps):
            # glTF has one interpolation mode per sampler. Constant components
            # may share a STEP sampler; continuous components cannot.
            for i in range(len(track.times)-1):
                for c in range(len(track.values[i])):
                    if not math.isinf(track.out_tangents[i][c]) and not (track.values[i][c] == track.values[i+1][c] and track.out_tangents[i][c] == track.in_tangents[i+1][c] == 0):
                        raise ValueError('Mixed discrete and continuous glTF channel')
            step=True
    times=sorted(set(_f32(t) for t in track.times))
    if len(times)!=len(track.times) or times[0]<0: raise ValueError('Unrepresentable glTF key times')
    maximum=0.
    for t in times:
        error=_error(value(t),native(t),rotation)
        if error>limit: raise ValueError('Float32 animation value exceeds sample error limit')
        maximum=max(maximum,error)
    if step: return tuple(times),tuple(value(t) for t in times),'STEP',maximum
    result=[times[0]]
    def interval(a,b,depth):
        nonlocal maximum
        va,vb=value(a),value(b)
        checks=[]
        for u in (.25,.5,.75):
            t=a+(b-a)*u
            sample=_slerp(va,vb,u) if rotation else tuple(x+(y-x)*u for x,y in zip(va,vb))
            checks.append(_error(sample,native(t),rotation))
        if max(checks)<=limit:
            maximum=max(maximum,*checks);result.append(b);return
        mid=_f32((a+b)*.5)
        if depth>=24 or not a<mid<b: raise ValueError('Animation subdivision cannot meet sample error limit')
        interval(a,mid,depth+1);interval(mid,b,depth+1)
    for a,b in zip(times,times[1:]): interval(a,b,0)
    values=[value(t) for t in result]
    if rotation:
        for i in range(1,len(values)):
            if sum(a*b for a,b in zip(values[i-1],values[i]))<0:values[i]=tuple(-v for v in values[i])
    return tuple(result),tuple(values),'LINEAR',maximum


def _identity(reader):
    return ObjectId(**source_id(reader))


def _contexts(source, catalog):
    """Read exact controller clip slots and Animator ownership without global paths."""
    controllers, contexts = {}, []
    for identity, record in sorted(catalog.objects.items()):
        if record.type != 'AnimatorController': continue
        reader=source.get(identity.file,identity.path_id)
        data=source.read_record(reader)
        slots=[]
        for pointer in data['m_AnimationClips']:
            target=source.resolve(reader,pointer)
            if target is not None and target.type.name != 'AnimationClip': raise ValueError('Controller clip slot has wrong type')
            slots.append(_identity(target) if target else None)
        controllers[identity]=slots
    for identity, record in sorted(catalog.objects.items()):
        if record.type not in ('Animator','Animation'): continue
        reader=source.get(identity.file,identity.path_id)
        data=source.read_record(reader)
        go=source.resolve(reader,data['m_GameObject'])
        root=source.transform_for(go)
        avatar=source.resolve(reader,data['m_Avatar']) if record.type == 'Animator' else None
        controller=source.resolve(reader,data['m_Controller']) if record.type == 'Animator' else None
        if controller and _identity(controller) not in controllers:
            raise ValueError(f'Unsupported controller type: {source_id(controller)}')
        clips=controllers[_identity(controller)] if controller else []
        if record.type == 'Animation':
            clips=[_identity(target) for pointer in [data['m_Animation'],*data['m_Animations']]
                   if (target:=source.resolve(reader,pointer))]
        contexts.append({'owner':asdict(identity),'owner_type':record.type,'game_object':source_id(go),
                         'root_transform':source_id(root),'avatar':source_id(avatar) if avatar else None,
                         'controller':source_id(controller) if controller else None,
                         'clips':[asdict(c) for c in clips if c], 'typed_record':typed_record(identity)})
    _dynamic_assignments(source,catalog,controllers,contexts)
    return controllers,contexts


def _context_paths(source, context):
    import zlib
    root=source.get(context['root_transform']['file'],context['root_transform']['path_id'])
    paths={}
    by_path={}
    for item in source.hierarchy(root):
        path=item['path'].partition('/')[2]
        identity=_identity(item['reader'])
        paths.setdefault(zlib.crc32(path.encode('utf-8')),[]).append((path,identity))
        by_path.setdefault(path,[]).append(identity)
    if context['avatar']:
        avatar=source.get(context['avatar']['file'],context['avatar']['path_id'])
        for checksum,path in source.read_record(avatar)['m_TOS']:
            for identity in by_path.get(path,[]):
                paths.setdefault(checksum,[]).append((path,identity))
    return paths


def _model_for_context(context, model_nodes):
    identity=ObjectId(**context['root_transform'])
    candidates=model_nodes.get(identity,[])
    if len(candidates) != 1:
        raise ValueError(f'Missing or ambiguous model for Animator root: {identity}')
    return candidates[0]


def _write_animation_package(source, output, record, context, model, bound):
    from .models import animation_model_document
    from ..gltf import GltfBuilder
    identity=record.source
    owner=ObjectId(**context['owner'])
    name=f'animations/clips/{identity.file}/{identity.path_id}/rigs/{owner.file}/{owner.path_id}/animation.gltf'
    path=output/name
    writer=GltfBuilder(source,path.parent)
    writer.document=animation_model_document(model,output,path.parent)
    gl=writer.gl
    mapping={ObjectId(**node.extras['source']):index for index,node in enumerate(writer.document.nodes)}
    animation=gl.Animation(name=record.settings['m_Name'],extras={'source':asdict(identity),'owner':context,'model':asdict(model.source)})
    buffer_index=len(writer.document.buffers)
    checks=[]
    occupied=set()
    morph_groups={}
    for track_index,target in bound:
        if record.tracks[track_index].binding['type_id']==137:
            morph_groups.setdefault(target,[]).append(track_index)
            continue
        track=record.tracks[track_index]
        node_index=mapping[target]
        attribute=track.binding['attribute']
        channel_path={1:'translation',2:'rotation',3:'scale',4:'rotation'}[attribute]
        if (node_index,channel_path) in occupied: raise ValueError('Duplicate glTF animation target property')
        occupied.add((node_index,channel_path))
        times,values,interpolation,error=bake_track(track)
        sampler=len(animation.samplers)
        animation.samplers.append(gl.AnimationSampler(
            input=writer.accessor(times,'SCALAR',target=None,buffer_index=buffer_index),
            output=writer.accessor(values,'VEC4' if channel_path=='rotation' else 'VEC3',target=None,buffer_index=buffer_index),
            interpolation=interpolation))
        animation.channels.append(gl.AnimationChannel(sampler=sampler,
            target=gl.AnimationChannelTarget(node=node_index,path=channel_path),
            extras={'track':track_index,'source_target':asdict(target)}))
        checks.append({'track':track_index,'source_target':asdict(target),'path':channel_path,
                       'keys':len(times),'interpolation':interpolation,'max_error':error,
                       'error_unit':'degrees' if channel_path=='rotation' else 'source_units'})
    for target,indices in morph_groups.items():
        node_index=mapping[target]
        node=writer.document.nodes[node_index]
        times,values,interpolation,error=_bake_morph(source,record,indices,node,writer.document)
        sampler=len(animation.samplers)
        animation.samplers.append(gl.AnimationSampler(
            input=writer.accessor(times,'SCALAR',target=None,buffer_index=buffer_index),
            output=writer.accessor([v for row in values for v in row],'SCALAR',target=None,buffer_index=buffer_index),
            interpolation=interpolation))
        animation.channels.append(gl.AnimationChannel(sampler=sampler,
            target=gl.AnimationChannelTarget(node=node_index,path='weights'),
            extras={'tracks':indices,'source_target':asdict(target)}))
        checks.append({'tracks':indices,'source_target':asdict(target),'path':'weights','keys':len(times),
                       'interpolation':interpolation,'max_error':error,'error_unit':'weight'})
    writer.document.animations=[animation]
    writer.document.buffers.append(gl.Buffer(uri='animation.bin',byteLength=len(writer.binary)))
    writer.document.extras={'source':asdict(identity),'model':asdict(model.source),'owner':context['owner'],
                            'geometry_storage':'shared_external_buffers'}
    binary=_file(output,path.with_name('animation.bin').relative_to(output).as_posix(),bytes(writer.binary))
    document=_file(output,name,writer.document.to_json().encode('utf-8'))
    return (document,binary),{'owner':context['owner'],'model':asdict(model.source),'file':name,'channels':checks}


def export_animations(source: Source, catalog: Catalog, models: tuple[AssetExport, ...], output: Path) -> tuple[AssetExport, ...]:
    controllers,contexts=_contexts(source,catalog)
    model_nodes={}
    for model in models:
        if model.details.get('type') != 'Hierarchy': continue
        for node in model.details['nodes']:
            model_nodes.setdefault(ObjectId(**node),[]).append(model)
    exports=[]
    path_cache={}
    objects=sorted((i,r) for i,r in catalog.objects.items() if r.type in ('AnimationClip','Avatar','AnimatorController'))
    for identity,obj_record in objects:
        reader=source.get(identity.file,identity.path_id)
        details={'type':obj_record.type,'typed_record':typed_record(identity)}
        dependencies=set(catalog.dependencies[identity])
        if obj_record.type != 'AnimationClip':
            data=source.read_record(reader)
            details['name']=data['m_Name']
            details['owners']=[c for c in contexts if c['avatar' if obj_record.type=='Avatar' else 'controller']==asdict(identity)]
            if obj_record.type=='AnimatorController':
                details['clip_slots']=[asdict(c) if c else None for c in controllers[identity]]
                details['state_clip_links']=_state_clip_links(data['m_Controller'],controllers[identity])
            else:
                details['path_table']=[{'hash':h,'path':p} for h,p in data['m_TOS']]
                details['path_count']=len(data['m_TOS'])
            for c in details['owners']:dependencies.add(ObjectId(**c['owner']))
            exports.append(AssetExport(identity,'animations','native_record','engine_dependent',
                'Native rig and state data require Unity runtime adapters.',(),tuple(sorted(dependencies)),details))
            continue
        record=decode_clip(source,reader,catalog)
        clip_owners=[c for c in contexts if asdict(identity) in c['clips']]
        clip_controllers=[asdict(c) for c,slots in controllers.items() if identity in slots]
        details.update({'name':record.settings['m_Name'],'track_count':len(record.tracks),
                        'curve_counts':record.settings['curve_counts'],'event_count':len(record.events),
                        'owners':clip_owners,'controllers':clip_controllers,'packages':[]})
        files=[]
        generic=[(i,t) for i,t in enumerate(record.tracks) if _gltf_binding(t.binding)]
        if generic and not clip_owners: raise ValueError(f'Generic clip has no owning hierarchy: {identity}')
        for context in clip_owners:
            owner=ObjectId(**context['owner'])
            dependencies.add(owner)
            for assignment in context.get('controller_assignments',[]):
                dependencies.add(ObjectId(**assignment['handler']))
                dependencies.add(ObjectId(**assignment['controller']))
                dependencies.update(ObjectId(**d) for d in assignment['declarations'])
            for name in ('avatar','controller','root_transform'):
                if context[name]:dependencies.add(ObjectId(**context[name]))
            bound=[]
            if generic:
                if owner not in path_cache:path_cache[owner]=_context_paths(source,context)
                model=_model_for_context(context,model_nodes)
                dependencies.add(model.source)
                for index,track in generic:
                    path,target=resolve_path(track.binding['path_hash'],path_cache[owner])
                    track.binding.setdefault('targets',[]).append({'owner':asdict(owner),'path':path,'source':asdict(target),'model':asdict(model.source)})
                    dependencies.add(target);bound.append((index,target))
                package_files,package_details=_write_animation_package(source,output,record,context,model,bound)
                files.extend(package_files);details['packages'].append(package_details)
            for track in record.tracks:
                if track.binding['type_id']==95:
                    track.binding.setdefault('targets',[]).append({'owner':asdict(owner),'source':asdict(owner),
                        'avatar':context['avatar'],'property':track.binding['attribute']})
        dependencies.update(ObjectId(**c) for c in clip_controllers)
        name=f'animations/clips/{identity.file}/{identity.path_id}/tracks.json'
        normalized=_native(asdict(record),source,reader,catalog)
        files.insert(0,_file(output,name,_json(normalized)))
        details['tracks_file']=name
        native_count=sum(not _gltf_binding(t.binding) for t in record.tracks)
        details['native_track_count']=native_count
        exports.append(AssetExport(identity,'animations','converted' if generic else 'native_record',
            'engine_dependent' if native_count else 'unverified',
            'Normalized native tracks require Unity runtime adapters.' if native_count else '',
            tuple(files),tuple(sorted(dependencies)),details))
        print(f'Exported animation: {identity.file}:{identity.path_id}; {len(record.tracks)} tracks; {len(details["packages"])} rigs',flush=True)
        del record,normalized,generic
        track=None
    ownership=_file(output,'animations/ownership.json',_json(contexts))
    _file(output,'animations/index.json',_json({'schema_version':1,'ownership':asdict(ownership),'exports':[asdict(e) for e in exports]}))
    return tuple(exports)


def _state_clip_links(value, slots, path='m_Controller'):
    links=[]
    if isinstance(value,dict):
        if 'm_ClipID' in value and not value.get('m_ChildIndices'):
            index=value['m_ClipID']
            if index != 4294967295:
                if not 0<=index<len(slots) or slots[index] is None:
                    raise ValueError(f'Missing controller clip slot: {path}: {index}')
                links.append({'path':path+'/m_ClipID','slot':index,'clip':asdict(slots[index])})
        for field,item in value.items():links.extend(_state_clip_links(item,slots,path+'/'+field))
    elif isinstance(value,list):
        for index,item in enumerate(value):links.extend(_state_clip_links(item,slots,path+'/'+str(index)))
    return links


def _gltf_binding(binding):
    return binding['type_id']==4 or (binding['type_id']==137 and binding['custom_type']==20)


def _bake_morph(source, record, indices, node, document):
    from .models import _morph_weights
    if node.mesh is None: raise ValueError('Morph target has no mesh')
    mesh=document.meshes[node.mesh]
    morph=mesh.extras['morph']
    renderer_links=[c for c in node.extras['renderers'] if source.get(c['source']['file'],c['source']['path_id']).type.name=='SkinnedMeshRenderer']
    if len(renderer_links)!=1: raise ValueError('Missing or ambiguous morph renderer')
    rid=renderer_links[0]['source']
    renderer=source.read_record(source.get(rid['file'],rid['path_id']))
    authored=renderer.get('m_BlendShapeWeights',[]) or [0.]*len(morph['channels'])
    selected={}
    tracks=[record.tracks[i] for i in indices]
    for track in tracks:
        matches=[i for i,c in enumerate(morph['channels']) if c['nameHash']==track.binding['attribute']]
        if len(matches)!=1: raise ValueError('Missing or ambiguous morph attribute')
        if matches[0] in selected: raise ValueError('Duplicate morph attribute')
        selected[matches[0]]=track
    times=tuple(sorted({t for track in tracks for t in track.times}))
    modes=[bake_track(t,transform=lambda v:v,rotation=False)[2] for t in tracks]
    if len(set(modes))>1 and any(mode=='STEP' and t.interpolation!='CONSTANT' for mode,t in zip(modes,tracks)):
        raise ValueError('Mixed discrete and continuous morph channel')
    mode='STEP' if all(m=='STEP' for m in modes) else 'LINEAR'
    def evaluate(time):
        values=list(authored)
        for index,track in selected.items():values[index]=sample_track(track,time)[0]
        return tuple(_morph_weights(mesh,{'m_BlendShapeWeights':values}))
    merged=AnimationTrack({},times,tuple(evaluate(t) for t in times),mode)
    return bake_track(merged,transform=lambda v:v,rotation=False,evaluate=evaluate)


DYNAMIC_ASSIGNMENT_EVIDENCE = {
    'assembly': 'TheLongDrive_Data/Managed/Assembly-CSharp.dll',
    'sha256': '9919a9eeddcce23bb4ff86e890437f32b4a3ef3c10af2efe3b8d889d894dd62e',
    'decompiler': 'ICSharpCode.Decompiler 11.1.0.9782',
    'method': 'weaponhandlingscript.Update',
    'target_field': 'weaponhandlingscript.anim.runtimeAnimatorController',
    'controller_field': 'meleeweaponscript.AnimationController',
    'selection': 'player.inHandP.melee; runtime selection, not initial state',
}


def _dynamic_assignments(source,catalog,controllers,contexts):
    """Link the pinned build's stored hand targets and runtime controller declarations."""
    scripts={}
    for identity,record in sorted(catalog.objects.items()):
        if record.type!='MonoScript':continue
        data=source.read_record(source.get(identity.file,identity.path_id))
        if data['m_ClassName'] in ('weaponhandlingscript','meleeweaponscript'):
            scripts[data['m_ClassName']]=identity
    if not scripts:return
    if len(scripts)!=2:raise ValueError('Incomplete native weapon assignment types')
    import hashlib
    assembly=source.root/DYNAMIC_ASSIGNMENT_EVIDENCE['assembly']
    if hashlib.sha256(assembly.read_bytes()).hexdigest()!=DYNAMIC_ASSIGNMENT_EVIDENCE['sha256']:
        raise ValueError('Native controller assignment evidence does not match assembly')
    declarations={};handlers=[]
    for identity,record in sorted(catalog.objects.items()):
        if record.type!='MonoBehaviour':continue
        deps=catalog.dependencies[identity]
        weapon=scripts['meleeweaponscript'] in deps
        handler=scripts['weaponhandlingscript'] in deps
        if not (weapon or handler):continue
        reader=source.get(identity.file,identity.path_id);data=source.read_record(reader)
        if weapon:
            controller=source.resolve(reader,data['AnimationController'])
            if controller:
                cid=_identity(controller)
                if cid not in controllers:raise ValueError('Native weapon declaration has missing controller')
                declarations.setdefault(cid,[]).append(identity)
        else:
            target=source.resolve(reader,data['anim'])
            if target is None or target.type.name!='Animator':raise ValueError('Native weapon handler has missing Animator')
            handlers.append((identity,_identity(target),source.resolve(reader,data['player'])))
    for handler,animator,player in handlers:
        matches=[c for c in contexts if c['owner']==asdict(animator)]
        if len(matches)!=1:raise ValueError('Native hand Animator has no unique context')
        context=matches[0]
        for controller,owners in sorted(declarations.items()):
            context.setdefault('controller_assignments',[]).append({
                'handler':asdict(handler),'anim_field':'anim','player':source_id(player) if player else None,
                'controller':asdict(controller),'declarations':[asdict(d) for d in owners],
                'declaration_field':'AnimationController','evidence':DYNAMIC_ASSIGNMENT_EVIDENCE})
            context['clips']=sorted({ObjectId(**i) for i in context['clips']}|{c for c in controllers[controller] if c})
            context['clips']=[asdict(i) for i in context['clips']]
