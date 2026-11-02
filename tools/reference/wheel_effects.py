"""Wheel ground effects: talajnezo per wheel, carscript gains and the mainscript skid record."""
import math
from .source import key, source_id
from .particles import particle, relative_pose
from .survival import managed_evidence

MAIN=('level1',64919)
# mainscript.particle order: tiresmoke is 5, tiresand is 6.
SKID_PARTICLES=(('tiresmoke',5),('tiresand',6))
EVIDENCE=[
    'mainscript.M.Particle(mainscript.particle.tiresmoke, hit.point, W.transform.forward * Mathf.Sign(0f - wrpm), (int)(mainscript.M.tireSmokeRate * num5));',
    'mainscript.M.Particle(mainscript.particle.tiresand, hit.point, W.transform.forward * Mathf.Sign(0f - wrpm), (int)(mainscript.M.tireSmokeRate * num5));']
VOICES=(('ground','GroundSource'),('spring','SpringSource'),('slide','SlideSource'))


def wheel_scripts(source, root):
    """Active, enabled talajnezo scripts under the car root."""
    return [c for node in source.hierarchy(root,activate_root=True) if node['active'] for c in node['components']
        if c.type.name=='MonoBehaviour' and source.class_name(c)=='talajnezo' and source.read(c)['m_Enabled']]


def wheel_links(source, root, wheels):
    """(talajnezo, data) of each road wheel, in `wheels` order."""
    if not wheels:raise ValueError(f'Car has no road wheels for wheel effects: {key(root)}')
    scripts={}
    for script in wheel_scripts(source,root):
        d=source.read(script);collider=source.resolve(script,d['W'])
        if collider is None:continue
        if key(collider) in scripts:raise ValueError(f'Duplicate talajnezo wheel link: {key(collider)}')
        scripts[key(collider)]=(script,d)
    for wheel in wheels:
        if key(wheel) not in scripts:raise ValueError(f'Wheel needs talajnezo: {key(wheel)}')
    return [scripts[key(wheel)] for wheel in wheels]


def recover_wheel_effects(source, car_reader, car, root, wheels, index, output):
    """One record per road wheel, in `wheels` order. `root` is the car root Transform."""
    result=[]
    for w,(script,d) in enumerate(wheel_links(source,root,wheels)):
        # A null Car is legal: the wheel uses its own gains and has no slide sound.
        linked=source.resolve(script,d['Car'])
        if linked is not None and key(linked)!=key(car_reader):raise ValueError(f'talajnezo Car link differs: {source_id(script)}')
        node=source.transform_for(source.resolve(script,d['m_GameObject']))
        dust=source.resolve(script,d['ps'])
        grass=[source.resolve(script,p) for p in ([d['grasstrail']] if d['grasstrail']['m_PathID'] else [])+list(d['grasss'])]
        if any(g is None for g in grass):raise ValueError(f'talajnezo grass link is null: {source_id(script)}')
        record={'source':source_id(script),'pose':relative_pose(source,node,root),'rate':d['rateM'],'ray_mask':d['LM']['m_Bits'],
            'own_gains':None if linked is not None else {'ground':d['groundSoundM'],'spring':d['springSoundM'],'spring_pitch':d['springPitchM']},
            # The runtime sets the dust rate from speed.
            'dust':particle(source,dust,f'wheel_{index}_{w}_dust',1.,output,root) if dust else None,
            'grass':[particle(source,g,f'wheel_{index}_{w}_grass_{i}',source.read(g)['EmissionModule']['rateOverTime']['scalar'],output,root)
                for i,g in enumerate(grass)]}
        result.append(record)
    return result


def recover_wheel_voices(source, root, wheels):
    """AudioSource ids of each wheel talajnezo, in `wheels` order. recover_audio replaces them with voice ids."""
    result=[]
    for script,d in wheel_links(source,root,wheels):
        record={}
        for field,name in VOICES:
            audio=source.resolve(script,d[name])
            if audio is None:raise ValueError(f'talajnezo {name} is null: {source_id(script)}')
            record[field]=source_id(audio)
        result.append(record)
    return result


def effect_gains(car):
    return {'ground':car['groundSoundM'],'spring':car['springSoundM'],'spring_pitch':car['springPitchM'],
        'slide_angle':car['slideAngle'],'max_slide_volume_speed':car['maxSlideVolumeSpeed']}


def recover_skid(source, output):
    """mainscript skid data. Clip fields hold AudioClip ids; recover_audio replaces them with clip ids."""
    managed_evidence(source,EVIDENCE)
    main=source.get(*MAIN);d=source.read(main)
    if source.class_name(main)!='mainscript':raise ValueError(f'{key(main)} is not mainscript')
    particles=[]
    for name,index in SKID_PARTICLES:
        system=source.resolve(main,d['Particles'][index])
        if system is None or system.type.name!='ParticleSystem':raise ValueError(f'mainscript particle {name} is not ParticleSystem')
        # Particle() moves the shared system and calls Emit(num); emission does not run.
        particles.append(particle(source,system,f'skid_{name}',0.,output))
    clips=[]
    for field in ('SSkidRoad','SSkidDirt'):
        clip=source.resolve(main,d[field])
        if clip is None:raise ValueError(f'mainscript {field} is null')
        clips.append(source_id(clip))
    return {'source':source_id(main),'road_clip':clips[0],'dirt_clip':clips[1],
        'road_volume':d['SkidRoadVolume'],'road_pitch':[d['SkidRoadPitchMin'],d['SkidRoadPitchMax']],
        'dirt_volume':d['SkidDirtVolume'],'dirt_pitch':[d['SkidDirtPitchMin'],d['SkidDirtPitchMax']],
        'speed_difference':[d['CarMinSpeedDif'],d['CarMaxSpeedDif']],'smoke_rate':d['tireSmokeRate'],'particles':particles}


def validate_skid(record):
    fields={'source','road_clip','dirt_clip','road_volume','road_pitch','dirt_volume','dirt_pitch','speed_difference','smoke_rate','particles'}
    if set(record)!=fields:raise ValueError('Skid record fields differ')
    numbers=[record['road_volume'],record['dirt_volume'],record['smoke_rate'],*record['road_pitch'],*record['dirt_pitch'],*record['speed_difference']]
    if not all(isinstance(v,(int,float)) and math.isfinite(v) for v in numbers):raise ValueError('Skid values are not finite')
    if any(r[0]>r[1] for r in (record['road_pitch'],record['dirt_pitch'],record['speed_difference'])):raise ValueError('Skid ranges are not ordered')
    if [p['role'] for p in record['particles']]!=[f'skid_{name}' for name,_ in SKID_PARTICLES]:raise ValueError('Skid particles differ')
