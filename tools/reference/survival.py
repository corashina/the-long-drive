"""Recover the selected player's needs, food prototypes, and sleep contracts."""
import hashlib
import json
import math
import re
import struct
from pathlib import Path
from .source import LibrarySource, key, source_id
from .collision import world_pose, geometry, step_surface, collider_tag
from .gltf import GltfExporter
from .actions import recover_pickup

SURVIVAL_FIELDS = 'waterToPissRatio foodToShitRatio bloodToHpRatio alcoholHeal alcoholConvertToPiss carFluidsDamage maxHealingSpeed healingToiletFullModfiier waterDamage foodDamage defTemp coldTemp hotTemp foodLoss coldFoodLossModifier hotFoodLossModifier waterLoss coldWaterLossModifier hotWaterLossModifier runFoodLossModifier runWaterLossModifier delaySpeed'.split()

def snake(name):
    return re.sub(r'(?<!^)(?=[A-Z])', '_', name).lower()

def f32(value):
    return struct.unpack('<f', struct.pack('<f', value))[0]

def managed_evidence(source, assertions):
    root=source.library.records_root/'managed'
    receipt=json.loads((root/'decompilation.json').read_bytes())
    if not receipt['complete'] or receipt['diagnostics'] or receipt['input']['sha256']!='9919a9eeddcce23bb4ff86e890437f32b4a3ef3c10af2efe3b8d889d894dd62e':
        raise ValueError('Required managed source receipt differs')
    content=(root/'Assembly-CSharp.cs').read_bytes()
    if len(content)!=receipt['output']['bytes'] or hashlib.sha256(content).hexdigest()!=receipt['output']['sha256']:
        raise ValueError('Managed output hash mismatch')
    code=content.decode('utf-8-sig').replace('\r\n','\n')
    for text in assertions:
        if text not in code: raise ValueError(f'Required managed source assertion absent: {text}')
    return {'assembly_sha256':receipt['input']['sha256'], 'methods':assertions}

def component(source, root, name):
    matches=[c for c in source.components(root)[1] if (source.class_name(c) if c.type.name=='MonoBehaviour' else c.type.name)==name]
    if len(matches)!=1:raise ValueError(f'{key(root)} requires one {name}')
    return matches[0]

# itemdatabase fields of every bite-based ediblescript. gszalcigi is the smoked cigarette.
FOOD_FIELDS=(('sausage','gkolbasz'),('bacon','gszalonna'),('bread_roll','gzsemle'),('kifli','gkifli'),('linzer','glinzer'),('chocolate','gcsoki'))

def selected_foods(source):
    database=source.get('level1',66024); db=source.read(database)
    player=source.get('level1',80125)
    return [*((kind,source.transform_for(source.resolve(database,db[field]))) for kind,field in FOOD_FIELDS),
            ('turd',source.transform_for(source.resolve(player,source.read(player)['GTurd'])))]

def part_groups(source, edible):
    data=source.read(edible)
    groups=[v['parts'] for v in data['moreFoodParts']] if data['moreParts'] else [[v] for v in data['parts']]
    return [[source.transform_for(source.resolve(edible,p)) for p in group] for group in groups]

def prepare_food(source, roots):
    roots['food']=[root for _,root in selected_foods(source)]
    source.food_geometry_nodes=set()
    source.food_active={}
    for root in [roots['items'][-1],*roots['food']]:
        edible=component(source,root,'ediblescript');data=source.read(edible)
        for index,group in enumerate(part_groups(source,edible)):
            for part in group:
                for node in source.hierarchy(part,activate_root=True):
                    source.food_geometry_nodes.add(key(node['reader']))
                    source.food_active[key(node['reader'])]=index==data['index']

