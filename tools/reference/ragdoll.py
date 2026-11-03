"""Player ragdoll prefab (fpscontroller.prefabRagdol): bodies, colliders, joints, and the pose map.

Body poses are in ragdoll root space. Collider geometry and joint anchors and axes are in body space.
Joint axes are axial: a positive source angle about the source axis stays positive about the exported axis.
Joint limits are in degrees.
"""
import math
from .source import key, source_id, vector
from .collision import world_pose, bake_points, quaternion_product, geometry, step_surface, collider_tag
from .survival import component, managed_evidence

COLLIDERS = ('BoxCollider', 'SphereCollider', 'CapsuleCollider', 'MeshCollider')
LOCKED, LIMITED = 0, 1  # ConfigurableJointMotion
EVIDENCE = [
    'spawnedRagdol = UnityEngine.Object.Instantiate(prefabRagdol, transform.position + Vector3.down * 10f, transform.rotation, transform);',
    'ragdolRBs[i].isKinematic = false;', 'ragdolRBs[i].velocity = velocity;',
    'ragdolColliders[l].enabled = true;', 'Physics.IgnoreCollision(collider, ragdolColliders[l]);',
    'spawnedRagdolrp.Upd(ragdolPos);', 'ragdolT[i].position = _other.ragdolT[i].position;',
    'ragdolT[i].rotation = _other.ragdolT[i].rotation;', 'spawnedRagdolro.CopyFrom(outfit);',
    'ragdolHeadPos = spawnedRagdolrp.head;', 'transform.position += ragdolHeadPos.position - Th.position;',
    'ragdolHeadPos.rotation = CamParent.rotation;']


def prefab_root(source):
    player = source.get('level1', 80125)
    prefab = source.resolve(player, source.read(player)['prefabRagdol'])
    if prefab is None or prefab.type.name != 'GameObject':
        raise ValueError('Missing ragdoll prefab')
    return source.transform_for(prefab)


def prepare_ragdoll(source, roots):
    """Export the prefab as its own asset. Instantiate replaces the root position and rotation."""
    root = prefab_root(source)
    roots['ragdoll'] = [root]
    source.ragdoll_root = key(root)


def axial(value):
    """Unity local axis to the converted space. Rotation angles keep their sign."""
    x, y, z = vector(value)
    length = math.sqrt(x * x + y * y + z * z)
    if length <= 1e-6:
        raise ValueError('Zero ragdoll joint axis')
    return [-x / length, -y / length, z / length]


def joint_limit(value):
    return {'limit': value['limit'], 'bounciness': value['bounciness'], 'contact_distance': value['contactDistance']}


def joint_spring(value):
    return {'spring': value['spring'], 'damper': value['damper']}


def joint_shape(reader, d):
    """Twist about `axis`, swing1 about `swing_axis`, swing2 about axis x swing_axis."""
    if reader.type.name == 'CharacterJoint':
        return {'kind': 'character', 'axis': axial(d['m_Axis']), 'swing_axis': axial(d['m_SwingAxis']),
            'low_twist': joint_limit(d['m_LowTwistLimit']), 'high_twist': joint_limit(d['m_HighTwistLimit']),
            'swing1': joint_limit(d['m_Swing1Limit']), 'swing2': joint_limit(d['m_Swing2Limit']),
            'twist_spring': joint_spring(d['m_TwistLimitSpring']), 'swing_spring': joint_spring(d['m_SwingLimitSpring']),
            'projection': bool(d['m_EnableProjection']), 'projection_distance': d['m_ProjectionDistance'],
            'projection_angle': d['m_ProjectionAngle']}
    if reader.type.name == 'ConfigurableJoint':
        # Only the CharacterJoint equivalent: linear locked, angular limited, no drives.
        if (any(d[f] != LOCKED for f in ('m_XMotion', 'm_YMotion', 'm_ZMotion'))
                or any(d[f] != LIMITED for f in ('m_AngularXMotion', 'm_AngularYMotion', 'm_AngularZMotion'))):
            raise ValueError(f'Unaudited ragdoll joint motion: {key(reader)}')
        if any(d[f]['positionSpring'] or d[f]['positionDamper'] for f in ('m_XDrive', 'm_YDrive', 'm_ZDrive', 'm_AngularXDrive', 'm_AngularYZDrive', 'm_SlerpDrive')):
            raise ValueError(f'Unaudited ragdoll joint drive: {key(reader)}')
        return {'kind': 'configurable', 'axis': axial(d['m_Axis']), 'swing_axis': axial(d['m_SecondaryAxis']),
            'low_twist': joint_limit(d['m_LowAngularXLimit']), 'high_twist': joint_limit(d['m_HighAngularXLimit']),
            'swing1': joint_limit(d['m_AngularYLimit']), 'swing2': joint_limit(d['m_AngularZLimit']),
            'twist_spring': joint_spring(d['m_AngularXLimitSpring']), 'swing_spring': joint_spring(d['m_AngularYZLimitSpring']),
            'projection': d['m_ProjectionMode'] != 0, 'projection_distance': d['m_ProjectionDistance'],
            'projection_angle': d['m_ProjectionAngle']}
    raise ValueError(f'Unsupported ragdoll joint: {reader.type.name}')


