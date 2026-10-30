"""Recover selected particle data from checked records."""
from .source import source_id,vector
from .survival import component
from .lighting import gradient,rgba,sky_material
from .collision import world_pose,quaternion_product

def curve(data):
    def keys(c):
        return {'keys':[{'time':k['time'],'value':k['value'],'in_tangent':k['inSlope'],'out_tangent':k['outSlope'],
            'in_weight':k['inWeight'],'out_weight':k['outWeight'],'weighted_mode':k['weightedMode']} for k in c['m_Curve']],
            'pre_wrap_mode':c['m_PreInfinity'],'post_wrap_mode':c['m_PostInfinity']}
    return {'mode':data['minMaxState'],'min_scalar':data['minScalar'],'scalar':data['scalar'],
        'min_curve':keys(data['minCurve']),'max_curve':keys(data['maxCurve'])}

def color(data):
    return {'mode':data['minMaxState'],'min_color':rgba(data['minColor']),'max_color':rgba(data['maxColor']),
        'min_gradient':gradient(data['minGradient']),'max_gradient':gradient(data['maxGradient'])}

def particle(source,reader,role,rate,output,owner=None):
    d=source.read(reader);root=source.transform_for(source.resolve(reader,d['m_GameObject']))
    renderer=component(source,root,'ParticleSystemRenderer');rd=source.read(renderer)
    result={'source':source_id(reader),'role':role,'capacity':d['InitialModule']['maxNumParticles'],
        'rate_over_time':rate,'rate_over_distance':d['EmissionModule']['rateOverDistance']['scalar'],
        'duration':d['lengthInSec'],'simulation_space':d['moveWithTransform'],'local_pose':relative_pose(source,root,owner) if owner is not None else world_pose(source,root),
        'module_flags':{k:bool(v['enabled']) for k,v in d.items() if isinstance(v,dict) and 'enabled' in v},
        'curves':{},'colors':{},'vectors':{},'scalars':{},'flags':{},'links':{},
        'materials':[sky_material(source,source.resolve(renderer,p),output) for p in rd['m_Materials'] if p['m_PathID']],
        'limits':[]}
    def visit(value,path):
        if isinstance(value,dict):
            if 'minMaxState' in value:
                target='curves' if 'scalar' in value else 'colors'
                result[target][path]=curve(value) if target=='curves' else color(value)
            elif set(value)=={'x','y','z'}:result['vectors'][path]=list(vector(value))
            elif set(value)=={'m_FileID','m_PathID'}:
                linked=source.resolve(reader,value)
                if linked:result['links'][path]=source_id(linked)
            else:
                for k,v in value.items():visit(v,f'{path}.{k}' if path else k)
        elif isinstance(value,list):
            for i,v in enumerate(value):visit(v,f'{path}.{i}')
        elif isinstance(value,bool):result['flags'][path]=value
        elif isinstance(value,(int,float)):result['scalars'][path]=value
        else:raise ValueError(f'Unsupported particle field: {path}')
    for k,v in d.items():
        if isinstance(v,dict) and 'enabled' in v and not v['enabled']:continue
        visit(v,k)
    result['flags']['gameObject.active']=bool(source.read(source.resolve(reader,d['m_GameObject']))['m_IsActive'])
    result['flags']['renderer.enabled']=bool(rd['m_Enabled'])
    result['scalars']['gameObject.layer']=source.read(source.resolve(reader,d['m_GameObject']))['m_Layer']
    for k in ['m_RenderMode','m_SortMode','m_MinParticleSize','m_MaxParticleSize','m_CameraVelocityScale','m_VelocityScale','m_LengthScale','m_SortingFudge','m_RenderAlignment','m_Pivot','m_Flip']:
        visit(rd[k],'renderer.'+k)
    if result['module_flags']['CollisionModule']:
        result['limits'].append({'source':source_id(reader),'module':'CollisionModule','visible_effect':'Retained ParticleState spray pools and FluidContact outlet pools cast masked non-sensor Rapier swept spheres, apply bounce, dampen and lifetime loss, and remove initial penetration. Native collision cache, collision messages and world tank-fill callbacks remain outside this route. Native particle trigger membership remains unproved; ray queriesHitTriggers does not govern this adaptation.'})
    return result

def recover_particles(source, output):
    particles=[]
    player=source.get('level1',80125);outlet=source.resolve(player,source.read(player)['piss']);od=source.read(outlet)
    player_root=source.transform_for(source.resolve(outlet,od['m_GameObject']))
    for role,field in [('urine','ps'),('urine_splash','sps')]:
        reader=source.resolve(outlet,od[field]);rate=od['maxRate'] if field=='ps' else source.read(reader)['EmissionModule']['rateOverTime']['scalar']
        particles.append(particle(source,reader,role,rate,output,player_root))
    for p in particles:
        outlet_values(source,p,od)
    source.lighting['textures']=list(source.lighting_textures.values())
    return particles

