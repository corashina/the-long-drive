"""Derive playback rules from accepted records and managed control flow."""
import copy
import hashlib
import json
import math
import re
import shutil
from pathlib import Path
from .library.reader import checked
from .library.types import ObjectId
from .source import key, source_id
from .wheel_effects import recover_wheel_voices
from .bus_doors import door_audio
from .particles import relative_pose

ASSEMBLY_HASH = '9919a9eeddcce23bb4ff86e890437f32b4a3ef3c10af2efe3b8d889d894dd62e'
NATIVE_HASH = '553c1e7560e1b42bd4638c53d0168dcd1b4b9eb5b9df324b0a44f14b3f18bc86'
PRIORITIES = [10, 25, 40, 55, 70, 80, 85, 100]
# These survival roles play once through the shared prefab.
ONE_SHOT = {'eat': 0, 'eat_crunchy': 10, 'item_hit': 11, 'car_hit': 12, 'rock_heat': 13, 'music_box': 14, 'footstep': 15, 'melee_hit': 17, 'radio_click': 21, 'break': 22}
# Other one-shots use AudioPriorities[7]. The music box keeps its source priority.
ONE_SHOT_PRIORITY = {'eat': 70, 'eat_crunchy': 70, 'footstep': PRIORITIES[3], 'radio_click': PRIORITIES[4], 'break': PRIORITIES[6]}
TIER_THRESHOLDS = [0.05, 0.2, 0.5, 0.7]
PHASES = (('morning', 'reggeledik', 'Reggel'), ('day', 'nappal', 'Nappal'),
          ('dusk', 'esteledik', 'Esteledik'), ('night', 'este', 'Este'))
# The hit and rock rules use these source statements as literal values.
HIT_SOURCE = (
    ('pickupable', 'OnCollisionEnter', ('c.relativeVelocity.magnitude * collisionSoundM * ((!pickedUp) ? 0.3f : 1f)',
        'if (num > 0.075f)', 'num - 0.075f', 'mainscript.AudioPriorities[7]')),
    ('pickupable', 'FixedUpdate', ('if (Time.time - lastInteriorSensed > 10f && !pickedUp)',
        'Physics.OverlapSphere(transform.position, 0.1f, m.interiorLayer)')),
    ('carscript', 'OnCollisionEnter', ('magnitude *= collisionSoundM * 0.3f;', 'if (magnitude > 0.7f)',
        'else if (magnitude > 0.5f)', 'else if (magnitude > 0.2f)', 'if (!(magnitude > 0.05f))', 'magnitude = 1f;')),
    ('soundplayscript', 'PlaySound', ('if (!(UnityEngine.Random.Range(0f, 1f) > chance))', 'mainscript.AudioPriorities[7]')),
    ('stepSoundScript', 'Step', ('Physics.Raycast(transform.position, -transform.up, out hit, rayDist, mainscript.M.floorLayerMask)',
        'otherStep = hit.collider.GetComponent<otherStepSoundScript>();',
        'hit.collider.gameObject.layer == mainscript.sandLayer || hit.collider.gameObject.layer == mainscript.sandLayer2 || hit.collider.CompareTag("Sand")',
        'hit.collider.CompareTag("Wood")',
        'mainscript.PlayClipAtPoint(StepSoundClip, transform.position, StepSoundVolume * _modifier * stepSound, randomPitch: true, mainscript.AudioPriorities[3]);')))
# interiorscript.Start uses the GameObject collider when `col` is empty.
INTERIOR_START = ('if (col == null)', 'col = GetComponent<Collider>();')
COLLIDER_TYPES = ('BoxCollider', 'SphereCollider', 'CapsuleCollider', 'MeshCollider')
# The footstep rule uses these source values. fpscontroller calls Step on a walk bob beat and on a grounded change.
STEP_CALL = 'stepScript.Step(duckValue + (input.run ? 0.15f : 0f));'
STEP_STATEMENTS = ('public static int sandLayer = 17;', 'public static int sandLayer2 = 19;')
STEP_SURFACES = ('road', 'desert', 'winter', 'grass', 'wood')


def station_records(source, owner, data, clip):
    stations = []
    for band, field in [('fm', 'radioChs'), ('am', 'radioChsAM')]:
        for value in data[field]:
            custom = bool(value['custom'])
            if custom and (value['music'] or value['talk']):
                raise ValueError('Custom station must retain external content')
            station = {'id': len(stations) + 1, 'band': band, 'frequency': value['FR'],
                'name': value['name'], 'custom': custom, 'content': 'external' if custom else 'embedded',
                'initial_music': bool(value['Bmusic']), 'start_time': value['startTime'],
                'initial_time': value['cTime'], 'initial_max_time': value['cMaxTime'],
                'initial_gain': value['currentVolume']}
            for kind in ['music', 'talk']:
                station[kind] = [clip(source.resolve(owner, p)) for p in value[kind]]
                station[kind + '_gains'] = value[kind + 'Volume']
                station[kind + '_holidays'] = value[kind + 'Holiday']
            stations.append(station)
    return stations


def managed_evidence(library):
    files = {r['file']: r for r in library.manifest['managed_analysis']['files']}
    receipt = json.loads(checked(library.records_root, files['managed/decompilation.json']))
    content = checked(library.records_root, files['managed/Assembly-CSharp.cs'])
    if (not receipt['complete'] or receipt['mode'] != 'whole_module'
            or receipt['input']['sha256'] != ASSEMBLY_HASH
            or receipt['output']['sha256'] != hashlib.sha256(content).hexdigest()
            or receipt['decompiler']['version'] != '11.1.0.9782'):
        raise ValueError('Audio requires the accepted complete managed analysis')
    text = content.decode('utf-8-sig')
    def scope(cls):
        declaration = re.search(r'\bpublic class ' + re.escape(cls) + r'\b', text)
        if declaration is None: raise ValueError(f'Missing audio class: {cls}')
        start = declaration.start()
        end = text.find('\npublic class ', start + 1)
        return start, text[start:end if end >= 0 else len(text)]
    def locate(cls, method):
        start, region = scope(cls)
        matches = list(re.finditer(r'\b(?:void|float|bool|GameObject|IEnumerator)\s+' + re.escape(method) + r'\(', region))
        if not matches: raise ValueError(f'Missing audio method: {cls}.{method}')
        match = matches[0]
        opening = region.index('{', matches[-1].end())
        depth = 1; closing = opening + 1
        while depth:
            depth += (region[closing] == '{') - (region[closing] == '}')
            closing += 1
        return start, match, closing, region
    def evidence(cls, method):
        start, match, closing, _ = locate(cls, method)
        return {'class': cls, 'method': method, 'file': 'managed/Assembly-CSharp.cs',
            'line_start': text[:start + match.start()].count('\n') + 1,
            'line_end': text[:start + closing].count('\n') + 1,
            'sha256': hashlib.sha256(content).hexdigest()}
    if 'new byte[8] { 10, 25, 40, 55, 70, 80, 85, 100 }' not in text:
        raise ValueError('Source audio priority ordering changed')
    # The source has one declaration and no reads of this field.
    if len(re.findall(r'\bmaxAudioCount\b', text)) != 1:
        raise ValueError('Source voice-budget control flow needs a new audit')
    for statement in ('startTime = Time.time;', 'cTime = Time.time - _radio[j].startTime;',
                      'cTime = Time.unscaledTime - _radio[i].startTime;'):
        if statement not in text:
            raise ValueError('Source radio clock needs a new audit')
    for cls, method, statements in HIT_SOURCE:
        _, match, closing, region = locate(cls, method)
        if any(s not in region[match.start():closing] for s in statements):
            raise ValueError(f'Source hit audio needs a new audit: {cls}.{method}')
    _, match, closing, region = locate('interiorscript', 'Start')
    if any(s not in region[match.start():closing] for s in INTERIOR_START):
        raise ValueError('Source interior collider needs a new audit')
    if (any(s not in text for s in STEP_STATEMENTS)
            or not re.search(r'enum walkSoundType\s*\{\s*' + r',\s*'.join(STEP_SURFACES) + r'\s*\}', text)):
        raise ValueError('Source footstep surfaces need a new audit')
    _, region = scope('fpscontroller')
    calls = [m.start() for m in re.finditer(re.escape(STEP_CALL), region)]
    if len(calls) != 2: raise ValueError('Source footstep callers need a new audit')
    # A caller is the last member declaration before the call.
    members = [(m.start(), m.group(1)) for m in re.finditer(r'\n\t[^\s][^\r\n]*?\b(\w+)\([^\r\n]*\)\r?\n\t\{', region)]
    callers = list(dict.fromkeys(max((m for m in members if m[0] < c), default=(0, None))[1] for c in calls))
    if None in callers: raise ValueError('Source footstep callers need a new audit')
    footsteps = [evidence('stepSoundScript', 'Step'), *(evidence('fpscontroller', name) for name in callers)]
    return receipt, evidence, footsteps


def interior_collider(source, reader, data):
    """Source interiorscript.Start: `col`, else the first collider on the GameObject."""
    collider = source.resolve(reader, data['col'])
    if collider is None:
        _, parts = source.components(source.transform_for(source.resolve(reader, data['m_GameObject'])))
        collider = next((c for c in parts if c.type.name in COLLIDER_TYPES), None)
    return collider


