"""Recover the sky traffic spawner and its prefabs from checked records."""
import math
from .source import source_id,key,vector
from .survival import managed_evidence,finite_record,component
from .acquisition_motion import motion_clips
from .particles import particle,validate_particle
from .validation import record,array,number,u32,optional,string,boolean,source as source_shape
from .weather import game_object
from .world_biome_materials import scene_component

MAIN_OBJECT=('level1',425)
# mainscript.randomFlyerPrefab: the RandomBat GameObject, its Animator controller and its one clip.
FLYER_PREFAB=('sharedassets1.assets',32394)
FLYER_CONTROLLER=('sharedassets1.assets',4511)
FLYER_CLIP=('sharedassets1.assets',3992)
# AirPlaneSpawnScript.AirPlanePrefabs in order: asset name, sharedassets1 GameObject.
PREFABS=(('sky_airplane',6483),('sky_ufo',38330))
# Spawner rules that only exist in AirPlaneSpawnScript.Update code.
SKY_EVIDENCE=['PlaneCustomSpace.position = mainscript.M.player.transform.position + Vector3.up * height;',
    'Vector3 vector = UnityEngine.Random.insideUnitCircle * spawnDistance;',
    'gameObject.transform.Rotate(0f, UnityEngine.Random.Range(0f - maxAngle, maxAngle), 0f);',
    'if (Planes[i] == null || Vector3.Distance(Planes[i].position, PlaneCustomSpace.position) > destroyDistance)',
    'Planes[i].position += Planes[i].forward * speed * Time.deltaTime;']

def spawner(source):
    reader=scene_component(source,source.library,'AirPlaneSpawnScript')
    if key(game_object(source,reader))!=MAIN_OBJECT:raise ValueError('Sky spawner is not on Main')
    return reader

def prefabs(source,reader):
    """Prefab root Transforms in AirPlanePrefabs order."""
    links=[source.resolve(reader,p) for p in source.read(reader)['AirPlanePrefabs']]
    if any(r is None for r in links) or [key(game_object(source,r)) for r in links]!=[('sharedassets1.assets',pid) for _,pid in PREFABS]:
        raise ValueError('Sky prefab links differ')
    return [source.transform_for(game_object(source,r)) for r in links]

def prepare_sky(source, roots):
    # Instantiate(prefab, position, Quaternion.identity, parent) replaces the root pose. The glTF root keeps only its scale.
    source.sky_roots=set()
    for (name,_),root in zip(PREFABS,prefabs(source,spawner(source))):roots[name]=[root];source.sky_roots.add(key(root))

def spin(source,records,nodes):
    found=[c for n in records for c in n['components'] if c.type.name=='MonoBehaviour' and source.class_name(c)=='fedoscript']
    if len(found)>1:raise ValueError('Sky prefab has several fedoscript')
    if not found:return None
    d=source.read(found[0]);rot=source.resolve(found[0],d['rot'])
    if rot is None or key(rot) not in nodes:raise ValueError('Sky spin node missing')
    # A rotation axis converts like a quaternion vector part.
    x,y,z=vector(d['rotaxis'])
    return {'node':nodes[key(rot)]['id'],'axis':[-x,-y,z],'degrees_per_second':d['rotspeed']}

def recover_sky(source, manifest, output):
    reader=spawner(source);d=source.read(reader)
    nodes={(n['source']['file'],n['source']['path_id']):n for n in manifest['nodes']}
    result={'source':source_id(reader),'chances':list(d['chances']),'max_planes':d['maxPlaneNum'],
        'spawn_time_range':[d['minSpawnTime'],d['maxSpawnTime']],'spawn_distance':d['spawnDistance'],'height':d['height'],
        'speed':d['speed'],'max_angle':d['maxAngle'],'destroy_distance':d['destroyDistance'],'contrail_rate':d['rate'],
        'prefabs':[],'evidence':managed_evidence(source,SKY_EVIDENCE)}
    for (name,_),root in zip(PREFABS,prefabs(source,reader)):
        if key(root) not in nodes:raise ValueError(f'Sky prefab not exported: {name}')
        records=source.hierarchy(root,activate_root=True)
        # GetComponentInChildren finds the first active ParticleSystem.
        system=next((c for n in records if n['active'] for c in n['components'] if c.type.name=='ParticleSystem'),None)
        result['prefabs'].append({'source':source_id(root),'asset':nodes[key(root)]['asset'],'root':nodes[key(root)]['id'],
            'contrail':particle(source,system,f'{name}_contrail',d['rate'],output,root) if system else None,
            # recover_audio replaces these AudioSource identities with voice ids.
            'voices':[source_id(c) for n in records for c in n['components'] if c.type.name=='AudioSource'],
            'spin':spin(source,records,nodes)})
    source.lighting['textures']=list(source.lighting_textures.values())
    return result