def food_record(source, kind, root, manifest):
    nodes={(n['source']['file'],n['source']['path_id']):n for n in manifest['nodes']}
    edible=component(source,root,'ediblescript');data=source.read(edible)
    if data['continous'] or not data['destroyOnEnd']:raise ValueError('Selected food consumption contract differs')
    # ediblescript.Start sets a null particlePos to the food root. The runtime emits at the root.
    if data['particlePos']['m_PathID']:raise ValueError(f'{key(edible)}: food particle position is not the root')
    groups=part_groups(source,edible)
    rigid=source.read(component(source,root,'Rigidbody'))
    pose=world_pose(source,root);pose['scale']=[1.,1.,1.]
    exporter=GltfExporter(source,None,source.library)
    physics=next(r for r in source.objects.values() if r.type.name=='PhysicsManager')
    matrix=source.read(physics)['m_LayerCollisionMatrix']
    colliders=[]
    for n in source.hierarchy(root,activate_root=True):
        layer=source.read(n['game_object'])['m_Layer']
        for c in n['components']:
            if c.type.name not in ('BoxCollider','SphereCollider','CapsuleCollider','MeshCollider'):continue
            d=source.read(c)
            colliders.append({'source':source_id(c),'node':nodes[key(n['reader'])]['id'],
                'membership':1<<layer,'filter':matrix[layer],'sensor':bool(d['m_IsTrigger']),
                'enabled':bool(d['m_Enabled']), 'geometry':geometry(source,exporter,c,n['reader'],pose,True),
                'step_surface':step_surface(source,n['game_object']),'tag':collider_tag(source,n['game_object'])})
    return {'source':source_id(edible),'kind':kind,'effect':dict(food=data['foodValue'],water=data['waterValue'],bowel=data['shitValue'],hp=data['hpValue']),
        'bites':len(groups),'index':data['index'],'destroy_on_end':bool(data['destroyOnEnd']),'spawn_if_shot':bool(data['spawnIfShot']),
        'asset':nodes[key(root)]['asset'],'root':nodes[key(root)]['id'],
        'mass_kg':source.read(component(source,root,'massScript'))['mass'],
        'linear_damping':rigid['m_Drag'],'angular_damping':rigid['m_AngularDrag'],
        'pickup':recover_pickup(source,component(source,root,'pickupable')),
        'parts':[[nodes[key(part)]['id'] for part in group] for group in groups],
        'colliders':colliders,'has_particle':bool(data['hasParticle']),'particle_type':data['particleType'],'min_particle':data['minParticle'],'max_particle':data['maxParticle'],
        'sound':data['sound'],'text':data['text']}

