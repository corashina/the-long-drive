"""Melee weapon controllers and checked normalized source tracks."""
import json
from .library.reader import checked
from .source import key, source_id
from .survival import finite_record

# The shovel controller keeps its checked source values. Other controllers get structure checks.
SHOVEL_CONTROLLER = {'file':'sharedassets1.assets', 'path_id':4515}
CLIPS = (3998, 4001, 3999, 3997, 4002, 4003)
STATES = (2008834259, 2530234843, 3863879828, 1326377276, 1368685456, 3895914711)
PARAMETERS = ('AttackLeft', 'AttackRight', 'AttackForward', 'AttackBack', 'AttackStab', 'Speed', 'Hit')
# weaponhandlingscript RH: every controller animates this node through the hand Animator.
TARGET = {'file':'level1', 'path_id':23118}
HANDLER = {'file':'level1', 'path_id':74298}
ANIMATOR = {'file':'level1', 'path_id':56797}
# (attribute, custom_type): position and rotation tracks.
TRACK_TYPES = {1:0, 4:4}
STORAGE = (('constant','CONSTANT'), ('streamed','CUBIC'))


def recover_motion(source, manifest, reader):
    """Motion of one meleeweaponscript.AnimationController."""
    from .library.reader import checked
    name = f'Controller {key(reader)}'
    if reader.type.name != 'AnimatorController':
        raise ValueError(f'{name}: unsupported type {reader.type.name}')
    shovel = source_id(reader) == SHOVEL_CONTROLLER
    controller = source.read(reader)
    slots = [source_id(source.resolve(reader, p)) for p in controller['m_AnimationClips']]
    if shovel and [s['path_id'] for s in slots] != list(CLIPS):
        raise ValueError(f'{name}: shovel clip slots differ')
    node = next(n['id'] for n in manifest['nodes'] if n['source'] == TARGET)
    handler = source.get(HANDLER['file'], HANDLER['path_id'])
    animator = source.resolve(handler, source.read(handler)['anim'])
    if source_id(animator) != ANIMATOR:
        raise ValueError(f'{name}: Animator owner differs')
    names = dict(controller['m_TOS'])
    data = controller['m_Controller']
    if len(data['m_StateMachineArray']) != 1 or len(data['m_LayerArray']) != 1:
        raise ValueError(f'{name}: layer count differs')
    machine = data['m_StateMachineArray'][0]['data']
    parameters = []
    for p in data['m_Values']['data']['m_ValueArray']:
        kind = p['m_Type']
        if kind not in (1,9):raise ValueError(f'{name}: parameter type differs')
        defaults = data['m_DefaultValues']['data']['m_FloatValues' if kind == 1 else 'm_BoolValues']
        parameters.append({'id':p['m_ID'], 'name':names[p['m_ID']], 'kind':'float' if kind==1 else 'trigger',
                           'index':p['m_Index'], 'default_float':defaults[p['m_Index']] if kind==1 else None,
                           'default_bool':defaults[p['m_Index']] if kind==9 else None})
    def transition(d):
        return {'id':d['m_ID'], 'full_path_id':d['m_FullPathID'], 'user_id':d['m_UserID'],
            'destination':d['m_DestinationState'], 'duration':d['m_TransitionDuration'], 'offset':d['m_TransitionOffset'],
            'exit_time':d['m_ExitTime'], 'has_exit_time':d['m_HasExitTime'], 'fixed_duration':d['m_HasFixedDuration'],
            'can_transition_to_self':d['m_CanTransitionToSelf'], 'interruption_source':d['m_InterruptionSource'],
            'ordered_interruption':d['m_OrderedInterruption'], 'conditions':[
                {'mode':c['data']['m_ConditionMode'], 'parameter':c['data']['m_EventID'],
                 'threshold':c['data']['m_EventThreshold'], 'exit_time':c['data']['m_ExitTime']}
                for c in d['m_ConditionConstantArray']]}
    behaviors = [source.resolve(reader, p) for p in controller['m_StateMachineBehaviours']]
    if any(source.class_name(b) != 'animationEventScript' for b in behaviors):
        raise ValueError(f'{name}: unsupported state callback')
    vector = controller['m_StateMachineBehaviourVectorDescription']
    callbacks = {}
    for owner, r in vector['m_StateMachineBehaviourRanges']:
        if owner['m_LayerIndex'] != 0 or r['m_Count'] != 1:
            raise ValueError(f'{name}: callback range differs')
        callbacks[owner['m_StateID']] = source_id(behaviors[vector['m_StateMachineBehaviourIndices'][r['m_StartIndex']]])
    states = []
    for state in machine['m_StateConstantArray']:
        d = state['data']
        blends = d['m_BlendTreeConstantArray']
        if len(blends)!=1 or len(blends[0]['data']['m_NodeArray'])!=1 or d['m_BlendTreeConstantIndexArray']!=[0]:
            raise ValueError(f'{name}: unsupported blend tree')
        blend = blends[0]['data']['m_NodeArray'][0]['data']
        if blend['m_ChildIndices'] or blend['m_BlendEventID']!=2**32-1 or blend['m_BlendEventYID']!=2**32-1:
            raise ValueError(f'{name}: unsupported blend operation')
        states.append({'id':d['m_FullPathID'], 'name_id':d['m_NameID'], 'name':names[d['m_NameID']],
            'path_id':d['m_PathID'], 'tag_id':d['m_TagID'], 'clip_slot':blend['m_ClipID'],
            'speed':d['m_Speed'], 'speed_parameter':d['m_SpeedParamID'], 'cycle_offset':d['m_CycleOffset'],
            'cycle_offset_parameter':d['m_CycleOffsetParamID'], 'time_parameter':d['m_TimeParamID'],
            'mirror_parameter':d['m_MirrorParamID'], 'mirror':d['m_Mirror'], 'loop':d['m_Loop'],
            'ik_on_feet':d['m_IKOnFeet'], 'write_default_values':d['m_WriteDefaultValues'],
            'clip_duration':blend['m_Duration'], 'clip_cycle_offset':blend['m_CycleOffset'],
            'clip_mirror':blend['m_Mirror'], 'callback':callbacks.get(d['m_FullPathID']),
            'transitions':[transition(t['data']) for t in d['m_TransitionConstantArray']]})
    def bind(b, target):
        if b['type_id']!=4 or b['attribute'] not in TRACK_TYPES or b['dimension']!=3 or b['is_pptr'] or b['script'] is not None or target!=TARGET:
            raise ValueError(f'{name}: unsupported track binding')
        return node
    clips = motion_clips(source, slots, animator, bind)
    layer = data['m_LayerArray'][0]['data']
    return {'controller':source_id(reader), 'handler':source_id(handler), 'animator':source_id(animator),
        'target':node, 'coordinate_space':'unity_parent_local', 'clip_slots':slots, 'parameters':parameters,
        'default_state':machine['m_DefaultState'], 'layer_binding':layer['m_Binding'],
        'layer_weight':layer['m_DefaultWeight'], 'layer_blending_mode':layer['(int&)m_LayerBlendingMode'],
        'layer_ik_pass':layer['m_IKPass'], 'states':states,
        'any_state_transitions':[transition(t['data']) for t in machine['m_AnyStateTransitionConstantArray']],
        'selectors':[{'id':s['data']['m_FullPathID'], 'entry':s['data']['m_IsEntry'],
            'destinations':[t['data']['m_Destination'] for t in s['data']['m_TransitionConstantArray']]}
            for s in machine['m_SelectorStateConstantArray']], 'clips':clips}


