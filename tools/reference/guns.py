"""Checked guns: weaponscript records, ammo, gun kind helpers, shot tracers and bullet-hole decals.

Vectors (positions, Euler angles, axes) keep source Unity local axes. Nodes are world prototype nodes.
"""
from .source import key, source_id, vector
from .survival import managed_evidence, finite_record
from .particles import particle, relative_pose
from .small_items import animation_curve, ammo
from .lighting import rgba, sky_material
from .gltf import convert_point

MAIN=('level1',64919)
AMMO_TYPES=7
# mainscript static arrays. They are not serialized; managed evidence checks them.
AMMO_MASS=[0.0005,0.0102,0.,0.,0.01,0.,0.006]
AMMO_VELOCITY=[300.,376.4,0.,0.,641.3,0.,365.]
# mainscript.GetDecal order: default, metal, flesh, wood, plant, glass.
DECALS=6
EVIDENCE=[
    'public static float[] ammoMass = new float[7] { 0.0005f, 0.0102f, 0f, 0f, 0.01f, 0f, 0.006f };',
    'public static float[] ammoVelocity = new float[7] { 300f, 376.4f, 0f, 0f, 641.3f, 0f, 365f };',
    'currentBulletHole = mainscript.M.ShootDecals[mainscript.GetDecal(hit[k].collider)];',
    'ParticleShootStart.Emit(1);',
    'ParticleShootEnd.Emit(1);',
    'yield return new WaitForSeconds(lifeTime);']
WEAPON_FIELDS=('prototype source pickup ammo kind condition max_condition_weakness infinite shot_count shot_count_spread automatic '
    'automatic_false_shot min_shoot_time mask mask2 muzzle clips audio_m flash recoil reload_hand barrels moving_parts triggers '
    'trigger_triggers use_conditional_trigger_triggers conditional_trigger_triggers fr sounds crosshair explode_stuffs explode_chance hithole')
CLIP_FIELDS={'one_shot':'OneShotClip','false_shot':'FalseShotClip','hit_sand':'ShotHitSand','hit_concrete':'ShotHitConcrete',
    'hit_flesh':'ShotHitFlesh','hit_glass':'ShotHitGlass'}
FR_FIELDS='value fr_speed use_x use_y breakable play_sound_even_if_cant_shoot shoot reload'
RULE_FIELDS='value equal bigger smaller near near_value'
KIND_FIELDS={'none':'',
    'revolver':'chambers cylinder axis angle duration shoot_bullet_pos cylinder_start_rotation turn_clip turn_volume',
    'bb_gun':'power power_gain max_power ammo_gain dont_use_power bullets',
    'magazine':'mag mag_pos mag_prefab has_mag'}


def _order(identity):
    return identity['file'],identity['path_id']


def _node(source, reader):
    """The Transform of a Transform, GameObject or component."""
    if reader.type.name=='Transform':return reader
    if reader.type.name!='GameObject':reader=source.resolve(reader,source.read(reader)['m_GameObject'])
    return source.transform_for(reader)