def recover_survival(source: LibrarySource, roots: dict, output: Path) -> dict:
    player=source.get('level1',80125);pd=source.read(player)
    owner=source.resolve(player,pd['survival']);d=source.read(owner)
    if key(owner)!=('level1',73239):raise ValueError('Active player survival link differs')
    main=source.get('level1',64919);md=source.read(main)
    outlet=source.resolve(player,pd['piss']);od=source.read(outlet)
    bladder=source.resolve(outlet,od['Tank']);bd=source.read(bladder)['F']
    maximum={name:d[field] for name,field in [('hp','maxHp'),('food','maxFood'),('water','maxWater'),('bowel','maxShit')]}
    initial={name:f32(value*f32(factor)) for (name,value),factor in zip(maximum.items(),[1.,.75,.6,.5])}
    from .manifest import fluid_entries
    quantities=fluid_entries(bd)
    evidence=managed_evidence(source,['water = maxWater * 0.6f;', 'food = maxFood * 0.75f;', 'shit = maxShit * 0.5f;',
        'currentWaterLoss = foodLoss * sleep *', 'piss.valve = Mathf.Lerp(piss.valve, (pissing && !emptiedBladder) ? 1f : 0f, dt * 3f);',
        'survival.shit -= survival.maxShit * 0.3f;', 'sleepEndTime = Time.time + sleepRealTime * 2f;',
        'float num = Mathf.InverseLerp(0f, 14f, Time.time - deathTime);', 'private float foodStatShowTime = 5f;', 'private float foodStatHideTime = 1f;', 'defaultInput[21].keys[0].Set(KeyCode.P);','defaultInput[22].keys[0].Set(KeyCode.B);'])
    result={'source':source_id(owner),'player':source_id(player),'blood':source_id(source.resolve(owner,d['blood'])),
        'initial':initial,'maximum':maximum,'initial_bladder':quantities,'bladder_capacity_liters':bd['maxC'],
        'initial_queue':dict(food=d['toEat'],water=d['toDrink'],bowel=d['toShit'],hp=d['toHp']),
        **{snake(k):d[k] for k in SURVIVAL_FIELDS},'evidence':evidence,
        'drink_modifier':pd['drinkModifier'],'death_velocity_change':pd['deathVelocityChange'],
        'food_stat_show_time':5.,'food_stat_hide_time':1.,
        'toilet':{'source':source_id(outlet),'bladder':source_id(bladder),'radius':od['radius'],'no_angle':bool(od['noAngle']),
            'valve_gain':3.,'empty_amount':.01,'bowel_fraction':.3,'fart_burp_ratio':pd['fartBurpRatio'],'fart_shit_ratio':pd['fartShitRatio'],
            **{snake(k):pd[k] for k in ['minFartPitch','maxFartPitch','minFartVolume','maxFartVolume','minBurpPitch','maxBurpPitch','minBurpVolume','maxBurpVolume']},
            'max_rate':od['maxRate'],'min_rate':od['minRate'],'max_speed':od['maxSpeed'],'force_color':list(od['forceColor'][c] for c in 'rgba'),'force_color_m':od['forceColorM']},
        'inputs':recover_inputs(source),
        'mode_source':source_id(source.get('level0',17179)),'mode':source.read(source.get('level0',17179))['defS']['survival'],
        'death_seconds':14.,'drinking':recover_drinking(source,player)}
    cycle=source.read(source.resolve(main,md['napszak']))
    sleep={'source':source_id(main),'need_modifier':d['sleepModifier'],'hold_gain':md['sleepGain'],'sleep_real_seconds':md['sleepRealTime'],
        'sleep_cycle_seconds':md['sleepTimeInGame'],'cooldown_seconds':md['minTimeBeforeSleep'],
        'sleep_cam_ring_min':md['sleepCamRingMin'],'sleep_cam_ring_max':md['sleepCamRingMax'],
        'sleep_cam_height_min':md['sleepCamHeightMin'],'sleep_cam_height_max':md['sleepCamHeightMax'],'sleep_layer_mask':md['sleepLayerMask']['m_Bits'],
        **{snake(k):cycle[k] for k in ['CloudLowMinSpeed','CloudLowMidSpeed','CloudLowMaxSpeed','CloudHighMinSpeed','CloudHighMidSpeed','CloudHighMaxSpeed']},
        'evidence':managed_evidence(source,['sleep *= sleepModifier;', 'sleepModifierDef = sleepTimeInGame / sleepRealTime * 0.5f;', 'Vector3 vector2 = new Vector3(vector.x, 10000f, vector.y);','player.MoveCamParent(position + Vector3.up * 50f);'])}
    source.survival=result; source.sleep=sleep;source.survival_roots=roots
    return result

def recover_inputs(source):
    managed_evidence(source,[])
    code=(source.library.records_root/'managed/Assembly-CSharp.cs').read_text(encoding='utf-8-sig')
    enum=code[code.index('public enum IN\n'):];enum=enum[enum.index('{')+1:enum.index('}')]
    actions=[s.strip() for s in enum.split(',') if s.strip()]
    result=[]
    for name in ('lmb','piss','burpfart','sleep'):
        index=actions.index(name)
        match=re.search(r'defaultInput\['+str(index)+r'\]\.keys\[0\]\.Set\(KeyCode\.(\w+)\);',code)
        if not match:raise ValueError(f'Missing input default: {name}')
        result.append({'action':name,'index':index,'key':match.group(1)})
    return result

def bind_survival(source,manifest):
    r=source.survival
    r['foods']=[food_record(source,kind,root,manifest) for kind,root in selected_foods(source)]
    starter=source.transform_for(source.get('level1',3542))
    node=next(n for n in manifest['nodes'] if n['source']==source_id(starter))
    r['starter_food']=next(i['id'] for i in manifest['items'] if i['node']==node['id'])
    node_ids={(n['source']['file'],n['source']['path_id']):n['id'] for n in manifest['nodes']}
    r['starter_parts']=[[node_ids[key(p)] for p in group] for group in part_groups(source,component(source,starter,'ediblescript'))]
    terrain=source.get('level1',64898)
    parent=source.resolve(starter,source.read(starter)['m_Father'])
    starts=[source.resolve(terrain,p) for p in source.read(terrain)['StartObjs']]
    if key(parent) not in {key(p) for p in starts}:raise ValueError('Starter food not under StartObjs')
    r['starter_parent']=source_id(parent);r['starter_pose']=world_pose(source,starter)
    r['evidence']['methods'].extend(managed_evidence(source,['StartObjs[i].position += Vector3.up * sp;'])['methods'])
    manifest.update(survival=r,sleep=source.sleep)
    validate_survival_export(r,manifest)

