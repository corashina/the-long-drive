"""Source node followers of world prototypes: `followspringscript`, `followavgposrot` and
`followposrotscript` (Bus02 leaf springs, axles and tipper rams)."""
import math
from .collision import quaternion_product
from .gltf import convert_point, convert_rotation
from .source import key, vector

FOLLOWERS = ('followspringscript', 'followavgposrot', 'followposrotscript')


def euler_quaternion(value):
    """Unity Quaternion.Euler (Z, then X, then Y), converted."""
    rotations = []
    for axis, angle in enumerate(vector(value)):
        half = math.radians(angle) * 0.5;q = [0., 0., 0., math.cos(half)];q[axis] = math.sin(half);rotations.append(q)
    return list(convert_rotation(quaternion_product(rotations[1], quaternion_product(rotations[0], rotations[2]))))


def recover_followers(source, hierarchy, mapping, vehicle, trailer):
    """Followers in hierarchy order and the `wheelgraphicsscript.T` node of each wheel, or None.

    Wheels follow the vehicle or trailer wheel order. Every Transform link that an enabled branch
    reads must be set: Unity would stop the script with a null reference.
    """
    found = [c for n in hierarchy for c in n['components']
        if c.type.name == 'MonoBehaviour' and source.class_name(c) in FOLLOWERS]
    if not found:return None
    if (vehicle is None) == (trailer is None):raise ValueError(f'{key(found[0])}: followers need one vehicle or trailer')
    def node(owner, pointer, field, needed=True):
        target = source.resolve(owner, pointer)
        if target is None:
            if needed:raise ValueError(f'{key(owner)} {field}: missing value')
            return None
        if key(target) not in mapping:raise ValueError(f'{key(owner)} {field}: outside its prototype')
        return mapping[key(target)]
    followers = []
    for c in found:
        d = source.read(c);name = source.class_name(c)
        if name == 'followspringscript':
            followers.append({'kind': 'spring', 'node': node(c, d['T'], 'T'),
                **{f: node(c, d[s], s) for f, s in (('target', 'TT'), ('from', 'T1'), ('to', 'T2'), ('start', 'TT1'), ('end', 'TT2'))}})
        elif name == 'followavgposrot':
            position, rotation = bool(d['position']), bool(d['rotation'])
            followers.append({'kind': 'average', 'node': node(c, d['T'], 'T'), 'position': position, 'rotation': rotation,
                'offset': list(convert_point(vector(d['Offset']))), 'axis': list(convert_point(vector(d['Axis']))),
                'first': node(c, d['TT1'], 'TT1', position), 'second': node(c, d['TT2'], 'TT2', position),
                'look': node(c, d['TR'], 'TR', rotation)})
        else:
            pos, lerp, clamp, rot, look = (bool(d[f]) for f in ('pos', 'lerp', 'clamp', 'rot', 'look'))
            if clamp:raise ValueError(f'{key(c)}: unsupported followposrotscript clamp')
            followers.append({'kind': 'pose', 'node': node(c, d['T'], 'T'), 'copy_position': pos, 'lerp': (not pos) and lerp,
                'copy_rotation': rot, 'look': (not rot) and look, 'lerp_amount': d['flerp'], 'look_rotation': euler_quaternion(d['lookAngles']),
                'position_target': node(c, d['TargetPos'], 'TargetPos', pos),
                'from': node(c, d['Target1'], 'Target1', (not pos) and lerp), 'to': node(c, d['Target2'], 'Target2', (not pos) and lerp),
                'rotation_target': node(c, d['TargetRot'], 'TargetRot', rot),
                'look_target': node(c, d['TargetLook'], 'TargetLook', (not rot) and look)})
    if trailer is not None:
        wheels = trailer['graphics']
    else:
        # wheelgraphicsscript.T of each vehicle wheel, by its WheelCollider.
        graphics = {}
        for n in hierarchy:
            for c in n['components']:
                if c.type.name != 'MonoBehaviour' or source.class_name(c) != 'wheelgraphicsscript':continue
                g = source.read(c);wheel = source.resolve(c, g['W'])
                if wheel is not None and source.resolve(c, g['T']) is not None:
                    graphics[key(wheel)] = node(c, g['T'], 'T')
        wheels = []
        for w in vehicle['wheels']:
            identity = (w['source']['file'], w['source']['path_id'])
            if identity not in graphics:raise ValueError(f'{identity}: follower wheel has no graphics')
            wheels.append(graphics[identity])
    return {'wheels': wheels, 'followers': followers}
