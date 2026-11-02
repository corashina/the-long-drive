"""Checked spirit level, spray bottle, genie lamp, binoculars, start packs, and mouth smoke inputs."""
from .source import key, source_id
from .survival import component, managed_evidence
from .lighting import gradient
from .particles import particle

# Catalog item index, catalog name, prefab script, and script class.
SPIRIT_LEVEL=(401,'waterlevel',147652,'bubbleLevelScript')
SPRAY_BOTTLE=(121,'jizzonit',142096,'spricniscript')
GENIE_LAMP=(211,'ginlamp',152760,'ginlampscript')
BINOCULARS=(85,'binoculars',144842,'tavcsoscript')
# StartSpawnItems copies: script and ammoscript.
START_BOTTLES=((71175,78224),(71176,78226))
START_PACKS=((80187,77156),(70712,70989),(70715,70994))
# The garage binoculars tavcsoscript.
START_BINOCULARS=72758
BINOCULAR_FIELDS=('eyeDist','focusDistance','focusDistanceSens','minfocusDistance','maxfocusDistance','minfov','maxfov')
START_GROUP=21481
# World prototype CobWeb wash triggers. Inactive level1 scene copies lie on no prototype.
COBWEBS=529
PLAYER=80125
SPRAY_FIELDS=('cleaningType','text','angleSpeed','triggerMaxAngle','triggerSetAngle','triggerResetAngle','invert',
    'volumeSet','volumeReset','gain','distance','washRadius')
EVIDENCE=[
    'value = Mathf.Lerp(value, bubbleCurve.Evaluate(Mathf.InverseLerp(-1f, 1f, Vector3.Dot((t2.position - t1.position).normalized, Vector3.up))), lerp * Time.deltaTime);',
    'bubble.position += up * moveXY * moveXYCurve.Evaluate(value);',
    'bubble.localScale = new Vector3(xyscale, xyscale, Mathf.Lerp(minScaleZ, maxScaleZ, scaleZCurve.Evaluate(value)));',
    'currentAngle = Mathf.Lerp(currentAngle, triggerMaxAngle, angleSpeed * Time.deltaTime);',
    'if (currentAngle >= triggerSetAngle && lastAngle < currentAngle && !triggered)',
    'UnityEngine.Object.Instantiate(particle, hole.position, Quaternion.LookRotation(target), transform);',
    'UnityEngine.Object.Instantiate(particle2, hit.point, Quaternion.LookRotation(hit.normal), hit.transform);',
    'value += gainValue * Time.deltaTime * mainscript.M.sleepModifier;',
    'value -= loseValue * Time.deltaTime * mainscript.M.sleepModifier;',
    'M.SetColor("_EmissionColor", emission.Evaluate(value));',
    'smokeParticleEmission.rateOverTime = smokeCharged * maxSmokeParticleRate;',
    'smokeCharged -= Time.deltaTime * smokeParticleChargeSpeed * 2f;',
    'smokeCharged += Time.deltaTime * smokeParticleChargeSpeed;',
    'if (repairValue > 1f && !neverRepair)',
    'UnityEngine.Object.Instantiate(particleOnDestroy, transform.position, Quaternion.identity);',
    'if (chanceToStartDisabled > 0f && UnityEngine.Random.Range(0f, 1f) < chanceToStartDisabled)',
    'if (input.reload && Time.time - lastReloadTime >= ammoscript2.pourTime)',
    'focusDistance = Mathf.Clamp(focusDistance, minfocusDistance, maxfocusDistance);',
    'currentFov = Mathf.Lerp(minfov, maxfov, Mathf.InverseLerp(maxfocusDistance, minfocusDistance, focusDistance));',
    'focusDistance += pf * focusDistanceSens;',
    'pP.tavcso.Focusing(scroll * dt);',
    'MoveCamParent(currentTavcso.CamR.transform.position);',
    'RotateCamParent(currentTavcso.CamR.transform.rotation);',
    'currentFieldOfView = currentTavcso.currentFov;']