def _weapon(source, prototype, c) -> dict:
    d=source.read(c)
    def link(owner, pointer, kind, cls=None, optional=False):
        target=source.resolve(owner,pointer)
        if target is None and optional:return None
        if target is None or target.type.name!=kind or (cls and source.class_name(target)!=cls):raise ValueError(f'link is not {cls or kind}')
        return target
    def node(owner, pointer, optional=False):
        target=source.resolve(owner,pointer)
        if target is None and optional:return None
        if target is None or target.type.name not in ('Transform','GameObject'):raise ValueError('link is not Transform')
        return source_id(_node(source,target))
    def clip(owner, pointer, optional=True):
        target=link(owner,pointer,'AudioClip',optional=optional)
        if target is None:return None
        source.item_clips.add(key(target))
        return source_id(target)
    def sound(clip_field, volume_field):
        identity=clip(c,d[clip_field])
        return None if identity is None else {'clip':identity,'volume':d[volume_field]}
    def rule(prefix, name):
        return {'value':d[f'FRCan{name}'],'equal':bool(d[f'FR{prefix}Equal']),'bigger':bool(d[f'FR{prefix}Bigger']),
            'smaller':bool(d[f'FR{prefix}Smaller']),'near':bool(d[f'FR{prefix}Near']),'near_value':d[f'FRCan{name}NearValue']}
    flash=link(c,d['flash'],'Light',optional=True)
    recoil=node(c,d['TRecoil'],True)
    reload_hand=node(c,d['ReloadLHPos'],True)
    icon=link(c,d['weaponCrossHairIcon'],'Sprite',optional=True)
    if icon is not None:source.crosshair_icons.add(key(icon))
    condition=link(c,d['condition'],'MonoBehaviour','partconditionscript',optional=True)
    move_source=link(c,d['SSource'],'AudioSource',optional=True)
    def barrel(b):
        axis=next((i for i,a in enumerate('xyz') if b[a]),None)
        return {'node':node(c,b['TRotBarrel']),'max_angle':b['FBarrelMaxAngle'],'inv':bool(b['inv']),'axis':axis,
            'curve':animation_curve(b['curve']) if b['useCurve'] else None}
    def trigger(t):
        return {'node':node(c,t['T']),'dont_work_without_shoot':bool(t['dontWorkWithoutShoot']),'duration':t['duration'],
            'use_angle':bool(t['useAngle']),'def_angle':list(vector(t['defAngle'])),'angle':list(vector(t['Angle'])),'angle_curve':animation_curve(t['AngleCurve']),
            'use_pos':bool(t['usePos']),'def_pos':list(vector(t['defPos'])),'pos':list(vector(t['Pos'])),'pos_curve':animation_curve(t['PosCurve'])}
    weapon={'prototype':prototype,'source':source_id(c),'pickup':source_id(link(c,d['P'],'MonoBehaviour','pickupable')),
        'ammo':ammo(source,link(c,d['ammo'],'MonoBehaviour','ammoscript')),'kind':_kind(source,c,d,link,node,clip),
        'condition':source_id(condition) if condition else None,'max_condition_weakness':d['maxConditionWeakness'],'infinite':bool(d['infinite']),
        'shot_count':d['shotCount'],'shot_count_spread':d['shotCountSpread'],'automatic':bool(d['automatic']),
        'automatic_false_shot':bool(d['automaticFalseShot']),'min_shoot_time':d['minShootTime'],
        'mask':d['LM']['m_Bits'],'mask2':d['LM2']['m_Bits'],'muzzle':node(c,d['S']),
        'clips':{k:[clip(c,p,False) for p in d[f]] for k,f in CLIP_FIELDS.items()},'audio_m':d['audioM'],
        'flash':None if flash is None else {'light':source_id(flash),'node':source_id(_node(source,flash)),'time':d['flashTime'],
            'intensity':d['defIntensity'],'curve':animation_curve(d['flashCurve'])},
        'recoil':None if recoil is None else {'node':recoil,'pos':list(vector(d['recoilPos'])),'rot':list(vector(d['recoilRot'])),
            'curve':animation_curve(d['recoilCurve']),'duration':d['recoilDuration']},
        'reload_hand':None if reload_hand is None else {'node':reload_hand,'default':node(c,d['ReloadLHPosDef']),
            'barrel':node(c,d['ReloadLHPosBarrel'],True),'reload':node(c,d['ReloadLHPosReload'],True)},
        'barrels':[barrel(b) for b in d['Barrels']],
        'moving_parts':[{'node':node(c,p['T']),'t0':node(c,p['T0']),'t1':node(c,p['T1']),'inv':bool(p['inv'])} for p in d['MovingParts']],
        'triggers':[trigger(t) for t in d['Triggers']],
        'trigger_triggers':[{'node':node(c,t['Trigger']),'dont_trigger':bool(t['dontTrigger']),'def_rotation':list(vector(t['defTriggerRot'])),
            'pulled_rotation':list(vector(t['pulledTriggerRot']))} for t in d['TriggerTriggers']],
        'use_conditional_trigger_triggers':bool(d['useConditionalTriggerTriggers']),
        'conditional_trigger_triggers':[{'ammo_count':t['ammoCount'],'use':[bool(u) for u in t['useTriggerTrigger']]} for t in d['conditionalTriggerTrigger']],
        'fr':{'value':d['FRValue'],'fr_speed':d['FRSpeed'],'use_x':bool(d['useX']),'use_y':bool(d['useY']),'breakable':bool(d['letorhetocso']),
            'play_sound_even_if_cant_shoot':bool(d['playSoundEvenIfCantShoot']),'shoot':rule('S','Shoot'),'reload':rule('R','Reload')},
        'sounds':{'source':source_id(move_source) if move_source else None,'open':sound('SOpen','openVolume'),'open_end':sound('SOpenEnd','openEndVolume'),
            'close':sound('SClose','closeVolume'),'move':clip(c,d['SMove']),'reload':sound('SReload','reloadVolume')},
        'crosshair':{'icon':source_id(icon) if icon else None,'color':rgba(d['weaponCrossHairColor']),'empty_color':rgba(d['weaponCrossHairColorEmpty'])},
        'explode_stuffs':bool(d['explodeStuffs']),'explode_chance':d['explodeChance'],'hithole':bool(d['hithole'])}
    return weapon


