"""Melee weapon controllers and checked normalized source tracks."""
import json
from .library.reader import checked
from .source import key, source_id

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


def recover_motion(source, manifest, reader):
    """Motion of one meleeweaponscript.AnimationController."""
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


