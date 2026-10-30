"""Recover action owners from component links and source startup rules."""
from .source import key


def recover_masks(player):
    fields = {'use_mask': 'useLayer', 'pickup_mask': 'pickupLayer',
        'attach_mask': 'attachLayer', 'push_mask': 'pushLayer'}
    result = {}
    for name, field in fields.items():
        mask = player[field]['m_Bits']
        if not isinstance(mask, int) or not 0 < mask <= 0xffffffff:
            raise ValueError(f'fpscontroller.{field}: invalid action mask')
        result[name] = mask
    return result


def recover_bindings(source, nodes, components, item_nodes, colliders, mechanisms, openings):
    component_items = {}
    for node, group in components.items():
        for readers in group.values():
            for reader in readers:
                if node in item_nodes:
                    component_items[key(reader)] = item_nodes[node]
    uses = {(r['source']['file'], r['source']['path_id']): r['item'] for r in mechanisms}
    uses.update({(r['source']['file'], r['source']['path_id']): r['cap'] for r in openings})
    layers = {}
    # pickupable.LayerStart assigns cols[].p after layerScript.FStart.
    for node, group in components.items():
        if not nodes[node - 1]['active']:
            continue
        for reader in group.get('pickupable', []):
            value = source.read(reader)
            if key(reader) not in component_items:
                raise ValueError(f'pickupable owner missing: {key(reader)}')
            for pointer in value['cols']:
                layer = source.resolve(reader, pointer)
                if layer is None or source.class_name(layer) != 'layerScript':
                    raise ValueError(f'pickupable.cols: invalid layerScript link: {key(reader)}')
                identity = key(layer)
                if identity in layers and layers[identity] != key(reader):
                    raise ValueError(f'pickupable.cols: duplicate owner: {identity}')
                layers[identity] = key(reader)
    def ancestor(node, classes):
        while node is not None:
            group = components[node]
            for name in classes:
                readers = group.get(name, [])
                if readers:
                    if len(readers) != 1:
                        raise ValueError(f'{name}: multiple action owners on node {node}')
                    return readers[0]
            node = nodes[node - 1]['parent']
        return None
    def item(reader, field):
        if reader is None:
            return None
        identity = key(reader)
        if identity not in component_items:
            raise ValueError(f'{field}: owner outside selected items: {identity}')
        return component_items[identity]
    result = []
    for collider in colliders:
        node = collider['node']
        use = ancestor(node, ('tankcapscript', 'usablescript', 'seatscript', 'flashlightscript'))
        use_owner = uses.get(key(use), item(use, 'use owner')) if use else None
        pickup = ancestor(node, ('pickupable',))
        pickup_owner = None
        if pickup is not None and not source.read(pickup)['cantPickup']:
            pickup_owner = item(pickup, 'pickupable')
        removal_owner = None
        for slot in components[node].get('partslotscript', []):
            part = source.resolve(slot, source.read(slot)['part'])
            if part is not None:
                if source.class_name(part) != 'partscript':
                    raise ValueError(f'partslotscript.part: invalid partscript: {key(slot)}')
                pick = source.resolve(part, source.read(part)['p'])
                if pick is None or source.class_name(pick) != 'pickupable':
                    raise ValueError(f'partscript.p: invalid pickupable: {key(part)}')
                removal_owner = item(pick, 'partslotscript.part.p')
        layer_readers = components[node].get('layerScript', [])
        for layer in layer_readers:
            value = source.read(layer)
            if value['col']['m_PathID']:
                linked = source.resolve(layer, value['col'])
                if linked is None or not linked.type.name.endswith('Collider'):
                    raise ValueError(f'layerScript.col: invalid collider: {key(layer)}')
            # The player queries layerScript on the hit GameObject. Its p owner
            # applies to each Collider on that GameObject, regardless of col.
            owner_id = layers.get(key(layer))
            owner = source.get(*owner_id) if owner_id else source.resolve(layer, value['p'])
            if owner is not None:
                if source.class_name(owner) != 'pickupable':
                    raise ValueError(f'layerScript.p: invalid pickupable: {key(layer)}')
                part = source.resolve(owner, source.read(owner)['part'])
                if part is not None:
                    if source.class_name(part) != 'partscript':
                        raise ValueError(f'pickupable.part: invalid partscript: {key(owner)}')
                    if removal_owner is None:
                        removal_owner = item(owner, 'layerScript.p.part')
        if removal_owner is None and pickup is not None:
            part = source.resolve(pickup, source.read(pickup)['part'])
            if part is not None:
                if source.class_name(part) != 'partscript':
                    raise ValueError(f'pickupable.part: invalid partscript: {key(pickup)}')
                removal_owner = item(pickup, 'pickupable.part')
        push = ancestor(node, ('pushablescript',))
        result.append({'collider': collider['id'], 'use_owner': use_owner,
            'pickup_owner': pickup_owner, 'removal_owner': removal_owner,
            'push_owner': item(push, 'pushablescript')})
    return result


