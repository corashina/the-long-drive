"""Checked melee weapons and hit effects: meleeweaponscript records, controllers, mainscript hit particles, sounds and volumes."""
from .source import key, source_id
from .survival import managed_evidence, finite_record
from .particles import particle
from .acquisition_motion import recover_motion, validate_motion

MAIN=('level1',64919)
# mainscript.particle order. Tyre smoke and tyre sand are not melee hit effects.
HIT_PARTICLES=('spark','blood','plant','wood','sand')
# mainscript.soundHitType order: metal, blood, plant, wood, sand, glass.
HIT_VOLUMES=[0.3,0.6,0.4,0.4,0.5,0.4]
# tosaveitemscript.IsRecentlyUnburied: an unburied item stays recent for 1 s.
RECENT_RECOVERY_SECONDS=1.
EVIDENCE=[
    'public static float[] hitVolume = new float[6] { 0.3f, 0.6f, 0.4f, 0.4f, 0.5f, 0.4f };',
    'Particles[(int)tipus].Emit(num);',
    'mainscript.M.Particle(mainscript.particle.sand, vector, T.position - vector, (int)((float)sparknum * sand));',
    'tosaveitemscript2.UpdBuried(shovelSpeed);',
    'mainscript.PlayClipAtPoint(mainscript.M.meleeHitSounds[(int)hitType].a[num], V, mainscript.hitVolume[(int)hitType], mainscript.AudioPriorities[7]);']
WEAPON_FIELDS='prototype source pickup target trigger contact_mask speed shovel_speed attack_volume attack_clips controller right_hand_poses left_hand_poses contact'
CONTACT_FIELDS={'force':'force','damage':'damage','damage_tank':'damageTank','hurt_food':'hurtFood','all_particles':'allparticle','spark':'spark','sand':'sand','blood':'blood','plant':'plant','wood':'wood'}
# The shovel (item 216) keeps its checked source values. Other weapons get structure and link checks.
SHOVEL={'file':'sharedassets1.assets','path_id':132938}
SHOVEL_ITEM=216
SHOVEL_CONTACT={'force':20000.,'damage':40.,'damage_tank':12.,'hurt_food':True,'all_particles':0.009999999776482582,'blood':.5,'sand':2.,'plant':1.,'wood':.5,'spark':.5}


def _order(identity):
    return identity['file'],identity['path_id']


def _weapon(source, prototype, c) -> dict:
    d=source.read(c)
    def link(pointer, kind, cls=None):
        target=source.resolve(c,pointer)
        if target is None or target.type.name!=kind or (cls and source.class_name(target)!=cls):raise ValueError(f'link is not {cls or kind}')
        return target
    weapon={'prototype':prototype,'source':source_id(c),'pickup':source_id(link(d['p'],'MonoBehaviour','pickupable')),
        'target':source_id(link(d['T'],'Transform')),'trigger':source_id(link(d['Trigger'],'Transform')),
        'contact_mask':d['mask']['m_Bits'],'speed':d['speed'],'shovel_speed':d['shovelSpeed'],'attack_volume':d['attackVolume'],
        'attack_clips':[source_id(link(p,'AudioClip')) for p in d['attackClips']],
        'controller':source_id(link(d['AnimationController'],'AnimatorController')),
        'right_hand_poses':[source_id(link(p,'Transform')) for p in d['RHs']],
        'left_hand_poses':[source_id(link(p,'Transform')) for p in d['LHs']],
        'contact':{k:d[v] for k,v in CONTACT_FIELDS.items()}}
    weapon['contact']['hurt_food']=bool(d['hurtFood'])
    for clip in weapon['attack_clips']:source.item_clips.add(_order(clip))
    return weapon


def recover_melee(source, manifest, output) -> dict:
    managed_evidence(source, EVIDENCE)
    main=source.get(*MAIN);d=source.read(main)
    if source.class_name(main)!='mainscript':raise ValueError(f'{key(main)} is not mainscript')
    particles=[]
    for name,pointer in zip(HIT_PARTICLES,d['Particles']):
        system=source.resolve(main,pointer)
        if system is None or system.type.name!='ParticleSystem':raise ValueError(f'mainscript particle {name} is not ParticleSystem')
        # Particle() moves the shared system and calls Emit(num); emission does not run.
        particles.append(particle(source,system,f'melee_{name}',0.,output))
    hit_clips=[]
    for group in d['meleeHitSounds']:
        clips=[source.resolve(main,p) for p in group['a']]
        if any(c is None or c.type.name!='AudioClip' for c in clips):raise ValueError('mainscript meleeHitSounds clip is not AudioClip')
        source.item_clips.update(key(c) for c in clips)
        hit_clips.append([source_id(c) for c in clips])
    # Every world prototype node with a meleeweaponscript is a weapon.
    weapons=[]
    for p in manifest['world']['prototypes']:
        for n in p['nodes']:
            for c in source.components(source.get(n['source']['file'],n['source']['path_id']))[1]:
                if c.type.name!='MonoBehaviour' or source.class_name(c)!='meleeweaponscript':continue
                # A body-part weapon (the leg kick) has no pickupable. Melee items are pickups only.
                if not source.read(c)['p']['m_PathID']:continue
                try:weapons.append(_weapon(source,p['source'],c))
                except (ValueError,KeyError) as error:raise ValueError(f'Melee weapon {key(c)}: {error}') from error
    weapons.sort(key=lambda w:(_order(w['prototype']),_order(w['source'])))
    motions=[]
    for identity in sorted({_order(w['controller']) for w in weapons}):
        try:motions.append(recover_motion(source,manifest,source.get(*identity)))
        except (ValueError,KeyError) as error:
            owners=[f"{w['source']['file']}:{w['source']['path_id']}" for w in weapons if _order(w['controller'])==identity]
            raise ValueError(f"Melee weapons {', '.join(owners)}: {error}") from error
    print(f"Exported melee weapons: {', '.join(str(w['source']['path_id']) for w in weapons)}; {len(motions)} controllers",flush=True)
    return {'source':source_id(main),'hit_volumes':HIT_VOLUMES,'particles':particles,'hit_clips':hit_clips,
        'recent_recovery_seconds':RECENT_RECOVERY_SECONDS,'weapons':weapons,'motions':motions}


