"""Recover trigger collider roles from ladder and hazard scripts."""
import math
from .source import key
from .gltf import transform_point
from .collision import world_pose
from .survival import managed_evidence


LADDER = ['public class ladderscript : MonoBehaviour', 'mainscript.M.player.ladderV = 5;',
    'RB.velocity = zero * FladderSpeed * TLadder.speed * (input.run ? FrunM : 1f);',
    'float num = Vector3.Angle(Th.forward, TLadder.T.up);']
HAZARD = ['bscript.TryBreak(damageBreakable * Time.deltaTime);',
    'pscript.DamageInstant(damagePlayer * Time.deltaTime, show: true);']


def recover_trigger_role(source, transform, root, body_pose):
    """Return the role of a trigger on transform, in the frame of body_pose."""
    while transform:
        for component in source.components(transform)[1]:
            if component.type.name != 'MonoBehaviour':continue
            name = source.class_name(component)
            if name == 'ladderscript':
                data = source.read(component)
                managed_evidence(source, LADDER)
                rotation = world_pose(source, source.resolve(component, data['T']))['rotation']
                body = body_pose['rotation']
                up = transform_point(transform_point((0., 1., 0.), (0., 0., 0.), rotation, (1., 1., 1.)),
                                     (0., 0., 0.), (-body[0], -body[1], -body[2], body[3]), (1., 1., 1.))
                length = math.sqrt(sum(v * v for v in up))
                if not math.isfinite(data['speed']) or not length > 0.:
                    raise ValueError(f'Ladder values differ: {key(component)}')
                return {'ladder': {'speed': data['speed'], 'up': [v / length for v in up]}}
            if name == 'playerdeathtriggerscript':
                data = source.read(component)
                managed_evidence(source, HAZARD)
                role = {'damage_player': data['damagePlayer'], 'damage_breakable': data['damageBreakable']}
                if not all(math.isfinite(v) and v >= 0. for v in role.values()):
                    raise ValueError(f'Hazard values differ: {key(component)}')
                return {'hazard': role}
        if key(transform) == key(root):return None
        transform = source.resolve(transform, source.read(transform)['m_Father'])
    return None
