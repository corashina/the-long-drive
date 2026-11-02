"""Checked scene lighting and reusable desert road lamp records."""
import hashlib
import json
import math
import struct
from pathlib import Path
from .source import Source, key, source_id, vector
from .gltf import converted_transform, convert_point
from .collision import world_pose

HOUSE=(67484,68886,72019,75508,76065)
IDENTITY={'translation':[0.,0.,0.],'rotation':[0.,0.,0.,1.],'scale':[1.,1.,1.]}

def rgba(value):return [value[c] for c in ('r','g','b','a')]

def resource(reader, output, suffix, payload):
    name=f'lighting-{reader.assets_file.name}-{reader.path_id}.{suffix}'
    path=output/name;path.write_bytes(payload)
    return {'source':source_id(reader),'path':f'reference/{name}','sha256':hashlib.sha256(payload).hexdigest()}

def texture_chunks(reader, output, payload):
    """Bound browser reader and upload ownership without changing decoded bytes."""
    width,height,count=struct.unpack_from('<III',payload,4)
    chunks=[];offset=16
    for mip in range(count):
        w=max(1,width>>mip);h=max(1,height>>mip)
        rows=max(1,(1024*1024)//(w*4))
        for row in range(0,h,rows):
            size=min(rows,h-row)
            part=b'SMIP'+struct.pack('<III',w,size,1)+payload[offset:offset+w*size*4]
            chunks.append({'data':resource(reader,output,f'mip-{mip}-row-{row}.smip',part),
                'mip':mip,'row':row,'width':w,'rows':size})
            offset+=w*size*4
    if offset!=len(payload):raise ValueError('Decoded texture payload length')
    return chunks


BATCH_BYTES = 1024 * 1024
BATCH_ENTRIES = 32
BATCH_DESCRIPTOR_BYTES = 64 * 1024


def _texture_regions(texture, output):
    width, height, count = (texture[k] for k in ('width', 'height', 'mip_count'))
    if min(width, height, count) <= 0 or count > max(width, height).bit_length():
        raise ValueError('Texture batch dimensions')
    index = 0
    texture_offset = 0
    for mip in range(count):
        w, h = max(1, width >> mip), max(1, height >> mip)
        row = 0
        while row < h:
            if index >= len(texture['chunks']):
                raise ValueError('Missing texture batch region')
            chunk = texture['chunks'][index]
            rows = chunk['rows']
            size = w * rows * 4
            if (chunk['mip'], chunk['row'], chunk['width']) != (mip, row, w) or not 0 < rows <= h-row or size > BATCH_BYTES or chunk['data']['source'] != texture['source']:
                raise ValueError('Texture batch region coverage')
            name = chunk['data']['path']
            if not name.startswith('reference/') or Path(name).name != name.removeprefix('reference/'):
                raise ValueError('Texture batch input path')
            path = output / Path(name).name
            data = path.read_bytes()
            if len(data) != size + 16 or data[:16] != b'SMIP' + struct.pack('<III', w, rows, 1) or hashlib.sha256(data).hexdigest() != chunk['data']['sha256']:
                raise ValueError(f'Texture batch input receipt/header: {texture["source"]} {name}')
            yield dict(input=dict(file=name, bytes=len(data), sha256=chunk['data']['sha256'], image_size=None),
                       mip=mip, row=row, width=w, rows=rows, texture_offset=texture_offset, decoded_offset=0, decoded_bytes=size)
            texture_offset += size
            row += rows
            index += 1
    if index != len(texture['chunks']):
        raise ValueError('Extra texture batch regions')


def _batch_descriptors(texture, output):
    regions = []
    decoded = 0
    for region in _texture_regions(texture, output):
        if regions and (decoded + region['decoded_bytes'] > BATCH_BYTES or len(regions) == BATCH_ENTRIES):
            yield dict(source=texture['source'], width=texture['width'], height=texture['height'], mip_count=texture['mip_count'], decoded_bytes=decoded, regions=regions)
            regions = []
            decoded = 0
        region['decoded_offset'] = decoded
        regions.append(region)
        decoded += region['decoded_bytes']
    if regions:
        yield dict(source=texture['source'], width=texture['width'], height=texture['height'], mip_count=texture['mip_count'], decoded_bytes=decoded, regions=regions)


def _descriptor_bytes(descriptor):
    return json.dumps(descriptor, sort_keys=True, separators=(',', ':'), allow_nan=False).encode()


def export_texture_batches(texture, output):
    from .runtime_package import file_receipt
    # Validate all chunks before publishing any descriptor.
    descriptors = list(_batch_descriptors(texture, output))
    references = []
    for descriptor in descriptors:
        data = _descriptor_bytes(descriptor)
        if len(data) > BATCH_DESCRIPTOR_BYTES:
            raise ValueError('Texture batch descriptor bound')
        name = hashlib.sha256(data).hexdigest() + '.stexbatch'
        (output / name).write_bytes(data)
        references.append(dict(descriptor=file_receipt(output / name, 'reference/' + name),
                               decoded_bytes=descriptor['decoded_bytes'],
                               largest_raw_input_bytes=max(r['input']['bytes'] for r in descriptor['regions']),
                               region_count=len(descriptor['regions'])))
    texture['batches'] = references


def validate_texture_batches(texture, output):
    from .runtime_package import file_receipt
    expected = list(_batch_descriptors(texture, output))
    if len(expected) != len(texture['batches']):
        raise ValueError('Texture batch count')
    for descriptor, reference in zip(expected, texture['batches']):
        receipt = reference['descriptor']
        path = output / Path(receipt['file']).name
        data = _descriptor_bytes(descriptor)
        if receipt['file'] != 'reference/' + hashlib.sha256(data).hexdigest() + '.stexbatch' or file_receipt(path, receipt['file']) != receipt or path.read_bytes() != data:
            raise ValueError('Texture batch descriptor receipt/coverage')
        if reference['decoded_bytes'] != descriptor['decoded_bytes'] or reference['region_count'] != len(descriptor['regions']) or reference['largest_raw_input_bytes'] != max(r['input']['bytes'] for r in descriptor['regions']):
            raise ValueError('Texture batch totals')


def export_manifest_texture_batches(manifest, output):
    for group in (manifest.get('lighting', {}).get('textures', []), manifest.get('world', {}).get('material_textures', []), manifest.get('world', {}).get('terrain', {}).get('textures', [])):
        for texture in group:
            export_texture_batches(texture, output)
            validate_texture_batches(texture, output)


def texture(source, reader, output):
    if reader is None or reader.type.name!='Texture2D':raise ValueError(f'Required lighting Texture2D: {key(reader) if reader else None}')
    identity=key(reader)
    if identity not in source.lighting_textures:
        from PIL import Image
        obj=source.texture_object(reader);name=f'lighting-{reader.assets_file.name}-{reader.path_id}.png'
        path=output/name;obj.image.transpose(Image.Transpose.FLIP_TOP_BOTTOM).save(path)
        from .mips import decoded_mips
        payload=decoded_mips(obj)
        mip_data=resource(reader,output,"smip",payload)
        s=obj.m_TextureSettings
        source.lighting_textures[identity]={'id':len(source.lighting_textures)+1,'source':source_id(reader),'path':f'reference/{name}',
            'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'width':obj.m_Width,'height':obj.m_Height,
            'source_format':obj.m_TextureFormat,'sample_channel':'alpha' if obj.m_TextureFormat==1 else 'rgba','color_space':obj.m_ColorSpace,'filter_mode':s.m_FilterMode,'wrap_u':s.m_WrapU,'wrap_v':s.m_WrapV,
            'chunks':texture_chunks(reader,output,payload),'mip_data':mip_data,'source_data':resource(reader,output,'texture.bin',obj.get_image_data()),'mip_count':obj.m_MipCount,'aniso':s.m_Aniso,'mip_bias':s.m_MipBias}
    return source.lighting_textures[identity]['id']

def sky_material(source, reader, output):
    data=source.read(reader);shader=source.resolve(reader,data['m_Shader'])
    if shader is None:raise ValueError(f'{key(reader)} m_Shader missing')
    shader_fields=source.read(shader)
    suffix,payload=source.shader_resource(shader)
    shader_data=resource(shader,output,suffix,payload)
    shader_data['compiled_data']=resource(shader,output,'shader.bin',source.raw_data(shader))
    shader_data['name']=shader_fields['m_Name'] or shader_fields['m_ParsedForm']['m_Name']
    props=data['m_SavedProperties']
    floats=dict(props['m_Floats'])
    colors={n:rgba(c) for n,c in props['m_Colors']}
    # Disabled particle fades serialize infinity in their parameter vectors.
    for toggle,parameter in [('_CameraFadingEnabled','_CameraFadeParams'),('_SoftParticlesEnabled','_SoftParticleFadeParams')]:
        if floats.get(toggle)==0:colors.pop(parameter,None)
    images=[]
    for name,env in props['m_TexEnvs']:
        image=source.resolve(reader,env['m_Texture'])
        if image:
            images.append({'property':name,'texture':texture(source,image,output),'scale':[env['m_Scale'][c] for c in ('x','y')],'offset':[env['m_Offset'][c] for c in ('x','y')]})
    return {'source':source_id(reader),'shader':shader_data,'source_data':resource(reader,output,'bin',source.raw_data(reader)),'floats':floats,
        'colors':colors,'textures':images}

def gradient(data):
    return {'mode':data['m_Mode'],'colors':[{'time':data[f'ctime{i}']/65535.,'value':rgba(data[f'key{i}'])} for i in range(data['m_NumColorKeys'])],
        'alphas':[{'time':data[f'atime{i}']/65535.,'value':data[f'key{i}']['a']} for i in range(data['m_NumAlphaKeys'])]}

def recover_fog_evidence(source):
    reader=source.get('level0',16865);settings=source.read(reader)
    lo=settings['DDSmin'];hi=settings['DDSmax']
    source.lighting_fog_evidence={'source':source_id(reader),
        'minimum':{'draw_distance':lo['drawDistance'],'fog_modifier':lo['fogModifier']},
        'maximum':{'draw_distance':hi['drawDistance'],'fog_modifier':hi['fogModifier']},
        'rule':'settingsscript.DrawDistanceChanged uses InverseLerp, then Lerp; each operation uses float32. The fog modifier multiplies the fog density.'}


def recover_grading_temperatures(source: Source) -> list:
    """postprocesscontrol.TemperatureValues on the main camera sets the main-view ColorGrading temperature."""
    player=source.get('level1',80125)
    camera=source.resolve(player,source.read(player)['Cam'])
    go=source.resolve(camera,source.read(camera)['m_GameObject'])
    controls=[]
    for pointer in source.read(go)['m_Component']:
        component=source.resolve(go,pointer['component'])
        if component.type.name=='MonoBehaviour' and source.class_name(component)=='postprocesscontrol':controls.append(component)
    if len(controls)!=1:raise ValueError('Main camera requires one postprocesscontrol')
    values=source.read(controls[0]).get('TemperatureValues')
    if not isinstance(values,list) or len(values)!=3 or not all(isinstance(v,(int,float)) and math.isfinite(v) for v in values):
        raise ValueError(f'{key(controls[0])} TemperatureValues requires three finite values')
    return values


def recover_cycle(source: Source, reader, output: Path) -> dict:
    output.mkdir(parents=True,exist_ok=True)
    if not hasattr(source,'lighting_textures'):source.lighting_textures={}
    f=source.read(reader);factor=60 if f['minute'] else 1
    result={'source':source_id(reader),'day_seconds':f['daytime']*factor,'night_seconds':f['nighttime']*factor,
        'dawn_seconds':f['reggelTime']*factor,'dusk_seconds':f['esteTime']*factor,'startup_offset_seconds':f['daytime']*factor*.2,
        'color':gradient(f['color']),'atmosphere_thickness':gradient(f['atmosphereThickness'])}
    fields={'dayIntensity':'day_intensity','nightIntensity':'night_intensity','noonIntensity':'noon_intensity','dayAmbient':'day_ambient','nightAmbient':'night_ambient',
        'minReflection':'min_reflection','maxReflection':'max_reflection','minExposure':'min_exposure','maxExposure':'max_exposure',
        'dayAngleMin':'day_angle_min','dayAngleMax':'day_angle_max','midnightAngleR':'midnight_angle_dawn','midnightAngleE':'midnight_angle_dusk',
        'minMoonAngle':'min_moon_angle','maxMoonAngle':'max_moon_angle','colorDelay':'color_delay','starSpeed':'star_speed',
        'CloudPerlinSpread':'cloud_perlin_spread','CloudPerlinSpeed':'cloud_perlin_speed','sunFlareLerpMStart':'flare_lerp_start','sunFlareLerpMEnd':'flare_lerp_end'}
    result.update({v:f[k] for k,v in fields.items()});result['day_yaw_degrees']=60.;result['biome_refresh_seconds']=1.;recover_fog_evidence(source)
    # The approved desert-only world uses TerrainGenerator's forced desert RGB selector.
    result['desert_color']=[1.,0.,0.,1.]
    for k,v in {'skyColors':'sky_colors','daySkyGroundColors':'day_ground_colors','nightSkyGroundColors':'night_ground_colors',
            'dayFogColors':'day_fog_colors','nightFogColors':'night_fog_colors'}.items():result[v]=[rgba(c) for c in f[k]]
    for k,v in {'fogDensityDays':'day_fog_densities','fogDensityNights':'night_fog_densities','sunFlareReggel':'flare_dawn','sunFlareDay':'flare_day','sunFlareEsteledik':'flare_dusk','sunFlareNight':'flare_night'}.items():result[v]=f[k]
    for k,v in {'dayColor':'day_color','nightColor':'night_color','dawnColor':'dawn_color','duskColor':'dusk_color','starColorDay':'star_color_day','starColorNight':'star_color_night'}.items():result[v]=rgba(f[k])
    for source_field,field in [('tempDays','temp_days'),('tempNights','temp_nights'),('tempAddNoons','temp_add_noons')]:
        values=f[source_field]
        if len(values)!=3 or not all(math.isfinite(v) for v in values):raise ValueError(f'{key(reader)} {source_field} requires three finite values')
        result[field]=values
    result['temperature_values']=recover_grading_temperatures(source)
    result['clouds']=[]
    for layer in ('Low','High'):
        cloud_node=source.get('level1',27215 if layer=='Low' else 27216)
        result['clouds'].append({'node':getattr(source,'lighting_nodes',{}).get(key(cloud_node)),'asset':getattr(source,'lighting_assets',{}).get(('level1',27217)),'material':sky_material(source,source.resolve(reader,f['materialCloud'+layer]),output),
            'day_color':rgba(f['Cloud'+layer+'DayColor']),'transition_color':rgba(f['Cloud'+layer+'TransitionColor']),'night_color':rgba(f['Cloud'+layer+'NightColor']),
            'min_speed':f['Cloud'+layer+'MinSpeed'],'mid_speed':f['Cloud'+layer+'MidSpeed'],'max_speed':f['Cloud'+layer+'MaxSpeed'],
            'min_density':f['Cloud'+layer+'DensityMin'],'max_density':f['Cloud'+layer+'DensityMax']})
    result['star_material']=sky_material(source,source.resolve(reader,f['starMaterial']),output)
    result['sky_resources']=[]
    result['sun_flares']=[]
    for field in ('stars','stars2','starTransform','cloudTransform','sunFlare',*['sunFlareFlare']):
        values=f[field] if isinstance(f[field],list) else [f[field]]
        for pointer in values:
            linked=source.resolve(reader,pointer)
            if linked is None:raise ValueError(f'{key(reader)} {field} missing')
            data=source.read(linked)
            result['sky_resources'].append(resource(linked,output,'bin',source.raw_data(linked)))
            if linked.type.name=='LensFlare':
                flare=source.resolve(linked,data['m_Flare'])
                if flare is None or flare.type.name!='Flare':raise ValueError(f'{key(linked)} m_Flare missing')
                flare_data=source.read(flare)
                atlas=source.resolve(flare,flare_data['m_FlareTexture'])
                result['sun_flares'].append({'source':source_id(linked),'component':result['sky_resources'][-1],
                    'flare':resource(flare,output,'flare.json',json.dumps(flare_data,allow_nan=False).encode()),
                    'atlas':texture(source,atlas,output),
                    'color':rgba(data['m_Color']),'fade_speed':data['m_FadeSpeed'],'ignore_layers':data['m_IgnoreLayers']['m_Bits'],
                    'directional':bool(data['m_Directional']),'pose':converted_transform(source.read(source.transform_for(source.resolve(linked,data['m_GameObject'])))),
                    'use_fog':bool(flare_data['m_UseFog']),'texture_layout':flare_data['m_TextureLayout'],
                    'elements':[{'image':e['m_ImageIndex'],'position':e['m_Position'],'size':e['m_Size'],'color':rgba(e['m_Color']),
                        'use_light_color':bool(e['m_UseLightColor']),'rotate':bool(e['m_Rotate']),'zoom':bool(e['m_Zoom']),'fade':bool(e['m_Fade'])} for e in flare_data['m_Elements']]})
            # Particle and flare renderer resources remain explicit, never inferred.
            if linked.type.name in ('ParticleSystem','GameObject'):
                transform=source.transform_for(linked if linked.type.name=='GameObject' else source.resolve(linked,data['m_GameObject']))
                for node in source.hierarchy(transform):
                    for component in node['components']:
                        if component.type.name=='ParticleSystemRenderer':
                            for p in source.read(component)['m_Materials']:
                                m=source.resolve(component,p)
                                if m:
                                    source.read(m)
                                    result['sky_resources'].append(resource(m,output,'bin',source.raw_data(m)))
    result['sky_geometry']={'asset':getattr(source,'lighting_assets',{}).get(('level1',27217)),'root':getattr(source,'lighting_nodes',{}).get(('level1',27217)),'pose':converted_transform(source.read(source.get('level1',27217)))}
    result['stars']=[]
    for field in ('stars','stars2'):
        particle=source.resolve(reader,f[field]);pd=source.read(particle);initial=pd['InitialModule'];shape=pd['ShapeModule'];emission=pd['EmissionModule']
        t=source.transform_for(source.resolve(particle,pd['m_GameObject']))
        renderer=next(c for c in source.components(t)[1] if c.type.name=='ParticleSystemRenderer');rd=source.read(renderer)
        for name in ('startLifetime','startSpeed','startSize','startRotation'):
            if initial[name]['minMaxState'] not in (0,3):raise ValueError(f'{key(particle)} {name} unsupported curve')
        result['stars'].append({'source':source_id(particle),'pose':converted_transform(source.read(t)),
            'max_particles':initial['maxNumParticles'],'radius':shape['radius']['value'],'shape_kind':shape['type'],
            'shape_scale':list(vector(shape['m_Scale'])),'radius_thickness':shape['radiusThickness'],
            'size_min':initial['startSize']['minScalar'] if initial['startSize']['minMaxState']==3 else initial['startSize']['scalar'],
            'size_max':initial['startSize']['scalar'],'speed':initial['startSpeed']['scalar'],
            'infinite_lifetime':initial['startLifetime']['scalar']==float('inf'),
            'lifetime_seconds':None if initial['startLifetime']['scalar']==float('inf') else initial['startLifetime']['scalar'],
            'start_color':rgba(initial['startColor']['maxColor']),'emission_rate':emission['rateOverTime']['scalar'],
            'burst_counts':[b['countCurve']['scalar'] for b in emission['m_Bursts']],
            'auto_random_seed':pd['autoRandomSeed'],'random_seed':pd['randomSeed'],'looping':pd['looping'],
            'duration_seconds':pd['lengthInSec'],'simulation_space':pd['moveWithTransform'],'render_mode':rd['m_RenderMode'],
            'alignment':rd['m_RenderAlignment'],'max_screen_size':rd['m_MaxParticleSize']})
    result['cloud_shadow_cookie']=texture(source,source.resolve(reader,f['cloudShadowCookie']),output)
    return result

def road_source(source):
    controller=source.get('sharedassets1.assets',144809)
    root=source.transform_for(source.resolve(controller,source.read(controller)['m_GameObject']))
    return controller,root

def recover_road_lamp(source: Source, output: Path) -> dict:
    controller,root=road_source(source)
    road=source.get('sharedassets1.assets',153827);f=source.read(road)
    generator=source.get('level1',64771);g=source.read(generator)
    prefab=source.resolve(generator,g['prefabs'][0])
    if key(prefab)!=('sharedassets1.assets',30934) or key(source.resolve(road,f['koz']))!=key(controller):raise ValueError('Desert road prefab controller mismatch')
    pose=world_pose(source,root)
    main=source.read(source.get('level1',64919))
    return {'asset':source.lighting_assets[key(root)],'source':source_id(controller),'road_source':source_id(road),'generator_source':source_id(generator),
        'spacing_meters':g['roadSegmentLength'],'road_pose':pose,'rotation_node':source.lighting_nodes[key(source.resolve(road,f['randomPoleRotTransform']))],
        'rotation_chances':[f['randomPoleRotChance1'],f['randomPoleRotChance2']],
        'rotation_angles':[[f[f'randomPoleRotAngle{a}{i}'] for a in 'XYZ'] for i in (1,2)],'controller':source.lighting_control_ids[key(controller)],
        'lights':[source.lighting_light_ids[key(source.resolve(controller,source.read(controller)['LLight']))]],
        'global_enabled':bool(main['kozlampakOn']),'startup_on':bool(main['kozlampakSwitchedOn']),'electronics_threshold':.2}

REAR_SIGNALS=(('H','tail'),('F','brake'),('R','reverse'))
def vehicle_states(f, kind, state, intensity):
    """headlightscript or rearlights states. `state` builds one state; `intensity` reads the first light."""
    if kind=='rear':
        return [state(signal,intensity(f.get(p+'Light',[])),f.get(p+'Light',[]),f[p+'Lamp'],f[p+'LampOn'],f[p+'LampOff'],dont_overwrite=f[p+'DontOverwrite'])
            for p,signal in REAR_SIGNALS]
    names=('off','low','high') if kind=='headlight' else ('off','cabin')
    return [state(names[i],s['intensity'],[s['L'],*s['Ls']],[s['R'],*s['Rs']],s['M'],f['MOFF']) for i,s in enumerate(f['states'])]

def vehicle_light_links(source, scripts, car):
    """inputconnect cabin routes by target and the InteractSwitchLights usables of one car."""
    routes={key(source.resolve(r,p)):r for r in scripts if source.class_name(r)=='inputconnect' for p in source.read(r)['headLightState1_If1']}
    switches=[c for c in scripts if source.class_name(c)=='usablescript'
        and any(call['m_Target']['m_PathID']==car.path_id and call['m_MethodName']=='InteractSwitchLights'
            for call in source.read(c)['turnedEvent']['m_PersistentCalls']['m_Calls'])]
    return routes,switches

def recover_controllers(source: Source, roots: list, node_ids: dict, material_ids: dict) -> list[dict]:
    def node(reader):
        t=reader if reader.type.name=='Transform' else source.transform_for(reader if reader.type.name=='GameObject' else source.resolve(reader,source.read(reader)['m_GameObject']))
        if key(t) not in node_ids:raise ValueError(f'{key(reader)} lighting node outside selected asset')
        return node_ids[key(t)]
    def linked(owner,p):return source.resolve(owner,p) if p['m_PathID'] else None
    def material(owner,p):
        r=linked(owner,p)
        return material_ids[(source.lighting_assets_for_node[node(owner)],key(r))] if r else None
    def lights(owner,pointers):
        result=[]
        for p in pointers:
            r=linked(owner,p)
            if not r:continue
            candidates=[r] if r.type.name=='Light' else [c for n in source.hierarchy(source.transform_for(r)) for c in n['components'] if c.type.name=='Light']
            for c in candidates:
                result.append(source.lighting_light_ids[key(c)])
        return result
    def nodes(owner,pointers):return [node(r) for p in pointers if (r:=linked(owner,p))]
    def state(owner,kind,intensity,ls,rs,on,off,broken=False,working=(),not_working=(),dont_overwrite=False):
        return {'signal':kind,'lights':lights(owner,ls),'intensity':intensity,'renderers':nodes(owner,rs),
            'on_material':material(owner,on),'off_material':material(owner,off),'broken':bool(broken),
            'working_nodes':nodes(owner,working),'broken_nodes':nodes(owner,not_working),'dont_overwrite':bool(dont_overwrite)}
    records=[]
    for root in roots:
        records.extend(source.hierarchy(root,activate_root=True))
    controls=[]
    for record in records:
        for reader in record['components']:
            if key(reader) not in source.lighting_control_ids:continue
            f=source.read(reader);cls=source.class_name(reader)
            if cls in ('headlightscript','rearlights'):raise ValueError(f'{key(reader)} car lights belong to the car prototypes')
            kind={'flashlightscript':'switch','kozlampa':'road'}[cls]
            elec=linked(reader,f.get('electronics',{'m_PathID':0}))
            e=source.read(elec) if elec else None
            if e and e['jamValue']!=0:raise ValueError(f'{key(elec)} nonzero startup jam requires source rule')
            c={'id':source.lighting_control_ids[key(reader)],'source':source_id(reader),'kind':kind,'node':node(reader),
                'aim_colliders':[],'animated_poses':[],'startup_on':bool(f.get('ON',False)),
                'current_state':f.get('currentState',0),'requested_state':f.get('switchToState',0),'energy':f.get('energy',1.),'energy_move':f.get('energyMove',0.),
                'enabled':bool(f['m_Enabled']),'electronics_source':source_id(elec) if elec else None,'electronics_output':e['maxOutput'] if e else 1.,
                'electronics_threshold':.2,'reverse_switch':bool(f.get('reverseSwitch',False)),'states':[],'usable_source':None,'usable_event':None}
            if kind=='switch':
                if f['randomStart'] or f['SingleLight']['m_PathID']:raise ValueError(f'{key(reader)} unsupported selected switch startup')
                for lamp in f['Lamps']:
                    lamp_lights=lights(reader,lamp['Lights'])
                    intensity=source.lighting_lights[lamp_lights[0]-1]['serialized_intensity'] if lamp_lights else 0.
                    c['states'].append(state(reader,'switch',intensity,lamp['Lights'],lamp['R'],lamp['mON'] if lamp['mON']['m_PathID'] else f['mON'],lamp['mOFF'] if lamp['mOFF']['m_PathID'] else f['mOFF'],lamp['broken'],lamp['Working'],lamp['nWorking']))
                for n in source.hierarchy(record['reader']):
                    c['aim_colliders'].extend(source.lighting_collider_ids[key(col)] for col in n['components'] if key(col) in source.lighting_collider_ids)
                if f['GButton']['m_PathID']:
                    parents=[source.read(linked(reader,f[field]))['m_Father'] for field in ('GButton','GButtonOff','GButtonOn')]
                    if parents[1:]!=parents[:-1]:raise ValueError(f'{key(reader)} switch button parent frame')
                    c['animated_poses'].append({'node':node(linked(reader,f['GButton'])),'states':[converted_transform(source.read(linked(reader,f['GButtonOff']))),converted_transform(source.read(linked(reader,f['GButtonOn'])))],'rule':'current_pose_lerp','speed':25.})
            if kind=='road':
                c['states'].append(state(reader,'road',e['maxOutput'],[f['LLight']],[],{'m_PathID':0},{'m_PathID':0}))
            controls.append(c)
    return controls

def recover_lighting(source: Source, roots: list, output: Path) -> dict:
    output.mkdir(parents=True,exist_ok=True)
    source.lighting_textures={};source.lighting_nodes={};source.lighting_assets={};source.lighting_assets_for_node={};source.lighting_collider_ids={}
    controller,pole=road_source(source);clouds=source.get('level1',27217);all_roots=[*roots,pole,clouds];records=[]
    for asset,root in enumerate(all_roots,1):
        asset=getattr(source,'lighting_root_assets',{}).get(key(root),asset)
        source.lighting_assets[key(root)]=asset
        for r in source.hierarchy(root,activate_root=True):
            records.append(r);i=len(records);source.lighting_nodes[key(r['reader'])]=i;source.lighting_assets_for_node[i]=asset
            for c in r['components']:
                if c.type.name in ('BoxCollider','SphereCollider','CapsuleCollider','MeshCollider'):source.lighting_collider_ids[key(c)]=len(source.lighting_collider_ids)+1
    readers=[c for r in records for c in r['components'] if c.type.name=='MonoBehaviour' and source.class_name(c) in ('flashlightscript','headlightscript','rearlights','kozlampa')]
    source.lighting_control_ids={key(c):i for i,c in enumerate(readers,1)}
    source.lighting_extra_materials={};source.lighting_startup_materials={}
    # Explicit state materials precede regular mesh materials, keeping their indices stable.
    material_ids={}
    for r in readers:
        asset=source.lighting_assets_for_node[source.lighting_nodes[key(source.transform_for(source.resolve(r,source.read(r)['m_GameObject'])))]]
        def walk(value):
            if isinstance(value,dict):
                if 'm_FileID' in value and value['m_PathID']:
                    linked=source.resolve(r,value)
                    if linked.type.name=='Material':
                        group=source.lighting_extra_materials.setdefault(asset,[])
                        if key(linked) not in [key(m) for m in group]:group.append(linked)
                        material_ids[asset,key(linked)]={'asset':asset,'gltf_material':next(i for i,m in enumerate(group) if key(m)==key(linked)),'source':source_id(linked),'condition_key':None}
                else:
                    for child in value.values():walk(child)
            elif isinstance(value,list):
                for child in value:walk(child)
        walk(source.read(r))
    lights=[source.get('level1',56812),*[c for r in records for c in r['components'] if c.type.name=='Light']]
    source.lighting_light_ids={key(r):i for i,r in enumerate(lights,1)};source.lighting_lights=[]
    for r in lights:
        f=source.read(r);t=source.transform_for(source.resolve(r,f['m_GameObject']));shadow=f['m_Shadows']
        source.lighting_lights.append({'id':source.lighting_light_ids[key(r)],'source':source_id(r),'node':source.lighting_nodes.get(key(t)),
            'pose':converted_transform(source.read(t)),'kind':{0:'spot',1:'directional',2:'point',3:'area'}[f['m_Type']],
            'color':rgba(f['m_Color']),'serialized_intensity':f['m_Intensity'],'startup_intensity':f['m_Intensity'],
            'range_meters':f['m_Range'],'full_cone_degrees':f['m_SpotAngle'],'inner_cone_degrees':f['m_InnerSpotAngle'],
            'enabled':bool(f['m_Enabled']),'culling_mask':f['m_CullingMask']['m_Bits'],
            'shadows':{'kind':shadow['m_Type'],'strength':shadow['m_Strength'],'bias':shadow['m_Bias'],'normal_bias':shadow['m_NormalBias'],'near_plane':shadow['m_NearPlane'],'resolution':shadow['m_Resolution'],'custom_resolution':shadow['m_CustomResolution']},
            'cookie':texture(source,source.resolve(r,f['m_Cookie']),output) if f['m_Cookie']['m_PathID'] else None,'cookie_size':f['m_CookieSize']})
    controls=recover_controllers(source,all_roots,source.lighting_nodes,material_ids)
    for c in controls:
        for i,state in enumerate(c['states']):
            for lid in state['lights']:
                l=source.lighting_lights[lid-1];l['enabled']=False if c['kind'] in ('switch','rear','road') else i==c['current_state']
                l['startup_intensity']=state['intensity']*c['energy']*c['electronics_output'] if c['kind'] in ('headlight','cabin') else c['electronics_output'] if c['kind']=='road' else l['serialized_intensity']
            for nid in state['renderers']:
                # Startup is off for every selected controller.
                if state['off_material']:
                    for rec in records:
                        if source.lighting_nodes[key(rec['reader'])]==nid:
                            for comp in rec['components']:
                                if comp.type.name in ('MeshRenderer','SkinnedMeshRenderer'):source.lighting_startup_materials[key(comp)]=source.get(state['off_material']['source']['file'],state['off_material']['source']['path_id'])
    cycle=recover_cycle(source,source.get('level1',64920),output)
    render=source.get('level1',56810);f=source.read(render);settings=source.get('globalgamemanagers',1);p=source.read(settings)
    result={'source':source_id(render),'color_space':p['m_ActiveColorSpace'],'ambient_mode':f['m_AmbientMode'],'ambient_intensity':f['m_AmbientIntensity'],
        'ambient_sky':rgba(f['m_AmbientSkyColor']),'ambient_equator':rgba(f['m_AmbientEquatorColor']),'ambient_ground':rgba(f['m_AmbientGroundColor']),
        'ambient_probe':list(f['m_AmbientProbe'].values()),'reflection_intensity':f['m_ReflectionIntensity'],
        'fog':{'enabled':bool(f['m_Fog']),'mode':f['m_FogMode'],'density':f['m_FogDensity'],'color':rgba(f['m_FogColor']),'start':f['m_LinearFogStart'],'end':f['m_LinearFogEnd']},
        'startup_ambient':cycle['day_ambient'],'startup_reflection':cycle['max_reflection'],'startup_fog_density':cycle['day_fog_densities'][0],
        'default_spot_cookie':texture(source,source.resolve(render,f['m_SpotCookie']),output),
        'skybox':sky_material(source,source.resolve(render,f['m_SkyboxMaterial']),output),'sun':1,'lights':source.lighting_lights,'cycle':cycle,'controllers':controls,
        'road_lamp':recover_road_lamp(source,output),'textures':list(source.lighting_textures.values()),'materials':[{'asset':m['asset'],'gltf_material':m['gltf_material'],'source':m['source']} for m in material_ids.values()]}
    graphics=source.get('globalgamemanagers',7);gf=source.read(graphics)
    quality=source.get('globalgamemanagers',12);qf=source.read(quality);q=qf['m_QualitySettings'][qf['m_CurrentQuality']]
    result['lights_use_linear_intensity']=bool(gf['m_LightsUseLinearIntensity'])
    result['quality']={'source':source_id(quality),'pixel_light_count':q['pixelLightCount'],'shadows':q['shadows'],
        'shadow_resolution':q['shadowResolution'],'shadow_projection':q['shadowProjection'],'shadow_cascades':q['shadowCascades'],
        'shadow_distance':q['shadowDistance'],'shadow_near_plane_offset':q['shadowNearPlaneOffset'],
        'shadow_cascade_splits':list(vector(q['shadowCascade4Split']))}
    result['cameras']=[]
    player=source.get('level1',80125);pf=source.read(player)
    for role in ('Cam','mainCam','farCamera','skyCamera'):
        camera=source.resolve(player,pf[role]);cf=source.read(camera)
        result['cameras'].append({'source':source_id(camera),'role':role,'near_plane':cf['near clip plane'],'far_plane':cf['far clip plane'],
            'field_of_view':cf['field of view'],'hdr':bool(cf['m_HDR']),'culling_mask':cf['m_CullingMask']['m_Bits']})
    source.lighting_lights[0]['startup_intensity']=cycle['day_intensity']
    source.lighting_lights[0]['color']=cycle['day_color']
    source.lighting=result
    return result


def validate_lighting_export(manifest: dict, output: Path) -> None:
    """Validate every exported byte and cross-asset link before acceptance."""
    lighting=manifest['lighting'];nodes={n['id']:n for n in manifest['nodes']}
    assets={a['id']:a for a in manifest['assets']};textures={t['id']:t for t in lighting['textures']}
    lights={l['id']:l for l in lighting['lights']};controls={c['id']:c for c in lighting['controllers']}
    if len(lights)!=len(lighting['lights']) or len(controls)!=len(lighting['controllers']):raise ValueError('Duplicate lighting ID')
    documents={a['id']:json.loads((output/Path(a['path']).name).read_bytes()) for a in manifest['assets']}
    def fail(source,field):raise ValueError(f"{source['file']}:{source['path_id']} {field}")
    def walk(value):
        if isinstance(value,dict):
            if 'path' in value and 'sha256' in value:
                path=output/Path(value['path']).name
                if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest()!=value['sha256']:fail(value['source'],'resource hash')
            for child in value.values():walk(child)
        elif isinstance(value,list):
            for child in value:walk(child)
    walk(lighting)
    from .mips import validate_mips
    for t in lighting["textures"]:
        mip=t.get("mip_data")
        if not mip:fail(t["source"],"mip_data missing")
        try:validate_mips((output/Path(mip["path"]).name).read_bytes(),t["width"],t["height"],t["mip_count"])
        except ValueError as error:fail(t["source"],str(error))
    for a in manifest['assets']:
        for d in a['dependencies']:
            path=output/Path(d['file']).name
            if not path.is_file() or hashlib.sha256(path.read_bytes()).hexdigest()!=d['sha256']:raise ValueError(f"{d['file']} dependency hash")
    for c in lighting['controllers']:
        if c['node'] not in nodes:fail(c['source'],'node')
        owner=nodes[c['node']]['asset']
        for state in c['states']:
            for nid in [*state['renderers'],*state['working_nodes'],*state['broken_nodes']]:
                if nid not in nodes or nodes[nid]['asset']!=owner:fail(c['source'],'state node asset')
            for lid in state['lights']:
                if lid not in lights or lights[lid]['node'] not in nodes or nodes[lights[lid]['node']]['asset']!=owner:fail(c['source'],'state light asset')
            for field in ('on_material','off_material'):
                m=state[field]
                if m:
                    document=documents.get(m['asset']);index=m['gltf_material']
                    if m['asset']!=owner or not document or not 0<=index<len(document['materials']) or document['materials'][index]['extras']['source']!=m['source']:fail(c['source'],field)
        if c['kind']=='switch' and not c['aim_colliders']:fail(c['source'],'aim colliders')
    for l in lighting['lights']:
        if l['cookie'] is not None and l['cookie'] not in textures:fail(l['source'],'cookie')
        if l['node'] is not None and l['node'] not in nodes:fail(l['source'],'node')
    for field in ('temp_days','temp_nights','temp_add_noons','temperature_values'):
        values=lighting['cycle'].get(field)
        if not isinstance(values,list) or len(values)!=3 or not all(isinstance(v,(int,float)) and math.isfinite(v) for v in values):
            fail(lighting['cycle']['source'],field+' requires three finite values')
    flares=lighting['cycle']['sun_flares']
    expected_flares=[57088,57090,57089]
    if len(flares)!=len(expected_flares):fail(lighting['cycle']['source'],'sun flare chain')
    for flare,pid in zip(flares,expected_flares):
        atlas=textures.get(flare['atlas'])
        if not (flare['source']=={'file':'level1','path_id':pid} and flare['component']['source']==flare['source']
                and flare['component'] in lighting['cycle']['sky_resources']
                and flare['flare']['source']=={'file':'sharedassets1.assets','path_id':4516}
                and atlas and atlas['source']=={'file':'sharedassets1.assets','path_id':744}):
            fail(flare['source'],'sun flare resource links')
        payload=json.loads((output/Path(flare['flare']['path']).name).read_bytes())
        if payload['m_FlareTexture']!={'m_FileID':0,'m_PathID':atlas['source']['path_id']}:
            fail(flare['source'],'sun flare atlas link')
        expected_elements=[{'image':e['m_ImageIndex'],'position':e['m_Position'],'size':e['m_Size'],'color':rgba(e['m_Color']),
            'use_light_color':bool(e['m_UseLightColor']),'rotate':bool(e['m_Rotate']),'zoom':bool(e['m_Zoom']),'fade':bool(e['m_Fade'])} for e in payload['m_Elements']]
        def finite_values(value,count):
            return isinstance(value,list) and len(value)==count and all(isinstance(v,(int,float)) and math.isfinite(v) for v in value)
        fade=flare.get('fade_speed');pose=flare.get('pose',{})
        if not (flare.get('elements')==expected_elements and expected_elements
                and flare.get('texture_layout')==payload['m_TextureLayout']==1
                and flare.get('use_fog')==bool(payload['m_UseFog'])
                and finite_values(flare.get('color'),4) and isinstance(fade,(int,float)) and math.isfinite(fade) and fade>=0
                and flare.get('directional') is True and isinstance(flare.get('ignore_layers'),int) and 0<=flare['ignore_layers']<=0xffffffff
                and all(finite_values(pose.get(field),count) for field,count in [('translation',3),('rotation',4),('scale',3)])):
            fail(flare['source'],'typed flare values')
    road=lighting['road_lamp']
    if assets[road['asset']]['usage']!='road_lamp_prototype' or controls[road['controller']]['kind']!='road':fail(road['source'],'prototype')
    if documents[road['asset']]['nodes'][0]['translation']!=IDENTITY['translation']:fail(road['source'],'prototype placement')
    json.dumps(lighting,allow_nan=False)


def validate_gradient(g):
    from .validation import record,array,number,integer
    def color(k):record(k,{'time':number,'value':array(number,4)})
    def alpha(k):record(k,{'time':number,'value':number})
    record(g,{'mode':integer(0,1),'colors':array(color),'alphas':array(alpha)})
    for keys in (g['colors'],g['alphas']):
        if not keys or any(not 0<=k['time']<=1 for k in keys) or any(a['time']>b['time'] for a,b in zip(keys,keys[1:])):
            raise ValueError('Invalid gradient keys')


def validate_sky_material(m):
    from .validation import record,array,mapping,number,u32,string,source
    def resource(r):
        record(r,{'source':source,'path':string,'sha256':string})
        if not r['path'] or len(r['sha256'])!=64 or any(c not in '0123456789abcdef' for c in r['sha256']):raise ValueError('Required material resource')
    def shader(s):
        record(s,{'source':source,'path':string,'sha256':string,'compiled_data':resource,'name':string})
        resource({k:s[k] for k in ('source','path','sha256')})
    def texture(t):record(t,{'property':string,'texture':u32,'scale':array(number,2),'offset':array(number,2)})
    record(m,{'source':source,'shader':shader,'source_data':resource,'floats':mapping(number),
              'colors':mapping(array(number,4)),'textures':array(texture)})


def light_payload(source,light,node,light_id,startup_intensity):
    """One Light component as a light record. `node` is its node id, `light_id` its id in its own table."""
    ld=source.read(light);shadow=ld['m_Shadows']
    transform=source.transform_for(source.resolve(light,ld['m_GameObject']))
    return {'id':light_id,'source':source_id(light),'node':node,'pose':converted_transform(source.read(transform)),
        'kind':{0:'spot',1:'directional',2:'point',3:'area'}[ld['m_Type']],'color':rgba(ld['m_Color']),
        'serialized_intensity':ld['m_Intensity'],'startup_intensity':startup_intensity,'range_meters':ld['m_Range'],
        'full_cone_degrees':ld['m_SpotAngle'],'inner_cone_degrees':ld['m_InnerSpotAngle'],'enabled':bool(ld['m_Enabled']),
        'culling_mask':ld['m_CullingMask']['m_Bits'],'shadows':{'kind':shadow['m_Type'],'strength':shadow['m_Strength'],
        'bias':shadow['m_Bias'],'normal_bias':shadow['m_NormalBias'],'near_plane':shadow['m_NearPlane'],'resolution':shadow['m_Resolution'],'custom_resolution':shadow['m_CustomResolution']},'cookie':None,'cookie_size':ld['m_CookieSize']}

def validate_light_payload(l):
    from .validation import record,array,number,u32,i32,integer,boolean,string,source,pose,optional
    def shadow(s):
        record(s,{'kind':integer(0,2),'strength':number,'bias':number,'normal_bias':number,'near_plane':number,'resolution':i32,'custom_resolution':i32})
        if not 0<=s['strength']<=1 or any(s[k]<0 for k in ('bias','normal_bias','near_plane')):raise ValueError('Light shadow values')
    record(l,{'id':u32,'source':source,'node':optional(u32),'pose':pose,'kind':string,'color':array(number,4),
              'serialized_intensity':number,'startup_intensity':number,'range_meters':number,'full_cone_degrees':number,
              'inner_cone_degrees':number,'enabled':boolean,'culling_mask':u32,'shadows':shadow,'cookie':optional(u32),'cookie_size':number})
    if l['kind'] not in ('spot','directional','point','area') or l['range_meters']<=0 or not 0<=l['inner_cone_degrees']<=l['full_cone_degrees']<=180 or l['full_cone_degrees']==0 or not l['culling_mask']:
        raise ValueError('Light source values')