def finite_record(value):
    if isinstance(value,float) and not math.isfinite(value):raise ValueError('Non-finite source value')
    if isinstance(value,dict):
        for child in value.values():finite_record(child)
    elif isinstance(value,list):
        for child in value:finite_record(child)

def validate_survival_export(record: dict, manifest: dict) -> None:
    finite_record(record);finite_record(manifest['sleep'])
    nodes={n['id']:n for n in manifest['nodes']}
    item=next((i for i in manifest['items'] if i['id']==record['starter_food'] and i['kind']=='food'),None)
    starter=nodes.get(item['node']) if item else None
    if not starter or starter['source']!=item['source'] or starter['source']!={'file':'level1','path_id':24573}:raise ValueError('Invalid starter food source')
    if len(record['starter_parts'])!=6 or any(not group or any(nodes.get(n,{}).get('asset')!=starter['asset'] for n in group) for group in record['starter_parts']):raise ValueError('Invalid starter parts')
    if {f['kind'] for f in record['foods']}!={*(k for k,_ in FOOD_FIELDS),'turd'} or len(record['foods'])!=len(FOOD_FIELDS)+1:raise ValueError('Duplicate or missing prototype kind')
    assets={a['id']:a for a in manifest['assets']}
    for food in record['foods']:
        if not 0<=food['particle_type']<5:raise ValueError(f"Food {food['kind']} particle type")
        if not 0<=food['index']<food['bites'] or food['bites']!=len(food['parts']):raise ValueError('Exhausted food index')
        if assets.get(food['asset'],{}).get('usage')!='food_prototype' or nodes.get(food['root'],{}).get('asset')!=food['asset']:raise ValueError('Missing food prototype')
        if food['mass_kg']<=0 or not food['colliders']:raise ValueError('Invalid food geometry')
        if type(food['spawn_if_shot']) is not bool:raise ValueError(f"Food {food['kind']} spawn_if_shot is not bool")
        for group in food['parts']:
            if not group or any(n not in nodes or nodes[n]['asset']!=food['asset'] for n in group):raise ValueError('Missing food part')
    for name in ('hp','food','water','bowel'):
        if not 0<=record['initial'][name]<=record['maximum'][name] or record['maximum'][name]<=0:raise ValueError('Invalid needs')


def recover_drinking(source,player):
    pd=source.read(player);cap=source.resolve(player,pd['drink']);d=source.read(cap)
    root=source.transform_for(source.resolve(cap,d['m_GameObject']))
    collider=source.resolve(player,pd['drinkCol'])
    evidence=managed_evidence(source,['float num = mainscript.M.pourSpeed * t2.valve * Time.deltaTime * t2.flowA;',
        'num *= drinkModifier;', '(item.intersection - item2.transform.position).magnitude < item2.radius * 0.1f',
        't.player.survival.Drink(T.poured * T.Tank.F.GetPercent(mainscript.fluidenum.water));'])
    return {'source':source_id(cap),'tank':source_id(source.resolve(cap,d['Tank'])),
        'collider':source_id(collider),'geometry':geometry(source,GltfExporter(source,None,source.library),collider,root,world_pose(source,root),False),
        'local_position':list(source.read(root)['m_LocalPosition'][axis] for axis in 'xyz'),
        'radius':d['radius'],'valve':d['valve'],'no_angle':bool(d['noAngle']),'no_pour':bool(d['noPour']),
        'is_drink_cap':bool(d['isDrinkCap']),'particle_contact_radius_multiplier':.1,'evidence':evidence}


