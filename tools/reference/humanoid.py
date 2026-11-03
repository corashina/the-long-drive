"""Human monster humanoid Animator (`newAiAnim.anim`): the `munkas01Avatar` muscle space, the
`ThirdPersonAnimatorController` states that `newAiAnim` drives, and the clip muscle tracks.
Poses stay in the source (left-handed) space under the Animator Transform. Angles are in radians."""
import json
from .library.reader import checked
from .source import key, source_id

# Mecanim human bones (`HumanBodyBones` without the fingers), in avatar `m_HumanBoneIndex` order.
BODY_BONES = ('Hips', 'LeftUpperLeg', 'RightUpperLeg', 'LeftLowerLeg', 'RightLowerLeg', 'LeftFoot', 'RightFoot',
    'Spine', 'Chest', 'UpperChest', 'Neck', 'Head', 'LeftShoulder', 'RightShoulder', 'LeftUpperArm', 'RightUpperArm',
    'LeftLowerArm', 'RightLowerArm', 'LeftHand', 'RightHand', 'LeftToes', 'RightToes', 'LeftEye', 'RightEye', 'Jaw')
HEAD = BODY_BONES.index('Head')
# Humanoid clip attributes: RootT, RootQ, then the 95 muscles. Goal curves are not used.
ROOT = range(7, 14)
MUSCLES = range(42, 137)
HUMANOID_TYPE = 8
STATES = ('Grounded', 'Crouching', 'Airborne')
FREEFORM_CARTESIAN = 3
# `newAiAnim` sets only these parameters. `OnGround` keeps its default, so `Airborne` never plays.
PARAMETERS = {'Forward': 1, 'Turn': 1, 'Crouch': 4, 'OnGround': 4, 'Jump': 1, 'JumpLeg': 1}
IF, IF_NOT = 1, 2


def pairs(tos):
    """`m_TOS` as a dict, from typed reads or from typed records."""
    return dict(p['$pair'] if isinstance(p, dict) else p for p in tos)


def vec(d):
    return [d['x'], d['y'], d['z']]


def quat(d):
    return [d['x'], d['y'], d['z'], d['w']]


def q_mul(a, b):
    ax, ay, az, aw = a; bx, by, bz, bw = b
    return [aw*bx + ax*bw + ay*bz - az*by, aw*by - ax*bz + ay*bw + az*bx,
            aw*bz + ax*by - ay*bx + az*bw, aw*bw - ax*bx - ay*by - az*bz]


def q_inv(a):
    return [-a[0], -a[1], -a[2], a[3]]


def q_rot(q, v):
    return q_mul(q_mul(q, [*v, 0.]), q_inv(q))[:3]


def skeleton_globals(skeleton, pose):
    """Avatar-space frames (translation, rotation) of a human skeleton pose. Scales must be 1."""
    out = []
    for node, x in zip(skeleton['m_Node'], pose):
        if any(abs(v - 1) > 1e-4 for v in vec(x['s'])):
            raise ValueError('Human skeleton pose scale')
        t, q = vec(x['t']), quat(x['q'])
        if node['m_ParentId'] >= 0:
            pt, pq = out[node['m_ParentId']]
            t, q = [a + b for a, b in zip(pt, q_rot(pq, t))], q_mul(pq, q)
        out.append((t, q))
    return out


