"""Checked melee weapons and hit effects: meleeweaponscript records, controllers, mainscript hit particles, sounds and volumes."""
from .source import id_key, key, source_id
from .survival import managed_evidence
from .particles import particle
from .acquisition_motion import recover_motion

MAIN=('level1',64919)
# mainscript.particle order. Tyre smoke and tyre sand are not melee hit effects.
HIT_PARTICLES=('spark','blood','plant','wood','sand')
# mainscript.soundHitType order: metal, blood, plant, wood, sand, glass.
HIT_VOLUMES=[0.3,0.6,0.4,0.4,0.5,0.4]
# In tosaveitemscript.IsRecentlyUnburied, an unburied item stays recent for 1 s.
RECENT_RECOVERY_SECONDS=1.
EVIDENCE=[
    'public static float[] hitVolume = new float[6] { 0.3f, 0.6f, 0.4f, 0.4f, 0.5f, 0.4f };',
    'Particles[(int)tipus].Emit(num);',
    'mainscript.M.Particle(mainscript.particle.sand, vector, T.position - vector, (int)((float)sparknum * sand));',
    'tosaveitemscript2.UpdBuried(shovelSpeed);',
    'mainscript.PlayClipAtPoint(mainscript.M.meleeHitSounds[(int)hitType].a[num], V, mainscript.hitVolume[(int)hitType], mainscript.AudioPriorities[7]);']
WEAPON_FIELDS='prototype source pickup target trigger contact_mask speed shovel_speed attack_volume attack_clips controller right_hand_poses left_hand_poses contact'
CONTACT_FIELDS={'force':'force','damage':'damage','damage_tank':'damageTank','hurt_food':'hurtFood','all_particles':'allparticle','spark':'spark','sand':'sand','blood':'blood','plant':'plant','wood':'wood'}


def _weapon(source, prototype, c, pickup) -> dict:
    d=source.read(c)
    def link(pointer, kind, cls=None):
        target=source.resolve(c,pointer)
        if target is None or target.type.name!=kind or (cls and source.class_name(target)!=cls):raise ValueError(f'link is not {cls or kind}')
        return target
    # meleeweaponscript.p only moves the hands to RHs and LHs. pickupable.melee links the held weapon.
    if d['p']['m_PathID'] and key(link(d['p'],'MonoBehaviour','pickupable'))!=key(pickup):raise ValueError('p is not the pickupable that holds it')
    weapon={'prototype':prototype,'source':source_id(c),'pickup':source_id(pickup),
        'target':source_id(link(d['T'],'Transform')),'trigger':source_id(link(d['Trigger'],'Transform')),
        'contact_mask':d['mask']['m_Bits'],'speed':d['speed'],'shovel_speed':d['shovelSpeed'],'attack_volume':d['attackVolume'],
        'attack_clips':[source_id(link(p,'AudioClip')) for p in d['attackClips']],
        'controller':source_id(link(d['AnimationController'],'AnimatorController')),
        'right_hand_poses':[source_id(link(p,'Transform')) for p in d['RHs']],
        'left_hand_poses':[source_id(link(p,'Transform')) for p in d['LHs']],
        'contact':{k:d[v] for k,v in CONTACT_FIELDS.items()}}
    weapon['contact']['hurt_food']=bool(d['hurtFood'])
    for clip in weapon['attack_clips']:source.item_clips.add(id_key(clip))
    return weapon


def recover_melee(source, manifest, output) -> dict:
    managed_evidence(source, EVIDENCE)
    main=source.get(*MAIN);d=source.read(main)
    if source.class_name(main)!='mainscript':raise ValueError(f'{key(main)} is not mainscript')
    particles=[]
    for name,pointer in zip(HIT_PARTICLES,d['Particles']):
        system=source.resolve(main,pointer)
        if system is None or system.type.name!='ParticleSystem':raise ValueError(f'mainscript particle {name} is not ParticleSystem')
        # Particle() moves the shared system and calls Emit(num). Emission does not run.
        particles.append(particle(source,system,f'melee_{name}',0.,output))
    hit_clips=[]
    for group in d['meleeHitSounds']:
        clips=[source.resolve(main,p) for p in group['a']]
        if any(c is None or c.type.name!='AudioClip' for c in clips):raise ValueError('mainscript meleeHitSounds clip is not AudioClip')
        source.item_clips.update(key(c) for c in clips)
        hit_clips.append([source_id(c) for c in clips])
    # A world prototype meleeweaponscript that a pickupable.melee links is a weapon.
    # A body-part weapon (the leg kick) has no pickupable. Melee items are pickups only.
    weapons=[]
    for p in manifest['world']['prototypes']:
        scripts,pickups=[],{}
        for n in p['nodes']:
            for c in source.components(source.get(n['source']['file'],n['source']['path_id']))[1]:
                if c.type.name!='MonoBehaviour':continue
                name=source.class_name(c)
                if name=='meleeweaponscript':scripts.append(c)
                elif name=='pickupable':
                    melee=source.resolve(c,source.read(c)['melee'])
                    if melee is not None:pickups[key(melee)]=c
        for c in scripts:
            if key(c) not in pickups:continue
            try:weapons.append(_weapon(source,p['source'],c,pickups[key(c)]))
            except (ValueError,KeyError) as error:raise ValueError(f'Melee weapon {key(c)}: {error}') from error
    weapons.sort(key=lambda w:(id_key(w['prototype']),id_key(w['source'])))
    motions=[]
    for identity in sorted({id_key(w['controller']) for w in weapons}):
        try:motions.append(recover_motion(source,manifest,source.get(*identity)))
        except (ValueError,KeyError) as error:
            owners=[f"{w['source']['file']}:{w['source']['path_id']}" for w in weapons if id_key(w['controller'])==identity]
            raise ValueError(f"Melee weapons {', '.join(owners)}: {error}") from error
    print(f"Exported melee weapons: {', '.join(str(w['source']['path_id']) for w in weapons)}; {len(motions)} controllers",flush=True)
    return {'source':source_id(main),'hit_volumes':HIT_VOLUMES,'particles':particles,'hit_clips':hit_clips,
        'recent_recovery_seconds':RECENT_RECOVERY_SECONDS,'weapons':weapons,'motions':motions}