def _kind(source, c, d, link, node, clip) -> dict:
    helpers=[(name,link(c,d[field],'MonoBehaviour',cls,optional=True)) for name,field,cls in
        (('bb_gun','bbgun','bbgunscript'),('revolver','revolver','revolverScript'),('magazine','geppityu','geppityuscript'))]
    helpers=[(name,h) for name,h in helpers if h is not None]
    if len(helpers)>1:raise ValueError('weapon has more than one kind helper')
    if not helpers:return {'type':'none'}
    name,h=helpers[0];hd=source.read(h)
    if name=='revolver':
        return {'type':name,'chambers':[node(h,p) for p in hd['Bullets']],'cylinder':node(h,hd['Cylinder']),'axis':list(vector(hd['Axis'])),
            'angle':hd['angle'],'duration':hd['duration'],'shoot_bullet_pos':node(h,hd['ShootBulletPos']),
            'cylinder_start_rotation':list(vector(hd['cylinderStartRotation'])),'turn_clip':clip(h,hd['teker']),'turn_volume':hd['tekerVolume']}
    if name=='bb_gun':
        return {'type':name,'power':hd['power'],'power_gain':hd['powerGain'],'max_power':hd['maxPower'],'ammo_gain':hd['ammoGain'],
            'dont_use_power':bool(hd['dontUsePower']),'bullets':[node(h,p) for p in hd['bullet']]}
    prefab=source.resolve(h,hd['magPrefab'])
    if prefab is None or prefab.type.name!='GameObject':raise ValueError('magPrefab is not GameObject')
    return {'type':name,'mag':node(h,hd['mag']),'mag_pos':node(h,hd['magPos']),'mag_prefab':source_id(_node(source,prefab)),'has_mag':bool(d['hasMag'])}


