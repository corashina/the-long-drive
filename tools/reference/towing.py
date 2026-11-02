"""Source trailers (`utanfutoscript`, `attachgripscript`) and trailer hitch targets of world prototypes.

Ids are local to one world prototype: node, body and collider indices.
"""
from .gltf import convert_point
from .source import key, source_id, vector
from .wheels import wheel_collider_record

HITCH_TYPE = 'utanfuto'


def scripts(source, hierarchy, name):
    return [(n, c) for n in hierarchy for c in n['components']
        if c.type.name == 'MonoBehaviour' and source.class_name(c) == name]


def recover_hitches(source, hierarchy, mapping, colliders):
    """`attachTargetscript` entries with tipus `utanfuto`. Each has one collider on its node."""
    hitches = []
    for n, component in scripts(source, hierarchy, 'attachTargetscript'):
        data = source.read(component)
        if HITCH_TYPE not in (data['tipus'] or []):continue
        target = source.resolve(component, data['target'])
        if target is None or key(target) not in mapping:
            raise ValueError(f'{key(component)} target: hitch target outside its prototype')
        node = mapping[key(n['reader'])]
        own = [i for i, c in enumerate(colliders) if c['node'] == node]
        if len(own) != 1:raise ValueError(f'{key(component)}: a hitch needs one collider on its node')
        hitches.append({'source': source_id(component), 'node': node, 'target': mapping[key(target)],
            'collider': own[0], 'types': list(data['tipus'])})
    return hitches


def recover_trailer(source, hierarchy, mapping, bodies, root_point, lights):
    """The root `utanfutoscript` and `attachgripscript` of a world prototype, or None.

    `root_point(transform)` gives a converted point in the root frame. `lights()` gives the
    light controller records of the prototype.
    """
    found = scripts(source, hierarchy, 'utanfutoscript')
    if not found:return None
    node, script = found[0]
    identity = key(script)
    if len(found) > 1 or node is not hierarchy[0]:raise ValueError(f'{identity}: utanfutoscript must be on the prototype root only')
    data = source.read(script)
    def required(owner, pointer, field):
        target = source.resolve(owner, pointer)
        if target is None:raise ValueError(f'{key(owner)} {field}: missing value')
        return target
    rb = required(script, data['RB'], 'RB')
    body = next((i for i, b in enumerate(bodies) if b['source'] == source_id(rb)), None)
    if body is None or bodies[body]['node'] != 0:raise ValueError(f'{identity} RB: the trailer body must be the root body')
    wheels = []
    for i, pointer in enumerate(data['wheels']):
        wheel = required(script, pointer, f'wheels[{i}]')
        if wheel.type.name != 'WheelCollider':raise ValueError(f'{identity} wheels[{i}]: not a WheelCollider')
        transform = source.transform_for(source.resolve(wheel, source.read(wheel)['m_GameObject']))
        if key(transform) not in mapping:raise ValueError(f'{identity} wheels[{i}]: wheel outside its prototype')
        wheels.append({'source': source_id(wheel), 'node': mapping[key(transform)],
            'collider': wheel_collider_record(source.read(wheel))})
    if not wheels:raise ValueError(f'{identity}: trailer has no wheels')
    # wheelgraphicsscript.Graphics poses `T` from its wheel. Trailers use neither W2 nor keepUpright.
    graphics = {}
    for _, graphic in scripts(source, hierarchy, 'wheelgraphicsscript'):
        g = source.read(graphic)
        wheel = source.resolve(graphic, g['W'])
        if wheel is None:continue
        if source.resolve(graphic, g['W2']) is not None or g['keepUpright']:
            raise ValueError(f'{key(graphic)}: unsupported trailer wheel graphics W2 or keepUpright')
        target = required(graphic, g['T'], 'T')
        if key(target) not in mapping:raise ValueError(f'{key(graphic)} T: outside its prototype')
        if key(wheel) in graphics:raise ValueError(f'{key(graphic)}: wheel has two graphics')
        graphics[key(wheel)] = mapping[key(target)]
    wheel_keys = [key(required(script, pointer, 'wheels')) for pointer in data['wheels']]
    if any(k not in graphics for k in wheel_keys):raise ValueError(f'{identity}: a trailer wheel has no graphics')
    brake = source.resolve(script, data['brake'])
    if brake is not None and (brake.type.name != 'MonoBehaviour' or source.class_name(brake) != 'flashlightscript'
            or not any(key(c) == key(brake) for n in hierarchy for c in n['components'])):
        raise ValueError(f'{identity} brake: not a flashlightscript of the trailer')
    grips = scripts(source, hierarchy, 'attachgripscript')
    if len(grips) != 1 or grips[0][0] is not hierarchy[0]:raise ValueError(f'{identity}: a trailer needs one root attachgripscript')
    grip_script = grips[0][1]
    grip = source.read(grip_script)
    if not grip['fixGripPos']:raise ValueError(f'{key(grip_script)}: unsupported grip without fixGripPos')
    if grip['grips'] or source.resolve(grip_script, grip['targetTosave']) is not None:
        raise ValueError(f'{key(grip_script)}: unsupported chained or preattached grip')
    prefab = required(grip_script, grip['GripPrefab'], 'GripPrefab')
    attachables = [c for c in source.components(source.transform_for(prefab))[1]
        if c.type.name == 'MonoBehaviour' and source.class_name(c) == 'attachablescript']
    if len(attachables) != 1:raise ValueError(f'{key(prefab)}: the grip needs one root attachablescript')
    attachable = source.read(attachables[0])
    if not attachable['onlyToPoint']:raise ValueError(f'{key(attachables[0])}: unsupported grip without onlyToPoint')
    def point(field):
        # Trailer prefabs link a GameObject or its Transform.
        target = required(script, data[field], field)
        return root_point(target if target.type.name == 'Transform' else source.transform_for(target))
    controllers = lights()
    rear = [record for record in controllers if record['kind'] == 'rear']
    if len(rear) != len(controllers):raise ValueError(f'{identity}: a trailer has only rearlights controllers')
    return {'source': source_id(script), 'body': body, 'wheels': wheels, 'graphics': [graphics[k] for k in wheel_keys],
        'center_of_mass': point('CenterOfMass'), 'center_of_mass_hitched': point('CenterOfMassAttached'),
        'brake_force': float(data['brakeForce']), 'brake': source_id(brake) if brake is not None else None,
        'grip': {'source': source_id(grip_script), 'prototype': source_id(source.transform_for(prefab)),
            'position': list(convert_point(vector(grip['VgripPos']))), 'types': list(attachable['tipus'] or [])},
        'lights': rear}

