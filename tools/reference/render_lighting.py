"""Source startup lighting and native bridge equations for the selected build."""
import math
import struct

from .gltf import convert_point


NATIVE_DLL_SHA256 = '553c1e7560e1b42bd4638c53d0168dcd1b4b9eb5b9df324b0a44f14b3f18bc86'
NATIVE_PDB_SHA256 = 'e076c85f2efc0a42464c7180dd892d1cddbb4a98bc664c75670e4d08b3f3f2e0'

# Native kCubemapOrthoBases, RVA0x1a62fa0. Rows are right, up, back.
CUBE_BASES = (
    ((0,0,-1), (0,-1,0), (-1,0,0)),
    ((0,0,1), (0,-1,0), (1,0,0)),
    ((1,0,0), (0,0,1), (0,-1,0)),
    ((1,0,0), (0,0,-1), (0,1,0)),
    ((1,0,0), (0,-1,0), (0,0,-1)),
    ((-1,0,0), (0,-1,0), (0,0,1)),
)


def f32(value):
    return struct.unpack('<f', struct.pack('<f', value))[0]


def gamma_to_linear(value):
    """Scalar native Color.linear equation; round each source f32 operation."""
    value = f32(value)
    if not math.isfinite(value) or value < 0:
        raise ValueError('Invalid source light color')
    if value <= f32(.04045):
        return f32(value / f32(12.92))
    if value < 1:
        return f32(f32(f32(value + f32(.055)) / f32(1.055)) ** f32(2.4))
    return f32(value ** f32(2.2))


def light_radiance(color, intensity):
    if not math.isfinite(intensity) or intensity < 0:
        raise ValueError('Invalid source light intensity')
    # lightsUseLinearIntensity=false. SharedLightData::PrecalcLightColor.
    return [gamma_to_linear(f32(value * intensity)) for value in color[:3]]


def flat_ambient_sh(value):
    # AddAmbientLight writes coefficient0/9/18; packing adds no pi or other scale.
    return [[0.,0.,0.,gamma_to_linear(value)] for _ in range(3)] + [[0.] * 4 for _ in range(4)]


def startup_light_enabled(light, hierarchy_active, switch_active):
    active = hierarchy_active and switch_active is not False
    return bool(light['m_Enabled'] and active)


def startup_sample():
    """Controller schedule: one awake Update, then25source fixed steps."""
    dt, speed, phase = f32(.02), f32(.0003), f32(0.)
    for _ in range(25):
        phase = f32(phase + f32(dt * speed))
        while phase > 0:
            phase = f32(phase - 1)
    if phase != -.9998494982719421:
        raise ValueError('Source cloud phase differs from the audited native sample')
    # Exact installed Perlin function executed in the isolated research sample.
    perlin = .4651719629764557
    noise = f32(f32(perlin - .5) * 2)
    value = min(1., max(0., f32(f32((20.-50.) / (-9.-50.)) + noise)))
    density = f32(f32(1.4) + f32(f32(f32(.8) - f32(1.4)) * value))
    p = f32(f32(240.5 - 240.) / f32(1020. - 240.))
    return {'elapsed_level_seconds': .5, 'fixed_updates': 25, 'fixed_delta_seconds': dt,
            'initial_awake_updates': 1, 'cloud_phase': phase, 'cloud_perlin': perlin,
            'cloud_density': density, 'shader_time': [f32(.5*f32(.05)),.5,1.,1.5],
            'sun_euler_degrees': [f32(40. + f32(f32(140.-40.)*p)),60.,0.],
            'sun_intensity': 1.125, 'ambient_gamma': f32(.2),
            'temperature': 20., 'heat_haze_weight': 0.}