def avatar_core(avatar):
    """Avatar data: (skeleton paths, bone records with a `path`, scale, masses, head_forward)."""
    human = avatar['m_Avatar']['m_Human']['data']
    skeleton = human['m_Skeleton']['data']
    names = pairs(avatar['m_TOS'])
    paths = [names[i] for i in skeleton['m_ID']]
    indices = list(human['m_HumanBoneIndex'])
    if len(indices) != len(BODY_BONES):
        raise ValueError('Human bone count')
    for hand in ('m_LeftHand', 'm_RightHand'):
        hand_bones = human[hand]['data']['m_HandBoneIndex']
        if len(hand_bones) != 15:
            raise ValueError('Human hand bone count')
        indices += hand_bones
    def bone(index):
        if index < 0:
            return None
        axes_id = skeleton['m_Node'][index]['m_AxesId']
        if axes_id < 0:
            raise ValueError('Human bone without axes')
        a = skeleton['m_AxesArray'][axes_id]
        return {'path': paths[index], 'axes': {'pre': quat(a['m_PreQ']), 'post': quat(a['m_PostQ']),
            'sign': vec(a['m_Sgn']), 'min': vec(a['m_Limit']['m_Min']), 'max': vec(a['m_Limit']['m_Max'])}}
    bones = [bone(i) for i in indices]
    if bones[0] is None or bones[HEAD] is None or human['m_HasTDoF']:
        raise ValueError('Human hips, head or translation DoF')
    globals_ = skeleton_globals(skeleton, human['m_SkeletonPose']['data']['m_X'])
    head_forward = q_rot(q_inv(globals_[indices[HEAD]][1]), [0., 0., 1.])
    masses = list(human['m_HumanBoneMass'])
    if len(masses) != len(BODY_BONES):
        raise ValueError('Human bone masses')
    return paths, bones, human['m_Scale'], masses, head_forward


def controller_core(controller):
    """Blend trees and fades of `ThirdPersonAnimatorController` as `newAiAnim` drives it.
    Children name clip slots under `slot`."""
    names = pairs(controller['m_TOS'])
    c = controller['m_Controller']
    if len(c['m_LayerArray']) != 1 or len(c['m_StateMachineArray']) != 1:
        raise ValueError('Human Animator layers')
    layer = c['m_LayerArray'][0]['data']
    if layer['(int&)m_LayerBlendingMode'] != 0 or not layer['m_IKPass']:
        raise ValueError('Human Animator layer')
    values = c['m_Values']['data']['m_ValueArray']
    params = {names[v['m_ID']]: v for v in values}
    if {k: v['m_Type'] for k, v in params.items()} != PARAMETERS:
        raise ValueError('Human Animator parameters')
    bools = c['m_DefaultValues']['data']['m_BoolValues']
    if bools[params['Crouch']['m_Index']] or not bools[params['OnGround']['m_Index']]:
        raise ValueError('Human Animator bool defaults')
    machine = c['m_StateMachineArray'][0]['data']
    states = [s['data'] for s in machine['m_StateConstantArray']]
    if [names[s['m_NameID']] for s in states] != list(STATES) or machine['m_DefaultState'] != 0:
        raise ValueError('Human Animator states')
    if machine['m_AnyStateTransitionConstantArray']:
        raise ValueError('Human Animator any state transitions')
    def tree(state):
        if state['m_Speed'] != 1 or not state['m_Loop'] or state['m_Mirror'] or state['m_CycleOffset'] != 0:
            raise ValueError('Human Animator state settings')
        [bt] = state['m_BlendTreeConstantArray']
        nodes = [n['data'] for n in bt['data']['m_NodeArray']]
        root = nodes[0]
        if (root['m_BlendType'] != FREEFORM_CARTESIAN or names[root['m_BlendEventID']] != 'Turn'
                or names[root['m_BlendEventYID']] != 'Forward'):
            raise ValueError('Human Animator blend tree')
        positions = root['m_Blend2dData']['data']['m_ChildPositionArray']
        children = [nodes[i] for i in root['m_ChildIndices']]
        if len(positions) != len(children) or any(n['m_ChildIndices'] or n['m_Mirror'] or n['m_CycleOffset']
                or not n['m_Duration'] > 0 for n in children):
            raise ValueError('Human Animator blend children')
        # The leaf `m_Duration` scales the clip duration. It is 1 / `timeScale`.
        return [{'slot': n['m_ClipID'], 'position': [p['x'], p['y']], 'speed': 1. / n['m_Duration']}
            for n, p in zip(children, positions)]
    def fade(state, target, event, mode):
        found = [t['data'] for t in states[state]['m_TransitionConstantArray'] if t['data']['m_DestinationState'] == target]
        if len(found) != 1:
            raise ValueError('Human Animator transition')
        t = found[0]
        conditions = [(names[x['data']['m_EventID']], x['data']['m_ConditionMode']) for x in t['m_ConditionConstantArray']]
        if (conditions != [(event, mode)] or t['m_HasExitTime'] or t['m_HasFixedDuration']
                or t['m_TransitionOffset'] != 0):
            raise ValueError('Human Animator transition settings')
        return t['m_TransitionDuration']
    return {'grounded': tree(states[0]), 'crouching': tree(states[1]),
        'fades': [fade(0, 1, 'Crouch', IF), fade(1, 0, 'Crouch', IF_NOT)]}