def world_interiors(source, prototype, readers, mechanisms, evidence):
    """interiorscript records of one world prototype, in component order. Doors are world mechanisms."""
    index = {key(r): i for i, r in enumerate(readers)}
    colliders = [c['source'] for c in prototype['colliders']]
    doors = {(m['recipe']['source']['file'], m['recipe']['source']['path_id'])
        for m in mechanisms if m['prototype'] == prototype['source']}
    def door(reader, pointer):
        target = source.resolve(reader, pointer)
        if target is None or key(target) not in doors:
            raise ValueError(f'World interior door outside prototype mechanisms: {key(reader)}')
        return source_id(target)
    result = []
    for r in readers:
        d = source.read(r)
        # GetValue reads doorSlots and parentDoorSlot only on a removableDoor interior.
        removable = bool(d['removableDoor'])
        slots = [source.resolve(r, p) for p in d['doorSlots']] if removable else []
        if any(s is None or source.class_name(s) != 'partslotscript' for s in slots):
            raise ValueError(f'World interior door slot is not a partslotscript: {key(r)}')
        parent_slot = source.resolve(r, d['parentDoorSlot']) if removable else None
        invert_slots = [bool(x) for x in d['invertindividual']][:len(slots)] if removable else []
        invert_slots += [False] * (len(slots) - len(invert_slots))
        collider = interior_collider(source, r, d)
        if collider is None or source_id(collider) not in colliders:
            raise ValueError(f'World interior collider outside prototype: {key(r)}')
        parent = source.resolve(r, d['parent'])
        if parent is not None and key(parent) not in index:
            raise ValueError(f'World interior parent outside prototype: {key(r)}')
        door_ids = [door(r, p) for p in d['doors']]
        invert = [bool(x) for x in d['invertindividual2']] or [False] * len(door_ids)
        if len(invert) != len(door_ids): raise ValueError(f'World interior door inversion differs: {key(r)}')
        result.append({'collider': source_id(collider), 'parent': index[key(parent)] if parent else None,
            'invert': bool(d['invert']), 'minimum_doors_to_max': d['minDoorToMax'],
            'maximum_transmission': d['maxAtengedes'], 'use_value': bool(d['useValue']), 'value': d['value'],
            'doors': door_ids, 'invert_doors': invert,
            'door_slots': [source_id(s) for s in slots], 'invert_slots': invert_slots,
            'parent_slot': source_id(parent_slot) if parent_slot is not None else None,
            'parent_door': door(r, d['parentDoor']) if d['parentDoor']['m_PathID'] and not removable else None,
            'method': evidence})
    return result


def collision_record(tiers, methods):
    return {'car_tiers': [{'threshold': t, 'clips': c, 'fixed_gain': 1. if i == 0 else None}
            for i, (t, c) in enumerate(zip(TIER_THRESHOLDS, tiers, strict=True))],
        'interior_check_seconds': 10., 'interior_radius': .1, 'loose_multiplier': .3,
        'item_minimum': .075, 'car_multiplier': .3, 'method': methods}


def pickup_hit_sources(records):
    """Pickupable source, clip source, and gain of each world pickup with a hit clip."""
    for r in records:
        if r['class_name']!='pickupable':continue
        f={x['name']:x['value'] for x in r['fields']}
        if f['collideSound']['kind']=='link':yield r['source'],f['collideSound']['value'],f['collisionSoundM']['value']


def footstep_record(main, step, local_position, clips, methods):
    if step['feetSens']: raise ValueError('Player foot sensors need a source route')
    return {'surfaces': [{'name': n, 'volume': s['volume'], 'clips': clips(s['clips'])}
            for n, s in zip(STEP_SURFACES, main['walkSounds'], strict=True)],
        'step_gain': step['stepSound'], 'ray_distance': step['rayDist'], 'ray_mask': main['floorLayerMask']['m_Bits'],
        'local_position': local_position, 'sand_layers': [17, 19], 'method': methods}


def ambience_record(prototype, d, clips, evidence):
    if d['SSource']['m_PathID']: raise ValueError('Rock ambience source needs a looping route')
    if not d['findNap']: raise ValueError('Rock ambience needs the day cycle phase')
    if not d['same']: raise ValueError('Rock ambience needs one clip set')
    return {'prototype': prototype, 'clips': clips(d['clipsReggel']),
        'phases': {name: [d['minLerp' + suffix], d['maxLerp' + suffix]] for name, flag, suffix in PHASES if d[flag]},
        'interval': [d['minTime'], d['maxTime']], 'chance': d['chance'], 'gain': [d['minVolume'], d['maxVolume']],
        'pitch': [1., 1.], 'priority': PRIORITIES[7], 'method': evidence}


def zone_record(prototype, zone, audio_source, pose, clip, evidence):
    """Source ambienceZone.Upd plays a 2D loop with gain volume * InverseLerp(maxDist, minDist, distance)."""
    if (not audio_source['Loop'] or audio_source['m_PlayOnAwake']
            or any(k['value']!=0 for k in audio_source['panLevelCustomCurve']['m_Curve'])):
        raise ValueError('Ambience zone source needs a 2D loop that the zone starts')
    return {'prototype': prototype, 'pose': pose, 'min_distance': zone['minDist'], 'max_distance': zone['maxDist'],
        'volume': zone['volume'], 'clip': clip, 'method': evidence}


def source_settings(d, priority):
    """AudioSource values that a browser voice slot reads."""
    return {'priority':priority,'loop':bool(d['Loop']),'gain':d['m_Volume'],'pitch':d['m_Pitch'],
        'min_distance':d['MinDistance'],'max_distance':d['MaxDistance'],'rolloff_mode':d['rolloffMode'],
        'spatial_blend':d['panLevelCustomCurve']['m_Curve'][0]['value'],
        'spatial_blend_curve':audio_curve(d['panLevelCustomCurve']),'rolloff_curve':audio_curve(d['rolloffCustomCurve'])}


def audio_curve(value):
    return {'keys': [{'time': k['time'], 'value': k['value'], 'in_tangent': k['inSlope'],
        'out_tangent': k['outSlope'], 'in_weight': k['inWeight'], 'out_weight': k['outWeight'],
        'weighted_mode': k['weightedMode']} for k in value['m_Curve']],
        'pre_wrap_mode': value['m_PreInfinity'], 'post_wrap_mode': value['m_PostInfinity'],
        'rotation_order': value['m_RotationOrder']}


def radio_voice_groups(templates):
    """Group world radios by equal SS, SC and SC2 settings. `templates` is (radioscript, voices) pairs."""
    strip=lambda t:[{k:v for k,v in voice.items() if k!='source'} for voice in t]
    groups=[]
    for identity,voices in templates:
        group=next((g for g in groups if strip(g['voices'])==strip(voices)),None)
        if group is None:groups.append({'sources':[identity],'voices':voices})
        else:group['sources'].append(identity)
    return groups


def car_voice(templates, record):
    """Id of a car voice template in `templates`. Equal templates share one id."""
    for t in templates:
        if {k:v for k,v in t.items() if k!='id'}==record:return t['id']
    templates.append({'id':len(templates)+1,**record})
    return templates[-1]['id']


# AudioSources that the car sound records own. Other voice lists skip them.
CAR_SOURCES={'enginescript':('SAlap','SMotor'),'carscript':('SHornSound','SSounds','CoolantBoil'),
    'talajnezo':('GroundSource','SpringSource','SlideSource')}


def sound_stimulus(source, readers, owner):
    """The stimulscript whose CheckAudioSources reads exactly `readers`, with its position in the `owner` frame."""
    found=getattr(source,'stimuli_by_audio',{}).get(frozenset(key(r) for r in readers))
    if found is None:return None
    reader,transform=found
    return {'offset':relative_pose(source,transform,owner)['translation'],'max_sound_value':source.read(reader)['maxSoundValue']}


def item_root(source, transform):
    """The nearest Transform at or above `transform` whose GameObject has a tosaveitemscript."""
    t=transform
    while t is not None:
        game_object=source.resolve(t,source.read(t)['m_GameObject'])
        if any(c.type.name=='MonoBehaviour' and source.class_name(c)=='tosaveitemscript'
               for c in (source.resolve(game_object,p['component']) for p in source.read(game_object)['m_Component'])):
            return t
        t=source.resolve(t,source.read(t)['m_Father'])
    raise ValueError(f'No item root above {key(transform)}')


def engine_audio(source, reader, sound, shot):
    """enginescript sounds of one engine part, in the frame of its GameObject.

    `sound(audio_source, owner, priority)` and `shot(owner_reader, clip_pointer, gain, audio_source, owner)`
    add car voice templates.
    """
    d=source.read(reader);owner=source.transform_for(source.resolve(reader,d['m_GameObject']))
    voices={};loops=[]
    for name,field in (('idle','SAlap'),('motor','SMotor')):
        r=source.resolve(reader,d[field])
        if r is None:raise ValueError(f'enginescript {field} is null: {source_id(reader)}')
        voices[name]=sound(r,owner,PRIORITIES[5]);loops.append(r)
    return {**voices,'stimulus':sound_stimulus(source,loops,owner),'starter':shot(reader,d['SStart'],d['SStartVolume'],None,owner),
        'stop':shot(reader,d['SStop'],d['SStopVolume'],None,owner),
        'idle_rpm':[d['SMinAlapRpm'],d['SMaxAlapRpm']],'motor_rpm':[d['SMinMotorRpm'],d['SMaxMotorRpm']],
        'idle_gain':d['SMaxAlapSound'],'motor_gain':d['SMaxMotorSound'],
        'idle_pitch':[d['minAlapPitch'],d['maxAlapPitch']],'motor_pitch':[d['minMotorPitch'],d['maxMotorPitch']]}