def animation_curve(data):
    return {'keys':[{'time':k['time'],'value':k['value'],'in_tangent':k['inSlope'],'out_tangent':k['outSlope'],
        'in_weight':k['inWeight'],'out_weight':k['outWeight'],'weighted_mode':k['weightedMode']} for k in data['m_Curve']],
        'pre_wrap_mode':data['m_PreInfinity'],'post_wrap_mode':data['m_PostInfinity']}


def ammo(source, reader):
    d=source.read(reader)
    if source.class_name(reader)!='ammoscript':raise ValueError(f'{key(reader)} is not ammoscript')
    return {'source':source_id(reader),'count':d['ammo'],'float_count':d['fammo'],'maximum':d['max'],
        'float_or_int':bool(d['floatOrInt']),'type':d['tipus'],'random_range':[d['ammoRmin'],d['ammoRmax']]}


def recover_small_items(source, manifest, output) -> dict:
    managed_evidence(source, EVIDENCE)
    def node_of(target):
        if target.type.name=='Transform':return target
        if target.type.name!='GameObject':target=source.resolve(target,source.read(target)['m_GameObject'])
        return source.transform_for(target)
    catalog=manifest['world']['catalog']['items']
    prototypes={(p['source']['file'],p['source']['path_id']):p for p in manifest['world']['prototypes']}
    def owner(spec):
        index,name,script,cname=spec
        item=catalog[index]
        if item['name']!=name or item['prototype'] is None:raise ValueError(f'Catalog item {index} {name} is not a world prototype')
        prototype=prototypes[(item['prototype']['file'],item['prototype']['path_id'])]
        reader=source.get('sharedassets1.assets',script)
        if reader.type.name!='MonoBehaviour' or source.class_name(reader)!=cname:raise ValueError(f'{key(reader)} is not {cname}')
        nodes={(n['source']['file'],n['source']['path_id']) for n in prototype['nodes']}
        go=source.resolve(reader,source.read(reader)['m_GameObject'])
        if key(source.transform_for(go)) not in nodes:raise ValueError(f'{key(reader)} lies outside catalog item {name}')
        def link(d, field, kind, inside=True):
            target=source.resolve(reader,d[field])
            if target is None or target.type.name!=kind:raise ValueError(f'{key(reader)} {field} is not {kind}')
            if inside and key(node_of(target)) not in nodes:
                raise ValueError(f'{key(reader)} {field} lies outside catalog item {name}')
            return target
        return {'item':index,'prototype':item['prototype'],'source':source_id(reader)},reader,source.read(reader),link

    record,reader,d,link=owner(SPIRIT_LEVEL)
    if len(d['bubbles'])!=3:raise ValueError(f'{key(reader)} bubble count differs')
    def bubble(b):
        targets={f:source.resolve(reader,b[f]) for f in ('bubble','t1','t2')}
        for f,t in targets.items():
            if t is None or t.type.name!='Transform':raise ValueError(f'{key(reader)} bubble {f} is not Transform')
        return {**{f:source_id(t) for f,t in targets.items()},'lerp':b['lerp'],'bubble_curve':animation_curve(b['bubbleCurve']),
            'scale_z_curve':animation_curve(b['scaleZCurve']),'scale_xy_curve':animation_curve(b['scaleXYCurve']),
            'move_xy_curve':animation_curve(b['moveXYCurve']),'min_scale_xy':b['minScaleXY'],'max_scale_xy':b['maxScaleXY'],
            'min_scale_z':b['minScaleZ'],'max_scale_z':b['maxScaleZ'],'move_xy':b['moveXY'],'startup_value':b['value']}
    spirit_level={**record,'bubbles':[bubble(b) for b in d['bubbles']]}

    record,reader,d,link=owner(SPRAY_BOTTLE)
    clips=[link(d,f,'AudioClip',False) for f in ('triggerSet','triggerReset')]
    source.item_clips.update(key(c) for c in clips)
    def effect(field,role):
        go=link(d,field,'GameObject',False);root=source.transform_for(go)
        system=component(source,root,'ParticleSystem')
        rate=source.read(system)['EmissionModule']['rateOverTime']['scalar']
        return particle(source,system,role,rate,output,root)
    holder=link(d,'ammo','MonoBehaviour');hd=source.read(holder)
    # noPerShow ends ReloadFromInventory: a bottle refills only from an aimed bottle.
    if not hd['noPerShow'] or hd['isMag'] or hd['w']['m_PathID']:
        raise ValueError(f'{key(holder)} reload path differs')
    spray_bottle={**record,'ammo':ammo(source,holder),'can_pour':bool(hd['canPour']),'pour_time':hd['pourTime'],
        'cleaning_type':d['cleaningType'],'text':d['text'],'angle_speed':d['angleSpeed'],
        'trigger_max_angle':d['triggerMaxAngle'],'trigger_set_angle':d['triggerSetAngle'],'trigger_reset_angle':d['triggerResetAngle'],
        'invert':bool(d['invert']),'trigger':source_id(link(d,'trigger','Transform')),'hole':source_id(link(d,'hole','Transform')),
        'set_clip':source_id(clips[0]),'reset_clip':source_id(clips[1]),'set_volume':d['volumeSet'],'reset_volume':d['volumeReset'],
        'mask':d['LM']['m_Bits'],'gain':d['gain'],'distance':d['distance'],'wash_radius':d['washRadius'],'wash_mask':d['washMask']['m_Bits'],
        'spray':effect('particle','spray_bottle_spray'),'splash':effect('particle2','spray_bottle_splash'),'startup':[]}
    group=source.get('level1',START_GROUP)
    def start_copy(script_id, ammo_id, cname, prefab=None):
        copy=source.get('level1',script_id);cd=source.read(copy)
        if source.class_name(copy)!=cname:raise ValueError(f'{key(copy)} is not {cname}')
        go=source.resolve(copy,cd['m_GameObject']);root=source.transform_for(go)
        if key(source.resolve(root,source.read(root)['m_Father']))!=key(group) or not source.read(go)['m_IsActive']:
            raise ValueError(f'{key(copy)} is not an active StartSpawnItems child')
        holder=source.resolve(copy,cd['ammo']) if 'ammo' in cd else component(source,root,'ammoscript')
        if key(holder)!=('level1',ammo_id):raise ValueError(f'{key(copy)} ammo link differs')
        if prefab is not None and (any(cd[f]!=prefab[1][f] for f in SPRAY_FIELDS) or cd['LM']!=prefab[1]['LM'] or cd['washMask']!=prefab[1]['washMask']
                or any(key(source.resolve(copy,cd[f]))!=key(source.resolve(prefab[0],prefab[1][f])) for f in ('particle','particle2','triggerSet','triggerReset'))):
            raise ValueError(f'{key(copy)} differs from its catalog prefab')
        save=source.read(component(source,root,'tosaveitemscript'))
        return {'source':source_id(copy),'transform':source_id(root),'item_id':save['id'],'ammo':ammo(source,holder)}
    for script_id,ammo_id in START_BOTTLES:
        spray_bottle['startup'].append(start_copy(script_id,ammo_id,'spricniscript',(reader,d)))

    record,reader,d,link=owner(GENIE_LAMP)
    wash=link(d,'washTrigger','MonoBehaviour')
    if source.class_name(wash)!='washtriggerscript':raise ValueError(f'{key(reader)} washTrigger is not washtriggerscript')
    genie_lamp={**record,'light':source_id(link(d,'light','Light')),'renderer':source_id(link(d,'R','MeshRenderer')),
        'material':source_id(link(d,'M','Material',False)),'wash_trigger':source_id(wash),
        'gain':d['gainValue'],'lose':d['loseValue'],'intensity':animation_curve(d['intensity']),'range':animation_curve(d['range']),
        'emission':gradient(d['emission']),'startup_value':d['value']}

    record,reader,d,link=owner(BINOCULARS)
    camera=node_of(link(d,'CamR','Camera'))
    hidden=link(d,'makeInvisible','MonoBehaviour');hd=source.read(hidden)
    if source.class_name(hidden)!='makeinvisible' or not hd['make']:raise ValueError(f'{key(hidden)} is not a set makeinvisible')
    root=source.transform_for(source.resolve(reader,d['m_GameObject']))
    renderers={key(c) for n in source.hierarchy(root) for c in n['components'] if c.type.name=='MeshRenderer'}
    # makeinvisible.R holds every renderer, so the runtime hides the whole item.
    if {key(source.resolve(hidden,r)) for r in hd['R']}!=renderers:raise ValueError(f'{key(hidden)} renderers differ from the item')
    if not d['minfocusDistance']<d['maxfocusDistance'] or not 0<d['minfov']<d['maxfov']:raise ValueError(f'{key(reader)} focus range differs')
    copy=source.get('level1',START_BINOCULARS);cd=source.read(copy)
    copy_root=source.transform_for(source.resolve(copy,cd['m_GameObject']))
    if (source.class_name(copy)!='tavcsoscript' or key(source.resolve(copy_root,source.read(copy_root)['m_Father']))!=key(group)
            or not source.read(source.resolve(copy,cd['m_GameObject']))['m_IsActive'] or any(cd[f]!=d[f] for f in BINOCULAR_FIELDS)):
        raise ValueError(f'{key(copy)} differs from its catalog prefab')
    binoculars={**record,'camera':source_id(camera),'focus_distance':d['focusDistance'],'focus_sensitivity':d['focusDistanceSens'],
        'min_focus':d['minfocusDistance'],'max_focus':d['maxfocusDistance'],'min_fov':d['minfov'],'max_fov':d['maxfov'],
        'startup':[{'source':source_id(copy),'transform':source_id(copy_root),
            'item_id':source.read(component(source,copy_root,'tosaveitemscript'))['id']}]}

    start_packs=[start_copy(script_id,ammo_id,'cigiscript') for script_id,ammo_id in START_PACKS]
    washes,wash_effects=recover_washes(source,manifest,output,genie_lamp['wash_trigger'])

    player=source.get('level1',PLAYER);pd=source.read(player)
    system=source.resolve(player,pd['smokeParticle'])
    if system is None or system.type.name!='ParticleSystem':raise ValueError('level1:80125 smokeParticle is not ParticleSystem')
    node=source.transform_for(source.resolve(system,source.read(system)['m_GameObject']))
    node_ids={(n['source']['file'],n['source']['path_id']):n['id'] for n in manifest['nodes']}
    if key(node) not in node_ids:raise ValueError('level1:80125 smokeParticle node is not exported')
    exhale={'controller':source_id(player),'node':node_ids[key(node)],'max_rate':pd['maxSmokeParticleRate'],
        'charge_speed':pd['smokeParticleChargeSpeed'],'particle':particle(source,system,'mouth_smoke',pd['maxSmokeParticleRate'],output,node)}
    # fpscontroller.Update: heatHaze.SetActive(BOldHeatHaze). The system simulates in the player root space.
    haze_object=source.resolve(player,pd['heatHaze'])
    if haze_object is None or haze_object.type.name!='GameObject':raise ValueError('level1:80125 heatHaze is not GameObject')
    haze_node=source.transform_for(haze_object)
    player_node=source.transform_for(source.resolve(player,pd['m_GameObject']))
    if key(source.resolve(haze_node,source.read(haze_node)['m_Father']))!=key(player_node):raise ValueError('level1:80125 heatHaze is not a player root child')
    haze_system=component(source,haze_node,'ParticleSystem')
    haze_data=source.read(haze_system)
    if haze_data['moveWithTransform']!=0 or not haze_data['prewarm'] or not haze_data['looping']:raise ValueError('Old heat haze is not a local looping prewarm system')
    if haze_data['ShapeModule']['type']!=0:raise ValueError('Old heat haze shape is not a sphere')
    managed_evidence(source,['heatHaze.SetActive(settingsscript.s.S.BOldHeatHaze);'])
    heat_haze=particle(source,haze_system,'old_heat_haze',haze_data['EmissionModule']['rateOverTime']['scalar'],output,player_node)
    return {'spirit_level':spirit_level,'spray_bottle':spray_bottle,'genie_lamp':genie_lamp,'binoculars':binoculars,'start_packs':start_packs,'exhale':exhale,
        'heat_haze':heat_haze,'washes':washes,'wash_effects':wash_effects}