def motion_clips(source, identities, animator, bind):
    """MotionClip records from the library tracks of `animator`. `bind(binding, target)` checks a track and gives its node."""
    index = next(i for i in source.library.manifest['category_indexes'] if i['file']=='animations/index.json')
    exports = json.loads(checked(source.library.root,index))['exports']
    clips = []
    for identity in identities:
        export = next(e for e in exports if e['source']==identity)
        receipt = next(f for f in export['files'] if f['file']==export['details']['tracks_file'])
        record = json.loads(checked(source.library.root,receipt))
        tracks = []
        for track in record['tracks']:
            b = track['binding']
            target = next((t for t in b.get('targets',[]) if t['owner']==source_id(animator)),None)
            if target is None:raise ValueError(f'Clip track has no target: {identity}')
            tracks.append({'node':bind(b,target['source']), 'attribute':b['attribute'], 'path_hash':b['path_hash'], 'custom_type':b['custom_type'],
                'curve_offset':b['curve_offset'], 'storage':b['storage'], 'interpolation':track['interpolation'],
                'times':track['times'], 'values':track['values'], 'in_tangents':track['in_tangents'],
                'out_tangents':track['out_tangents'], 'coefficients':track['coefficients']})
        # Shovel callbacks are state behaviors. No selected clip has events.
        if record['events']:raise ValueError(f'Unexpected clip events: {identity}')
        muscle = record['settings']['muscle']
        clips.append({'source':identity, 'source_tracks':receipt, 'name':record['settings']['m_Name'],
            'start_time':muscle['m_StartTime'], 'stop_time':muscle['m_StopTime'],
            'sample_rate':record['settings']['m_SampleRate'], 'loop':muscle['m_LoopTime'],
            'cycle_offset':muscle['m_CycleOffset'], 'tracks':tracks, 'events':[]})
    return clips