def car_audio(source, car_reader, wheels, cycle, sound, shot):
    """carscript horn and switch sounds and talajnezo wheel sounds, in the chassis (`RB`) frame.

    `wheels` holds the GroundSource, SpringSource and SlideSource readers of each wheel, in wheel effect order.
    """
    d=source.read(car_reader)
    body=source.resolve(car_reader,d['RB'])
    owner=source.transform_for(source.resolve(body,source.read(body)['m_GameObject']))
    horn=source.resolve(car_reader,d['SHornSound'])
    switch=source.resolve(car_reader,d['SSounds'])
    boil=source.resolve(car_reader,d['CoolantBoil'])
    return {'horn':None if horn is None else {'sound':sound(horn,owner,None),'stimulus':sound_stimulus(source,[horn],owner),
            'pitch':[d['HornPitchMin'],d['HornPitchMax']],'temperature':[cycle['tempNights'][0],cycle['tempDays'][0]],
            'attack_release_rate':10.},
        'switch':None if switch is None else shot(car_reader,d['turnedRadioSOund'],1.,switch,owner),
        'boil':None if boil is None else {'sound':sound(boil,owner,None),'max_gain':d['coolantBoilMaxSound']},
        'wheels':[{field:sound(w[field],owner,None) for field in ('ground','spring','slide')} for w in wheels],
        'route':None}


def car_route(source, reader, interiors, mixer):
    """audioBasedOnInteriorsensscript of a car prototype. `interiors` are the prototype interiorscript readers."""
    d=source.read(reader)
    index={key(r):i for i,r in enumerate(interiors)}
    result=[]
    for p in d['defInterior']:
        target=source.resolve(reader,p)
        if target is None or key(target) not in index:raise ValueError(f'Car audio route interior outside prototype: {key(reader)}')
        result.append(index[key(target)])
    return {'interiors':result,'main_mixer':mixer(source.resolve(reader,d['GMain'])),
        'outside_mixer':mixer(source.resolve(reader,d['GOutSide'])),
        'car_mixer':mixer(source.resolve(reader,d['GCarSounds'])) if d['GCarSounds']['m_PathID'] else None}


def copy_audio(library, identity: ObjectId, clip_id: int, output: Path, usage: str) -> dict:
    path, metadata = library.audio(identity)
    name = f'audio-{identity.file}-{identity.path_id}.wav'
    shutil.copyfile(path, output / name)
    return {'id': clip_id, 'source': {'file':identity.file,'path_id':identity.path_id},
        'path': 'reference/' + name, 'usages':[usage], **metadata}