CIGARETTE_METHODS=[
    'public void UpdParts(bool _updMulti)\n\t{\n\t\tif (continous)\n\t\t{\n\t\t\tfor (int i = 0; i < parts.Length; i++)\n\t\t\t{\n\t\t\t\tparts[i].transform.position = Vector3.Lerp(end[i].position, start[i].position, hp);\n\t\t\t\tparts[i].transform.rotation = Quaternion.Lerp(end[i].rotation, start[i].rotation, hp);\n\t\t\t\tparts[i].transform.localScale = Vector3.Lerp(end[i].localScale, start[i].localScale, hp);\n\t\t\t}\n\t\t}\n\t\telse if (moreParts)',
    'if (!ateOnce)\n\t\t{\n\t\t\thp = 1f;',
    'float num = ((!auto) ? (eatSpeed * Time.deltaTime) : (autoEatSpeed * Time.deltaTime));',
    'hp = Mathf.Clamp(hp - num, 0f, 1f);',
    'if (ember != null && (!auto || emberInAuto))',
    'if (hp <= 0f)\n\t\t\t\t{\n\t\t\t\t\tember.End();',
    'parts[i].transform.position = Vector3.Lerp(end[i].position, start[i].position, hp);',
    'Eat(out var _, out var _, out var _, _playSound: true, auto: true);',
    'survival.Eat(weared[i].edible.Eat(out tempShitValue, out tempHpValue, out tempWaterValue, _playSound: true), tempShitValue, tempHpValue, tempWaterValue);',
    'if (!(weared[num3] == null) && weared[num3].edible != null)\n\t\t\t\t{\n\t\t\t\t\tTakeOff(num3);',
]


def recover_cigarette(source,root,manifest,output):
    from tempfile import TemporaryDirectory
    from .particles import particle
    from .gltf import converted_transform
    from .lighting import rgba
    reader=component(source,root,'ediblescript');d=source.read(reader)
    nodes={(n['source']['file'],n['source']['path_id']):n['id'] for n in manifest['nodes']}
    def node(reader):
        if reader.type.name=='GameObject':reader=source.transform_for(reader)
        elif reader.type.name!='Transform':reader=source.transform_for(source.resolve(reader,source.read(reader)['m_GameObject']))
        return nodes[key(reader)]
    def link(owner,field):return source.resolve(owner,source.read(owner)[field])
    ember=link(reader,'ember');e=source.read(ember);light=link(ember,'feny');ld=source.read(light)
    emitter=link(ember,'ps');pd=source.read(emitter);render=component(source,source.transform_for(source.resolve(emitter,pd['m_GameObject'])),'ParticleSystemRenderer')
    source.lighting_textures=getattr(source,'lighting_textures',{(t['source']['file'],t['source']['path_id']):t for t in manifest['lighting']['textures']})
    if output is None:
        with TemporaryDirectory(prefix='checked-cigarette-') as temp:p=particle(source,emitter,'cigarette',e['startParticle'],Path(temp),root)
    else:p=particle(source,emitter,'cigarette',e['startParticle'],output,root)
    if ld['m_Cookie']['m_PathID']:raise ValueError('Unaudited cigarette light cookie')
    from .lighting import light_payload
    l=light_payload(source,light,node(light),1,e['startLightIntensity'])
    result={'source':source_id(reader),'continuous':bool(d['continous']),'automatic':bool(d['continousAuto']),
        'destroy_on_end':bool(d['destroyOnEnd']),'ate_once':bool(d['ateOnce']),'serialized_hp':d['hp'],'startup_hp':d['hp'] if d['ateOnce'] else 1.,
        'eat_speed':d['eatSpeed'],'auto_eat_speed':d['autoEatSpeed'],'effect':dict(food=d['foodValue'],water=d['waterValue'],bowel=d['shitValue'],hp=d['hpValue']),
        'parts':[node(source.resolve(reader,v)) for v in d['parts']], 'start':[node(source.resolve(reader,v)) for v in d['start']], 'end':[node(source.resolve(reader,v)) for v in d['end']],
        'index':d['index'],'is_smoke':bool(d['isSmoke']),'has_particle':bool(d['hasParticle']),'particle_position':node(link(reader,'particlePos')),
        'particle_type':d['particleType'],'min_particle':d['minParticle'],'max_particle':d['maxParticle'],'sound':d['sound'],'text':d['text'],
        'dont_show_stats':bool(d['dontShowStats']),'ember_in_auto':bool(d['emberInAuto']),
        'ember':{'source':source_id(ember),'renderer':node(link(ember,'r')),'renderer_source':source_id(link(ember,'r')),
            'material':source_id(link(ember,'material')),'light':l,'particle':p,'particle_node':node(emitter),'particle_renderer':source_id(render),
            'colors':[rgba(e[k+'C']) for k in ('start','min','max','end')],
            'light_intensities':[e[k+'LightIntensity'] for k in ('start','min','max','end')],
            'particle_rates':[e[k+'Particle'] for k in ('start','min','max','end')],'speed_up':e['speedUp'],'speed_down':e['speedDown'],'lerp':e['lerp']},
        'evidence':managed_evidence(source,CIGARETTE_METHODS)}
    return result