def validate_particles_export(particles,manifest):
    if len(particles)!=2+2*len(manifest['openings']) or len({p['role'] for p in particles})!=len(particles):raise ValueError('Missing selected particles')
    textures={t['id'] for t in manifest['lighting']['textures']}
    for p in particles:validate_particle(p,textures)

def validate_particle(p,textures):
    if not 0<p['capacity']<=1000000 or p['rate_over_time']<0 or p['rate_over_distance']<0:raise ValueError('Invalid particle capacity or rate')
    if p['capacity']!=p['scalars']['InitialModule.maxNumParticles']:raise ValueError('Particle authored capacity differs')
    if not p['materials'] or 'InitialModule.startLifetime' not in p['curves']:raise ValueError('Missing particle data')
    for material in p['materials']:
        if any(t['texture'] not in textures for t in material['textures']):raise ValueError('Missing particle texture')

# Modules that crates/simulation/src/particles.rs runs. ShapeModule and EmissionModule feed emission; the others change live particles.
EXHAUST_MODULES=frozenset({'InitialModule','ShapeModule','EmissionModule','VelocityModule','ForceModule',
    'InheritVelocityModule','SizeModule','ColorModule','RotationModule','CollisionModule','UVModule'})

def recover_exhaust(source,car_reader,car,owner,index,output):
    """Read carscript.psSmoke. The runtime sets the rate, so the exported rate is 1."""
    systems=[source.resolve(car_reader,pointer) for pointer in car['psSmoke']]
    if not systems or any(s is None for s in systems):raise ValueError(f'Car needs exhaust particle systems: {source_id(car_reader)}')
    result=[particle(source,s,f'exhaust_{index}_{i}',1.,output,owner) for i,s in enumerate(systems)]
    for p in result:validate_runtime_particle(p)
    return result

# Coolant steam also runs the NoiseModule position noise.
STEAM_MODULES=EXHAUST_MODULES|{'NoiseModule'}

def recover_coolant_steam(source,car_reader,car,owner,output):
    """Read carscript.CoolantSteam. carscript runs it only with CoolantBoil, so a car without one has none.

    The runtime sets the rate, so the exported rate is 1.
    """
    steam=source.resolve(car_reader,car['CoolantSteam']);boil=source.resolve(car_reader,car['CoolantBoil'])
    if steam is None:return None
    # carscript enables the emission. Without CoolantBoil nothing enables it.
    if source.read(steam)['EmissionModule']['enabled']:raise ValueError(f'Coolant steam emission is on: {source_id(steam)}')
    if boil is None:return None
    p=particle(source,steam,f'coolant_steam_{car_reader.path_id}',1.,output,owner)
    validate_runtime_particle(p,STEAM_MODULES)
    if p['module_flags'].get('NoiseModule'):
        if p['flags']['NoiseModule.remapEnabled'] or any(p['curves'][f'NoiseModule.{k}']['mode']!=0 or p['curves'][f'NoiseModule.{k}']['scalar']!=0
                for k in ('rotationAmount','sizeAmount')):raise ValueError(f"Unsupported coolant steam noise {p['role']}")
    return {'particle':p,'max_rate':car['CoolantSteamMaxRate']}

# mainscript.foodParticleType order.
FOOD_PARTICLES=('crumbs','meat','shit','ash','csoki')

def recover_food_particles(source,output):
    """mainscript.FoodParticles. FoodParticle moves the shared system to the bite and calls Emit(num)."""
    main=source.get('level1',64919);systems=source.read(main)['FoodParticles']
    if len(systems)!=len(FOOD_PARTICLES):raise ValueError('mainscript FoodParticles count differs')
    result=[]
    for name,pointer in zip(FOOD_PARTICLES,systems):
        system=source.resolve(main,pointer)
        if system is None or system.type.name!='ParticleSystem':raise ValueError(f'mainscript FoodParticles {name} is not ParticleSystem')
        # Emission does not run. The system pose is the bite pose.
        p=particle(source,system,f'food_{name}',0.,output)
        p['local_pose']={'translation':[0.,0.,0.],'rotation':[0.,0.,0.,1.],'scale':[1.,1.,1.]}
        if p['module_flags']['EmissionModule']:raise ValueError(f'Food particle emission is on: {name}')
        validate_runtime_particle(p,EXHAUST_MODULES,1)
        result.append(p)
    return result

def validate_runtime_particle(p,modules=EXHAUST_MODULES,simulation_space=2):
    unsupported=sorted(k for k,v in p['module_flags'].items() if v and k not in modules)
    if unsupported:raise ValueError(f"Unsupported particle modules {p['role']}: {unsupported}")
    # The runtime plays a whole grid sheet over lifetime only.
    if p['module_flags'].get('UVModule') and any(p['scalars'][f'UVModule.{k}']!=0 for k in ('mode','animationType','timeMode')):
        raise ValueError(f"Unsupported texture sheet {p['role']}")
    validate_particle_data(p,simulation_space)