def recover_audio(source, roots: dict, manifest: dict, library, output: Path) -> dict:
    receipt, method, footstep_methods = managed_evidence(library)
    audio = {'schema_version': 1, 'radio_stimuli': [], 'clips': [], 'emitters': [], 'voices': [], 'car_voices': [], 'mixers': [],
        'bindings': [], 'mechanisms': [], 'fluids': [], 'interiors': [], 'routes': [], 'lamps': [], 'silent_actions': ['pickup', 'detach', 'handbrake'],
        'managed': {'file': receipt['input']['file'], 'sha256': receipt['input']['sha256'],
                    'decompiled_sha256': receipt['output']['sha256']},
        'shared': {'priorities': PRIORITIES, 'random_pitch': [0.85, 1.15],
            'declared_max_audio_count': 32, 'enforced_voice_limit': None,
            'gain_above_one': 'clamp_gain_scale_distances_by_gain_minus_one',
            'destroy_after_source_duration': True, 'method': method('mainscript', 'PlayClipAtPoint')}}
    nodes = {(n['source']['file'], n['source']['path_id']): n for n in manifest['nodes']}
    item_by_node = {i['node']: i['id'] for i in manifest['items']}
    def item_for(node):
        while node:
            if node['id'] in item_by_node: return item_by_node[node['id']]
            parent = node['parent']; node = next((n for n in nodes.values() if n['id'] == parent), None)
        return None
    clips = {}; emitters = {}; voices = {}; groups = {}
    def clip(reader, usage='effect'):
        if reader is None: raise ValueError('Required audio clip pointer is null')
        identity = key(reader)
        if identity not in clips:
            clips[identity] = len(audio['clips']) + 1
            audio['clips'].append(copy_audio(library,ObjectId(*identity),clips[identity],output,usage))
        elif usage not in audio['clips'][clips[identity]-1]['usages']:
            audio['clips'][clips[identity]-1]['usages'].append(usage)
        return clips[identity]
    def emitter(reader):
        go = source.resolve(reader, source.read(reader)['m_GameObject'])
        transform = source.transform_for(go); identity = key(transform)
        if identity not in nodes: raise ValueError(f'Audio emitter outside scene nodes: {identity}')
        if identity not in emitters:
            node = nodes[identity]; emitters[identity] = len(audio['emitters']) + 1
            audio['emitters'].append({'id': emitters[identity], 'node': node['id'],
                'item': item_for(node), 'source': source_id(transform)})
        return emitters[identity]
    def mixer(reader):
        if reader is None: raise ValueError('Required audio mixer pointer is null')
        identity = key(reader)
        if identity not in groups:
            groups[identity] = len(groups) + 1
        return groups[identity]
    components = []
    for readers in roots.values():
        for root in readers:
            for node in source.hierarchy(root): components.extend(node['components'])
    main_r = source.get('level1', 64919); main = source.read(main_r)
    prefab_go = source.resolve(main_r, main['Audio'])
    _, prefab_components = source.components(source.transform_for(prefab_go))
    prefab = next(c for c in prefab_components if c.type.name == 'AudioSource')
    manager = source.get('globalgamemanagers', 4)
    manager_data = source.read(manager)
    audio['shared']['audio_manager'] = {'source': source_id(manager), 'real_voices': manager_data['m_RealVoiceCount'],
        'virtual_voices': manager_data['m_VirtualVoiceCount'], 'virtualize_effects': manager_data['m_VirtualizeEffects'],
        'doppler_factor': manager_data['Doppler Factor'], 'rolloff_scale': manager_data['Rolloff Scale']}
    audio['shared']['source'] = source_id(prefab)
    prefab_data = source.read(prefab)
    curve=audio_curve
    def voice(reader, evidence=None):
        identity = key(reader)
        if identity in voices:
            for value in evidence or []:
                if value not in audio['voices'][voices[identity]-1]['methods']:
                    audio['voices'][voices[identity]-1]['methods'].append(value)
            return voices[identity]
        v = {'id': len(audio['voices']) + 1, 'source': source_id(reader), 'emitter': emitter(reader),
            **voice_settings(reader), 'methods': evidence or []}
        voices[identity] = v['id']; audio['voices'].append(v); return v['id']
    def voice_settings(reader):
        d = source.read(reader); cid = source.resolve(reader, d['m_audioClip'])
        return {'clip': clip(cid) if cid else None, 'authored_clip':clip(cid) if cid else None,
            'authored_priority':d['Priority'],'authored_loop':bool(d['Loop']), 'mixer': mixer(source.resolve(reader, d['OutputAudioMixerGroup'])),
            'priority': d['Priority'], 'loop': bool(d['Loop']), 'gain': d['m_Volume'], 'pitch': d['m_Pitch'],
            'spatial_blend': d['panLevelCustomCurve']['m_Curve'][0]['value'], 'pan': d['Pan2D'],
            'min_distance': d['MinDistance'], 'max_distance': d['MaxDistance'], 'rolloff_mode': d['rolloffMode'],
            'doppler': d['DopplerLevel'], 'play_on_awake': bool(d['m_PlayOnAwake']),
            'mute': bool(d['Mute']), 'enabled': bool(d['m_Enabled']),
            'curves': {name: curve(d[field]) for name, field in [('rolloff', 'rolloffCustomCurve'),
                ('spatial_blend', 'panLevelCustomCurve'), ('spread', 'spreadCustomCurve'),
                ('reverb_mix', 'reverbZoneMixCustomCurve')]}}
    mono = [(c, source.class_name(c), source.read(c)) for c in components if c.type.name == 'MonoBehaviour']
    car_sources = {key(r) for c, cls, d in mono for field in CAR_SOURCES.get(cls, ())
        if (r := source.resolve(c, d[field])) is not None}
    for c in components:
        if c.type.name == 'AudioSource' and key(c) not in car_sources: voice(c)
    # fpscontroller.pointKefe slide voice. The player root is not an audio root.
    point = manifest['conditions']['player_brush']['tool']['voice_source']
    if point: voice(source.get(point['file'], point['path_id']),
        [method('objSlideOnColliderScript', 'Update'), method('objSlideOnColliderScript', 'Disable')])
    # AirPlaneSpawnScript.Update spawns the sky prefabs. Their loop sources play on awake.
    for prefab in manifest['sky']['prefabs']:
        prefab['voices'] = [voice(source.get(v['file'], v['path_id']), [method('AirPlaneSpawnScript', 'Update')]) for v in prefab['voices']]
    # randomFlyer.Sound plays a random clip at the bat and follows it.
    flyer = manifest['sky']['flyer']
    flyer['clips'] = [clip(source.get(c['file'], c['path_id'])) for c in flyer['clips']]
    audio['shared']['settings'] = source_settings(prefab_data, prefab_data['Priority'])
    audio['shared']['mixer'] = mixer(source.resolve(main_r, main['defGroup']))
    def offset(reader, owner):
        transform = source.transform_for(source.resolve(reader, source.read(reader)['m_GameObject']))
        return relative_pose(source, transform, owner)['translation']
    def car_template(reader, priority, one_shot):
        d = source.read(reader); cid = source.resolve(reader, d['m_audioClip'])
        return car_voice(audio['car_voices'], {'source': source_id(reader), 'one_shot': one_shot,
            'clip': clip(cid) if cid else None, 'settings': source_settings(d, d['Priority'] if priority is None else priority),
            'mixer': mixer(source.resolve(reader, d['OutputAudioMixerGroup'])), 'doppler': d['DopplerLevel'], 'pan': d['Pan2D']})
    def car_sound(reader, owner, priority):
        return {'voice': car_template(reader, priority, False), 'position': offset(reader, owner)}
    def car_shot(owner_reader, pointer, gain, reader, owner, priority=PRIORITIES[2]):
        """A clip played once: on AudioSource `reader`, else through mainscript.PlayClipAtPoint at `owner`."""
        target = source.resolve(owner_reader, pointer)
        if target is None: return None
        if reader is not None:
            return {'voice': car_template(reader, None, True), 'clip': clip(target), 'gain': gain, 'position': offset(reader, owner)}
        return {'voice': shared_voice(priority), 'clip': clip(target), 'gain': gain, 'position': [0., 0., 0.]}
    def shared_voice(priority):
        """The mainscript.PlayClipAtPoint template at `priority`."""
        return car_voice(audio['car_voices'], {'source': audio['shared']['source'], 'one_shot': True, 'clip': None,
            'settings': {**audio['shared']['settings'], 'priority': priority}, 'mixer': audio['shared']['mixer'],
            'doppler': prefab_data['DopplerLevel'], 'pan': prefab_data['Pan2D']})
    # randomFlyer.Sound plays its clip through mainscript.PlayClipAtPoint at AudioPriorities[3].
    # Loot tool loops: one car voice template for each prototype tool, at its root offset.
    for tool in manifest['conditions']['prototype_tools']:
        (kind,record),=tool['kind'].items()
        voice_source=record['tool']['voice_source']
        if voice_source is None or (kind=='brush' and not record['tool']['has_sound']):continue
        reader=source.get(voice_source['file'],voice_source['path_id'])
        root=source.get(tool['prototype']['file'],tool['prototype']['path_id'])
        record['sound']=car_sound(reader,root,None)
    flyer['voice'] = shared_voice(PRIORITIES[3])
    # nyulscript.Die plays a random Sdie clip through mainscript.PlayClipAtPoint at AudioPriorities[3].
    rabbit = manifest['creatures']['rabbit']
    rabbit['die_clips'] = [clip(source.get(c['file'], c['path_id'])) for c in rabbit['die_clips']]
    rabbit['die_voice'] = shared_voice(PRIORITIES[3])
    # nyulscript Roar, Attack and the idle growl also play through mainscript.PlayClipAtPoint at AudioPriorities[3].
    evil = rabbit['evil']
    for field in ('roar_clips', 'idle_clips', 'attack_clips'):
        evil[field] = [clip(source.get(c['file'], c['path_id'])) for c in evil[field]]
    def binding(owner, trigger, pointers, gain, pitch, priority, evidence, route=None, voice_id=None):
        choices = [clip(source.resolve(owner, p)) for p in pointers if p['m_PathID']]
        if not choices: return None
        record = {'id': len(audio['bindings']) + 1, 'source': source_id(owner), 'trigger': trigger,
            'emitter': emitter(owner), 'clips': choices, 'gain': gain, 'pitch_range': pitch,
            'priority': priority, 'mixer': route or audio['shared']['mixer'], 'voice': voice_id, 'method': evidence}
        audio['bindings'].append(record); return record['id']
    car_group = None
    for c, cls, d in mono:
        if cls == 'usablescript':
            movement = None
            if d['moveSound'] and d['SSource']['m_PathID'] and d['SMove']['m_PathID']:
                movement = voice(source.resolve(c, d['SSource']), [method(cls, 'RotMoveSound'), method(cls, 'SlideMoveSound')])
                v = audio['voices'][movement-1]; v['clip'] = clip(source.resolve(c,d['SMove']));v['loop'] = False
                v['pitch'] = 1
            entry = {'source': source_id(c), 'emitter': emitter(c), 'movement_voice': movement,
                'audio_multiplier': d['audioM'], 'slide_multiplier': d['audioMSlide'] or 1,
                'rotation_gain': 0.1, 'slide_gain': 2, 'gain_smoothing': 3, 'slide_stop_seconds': 0.2,
                'squeak_chance': d['dontSqueakChanceOverride'] if d['useDontSqueakChanceOverride'] else 0.75,
                'pitch_range': [0.95,1.05], 'endpoints': []}
            for field in ['startPosEnd','endPosEnd','startPosStart','endPosStart']:
                bid = binding(c, field, [d[field]], d['audioM'], [0.85,1.15], PRIORITIES[4], method(cls,'StartEndStuff'))
                if bid: entry['endpoints'].append(bid)
            entry['turn_binding'] = binding(c,'turn', d['turnClips'],d['turnVolume'],[0.85,1.15],PRIORITIES[4],method(cls,'Turn'))
            if movement or entry['endpoints'] or entry['turn_binding']:audio['mechanisms'].append(entry)
        elif cls == 'pickupable' and d['collideSound']['m_PathID']:
            binding(c,'crafted', [d['collideSound']],0.25,[0.85,1.15],PRIORITIES[4],method(cls,'Crafted'))
        elif cls == 'tankcapscript':
            fill_voice = voice(source.resolve(c,d['SSource']), [method(cls,'PlayFillSound'),method(cls,'Update')]) if d['SSource']['m_PathID'] else None
            audio['fluids'].append({'source':source_id(c),'emitter':emitter(c),'fill_voice':fill_voice,
                'max_gain':d['maxVolume'],'tune_speed':d['tuneSpeed'],'fluid_tune_speed':main['fluidTuneSpeed'],
                'pour_speed_min':main['minPourSpeedSound'],'pour_speed_max':main['maxPourSpeedSound'],
                'fill_loop':False,'fill_latch_steps':3, 'open_binding':binding(c,'cap_open',[d['openSound']],d['openVolume'],[1,1],PRIORITIES[4],method(cls,'USE')),
                'close_binding':binding(c,'cap_close',[d['closeSound']],d['closeVolume'],[1,1],PRIORITIES[4],method(cls,'USE')),
                'tune_binding':binding(c,'cap_tune',[d['tekerSound']],1,[1,1],PRIORITIES[4],method(cls,'TUNE')),
                'method':method(cls,'Update')})
        elif cls == 'flashlightscript':
            control=next((v['id'] for v in manifest['lighting']['controllers'] if v['source']==source_id(c)),None)
            lamp_voices=[]
            for lamp in d['Lamps']:
                for pointer in lamp['Sources']:
                    lamp_voices.append(voice(source.resolve(c,pointer),[method(cls,'Refresh')]))
            on=binding(c,'lamp_on',[d['sound']],d['volume'],[0.85,1.15],PRIORITIES[4],method(cls,'TurnOn'))
            off=binding(c,'lamp_off',[d['sound']],d['volume'],[0.85,1.15],PRIORITIES[4],method(cls,'TurnOff'))
            if lamp_voices or on or off:
                if control is None:raise ValueError('Lamp audio lacks selected light controller')
                audio['lamps'].append({'source':source_id(c),'control':control,'voices':lamp_voices,
                    'on_binding':on,'off_binding':off,'method':method(cls,'Refresh')})
    # radioscript settings and stations of the car prototype radios. Every world radio uses them.
    car_radios=[c for p in manifest['world']['prototypes'] if p.get('vehicle')
        for node in source.hierarchy(source.get(p['source']['file'],p['source']['path_id']),activate_root=True) for c in node['components']
        if c.type.name=='MonoBehaviour' and source.class_name(c)=='radioscript']
    if not car_radios:raise ValueError('The car prototypes require a radio')
    def radio_constants(reader):
        data=source.read(reader)
        return {k:data[k] for k in ['AMmin','AMmax','FMmin','FMmax','AMStep','FMStep','vSpeed','turnVolume']}
    constants=radio_constants(car_radios[0])
    for reader in car_radios:
        if radio_constants(reader)!=constants:raise ValueError('{}:{} car radio constants differ'.format(*key(reader)))
    audio['radio'] = {'tuning_repeat_seconds':0.2,'holiday_ranges':[[10,15,11,15],[12,15,1,15]],
        'constants':constants,
        'fm_tolerance':main['FRTFM'],'am_tolerance':main['FRTAM'],'electronics_cutoff':0.2,
        'time_correction_seconds':0.5,'late_seek_margin_seconds':0.1,'custom_gain':0.5,
        'clock':{'kind':'scaled','selected':method('mainscript','RADIO'),
            'music_start':method('radioCH','SelectRandomMusic'),
            'talk_start':method('radioCH','SelectRandomTalk'),
            'multiplayer':method('mainscript','RADIO2')},
        'stations':station_records(source,main_r,main,lambda reader:clip(reader,'radio')),'methods':[method('radioscript','SetRadio'),method('radioscript','Update'),method('mainscript','Update'),
            method('radioCH','SelectRandomMusic'),method('radioCH','SelectRandomTalk'),method('radioCH','AdvanceRadio'),
            method('fpscontroller','Update'),method('electronicsscript','Start'),method('electronicsscript','UpdOutPut'),method('mainscript','CheckHoliday')]}
    main=source.get('level1',64919);md=source.read(main)
    player=source.get('level1',80125);pd=source.read(player)
    prefab=next(c for c in source.components(source.transform_for(source.resolve(main,md['Audio'])))[1] if c.type.name=='AudioSource')
    sounds=source.get('level1',70770)
    if source.class_name(sounds)!='soundMainScript':raise ValueError('Toilet source differs')
    def readers(owner,pointers):return [source.resolve(owner,p) for p in pointers]
    def unique(values):return list({key(r):r for r in values}.values())
    survival_groups=[('eat',0,main,readers(main,md['soundEat']),prefab,[md['eatVolume']]*2,[1.,1.]),
        ('eat_crunchy',10,main,readers(main,md['soundEatCrunchy']),prefab,[md['eatCrunchyVolume']]*2,[1.,1.])]
    for role,kind,field,group in zip(('burp','fart','shit'),(1,2,9),('SBurp','SFart','SFart'),source.read(sounds)['clips'],strict=True):
        prefix='Burp' if role=='burp' else 'Fart'
        survival_groups.append((role,kind,sounds,readers(sounds,group['clips']),source.resolve(player,pd[field]),[pd['min'+prefix+'Volume'],pd['max'+prefix+'Volume']],[pd['min'+prefix+'Pitch'],pd['max'+prefix+'Pitch']]))
    survival_groups.append(('death',5,main,readers(main,[md['SdeathBell']]),source.resolve(player,pd['SDeathBellSource']),[1.,1.],[.8,1.2]))
    cycle=source.resolve(main,md['napszak']);cd=source.read(cycle)
    storm=source.get('sharedassets1.assets',158276)
    if source.class_name(storm)!='sandstormscript':raise ValueError('Storm audio source differs')
    for role,kind,owner,pointer in [('sleep',4,player,pd['SleepCamSource']),('horror',24,player,pd['Shorror']),('storm',6,storm,source.read(storm)['source']),('storm_inside',16,cycle,cd['sandStormSource']),('ambient_day',7,cycle,cd['DayAmbientSound'][0]),('ambient_night',8,cycle,cd['NightAmbientSound'][0])]:
        selected_source=source.resolve(owner,pointer);ad=source.read(selected_source)
        survival_groups.append((role,kind,selected_source,readers(selected_source,[ad['m_audioClip']]),selected_source,[ad['m_Volume']]*2,[ad['m_Pitch']]*2))
    tiers=[readers(main,md[f'collide{i}']) for i in range(4)]
    audio['collisions']=collision_record([[clip(r) for r in tier] for tier in tiers],
        [method('pickupable','OnCollisionEnter'),method('carscript','OnCollisionEnter'),method('pickupable','FixedUpdate')])
    item_clips=[]
    for record in [*manifest['items'],*(x for p in manifest['item_prototypes'] for x in p['pickups'])]:
        if record['collision_clip']:item_clips.append(source.get(record['collision_clip']['file'],record['collision_clip']['path_id']))
    for item in manifest['items']:
        if item['collision_clip']:item['collision_clip']=clip(source.get(item['collision_clip']['file'],item['collision_clip']['path_id']))
    hits=list(pickup_hit_sources(manifest['world']['records']))
    world_clips=[source.get(c['file'],c['path_id']) for _,c,_ in hits]
    if any(c.type.name!='AudioClip' for c in world_clips):raise ValueError('Invalid world pickup collision clip')
    item_clips+=world_clips
    # The runtime finds the hit sound of a generated item by its pickupable source.
    manifest['world']['pickup_hits']=[{'source':s,'collision_clip':clip(c),'collision_gain':g} for (s,_,g),c in zip(hits,world_clips)]
    audio['ambience']=[];rocks=[];audio['ambience_zones']=[];zones=[];sleds=[]
    for prototype in manifest['world']['prototypes']:
        interiors=[];root=source.get(prototype['source']['file'],prototype['source']['path_id'])
        for node in source.hierarchy(root,activate_root=True):
            for c in node['components']:
                if c.type.name!='MonoBehaviour':continue
                if source.class_name(c)=='interiorscript':interiors.append(c)
                if source.class_name(c)=='ambienceZone':zones.append((prototype['source'],root,node['reader'],c))
                if source.class_name(c)!='soundplayscript':continue
                rocks.append(c)
                audio['ambience'].append(ambience_record(prototype['source'],source.read(c),
                    lambda pointers,c=c:[clip(r) for r in readers(c,pointers)],method('soundplayscript','PlaySound')))
        prototype['interiors']=world_interiors(source,prototype,interiors,manifest['world']['mechanisms'],method('interiorscript','GetValue'))
        for engine in prototype.get('engines',[]):
            engine['audio']=engine_audio(source,source.get(engine['source']['file'],engine['source']['path_id']),car_sound,car_shot)
        if prototype.get('vehicle'):
            vehicle=prototype['vehicle'];car=source.get(vehicle['source']['file'],vehicle['source']['path_id'])
            group=source.resolve(car,source.read(car)['groupScript'])
            # Cars may have their own groupScript, or none. The car hit row needs one GCarSounds mixer group.
            if group is not None:
                hits=source.resolve(group,source.read(group)['GCarSounds'])
                if car_group is not None and key(hits)!=key(car_hits):raise ValueError(f'Car hit audio routes differ: {key(car)}')
                car_group,car_hits=group,hits
            wheels=recover_wheel_voices(source,root,[source.get(w['source']['file'],w['source']['path_id']) for w in vehicle['wheels']])
            vehicle['audio']=car_audio(source,car,[{f:source.get(v['file'],v['path_id']) for f,v in w.items()} for w in wheels],cd,car_sound,car_shot)
            vehicle['audio']['doors']=door_audio(source,root,car,lambda r,p:car_shot(r,p,1.,None,None,PRIORITIES[4]))
            if vehicle['blood_clip']:vehicle['blood_clip']=clip(source.get(vehicle['blood_clip']['file'],vehicle['blood_clip']['path_id']))
            routes=[c for node in source.hierarchy(root,activate_root=True) for c in node['components']
                if c.type.name=='MonoBehaviour' and source.class_name(c)=='audioBasedOnInteriorsensscript' and source.read(c)['def']['m_PathID']]
            if len(routes)>1:raise ValueError(f'Car prototype has more than one audio route: {key(car)}')
            if routes:vehicle['audio']['route']=car_route(source,routes[0],interiors,mixer)
        if prototype['sled']:
            sled=prototype['sled'];sleds.append(sled)
            sled['clip']=clip(source.get(sled['clip']['file'],sled['clip']['path_id']))
    if len(rocks)!=1:raise ValueError('Selected world needs one rock ambience')
    if car_group is None:raise ValueError('Car hit audio needs a car prototype route')
    rock=audio['ambience'][0]
    survival_groups+=[('item_hit',11,main,unique(item_clips),prefab,[1.,1.],[1.,1.]),
        ('car_hit',12,main,unique(r for tier in tiers for r in tier),prefab,[1.,1.],list(audio['shared']['random_pitch'])),
        ('rock_heat',13,rocks[0],readers(rocks[0],source.read(rocks[0])['clipsReggel']),prefab,rock['gain'],rock['pitch'])]
    # meleeweaponscript plays mainscript.meleeHitSounds; the runtime picks the group and its hitVolume gain.
    survival_groups.append(('melee_hit',17,main,unique(source.get(c['file'],c['path_id']) for group in manifest['melee']['hit_clips'] for c in group),prefab,[1.,1.],[1.,1.]))
    # breakablescript.Break plays one of its clips through mainscript.PlayClipAtPoint at AudioPriorities[6].
    break_clips=unique(source.get(c['file'],c['path_id']) for b in manifest['breakables'] for c in b['clips'])
    survival_groups.append(('break',22,main,break_clips,prefab,[1.,1.],[1.,1.]))
    # metalDetectorScript.Source loops one beep; UpdSound sets its pitch and volume each frame.
    detectors=[source.get(r['source']['file'],r['source']['path_id']) for r in manifest['world']['records'] if r['class_name']=='metalDetectorScript']
    beeps=[source.resolve(r,source.read(r)['Source']) for r in detectors]
    if not beeps:raise ValueError('Missing metal detector prefabs')
    def beep(reader):
        d=source.read(reader);return (d['Loop'],d['m_Volume'],d['m_Pitch'],d['MinDistance'],d['MaxDistance'],d['rolloffMode'],key(source.resolve(reader,d['m_audioClip'])))
    if any(b is None for b in beeps) or not beep(beeps[0])[0] or any(beep(b)!=beep(beeps[0]) for b in beeps):raise ValueError('Metal detector beeps differ')
    survival_groups.append(('metal_detector',18,detectors[0],readers(beeps[0],[source.read(beeps[0])['m_audioClip']]),beeps[0],[1.,1.],[1.,1.]))
    step=source.resolve(player,pd['stepScript'])
    if source.class_name(step)!='stepSoundScript':raise ValueError('Player step source differs')
    from .collision import world_pose,bake_points
    # The step point is in the player body frame, like the player sphere colliders.
    step_transform=source.transform_for(source.resolve(step,source.read(step)['m_GameObject']))
    body=world_pose(source,source.transform_for(source.resolve(player,pd['m_GameObject'])))
    walk=[readers(main,s['clips']) for s in md['walkSounds']]
    audio['footsteps']=footstep_record(md,source.read(step),bake_points([(0.,0.,0.)],source.chain(step_transform),body)[0],
        lambda pointers:[clip(r) for r in readers(main,pointers)],footstep_methods)
    zone_sources=[]
    for prototype,root,node,c in zones:
        d=source.read(c);reader=source.resolve(c,d['Source']);ad=source.read(reader)
        # The pose is in the prototype root frame, like the prototype body poses.
        local=copy.copy(source);count=len(source.chain(root))
        local.chain=lambda transform,count=count:source.chain(transform)[:-count]
        audio['ambience_zones'].append(zone_record(prototype,d,ad,world_pose(local,node),
            clip(source.resolve(reader,ad['m_audioClip'])),method('ambienceZone','Upd')))
        zone_sources.append(reader)
    if zone_sources:
        # One playback row serves every zone, so the zone sources must share their settings.
        shared=lambda r:{k:v for k,v in source.read(r).items() if k not in ('m_GameObject','m_audioClip')}
        if any(shared(r)!=shared(zone_sources[0]) for r in zone_sources):raise ValueError('Ambience zone sources differ')
        first=source.read(zone_sources[0])
        survival_groups.append(('ambience_zone',19,zones[0][3],unique(source.resolve(r,source.read(r)['m_audioClip']) for r in zone_sources),
            zone_sources[0],[first['m_Volume']]*2,[first['m_Pitch']]*2))
    if sleds:
        # szankoScript.Update sets the volume and pitch of its own source each frame. One row serves every sled.
        sled_sources=[source.get(v['audio_source']['file'],v['audio_source']['path_id']) for v in sleds]
        shared=lambda r:{k:v for k,v in source.read(r).items() if k!='m_GameObject'}
        if any(shared(r)!=shared(sled_sources[0]) for r in sled_sources):raise ValueError('Sled sources differ')
        first=source.read(sled_sources[0]);owner=source.get(sleds[0]['source']['file'],sleds[0]['source']['path_id'])
        survival_groups.append(('sled',20,owner,[source.resolve(sled_sources[0],first['m_audioClip'])],sled_sources[0],
            [0.,sleds[0]['sound_max_volume']],[sleds[0]['sound_min_pitch'],sleds[0]['sound_max_pitch']]))
    # radioscript.TurnRadio plays Cturned at the radio through mainscript.PlayClipAtPoint. One row serves every world radio.
    radios=[source.get(r['source']['file'],r['source']['path_id']) for r in manifest['world']['records'] if r['class_name']=='radioscript']
    clicks={(key(source.resolve(r,d['Cturned'])),d['turnVolume']) for r in radios for d in [source.read(r)]}
    if len(clicks)!=1:raise ValueError('World radio clicks differ or are missing')
    (click,turn_volume),=clicks
    survival_groups.append(('radio_click',21,radios[0],[source.get(*click)],prefab,[turn_volume]*2,[0.85,1.15]))
    survival_groups.append(('footstep',15,step,unique(r for group in walk for r in group),prefab,[1.,1.],list(audio['shared']['random_pitch'])))
    default_route=source.resolve(main,md['defGroup'])
    routes={'death':source.resolve(player,pd['deathbellGroup']),'item_hit':default_route,'melee_hit':default_route,'radio_click':default_route,'break':default_route,'rock_heat':default_route,'footstep':default_route,
        'car_hit':car_hits}

    audio['survival']=[]
    for role,kind,owner,clip_readers,reader,gain,pitch in survival_groups:
        d=source.read(reader);one_shot=role in ONE_SHOT
        route=routes[role] if role in routes else source.resolve(reader,d['OutputAudioMixerGroup'])
        transform=source.transform_for(source.resolve(reader,d['m_GameObject']))
        # The sled source moves with its Rigidbody. The runtime passes its position each frame.
        if role!='sled' and any(c.type.name=='Rigidbody' for c in source.components(transform)[1]):raise ValueError('Selected audio velocity needs a Rigidbody route')
        pose=world_pose(source,transform)
        player_pose=world_pose(source,source.transform_for(source.resolve(player,pd['m_GameObject'])))
        offset=[pose['translation'][i]-player_pose['translation'][i] for i in range(3)] if role in ('burp','fart','shit','death','sleep','horror') else [0.,0.,0.]
        settings={'priority':ONE_SHOT_PRIORITY.get(role,PRIORITIES[7]) if one_shot else d['Priority'],'loop':bool(d['Loop']) if role!='death' else False,
            'gain':d['m_Volume'],'pitch':d['m_Pitch'],'min_distance':d['MinDistance'],'max_distance':d['MaxDistance'],
            'rolloff_mode':d['rolloffMode'],'spatial_blend':1. if one_shot else d['panLevelCustomCurve']['m_Curve'][0]['value'],
            'spatial_blend_curve':curve(d['panLevelCustomCurve']),'rolloff_curve':curve(d['rolloffCustomCurve'])}
        if one_shot:
            for k in settings['spatial_blend_curve']['keys']:k['value']=1.
        cls,name={'eat':('mainscript','PlayClipAtPoint'),'eat_crunchy':('mainscript','PlayClipAtPoint'),'burp':('fpscontroller','Burp'),'fart':('fpscontroller','Fart'),'shit':('fpscontroller','Shit'),'death':('fpscontroller','Death2'),'sleep':('mainscript','Update'),'horror':('mainscript','RandomHorrorSound'),
            'item_hit':('pickupable','OnCollisionEnter'),'car_hit':('carscript','OnCollisionEnter'),'rock_heat':('soundplayscript','PlaySound'),'footstep':('stepSoundScript','Step'),'storm':('sandstormscript','Update'),'melee_hit':('meleeweaponscript','Update'),'metal_detector':('metalDetectorScript','UpdSound'),'ambience_zone':('ambienceZone','Upd'),'sled':('szankoScript','Update'),'radio_click':('radioscript','TurnRadio'),'break':('breakablescript','Break')}.get(role,('napszakvaltakozas','FixedUpdate'))
        audio['survival'].append({'role':role,'kind':kind,'one_shot':one_shot,'source':source_id(owner),'audio_source':source_id(reader),
            'clips':[clip(r) for r in clip_readers], 'gain':gain,'pitch':pitch,'offset':offset,
            'settings':settings,'mixer':mixer(route),'doppler':d['DopplerLevel'],'pan':d['Pan2D'],'method':method(cls,name)})
    # musicboxscript.PlayNote adds a default AudioSource for each note.
    boxes=[r['source'] for r in manifest['world']['records'] if r['class_name']=='musicboxscript']
    notes=[]
    for identity in boxes:
        r=source.get(identity['file'],identity['path_id']);d=source.read(r)
        notes.append((clip(source.resolve(r,d['clip'])),d['volume'],mixer(source.resolve(r,d['group']))))
    if not notes or any(n!=notes[0] for n in notes):raise ValueError('Music box notes differ or are missing')
    flat={'keys':[{'time':0.,'value':1.,'in_tangent':0.,'out_tangent':0.,'in_weight':1/3,'out_weight':1/3,'weighted_mode':0}],
        'pre_wrap_mode':2,'post_wrap_mode':2,'rotation_order':4}
    audio['survival'].append({'role':'music_box','kind':14,'one_shot':True,'source':boxes[0],'audio_source':boxes[0],
        'clips':[notes[0][0]],'gain':[notes[0][1]]*2,'pitch':[1.,1.],'offset':[0.,0.,0.],
        'settings':{'priority':128,'loop':False,'gain':1.,'pitch':1.,'min_distance':1.,'max_distance':500.,'rolloff_mode':0,
            'spatial_blend':1.,'spatial_blend_curve':flat,'rolloff_curve':flat},
        'mixer':notes[0][2],'doppler':1.,'pan':0.,'method':method('musicboxscript','PlayNote')})
    # computerscript.PlayNote adds one looped 3D AudioSource for each held note.
    computer=source.get('sharedassets1.assets',133932);cd=source.read(computer)
    if source.class_name(computer)!='computerscript':raise ValueError('Computer audio source differs')
    audio['survival'].append({'role':'computer_note','kind':23,'one_shot':False,'source':source_id(computer),'audio_source':source_id(computer),
        'clips':[clip(source.resolve(computer,cd['musicClip']))],'gain':[cd['musicVolume']]*2,'pitch':[1.,1.],'offset':[0.,0.,0.],
        'settings':{'priority':128,'loop':True,'gain':1.,'pitch':1.,'min_distance':1.,'max_distance':500.,'rolloff_mode':0,
            'spatial_blend':1.,'spatial_blend_curve':flat,'rolloff_curve':flat},
        'mixer':mixer(source.resolve(computer,cd['musicGroup'])),'doppler':1.,'pan':0.,'method':method('computerscript','PlayNote')})
    def control_reader(reader):
        if reader is None: return None
        eid = emitter(reader); node = audio['emitters'][eid-1]['node']
        mid = next((m['id'] for m in manifest['mechanisms'] if m['source']==source_id(reader)),None)
        if mid is None: raise ValueError(f'Interior door lacks mechanism: {key(reader)}')
        return {'source':source_id(reader),'node':node,'mechanism':mid}
    def sensor(reader, player=False):
        d=source.read(reader);go=source.resolve(reader,d['m_GameObject']);tr=source.transform_for(go)
        sd={'source':source_id(reader),'transform_source':source_id(tr),'node':nodes[key(tr)]['id'] if key(tr) in nodes else None,
            'player':player,'initial_sensed':bool(d['sensed']),'initial_count':d['v'],
            'radius':d.get('radius'),'mask':d.get('LM',{}).get('m_Bits'),
            'method':method('sens2' if 'radius' in d else 'sens','FixedUpdate')}
        if player:
            # This is the source sensor's local pose, not another scene hierarchy.
            td=source.read(tr);sd['local_position']=[td['m_LocalPosition'][axis] for axis in ['x','y','z']]
            sd['parent_source']=source_id(source.resolve(tr,td['m_Father']))
        else:sd['local_position']=None;sd['parent_source']=None
        return sd
    interior_readers=[c for c,cls,d in mono if cls=='interiorscript']
    interior_ids={key(r):i+1 for i,r in enumerate(interior_readers)}
    for r in interior_readers:
        d=source.read(r);eid=emitter(r);node=audio['emitters'][eid-1]['node']
        collider=interior_collider(source,r,d)
        collider_id=next((c['id'] for c in manifest['colliders'] if c['source']==source_id(collider)),None) if collider else None
        if collider_id is None:raise ValueError(f'Interior sensor collider outside selected gameplay: {key(r)}')
        parent=source.resolve(r,d['parent'])
        if parent and key(parent) not in interior_ids:raise ValueError('Interior parent outside selected roots')
        doors=[control_reader(source.resolve(r,p)) for p in d['doors']]
        slots=[]
        for p in d['doorSlots']:
            sr=source.resolve(r,p);slot=next((s for s in manifest['part_slots'] if s['source']==source_id(sr)),None)
            if slot is None:raise ValueError(f'Interior door slot outside gameplay: {key(sr)}')
            slots.append(slot['id'])
        parent_slot=source.resolve(r,d['parentDoorSlot'])
        parent_slot_id=next((s['id'] for s in manifest['part_slots'] if s['source']==source_id(parent_slot)),None) if parent_slot else None
        audio['interiors'].append({'id':interior_ids[key(r)],'source':source_id(r),'node':node,'collider':collider_id,
            'parent':interior_ids[key(parent)] if parent else None,'invert':bool(d['invert']),
            'minimum_doors_to_max':d['minDoorToMax'],'maximum_transmission':d['maxAtengedes'],
            'removable_doors':bool(d['removableDoor']),'use_value':bool(d['useValue']),'value':d['value'],
            'doors':doors,'door_slots':slots,'invert_doors':[bool(x) for x in d['invertindividual2']] or [False]*len(doors),
            'invert_slots':[bool(x) for x in d['invertindividual']] or [False]*len(slots),
            'parent_door':control_reader(source.resolve(r,d['parentDoor'])), 'parent_slot':parent_slot_id,
            'method':method('interiorscript','GetValue')})
    for r,cls,d in mono:
        if cls=='objSlideOnColliderScript' and d['hasSound']:
            voice(source.resolve(r,d['SSource']),[method(cls,'Update'),method(cls,'Disable')])
        elif cls=='sprayscript':
            voice(source.resolve(r,d['Saudio']),[method(cls,'Update'),method(cls,'Fire')])
    for r,cls,d in mono:
        if cls!='audioBasedOnInteriorsensscript':continue
        route_voices=[voice(v,[method(cls,'Update'),method(cls,'SetSources')]) for p in d['Sources']
            if key(v:=source.resolve(r,p)) not in car_sources]
        default_interiors=[]
        for p in d['defInterior']:
            target=source.resolve(r,p)
            if key(target) not in interior_ids:raise ValueError('Audio route interior outside selected roots')
            default_interiors.append(interior_ids[key(target)])
        audio['routes'].append({'source':source_id(r),'voices':route_voices,'interiors':default_interiors,
            'sensor':sensor(source.resolve(r,d['interiorSens'])),
            'main_mixer':mixer(source.resolve(r,d['GMain'])),'outside_mixer':mixer(source.resolve(r,d['GOutSide'])),
            'car_mixer':mixer(source.resolve(r,d['GCarSounds'])) if d['GCarSounds']['m_PathID'] else None,
            'initial_outside':bool(d['outSide']),'initial_player_inside':bool(d['playerInside']),
            'methods':[method(cls,'Update'),method(cls,'SetSources')]})

    # World radios (portable and car radios) share a voice template per equal setting set.
    # The browser keeps a fixed slot pool per template.
    portable=[r['source'] for r in manifest['world']['records'] if r['class_name']=='radioscript']
    templates=[]
    for identity in portable:
        r=source.get(identity['file'],identity['path_id']);d=source.read(r)
        template=[{'id':0,'source':source_id(source.resolve(r,d[f])),'emitter':0,
            **voice_settings(source.resolve(r,d[f])),'methods':[method('radioscript','Start'),method('radioscript','SetRadio')]}
            for f in ['SS','SC','SC2']]
        for v in template[:2]:v['priority']=PRIORITIES[5]
        templates.append((identity,template))
        # SS, SC and SC2 each have a stimulscript, at a position in the radio item frame.
        owner=item_root(source,source.transform_for(source.resolve(r,d['m_GameObject'])))
        for index,f in enumerate(['SS','SC','SC2']):
            stimulus=sound_stimulus(source,[source.resolve(r,d[f])],owner)
            if stimulus is not None:audio['radio_stimuli'].append({'radio':identity,'voice':index,**stimulus})
    if not templates:raise ValueError('Missing portable radio prefabs')
    audio['portable_radios']=radio_voice_groups(templates)
    # Preserve the compiled group graph, snapshot parameters, and native effect identity.
    mixer_sources = {key(source.resolve(source.get(*identity),source.read(source.get(*identity))['m_AudioMixer'])) for identity in groups}
    if len(mixer_sources)!=1:raise ValueError('Selected voices must resolve one source mixer')
    mr=source.get(*next(iter(mixer_sources)));md=source.read(mr);constant=md['m_MixerConstant']
    for obj in source.objects.values():
        if obj.type.name=='AudioMixerGroupController' and key(source.resolve(obj,source.read(obj)['m_AudioMixer']))==key(mr):mixer(obj)
    snapshot=source.resolve(mr,md['m_StartSnapshot']);sd=source.read(snapshot)
    si=constant['snapshotGUIDs'].index(sd['m_SnapshotID']);values=constant['snapshots'][si]['values']
    native=library.snapshot.root/'UnityPlayer.dll'
    with native.open('rb') as stream:
        if hashlib.file_digest(stream,'sha256').hexdigest()!=NATIVE_HASH:raise ValueError('Native audio engine hash changed')
    for identity,gid in groups.items():
        r=source.get(*identity);d=source.read(r);i=constant['groupGUIDs'].index(d['m_GroupID']);g=constant['groups'][i]
        parent=g['parentConstantIndex'];parent_id=None
        if parent>=0:
            parent_id=next(groups[k] for k in groups if source.read(source.get(*k))['m_GroupID']==constant['groupGUIDs'][parent])
        effects=[]
        for fx in constant['effects']:
            if fx['groupConstantIndex']!=i:continue
            if fx['type'] not in [-2,10]:raise ValueError(f"Unsupported required mixer effect: {fx['type']}")
            params=[values[j] for j in fx['parameterIndices']]
            effects.append({'type':fx['type'],'name':'attenuation' if fx['type']==-2 else 'parameq',
                'bypass':fx['bypass'],'parameter_indices':fx['parameterIndices'],'parameters':params,
                'previous_effect':fx['prevEffectIndex'],'send_target':fx['sendTargetEffectIndex'],'wet_mix_index':fx['wetMixLevelIndex']})
        audio['mixers'].append({'id':gid,'source':source_id(r),'controller':source_id(mr),'snapshot':source_id(snapshot),
            'name':d['m_Name'],'parent':parent_id,'gain_db':values[g['volumeIndex']],'pitch':values[g['pitchIndex']],
            'mute':g['mute'],'solo':g['solo'],'bypass_effects':g['bypassEffects'],'effects':effects})
    audio['native']={'file':'UnityPlayer.dll','sha256':NATIVE_HASH,'effect_type':10,
        'registration_va':'0x1814436f0','type_write_va':'0x181443798','coefficient_va':'0x181444ff0',
        'parameter_names':['Center freq','Octave range','Frequency gain'],
        'browser_q_rule':'1 / bandwidth','browser_gain_db_rule':'40 * log10(gain)'}
    audio['mixer_controls']={'source':source_id(mr),'parameter_indices':constant['exposedParameterIndices'],
        'parameter_name_hashes':constant['exposedParameterNames'],
        'parameter_names':['CarSounds','PlayerEar','MasterVolume','Outside','InGame'],
        'methods':[method('mainscript','Update'),method('mainscript','SetVolume'),method('fpscontroller','SetOutSideAudio')]}
    player = source.read(source.get('level1',80125))
    audio['mixer_controls']['outside'] = {'default_interior_gain': player['defInteriorM'],
        'gain_smoothing':player['interiorLerpM'], 'car_sound_multiplier':player['FCarSoundM'],
        'initial_gain':player['outsideVolume'], 'sensor':sensor(source.get('level1',80121),True), 'methods':[method('fpscontroller','SetOutSideAudio')]}
    skid=manifest['skid']
    for field in ('road_clip','dirt_clip'):skid[field]=clip(source.get(skid[field]['file'],skid[field]['path_id']))
    for field in ['radio','portable_radios']:
        if field not in audio:raise ValueError(f'Missing required selected audio: {field}')
    for voice_record in audio['voices']:
        if not voice_record['methods']:raise ValueError(f"Audio voice lacks source control flow: {voice_record['source']}")
    engines=[e['audio'] for p in manifest['world']['prototypes'] for e in p.get('engines',[]) if e.get('audio')]
    horns=[p['vehicle']['audio']['horn'] for p in manifest['world']['prototypes'] if p.get('vehicle') and p['vehicle']['audio']['horn']]
    print(f"Sound stimuli: {sum(e['stimulus'] is not None for e in engines)}/{len(engines)} engines, "
          f"{sum(h['stimulus'] is not None for h in horns)}/{len(horns)} horns, {len(audio['radio_stimuli'])} radio voices",flush=True)
    validate_audio_export(audio,output,manifest)
    for identity in sorted(getattr(source, 'item_clips', set())):
        clip(source.get(*identity), 'effect')
    return audio