def validate_cigarette(c,p,m):
    from .validation import record,array,number,u32,i32,boolean,string,source
    from .particles import validate_particle_data
    from .lighting import validate_light_payload
    def effect(v):record(v,dict.fromkeys(('food','water','bowel','hp'),number))
    def evidence(v):record(v,{'assembly_sha256':string,'methods':array(string)})
    def ember(v):
        record(v,{'source':source,'renderer':u32,'renderer_source':source,'material':source,
                  'light':validate_light_payload,'particle':lambda p:validate_particle_data(p,1),'particle_node':u32,
                  'particle_renderer':source,'colors':array(array(number,4),4),'light_intensities':array(number,4),
                  'particle_rates':array(number,4),'speed_up':number,'speed_down':number,'lerp':number})
    record(c,{'source':source,**dict.fromkeys(('continuous','automatic','destroy_on_end','ate_once','is_smoke','has_particle','dont_show_stats','ember_in_auto'),boolean),
              **dict.fromkeys(('serialized_hp','startup_hp','eat_speed','auto_eat_speed'),number),
              **dict.fromkeys(('index','particle_position','particle_type','sound','text'),u32),
              'min_particle':i32,'max_particle':i32,'effect':effect,'parts':array(u32),'start':array(u32),'end':array(u32),
              'ember':ember,'evidence':evidence})
    finite_record(c);sid=lambda i:{'file':'sharedassets1.assets','path_id':i}
    nodes={n['id']:n for n in m['nodes'] if n['asset']==p['asset']}
    if c['source']!=sid(134642) or not c['continuous'] or not c['automatic'] or c['destroy_on_end'] or c['ate_once'] or c['serialized_hp']!=0 or c['startup_hp']!=1 or c['min_particle']!=-100 or c['max_particle']!=1 or c['ember_in_auto']:raise ValueError('Cigarette source consumption')
    for field,expected in [('parts',[78874,87786]),('start',[73923,74031]),('end',[50758,78154])]:
        if [nodes.get(n,{}).get('source') for n in c[field]]!=[sid(i) for i in expected]:raise ValueError('Cigarette part links')
    if nodes.get(c['particle_position'],{}).get('source')!=sid(84276):raise ValueError('Cigarette particle position')
    if any(c[k]!=v for k,v in {'eat_speed': 0.019999999552965164, 'auto_eat_speed': 0.0020833334419876337, 'index': 0, 'is_smoke': 1, 'has_particle': 1, 'particle_type': 3, 'sound': 0, 'text': 324, 'dont_show_stats': 0}.items()) or c['effect']!={'food': -0.00019999999494757503, 'water': -0.00019999999494757503, 'bowel': 0.00019999999494757503, 'hp': 0.0}:raise ValueError('Cigarette exact rates and flags')
    e=c['ember']
    if e['source']!=sid(156940) or e['renderer_source']!=sid(94155) or e['material']!=sid(460) or not any(r['node']==e['renderer'] and r['source']==e['renderer_source'] for r in p['renderers']):raise ValueError('Cigarette ember renderer')
    if not any(r['asset']==p['asset'] and r['source']==e['material'] for r in m['rendering']['materials']):raise ValueError('Cigarette ember material')
    if e['light']['source']!=sid(123875) or nodes.get(e['light']['node'],{}).get('source')!=sid(85874) or e['light']['startup_intensity']!=0 or e['light']['kind']!='point':raise ValueError('Cigarette local light')
    if e['light_intensities']!=[0.,0.10000000149011612,0.25,0.] or e['particle_rates']!=[0.,100.,0.,0.] or e['speed_up']!=1 or e['speed_down']!=1 or e['lerp']!=0:raise ValueError('Cigarette ember source rates')
    particle=e['particle']
    if particle['source']!=sid(128014) or e['particle_renderer']!=sid(128908) or nodes.get(e['particle_node'],{}).get('source')!=sid(87786) or particle['simulation_space']!=1 or particle['role']!='cigarette':raise ValueError('Cigarette local particles')
    if particle['module_flags']!=CIGARETTE_PARTICLE_MODULES:raise ValueError('Cigarette source particle modules')
    if particle['materials'][0]['source']!=sid(60) or particle['materials'][0]['shader']['name']!='Legacy Shaders/Particles/Alpha Blended':raise ValueError('Cigarette particle material')
    if any(sorted(particle[k])!=v for k,v in CIGARETTE_PARTICLE_KEYS.items()) or particle['flags']!=CIGARETTE_PARTICLE_FLAGS or particle['links']!={'m_GameObject':sid(37823)}:raise ValueError('Cigarette required particle data')
    if any(particle['scalars'][k]!=v for k,v in CIGARETTE_PARTICLE_OPERATIONS.items()):raise ValueError('Cigarette source particle operation')
    if any(c[k][w]!=2 for c in particle['curves'].values() for k in ('min_curve','max_curve') for w in ('pre_wrap_mode','post_wrap_mode')):raise ValueError('Cigarette source curve wrap operation')
    if e['light']['cookie'] is not None:raise ValueError('Cigarette light cookie')
    if c['evidence']['assembly_sha256']!='9919a9eeddcce23bb4ff86e890437f32b4a3ef3c10af2efe3b8d889d894dd62e':raise ValueError('Cigarette method assembly')
    textures={t['id'] for t in m['lighting']['textures']}
    if any(t['texture'] not in textures for mat in particle['materials'] for t in mat['textures']):raise ValueError('Cigarette particle texture')
    if c['evidence']['methods']!=CIGARETTE_METHODS:raise ValueError('Cigarette consumption method evidence')
    if any(l['source']==e['light']['source'] for l in m['lighting']['lights']) or any(x['source']==particle['source'] for x in m['particles']):raise ValueError('Cigarette active effects publication')