def validate_motion(m, manifest):
    """Structure of every controller. The shovel controller also keeps its checked values."""
    from .acquisition import _shape,_source,_integer,_number,_boolean,_list,_numbers
    _shape(m,'controller handler animator target coordinate_space clip_slots parameters default_state layer_binding layer_weight layer_blending_mode layer_ik_pass states any_state_transitions selectors clips')
    finite_record(m)
    _source(m['controller'])
    shovel=m['controller']==SHOVEL_CONTROLLER
    name=f"Controller {m['controller']['file']}:{m['controller']['path_id']}"
    def fail(text):raise ValueError(f'{name}: {text}')
    for field,identity in [('handler',HANDLER),('animator',ANIMATOR)]:
        _source(m[field])
        if m[field]!=identity:fail('motion source differs')
    _integer(m['target'])
    node=next((n for n in manifest['nodes'] if n['id']==m['target']),{})
    if node.get('source')!=TARGET or m['coordinate_space']!='unity_parent_local':fail('motion target or coordinate space differs')
    for identity in _list(m['clip_slots']):_source(identity)
    if shovel and m['clip_slots']!=[{'file':'sharedassets1.assets','path_id':pid} for pid in CLIPS]:fail('shovel clip slot links differ')
    parameters={}
    if [p.get('name') for p in _list(m['parameters'])]!=list(PARAMETERS):fail('parameter names differ')
    for p in m['parameters']:
        _shape(p,'id name kind index default_float default_bool');_integer(p['id']);_integer(p['index'])
        if p['id'] in parameters:fail('parameter identity differs')
        parameters[p['id']]=p
        if p['kind']=='float':
            _number(p['default_float'])
            if p['name']!='Speed' or p['default_bool'] is not None:fail('float parameter differs')
        elif p['kind']=='trigger':
            _boolean(p['default_bool'])
            if p['name']=='Speed' or p['default_float'] is not None:fail('trigger parameter differs')
        else:fail('parameter operation differs')
    if shovel and m['parameters']!=MOTION_PARAMETERS:fail('shovel parameter source identities differ')
    speed=next(p['id'] for p in m['parameters'] if p['name']=='Speed')
    for f in ('default_state','layer_binding','layer_blending_mode'):_integer(m[f])
    _number(m['layer_weight']);_boolean(m['layer_ik_pass'])
    if shovel and (m['default_state']!=0 or m['layer_binding']!=756556552 or m['layer_blending_mode']!=0 or m['layer_weight']!=0. or m['layer_ik_pass']):fail('shovel layer contract differs')
    states=_list(m['states'])
    if m['default_state']>=len(states):fail('default state missing')
    triggered=set()
    def transition(t):
        _shape(t,'id full_path_id user_id destination duration offset exit_time has_exit_time fixed_duration can_transition_to_self interruption_source ordered_interruption conditions')
        for f in ('id','full_path_id','user_id','destination','interruption_source'):_integer(t[f])
        for f in ('duration','offset','exit_time'):_number(t[f])
        for f in ('has_exit_time','fixed_duration','can_transition_to_self','ordered_interruption'):_boolean(t[f])
        if t['destination']>=len(states) or t['duration']<0 or t['interruption_source']!=0 or t['user_id']!=0:fail('transition target differs')
        for c in _list(t['conditions']):
            _shape(c,'mode parameter threshold exit_time');_integer(c['mode']);_integer(c['parameter']);_number(c['threshold']);_number(c['exit_time'])
            if c['mode']!=1 or c['parameter'] not in parameters or parameters[c['parameter']]['kind']!='trigger' or c['threshold']!=0. or c['exit_time']!=0.:
                fail('transition condition differs')
            triggered.add(parameters[c['parameter']]['name'])
    if shovel and (len(states)!=6 or [s.get('id') for s in states]!=list(STATES)):fail('shovel complete states differ')
    for i,s in enumerate(states):
        _shape(s,'id name_id name path_id tag_id clip_slot speed speed_parameter cycle_offset cycle_offset_parameter time_parameter mirror_parameter mirror loop ik_on_feet write_default_values clip_duration clip_cycle_offset clip_mirror callback transitions')
        for f in ('id','name_id','path_id','tag_id','clip_slot','speed_parameter','cycle_offset_parameter','time_parameter','mirror_parameter'):_integer(s[f])
        for f in ('speed','cycle_offset','clip_duration','clip_cycle_offset'):_number(s[f])
        for f in ('mirror','loop','ik_on_feet','write_default_values','clip_mirror'):_boolean(s[f])
        if type(s['name']) is not str or s['clip_slot']>=len(m['clip_slots']) or s['path_id']!=s['id'] or s['speed_parameter'] not in (0,speed):fail('state clip or speed binding differs')
        if s['mirror'] or s['ik_on_feet'] or s['clip_mirror'] or not s['write_default_values'] or any(s[f]!=0 for f in ('tag_id','cycle_offset','cycle_offset_parameter','time_parameter','mirror_parameter','clip_cycle_offset')):
            fail('state operation differs')
        # Attack states call AttackStart on enter and AttackEnd on exit. The default state has no callback.
        if s['callback'] is not None:_source(s['callback'])
        if (s['callback'] is None)!=(i==m['default_state']):fail('attack state callback differs')
        if shovel:
            if s['clip_slot']!=(1,0,2,3,4,5)[i] or s['speed']!=1. or s['loop']!=(i==0) or s['speed_parameter']!=(0 if i==0 else speed):fail('shovel state clip or speed binding differs')
            if (s['name'],s['name_id'],s['clip_duration'])!=STATE_BINDINGS[i]:fail('shovel state source fields differ')
            callback=None if i==0 else {'file':'sharedassets1.assets','path_id':(130091,130154,130160,130158,130156)[i-1]}
            if s['callback']!=callback:fail('shovel attack callback target differs')
            if len(_list(s['transitions']))!=(5 if i==0 else 0):fail('shovel state transitions incomplete')
        for t in _list(s['transitions']):transition(t)
    for t in _list(m['any_state_transitions']):transition(t)
    if not set(PARAMETERS[:5])<=triggered:fail('attack triggers lack transitions')
    if shovel and (m['any_state_transitions']!=ANY_TRANSITIONS or states[0]['transitions']!=IDLE_TRANSITIONS):fail('shovel checked transitions differ')
    for s in _list(m['selectors']):
        _shape(s,'id entry destinations');_integer(s['id']);_boolean(s['entry'])
        for d in _list(s['destinations']):_integer(d)
    if shovel and m['selectors']!=[{'id':756556552,'entry':True,'destinations':[0]},{'id':756556552,'entry':False,'destinations':[30000]}]:fail('shovel controller selector differs')
    clips=_list(m['clips'])
    if len(clips)!=len(m['clip_slots']):fail('complete clips required')
    for i,c in enumerate(clips):
        _shape(c,'source source_tracks name start_time stop_time sample_rate loop cycle_offset tracks events');_source(c['source'])
        if c['source']!=m['clip_slots'][i] or type(c['name']) is not str:fail('clip identity differs')
        receipt=c['source_tracks'];_shape(receipt,'bytes file sha256');_integer(receipt['bytes'])
        for f in ('start_time','stop_time','sample_rate','cycle_offset'):_number(c[f])
        _boolean(c['loop'])
        if c['start_time']<0 or c['stop_time']<c['start_time'] or c['sample_rate']<=0 or c['cycle_offset']!=0. or c['events']!=[]:fail('clip timing or events differ')
        tracks=_list(c['tracks'])
        if not tracks or len({t.get('attribute') for t in tracks})!=len(tracks):fail('clip track attributes differ')
        if shovel:
            if receipt!=TRACK_RECEIPTS[i]:fail('shovel checked track receipt differs')
            if c['sample_rate']!=60. or c['start_time']!=0. or c['stop_time']!=CLIP_TIMES[i] or c['loop']!=(i==1):fail('shovel source clip length differs')
            if len(tracks)!=2:fail('shovel track count differs')
        for j,t in enumerate(tracks):
            _shape(t,'node attribute path_hash custom_type curve_offset storage interpolation times values in_tangents out_tangents coefficients')
            for f in ('node','attribute','path_hash','custom_type','curve_offset'):_integer(t[f])
            if t['node']!=m['target'] or TRACK_TYPES.get(t['attribute'])!=t['custom_type'] or t['path_hash']!=0:fail('track binding differs')
            if (t['storage'],t['interpolation']) not in STORAGE:fail('track interpolation differs')
            constant=t['storage']=='constant'
            times=_list(t['times'])
            if not times or any(b<=a for a,b in zip(times,times[1:])):fail('track time order differs')
            for time in times:
                _number(time)
                if time<0 or time>c['stop_time']+1e-6:fail('track time out of clip')
            if shovel:
                if t['attribute']!=(1,4)[j] or t['curve_offset']!=j*3 or constant!=(i==1):fail('shovel track binding differs')
                if len(times)!=(5,1,5,4,3,3)[i] or times[0]!=(.1666666716337204,0.,.1666666716337204,.1666666716337204,0.,.1666666716337204)[i] or times[-1]!=c['stop_time']:fail('shovel source key extent differs')
            for f in ('values','in_tangents','out_tangents'):
                if len(_list(t[f]))!=len(times):fail('track key count differs')
                for row in t[f]:_numbers(row,3)
            coeff=_list(t['coefficients'])
            if len(coeff)!=(0 if constant else len(times)):fail('polynomial key count differs')
            for k in coeff:
                if len(_list(k))!=3:fail('polynomial dimension differs')
                for row in k:_numbers(row,3)

