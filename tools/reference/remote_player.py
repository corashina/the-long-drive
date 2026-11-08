"""Recover the remote player body: source `mpplayerscript` on `snTempPlayerSync.playerPrefab`."""
from .source import continuous, key, source_id, vector
from .gltf import convert_point
from .collision import collider_tag, geometry, world_pose
from .survival import managed_evidence

STANDING_LAYER = 0
SEATED_LAYER = 10
# `meleeweaponscript` and `weaponscript` send `SDmgPlayer` for a hit on this layer.
HIT_LAYER = 9
HIT_COLLIDERS = 10


def recover_remote_player(source, local_player, by_source, matrix):
    """`local_player` is the fpscontroller Transform. `by_source` maps Transform keys to node ids."""
    sync = source.get('level1', 80447)
    if source.class_name(sync) != 'snTempPlayerSync':
        raise ValueError('Remote player sync differs')
    prefab = source.resolve(sync, source.read(sync)['playerPrefab'])
    root = source.transform_for(prefab)
    scripts = [c for c in source.components(root)[1]
               if c.type.name == 'MonoBehaviour' and source.class_name(c) == 'mpplayerscript']
    if len(scripts) != 1:
        raise ValueError('Remote player prefab requires one mpplayerscript')
    script = scripts[0]
    data = source.read(script)
    root_pose = world_pose(source, root)

    def child_of_root(reader):
        local = source.read(reader)
        father = source.resolve(reader, local['m_Father'])
        if father is None or key(father) != key(root):
            raise ValueError('Remote player node is not a root child')
        if any(abs(v) > 1e-6 for v in vector(local['m_LocalRotation'], 3)) or vector(local['m_LocalScale']) != (1., 1., 1.):
            raise ValueError('Remote player node is rotated or scaled')
        return local

    def local_node(transform):
        """The node of the local player rig with the same path as prefab Transform `transform`."""
        path = []
        node = transform
        while key(node) != key(root):
            path.insert(0, source.read(source.resolve(node, source.read(node)['m_GameObject']))['m_Name'])
            node = source.resolve(node, source.read(node)['m_Father'])
            if node is None:
                raise ValueError('Remote player node is outside the prefab')
        local = local_player
        for name in path:
            children = [source.resolve(local, c) for c in source.read(local)['m_Children']]
            matches = [c for c in children if source.read(source.resolve(c, source.read(c)['m_GameObject']))['m_Name'] == name]
            if len(matches) != 1:
                raise ValueError(f'Local player lacks remote path {path}')
            local = matches[0]
        return by_source[key(local)]

    # `THead` sits in the skeleton. The local player rig holds the same path.
    head = local_node(source.resolve(script, data['THead']))

    prone = child_of_root(source.resolve(script, data['TProneHeadPos']))
    head_transform = source.resolve(script, data['HeadCollider'])
    child_of_root(head_transform)
    cols = [source.resolve(script, c) for c in data['cols']]
    kinds = {}
    for collider in cols:
        raw = source.read(collider)
        if raw['m_IsTrigger'] or not raw['m_Enabled']:
            raise ValueError('Remote player collider differs')
        owner = source.transform_for(source.resolve(collider, raw['m_GameObject']))
        kinds[collider.type.name, key(owner)] = (collider, owner, raw)
    body = source.resolve(script, data['CBody'])
    body_owner = source.transform_for(source.resolve(body, source.read(body)['m_GameObject']))
    head_sphere = kinds.get(('SphereCollider', key(head_transform)))
    head_box = kinds.get(('BoxCollider', key(head_transform)))
    feet = [v for (kind, owner), v in kinds.items() if kind == 'SphereCollider' and owner != key(head_transform)]
    if ('CapsuleCollider', key(body_owner)) not in kinds or head_sphere is None or head_box is None or len(feet) != 1 or len(cols) != 4:
        raise ValueError('Remote player colliders differ')
    child_of_root(body_owner)
    feet_shape = geometry(source, None, feet[0][0], feet[0][1], root_pose, True)

    def friction(collider):
        material = source.resolve(collider, source.read(collider)['m_Material'])
        value = source.read(material)
        # PhysicMaterialCombine.Minimum is 1.
        if value['frictionCombine'] != 1 or value['dynamicFriction'] != value['staticFriction']:
            raise ValueError('Remote player material differs')
        return value['dynamicFriction']

    body_friction = friction(body)
    if friction(feet[0][0]) != body_friction or head_sphere[2]['m_Material']['m_PathID'] or head_box[2]['m_Material']['m_PathID']:
        raise ValueError('Remote player materials differ')
    mass = source.read(source.resolve(script, data['mass']))
    rigidbody = source.read(source.resolve(script, mass['RB']))
    # Constraints 112 is FreezeRotation.
    if rigidbody['m_UseGravity'] or rigidbody['m_IsKinematic'] or rigidbody['m_Constraints'] != 112 or rigidbody['m_Mass'] != mass['mass']:
        raise ValueError('Remote player rigidbody differs')
    # The bone colliders on layer 9. Each one follows its bone.
    hits = []
    for entry in source.hierarchy(root):
        if source.read(entry['game_object'])['m_Layer'] != HIT_LAYER:
            continue
        for collider in entry['components']:
            if collider.type.name not in ('BoxCollider', 'CapsuleCollider'):
                continue
            raw = source.read(collider)
            if raw['m_IsTrigger'] or not raw['m_Enabled'] or not entry['active']:
                raise ValueError('Remote player hit collider differs')
            bone = entry['parent']
            hits.append(dict(
                source=source_id(collider), node=local_node(bone),
                geometry=geometry(source, None, collider, entry['reader'], world_pose(source, bone), True),
                tag=collider_tag(source, entry['game_object'])))
    if len(hits) != HIT_COLLIDERS:
        raise ValueError('Remote player hit colliders differ')
    box = head_box[2]
    result = dict(
        source=source_id(script), head=head,
        prone_head=list(convert_point(vector(prone['m_LocalPosition']))),
        minimum_radius=data['minBodyRadius'], maximum_radius=data['maxBodyRadius'],
        head_radius=head_sphere[2]['m_Radius'],
        head_box_center=list(convert_point(vector(box['m_Center']))),
        head_box_half=[v * 0.5 for v in vector(box['m_Size'])],
        feet_center=feet_shape['center'], feet_radius=feet_shape['radius'],
        friction=body_friction, mass_kg=mass['mass'], ccd=continuous(rigidbody),
        standing_membership=1 << STANDING_LAYER, standing_filter=matrix[STANDING_LAYER],
        seated_membership=1 << SEATED_LAYER, seated_filter=matrix[SEATED_LAYER],
        hits=hits, hit_membership=1 << HIT_LAYER,
        evidence=managed_evidence(source, [
            'StartCoroutine(EnableCollidersWithDelay(3f, _enable: true));',
            'StartCoroutine(EnableCollidersWithDelay(1f, _enable: true));',
            'CBody.radius = Mathf.Lerp(minBodyRadius, maxBodyRadius, _duck);',
            'CBody.height = Vector3.Distance(hposy, TProneHeadPos.position);',
            'HeadCollider.position = THead.position;',
            'cols[i].gameObject.layer = 10;',
            'mass.RB.constraints = RigidbodyConstraints.FreezeRotation;',
            'mpscript.UpdCollider(_duckValue, _sitting);',
            'mpplayerscript componentInParent = collider.GetComponentInParent<mpplayerscript>();',
            'if (!(componentInParent != null) || collider.gameObject.layer != 9)',
            'sns.s.SDmgPlayer(componentInParent, damage * Time.deltaTime * playerDamageMultiplier);',
            'public static float playerDamageMultiplier = 0.25f;',
            'sns.s.SDmgPlayer(componentInParent, tempForce * mainscript.M.jouleToHp);',
            'mainscript.M.player.survival.Damage(d, show: true);',
        ]))
    return result