def validate_audio_export(audio: dict, output: Path, manifest: dict) -> None:
    clock = audio['radio']['clock']
    if (clock['kind'] != 'scaled'
            or [(clock[name]['class'], clock[name]['method']) for name in
                ('selected', 'music_start', 'talk_start', 'multiplayer')]
            != [('mainscript', 'RADIO'), ('radioCH', 'SelectRandomMusic'),
                ('radioCH', 'SelectRandomTalk'), ('mainscript', 'RADIO2')]):
        raise ValueError('Source radio clock differs from selected single-player clock')
    ids=set()
    for c in audio['clips']:
        if c['id'] in ids:raise ValueError('Duplicate audio clip')
        ids.add(c['id']);name=c['path'].removeprefix('reference/')
        checked(output,{'file':name,'bytes':c['bytes'],'sha256':c['sha256']})
        from .library.audio import validate_wav
        actual=validate_wav(output/name,{'m_Channels':c['channels'],'m_Frequency':c['frequency'],
            'm_BitsPerSample':c['bits_per_sample'],'m_Length':c['source_duration_seconds']})
        if any(c[k]!=v for k,v in actual.items()):raise ValueError('Runtime WAV metadata mismatch')
    emitters = {v['id'] for v in audio['emitters']}
    mixers = {v['id'] for v in audio['mixers']}
    for v in audio['voices']:
        if v['clip'] is not None and v['clip'] not in ids:raise ValueError('Unresolved voice clip')
        if v['authored_clip'] is not None and v['authored_clip'] not in ids:raise ValueError('Unresolved authored voice clip')
        if v['emitter'] not in emitters or v['mixer'] not in mixers:raise ValueError('Unresolved voice owner or mixer')
        if not v['methods'] or any(not m['sha256'] or m['line_start']<=0 for m in v['methods']):raise ValueError('Missing voice method evidence')
        if set(v['curves'])!={'rolloff','spatial_blend','spread','reverb_mix'}:raise ValueError('Missing voice curves')
        # Linear rolloff is defined from distance 0. The airplane source starts there.
        if not math.isfinite(v['min_distance']) or not math.isfinite(v['max_distance']) or not 0<=v['min_distance']<=v['max_distance'] or v['max_distance']<=0 or v['min_distance']==0 and v['rolloff_mode']!=1:
            raise ValueError('Invalid audio distance')
    radios={tuple(sorted(r.items())) for g in audio['portable_radios'] for r in g['sources']}
    for r in audio['radio_stimuli']:
        if (tuple(sorted(r['radio'].items())) not in radios or r['voice'] not in (0,1,2) or len(r['offset'])!=3
                or not all(map(math.isfinite,[*r['offset'],r['max_sound_value']])) or r['max_sound_value']<0):
            raise ValueError('Invalid radio stimulus')
    # The runtime indexes car voice templates by id.
    if [v['id'] for v in audio['car_voices']]!=list(range(1,len(audio['car_voices'])+1)):
        raise ValueError('Car voice ids must run 1..n')
    for v in audio['car_voices']:
        s=v['settings']
        if (v['clip'] is not None and v['clip'] not in ids) or v['mixer'] not in mixers:raise ValueError('Unresolved car voice clip or mixer')
        if not all(map(math.isfinite,(s['gain'],s['pitch'],s['min_distance'],s['max_distance']))) or not 0<s['min_distance']<=s['max_distance']:
            raise ValueError('Invalid car voice settings')
    for b in audio['bindings']:
        if not b['clips'] or any(c not in ids for c in b['clips']):raise ValueError('Unresolved event clip')
        if b['emitter'] not in emitters or b['mixer'] not in mixers:raise ValueError('Unresolved event owner or mixer')
    settings=audio['shared']['settings']
    if (not all(math.isfinite(settings[k]) for k in ('gain','pitch','min_distance','max_distance','spatial_blend'))
            or settings['gain']<0 or settings['pitch']<=0
            or not 0<settings['min_distance']<=settings['max_distance']
            or not 0<=settings['spatial_blend']<=1
            or not 0<=settings['rolloff_mode']<=2):
        raise ValueError('Invalid shared audio source settings')
    for name in ('rolloff_curve','spatial_blend_curve'):
        keys=settings[name]['keys']
        if (not keys or any(not all(math.isfinite(key[field]) for field in
                ('time','value','in_tangent','out_tangent','in_weight','out_weight'))
                or not 0<=key['weighted_mode']<=3 for key in keys)
                or any(a['time']>=b['time'] for a,b in zip(keys,keys[1:]))):
            raise ValueError(f'Invalid shared audio curve: {name}')
    validate_hit_audio(audio, ids, manifest['world']['pickup_hits'])
    validate_storm_audio(audio, ids)
    validate_footsteps(audio, ids, manifest)
    validate_ambience_zones(audio, ids)