TRACK_RECEIPTS = [{'bytes': 9870,
  'file': 'animations/clips/sharedassets1.assets/3998/tracks.json',
  'sha256': '8851780dbb8517494a90bac266283eedb8acae717148c979e0f6e7fb34c1d239'},
 {'bytes': 6835,
  'file': 'animations/clips/sharedassets1.assets/4001/tracks.json',
  'sha256': '7035332bc89364a4a1f52db9a6c4455a813f4922ac6f600773930a9703363239'},
 {'bytes': 9832,
  'file': 'animations/clips/sharedassets1.assets/3999/tracks.json',
  'sha256': 'a70bbc9445f598dcf3a9cb55f482a00e17cfa232cd7dd5bb471c4016090efc89'},
 {'bytes': 9084,
  'file': 'animations/clips/sharedassets1.assets/3997/tracks.json',
  'sha256': '3eb3053001e6920fd0912df821ed2504996cddfc3891bbf2626ab8d544beeaa6'},
 {'bytes': 8314,
  'file': 'animations/clips/sharedassets1.assets/4002/tracks.json',
  'sha256': '3adf64bf7bfa84c12d5a54a42b9735424bb0744e28ed9caa21ccc27f360f3704'},
 {'bytes': 8344,
  'file': 'animations/clips/sharedassets1.assets/4003/tracks.json',
  'sha256': '7f94cbd4c86427963acbb8e5e08d7d5d3b26444e4fca2369fdea340c5449eb31'}]