def recover_joints(source, by_node, bodies, poses):
    """`by_node`: node id to hierarchy record. `poses`: body node to world pose."""
    body_nodes = {b['node']: b['id'] for b in bodies}
    rigid_bodies = {(b['source']['file'], b['source']['path_id']): b['id'] for b in bodies}
    def transform_of(reader):
        return source.transform_for(source.resolve(reader, source.read(reader)['m_GameObject']))
    def local(point, transform, body):
        return bake_points([vector(point)], source.chain(transform), poses[bodies[body - 1]['node']])[0]
    joints = []
    for node, record in by_node.items():
        for c in record['components']:
            if not c.type.name.endswith('Joint'):
                continue
            d = source.read(c)
            shape = joint_shape(c, d)
            target = source.resolve(c, d['m_ConnectedBody'])
            if node not in body_nodes or target is None or key(target) not in rigid_bodies:
                raise ValueError(f'Ragdoll joint lacks body: {key(c)}')
            if not math.isinf(d['m_BreakForce']) or not math.isinf(d['m_BreakTorque']):
                raise ValueError(f'Breakable ragdoll joint: {key(c)}')
            if d.get('m_MassScale', 1.) != 1. or d.get('m_ConnectedMassScale', 1.) != 1.:
                raise ValueError(f'Unaudited ragdoll joint mass scale: {key(c)}')
            body, other = body_nodes[node], rigid_bodies[key(target)]
            # An auto connected anchor is the anchor point at Instantiate.
            connected = (local(d['m_Anchor'], record['reader'], other) if d['m_AutoConfigureConnectedAnchor']
                else local(d['m_ConnectedAnchor'], transform_of(target), other))
            joints.append({'source': source_id(c), 'body': body, 'connected_body': other,
                'anchor': local(d['m_Anchor'], record['reader'], body), 'connected_anchor': connected,
                'collide_connected': bool(d['m_EnableCollision']), **shape})
    return joints


def recover_pose_map(source, own, live, node):
    """ragdolPosScript.Upd copies each live ragdolT[i] world pose to the ragdoll ragdolT[i]."""
    for script in (own, live):
        if script is None or script.type.name != 'MonoBehaviour' or source.class_name(script) != 'ragdolPosScript':
            raise ValueError('Ragdoll pose script')
    d, l = source.read(own), source.read(live)
    if len(d['ragdolT']) != len(l['ragdolT']) or not d['ragdolT']:
        raise ValueError('Ragdoll pose map lengths differ')
    return {'source': source_id(own), 'live_source': source_id(live), 'head': node(source.resolve(own, d['head'])),
        'pairs': [{'ragdoll': node(source.resolve(own, a)), 'live': node(source.resolve(live, b))}
                  for a, b in zip(d['ragdolT'], l['ragdolT'], strict=True)]}