def _decal(source, prefab, output) -> dict:
    """One ShootDecals prefab: its single mesh, material, pose under the root and DecalDestroyer lifetime."""
    if prefab is None or prefab.type.name!='GameObject':raise ValueError('ShootDecals entry is not GameObject')
    root=source.transform_for(prefab)
    hierarchy=source.hierarchy(root,activate_root=True)
    def kind(n,name):return [c for c in n['components'] if (source.class_name(c) if c.type.name=='MonoBehaviour' else c.type.name)==name]
    meshes=[(n,f) for n in hierarchy for f in kind(n,'MeshFilter')]
    destroyers=[c for n in hierarchy for c in kind(n,'DecalDestroyer')]
    if not meshes or len(destroyers)>1:raise ValueError(f'{key(root)} decal needs a mesh and at most one DecalDestroyer')
    # Decals are a front quad and a back quad that faces into the hit surface. The decal pipeline does not
    # cull, so the first quad draws both sides. The quads share mesh, material and scale.
    def quad(entry):
        n,f=entry;r=kind(n,'MeshRenderer')
        materials=[m['m_PathID'] for m in source.read(r[0])['m_Materials']] if len(r)==1 else None
        return source.read(f)['m_Mesh']['m_PathID'],materials,vector(source.read(n['reader'])['m_LocalScale'])
    if any(quad(entry)!=quad(meshes[0]) for entry in meshes[1:]):raise ValueError(f'{key(root)} decal quads differ')
    n,f=meshes[0]
    mesh=source.resolve(f,source.read(f)['m_Mesh'])
    renderers=kind(n,'MeshRenderer')
    if mesh is None or mesh.type.name!='Mesh' or len(renderers)!=1:raise ValueError(f'{key(root)} decal mesh differs')
    materials=[source.resolve(renderers[0],p) for p in source.read(renderers[0])['m_Materials']]
    if len(materials)!=1 or materials[0] is None:raise ValueError(f'{key(root)} decal needs one material')
    bounds=source.read(mesh)['m_LocalAABB']
    return {'source':source_id(root),'node':source_id(n['reader']),'local_pose':relative_pose(source,n['reader'],root),
        'mesh':{'source':source_id(mesh),'bounds_center':list(convert_point(vector(bounds['m_Center']))),'bounds_extents':list(vector(bounds['m_Extent']))},
        'material':sky_material(source,materials[0],output),
        'lifetime':source.read(destroyers[0])['lifeTime'] if destroyers else None}


def recover_guns(source, manifest, output) -> dict:
    managed_evidence(source, EVIDENCE)
    main=source.get(*MAIN);d=source.read(main)
    if source.class_name(main)!='mainscript':raise ValueError(f'{key(main)} is not mainscript')
    tracer={}
    for name,field in (('start','ParticleShootStart'),('end','ParticleShootEnd')):
        system=source.resolve(main,d[field])
        if system is None or system.type.name!='ParticleSystem':raise ValueError(f'mainscript {field} is not ParticleSystem')
        # ShootStart/ShootEnd move the shared system and call Emit(1); emission does not run.
        tracer[name]=particle(source,system,f'gun_tracer_{name}',0.,output)
    decals=[_decal(source,source.resolve(main,p),output) for p in d['ShootDecals']]
    # Every world prototype node with a weaponscript is a gun.
    weapons=[]
    for p in manifest['world']['prototypes']:
        for n in p['nodes']:
            for c in source.components(source.get(n['source']['file'],n['source']['path_id']))[1]:
                if c.type.name!='MonoBehaviour' or source.class_name(c)!='weaponscript':continue
                try:weapons.append(_weapon(source,p['source'],c))
                except (ValueError,KeyError) as error:raise ValueError(f'Gun {key(c)}: {error}') from error
    weapons.sort(key=lambda w:(_order(w['prototype']),_order(w['source'])))
    print(f"Exported guns: {', '.join(str(w['source']['path_id']) for w in weapons)}; {len(decals)} decals",flush=True)
    return {'source':source_id(main),'ammo_mass':AMMO_MASS,'ammo_velocity':AMMO_VELOCITY,'tracer':tracer,'decals':decals,'weapons':weapons}


def _sid(value):
    if type(value) is not dict or set(value)!={'file','path_id'} or type(value['file']) is not str or not value['file'] or type(value['path_id']) is not int or not value['path_id']:
        raise ValueError('source identity differs')


def _curve(c):
    from .acquisition import _shape,_number,_list
    _shape(c,'keys pre_wrap_mode post_wrap_mode')
    keys=_list(c['keys'])
    if any(a['time']>=b['time'] for a,b in zip(keys,keys[1:])):raise ValueError('curve keys are not sorted')
    for k in keys:
        for f in ('time','value','in_tangent','out_tangent','in_weight','out_weight'):_number(k[f])