def recover_light_controls(source, nodes):
    """Apply selected startup switches; reject unproved random or nonzero states."""
    from .source import key, source_id
    enabled, active, evidence = {}, {}, []
    def material_assignment(owner, renderer_pointer, material_pointer):
        renderer = source.resolve(owner, renderer_pointer)
        if renderer is None:
            return
        material = source.resolve(owner, material_pointer)
        data = source.read(renderer)
        current = source.resolve(renderer, data['m_Materials'][0])
        if material is None or current is None:
            raise ValueError(f'{source_id(owner)} missing startup material at {source_id(renderer)} slot0')
        evidence.append({'controller': source_id(owner), 'renderer': source_id(renderer),
                         'material': source_id(material), 'previous': source_id(current), 'slot': 0})
    for node in nodes:
        if not node['active']:
            continue
        for reader in node['components']:
            if reader.type.name != 'MonoBehaviour':
                continue
            name = source.class_name(reader)
            if name not in {'headlightscript', 'flashlightscript'}:
                continue
            data = source.read(reader)
            if not data['m_Enabled']:
                continue
            if name == 'headlightscript':
                if data['currentState'] != 0 or data['switchToState'] != 0 or data['energy'] != 0:
                    raise ValueError(f'{source_id(reader)} unresolved startup headlight state')
                current = data['states'][0]
                if current['L']['m_PathID'] or current['Ls']:
                    raise ValueError(f'{source_id(reader)} startup headlight state0 contains a light')
                for state in data['states'][1:]:
                    for pointer in [state['L'], *state['Ls']]:
                        light = source.resolve(reader, pointer)
                        if light is not None:
                            enabled[key(light)] = False
                    material_assignment(reader, state['R'], data['MOFF'])
                    for pointer in state['Rs']:
                        material_assignment(reader, pointer, data['MOFF'])
            else:
                if data['randomStart'] or data['ON'] or data['started'] or data['SingleLight']['m_PathID']:
                    raise ValueError(f'{source_id(reader)} unresolved startup flashlight state')
                for lamp in data['Lamps']:
                    if lamp['nLights']:
                        raise ValueError(f'{source_id(reader)} unsupported inverse startup light')
                    for pointer in lamp['Lights']:
                        game_object = source.resolve(reader, pointer)
                        active[key(game_object)] = False
                    off = lamp['mOFF'] if lamp['mOFF']['m_PathID'] else data['mOFF']
                    for pointer in lamp['R']:
                        material_assignment(reader, pointer, off)
    return enabled, active, evidence


def prepare_startup_lighting(source, roots):
    """Resolve startup switch effects before material records and hashes exist."""
    from .source import key
    nodes = [node for root in roots for node in source.hierarchy(root, activate_root=True)]
    enabled, active, evidence = recover_light_controls(source, nodes)
    materials = {}
    for entry in evidence:
        renderer = (entry['renderer']['file'], entry['renderer']['path_id'])
        material = source.get(entry['material']['file'], entry['material']['path_id'])
        previous = materials.get(renderer)
        if previous is not None and key(previous) != key(material):
            raise ValueError(f'{renderer} conflicting startup material assignments')
        condition = getattr(source, 'condition_materials', {}).get(renderer)
        if condition is not None and key(condition) != key(material):
            raise ValueError(f'{renderer} unresolved light/condition initialization order')
        materials[renderer] = material
    source.startup_light_enabled = enabled
    source.startup_object_active = active
    source.startup_materials = materials
    source.startup_light_material_evidence = evidence


def probe_settings(identity, data):
    """Checked realtime ReflectionProbe values of one source probe."""
    from .source import vector
    expected={'m_Mode':1,'m_RefreshMode':1,'m_TimeSlicingMode':1,'m_Resolution':64,
        'm_HDR':True,'m_BoxProjection':False,'m_UseOcclusionCulling':False}
    for field,value in expected.items():
        if data[field]!=value:raise ValueError(f'{identity} unsupported {field}')
    if data['m_BakedTexture']['m_PathID'] or data['m_CustomBakedTexture']['m_PathID']:
        raise ValueError(f'{identity} unexpected authored probe texture')
    return {'enabled':bool(data['m_Enabled']),'bounds_size':list(vector(data['m_BoxSize'])),
        'bounds_offset':list(convert_point(vector(data['m_BoxOffset']))),
        'box_projection':False,'blend_distance':data['m_BlendDistance'],
        'intensity':data['m_IntensityMultiplier'],'importance':data['m_Importance'],
        'near':data['m_NearClip'],'far':data['m_FarClip'],'shadow_distance':data['m_ShadowDistance'],
        'mask':data['m_CullingMask']['m_Bits'],'resolution':64,'refresh':'individual_faces_every_frame',
        'decode':[1.,1.,0.,0.],'faces':[{'index':face,'basis':basis} for face,basis in enumerate(CUBE_BASES)],
        'mip_extents':[64>>mip for mip in range(7)]}


