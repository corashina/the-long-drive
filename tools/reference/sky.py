"""Recover the sky traffic spawner and its prefabs from checked records."""
import math
from .source import source_id,key,vector
from .survival import managed_evidence,finite_record,component
from .small_items import animation_curve
from .acquisition_motion import motion_clips
from .particles import particle,validate_particle
from .validation import record,array,number,u32,optional,string,boolean,source as source_shape
from .weather import game_object
from .world_biome_materials import scene_component
from .collision import world_pose,geometry

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
# fedospawnscript on the active Player. It spawns the sky_ufo prefab at night.
UFO_SPAWNER=('level1',68413)
# Ground UFO rules that only exist in fedospawnscript and fedoscript code.
UFO_EVIDENCE=['T.position = transform.position + new Vector3(UnityEngine.Random.Range(0f - leftRightDist, leftRightDist), distUp, 0f - UnityEngine.Random.Range(minDistBack, maxDistBack));',
    'tempVector = mainscript.M.terrainManager.generator.roads[0].OneCoord(transform.position + transform.forward * 50f);',
    'currentSpeed = Mathf.Clamp(speedOfTarget, speed, highspeed) * Mathf.Lerp(aiMinMultiplier, aiMaxMultiplier, Mathf.InverseLerp(aiMinDist, aiMaxDist, distToTarget));',
    'tempSpeed = speedOfTarget * Mathf.Lerp(aiMinMultiplier, aiMaxMultiplier, Mathf.InverseLerp(aiMinDist, aiMaxDist, distToTarget));',
    'currentSpeed = Mathf.Clamp(currentSpeed, speed, highspeed);']

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

def seated(source,records,nodes):
    """fedoscript.seatedObj node. Start sets it active only while the seat is in use."""
    found=[c for n in records for c in n['components'] if c.type.name=='MonoBehaviour' and source.class_name(c)=='fedoscript']
    if not found:return None
    go=source.resolve(found[0],source.read(found[0])['seatedObj'])
    if go is None or key(source.transform_for(go)) not in nodes:raise ValueError('Sky seated node missing')
    return nodes[key(source.transform_for(go))]['id']

def emp(source,records):
    found=[c for n in records for c in n['components'] if c.type.name=='MonoBehaviour' and source.class_name(c)=='empscript']
    if len(found)>1:raise ValueError('Sky prefab has several empscript')
    if not found:return None
    d=source.read(found[0])
    return {'affect_ambient_sound':bool(d['affectAmbientSound']),'make_rabbits_evil':bool(d['makeRabbitsEvil']),
        'range':d['range'],'value':d['value'],'range_spread':d['rangeSpread'],'range_speed':d['rangeSpeed'],
        'value_spread':d['valueSpread'],'value_speed':d['valueSpeed'],'fall_off':animation_curve(d['fallOff']),'layers':d['lm']['m_Bits']}

def recover_ground_ufo(source, prefabs):
    """fedospawnscript on the Player and the fedoscript AI values of its prefab."""
    spawner=source.get(*UFO_SPAWNER)
    if source.class_name(spawner)!='fedospawnscript' or not source.read(game_object(source,spawner))['m_IsActive']:
        raise ValueError('UFO spawner is not the active Player fedospawnscript')
    d=source.read(spawner);go=source.resolve(spawner,d['prefab'])
    index=next((i for i,root in enumerate(prefabs) if key(game_object(source,root))==key(go)),None)
    if index is None:raise ValueError('UFO prefab is not a sky prefab')
    fedo=component(source,prefabs[index],'fedoscript');f=source.read(fedo)
    return {'source':source_id(spawner),'prefab':index,'night_range':[d['minNight'],d['maxNight']],'nights_after_spawn':d['nightsAfterSpawn'],
        'chance':d['chance'],'back_range':[d['minDistBack'],d['maxDistBack']],'side_distance':d['leftRightDist'],'up_distance':d['distUp'],
        'fedo':source_id(fedo),'speed':f['speed'],'high_speed':f['highspeed'],'target_distance':f['targetDist'],'road_height_offset':f['roadHeightOffset'],
        'ai_distance_range':[f['aiMinDist'],f['aiMaxDist']],'ai_multiplier_range':[f['aiMinMultiplier'],f['aiMaxMultiplier']],
        'visited_distance':f['aiVisitedDistance'],'disappear_distance':f['aiDisappearDistance'],
        **ufo_body(source,prefabs[index],fedo,f),'evidence':managed_evidence(source,UFO_EVIDENCE)}