MOTION_PARAMETERS = [{'default_bool': False,
  'default_float': None,
  'id': 2717246184,
  'index': 0,
  'kind': 'trigger',
  'name': 'AttackLeft'},
 {'default_bool': False,
  'default_float': None,
  'id': 1504273457,
  'index': 1,
  'kind': 'trigger',
  'name': 'AttackRight'},
 {'default_bool': False,
  'default_float': None,
  'id': 1708274416,
  'index': 2,
  'kind': 'trigger',
  'name': 'AttackForward'},
 {'default_bool': False,
  'default_float': None,
  'id': 3059532983,
  'index': 3,
  'kind': 'trigger',
  'name': 'AttackBack'},
 {'default_bool': False,
  'default_float': None,
  'id': 268324848,
  'index': 4,
  'kind': 'trigger',
  'name': 'AttackStab'},
 {'default_bool': None, 'default_float': 0.0, 'id': 3471299058, 'index': 0, 'kind': 'float', 'name': 'Speed'},
 {'default_bool': False,
  'default_float': None,
  'id': 1654612129,
  'index': 5,
  'kind': 'trigger',
  'name': 'Hit'}]

ANY_TRANSITIONS = [{'can_transition_to_self': True,
  'conditions': [{'exit_time': 0.0, 'mode': 1, 'parameter': 1504273457, 'threshold': 0.0}],
  'destination': 2,
  'duration': 0.10000000149011612,
  'exit_time': 0.800000011920929,
  'fixed_duration': True,
  'full_path_id': 666786194,
  'has_exit_time': True,
  'id': 1699225119,
  'interruption_source': 0,
  'offset': 0.0,
  'ordered_interruption': True,
  'user_id': 0},
 {'can_transition_to_self': True,
  'conditions': [{'exit_time': 0.0, 'mode': 1, 'parameter': 1708274416, 'threshold': 0.0}],
  'destination': 3,
  'duration': 0.10000000149011612,
  'exit_time': 0.800000011920929,
  'fixed_duration': True,
  'full_path_id': 834792901,
  'has_exit_time': True,
  'id': 3233637805,
  'interruption_source': 0,
  'offset': 0.0,
  'ordered_interruption': True,
  'user_id': 0},
 {'can_transition_to_self': True,
  'conditions': [{'exit_time': 0.0, 'mode': 1, 'parameter': 2717246184, 'threshold': 0.0}],
  'destination': 1,
  'duration': 0.10000000149011612,
  'exit_time': 0.800000011920929,
  'fixed_duration': True,
  'full_path_id': 129301733,
  'has_exit_time': True,
  'id': 1131093452,
  'interruption_source': 0,
  'offset': 0.0,
  'ordered_interruption': True,
  'user_id': 0},
 {'can_transition_to_self': False,
  'conditions': [{'exit_time': 0.0, 'mode': 1, 'parameter': 1654612129, 'threshold': 0.0}],
  'destination': 0,
  'duration': 0.10000000149011612,
  'exit_time': 9.427516545201797e-09,
  'fixed_duration': True,
  'full_path_id': 3058440661,
  'has_exit_time': False,
  'id': 4105854552,
  'interruption_source': 0,
  'offset': 0.0,
  'ordered_interruption': True,
  'user_id': 0},
 {'can_transition_to_self': True,
  'conditions': [{'exit_time': 0.0, 'mode': 1, 'parameter': 3059532983, 'threshold': 0.0}],
  'destination': 4,
  'duration': 0.10000000149011612,
  'exit_time': 0.800000011920929,
  'fixed_duration': True,
  'full_path_id': 1694822627,
  'has_exit_time': True,
  'id': 4259996521,
  'interruption_source': 0,
  'offset': 0.0,
  'ordered_interruption': True,
  'user_id': 0},
 {'can_transition_to_self': True,
  'conditions': [{'exit_time': 0.0, 'mode': 1, 'parameter': 268324848, 'threshold': 0.0}],
  'destination': 5,
  'duration': 0.10000000149011612,
  'exit_time': 0.800000011920929,
  'fixed_duration': True,
  'full_path_id': 3701898148,
  'has_exit_time': True,
  'id': 1145628718,
  'interruption_source': 0,
  'offset': 0.0,
  'ordered_interruption': True,
  'user_id': 0},
 {'can_transition_to_self': False,
  'conditions': [],
  'destination': 0,
  'duration': 0.10000000149011612,
  'exit_time': 0.800000011920929,
  'fixed_duration': True,
  'full_path_id': 3058440661,
  'has_exit_time': True,
  'id': 4105854552,
  'interruption_source': 0,
  'offset': 0.0,
  'ordered_interruption': True,
  'user_id': 0}]