def relative_pose(source, root, owner):
    from .gltf import transform_point
    a=world_pose(source,owner);b=world_pose(source,root)
    inverse=[-v for v in a['rotation'][:3]]+[a['rotation'][3]]
    delta=[b['translation'][i]-a['translation'][i] for i in range(3)]
    return {'translation':list(transform_point(delta,(0.,0.,0.),inverse,(1.,1.,1.))),
        'rotation':list(quaternion_product(inverse,b['rotation'])),
        'scale':[b['scale'][i]/a['scale'][i] for i in range(3)]}

def outlet_values(source,p,cap):
    p['scalars'].update({'outlet.maxSpeed':cap['maxSpeed'],'outlet.minRate':cap['minRate'],'outlet.maxRate':cap['maxRate'],
        'outlet.forceColorM':cap['forceColorM']})
    p['flags']['outlet.forceColor']=bool(cap['BforceColor'])
    p['vectors']['outlet.forceColor']=rgba(cap['forceColor'])[:3]
    p['scalars']['outlet.forceAlpha']=cap['forceColor']['a']
    main=source.read(source.get('level1',64919))
    for i,f in enumerate(main['fluids']):
        p['vectors'][f'fluid.{i}']=rgba(f['color'])[:3]
        p['scalars'][f'fluid.{i}.alpha']=f['color']['a']

def cap_particles(source,cap,suffix,output):
    """Outlet and splash particles of one tankcapscript, relative to its cap Transform."""
    cd=source.read(cap);owner=source.transform_for(source.resolve(cap,cd['m_GameObject']));particles=[]
    for prefix,field in [('outlet','ps'),('splash','sps')]:
        reader=source.resolve(cap,cd[field]);rate=cd['maxRate'] if field=='ps' else source.read(reader)['EmissionModule']['rateOverTime']['scalar']
        p=particle(source,reader,f'{prefix}_{suffix}',rate,output,owner)
        outlet_values(source,p,cd);particles.append(p)
    return particles

def recover_outlet_particles(source,manifest,output):
    return [p for opening in manifest['openings']
        for p in cap_particles(source,source.get(opening['source']['file'],opening['source']['path_id']),opening['id'],output)]



def validate_curve(c):
    from .validation import record,array,number,i32,integer
    def key(k):
        record(k,{**dict.fromkeys(('time','value','in_tangent','out_tangent','in_weight','out_weight'),number),
                  'weighted_mode':integer(0,3)})
    record(c,{'keys':array(key),'pre_wrap_mode':i32,'post_wrap_mode':i32})
    if any(a['time']>=b['time'] for a,b in zip(c['keys'],c['keys'][1:])):raise ValueError('Unordered curve keys')


def validate_particle_data(p, simulation_space):
    from .validation import record,array,mapping,number,u32,integer,boolean,string,source,pose
    from .lighting import validate_gradient,validate_sky_material
    def particle_curve(c):
        record(c,{'mode':integer(0,3),'min_scalar':number,'scalar':number,'min_curve':validate_curve,'max_curve':validate_curve})
        if c['mode'] in (1,2) and not c['max_curve']['keys'] or c['mode']==2 and not c['min_curve']['keys']:
            raise ValueError('Missing particle curve keys')
    def particle_color(c):
        record(c,{'mode':integer(0,4),'min_color':array(number,4),'max_color':array(number,4),
                  'min_gradient':validate_gradient,'max_gradient':validate_gradient})
    def limit(l):record(l,{'source':source,'module':string,'visible_effect':string})
    record(p,{'source':source,'role':string,'capacity':u32,'rate_over_time':number,'rate_over_distance':number,
              'duration':number,'simulation_space':integer(0,255),'local_pose':pose,'module_flags':mapping(boolean),
              'curves':mapping(particle_curve),'colors':mapping(particle_color),'vectors':mapping(array(number,3)),
              'scalars':mapping(number),'flags':mapping(boolean),'links':mapping(source),
              'materials':array(validate_sky_material,1),'limits':array(limit)})
    if not 0<p['capacity']<=1000000 or p['scalars'].get('InitialModule.maxNumParticles')!=p['capacity']:
        raise ValueError('Particle capacity')
    if p['rate_over_time']<0 or p['rate_over_distance']<0 or p['duration']<=0 or p['simulation_space']!=simulation_space:
        raise ValueError('Particle rates and simulation space')
    if p['module_flags'].get('InitialModule') is not True or 'InitialModule.startLifetime' not in p['curves']:
        raise ValueError('Required particle initial module')
    mat=p['materials'][0];tint='_TintColor' if mat['shader']['name']=='Legacy Shaders/Particles/Alpha Blended' else '_Color'
    if tint not in mat['colors'] or not any(t['property']=='_MainTex' for t in mat['textures']):raise ValueError('Particle material payload')
    for l in p['limits']:
        if l['source']!=p['source'] or not l['visible_effect'] or not (p['module_flags'].get(l['module']) or l['module'] in ('material.soft_particles','material.distortion')):
            raise ValueError('Particle limit')
