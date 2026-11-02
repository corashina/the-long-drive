"""Start menu figure (`mainmenuscript.menuCharacter`, `menuIkScript`) in the level0 menu scene.
All poses are local to the figure root `MenuCharacter`. The assets are not in the cloud, so this reader fails loudly."""
from .collision import quaternion_product
from .appearance_preview import LIGHT_KINDS, camera_record, figure_record, light_record, stage_pose, transform_of, valid_pose, validate_figure

SCRIPT = 'menuIkScript'
SCENE = 'level0'
PLAYER_TAG = 6
FLAT_AMBIENT = 3
SCALARS = dict(mouse_forward='mouseForward', mouse_forward_l='mouseForwardL', mouse_forward_h='mouseForwardH',
    max_lerp='maxLerp', lerp_speed='lerpSpeed', rot_lerp_speed='rotlerpSpeed', body_stance='bodystance')
# `HumanBodyBones` names of the bones that the humanoid IK moves.
BONES = dict(hips='Hips', neck='Neck', head='Head',
    left_upper_arm='LeftUpperArm', left_lower_arm='LeftLowerArm', left_hand='LeftHand',
    right_upper_arm='RightUpperArm', right_lower_arm='RightLowerArm', right_hand='RightHand',
    left_upper_leg='LeftUpperLeg', left_lower_leg='LeftLowerLeg', left_foot='LeftFoot',
    right_upper_leg='RightUpperLeg', right_lower_leg='RightLowerLeg', right_foot='RightFoot')


def node_name(source, transform):
    return source.read(source.resolve(transform, source.read(transform)['m_GameObject']))['m_Name']


def relative_pose(source, reader, stage):
    """Pose of Transform `reader` in the space of `stage`, which has unit scale. They need not be related."""
    from .collision import world_pose
    from .gltf import transform_point
    s, n = world_pose(source, stage), world_pose(source, reader)
    if any(abs(v - 1) > 1e-5 for v in s['scale']):
        raise ValueError('Menu figure root scale')
    x, y, z, w = s['rotation']
    inverse = (-x, -y, -z, w)
    offset = tuple(n['translation'][i] - s['translation'][i] for i in range(3))
    return {'translation': list(transform_point(offset, (0., 0., 0.), inverse, (1., 1., 1.))),
        'rotation': list(quaternion_product(inverse, n['rotation'])), 'scale': n['scale']}


def scene_object(source, type_name):
    found = [o for o in source.objects.values() if o.type.name == type_name and o.assets_file.source_container_path == SCENE]
    if len(found) != 1:
        raise ValueError(f'Expected one {SCENE} {type_name}, found {len(found)}')
    return found[0]


def recover_menu_figure(source):
    from .world_biome_materials import scene_component
    script = scene_component(source, source.library, SCRIPT, SCENE)
    d = source.read(script)
    figure = source.resolve(script, d['outfit'])
    if figure is None or source.class_name(figure) != 'playermodeloutfitscript':
        raise ValueError('Menu figure outfit is missing')
    stage = transform_of(source, figure)
    def link(field):
        reader = source.resolve(script, d[field])
        if reader is None or reader.type.name not in ('Transform', 'RectTransform'):
            raise ValueError(f'Menu figure {field} is missing')
        return reader
    def parent_name(reader):
        return node_name(source, source.resolve(reader, source.read(reader)['m_Father']))
    camera = source.resolve(script, d['cam'])
    c = source.read(camera)
    settings = source.read(scene_object(source, 'RenderSettings'))
    if settings['m_AmbientMode'] != FLAT_AMBIENT:
        raise ValueError('Menu ambient is not flat')
    sun = source.resolve(scene_object(source, 'RenderSettings'), settings['m_Sun'])
    if sun is None or sun.type.name != 'Light':
        raise ValueError('Menu sun is missing')
    collider = next((r for r in source.components(stage)[1] if r.type.name == 'CapsuleCollider'), None)
    if collider is None:
        raise ValueError('Menu figure collider is missing')
    cd = source.read(collider)
    tag = source.read(source.resolve(stage, source.read(stage)['m_GameObject']))['m_Tag']
    if tag != PLAYER_TAG or not cd['m_Enabled'] or cd['m_IsTrigger'] or cd['m_Direction'] != 1:
        raise ValueError('Menu figure collider')
    animator = source.resolve(script, d['anim'])
    avatar = source.resolve(animator, source.read(animator)['m_Avatar'])
    human = {h['m_HumanName']: h['m_BoneName'] for h in source.read(avatar)['m_HumanDescription']['m_Human']}
    lh, rh = link('LH'), link('RH')
    pose = lambda field: stage_pose(source, link(field), stage)
    return {
        'camera': camera_record(source, camera, lambda t: relative_pose(source, t, stage)),
        'background': [c['m_BackGroundColor'][k] for k in 'rgb'],
        'ambient': [settings['m_AmbientSkyColor'][k] * settings['m_AmbientIntensity'] for k in 'rgb'],
        'lights': [light_record(source, sun, relative_pose(source, transform_of(source, sun), stage))],
        'figure': figure_record(source, figure, stage),
        'body': pose('Body'),
        'collider': {'center': [cd['m_Center'][k] for k in 'xyz'], 'radius': cd['m_Radius'], 'height': cd['m_Height']},
        'ik': {
            **{name: d[field] for name, field in SCALARS.items()},
            'b': pose('B'), 'h': pose('H'), 'll': pose('LL'), 'rl': pose('RL'),
            'lh': {'parent': parent_name(lh), 'pose': stage_pose(source, lh, stage)},
            'rh': {'parent': parent_name(rh), 'pose': stage_pose(source, rh, stage)},
            'llu': node_name(source, link('LLU')), 'rhu': node_name(source, link('RHU')),
            'bones': {name: human[field] for name, field in BONES.items()},
        },
    }


def validate_menu_figure(menu, clothing, manifest):
    camera = menu['camera']
    if not valid_pose(camera['pose']) or not 0 < camera['fov'] < 180 or not 0 < camera['near'] < camera['far']:
        raise ValueError('Menu camera')
    for l in menu['lights']:
        if l['kind'] != LIGHT_KINDS[1] or l['intensity'] < 0 or not valid_pose(l['pose']):
            raise ValueError('Menu light')
    if len(menu['background']) != 3 or len(menu['ambient']) != 3:
        raise ValueError('Menu colors')
    validate_figure(menu['figure'], clothing, manifest)
    ik = menu['ik']
    # `Body.position = B.position` moves the figure root, so both start at the same pose.
    if menu['body'] != ik['b'] or not valid_pose(ik['b']):
        raise ValueError('Menu figure body')
    c = menu['collider']
    if len(c['center']) != 3 or c['radius'] <= 0 or c['height'] <= 0:
        raise ValueError('Menu figure collider')
    if any(not ik[name] > 0 for name in SCALARS):
        raise ValueError('Menu IK scalars')
    joints = {j['name'] for j in menu['figure']['joints']}
    names = [ik['lh']['parent'], ik['rh']['parent'], ik['llu'], ik['rhu'], *ik['bones'].values()]
    if not all(n in joints for n in names) or set(ik['bones']) != set(BONES):
        raise ValueError('Menu IK bones')
    if not all(valid_pose(p) for p in (ik['h'], ik['ll'], ik['rl'], ik['lh']['pose'], ik['rh']['pose'])):
        raise ValueError('Menu IK targets')