def recover_lights_and_probes(source, roots):
    """Resolve identities through live component links, never prose source IDs."""
    from .source import key, source_id
    from .collision import world_pose
    from .gltf import convert_rotation
    nodes = [node for root in roots for node in source.hierarchy(root, activate_root=True)]
    node_ids = {key(node['reader']): index+1 for index,node in enumerate(nodes)}
    records = {key(node['reader']):node for node in nodes}
    def binding(component):
        go = source.resolve(component,source.read(component)['m_GameObject'])
        transform = source.transform_for(go)
        return transform,node_ids.get(key(transform))
    lights=[]; probes=[]
    components=[component for node in nodes for component in node['components']]
    components.insert(0,source.get('level1',56812))
    seen=set()
    for reader in components:
        identity=key(reader)
        if identity in seen:continue
        seen.add(identity)
        if reader.type.name not in {'Light','ReflectionProbe'}:continue
        data=source.read(reader); transform,node=binding(reader)
        pose=world_pose(source,transform)
        if reader.type.name=='Light':
            is_sun=identity==('level1',56812)
            if not is_sun and node is None:raise ValueError(f'{identity} missing light node')
            kind={0:'spot',1:'directional',2:'point'}.get(data['m_Type'])
            if kind is None:raise ValueError(f'{identity} unsupported source light type')
            color=[data['m_Color'][c] for c in ('r','g','b','a')]
            intensity=data['m_Intensity']
            enabled=bool(data['m_Enabled'])
            if is_sun:
                sample=startup_sample();intensity=sample['sun_intensity'];color=[1.]*4
                # Unity Euler uses Z, then X, then Y. Here Z is exactly zero.
                x,y=[math.radians(v)*.5 for v in sample['sun_euler_degrees'][:2]]
                q=(math.sin(x)*math.cos(y),math.cos(x)*math.sin(y),-math.sin(x)*math.sin(y),math.cos(x)*math.cos(y))
                pose['rotation']=list(convert_rotation(q))
            else:
                enabled=source.startup_light_enabled.get(identity,enabled)
                hierarchy=records[key(transform)]['active']
                ancestor=transform
                while ancestor is not None:
                    go=source.resolve(ancestor,source.read(ancestor)['m_GameObject'])
                    hierarchy=hierarchy and source.startup_object_active.get(key(go),True)
                    ancestor=source.resolve(ancestor,source.read(ancestor)['m_Father'])
                enabled=bool(enabled and hierarchy)
            cookie=source.resolve(reader,data['m_Cookie'])
            shadows=data['m_Shadows']
            lights.append({'source':source_id(reader),'node':node,'world_pose':pose,'kind':kind,
                'color':color,'intensity':intensity,'radiance':light_radiance(color,intensity),
                'range':data['m_Range'],'spot_angles':[data['m_InnerSpotAngle'],data['m_SpotAngle']],
                'mask':data['m_CullingMask']['m_Bits'],'enabled':enabled,
                'cookie':source_id(cookie) if cookie else None,'cookie_size':data['m_CookieSize'],
                'shadows':{'mode':shadows['m_Type'],'strength':shadows['m_Strength'],
                    'depth_bias':shadows['m_Bias'],'normal_bias':shadows['m_NormalBias'],
                    'near':shadows['m_NearPlane']}})
        else:
            raise ValueError(f'{identity} reflection probes belong to the car prototypes')
    # The car prototype probe holds the shared capture settings. The runtime moves it to the last seated car.
    car_probes=[c for node in source.hierarchy(source.lighting_probe_prototype,activate_root=True)
        for c in node['components'] if c.type.name=='ReflectionProbe']
    if len(car_probes)!=1:raise ValueError('The car prototype requires one reflection probe')
    probes.append({'source':source_id(car_probes[0]),**probe_settings(key(car_probes[0]),source.read(car_probes[0]))})
    if any(light['enabled'] for light in lights[1:]):
        raise ValueError('The selected startup profile contains an unresolved enabled local light')
    return lights,probes


