"""Appearance editor preview stage (`settingsplayercustomscript.camStuff`): camera, lights and figure.
All poses are local to `camStuff`. The assets are not in the cloud, so this reader fails loudly."""
import math
from .source import key, source_id, vector
from .gltf import convert_point, convert_rotation, transform_point
from .collision import quaternion_product

SCRIPT = 'settingsplayercustomscript'
PLAYER_OUTFIT = ('level1', 70544)
LIGHT_KINDS = {0: 'spot', 1: 'directional', 2: 'point'}


def preview_script(source):
    """The one `settingsplayercustomscript` in level1."""
    from .world_biome_materials import scene_component
    # The main menu scene has its own copy.
    return scene_component(source, source.library, SCRIPT, 'level1')


def rect(source, reader):
    """Unity `RectTransform.rect` of `reader` as min x, min y, width and height. A plain Transform has a zero rect."""
    d = None if reader is None else source.read(reader)
    if d is None or 'm_AnchorMin' not in d:
        return (0., 0., 0., 0.)
    p = rect(source, source.resolve(reader, d['m_Father']))
    size = [(d['m_AnchorMax'][a] - d['m_AnchorMin'][a]) * p[2 + i] + d['m_SizeDelta'][a] for i, a in enumerate('xy')]
    return (-d['m_Pivot']['x'] * size[0], -d['m_Pivot']['y'] * size[1], *size)


def local_position(source, reader, d):
    """Unity local position. A RectTransform takes x and y from its anchors and `m_AnchoredPosition`.
    Its serialized `m_LocalPosition` x and y can be stale."""
    if 'm_AnchorMin' not in d:
        return vector(d['m_LocalPosition'])
    p = rect(source, source.resolve(reader, d['m_Father']))
    xy = [p[i] + p[2 + i] * (d['m_AnchorMin'][a] + (d['m_AnchorMax'][a] - d['m_AnchorMin'][a]) * d['m_Pivot'][a])
          + d['m_AnchoredPosition'][a] for i, a in enumerate('xy')]
    return (*xy, d['m_LocalPosition']['z'])


def stage_pose(source, reader, stage):
    """Pose of Transform `reader` in the space of its ancestor `stage`."""
    position, rotation, scale = (0., 0., 0.), (0., 0., 0., 1.), (1., 1., 1.)
    seen = set()
    while key(reader) != key(stage):
        if key(reader) in seen:
            raise ValueError('Transform parent cycle')
        seen.add(key(reader))
        d = source.read(reader)
        local = vector(d['m_LocalScale'])
        position = transform_point(position, local_position(source, reader, d), vector(d['m_LocalRotation'], 4), local)
        rotation = quaternion_product(vector(d['m_LocalRotation'], 4), rotation)
        scale = tuple(scale[i] * local[i] for i in range(3))
        reader = source.resolve(reader, d['m_Father'])
        if reader is None:
            raise ValueError('Preview node is not under camStuff')
    return {'translation': list(convert_point(position)), 'rotation': list(convert_rotation(rotation)), 'scale': list(scale)}


def transform_of(source, component):
    return source.transform_for(source.resolve(component, source.read(component)['m_GameObject']))


def camera_record(source, camera, pose):
    """`pose(transform)` gives the stage pose of a Transform."""
    if camera is None or camera.type.name != 'Camera':
        raise ValueError('Preview camera is missing')
    c = source.read(camera)
    if c['orthographic']:
        raise ValueError('Preview camera is orthographic')
    return {'pose': pose(transform_of(source, camera)), 'fov': c['field of view'],
        'near': c['near clip plane'], 'far': c['far clip plane']}


def light_record(source, light, pose):
    l = source.read(light)
    kind = LIGHT_KINDS.get(l['m_Type'])
    if kind is None:
        raise ValueError(f'Unsupported preview light type {l["m_Type"]}')
    return {'kind': kind, 'color': [l['m_Color'][k] for k in 'rgba'], 'intensity': l['m_Intensity'],
        'range': l['m_Range'], 'spot_angle': l['m_SpotAngle'], 'pose': pose}


