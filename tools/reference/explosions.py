"""Checked explosions: mainscript explosion prefabs, the tank hole prefab, player blast fields and explosive fluids."""
from .source import key, source_id
from .survival import managed_evidence, finite_record, component
from .particles import particle

MAIN=('level1',64919)
PLAYER=('level1',80125)
# mainscript.GetExplosionType: 0 and 1 ExplosionRed, 2 ExplosionBlue, 3 ExplosionPaint.
TYPES=('ExplosionRed','ExplosionRed','ExplosionBlue','ExplosionPaint')
# mainscript.fluidenum count.
FLUID_TYPES=6
EVIDENCE=[
    '0 => ExplosionRed,',
    '1 => ExplosionRed,',
    '2 => ExplosionBlue,',
    '3 => ExplosionPaint,',
    'explosionDist = explosionForce * 0.5f + distAdd;',
    # fluidcontainer.isExplosive: explosive share > 0.4 and total amount > 0.2.
    'if (f / GetAmount() > 0.4f)',
    'return all > 0.2f;',
    'GameObject gameObject2 = UnityEngine.Object.Instantiate(mainscript.M.tankcaphole, list2[l] + list3[l] * 0.01f, Quaternion.LookRotation(list3[l]));',
    'EarRingSource.volume = Mathf.Clamp(earRinging, 0f, 1f);']
TYPE_FIELDS='prefab source explosion_force multiplier explosion_dist particle sound'
SOUND_FIELDS='source clip volume'
HOLE_FIELDS={'radius':'radius','valve':'valve','def_pour_angle':'defPourAngle','tune_speed':'tuneSpeed','min_rate':'minRate','max_rate':'maxRate',
    'no_angle':'noAngle','no_pour':'noPour','cant_tube':'canttube','can_toggle':'canToggle','can_tune':'canTune','has_name':'hasName','no_text':'noText'}
HOLE_FLAGS=('no_angle','no_pour','cant_tube','can_toggle','can_tune','has_name','no_text')
PLAYER_FIELDS={'min_force_to_die':'minForceToDieFromExplosion','hear_loss_modifier':'hearLossModifier','hear_regen':'hearRegen',
    'ear_ringing_gain':'earRingingGain','ear_ringing_regen':'earRingingRegen'}


def _order(identity):
    return identity['file'],identity['path_id']


def _sound(source, owner, pointer) -> dict:
    """An AudioSource link: its clip joins the audio clips."""
    s=source.resolve(owner,pointer)
    if s is None or s.type.name!='AudioSource':raise ValueError('link is not AudioSource')
    d=source.read(s);clip=source.resolve(s,d['m_audioClip'])
    if clip is None or clip.type.name!='AudioClip':raise ValueError('AudioSource has no AudioClip')
    source.item_clips.add(key(clip))
    return {'source':source_id(s),'clip':source_id(clip),'volume':d['m_Volume']}


def _prefab(source, main, field):
    g=source.resolve(main,source.read(main)[field])
    if g is None or g.type.name!='GameObject':raise ValueError(f'mainscript {field} is not GameObject')
    return source.transform_for(g)


def _type(source, main, index, field, output) -> dict:
    root=_prefab(source,main,field)
    c=component(source,root,'explosionscript');d=source.read(c)
    system=source.resolve(c,d['particle'])
    if system is None or system.type.name!='ParticleSystem':raise ValueError(f'{field} particle is not ParticleSystem')
    rate=source.read(system)['EmissionModule']['rateOverTime']['scalar']
    return {'prefab':source_id(root),'source':source_id(c),'explosion_force':d['explosionForce'],'multiplier':d['multiplier'],
        'explosion_dist':d['explosionDist'],'particle':particle(source,system,f'explosion_{index}',rate,output,root),'sound':_sound(source,c,d['Source'])}


def recover_explosions(source, manifest, output) -> dict:
    managed_evidence(source, EVIDENCE)
    main=source.get(*MAIN);d=source.read(main)
    if source.class_name(main)!='mainscript':raise ValueError(f'{key(main)} is not mainscript')
    types=[]
    for i,field in enumerate(TYPES):
        try:types.append(_type(source,main,i,field,output))
        except (ValueError,KeyError) as error:raise ValueError(f'Explosion type {i}: {error}') from error
    hole_root=_prefab(source,main,'tankcaphole')
    cap=component(source,hole_root,'tankcapscript');cd=source.read(cap)
    hole={'prefab':source_id(hole_root),'source':source_id(cap),**{k:cd[f] for k,f in HOLE_FIELDS.items()}}
    for f in HOLE_FLAGS:hole[f]=bool(hole[f])
    player=source.get(*PLAYER);pd=source.read(player)
    if source.class_name(player)!='fpscontroller':raise ValueError(f'{key(player)} is not fpscontroller')
    blast={'source':source_id(player),**{k:pd[f] for k,f in PLAYER_FIELDS.items()},'ear_ring':_sound(source,player,pd['EarRingSource'])}
    fluids=[int(f) for f in d['explosivefluids']]
    print(f"Exported explosions: {len(types)} types, explosive fluids {fluids}",flush=True)
    return {'source':source_id(main),'types':types,'hole':hole,'player':blast,'explosive_fluids':fluids}


def validate_explosions(record, manifest) -> None:
    from .acquisition import _shape,_source,_integer,_number,_boolean,_list
    _shape(record,'source types hole player explosive_fluids')
    finite_record(record)
    _source(record['source'])
    audio={_order(c['source']) for c in manifest['audio']['clips']}
    textures={t['id'] for t in manifest['lighting']['textures']}
    def sound(s):
        _shape(s,SOUND_FIELDS);_source(s['source']);_source(s['clip']);_number(s['volume'])
        if _order(s['clip']) not in audio:raise ValueError(f"clip {s['clip']['file']}:{s['clip']['path_id']} missing from audio")
    types=_list(record['types'])
    if len(types)!=len(TYPES):raise ValueError('Explosion type count differs')
    for i,t in enumerate(types):
        try:
            _shape(t,TYPE_FIELDS);_source(t['prefab']);_source(t['source'])
            for f in ('explosion_force','multiplier','explosion_dist'):_number(t[f])
            p=t['particle']
            if p['role']!=f'explosion_{i}' or not p['materials'] or 'InitialModule.startLifetime' not in p['curves']:raise ValueError('particle data missing')
            if any(x['texture'] not in textures for m in p['materials'] for x in m['textures']):raise ValueError('particle texture is not exported')
            sound(t['sound'])
        except (ValueError,KeyError,TypeError) as error:raise ValueError(f'Explosion type {i}: {error}') from error
    h=record['hole']
    _shape(h,' '.join(['prefab','source',*HOLE_FIELDS]));_source(h['prefab']);_source(h['source'])
    for f in HOLE_FIELDS:
        if f in HOLE_FLAGS:_boolean(h[f])
        else:_number(h[f])
    p=record['player']
    _shape(p,' '.join(['source','ear_ring',*PLAYER_FIELDS]));_source(p['source']);sound(p['ear_ring'])
    for f in PLAYER_FIELDS:_number(p[f])
    fluids=_list(record['explosive_fluids'])
    for f in fluids:_integer(f,FLUID_TYPES-1)
    if not fluids or len(set(fluids))!=len(fluids):raise ValueError('Explosive fluids are missing or not unique')