def recover_flyer(source, manifest):
    """randomFlyer values and the wing clip of the RandomBat world prototype."""
    main=source.get('level1',64919)
    go=source.resolve(main,source.read(main)['randomFlyerPrefab'])
    if go is None or key(go)!=FLYER_PREFAB:raise ValueError('Flyer prefab link differs')
    root=source.transform_for(go)
    prototype=next((p for p in manifest['world']['prototypes'] if p['source']==source_id(root)),None)
    if prototype is None:raise ValueError('Flyer prefab is not a world prototype')
    flyer=component(source,root,'randomFlyer');d=source.read(flyer)
    save=source.read(component(source,root,'tosaveitemscript'))
    animator=component(source,root,'Animator')
    controller=source.resolve(animator,source.read(animator)['m_Controller'])
    clips=[source.resolve(controller,p) for p in source.read(controller)['m_AnimationClips']] if controller else []
    if controller is None or key(controller)!=FLYER_CONTROLLER or [key(c) for c in clips if c]!=[FLYER_CLIP]:
        raise ValueError('Flyer wing clip differs')
    nodes={(n['source']['file'],n['source']['path_id']):i for i,n in enumerate(prototype['nodes'])}
    def bind(b,target):
        # Tracks hold Transform values in the source parent-local space. A quaternion track has 4 values.
        identity=(target['file'],target['path_id'])
        if b['type_id']!=4 or b['dimension']!=(4 if b['attribute']==2 else 3) or b['is_pptr'] or b['script'] is not None or identity not in nodes:
            raise ValueError('Unsupported flyer track binding')
        return nodes[identity]
    wing,=motion_clips(source,[source_id(clips[0])],animator,bind)
    return {'source':source_id(flyer),'prefab':source_id(root),'asset':prototype['asset'],'item_id':save['id'],
        'time_range':[d['minTime'],d['maxTime']],'range':[d['minRange'],d['maxRange']],'max_range_from_start':d['maxRangeFromStart'],
        'speed_range':[d['minSpeed'],d['maxSpeed']],'velocity_lerp':d['velocityLerp'],'rot_lerp':d['rotLerp'],
        'sound_time_range':[d['soundMinTime'],d['soundMaxTime']],'volume_range':[d['soundMinVolume'],d['soundMaxVolume']],
        'pitch_range':[d['soundMinPitch'],d['soundMaxPitch']],
        # recover_audio replaces these AudioClip identities with clip ids and sets the shared one-shot voice.
        'clips':[source_id(source.resolve(flyer,c)) for c in d['soundClips']],'voice':None,'wing':wing}