def validate_melee(record, manifest) -> None:
    from .acquisition import _shape,_source,_integer,_number,_boolean,_list
    if type(record) is not dict or set(record)!={'source','hit_volumes','particles','hit_clips','recent_recovery_seconds','weapons','motions'}:raise ValueError('Melee record fields differ')
    finite_record(record)
    if record['hit_volumes']!=HIT_VOLUMES or len(record['particles'])!=len(HIT_PARTICLES) or record['recent_recovery_seconds']!=RECENT_RECOVERY_SECONDS:raise ValueError('Melee hit values differ')
    for p in record['particles']:
        if not p['materials'] or 'InitialModule.startLifetime' not in p['curves']:raise ValueError('Melee particle data missing')
    audio={_order(c['source']) for c in manifest['audio']['clips']}
    def clips(values):
        for clip in _list(values):
            _source(clip)
            if _order(clip) not in audio:raise ValueError(f"clip {clip['file']}:{clip['path_id']} missing from audio")
    if len(_list(record['hit_clips']))!=len(HIT_VOLUMES):raise ValueError('Melee hit clip groups differ')
    for group in record['hit_clips']:clips(group)
    motions={}
    for m in _list(record['motions']):
        validate_motion(m,manifest)
        if _order(m['controller']) in motions:raise ValueError('Duplicate melee controller')
        motions[_order(m['controller'])]=m
    if list(motions)!=sorted(motions):raise ValueError('Melee controllers are not sorted')
    world={_order(p['source']):p for p in manifest['world']['prototypes']}
    weapons=_list(record['weapons'])
    for w in weapons:
        try:
            _shape(w,WEAPON_FIELDS)
            for f in ('prototype','source','pickup','target','trigger','controller'):_source(w[f])
            for f in ('speed','shovel_speed','attack_volume'):_number(w[f])
            _integer(w['contact_mask'])
            _shape(w['contact'],' '.join(CONTACT_FIELDS))
            for k,v in w['contact'].items():
                if k=='hurt_food':_boolean(v)
                else:_number(v)
            clips(w['attack_clips'])
            prototype=world.get(_order(w['prototype']))
            if prototype is None:raise ValueError('prototype is not a world prototype')
            nodes={_order(n['source']) for n in prototype['nodes']}
            for pose in [w['target'],w['trigger'],*_list(w['right_hand_poses']),*_list(w['left_hand_poses'])]:
                _source(pose)
                if _order(pose) not in nodes:raise ValueError('Transform link is outside the prototype')
            if _order(w['controller']) not in motions:raise ValueError('controller has no motion')
            if w['source']==SHOVEL:_shovel(w,record,manifest)
        except (ValueError,KeyError,TypeError) as error:
            source=w.get('source',{}) if type(w) is dict else {}
            raise ValueError(f"Melee weapon {source.get('file')}:{source.get('path_id')}: {error}") from error
    prototypes=[_order(w['prototype']) for w in weapons]
    if prototypes!=sorted(set(prototypes)):raise ValueError('Melee weapon prototypes are not unique and sorted')
    if sum(w['source']==SHOVEL for w in weapons)!=1:raise ValueError('Melee weapons lack the shovel')
    used={_order(w['controller']) for w in weapons}
    if set(motions)!=used:raise ValueError('Melee controller without weapon')


def _shovel(w, record, manifest) -> None:
    """Checked shovel values and links to the item 216 prototype."""
    p=next((p for p in manifest['item_prototypes'] if p['item_id']==SHOVEL_ITEM),None)
    if p is None or w['prototype']!=p['id']:raise ValueError('shovel prototype differs')
    b=p['behavior']
    if (w['source']!=b['melee'] or w['target']!=b['attack_target'] or w['trigger']!=b['attack_trigger'] or w['pickup']!=p['pickups'][0]['pickup']['source']
        or w['attack_clips']!=b['clips'] or record['hit_clips']!=b['contact_clips']):raise ValueError('shovel links differ')
    if w['contact_mask']!=8552961 or w['speed']!=.699999988079071 or w['shovel_speed']!=.5 or w['attack_volume']!=.5:raise ValueError('shovel values differ')
    if w['right_hand_poses'] or w['left_hand_poses']:raise ValueError('shovel hand pose arrays differ')
    if w['contact']!=SHOVEL_CONTACT:raise ValueError('shovel contact values differ')