CIGARETTE_PARTICLE_MODULES={'ClampVelocityModule': False, 'CollisionModule': False, 'ColorBySpeedModule': False, 'ColorModule': True, 'CustomDataModule': False, 'EmissionModule': True, 'ExternalForcesModule': False, 'ForceModule': True, 'InheritVelocityModule': False, 'InitialModule': True, 'LifetimeByEmitterSpeedModule': False, 'LightsModule': False, 'NoiseModule': False, 'RotationBySpeedModule': False, 'RotationModule': False, 'ShapeModule': True, 'SizeBySpeedModule': False, 'SizeModule': True, 'SubModule': False, 'TrailModule': False, 'TriggerModule': False, 'UVModule': False, 'VelocityModule': True}

CIGARETTE_PARTICLE_KEYS={'curves': ['EmissionModule.rateOverDistance', 'EmissionModule.rateOverTime', 'ForceModule.x', 'ForceModule.y', 'ForceModule.z', 'InitialModule.gravityModifier', 'InitialModule.startLifetime', 'InitialModule.startRotation', 'InitialModule.startRotationX', 'InitialModule.startRotationY', 'InitialModule.startSize', 'InitialModule.startSizeY', 'InitialModule.startSizeZ', 'InitialModule.startSpeed', 'ShapeModule.arc.speed', 'ShapeModule.m_MeshSpawn.speed', 'ShapeModule.radius.speed', 'SizeModule.curve', 'SizeModule.y', 'SizeModule.z', 'VelocityModule.orbitalOffsetX', 'VelocityModule.orbitalOffsetY', 'VelocityModule.orbitalOffsetZ', 'VelocityModule.orbitalX', 'VelocityModule.orbitalY', 'VelocityModule.orbitalZ', 'VelocityModule.radial', 'VelocityModule.speedModifier', 'VelocityModule.x', 'VelocityModule.y', 'VelocityModule.z', 'startDelay'], 'colors': ['ColorModule.gradient', 'InitialModule.startColor'], 'vectors': ['InitialModule.customEmitterVelocity', 'ShapeModule.boxThickness', 'ShapeModule.m_Position', 'ShapeModule.m_Rotation', 'ShapeModule.m_Scale', 'renderer.m_Flip', 'renderer.m_Pivot'], 'scalars': ['EmissionModule.m_BurstCount', 'InitialModule.maxNumParticles', 'InitialModule.randomizeRotationDirection', 'ShapeModule.angle', 'ShapeModule.arc.mode', 'ShapeModule.arc.spread', 'ShapeModule.arc.value', 'ShapeModule.donutRadius', 'ShapeModule.length', 'ShapeModule.m_MeshMaterialIndex', 'ShapeModule.m_MeshNormalOffset', 'ShapeModule.m_MeshSpawn.mode', 'ShapeModule.m_MeshSpawn.spread', 'ShapeModule.m_TextureClipChannel', 'ShapeModule.m_TextureClipThreshold', 'ShapeModule.m_TextureUVChannel', 'ShapeModule.placementMode', 'ShapeModule.radius.mode', 'ShapeModule.radius.spread', 'ShapeModule.radius.value', 'ShapeModule.radiusThickness', 'ShapeModule.randomDirectionAmount', 'ShapeModule.randomPositionAmount', 'ShapeModule.sphericalDirectionAmount', 'ShapeModule.type', 'cullingMode', 'emitterVelocityMode', 'gameObject.layer', 'lengthInSec', 'moveWithTransform', 'randomSeed', 'renderer.m_CameraVelocityScale', 'renderer.m_LengthScale', 'renderer.m_MaxParticleSize', 'renderer.m_MinParticleSize', 'renderer.m_RenderAlignment', 'renderer.m_RenderMode', 'renderer.m_SortMode', 'renderer.m_SortingFudge', 'renderer.m_VelocityScale', 'ringBufferLoopRange.x', 'ringBufferLoopRange.y', 'ringBufferMode', 'scalingMode', 'simulationSpeed', 'stopAction']}
CIGARETTE_PARTICLE_FLAGS={'ColorModule.enabled': True, 'EmissionModule.enabled': True, 'ForceModule.enabled': True, 'ForceModule.inWorldSpace': True, 'ForceModule.randomizePerFrame': True, 'InitialModule.enabled': True, 'InitialModule.rotation3D': False, 'InitialModule.size3D': False, 'ShapeModule.alignToDirection': False, 'ShapeModule.enabled': True, 'ShapeModule.m_TextureAlphaAffectsParticles': True, 'ShapeModule.m_TextureBilinearFiltering': False, 'ShapeModule.m_TextureColorAffectsParticles': True, 'ShapeModule.m_UseMeshColors': True, 'ShapeModule.m_UseMeshMaterialIndex': False, 'SizeModule.enabled': True, 'SizeModule.separateAxes': False, 'VelocityModule.enabled': True, 'VelocityModule.inWorldSpace': True, 'autoRandomSeed': True, 'looping': True, 'playOnAwake': True, 'prewarm': False, 'useUnscaledTime': False, 'gameObject.active': True, 'renderer.enabled': True}

# Checked ParticleSystem128014 and ParticleSystemRenderer128908 operation selectors.
CIGARETTE_PARTICLE_OPERATIONS={'ShapeModule.type': 0, 'ShapeModule.arc.mode': 0, 'ShapeModule.radius.mode': 0, 'ShapeModule.m_MeshSpawn.mode': 0, 'ShapeModule.placementMode': 0, 'ShapeModule.m_TextureClipChannel': 3, 'ShapeModule.m_TextureUVChannel': 0, 'renderer.m_RenderMode': 0, 'renderer.m_RenderAlignment': 0, 'renderer.m_SortMode': 0, 'cullingMode': 0, 'emitterVelocityMode': 1, 'ringBufferMode': 0, 'scalingMode': 1, 'stopAction': 0, 'moveWithTransform': 1, 'EmissionModule.m_BurstCount': 0}