def validate_flyer(f, manifest):
    prototype=next((p for p in manifest['world']['prototypes'] if p['source']==f['prefab']),None)
    clips={c['id'] for c in manifest['audio']['clips']}
    shots={v['id'] for v in manifest['audio']['car_voices'] if v['one_shot']}
    def track(t):
        width=4 if t.get('attribute')==2 else 3
        record(t,{'node':u32,'attribute':u32,'path_hash':u32,'custom_type':u32,'curve_offset':u32,'storage':string,'interpolation':string,
            'times':array(number),'values':array(array(number,width)),'in_tangents':array(array(number,width)),
            'out_tangents':array(array(number,width)),'coefficients':array(array(array(number,3),width))})
    def clip(w):
        record(w,{'source':source_shape,'source_tracks':lambda r:record(r,{'bytes':u32,'file':string,'sha256':string}),'name':string,
            'start_time':number,'stop_time':number,'sample_rate':number,'loop':boolean,'cycle_offset':number,'tracks':array(track),'events':array(lambda e:None)})
    record(f,{'source':source_shape,'prefab':source_shape,'asset':u32,'item_id':u32,'time_range':array(number,2),'range':array(number,2),
        'max_range_from_start':number,'speed_range':array(number,2),'velocity_lerp':number,'rot_lerp':number,'sound_time_range':array(number,2),
        'volume_range':array(number,2),'pitch_range':array(number,2),'clips':array(u32),'voice':u32,'wing':clip})
    finite_record(f)
    if prototype is None or prototype['asset']!=f['asset']:raise ValueError('Invalid flyer prototype')
    if any(not 0<=r[0]<=r[1] for r in (f['time_range'],f['range'],f['speed_range'],f['sound_time_range'],f['volume_range'],f['pitch_range'])):
        raise ValueError('Invalid flyer range')
    if (min(f['max_range_from_start'],f['velocity_lerp'],f['rot_lerp'])<=0 or not f['clips'] or any(c not in clips for c in f['clips'])
            or f['voice'] not in shots):
        raise ValueError('Invalid flyer values')
    w=f['wing']
    if (w['source']!={'file':FLYER_CLIP[0],'path_id':FLYER_CLIP[1]} or not w['loop'] or not w['start_time']<w['stop_time']
            or not w['tracks'] or w['events']):raise ValueError('Invalid flyer wing clip')
    for t in w['tracks']:
        if (t['node']>=len(prototype['nodes']) or t['attribute'] not in (1,2,3,4) or not t['times'] or any(b<=a for a,b in zip(t['times'],t['times'][1:]))
                or len(t['values'])!=len(t['times']) or len(t['in_tangents'])!=len(t['times']) or len(t['out_tangents'])!=len(t['times'])
                or len(t['coefficients']) not in (0,len(t['times']))):raise ValueError('Invalid flyer wing track')

def validate_sky(sky, manifest):
    textures={t['id'] for t in manifest['lighting']['textures']}
    nodes={n['id']:n for n in manifest['nodes']};assets={a['id']:a for a in manifest['assets']}
    voices={v['id']:v for v in manifest['audio']['voices']};emitters={e['id']:e for e in manifest['audio']['emitters']}
    def prefab(p):
        record(p,{'source':source_shape,'asset':u32,'root':u32,'contrail':optional(lambda c:validate_particle(c,textures)),'voices':array(u32),
            'spin':optional(lambda s:record(s,{'node':u32,'axis':array(number,3),'degrees_per_second':number}))})
    record(sky,{'source':source_shape,'chances':array(number),'max_planes':u32,'spawn_time_range':array(number,2),
        'spawn_distance':number,'height':number,'speed':number,'max_angle':number,'destroy_distance':number,'contrail_rate':number,
        'prefabs':array(prefab,len(PREFABS)),'evidence':lambda e:record(e,{'assembly_sha256':string,'methods':array(string)}),'flyer':lambda f:None})
    finite_record(sky)
    if len(sky['chances'])!=len(sky['prefabs']) or any(c<0 for c in sky['chances']) or sum(sky['chances'])<=0:raise ValueError('Invalid sky chances')
    if sky['max_planes']<=0 or not 0<=sky['spawn_time_range'][0]<=sky['spawn_time_range'][1]:raise ValueError('Invalid sky schedule')
    if (min(sky['spawn_distance'],sky['speed'],sky['destroy_distance'])<=0 or sky['contrail_rate']<0
            or not 0<=sky['max_angle']<=180):raise ValueError('Invalid sky spawner')
    for p in sky['prefabs']:
        asset=assets.get(p['asset'],{});root=nodes.get(p['root'],{})
        if asset.get('usage')!='sky_traffic' or asset.get('root')!=p['source'] or root.get('source')!=p['source'] or root.get('parent') is not None:
            raise ValueError('Invalid sky prefab root')
        if any(v not in voices or nodes.get(emitters[voices[v]['emitter']]['node'],{}).get('asset')!=p['asset'] for v in p['voices']):
            raise ValueError('Invalid sky voice')
        s=p['spin']
        if s and (nodes.get(s['node'],{}).get('asset')!=p['asset'] or math.hypot(*s['axis'])==0):raise ValueError('Invalid sky spin')
    validate_flyer(sky['flyer'],manifest)
