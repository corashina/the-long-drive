"""Recover the siphon hose (prefab Leszivocso): tubescript chain, joints, end caps, and skin."""
import math
from .source import key, source_id, vector
from .collision import bake_points, quaternion_product, world_pose
from .caps import cap_flow, opening_metadata
from .particles import cap_particles
from .survival import finite_record

SEGMENTS = 6
LINEAR = ('m_XMotion', 'm_YMotion', 'm_ZMotion')
ANGULAR = ('m_AngularXMotion', 'm_AngularYMotion', 'm_AngularZMotion')
LOCKED, FREE = 0, 2  # ConfigurableJointMotion


def check_joint(fields):
    """tubescript chains are ball joints: linear motion locked, angular motion free, unbreakable."""
    if any(fields[f] != LOCKED for f in LINEAR) or any(fields[f] != FREE for f in ANGULAR):
        raise ValueError('Hose joint motion differs')
    if math.isfinite(fields['m_BreakForce']) or math.isfinite(fields['m_BreakTorque']):
        raise ValueError('Hose joint is breakable')


def chain_order(joints):
    """Order joints from the free start segment. Return (segments, joints)."""
    after = {j['body']: j for j in joints}
    targets = {j['connected_body'] for j in joints}
    starts = [b for b in after if b not in targets]
    if len(after) != len(joints) or len(targets) != len(joints) or len(starts) != 1:
        raise ValueError('Hose joints do not form one chain')
    segments = starts; ordered = []
    while segments[-1] in after:
        ordered.append(after[segments[-1]]); segments.append(ordered[-1]['connected_body'])
    if len(ordered) != len(joints):
        raise ValueError('Hose joints do not form one chain')
    return segments, ordered


def recover_hose(source, by_node, bodies, poses, node_ids, exporter, main, output) -> dict:
    """`by_node`: prototype node id to hierarchy record. `poses`: body node to world pose."""
    body_nodes = {b['node']: b['id'] for b in bodies}
    rigid_bodies = {(b['source']['file'], b['source']['path_id']): b['id'] for b in bodies}
    def transform_of(reader):
        return source.transform_for(source.resolve(reader, source.read(reader)['m_GameObject']))
    def local(point, transform, body):
        return bake_points([vector(point)], source.chain(transform), poses[bodies[body - 1]['node']])[0]
    tubes = {}; scripts = {}; joints = []; skins = []
    for node, record in by_node.items():
        for c in record['components']:
            if c.type.name == 'MonoBehaviour' and source.class_name(c) == 'tubescript':
                tubes[key(c)] = body_nodes[node]; scripts[body_nodes[node]] = c
            elif c.type.name == 'ConfigurableJoint':
                d = source.read(c); check_joint(d)
                target = source.resolve(c, d['m_ConnectedBody'])
                if target is None or key(target) not in rigid_bodies: raise ValueError(f'Hose joint lacks body: {key(c)}')
                body, other = body_nodes[node], rigid_bodies[key(target)]
                # An auto connected anchor is the anchor point at load time.
                connected = (local(d['m_Anchor'], record['reader'], other) if d['m_AutoConfigureConnectedAnchor']
                    else local(d['m_ConnectedAnchor'], transform_of(target), other))
                joints.append({'body': body, 'connected_body': other,
                    'anchor': local(d['m_Anchor'], record['reader'], body), 'connected_anchor': connected})
            elif c.type.name == 'SkinnedMeshRenderer':
                skins.append((node, c))
    if len(tubes) != SEGMENTS or len(skins) != 1: raise ValueError('Hose needs 6 tubescripts and one skin')
    segments, joints = chain_order(joints)
    if sorted(scripts) != sorted(segments): raise ValueError('Hose tubescript outside chain')
    # Both source ends set isMain. Each main runs Fill once per frame.
    mains = [body for body in segments if source.read(scripts[body])['isMain']]
    if not mains: raise ValueError('Hose needs a main segment')
    main_script = scripts[mains[0]]
    if any(source.read(scripts[b])[f] != source.read(main_script)[f] for b in mains for f in ('T1', 'T2')):
        raise ValueError('Hose mains differ in ends')
    def tube(field):
        target = source.resolve(main_script, source.read(main_script)[field])
        if target is None or key(target) not in tubes: raise ValueError(f'Hose {field} outside chain')
        return tubes[key(target)]
    def end(body):
        script = scripts[body]
        cap = source.resolve(script, source.read(script)['owncap'])
        if cap is None or source.class_name(cap) != 'tankcapscript': raise ValueError(f'Hose end lacks owncap: {key(script)}')
        transform = transform_of(cap)
        body_rotation = poses[bodies[body - 1]['node']]['rotation']
        inverse = [-v for v in body_rotation[:3]] + [body_rotation[3]]
        opening = {'source': source_id(cap), 'tank': None, **cap_flow(source, cap, transform, main['pourSpeed'])}
        opening.update(opening_metadata(source, cap, node_ids, main['fluidTuneSpeed']))
        visual, splash = cap_particles(source, cap, f'hose_{len(ends) + 1}', output)
        ends.append({'body': body, 'node': node_ids[key(transform)], 'opening': opening, 'visual': visual, 'splash': splash,
            'pose': {'translation': local({'x': 0., 'y': 0., 'z': 0.}, transform, body),
                'rotation': list(quaternion_product(inverse, world_pose(source, transform)['rotation'])), 'scale': [1., 1., 1.]}})
    from .world import skin_record
    skin_node, renderer = skins[0]
    t1, t2 = tube('T1'), tube('T2'); ends = []
    end(t1); end(t2)
    return {'source': source_id(main_script), 'segments': segments, 'mains': mains, 't1': t1, 't2': t2,
        'break_force': [source.read(scripts[b])['breakForce'] for b in segments],
        'connect_anchor': [local(source.read(scripts[b])['connectAnchor'], transform_of(scripts[b]), b) for b in segments],
        'joints': joints, 'ends': ends,
        'skin': skin_record(source, exporter, renderer, skin_node, lambda bone: body_nodes[node_ids[key(bone)]])}


