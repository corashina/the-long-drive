"""Recover the selected player's continuous posture and gameplay view."""
import math
from .source import key, source_id
from .gltf import converted_transform
from .collision import geometry, world_pose
from .survival import managed_evidence


VIEW_LITERALS = dict(tap_seconds=0.2, return_window_seconds=0.5, return_rate=10.0,
    free_look_limit_degrees=90.0, zoom_multiplier=0.4, zoom_in_rate=10.0,
    zoom_out_rate=20.0, lean_scroll_multiplier=5.0, height_scroll_step=0.05)


def recover_player_movement(source):
    reader = source.get('level1', 80125)
    data = source.read(reader)
    managed_evidence(source, [
        'if (input.leanUp && leancharge < 0.2f)',
        'if (Time.time - lookTime < 0.5f)',
        'FLookXRot = Mathf.Lerp(FLookXRot, 0f, dt * 10f);',
        'FLookXRot = Mathf.Clamp(FLookXRot - LOOKY, -90f, 90f);',
        'FmFovZoom = (zooming ? Mathf.Lerp(FmFovZoom, 0.4f, dt * 10f) : Mathf.Lerp(FmFovZoom, 1f, dt * 20f));',
        'Tb.transform.up * scroll * 5f',
        'FLookPos = Mathf.Clamp(FLookPos + scroll * 0.05f, 0f - FminLook, FmaxLook);',
        'F = 1f / (float)num * (float)i;',
        'T.position = pTStart.position + Vector3.up * (pDist / (float)num * (float)i);',
        'public class sens : MonoBehaviour\n{\n\tpublic Collider selfCol;\n\n\tpublic int v;',
        'if (v > 0)\n\t\t{\n\t\t\tv--;\n\t\t\treturn;\n\t\t}',
        'private void OnTriggerStay(Collider c)\n\t{\n\t\tT = c.transform;\n\t\tsensed = true;\n\t\tv = 3;\n\t}',
        'RB.velocity = zero * FladderSpeed * TLadder.speed * (input.run ? FrunM : 1f);',
        'currentAdd = dt * walkBobSpeed + tempRBfloat * walkBobSpeedRB * dt;',
        'ws = Mathf.Sin(walkBobState) * maxWalkBob;',
        'if (ws <= 0f - walkbobSoundF)',
        'RB.AddForce(dir * FairM * dt);',
        'FmaxSpeed = Mathf.Clamp(FmaxSpeed, 0f - speedWhenJumped, speedWhenJumped);',
        'RB.velocity = Vector3.ClampMagnitude(new Vector3(RB.velocity.x, 0f, RB.velocity.z), velocity.magnitude * Mathf.InverseLerp(90f, 0f, Vector3.Angle(RB.velocity, velocity)) + FmaxSpeed);',
        'if (!canwalljump2.sensed && canwalljump.sensed && Bwalljump2ed)',
        'RB.AddForce(Tb.up * FjumpForce * FwallJumpM);',
        'RB.AddForce(Tb.up * FjumpForce * FwallJumpM2);',
        'if (RB.velocity.y < 0f && RB.velocity.magnitude <= FmaxGripFallSpeed)',
        'RB.velocity = new Vector3(RB.velocity.x, Mathf.Clamp(RB.velocity.y, FGripLoose, FmaxFall), RB.velocity.z);',
        'cols = Physics.OverlapSphere(transform.position, radius, LM);',
    ])
    standing = source.resolve(reader, data['TStandHeadPos'])
    prone = source.resolve(reader, data['TProneHeadPos'])
    default_standing = source.resolve(reader, data['TdefStandPos'])
    owner = source.transform_for(source.resolve(reader, data['m_GameObject']))
    body_pose = world_pose(source, owner)
    stand_pose = converted_transform(source.read(standing))
    prone_pose = converted_transform(source.read(prone))
    distance = math.dist(world_pose(source, prone)['translation'],
                         world_pose(source, default_standing)['translation'])
    physics = source.read(source.get('globalgamemanagers', 10))
    matrix = physics['m_LayerCollisionMatrix']
    if len(matrix) != 32 or data['sensNum'] != 10:
        raise ValueError('Player clearance count or layer matrix differs')

    def sphere(collider, local_pose):
        raw = source.read(collider)
        game_object = source.resolve(collider, raw['m_GameObject'])
        transform = source.transform_for(game_object)
        layer = source.read(game_object)['m_Layer']
        if collider.type.name != 'SphereCollider' or not raw['m_Enabled']:
            raise ValueError('Player sphere collider differs')
        shape = geometry(source, None, collider, transform, local_pose, False)
        return dict(source=source_id(collider), center=shape['center'], radius=shape['radius'],
                    membership=1 << layer, filter=matrix[layer])

    def sensor_collider(field, kind):
        sens = source.resolve(reader, data[field])
        game_object = source.resolve(sens, source.read(sens)['m_GameObject'])
        colliders = [r for r in source.components(source.transform_for(game_object))[1] if r.type.name.endswith('Collider')]
        if source.class_name(sens) != 'sens' or len(colliders) != 1 or colliders[0].type.name != kind \
                or not source.read(colliders[0])['m_IsTrigger']:
            raise ValueError(f'Player {field} sensor differs')
        return colliders[0]

    def capsule_sensor(collider, local_pose):
        raw = source.read(collider)
        game_object = source.resolve(collider, raw['m_GameObject'])
        layer = source.read(game_object)['m_Layer']
        if not raw['m_Enabled'] or raw['m_Direction'] != 1:
            raise ValueError('Player capsule sensor differs')
        shape = geometry(source, None, collider, source.transform_for(game_object), local_pose, False)
        return dict(source=source_id(collider), a=shape['a'], b=shape['b'], radius=shape['radius'],
                    membership=1 << layer, filter=matrix[layer])

    wall_sensors = dict(wall=capsule_sensor(sensor_collider('canwalljump', 'CapsuleCollider'), body_pose),
                        ledge=sphere(sensor_collider('canwalljump2', 'SphereCollider'), body_pose))
    interior = source.resolve(reader, data['interiorsens'])
    if key(interior) != ('level1', 80121) or source.class_name(interior) != 'sens2':
        raise ValueError('Player interior sensor differs')
    interior_data = source.read(interior)
    # The sensor pose is local to the head.
    interior_transform = source.read(source.transform_for(source.resolve(interior, interior_data['m_GameObject'])))
    interior_sensor = dict(source=source_id(interior), radius=interior_data['radius'], mask=interior_data['LM']['m_Bits'],
                           local_position=converted_transform(interior_transform)['translation'])

    capsule = source.resolve(reader, data['CBody'])
    capsule_game_object = source.resolve(capsule, source.read(capsule)['m_GameObject'])
    capsule_layer = source.read(capsule_game_object)['m_Layer']
    if key(capsule) != ('level1', 58456) or capsule.type.name != 'CapsuleCollider' or capsule_layer != 12:
        raise ValueError('Player capsule layer differs')
    feet_reader = source.resolve(reader, data['Feet2'])
    feet = sphere(feet_reader, body_pose)
    prefab = source.resolve(reader, data['sensPrefab'])
    if key(prefab) != ('sharedassets1.assets', 42627):
        raise ValueError('Player clearance prefab differs')
    prefab_transform = source.transform_for(prefab)
    colliders = [r for r in source.components(prefab_transform)[1] if r.type.name == 'SphereCollider']
    if len(colliders) != 1 or not source.read(colliders[0])['m_IsTrigger']:
        raise ValueError('Player clearance sphere differs')
    template = sphere(colliders[0], world_pose(source, prefab_transform))
    clearance = []
    for index in range(1, data['sensNum'] + 1):
        fraction = index / data['sensNum']
        center = [prone_pose['translation'][axis] + template['center'][axis]
                  + (distance * fraction if axis == 1 else 0.0) for axis in range(3)]
        clearance.append(dict(posture=fraction, sphere={**template, 'center': center}))
    result = dict(
        posture=dict(capsule_membership=1 << capsule_layer, capsule_filter=matrix[capsule_layer], standing_head=stand_pose, prone_head=prone_pose,
            minimum_radius=data['minBodyRadius'], maximum_radius=data['maxBodyRadius'],
            ctrl_value=data['ctrlDuckValue'], scroll_step=data['duckSens'], rise_rate=data['FduckLerp'],
            feet=feet, clearance=clearance, sensor_hold_steps=3),
        view=dict(head_lerp=data['FHeadLerp'], lean_sensitivity=data['FsensLean'],
            lean_distance=data['FleanDist'], height_limits=[-data['FminLook'], data['FmaxLook']], **VIEW_LITERALS),
        prone_multiplier=data['FProneM'], duck_jump_multiplier=data['FduckJumpM'],
        inverse_duck_multiplier=data['FinverseDuck'], max_fall_speed=data['FmaxFall'],
        ladder_speed=data['FladderSpeed'], walk_bob_speed=data['walkBobSpeed'], walk_bob_speed_rb=data['walkBobSpeedRB'],
        max_walk_bob=data['maxWalkBob'], walk_bob_sound_threshold=data['walkbobSoundF'],
        max_depenetration_velocity=physics['m_DefaultMaxDepenetrationVelocity'],
        air_force=data['FairM'], grip_multiplier=data['FGripM'], wall_jump_multiplier=data['FwallJumpM'],
        ledge_jump_multiplier=data['FwallJumpM2'], grip_loose=data['FGripLoose'],
        max_grip_fall_speed=data['FmaxGripFallSpeed'], wall_sensors=wall_sensors, interior_sensor=interior_sensor)
    return result