def clip_core(record, source):
    """One humanoid clip from its library tracks record."""
    s = record['settings']; m = s['muscle']
    if (not m['m_LoopTime'] or m['m_StartTime'] != 0 or m['m_KeepOriginalOrientation']
            or m['m_KeepOriginalPositionXZ'] or record['events']):
        raise ValueError(f"Humanoid clip settings: {s['m_Name']}")
    tracks = []
    for t in record['tracks']:
        b = t['binding']
        if b['custom_type'] != HUMANOID_TYPE or b['dimension'] != 1 or b['is_pptr']:
            raise ValueError(f"Humanoid clip binding: {s['m_Name']}")
        if b['attribute'] not in ROOT and b['attribute'] not in MUSCLES:
            continue
        tracks.append({'node': 0, 'attribute': b['attribute'], 'path_hash': b['path_hash'],
            'custom_type': b['custom_type'], 'curve_offset': b['curve_offset'], 'storage': b['storage'],
            'interpolation': t['interpolation'], 'times': t['times'], 'values': t['values'],
            'in_tangents': t['in_tangents'], 'out_tangents': t['out_tangents'], 'coefficients': t['coefficients']})
    if {t['attribute'] for t in tracks} < set(ROOT):
        raise ValueError(f"Humanoid clip root: {s['m_Name']}")
    return {'source': source, 'name': s['m_Name'], 'stop_time': m['m_StopTime'],
        'orientation_offset': m['m_OrientationOffsetY'], 'level': m['m_Level'], 'mirror': m['m_Mirror'],
        'cycle_offset': m['m_CycleOffset'], 'bake_orientation': m['m_LoopBlendOrientation'],
        'bake_position': m['m_LoopBlendPositionXZ'],
        'bake_height': m['m_LoopBlendPositionY'], 'tracks': tracks}


def clip_tracks(source, identity):
    """The library tracks record of AnimationClip `identity`."""
    index = next(i for i in source.library.manifest['category_indexes'] if i['file'] == 'animations/index.json')
    exports = json.loads(checked(source.library.root, index))['exports']
    export = next(e for e in exports if e['source'] == identity)
    receipt = next(f for f in export['files'] if f['file'] == export['details']['tracks_file'])
    return json.loads(checked(source.library.root, receipt))


def child(source, transform, name):
    for c in source.read(transform)['m_Children']:
        reader = source.resolve(transform, c)
        if source.read(source.resolve(reader, source.read(reader)['m_GameObject']))['m_Name'] == name:
            return reader
    raise ValueError(f'Human skeleton node is missing: {name}')