def validate_guns(record, manifest) -> None:
    from .acquisition import _shape,_integer,_number,_boolean,_list,_numbers
    _shape(record,'source ammo_mass ammo_velocity tracer decals weapons')
    finite_record(record)
    if record['ammo_mass']!=AMMO_MASS or record['ammo_velocity']!=AMMO_VELOCITY:raise ValueError('Gun ammo values differ')
    _shape(record['tracer'],'start end')
    for p in record['tracer'].values():
        if not p['materials'] or 'InitialModule.startLifetime' not in p['curves']:raise ValueError('Gun tracer particle data missing')
    if len(_list(record['decals']))!=DECALS:raise ValueError('Gun decal count differs')
    textures={t['id'] for t in manifest['lighting']['textures']}
    for i,decal in enumerate(record['decals']):
        try:
            _shape(decal,'source node local_pose mesh material lifetime')
            _sid(decal['source']);_sid(decal['node'])
            _shape(decal['mesh'],'source bounds_center bounds_extents');_sid(decal['mesh']['source'])
            _numbers(decal['mesh']['bounds_center'],3);_numbers(decal['mesh']['bounds_extents'],3)
            if decal['lifetime'] is not None:_number(decal['lifetime'])
            if any(t['texture'] not in textures for t in decal['material']['textures']):raise ValueError('material texture is not exported')
        except (ValueError,KeyError,TypeError) as error:raise ValueError(f'Gun decal {i}: {error}') from error
    audio={_order(c['source']) for c in manifest['audio']['clips']}
    sprites={_order(s['source']) for s in manifest['ui']['sprites']}
    world={_order(p['source']):p for p in manifest['world']['prototypes']}
    weapons=_list(record['weapons'])
    for w in weapons:
        try:
            _shape(w,WEAPON_FIELDS)
            for f in ('prototype','source','pickup','muzzle'):_sid(w[f])
            prototype=world.get(_order(w['prototype']))
            if prototype is None:raise ValueError('prototype is not a world prototype')
            nodes={_order(n['source']) for n in prototype['nodes']}
            def node(value):
                _sid(value)
                if _order(value) not in nodes:raise ValueError('Transform link is outside the prototype')
            def clip(value):
                _sid(value)
                if _order(value) not in audio:raise ValueError(f"clip {value['file']}:{value['path_id']} missing from audio")
            def vec(value):_numbers(value,3)
            a=w['ammo'];_shape(a,'source count float_count maximum float_or_int type random_range');_sid(a['source'])
            _integer(a['count']);_integer(a['type'],AMMO_TYPES-1);_number(a['float_count']);_number(a['maximum']);_boolean(a['float_or_int']);_numbers(a['random_range'],2)
            if w['condition'] is not None:_sid(w['condition'])
            for f in ('max_condition_weakness','shot_count_spread','min_shoot_time','audio_m','explode_chance'):_number(w[f])
            for f in ('infinite','automatic','automatic_false_shot','use_conditional_trigger_triggers','explode_stuffs','hithole'):_boolean(w[f])
            for f in ('shot_count','mask','mask2'):_integer(w[f])
            node(w['muzzle'])
            _shape(w['clips'],' '.join(CLIP_FIELDS))
            for v in w['clips'].values():
                for c in _list(v):clip(c)
            if w['flash'] is not None:
                f=w['flash'];_shape(f,'light node time intensity curve');_sid(f['light']);node(f['node']);_number(f['time']);_number(f['intensity']);_curve(f['curve'])
            if w['recoil'] is not None:
                r=w['recoil'];_shape(r,'node pos rot curve duration');node(r['node']);vec(r['pos']);vec(r['rot']);_curve(r['curve']);_number(r['duration'])
            if w['reload_hand'] is not None:
                h=w['reload_hand'];_shape(h,'node default barrel reload');node(h['node']);node(h['default'])
                for f in ('barrel','reload'):
                    if h[f] is not None:node(h[f])
            for b in _list(w['barrels']):
                _shape(b,'node max_angle inv axis curve');node(b['node']);_number(b['max_angle']);_boolean(b['inv'])
                if b['axis'] is not None:_integer(b['axis'],2)
                if b['curve'] is not None:_curve(b['curve'])
            for p in _list(w['moving_parts']):
                _shape(p,'node t0 t1 inv');node(p['node']);node(p['t0']);node(p['t1']);_boolean(p['inv'])
            for t in _list(w['triggers']):
                _shape(t,'node dont_work_without_shoot duration use_angle def_angle angle angle_curve use_pos def_pos pos pos_curve');node(t['node'])
                _number(t['duration'])
                for f in ('dont_work_without_shoot','use_angle','use_pos'):_boolean(t[f])
                for f in ('def_angle','angle','def_pos','pos'):vec(t[f])
                _curve(t['angle_curve']);_curve(t['pos_curve'])
            for t in _list(w['trigger_triggers']):
                _shape(t,'node dont_trigger def_rotation pulled_rotation');node(t['node']);_boolean(t['dont_trigger']);vec(t['def_rotation']);vec(t['pulled_rotation'])
            for t in _list(w['conditional_trigger_triggers']):
                _shape(t,'ammo_count use');_integer(t['ammo_count'])
                if len(_list(t['use']))<len(w['trigger_triggers']):raise ValueError('conditional trigger flags are fewer than trigger triggers')
                for u in t['use']:_boolean(u)
            fr=w['fr'];_shape(fr,FR_FIELDS)
            for f in ('value','fr_speed'):_number(fr[f])
            for f in ('use_x','use_y','breakable','play_sound_even_if_cant_shoot'):_boolean(fr[f])
            for r in (fr['shoot'],fr['reload']):
                _shape(r,RULE_FIELDS);_number(r['value']);_number(r['near_value'])
                for f in ('equal','bigger','smaller','near'):_boolean(r[f])
            s=w['sounds'];_shape(s,'source open open_end close move reload')
            if s['source'] is not None:_sid(s['source'])
            if s['move'] is not None:clip(s['move'])
            for f in ('open','open_end','close','reload'):
                if s[f] is not None:_shape(s[f],'clip volume');clip(s[f]['clip']);_number(s[f]['volume'])
            x=w['crosshair'];_shape(x,'icon color empty_color');_numbers(x['color'],4);_numbers(x['empty_color'],4)
            if x['icon'] is not None and _order(x['icon']) not in sprites:raise ValueError('crosshair icon missing from ui sprites')
            k=w['kind']
            if type(k) is not dict or k.get('type') not in KIND_FIELDS:raise ValueError('kind differs')
            _shape(k,' '.join(['type',*KIND_FIELDS[k['type']].split()]))
            if k['type']=='revolver':
                for n in [k['cylinder'],k['shoot_bullet_pos'],*_list(k['chambers'])]:node(n)
                vec(k['axis']);vec(k['cylinder_start_rotation']);_number(k['angle']);_number(k['duration']);_number(k['turn_volume'])
                if k['turn_clip'] is not None:clip(k['turn_clip'])
            elif k['type']=='bb_gun':
                for f in ('power','power_gain','max_power','ammo_gain'):_integer(k[f])
                _boolean(k['dont_use_power'])
                for n in _list(k['bullets']):node(n)
            elif k['type']=='magazine':
                node(k['mag']);node(k['mag_pos']);_boolean(k['has_mag']);_sid(k['mag_prefab'])
                if _order(k['mag_prefab']) not in world:raise ValueError('magazine prefab is not a world prototype')
        except (ValueError,KeyError,TypeError) as error:
            source=w.get('source',{}) if type(w) is dict else {}
            raise ValueError(f"Gun {source.get('file')}:{source.get('path_id')}: {error}") from error
    prototypes=[_order(w['prototype']) for w in weapons]
    if not weapons or prototypes!=sorted(set(prototypes)):raise ValueError('Gun prototypes are missing, not unique or not sorted')