def validate_storm_audio(audio: dict, ids: set) -> None:
    for role,kind in (('storm',6),('storm_inside',16)):
        found=[r for r in audio['survival'] if r['role']==role]
        if (len(found)!=1 or found[0]['kind']!=kind or found[0]['one_shot'] or not found[0]['settings']['loop']
                or len(found[0]['clips'])!=1 or set(found[0]['clips'])-ids or found[0]['gain'][0]!=found[0]['gain'][1]):
            raise ValueError(f'Invalid storm audio: {role}')


def validate_hit_audio(audio: dict, ids: set, pickup_hits: list) -> None:
    tiers=audio['collisions']['car_tiers']
    if [t['threshold'] for t in tiers]!=TIER_THRESHOLDS or [t['fixed_gain'] for t in tiers]!=[1.,None,None,None]:
        raise ValueError('Car hit tiers differ from source')
    if any(not t['clips'] or set(t['clips'])-ids for t in tiers):raise ValueError('Unresolved car hit clip')
    roles={r['role']:r for r in audio['survival']}
    if any(r['one_shot']!=(r['role'] in ONE_SHOT) for r in audio['survival']):raise ValueError('Survival one-shot flag differs')
    for role,kind in ONE_SHOT.items():
        r=roles.get(role)
        if r is None or r['kind']!=kind or not r['clips'] or set(r['clips'])-ids or r['settings']['spatial_blend']!=1:
            raise ValueError(f'Invalid one-shot audio: {role}')
        if role!='music_box' and r['settings']['priority']!=ONE_SHOT_PRIORITY.get(role,PRIORITIES[7]):raise ValueError(f'Invalid one-shot priority: {role}')
    if not {c for t in tiers for c in t['clips']}<=set(roles['car_hit']['clips']):raise ValueError('Car hit record lacks tier clips')
    if any(h['collision_clip'] not in roles['item_hit']['clips'] or not h['collision_gain']>=0 for h in pickup_hits):
        raise ValueError('Invalid world pickup hit')
    if len(audio['ambience'])!=1:raise ValueError('Selected world needs one rock ambience')
    for a in audio['ambience']:
        if (not a['clips'] or set(a['clips'])-ids or not set(a['clips'])<=set(roles['rock_heat']['clips'])
                or not a['phases'] or set(a['phases'])-{name for name,_,_ in PHASES}
                or any(not 0<=low<=high<=1 for low,high in a['phases'].values())
                or not 0<=a['interval'][0]<=a['interval'][1] or not 0<=a['chance']<=1
                or not 0<=a['gain'][0]<=a['gain'][1] or a['priority']!=PRIORITIES[7]):
            raise ValueError('Invalid rock ambience')