def humanoid_record(source, ai, anim, node, relative_pose):
    """`newAiAnim.anim` and its fields, `newAiScript.headRotLimit`, `bones` and `bonesTarget`.
    `relative_pose(reader, stage)` gives the pose of Transform `reader` in the space of `stage`."""
    n = source.read(anim)
    animator = source.resolve(anim, n['anim'])
    a = source.read(animator)
    if a['m_ApplyRootMotion'] or not a['m_Enabled'] or not a['m_HasTransformHierarchy']:
        raise ValueError('Human Animator settings')
    transform = source.transform_for(source.resolve(animator, a['m_GameObject']))
    avatar = source.resolve(animator, a['m_Avatar'])
    controller = source.resolve(animator, a['m_Controller'])
    paths, bones, scale, masses, head_forward = avatar_core(source.read(avatar))
    def path_node(path):
        reader = transform
        for name in path.split('/'):
            reader = child(source, reader, name)
        return reader
    for b in bones:
        if b is not None:
            b['node'] = node(path_node(b.pop('path')))
    core = controller_core(source.read(controller))
    slots = [source.resolve(controller, p) for p in source.read(controller)['m_AnimationClips']]
    used = sorted({c['slot'] for c in core['grounded'] + core['crouching']})
    clips = [clip_core(clip_tracks(source, source_id(slots[s])), source_id(slots[s])) for s in used]
    for c in core['grounded'] + core['crouching']:
        c['clip'] = used.index(c.pop('slot'))
    d = source.read(ai)
    under = {key(r['reader']) for r in source.hierarchy(transform)}
    copies = []
    for bone, target in zip(d['bones'], d['bonesTarget'], strict=True):
        b, t = source.resolve(ai, bone), source.resolve(ai, target)
        # The target moves with the Animator pose, so the runtime reads it from the rig.
        if key(b) not in under or key(t) not in under:
            raise ValueError('Human bone copy is outside the Animator Transform')
        copies.append({'node': node(b), 'target': node(t)})
    # `LateUpdate` limits the head about the monster root rotation. The runtime uses the Animator one.
    root = source.transform_for(source.resolve(ai, d['m_GameObject']))
    t = source.read(transform)
    if key(source.resolve(transform, t['m_Father'])) != key(root) or quat(t['m_LocalRotation']) != [0., 0., 0., 1.]:
        raise ValueError('Human Animator Transform is not an unrotated child of the root')
    return {'controller': source_id(controller), 'avatar': source_id(avatar), 'root': node(transform),
        'bones': bones, 'scale': scale, 'masses': masses, 'head_forward': head_forward,
        'clips': clips, 'grounded': core['grounded'], 'crouching': core['crouching'], 'fades': core['fades'],
        'forward_lerp': n['forwardLerp'], 'turn_lerp': n['turnLerp'], 'look_lerp': n['lookLerp'],
        'hand_weight_up': n['handWeightUp'], 'hand_weight_down': n['handWeightDown'],
        'hand_weight_down_delay': n['handWeightDownMinTime'],
        'head_limit': d['headRotLimit'], 'copies': copies}


def voice_record(source, ai, node, relative_pose):
    """`newAiScript` voice. `recover_audio` replaces the clip identities with clip ids and lengths
    and sets the `SourceVoice` template and position."""
    d = source.read(ai)
    def clips(field):
        readers = [source.resolve(ai, c) for c in d[field]]
        if not readers or any(r is None or r.type.name != 'AudioClip' for r in readers):
            raise ValueError(f'Human {field} clips')
        return [source_id(r) for r in readers]
    mouth = source.resolve(ai, d['TVoice'])
    parent = source.read(mouth)['m_Father']
    def pose(field):
        reader = source.resolve(ai, d[field])
        # `TVoice` lerps between world poses. Siblings make that a parent-local lerp.
        if source.read(reader)['m_Father'] != parent:
            raise ValueError(f'Human {field} is not a sibling of TVoice')
        return relative_pose(reader, source.resolve(mouth, parent))
    voice = source.resolve(ai, d['SourceVoice'])
    if voice is None or voice.type.name != 'AudioSource':
        raise ValueError('Human SourceVoice')
    return {'voice': source_id(voice), 'position': [0., 0., 0.],
        'idle': clips('Sound_Idle'), 'chase': clips('Sound_Chase'), 'notice': clips('Sound_Notice'),
        'attack': clips('Sound_Attack'), 'death': clips('Sound_Death'),
        'idle_time': [d['idleVoiceMinTime'], d['idleVoiceMaxTime']],
        'chase_time': [d['chaseVoiceMinTime'], d['chaseVoiceMaxTime']],
        'seeing_time': [d['chaseChaseVoiceMinTime'], d['chaseChaseVoiceMaxTime']],
        'mouth': node(mouth), 'closed': pose('TVoiceClose'), 'open': pose('TVoiceOpen')}