def recover_pickup(source, reader):
    import math
    from .source import source_id, vector
    from .gltf import convert_point, convert_rotation
    from .collision import quaternion_product
    fields = source.read(reader)
    def rotation_of(field):
        x, y, z = [math.radians(v) * .5 for v in vector(fields[field])]
        return convert_rotation(quaternion_product((0., math.sin(y), 0., math.cos(y)),
            quaternion_product((math.sin(x), 0., 0., math.cos(x)), (0., 0., math.sin(z), math.cos(z)))))
    def link(field, kind):
        target = source.resolve(reader, fields[field])
        if target is None:
            return None
        if target.type.name != kind:
            raise ValueError(f'pickupable.{field}: expected {kind}: {key(reader)}')
        if kind == 'Sprite':
            if not hasattr(source, 'pickup_icons'): source.pickup_icons = set()
            source.pickup_icons.add(key(target))
        return source_id(target)
    owner = source.transform_for(source.resolve(reader, fields['m_GameObject']))
    stored = link('disableThisWhenStored', 'Transform')
    # pickupable.FStart assigns the owner Transform when this pointer is null.
    if stored is None:
        stored = source_id(owner)
    return {'source': source_id(reader), 'cant_pickup': bool(fields['cantPickup']),
        'cant_pickup_while_mounted': bool(fields['cantPickupWhileMount']),
        'cant_pickup_while_seated': bool(fields['cantPickupWhileSitting']),
        'no_rotation': bool(fields['noRot']), 'move_with_rotation': bool(fields['moveWithRot']),
        'use_recenter_position': bool(fields['useRecenter']),
        'recenter_position': convert_point(vector(fields['recenterPos'])),
        'use_recenter_rotation': bool(fields['useRecenterRot']),
        'recenter_rotation': rotation_of('recenterRot'), 'recenter_distance': fields['recenterDist'],
        'can_inventory': bool(fields['canInventory']), 'can_hold_right_hand': bool(fields['canHoldRightHand']),
        'disable_rh_fix': bool(fields['disableRHFix']), 'right_hand_position': convert_point(vector(fields['RHP'])),
        'right_hand_rotation': rotation_of('RHR'), 'right_hand_grip': link('RightHandPos', 'Transform'),
        'left_hand_grip': link('LeftHandPos', 'Transform'), 'double_hand': bool(fields['doubleHand']),
        'disable_when_stored': stored,
        'keep_when_stored': link('dontDisableWhenStored', 'Transform'), 'inventory_icon': link('invImg', 'Sprite')}


def push_record(source, reader, node, body_pose, grips, node_of, colliders_of):
    """pushablescript values and the pushgripable whose car is on `node`.
    `node_of` maps a Transform to a node id. `colliders_of` maps a node id to its collider ids.
    The grip pose is relative to the body of `body_pose`."""
    from .source import source_id
    from .collision import world_pose, bake_points, quaternion_product
    fields = source.read(reader)
    record = {'source': source_id(reader), 'modifier': fields['modifier'],
        'only_projected': bool(fields['onlyProjected']), 'both': bool(fields['both']),
        'push_even_if_same_parent': bool(fields['pushEvenIfSameParent']), 'grip': None}
    for grip_reader in grips:
        grip = source.read(grip_reader)
        car = source.resolve(grip_reader, grip['car'])
        if car is None:
            raise ValueError(f'pushgripable.car: missing carscript: {key(grip_reader)}')
        car_go = source.resolve(car, source.read(car)['m_GameObject'])
        if node_of(source.transform_for(car_go)) != node:
            continue
        if record['grip'] is not None:
            raise ValueError(f'Multiple push grips on one car: {key(reader)}')
        transform = source.transform_for(source.resolve(grip_reader, grip['m_GameObject']))
        grip_node = node_of(transform)
        # A null HT moves the nearest hand to the grip Transform.
        hand = source.resolve(grip_reader, grip['HT'])
        rotation = body_pose['rotation']
        inverse = [-rotation[0], -rotation[1], -rotation[2], rotation[3]]
        record['grip'] = {'source': source_id(grip_reader), 'node': grip_node,
            'hand_node': node_of(hand) if hand is not None else None,
            'colliders': colliders_of(grip_node),
            'local_pose': {'translation': bake_points([(0., 0., 0.)], source.chain(transform), body_pose)[0],
                'rotation': list(quaternion_product(inverse, world_pose(source, transform)['rotation'])), 'scale': [1., 1., 1.]},
            'in_use': bool(grip['inuse'])}
    return record


def recover_pushables(source, nodes, components, item_nodes, colliders, bodies, mask):
    by_source = {(n['source']['file'], n['source']['path_id']): n['id'] for n in nodes}
    grips = [reader for node, group in components.items() if nodes[node - 1]['active']
        for reader in group.get('pushgripable', [])]
    records = []
    for node, group in components.items():
        if not nodes[node - 1]['active']:
            continue
        for reader in group.get('pushablescript', []):
            if not source.read(reader)['m_Enabled']:
                continue
            body = next(b for b in bodies if b['node'] == node)
            record = push_record(source, reader, node, body['pose'], grips, lambda t: by_source[key(t)],
                lambda n: [c['id'] for c in colliders if c['node'] == n])
            records.append({'owner': item_nodes[node], 'body': body['id'], 'mask': mask, **record})
    return records


def recover_world_pushes(source, hierarchy, mapping, bodies, colliders):
    """pushablescript records of one world prototype. Ids are prototype node, body and collider indices.
    `source.chain` must stop at the prototype root. pushablescript.Push needs a Rigidbody on its node."""
    def scripts(name):
        return [(i, c) for i, n in enumerate(hierarchy) if n['active'] for c in n['components']
            if c.type.name == 'MonoBehaviour' and source.class_name(c) == name]
    grips = [c for _, c in scripts('pushgripable')]
    records = []
    for node, reader in scripts('pushablescript'):
        if not source.read(reader)['m_Enabled']:
            continue
        body = next((i for i, b in enumerate(bodies) if b['node'] == node), None)
        if body is None:
            continue
        def node_of(transform):
            if key(transform) not in mapping:
                raise ValueError(f'pushgripable: Transform outside its prototype: {key(transform)}')
            return mapping[key(transform)]
        record = push_record(source, reader, node, bodies[body]['pose'], grips, node_of,
            lambda n: [i for i, c in enumerate(colliders) if c['node'] == n])
        records.append({'node': node, 'body': body, **record})
    return records