IDLE_TRANSITIONS = [{'can_transition_to_self': True,
  'conditions': [{'exit_time': 0.0, 'mode': 1, 'parameter': 2717246184, 'threshold': 0.0}],
  'destination': 1,
  'duration': 0.10000000149011612,
  'exit_time': 0.0,
  'fixed_duration': True,
  'full_path_id': 156787784,
  'has_exit_time': False,
  'id': 3869188192,
  'interruption_source': 0,
  'offset': 0.0,
  'ordered_interruption': True,
  'user_id': 0},
 {'can_transition_to_self': True,
  'conditions': [{'exit_time': 0.0, 'mode': 1, 'parameter': 1504273457, 'threshold': 0.0}],
  'destination': 2,
  'duration': 0.10000000149011612,
  'exit_time': 0.0,
  'fixed_duration': True,
  'full_path_id': 2413259359,
  'has_exit_time': False,
  'id': 3129804257,
  'interruption_source': 0,
  'offset': 0.0,
  'ordered_interruption': True,
  'user_id': 0},
 {'can_transition_to_self': True,
  'conditions': [{'exit_time': 0.0, 'mode': 1, 'parameter': 1708274416, 'threshold': 0.0}],
  'destination': 3,
  'duration': 0.10000000149011612,
  'exit_time': 0.0,
  'fixed_duration': True,
  'full_path_id': 2557134833,
  'has_exit_time': False,
  'id': 3256176142,
  'interruption_source': 0,
  'offset': 0.0,
  'ordered_interruption': True,
  'user_id': 0},
 {'can_transition_to_self': True,
  'conditions': [{'exit_time': 0.0, 'mode': 1, 'parameter': 3059532983, 'threshold': 0.0}],
  'destination': 4,
  'duration': 0.10000000149011612,
  'exit_time': 0.0,
  'fixed_duration': True,
  'full_path_id': 3216411534,
  'has_exit_time': False,
  'id': 33870250,
  'interruption_source': 0,
  'offset': 0.0,
  'ordered_interruption': True,
  'user_id': 0},
 {'can_transition_to_self': True,
  'conditions': [{'exit_time': 0.0, 'mode': 1, 'parameter': 268324848, 'threshold': 0.0}],
  'destination': 5,
  'duration': 0.10000000149011612,
  'exit_time': 4.299437961208241e-08,
  'fixed_duration': True,
  'full_path_id': 101974217,
  'has_exit_time': False,
  'id': 3148241645,
  'interruption_source': 0,
  'offset': 0.0,
  'ordered_interruption': True,
  'user_id': 0}]

STATE_BINDINGS = [('katana_idle', 3361416310, 1.0),
 ('AttackLeft', 2717246184, 1.0),
 ('AttackRight', 1504273457, 1.0),
 ('katana_AttackForward', 1634851761, 1.0),
 ('shovel_AttackBack', 815580648, 1.0),
 ('shovel_AttackStab', 2302561967, 1.0)]

CLIP_TIMES = [0.6666666865348816, 0.0, 0.6666666865348816, 0.6666666865348816, 0.5833333134651184, 0.6666666865348816]