def validate_ambience_zones(audio: dict, ids: set) -> None:
    rows=[r for r in audio['survival'] if r['kind']==19]
    if len(rows)!=bool(audio['ambience_zones']):raise ValueError('Ambience zones need one playback row')
    for z in audio['ambience_zones']:
        values=[z['min_distance'],z['max_distance'],z['volume'],*z['pose']['translation']]
        if (not all(map(math.isfinite,values)) or not 0<=z['min_distance']<z['max_distance'] or not 0<=z['volume']<=1
                or z['clip'] not in ids or z['clip'] not in rows[0]['clips']):
            raise ValueError('Invalid ambience zone')
    for r in rows:
        if (r['role']!='ambience_zone' or r['one_shot'] or not r['settings']['loop'] or r['settings']['spatial_blend']!=0
                or set(r['clips'])-ids):
            raise ValueError('Invalid ambience zone playback')


def validate_footsteps(audio: dict, ids: set, manifest: dict) -> None:
    f=audio['footsteps'];played=set(next(r['clips'] for r in audio['survival'] if r['role']=='footstep'))
    if ([s['name'] for s in f['surfaces']]!=list(STEP_SURFACES)
            or any(not s['clips'] or not set(s['clips'])<=ids&played or not s['volume']>=0 for s in f['surfaces'])
            or not f['step_gain']>=0 or not f['ray_distance']>0 or not isinstance(f['ray_mask'],int)
            or len(f['local_position'])!=3 or not all(map(math.isfinite,f['local_position']))
            or f['sand_layers']!=[17,19] or not isinstance(f['method'],list) or not f['method']):
        raise ValueError('Invalid footstep audio')
    owners=[*manifest['item_prototypes'],*manifest['world']['prototypes'],*manifest['survival']['foods']]
    colliders=[*manifest['colliders'],*(c for p in owners for c in p['colliders'])]
    if any(c['step_surface'] not in (*STEP_SURFACES,'sand','wood_tag',None) for c in colliders):raise ValueError('Invalid collider step surface')


def recover_ambient(source):
    main=source.get('level1',64919)
    cycle=source.resolve(main,source.read(main)['napszak']);cd=source.read(cycle)
    from .survival import managed_evidence
    managed_evidence(source,['public static float jamDecrease = 0.1f;'])
    return {'source':source_id(cycle),
        'day_volume':[cd['dayAmbientSoundVolumeDay'][0],cd['dayAmbientSoundVolumeNight'][0]],
        'night_volume':[cd['nightAmbientSoundVolumeDay'][0],cd['nightAmbientSoundVolumeNight'][0]],
        'modifier':cd['ambientSoundModifier'],'only_modifier':cd['onlyAmbientSoundModifier'],'recovery_per_second':0.1}