def figure_record(source, figure, stage):
    """A `playermodeloutfitscript` figure: root pose and the skin joint poses of its selected character."""
    f = source.read(figure)
    character = f['selectedCharacter']
    if not 0 <= character < len(f['characters']):
        raise ValueError('Preview figure character')
    owner = source.resolve(figure, f['characters'][character])
    if owner is None:
        raise ValueError('Preview figure character is missing')
    bones, seen = [], set()
    for pointer in source.read(owner)['ch']['renderers']:
        renderer = source.resolve(owner, pointer)
        if renderer is None or renderer.type.name != 'SkinnedMeshRenderer':
            continue
        for bone in source.read(renderer)['m_Bones']:
            bone = source.resolve(renderer, bone)
            if bone is not None and key(bone) not in seen:
                seen.add(key(bone))
                bones.append(bone)
    if not bones:
        raise ValueError('Preview figure has no skin joints')
    joints = [{'name': source.read(source.resolve(b, source.read(b)['m_GameObject']))['m_Name'],
        'pose': stage_pose(source, b, stage)} for b in bones]
    if len({j['name'] for j in joints}) != len(joints):
        raise ValueError('Preview figure joint names are not unique')
    return {'root_pose': stage_pose(source, transform_of(source, figure), stage), 'character': character, 'joints': joints}


def recover_preview(source):
    script = preview_script(source)
    d = source.read(script)
    stage = source.resolve(script, d['camStuff'])
    if stage is None or stage.type.name not in ('Transform', 'RectTransform'):
        raise ValueError('Preview camStuff is missing')
    figure = source.resolve(script, d['outfit'])
    if figure is None or source.class_name(figure) != 'playermodeloutfitscript':
        raise ValueError('Preview figure is missing')
    if key(figure) == PLAYER_OUTFIT:
        raise ValueError('Preview figure is the player outfit')
    lights = []
    for record in source.hierarchy(stage, activate_root=True):
        for component in record['components']:
            if component.type.name != 'Light':
                continue
            if record['active'] and source.read(component)['m_Enabled']:
                lights.append(light_record(source, component, stage_pose(source, record['reader'], stage)))
    return {'camera': camera_record(source, source.resolve(script, d['cam']), lambda t: stage_pose(source, t, stage)), 'lights': lights,
        'figure': figure_record(source, figure, stage)}


def valid_pose(p):
    t, r, s = p['translation'], p['rotation'], p['scale']
    return (len(t) == 3 and len(r) == 4 and len(s) == 3 and all(math.isfinite(v) for v in (*t, *r, *s))
        and all(v != 0 for v in s) and abs(sum(v * v for v in r) - 1) < .002)


def validate_preview(preview, clothing, manifest):
    camera, figure = preview['camera'], preview['figure']
    if not valid_pose(camera['pose']) or not 0 < camera['fov'] < 180 or not 0 < camera['near'] < camera['far']:
        raise ValueError('Preview camera')
    for l in preview['lights']:
        if l['kind'] not in LIGHT_KINDS.values() or len(l['color']) != 4 or l['intensity'] < 0 or l['range'] <= 0 or not valid_pose(l['pose']):
            raise ValueError('Preview light')
    validate_figure(figure, clothing, manifest)
    if not frames_figure(camera, figure):
        raise ValueError('Preview camera does not frame the figure joints')


def frames_figure(camera, figure):
    """Each figure joint is in front of the camera and inside its vertical field of view."""
    t, (x, y, z, w) = camera['pose']['translation'], camera['pose']['rotation']
    half = math.tan(math.radians(camera['fov']) / 2)
    for joint in figure['joints']:
        offset = [joint['pose']['translation'][i] - t[i] for i in range(3)]
        local = transform_point(offset, (0., 0., 0.), (-x, -y, -z, w), (1., 1., 1.))
        # Glb cameras look along -z.
        depth = -local[2]
        if depth <= camera['near'] or abs(local[1]) > depth * half:
            return False
    return True


def validate_figure(figure, clothing, manifest):
    if not valid_pose(figure['root_pose']) or not 0 <= figure['character'] < len(clothing['characters']):
        raise ValueError('Preview figure')
    character = clothing['characters'][figure['character']]
    joints = {j for s in character['skins'] for j in s['joints']}
    names = {n['name'] for n in manifest['nodes'] if n['id'] in joints}
    given = [j['name'] for j in figure['joints']]
    if len(set(given)) != len(given) or set(given) != names or not all(valid_pose(j['pose']) for j in figure['joints']):
        raise ValueError('Preview figure joints')
