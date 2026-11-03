"""Recover the selected storm record from checked records."""
from .source import source_id,key,vector
from .survival import component,managed_evidence,snake
from .lighting import gradient,rgba
from .particles import particle
from .world_biome_materials import scene_component

MAIN=('level1',64919)
DATABASE=('level1',66024)
STORM=('sharedassets1.assets',158276)
STORM_FIELDS='lifeLossOnOtherBiome lifeGain destroyDist life radius fogDensity atmosphereThickness intensityMultipler ambientMultipler volume ambientDecrease yoffset windForce defSRateT defSRateD defBRateT defBRateD'.split()
STORM_COLORS=['nightColor','skyColor','skyGroundColor','fogColor','CloudColor']
# Values that only exist in sandstormscript code. The evidence lines prove each one.
STORM_EVIDENCE=['size = UnityEngine.Random.Range(0.5f, 1.5f);','speed = UnityEngine.Random.Range(3f, 12f);',
    'yield return new WaitForSeconds(1f);','nextWindTime = Time.time + UnityEngine.Random.Range(0.1f, 1.5f);',
    'mainscript.M.interiorCols[j].transform.root != windRbs[i].transform.root']
TUMBLEWEED_ITEM=(312,'gthumbleweed')
# thumbleweedMain fields: record name -> source fields. Ranges are [min, max].
TUMBLEWEED_SCALARS={'ray_count':'rayCount','ray_length':'rayLength','ray_count_to_stay':'rayCountToStay','limit':'thumbleweedLimit','fov':'fov'}
TUMBLEWEED_RANGES={'wind_force_range':'WindForce','wind_time_range':'WindTime','wind_live_time_range':'WindLiveTime',
    'short_spawn_time_range':'ShortSpawnTime','long_spawn_time_range':'LongSpawnTime'}
TUMBLEWEED_DISTANCES={'spawn_dist_range':'Dist','spawn_dist_in_fov_range':'DistinFov'}
TUMBLEWEED_MASKS={'spawn_ray_lm':'spawnRayLm','ray_lm':'rayLm'}
# Values that only exist in thumbleweedMain code. The 360 degree wall ring has no single evidence line.
TUMBLEWEED_EVIDENCE=['if (UnityEngine.Random.Range(0f, 1f) < 0.3f)','spawnPos.y = hit.point.y + 2f;',
    'if (Physics.Raycast(spawnPos + Vector3.up * 100000f, Vector3.down, out hit, float.PositiveInfinity, spawnRayLm))']

def owner(source,reader,name):
    if source.class_name(reader)!=name:raise ValueError(f'{key(reader)} is not {name}')
    return reader

def game_object(source,reader):
    return reader if reader.type.name=='GameObject' else source.resolve(reader,source.read(reader)['m_GameObject'])