def recover_rig(source, root, by_node, parents, rigid, collider, sensor):
    """Bodies and colliders under `root`. `rigid(reader, data)` and `collider(reader)` select the records.
    `sensor` overrides the collider trigger flag; None keeps it. Gives bodies, colliders and body world poses."""
    from .gltf import GltfExporter
    matrix = source.read(next(r for r in source.objects.values() if r.type.name == 'PhysicsManager'))['m_LayerCollisionMatrix']
    root_pose = world_pose(source, root)
    inverse = [-v for v in root_pose['rotation'][:3]] + [root_pose['rotation'][3]]
    bodies = []; body_nodes = {}; poses = {}
    for n, record in by_node.items():
        rigid_bodies = [c for c in record['components'] if c.type.name == 'Rigidbody' and rigid(c, source.read(c))]
        if not rigid_bodies:
            continue
        if len(rigid_bodies) != 1:
            raise ValueError('Duplicate ragdoll Rigidbody')
        d = source.read(rigid_bodies[0])
        pose = world_pose(source, record['reader']); pose['scale'] = [1., 1., 1.]
        poses[n] = pose
        # SpawnRagdol and SetRagdol make every Rigidbody dynamic.
        bodies.append({'id': len(bodies) + 1, 'source': source_id(rigid_bodies[0]), 'node': n, 'dynamic': True,
            'mass_kg': d['m_Mass'], 'linear_damping': d['m_Drag'], 'angular_damping': d['m_AngularDrag'],
            'pose': {'translation': bake_points([(0., 0., 0.)], source.chain(record['reader']), root_pose)[0],
                     'rotation': list(quaternion_product(inverse, pose['rotation'])), 'scale': [1., 1., 1.]},
            'parent_body': None})
        body_nodes[n] = bodies[-1]['id']
    def body_for(n):
        while n is not None:
            if n in body_nodes:
                return body_nodes[n]
            n = parents[n]
        return None
    for body in bodies:
        body['parent_body'] = body_for(parents[body['node']])
    exporter = None; colliders = []
    for n, record in by_node.items():
        for c in record['components']:
            if c.type.name not in COLLIDERS:
                if c.type.name.endswith('Collider'):
                    raise ValueError(f'Unsupported ragdoll collider: {key(c)}')
                continue
            if not collider(c):
                continue
            d = source.read(c); owner = body_for(n)
            if owner is None:
                raise ValueError(f'Ragdoll collider lacks body: {key(c)}')
            if c.type.name == 'MeshCollider' and exporter is None:
                exporter = GltfExporter(source, None, source.library)
            layer = source.read(record['game_object'])['m_Layer']
            material = source.resolve(c, d['m_Material'])
            properties = source.read(material) if material else {'dynamicFriction': .6, 'bounciness': 0.}
            # SpawnRagdol and SetRagdol enable every ragdoll collider.
            colliders.append({'id': len(colliders) + 1, 'source': source_id(c), 'node': n, 'body': owner,
                'sensor': bool(d['m_IsTrigger']) if sensor is None else sensor, 'enabled': True,
                'membership': 1 << layer, 'filter': matrix[layer],
                'friction': properties['dynamicFriction'], 'restitution': properties['bounciness'],
                'geometry': geometry(source, exporter, c, record['reader'], poses[bodies[owner - 1]['node']], True),
                'step_surface': step_surface(source, record['game_object']), 'tag': collider_tag(source, record['game_object'])})
    return bodies, colliders, poses


def recover_ragdoll(source, manifest, output):
    from .clothing import recover_characters, character_renderers
    from .player_pose import resolved_transform
    nodes = {key(source.get(n['source']['file'], n['source']['path_id'])): n for n in manifest['nodes']}
    def node(reader):
        reader = resolved_transform(source, reader)
        if key(reader) not in nodes:
            raise ValueError('Ragdoll link outside retained hierarchy')
        return nodes[key(reader)]['id']
    parents = {n['id']: n['parent'] for n in manifest['nodes']}
    root = prefab_root(source)
    asset = next(a for a in manifest['assets'] if a['usage'] == 'ragdoll_prototype')
    if key(root) != getattr(source, 'ragdoll_root', None) or nodes[key(root)]['asset'] != asset['id']:
        raise ValueError('Ragdoll asset root')
    records = source.hierarchy(root, activate_root=True)
    by_node = {nodes[key(r['reader'])]['id']: r for r in records}
    def checked(reader, d):
        if not d['m_UseGravity'] or d['m_Constraints']:
            raise ValueError(f'Unaudited ragdoll Rigidbody: {key(reader)}')
        return True
    bodies, colliders, poses = recover_rig(source, root, by_node, parents, checked, lambda c: True, None)
    outfit = component(source, root, 'playermodeloutfitscript')
    renderers = {key(c) for r in records for c in r['components'] if c.type.name.endswith('Renderer')}
    if renderers != {key(r) for r in character_renderers(source, outfit)}:
        raise ValueError('Ragdoll renderer outside characters')
    characters, _ = recover_characters(source, manifest, output, outfit)
    player = source.get('level1', 80125)
    pose = recover_pose_map(source, component(source, root, 'ragdolPosScript'),
        source.resolve(player, source.read(player)['ragdolPos']), node)
    return {'source': source_id(root), 'asset': asset['id'], 'root': nodes[key(root)]['id'],
        'outfit_source': source_id(outfit),
        'invisible_material': source_id(source.resolve(outfit, source.read(outfit)['defInvisible'])),
        'characters': characters, 'bodies': bodies, 'colliders': colliders,
        'joints': recover_joints(source, by_node, bodies, poses), 'pose': pose,
        'evidence': managed_evidence(source, EVIDENCE)}