def ufo_body(source,root,fedo,f):
    """Fedo01 Rigidbody, colliders, seatscript and stimulscript. Positions are root space."""
    records=source.hierarchy(root,activate_root=True)
    rigid=[c for n in records for c in n['components'] if c.type.name=='Rigidbody']
    if len(rigid)!=1 or key(rigid[0])!=key(source.resolve(fedo,f['RB'])) or key(source.transform_for(source.resolve(rigid[0],source.read(rigid[0])['m_GameObject'])))!=key(root):
        raise ValueError('UFO Rigidbody is not the one root body')
    r=source.read(rigid[0])
    # Constraints 112 freeze all rotation. The port body keeps an identity rotation.
    if r['m_IsKinematic'] or r['m_UseGravity'] or r['m_Constraints']!=112:raise ValueError('UFO Rigidbody differs')
    root_pose=world_pose(source,root)
    matrix=source.read(next(o for o in source.objects.values() if o.type.name=='PhysicsManager'))['m_LayerCollisionMatrix']
    rot=source.resolve(fedo,f['rot']);seated_go=source.resolve(fedo,f['seatedObj'])
    rd=source.read(rot)
    if any(vector(rd['m_LocalPosition'])) or key(source.resolve(rot,rd['m_Father']))!=key(root):
        raise ValueError('UFO rotor is not at the root')
    seat=source.resolve(fedo,f['seat'])
    usables={key(source.resolve(fedo,f[n])):role for n,role in (('enableFollowRoad','auto_pilot'),('setHighSpeed','speed'),('setAltitude','altitude'))}
    parents={key(n['reader']):key(n['parent']) if n['parent'] is not None else None for n in records}
    def under(reader,top):
        node=key(reader)
        while node is not None:
            if node==key(top):return True
            node=parents[node]
        return False
    colliders=[];stimulus=None
    for n in records:
        if not n['active']:raise ValueError('UFO has an inactive node')
        layer=source.read(n['game_object'])['m_Layer']
        group={(source.class_name(c) if c.type.name=='MonoBehaviour' else c.type.name):c for c in n['components']}
        for c in n['components']:
            if c.type.name not in ('BoxCollider','SphereCollider','CapsuleCollider'):continue
            d=source.read(c)
            if d['m_IsTrigger'] or not d['m_Enabled'] or source.resolve(c,d['m_Material']) is not None:raise ValueError('UFO collider state differs')
            role='seat' if 'seatscript' in group and key(group['seatscript'])==key(seat) else usables.get(key(group['usablescript']),'body') if 'usablescript' in group else 'body'
            colliders.append({'source':source_id(c),'role':role,'spin':under(n['reader'],rot),'seated':under(n['reader'],source.transform_for(seated_go)),
                'membership':1<<layer,'filter':matrix[layer],'geometry':geometry(source,None,c,n['reader'],root_pose,True)})
            if 'stimulscript' in group:
                if stimulus is not None or c.type.name!='SphereCollider':raise ValueError('UFO stimulus differs')
                g=colliders[-1]['geometry'];t=source.read(group['stimulscript'])
                if not t['visible'] or t['smell'] or t['audioSources']:raise ValueError('UFO stimulus differs')
                stimulus={'source':source_id(group['stimulscript']),'center':g['center'],'radius':g['radius']}
    if stimulus is None:raise ValueError('UFO stimulus missing')
    d=source.read(seat)
    for link in ('LHpos','RHpos','LLpos','RLpos','mainseat'):
        if source.resolve(seat,d[link]) is not None:raise ValueError('UFO seat link is not audited')
    if d['BcantSitAngle']:raise ValueError('UFO seat angle is not audited')
    seat_t=source.transform_for(source.resolve(seat,d['m_GameObject']));sit=source.resolve(seat,d['sitPos']) or seat_t
    return {'mass':r['m_Mass'],'speed_lerp':f['speedlerp'],'colliders':colliders,'stimulus':stimulus,
        'seat':{'source':source_id(seat),'node':world_pose(source,seat_t)['translation'],'sit':world_pose(source,sit)['translation'],
            'rot_limit':d['rotLimit'],'yaw_unlimited':bool(d['limitLessRot']),'look_instead_of_lean':bool(d['lookInsteadOfLean']),
            'root_origin':bool(d['rootOrigin']),'default_pitch':d['defYrot'],'min_exit_angle':d['minGetUpAnlge'],'exit_height':d['getOutHeight'],
            'show_sleep_text':bool(d['showSleepText']),'cant_push_from_seat':bool(d['cantPushFromSeat']),
            'push_force_modifier':d['pushForce'] if d['usePushForceModifier'] else 1.,'def_pose':d['defPose']}}

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
            'spin':spin(source,records,nodes),'seated':seated(source,records,nodes),'emp':emp(source,records)})
    result['ground_ufo']=recover_ground_ufo(source,prefabs(source,reader))
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
            'spin':optional(lambda s:record(s,{'node':u32,'axis':array(number,3),'degrees_per_second':number})),'seated':optional(u32),'emp':optional(emp_shape)})
    def key_shape(k):
        record(k,{'time':number,'value':number,'in_tangent':number,'out_tangent':number,'in_weight':number,'out_weight':number,'weighted_mode':u32})
    def emp_shape(e):
        record(e,{'affect_ambient_sound':boolean,'make_rabbits_evil':boolean,'range':number,'value':number,
            'range_spread':number,'range_speed':number,'value_spread':number,'value_speed':number,
            'fall_off':lambda c:record(c,{'keys':array(key_shape),'pre_wrap_mode':number,'post_wrap_mode':number}),'layers':u32})
    def ufo_shape(u):
        record(u,{'source':source_shape,'prefab':u32,'night_range':array(number,2),'nights_after_spawn':number,'chance':number,
            'back_range':array(number,2),'side_distance':number,'up_distance':number,'fedo':source_shape,'speed':number,'high_speed':number,
            'target_distance':number,'road_height_offset':number,'ai_distance_range':array(number,2),'ai_multiplier_range':array(number,2),
            'visited_distance':number,'disappear_distance':number,'mass':number,'speed_lerp':number,
            'colliders':array(lambda c:record(c,{'source':source_shape,'role':string,'spin':boolean,'seated':boolean,'membership':u32,'filter':u32,'geometry':lambda g:None})),
            'seat':lambda t:record(t,{'source':source_shape,'node':array(number,3),'sit':array(number,3),'rot_limit':number,'yaw_unlimited':boolean,
                'look_instead_of_lean':boolean,'root_origin':boolean,'default_pitch':number,'min_exit_angle':number,'exit_height':number,
                'show_sleep_text':boolean,'cant_push_from_seat':boolean,'push_force_modifier':number,'def_pose':u32}),
            'stimulus':lambda t:record(t,{'source':source_shape,'center':array(number,3),'radius':number}),
            'evidence':lambda e:record(e,{'assembly_sha256':string,'methods':array(string)})})
    record(sky,{'source':source_shape,'chances':array(number),'max_planes':u32,'spawn_time_range':array(number,2),
        'spawn_distance':number,'height':number,'speed':number,'max_angle':number,'destroy_distance':number,'contrail_rate':number,
        'prefabs':array(prefab,len(PREFABS)),'evidence':lambda e:record(e,{'assembly_sha256':string,'methods':array(string)}),'ground_ufo':ufo_shape,'flyer':lambda f:None})
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
    u=sky['ground_ufo']
    if (u['prefab']>=len(sky['prefabs']) or sky['prefabs'][u['prefab']]['emp'] is None or not 0<=u['night_range'][0]<=u['night_range'][1]
            or not 0<=u['chance']<=1 or not 0<=u['back_range'][0]<=u['back_range'][1] or not 0<u['speed']<=u['high_speed']
            or sky['prefabs'][u['prefab']]['seated'] is None or u['mass']<=0
            or {c['role'] for c in u['colliders']}!={'body','seat','auto_pilot','speed','altitude'}
            or any(c['seated']!=(c['role'] in ('auto_pilot','speed','altitude')) for c in u['colliders'])):
        raise ValueError('Invalid ground UFO')
    validate_flyer(sky['flyer'],manifest)