def recover_washes(source, manifest, output, lamp):
    """washtriggerscript on world prototype nodes, except the genie lamp trigger."""
    nodes={}
    for p in manifest['world']['prototypes']:
        for i,n in enumerate(p['nodes']):nodes[(n['source']['file'],n['source']['path_id'])]=(p,i)
    washes=[];effects={}
    for r in manifest['world']['records']:
        if r['class_name']!='washtriggerscript' or r['source']==lamp:continue
        reader=source.get(r['source']['file'],r['source']['path_id']);d=source.read(reader)
        root=source.transform_for(source.resolve(reader,d['m_GameObject']))
        if key(root) not in nodes:continue
        p,index=nodes[key(root)]
        # WashTriggers with CleanProperty 1 clean dirt objects. They are not building cobwebs, and
        # the runtime has no dirt wash yet, so they are not exported here.
        if d['CleanProperty']!=2:continue
        def below(i):
            while i is not None:
                if i==index:return True
                i=p['nodes'][i]['parent']
            return False
        def inside(go):
            node=source.transform_for(source.resolve(reader,go))
            if nodes.get(key(node),(None,))[0] is not p:raise ValueError(f'{key(reader)} object lies outside its prototype')
            return source_id(node)
        effect=source.resolve(reader,d['particleOnDestroy'])
        if effect is not None and key(effect) not in effects:
            if effect.type.name!='GameObject':raise ValueError(f'{key(reader)} particleOnDestroy is not GameObject')
            er=source.transform_for(effect);system=component(source,er,'ParticleSystem');sd=source.read(system)
            audio=source.read(component(source,er,'AudioSource'))
            clip=source.resolve(effect,audio['m_audioClip'])
            if clip is None or clip.type.name!='AudioClip' or not audio['m_PlayOnAwake'] or audio['Loop']:
                raise ValueError(f'{key(effect)} destroy sound differs')
            source.item_clips.add(key(clip))
            effects[key(effect)]={'source':source_id(effect),
                'particle':particle(source,system,'wash_destroy',sd['EmissionModule']['rateOverTime']['scalar'],output),
                'clip':source_id(clip),'volume':audio['m_Volume']}
        washes.append({'source':source_id(reader),'prototype':p['source'],'node':source_id(root),
            'colliders':[c['source'] for c in p['colliders'] if below(c['node'])],
            'permission':d['CleanProperty'],'never_repair':bool(d['neverRepair']),'destroy_on_repair':bool(d['destroyOnRepair']),
            'objects':[inside(o) for o in d['objects']],'chance_to_start_disabled':d['chanceToStartDisabled'],
            'particle_on_destroy':effects[key(effect)]['source'] if effect is not None else None})
    return washes,list(effects.values())