def validate_hose(hose, prototype) -> None:
    finite_record(hose)
    bodies = {b['id'] for b in prototype['bodies']}
    segments = hose['segments']
    if len(segments) != SEGMENTS or len(set(segments)) != SEGMENTS or not set(segments) <= bodies:
        raise ValueError('Hose needs 6 segment bodies')
    joints = hose['joints']
    if [(j['body'], j['connected_body']) for j in joints] != list(zip(segments, segments[1:])):
        raise ValueError('Hose joints do not form one chain')
    if any(len(j[f]) != 3 for j in joints for f in ('anchor', 'connected_anchor')):
        raise ValueError('Hose joint anchor')
    mains = hose['mains']
    if (not mains or len(set(mains)) != len(mains) or not {*mains, hose['t1'], hose['t2']} <= set(segments)
            or hose['t1'] == hose['t2']):
        raise ValueError('Hose main or end outside chain')
    if len(hose['break_force']) != SEGMENTS or min(hose['break_force']) <= 0:
        raise ValueError('Hose break force')
    if len(hose['connect_anchor']) != SEGMENTS or any(len(a) != 3 for a in hose['connect_anchor']):
        raise ValueError('Hose connect anchor')
    ends = hose['ends']
    if [e['body'] for e in ends] != [hose['t1'], hose['t2']]:
        raise ValueError('Hose needs both ends')
    for index, e in enumerate(ends, 1):
        o = e['opening']
        if o['tank'] is not None or o['flow_liters_per_second'] <= 0 or o['flow_radius'] <= 0:
            raise ValueError('Hose end opening')
        if [e['visual']['role'], e['splash']['role']] != [f'outlet_hose_{index}', f'splash_hose_{index}']:
            raise ValueError('Hose end particles')
    skin = hose['skin']
    if not skin['bones'] or not set(skin['bones']) <= set(segments) or len(skin['bones']) != len(skin['bind_poses']):
        raise ValueError('Hose skin bones')
