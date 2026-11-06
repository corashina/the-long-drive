"""Recover the selected player's needs, food prototypes, and sleep contracts."""
import hashlib
import json
import re
from pathlib import Path
from .source import LibrarySource, continuous, key, source_id
from .collision import world_pose, geometry, step_surface, collider_tag
from .gltf import GltfExporter
from .actions import recover_pickup
from .render_lighting import f32

SURVIVAL_FIELDS = 'waterToPissRatio foodToShitRatio bloodToHpRatio alcoholHeal alcoholConvertToPiss carFluidsDamage maxHealingSpeed healingToiletFullModfiier waterDamage foodDamage defTemp coldTemp hotTemp foodLoss coldFoodLossModifier hotFoodLossModifier waterLoss coldWaterLossModifier hotWaterLossModifier runFoodLossModifier runWaterLossModifier delaySpeed'.split()

def snake(name):
    return re.sub(r'(?<!^)(?=[A-Z])', '_', name).lower()


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

def food_look(source, root):
    """The `partconditionscript` of a food root, or None. `FStart` with `randomStart` picks `state` in
    `randomMin..=randomMax` and `state3` over `GoList`. `Refresh` puts `MatList[state]` with the forced white
    color on every renderer and shows only `GoList[state3]`."""
    looks=[(n,c) for n in source.hierarchy(root,activate_root=True) for c in n['components']
        if c.type.name=='MonoBehaviour' and source.class_name(c)=='partconditionscript']
    if not looks:return None
    if len(looks)!=1 or key(looks[0][0]['reader'])!=key(root):raise ValueError(f'{key(root)} food condition is not one root component')
    reader=looks[0][1];d=source.read(reader)
    flags={'UseMatList':1,'UseGoList':1,'UseMesList':0,'randomStart':1,'DontUseWorn':1,'forceColor':1,'disableColor':0,
        'useRandomTipus':0,'randomStartState3InFStart':0,'isChild':0,'enableOverrideMin':0,'enableOverrideMax':0,'loaded':0}
    if any(d[k]!=v for k,v in flags.items()) or d['childs'] or d['colors'] or d['color']!={'r':1.,'g':1.,'b':1.,'a':1.}:
        raise ValueError(f'{key(reader)} food condition contract differs')
    if not 0<=d['randomMin']<=d['randomMax']<len(d['MatList']) or len(d['GoList'])<2:raise ValueError(f'{key(reader)} food condition range')
    # The runtime warms the look materials without pass sort data.
    from .materials import recover_material
    if any(recover_material(source,source.resolve(reader,p))['state']['alpha_mode']!='opaque' for p in d['MatList']):raise ValueError(f'{key(reader)} food look material is not opaque')
    return reader

def prepare_food(source, roots):
    roots['food']=[root for _,root in selected_foods(source)]
    # Look materials go into the food asset. Its glTF root is the first food root.
    for root in roots['food']:
        reader=food_look(source,root)
        if reader:
            source.extra_root_materials.setdefault(key(roots['food'][0]),[]).extend(source.resolve(reader,p) for p in source.read(reader)['MatList'])
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
        'linear_damping':rigid['m_Drag'],'angular_damping':rigid['m_AngularDrag'],'ccd':continuous(rigid),
        'pickup':recover_pickup(source,component(source,root,'pickupable')),
        'parts':[[nodes[key(part)]['id'] for part in group] for group in groups],
        'colliders':colliders,'has_particle':bool(data['hasParticle']),'particle_type':data['particleType'],'min_particle':data['minParticle'],'max_particle':data['maxParticle'],
        'sound':data['sound'],'text':data['text'],'look':look_record(source,root,nodes,manifest)}

def look_record(source, root, nodes, manifest):
    reader=food_look(source,root)
    if reader is None:return None
    d=source.read(reader);asset=nodes[key(root)]['asset'];first=key(selected_foods(source)[0][1])
    def node(pointer):
        target=source.resolve(reader,pointer)
        go=target if target.type.name=='GameObject' else source.resolve(target,source.read(target)['m_GameObject'])
        return nodes[key(source.transform_for(go))]['id']
    def material(pointer):
        m=source.resolve(reader,pointer)
        return {'source':source_id(m),'asset':asset,'gltf_material':source.extra_material_indices[first,key(m)]}
    return {'source':source_id(reader),'materials':[material(p) for p in d['MatList']],
        'renderers':[node(p) for p in d['renderers']],'variants':[node(p) for p in d['GoList']],
        'state_range':[d['randomMin'],d['randomMax']]}

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
        'float num = Mathf.InverseLerp(0f, 14f, Time.time - deathTime);', 'private float foodStatShowTime = 5f;', 'private float foodStatHideTime = 1f;', 'defaultInput[21].keys[0].Set(KeyCode.P);','defaultInput[22].keys[0].Set(KeyCode.B);',
        'inputHumVolume = maxHumVolume * 0.5f;','humStartTime = Time.time + humSource.clip.length * 0.75f;','humPitch = Mathf.Pow(2f, humNote / 12f);',
        'humStimul.SetAudioValue(humSource.volume * humStimulValues[selectedHum]);'])
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
        'hum':{'lerp':pd['humLerp'],'pitch_sensitivity':pd['humPitchSens'],'volume_sensitivity':pd['humVolumeSens'],
            'max_volume':pd['maxHumVolume'],'note':[pd['minHumNote'],pd['maxHumNote']],'stimulus_values':pd['humStimulValues']},
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