def _shape(value, fields):
    if type(value) is not dict or set(value)!=set(fields.split()):raise ValueError('Small item record fields differ')


def _number(value):
    import math
    if type(value) not in (int,float) or type(value) is bool or not math.isfinite(value):raise ValueError('Small item number differs')


def _curve(c):
    _shape(c,'keys pre_wrap_mode post_wrap_mode')
    if not c['keys'] or any(a['time']>=b['time'] for a,b in zip(c['keys'],c['keys'][1:])):raise ValueError('Small item curve keys differ')
    for k in c['keys']:
        for f in ('time','value','in_tangent','out_tangent','in_weight','out_weight'):_number(k[f])


def validate_small_items(record, manifest) -> None:
    _shape(record,'spirit_level spray_bottle genie_lamp binoculars start_packs exhale heat_haze washes wash_effects')
    clips={(c['source']['file'],c['source']['path_id']) for c in manifest['audio']['clips']}
    l=record['spirit_level'];_shape(l,'item prototype source bubbles')
    if l['item']!=SPIRIT_LEVEL[0] or len(l['bubbles'])!=3:raise ValueError('Spirit level owner differs')
    for b in l['bubbles']:
        _shape(b,'bubble t1 t2 lerp bubble_curve scale_z_curve scale_xy_curve move_xy_curve min_scale_xy max_scale_xy min_scale_z max_scale_z move_xy startup_value')
        for f in ('bubble_curve','scale_z_curve','scale_xy_curve','move_xy_curve'):_curve(b[f])
        for f in ('lerp','min_scale_xy','max_scale_xy','min_scale_z','max_scale_z','move_xy','startup_value'):_number(b[f])
    s=record['spray_bottle']
    _shape(s,'item prototype source ammo can_pour pour_time cleaning_type text angle_speed trigger_max_angle trigger_set_angle trigger_reset_angle invert trigger hole set_clip reset_clip set_volume reset_volume mask gain distance wash_radius wash_mask spray splash startup')
    if s['item']!=SPRAY_BOTTLE[0] or not 0<s['trigger_reset_angle']<s['trigger_set_angle']<s['trigger_max_angle']:raise ValueError('Spray bottle trigger differs')
    if any((c['file'],c['path_id']) not in clips for c in (s['set_clip'],s['reset_clip'])):raise ValueError('Spray bottle clip is not exported')
    _number(s['pour_time'])
    if s['can_pour'] is not False or s['pour_time']<0:raise ValueError('Spray bottle pour differs')
    if len(s['startup'])!=len(START_BOTTLES) or any(c['item_id']!=SPRAY_BOTTLE[0] for c in s['startup']):raise ValueError('Start spray bottles differ')
    g=record['genie_lamp']
    _shape(g,'item prototype source light renderer material wash_trigger gain lose intensity range emission startup_value')
    _curve(g['intensity']);_curve(g['range'])
    if g['item']!=GENIE_LAMP[0] or not g['emission']['colors'] or not 0<g['lose']<g['gain']:raise ValueError('Genie lamp values differ')
    b=record['binoculars']
    _shape(b,'item prototype source camera focus_distance focus_sensitivity min_focus max_focus min_fov max_fov startup')
    for f in ('focus_distance','focus_sensitivity','min_focus','max_focus','min_fov','max_fov'):_number(b[f])
    if b['item']!=BINOCULARS[0] or not b['min_focus']<b['max_focus'] or not 0<b['min_fov']<b['max_fov'] or b['focus_sensitivity']<=0:
        raise ValueError('Binocular values differ')
    if len(b['startup'])!=1 or b['startup'][0]['item_id']!=BINOCULARS[0]:raise ValueError('Start binoculars differ')
    for c in b['startup']:_shape(c,'source transform item_id')
    if len(record['start_packs'])!=len(START_PACKS) or any(p['item_id']!=74 or p['ammo']['type']!=5 for p in record['start_packs']):
        raise ValueError('Start cigarette packs differ')
    e=record['exhale'];_shape(e,'controller node max_rate charge_speed particle')
    if e['max_rate']<=0 or e['charge_speed']<=0:raise ValueError('Mouth smoke rates differ')
    _washes(record['washes'],record['wash_effects'],clips)
    h=record['heat_haze']
    if [m['shader']['name'] for m in h['materials']]!=['FX/Glass/Stained BumpDistort']:raise ValueError('Old heat haze material differs')
    for p in (s['spray'],s['splash'],e['particle'],h)+tuple(f['particle'] for f in record['wash_effects']):
        if not p['materials'] or 'InitialModule.startLifetime' not in p['curves']:raise ValueError('Small item particle data missing')


def _washes(washes, effects, clips):
    """CobWeb triggers: CleanProperty 2, destroyOnRepair, no objects, one sphere trigger each."""
    if len(effects)!=1:raise ValueError('Wash destroy effects differ')
    for f in effects:
        _shape(f,'source particle clip volume');_number(f['volume'])
        if (f['clip']['file'],f['clip']['path_id']) not in clips or not 0<=f['volume']<=1:raise ValueError('Wash destroy sound differs')
    sources={(f['source']['file'],f['source']['path_id']) for f in effects}
    for w in washes:
        _shape(w,'source prototype node colliders permission never_repair destroy_on_repair objects chance_to_start_disabled particle_on_destroy')
        _number(w['chance_to_start_disabled'])
        e=w['particle_on_destroy']
        if (w['permission']!=2 or w['never_repair'] or not w['destroy_on_repair'] or w['objects'] or len(w['colliders'])!=1
                or not 0<=w['chance_to_start_disabled']<1 or e is None or (e['file'],e['path_id']) not in sources):
            raise ValueError('Cobweb wash trigger differs')
    if len(washes)!=COBWEBS or len({(w['node']['file'],w['node']['path_id']) for w in washes})!=COBWEBS:raise ValueError('Cobweb wash count differs')