def recover_renderer(source, reader, mesh, usage="scene"):
    """Keep source renderer state separate from shared mesh geometry."""
    from .source import source_id, vector
    data = source.read(reader)
    if reader.type.name not in ('MeshRenderer','SkinnedMeshRenderer'):
        raise ValueError(f'{source_id(reader)} unsupported source renderer type')
    if data['m_ProbeAnchor']['m_PathID']:
        raise ValueError(f'{source_id(reader)} unsupported probe anchor')
    fields = {'cast_shadows':data['m_CastShadows'],
              'receive_shadows':bool(data['m_ReceiveShadows']),
              'reflection_probe_usage':data['m_ReflectionProbeUsage'],
              'motion_vectors':data['m_MotionVectors'],
              'sorting_layer':data['m_SortingLayer'], 'sorting_order':data['m_SortingOrder']}
    if data['m_SortingLayerID'] != 0 or fields['sorting_layer'] != 0 or fields['sorting_order'] != 0:
        raise ValueError(f'{source_id(reader)} unsupported source sorting state')
    if fields['cast_shadows'] not in (0,1,2,3) or fields['reflection_probe_usage'] not in (0,1):
        raise ValueError(f'{source_id(reader)} unsupported source renderer flags')
    if data['m_ReceiveShadows'] not in (0,1):
        raise ValueError(f'{source_id(reader)} unsupported shadow receiver state')
    if fields['motion_vectors'] != 1:
        raise ValueError(f'{source_id(reader)} unsupported source motion mode')
    bounds = data['m_AABB'] if reader.type.name == 'SkinnedMeshRenderer' else source.read(mesh)['m_LocalAABB']
    center = list(convert_point(vector(bounds['m_Center'])))
    extents = list(vector(bounds['m_Extent']))
    if not all(math.isfinite(v) for v in center+extents) or min(extents) < 0:
        raise ValueError(f'{source_id(reader)} invalid local renderer bounds')
    return {'source':source_id(reader),'bounds_center':center,'bounds_extents':extents,**fields}

def recover_lighting(source, roots, output):
    """Export the fixed startup lighting profile and its runtime dependencies."""
    from .render_lighting_resources import recover_sky_resources
    from .postprocessing import recover_camera_stack
    project=source.read(source.get('globalgamemanagers',1))
    graphics=source.read(source.get('globalgamemanagers',7))
    quality=source.read(source.get('globalgamemanagers',12))['m_QualitySettings'][0]
    settings=source.read(source.get('level1',56810))
    controls=source.read(source.get('level1',64920))
    if project['m_ActiveColorSpace']!=1 or project['useHDRDisplay'] or not project['forceSRGBBlit']:
        raise ValueError('Unrecovered source output color-space profile')
    if graphics['m_LightsUseLinearIntensity'] or graphics['m_LightsUseColorTemperature']:
        raise ValueError('Unrecovered source light conversion profile')
    if settings['m_FogMode']!=3 or not settings['m_Fog'] or settings['m_ReflectionBounces']!=1:
        raise ValueError('Unrecovered source fog or reflection profile')
    if quality['shadowCascades']!=4 or quality['shadowProjection']!=1 or quality['shadowResolution']!=2:
        raise ValueError('Unrecovered source directional shadow profile')
    if any(controls['CurrentColor'][c]!=0 for c in ('r','g','b','a')) or controls['sandStorms']:
        raise ValueError('Source startup sample must precede biome refresh and storms')
    if controls['minReflection']!=0 or controls['maxReflection']!=0:
        raise ValueError('Unrecovered startup environment reflection intensity')
    resources=recover_sky_resources(source,output)
    cookie=resources.pop('cookie')
    lights,probes=recover_lights_and_probes(source,roots)
    if lights[0]['cookie']!=cookie:
        raise ValueError('Source sun cookie differs from enabled cloud-shadow control')
    gamma=[controls['dayFogColors'][2][c] for c in ('r','g','b','a')]
    sample=startup_sample()
    return dict(resources,project_color_space='linear',output_transform='neutral_lut_srgb_dither',
                light_conversion='legacy_gamma_before_linear',msaa_samples=8,
                ambient_sh=flat_ambient_sh(controls['dayAmbient']),global_reflection_intensity=0.,
                reflection_bounces=1,sample=sample,lights=lights,probes=probes,
                cameras=recover_camera_stack(source),
                fog={'mode':'exp_squared','color_gamma':gamma,
                     'color_linear':[gamma_to_linear(v) for v in gamma[:3]]+[gamma[3]],
                     'density':controls['fogDensityDays'][2]},
                shadows={'cascades':quality['shadowCascades'],'projection':'stable_fit',
                         'resolution':quality['shadowResolution'],'distance':quality['shadowDistance'],
                         'near_offset':quality['shadowNearPlaneOffset'],
                         'split_fractions':[quality['shadowCascade4Split'][c] for c in ('x','y','z')]})