def recover_tumbleweeds(source):
    main=scene_component(source,source.library,'thumbleweedMain');d=source.read(main)
    prefab=source.resolve(main,d['prefab'])
    if prefab is None:raise ValueError('Tumbleweed prefab link missing')
    prefab=game_object(source,prefab)
    database=owner(source,source.get(*DATABASE),'itemdatabase')
    item=source.resolve(database,source.read(database)[TUMBLEWEED_ITEM[1]])
    if item is None or key(item)!=key(prefab):raise ValueError('Tumbleweed prefab differs from item database')
    root=source.transform_for(prefab)
    saved=component(source,root,'tosaveitemscript')
    script=source.resolve(saved,source.read(saved)['thumbleweed'])
    if script is None or source.class_name(script)!='thumbleweedScript':raise ValueError('Tumbleweed script link missing')
    body=component(source,source.transform_for(game_object(source,script)),'Rigidbody')
    gibs={}
    for node in source.hierarchy(root,activate_root=True):
        for c in node['components']:
            if c.type.name!='MonoBehaviour':continue
            for field,value in source.read(c).items():
                if 'gib' in field.lower() and isinstance(value,dict) and set(value)=={'m_FileID','m_PathID'}:
                    target=source.resolve(c,value)
                    if target is not None:gibs[key(target)]=(target,f'{source.class_name(c)}.{field}')
    if len(gibs)>1:raise ValueError(f'Tumbleweed has several gib links: {[g[1] for g in gibs.values()]}')
    gib=next(iter(gibs.values()),None)
    settings=scene_component(source,source.library,'TerrainGenerationSettings')
    # CheckImportantObjsNull drops missing entries before each spawn.
    important=[t for t in (source.resolve(settings,e['T']) for e in source.read(settings)['importantObjs']) if t is not None]
    if not important:raise ValueError('Tumbleweed important object list is empty')
    result={'source':source_id(main),'item_id':TUMBLEWEED_ITEM[0],'prefab':source_id(prefab),'script':source_id(script),'body':source_id(body),
        'gib':source_id(game_object(source,gib[0])) if gib else None,
        'important_objects':[source_id(t) for t in important],
        **{k:d[f] for k,f in TUMBLEWEED_SCALARS.items()},
        **{k:[d['min'+f],d['max'+f]] for k,f in TUMBLEWEED_RANGES.items()},
        **{k:[d['spawnMin'+f],d['spawnMax'+f]] for k,f in TUMBLEWEED_DISTANCES.items()},
        **{k:d[f]['m_Bits'] for k,f in TUMBLEWEED_MASKS.items()},
        'long_spawn_chance':.3,'spawn_height':2.,'ray_height':100000.,'ring_degrees':360.,
        'evidence':managed_evidence(source,TUMBLEWEED_EVIDENCE)}
    names=[source.read(game_object(source,t))['m_Name'] for t in important]
    print(f"Tumbleweed spawner {key(main)}; important objects {[(key(t),n) for t,n in zip(important,names)]}; gib {gib[1] if gib else None}",flush=True)
    return result

def recover_weather(source, output):
    main=owner(source,source.get(*MAIN),'mainscript')
    scheduler=source.resolve(main,source.read(main)['sandStormSpawn'])
    if scheduler is None:raise ValueError('Storm scheduler link missing')
    sd=source.read(owner(source,scheduler,'sandStormSpawnScript'))
    database=owner(source,source.get(*DATABASE),'itemdatabase');storms=source.read(database)['storms']
    if not 0<=sd['prefabid']<len(storms):raise ValueError('Storm prefab id differs')
    prefab=source.resolve(database,storms[sd['prefabid']])
    if prefab is None:raise ValueError('Storm database link missing')
    root=source.transform_for(prefab);storm=component(source,root,'sandstormscript');d=source.read(storm)
    if key(storm)!=STORM:raise ValueError('Storm database link differs')
    result={'source':source_id(scheduler),'prefab_id':sd['prefabid'],'min_interval_seconds':sd['minTime'],'max_interval_seconds':sd['maxTime'],'spawn_radius':sd['radius'],
        'storm':{'source':source_id(storm),'biome':d['biome'],**{snake(k):d[k] for k in STORM_FIELDS},
            **{snake(k):rgba(d[k]) for k in STORM_COLORS},
            'day_color1':gradient(d['dayColor1']),'day_color2':gradient(d['dayColor2']),'wind_lm':d['windLm']['m_Bits'],
            'current_force':list(vector(d['currentForce'])),'next_force':list(vector(d['nextForce'])),'distance_lerp':d['distanceLerp'],
            'size_range':[.5,1.5],'speed_range':[3.,12.],'wind_time_range':[.1,1.5],'wind_scan_seconds':1.,'wind_bounds_range':[1.,30.]},
        'evidence':managed_evidence(source,STORM_EVIDENCE),
        'particles':[particle(source,source.resolve(storm,d[field]),role,rate,output,root)
            for role,field,rate in [('storm_big','bigParticleSystem',d['defBRateT']),('storm_small','smallParticleSystem',d['defSRateT'])]],
        'tumbleweeds':recover_tumbleweeds(source)}
    source.lighting['textures']=list(source.lighting_textures.values())
    return result
