"""Recover selected world inputs from the checked source library."""
import hashlib
import math
import copy
from pathlib import Path
from .source import LibrarySource, key, source_id, vector
from .library.reader import LibraryReader
from .library.types import ObjectId
from .gltf import GltfExporter, convert_bind_poses, converted_transform
from .collision import geometry, step_surface, collider_tag, world_pose
from .triggers import recover_trigger_role
from . import draw_distance, mirrors
from .sled import centers_of_mass, recover_sled
from .actions import recover_world_pushes
from .towing import recover_hitches, recover_trailer
from .followers import recover_followers

# Unity Random state before and after the garage startup draws, captured once from the source game
# with the clock frozen at 2026-10-12T09:00:00 local.
SPAWN_CONTEXT = {'classification':'controlled_startup_before_garage',
    'engine_sha256':'553c1e7560e1b42bd4638c53d0168dcd1b4b9eb5b9df324b0a44f14b3f18bc86',
    'clock_input':'2026-10-12T09:00:00 local',
    'initial_state':[123456789,2139825738,225031475,1427228704],
    'startup_state':[2365297906,2858699345,3940856323,829372343],
    'startup_draws':4,'date':[2026,10,12]}
WORLD_DATABASE_ID = 66024
BUILDING_FIELDS = ('berror','bkezdohaz','bkut','bbudi','bbenzinkut01','bbuszmegallo','blegjobbbenzinkut','bmuhely','bnagybenzinkut','boldhouse','boldpetrolstation','bturrethaz','bvizmu','btelefonfulke','bpost','bcabin','byellowfoodkut','bfalu01','bcastle')
HOME_BUILDING = 70166
GENERATORS = (65060, 64958, 64720, 65037)
RULE_CLASSES = {'roadscript', 'buildingscript', 'newRandomStuffSpawnScript', 'stuffspawn',
    'terrainRandomStuffSpawn', 'terrainHeightAlignToBuildingScript', 'digholescript2',
    'tankscript', 'openablefluidcontainerscript', 'benzinkut', 'kozlampscript', 'kozlampscript2',
    'pickupable','usablescript','pushablescript','massScript','layerScript','tosaveitemscript',
    'enginescript','coolantTankscript','seatscript','tankcapscript','gumiscript','wheelscript',
    'craftscript','partscript','capsscript','kozlampa','electronicsscript','headlightscript','rearlights','flashlightscript',
    'randomFlyer','newAiScript','nyulscript','paintingMain','childunparent','randomRock','RandomSpawnedObjScript','randomScaleScript','undergroundSpawnScript','spawnInBoxScript','paintingScript','ammoscript','foodscript','randomTypeSelector','ginlampscript','partconditionscript','partslotscript','wheelgraphicsscript','doorscript','benzinkutscript',
    # Portable electronics (P09): compass, clock, radios, music box and their dials.
    'compassscript','simplecompassscript','clockscript','radioscript','musicboxscript','meterscript','mutatoscript','forgoszamlap',
    # Small item actions (P10): spirit level, spray bottle, boxes, wash spots, cigarette packs.
    'bubbleLevelScript','spricniscript','breakablescript','washtriggerscript','cigiscript',
    # Digging and detection (W06): metal detector.
    'metalDetectorScript',
    # Power cables (W04): rope segments and their pole targets.
    'ropeScript',
    # Towing (V05): the trailer grip spawns at Start.
    'attachgripscript',
    # Explosions (X02 part 4): explosive props.
    'explosivescript',
    # Binoculars (S2).
    'tavcsoscript'}
# Catalog items that only StartSpawnItems holds. No spawner reaches them.
START_ONLY_ITEMS = {85}
# ediblescript components of the selected food prototypes (survival.FOOD_FIELDS and GTurd).
EDIBLE_COMPONENTS = {('sharedassets1.assets',path_id) for path_id in (134641,134643,134644,134645,134646,134647,149631)}
def selected_actions(source, scripts, actions):
    return sorted({source.class_name(c) for c in scripts
        if source.class_name(c) in actions and key(c) not in EDIBLE_COMPONENTS})

VEHICLE_CLASSES = {'carscript'}
# The StartCar02 prefab.
LADA = ('sharedassets1.assets',82556)
# Catalog categories whose car items export and spawn from pools.
CAR_CATEGORIES = ('bus', 'bike')
# Catalog car items that export by name and spawn from pools (`kaszni` bare body, `carfull` full car).
# Car06P and Car09T are V03. The full Lada is LADA; car02 is its bare body.
CAR_ITEMS = ('car01','car01full','car02','car02k','car02kfull','car04','car04full','car05','car05full','car06','car06full',
    'car03','car03full','car03k','car03kfull','car07','car07full','car08','car08full','car08c','car08cfull',
    'car09','car09full','car09fullS','car09P','car09Pfull','car10','car10full','car10k','car10kfull')
# TerrainGenerationSettings start car fields and the catalog item of their car type. StartCar02 is the Lada.
START_CARS = {'StartCar01':'car01full','StartCar02':None,'StartCar02k':'car02kfull','StartCar03':'car03full','StartCar03k':'car03kfull',
    'StartCar04':'car04full','StartCar05':'car05full','StartCar06':'car06full','StartCar08':'car08full','StartCar08c':'car08cfull',
    'StartCar09':'car09full','StartCar09S':'car09fullS','StartCar09P':'car09Pfull','StartCar10':'car10full','StartCar10k':'car10kfull'}
# partconditionscript fields that a scene start car sets on its prefab copy.
START_CONDITION_FIELDS = ('randomStart','randomMin','randomMax','enableOverrideMin','overRideMin','enableOverrideMax','overRideMax')
# Known scene car roots (level1 GameObject) of start car fields: the Lada and Car09full_slide.
START_ROOTS = {'StartCar02':17687,'StartCar09S':12382}
def vehicle_exclusion(root, classes, cars):
    """Car roots outside `cars` stay other_vehicle."""
    return 'other_vehicle' if classes & VEHICLE_CLASSES and key(root) not in cars else ''

def category_items(catalog, name):
    """Item indices of catalog category `name` and its child categories."""
    rows=catalog['categories'];found=set();seen=set()
    def visit(row):
        if row['index'] in seen:return
        seen.add(row['index']);found.update(row['items'] or [])
        for child in row['categories'] or []:
            if child<len(rows):visit(rows[child])
    matches=[r for r in rows if r['name']==name]
    if not matches:raise ValueError(f'level1:{WORLD_DATABASE_ID} unknown category {name}')
    visit(matches[0])
    return sorted(found)

def catalog_item(catalog, name):
    """The one catalog item named `name`."""
    matches=[i for i in catalog['items'] if i['name']==name]
    if len(matches)!=1:raise ValueError(f'level1:{WORLD_DATABASE_ID} item {name}: {len(matches)} catalog matches')
    return matches[0]

def script_classes(source, nodes):
    return {source.class_name(c) for n in nodes for c in n['components']
        if c.type.name=='MonoBehaviour' and source.read(c)['m_Script']['m_PathID']}

def car_prototypes(source, catalog):
    """The car roots that export as world prototypes: the Lada, the CAR_CATEGORIES pools and the CAR_ITEMS.
    A trailer root (utanfutoscript) has no carscript and exports as a world prototype too."""
    cars={LADA}
    def root(item):
        return _source_root(source,source.get(item['source']['file'],item['source']['path_id']))
    for name in CAR_CATEGORIES:
        for index in category_items(catalog,name):
            target=root(catalog['items'][index])
            classes=script_classes(source,source.hierarchy(target))
            if 'carscript' in classes:cars.add(key(target))
    for name in CAR_ITEMS:
        target=root(catalog_item(catalog,name))
        classes=script_classes(source,source.hierarchy(target))
        if 'carscript' not in classes:
            raise ValueError('{}:{} item {}: root is not a car'.format(*key(target),name))
        cars.add(key(target))
    return cars

def start_placements(source, settings_owner, settings, catalog, cars):
    """One start car candidate per START_CARS field, at the scene pose of the car root in its group.
    The scene root is a copy of the prefab of the catalog item. The placement uses the prefab."""
    rows=[]
    for field,item in START_CARS.items():
        group=source.resolve(settings_owner,settings[field])
        if group is None:raise ValueError('{}:{} {}: missing start car group'.format(*key(settings_owner),field))
        roots=[n for n in source.hierarchy(source.transform_for(group)) if 'carscript' in script_classes(source,[n])]
        if len(roots)!=1:raise ValueError('{}:{} {}: {} car roots in the start car group'.format(*key(group),field,len(roots)))
        car=roots[0]
        if field in START_ROOTS and key(car['game_object'])!=('level1',START_ROOTS[field]):
            raise ValueError('{}:{} {}: the car root is not level1 GO {}'.format(*key(car['game_object']),field,START_ROOTS[field]))
        if item is None:prototype=source.get(*LADA)
        else:
            row=catalog_item(catalog,item)
            prototype=_source_root(source,source.get(row['source']['file'],row['source']['path_id']))
        if key(prototype) not in cars:raise ValueError('{}:{} {}: start car prefab is not an allowed car'.format(*key(prototype),field))
        rows.append({'group':field,'source':source_id(car['reader']),'pose':world_pose(source,car['reader']),
            'prototype':source_id(prototype),'start_car':True,'conditions':start_conditions(source,car['reader'],prototype)})
    return rows

def start_conditions(source, car, prototype):
    """Scene values of the partconditionscript random start fields that differ from the prefab.
    The scene start cars start worn (randomStart, ranges 1-3); their prefabs start new."""
    scene=source.hierarchy(car);prefab=source.hierarchy(prototype)
    if len(scene)!=len(prefab):raise ValueError('{}:{} start car hierarchy differs from its prefab'.format(*key(car)))
    def conditions(node):
        return [c for c in node['components'] if c.type.name=='MonoBehaviour' and source.class_name(c)=='partconditionscript']
    rows=[]
    for s,p in zip(scene,prefab):
        if s['path'].partition('/')[2]!=p['path'].partition('/')[2]:
            raise ValueError('{}:{} start car node differs from its prefab'.format(*key(s['reader'])))
        sc,pc=conditions(s),conditions(p)
        if len(sc)!=len(pc):raise ValueError('{}:{} start car conditions differ from its prefab'.format(*key(s['reader'])))
        for a,b in zip(sc,pc):
            d,e=source.read(a),source.read(b)
            if all(d[f]==e[f] for f in START_CONDITION_FIELDS):continue
            rows.append({'source':source_id(b),'random_start':bool(d['randomStart']),'random_min':d['randomMin'],'random_max':d['randomMax'],
                'override_min':d['overRideMin'] if d['enableOverrideMin'] else None,
                'override_max':d['overRideMax'] if d['enableOverrideMax'] else None})
    return rows

IDENTITY = {'translation':[0.,0.,0.], 'rotation':[0.,0.,0.,1.], 'scale':[1.,1.,1.]}


def part_scope(parts, slots, excluded_actions):
    """Return matching slot sources. A part without a slot spawns loose and cannot attach."""
    matches=[slot['source'] for slot in slots if any(
        (not slot['main_types'] or not p['maintipus'] or set(slot['main_types']) & set(p['maintipus']))
        and set(slot['part_types']) & set(p['tipus']) for p in parts)]
    if parts and not matches:excluded_actions=sorted(set(excluded_actions) | {'part_attachment'})
    return matches,excluded_actions

def required_record(source, owner, fields):
    """Check required fields and all their links before opening a dependent record."""
    identity = f'{key(owner)[0]}:{key(owner)[1]}'
    try:
        data = source.read(owner)
    except (KeyError, ValueError) as error:
        raise ValueError(f'{identity} record: {error}') from error
    def check(value, field):
        if isinstance(value, dict):
            if set(value) == {'m_FileID','m_PathID'}:
                if value['m_PathID']:
                    try:
                        if source.resolve(owner,value) is None: raise KeyError('null target')
                    except (KeyError, IndexError, ValueError) as error:
                        raise ValueError(f'{identity} {field}: unresolved source link {value}') from error
            else:
                for name, child in value.items(): check(child, f'{field}.{name}')
        elif isinstance(value, (list,tuple)):
            for index, child in enumerate(value): check(child, f'{field}[{index}]')
        elif isinstance(value,float) and not math.isfinite(value):
            raise ValueError(f'{identity} {field}: nonfinite source value')
    for field in fields:
        if field not in data: raise ValueError(f'{identity} {field}: missing required field')
        check(data[field],field)
    return data


class Selection:
    def __init__(self, source, reader):
        self.source,self.reader = source,reader
        self.receipts = {}
        self.records = {}

    def transform_chain(self, owner):
        while owner is not None:
            self.record(owner)
            data = required_record(self.source, owner, ('m_Father',))
            owner = self.source.resolve(owner, data['m_Father'])

    def receipt(self, owner):
        identity=key(owner)
        if identity not in self.receipts:
            # record() checks the identity and bytes before any dependent file read.
            self.source.read(owner)
            record=self.reader.record_files[f'{identity[0]}/{identity[1]}.json']
            self.receipts[identity]={'source':source_id(owner),'sha256':record['sha256']}
        return source_id(owner)

    def component_record(self, owner):
        """Retain bounds and lamp inputs, and owner evidence for other components."""
        class_name = self.source.class_name(owner) if owner.type.name == 'MonoBehaviour' else owner.type.name
        if class_name in {'LODGroup', 'Light', 'kozlampa'}:
            return self.record(owner)
        identity=key(owner)
        if identity in self.records:return self.records[identity]
        data=required_record(self.source,owner,('m_GameObject',))
        game_object=self.source.resolve(owner,data['m_GameObject'])
        if game_object is None:raise ValueError(f'{identity} m_GameObject: missing component owner')
        target=self.receipt(game_object)
        result={'source':self.receipt(owner),
                'class_name':class_name,
                'fields':{'m_GameObject':{'kind':'link','value':target}},
                'links':[{'field':'m_GameObject','target':target}]}
        self.records[identity]=result
        return result

    def record(self, owner):
        identity=key(owner)
        if identity in self.records:return self.records[identity]
        data=self.source.read(owner)
        required_record(self.source,owner,tuple(data))
        links=[]
        def convert(value, field):
            if isinstance(value,dict):
                if set(value)=={'m_FileID','m_PathID'}:
                    target=self.source.resolve(owner,value)
                    if target is None:return {'kind':'empty'}
                    identity=self.receipt(target)
                    links.append({'field':field,'target':identity})
                    return {'kind':'link','value':identity}
                return {'kind':'record','value':{k:convert(v,f'{field}.{k}') for k,v in value.items()}}
            if isinstance(value,(list,tuple)):
                return {'kind':'array','value':[convert(v,f'{field}[{i}]') for i,v in enumerate(value)]}
            if value is None:return {'kind':'empty'}
            kind='bool' if isinstance(value,bool) else 'integer' if isinstance(value,int) else 'number' if isinstance(value,float) else 'text'
            return {'kind':kind,'value':value}
        result={'source':self.receipt(owner), 'class_name':self.source.class_name(owner) if owner.type.name=='MonoBehaviour' else owner.type.name,
                'fields':{k:convert(v,k) for k,v in data.items()}, 'links':links}
        self.records[identity]=result
        return result


def _source_root(source, owner):
    return owner if owner.type.name=='Transform' else source.transform_for(owner if owner.type.name=='GameObject' else source.resolve(owner,source.read(owner)['m_GameObject']))


def source_material(source, selected, owner, output, texture_source):
    """Retain source shader inputs, including uninitialized render targets."""
    from .lighting import resource, texture, rgba
    data=required_record(source,owner,('m_Shader','m_SavedProperties','m_ValidKeywords'))
    selected.record(owner)
    shader=source.resolve(owner,data['m_Shader'])
    shader_fields=source.read(shader)
    suffix,payload=source.shader_resource(shader)
    shader_data=resource(shader,output,suffix,payload)
    shader_data.update(compiled_data=resource(shader,output,'shader.bin',source.raw_data(shader)),name=shader_fields['m_Name'] or shader_fields['m_ParsedForm']['m_Name'])
    props=data['m_SavedProperties'];textures=[];targets=[]
    for name,env in props['m_TexEnvs']:
        image=source.resolve(owner,env['m_Texture'])
        if image is None:continue
        binding={'property':name,'scale':[env['m_Scale'][c] for c in ('x','y')],'offset':[env['m_Offset'][c] for c in ('x','y')]}
        if image.type.name=='RenderTexture':
            selected.record(image);targets.append(dict(binding,source=source_id(image)))
        else:textures.append(dict(binding,texture=texture(texture_source,image,output)))
    return {'source':source_id(owner),'valid_keywords':data['m_ValidKeywords'],'shader':shader_data,'source_data':resource(owner,output,'bin',source.raw_data(owner)),
        'floats':dict(props['m_Floats']),'colors':{k:rgba(v) for k,v in props['m_Colors']},'textures':textures,'render_targets':targets,
        'texture_coordinates':{name:[env['m_Scale'][c] for c in ('x','y')]+[env['m_Offset'][c] for c in ('x','y')] for name,env in props['m_TexEnvs']}}


def recover_world(source: LibrarySource, reader: LibraryReader, output: Path, first_asset: int) -> dict:
    from .export import provenance
    print('Selecting world source records',flush=True)
    selected=Selection(source,reader)
    settings_owner=source.get('level1',64898)
    settings=required_record(source,settings_owner,('roadGens','terranGenerator','roadBuildingGeneration','objGeneration','mountainGeneration','desertTowerGeneration','desertBuildingGeneration','Terrain','DistantTerrain','DistantTerrain2',*START_CARS))
    road_owner=source.get('level1',64771)
    road=required_record(source,road_owner,('settings','terrainGenerator','prefabs','noises','roadBoneNum','roadBoneLength','length'))
    if [key(source.resolve(settings_owner,p)) for p in settings['roadGens']] != [key(road_owner)]:
        raise ValueError('level1:64898 roadGens: unsupported selected generator')
    roots={}
    roles={}
    selections=[]
    def add(owner,role):
        root=_source_root(source,owner)
        if key(root) not in roots:
            roots[key(root)]=root;roles[key(root)]=role
        return source_id(root)
    road_roots=[add(source.resolve(road_owner,p),'road') for p in road['prefabs']]
    for road_root in road_roots:
        root=source.get(road_root['file'],road_root['path_id'])
        controller=next(c for c in source.hierarchy(root,activate_root=True)[0]['components'] if c.type.name=='MonoBehaviour' and source.class_name(c)=='roadscript')
        pole=source.resolve(controller,source.read(controller)['randomPoleRotTransform'])
        roots[key(pole)]=pole;roles[key(pole)]='prop'
    generators=[]
    for pid in GENERATORS:
        owner=source.get('level1',pid)
        data=required_record(source,owner,('objTypes','settings','noSpawnChance','pointDist','seedplus'))
        types=[]
        for index,row in enumerate(data['objTypes']):
            target=source.resolve(owner,row['prefab'])
            prototype=add(target,'structure' if data['buildingspawn'] else 'prop') if target else None
            types.append({'prototype':prototype,**{k:v for k,v in row.items() if k!='prefab'}})
        generators.append({'source':source_id(owner),'types':types,**{k:data[k] for k in (
            'seedplus','noSpawnChance','pointDist','chunkDist','heightAdd','affectedByRoad','affectsTerrain','alignToBuilding','alignToRoad','buildingspawn','waitForTerrain')}})
        selected.record(owner)
    building_owner=source.get('level1',64750)
    buildings=required_record(source,building_owner,('groups','defaultSides','road','settings'))
    active_types=sorted({row['type'] for group in buildings['groups'] for row in group['types']})
    # The scene database singleton owns the source building type lookup.
    database=source.get('level1',WORLD_DATABASE_ID)
    db=required_record(source,database,tuple(BUILDING_FIELDS[k] for k in active_types))
    selected.record(database)
    building_types=[]
    for kind in active_types:
        entry=db[BUILDING_FIELDS[kind]]
        target=source.resolve(database,entry)
        building_types.append({'type_id':kind,'prototype':add(target,'structure')})
    selected.record(building_owner)
    catalog=recover_catalog(source,reader,selected,database)
    cars=car_prototypes(source,catalog)
    register_mirrors(source,cars)
    part_scope_slots=[]
    for node in source.hierarchy(_source_root(source,source.get('level1',17687))):
        for component in node['components']:
            if component.type.name=='MonoBehaviour' and source.class_name(component)=='partslotscript':
                selected.record(component)
                data=source.read(component)
                part_scope_slots.append({'source':source_id(component),'main_types':data['maintipus'],'part_types':data['tipus']})
    terrain_owner=source.get('level1',64900)
    terrain=required_record(source,terrain_owner,('TerrainPrefab','DistantTerrainPrefab','DistantTerrainPrefab2','manager'))
    terrain_prefabs=[]
    terrain_materials=[]
    terrain_source=copy.copy(source)
    terrain_source.lighting_textures={}
    for prefab,material in zip(('TerrainPrefab','DistantTerrainPrefab','DistantTerrainPrefab2'),('MaterialTerrain','MaterialDistantTerrain','MaterialDistant2Terrain')):
        target=source.resolve(terrain_owner,terrain[prefab])
        terrain_prefabs.append(selected.receipt(_source_root(source,target)))
        material_owner=source.resolve(settings_owner,settings[material])
        selected.record(material_owner)
        terrain_materials.append(source_material(source,selected,material_owner,output,terrain_source))
        for node in source.hierarchy(_source_root(source,target)):
            selected.record(source.resolve(node['reader'],source.read(node['reader'])['m_GameObject']))
            for component in node['components']:
                selected.record(component)
                if component.type.name=='MeshFilter' and source.read(component)['m_Mesh']['m_PathID']:
                    raise ValueError(f'{key(component)} terrain prefab requires source mesh export')
    main_owner=source.get('level1',64919)
    main=required_record(source,main_owner,('GSettings','magneticField'))
    # compassscript.GetNorthValue reads mainscript.magneticField.forward.
    magnetic_field=world_pose(source,source.resolve(main_owner,main['magneticField']))['rotation']
    selected.record(main_owner)
    for owner in (settings_owner,road_owner,terrain_owner,source.get('level1',64899)):
        selected.record(owner)
    # Retain all startup helper frames and rules through selected transforms.
    startup=[]
    for field in ('StartObjs','StartObjsFix','StartObjsDonTMoveOnLoad'):
        for pointer in settings[field]:
            root=source.resolve(settings_owner,pointer)
            startup.append({'group':field,'source':source_id(root),'pose':world_pose(source,root),'prototype':None,'start_car':False,'conditions':[]})
            selected.transform_chain(root)
    # Start car candidates. Startup frames ignore these groups; the vehicle binder spawns the picked one.
    starts=start_placements(source,settings_owner,settings,catalog,cars)
    for row in starts:selected.transform_chain(source.get(row['source']['file'],row['source']['path_id']))
    startup.extend(starts)
    start_roots={(r['source']['file'],r['source']['path_id']) for r in starts}
    for pid in (79970,68463,72657,77620):
        component=source.get('level1',pid)
        selected.record(component)
        node=_source_root(source,component)
        current=node
        ancestors=[]
        while current:
            ancestors.append(source_id(current));selected.record(current)
            current=source.resolve(current,source.read(current)['m_Father'])
        if not any(row['source'] in ancestors for row in startup):
            raise ValueError(f'level1:{pid} m_GameObject: helper is outside selected startup roots')
        for link in list(selected.records[key(component)]['links']):
            target=source.get(link['target']['file'],link['target']['path_id'])
            if target.type.name=='Transform':selected.record(target)
    # Include direct supported nested prefab links. Keep every rule even for excluded draws.
    excluded=[]
    required_categories=set()
    required_items=set(START_ONLY_ITEMS)
    def catalog_items(name,is_item):
        if is_item:
            matches=[i['index'] for i in catalog['items'] if i['name']==name]
            if not matches:raise ValueError(f'level1:66024 unknown item {name}')
            required_items.update(matches);return
        rows=[c for c in catalog['categories'] if c['name']==name]
        if not rows:raise ValueError(f'level1:66024 unknown category {name}')
        def visit(row):
            if row['index'] in required_categories:return
            required_categories.add(row['index'])
            required_items.update(row['items'] or [])
            for child in row['categories'] or []:
                if child<len(catalog['categories']):visit(catalog['categories'][child])
        visit(rows[0])
    scope_cache={}
    part_matches={}
    actors={'aiscript','newAiScript','nyulscript','vultureAI','movecreaturescript'}
    actions={'GunAim','GunShoot','bbgunscript','meleeweaponscript','weaponscript','weaponhandlingscript','wearable','ediblescript','survivalscript','cigiscript'}
    def scope(target):
        root=_source_root(source,target)
        if key(root) not in scope_cache:
            root_scripts=[c for c in source.components(root)[1] if c.type.name=='MonoBehaviour']
            root_classes={source.class_name(c) for c in root_scripts}
            if vehicle_exclusion(root,root_classes,cars):result=('other_vehicle',[])
            elif root_classes & actors:result=('actor_behavior',[])
            else:
                scripts=[c for n in source.hierarchy(root) for c in n['components'] if c.type.name=='MonoBehaviour']
                classes={source.class_name(c) for c in scripts}
                result=(vehicle_exclusion(root,classes,cars) or ('actor_behavior' if classes & actors else ''),selected_actions(source,scripts,actions))
                parts=[source.read(c) for c in root_scripts if source.class_name(c)=='partscript']
                part_matches[key(root)],excluded_actions=part_scope(parts,part_scope_slots,result[1])
                result=(result[0],excluded_actions)
            scope_cache[key(root)]=result
        return scope_cache[key(root)]
    def startup_hierarchy(root):
        result=[]
        def visit(node,parent,path,active):
            game_object,components=source.components(node)
            data=source.read(game_object)
            active=active and (bool(data['m_IsActive']) or parent is None)
            path=f"{path}/{data['m_Name']}" if path else data['m_Name']
            classes={source.class_name(c) for c in components if c.type.name=='MonoBehaviour'}
            selected.record(node)
            selected.record(game_object)
            if classes & (VEHICLE_CLASSES | actors):
                reason='startup_vehicle_owned_by_existing_scene' if key(node) in start_roots else 'unsupported_startup_vehicle_or_actor'
                excluded.append({'source':source_id(node),'field':'m_GameObject','target':selected.receipt(game_object),'reason':reason,'catalog':''})
                return
            result.append({'reader':node,'game_object':game_object,'components':components,'parent':parent,'path':path,'active':active})
            for child in source.read(node)['m_Children']:visit(source.resolve(node,child),node,path,active)
        visit(root,None,'',True)
        return result
    for identity in sorted(cars):add(source.get(*identity),'nested')
    scanned=set()
    startup_pending=[source.get(r['source']['file'],r['source']['path_id']) for r in startup if r['prototype'] is None and r['source']['path_id'] not in (21819,39843)]
    while startup_pending or set(roots)-scanned:
        startup_root=bool(startup_pending)
        root=startup_pending.pop(0) if startup_root else roots[next(k for k in roots if k not in scanned)]
        scanned.add(key(root))
        print(f'Selecting hierarchy {len(scanned)}: {key(root)}; {len(roots)} prototype roots',flush=True)
        for node in (startup_hierarchy(root) if startup_root else source.hierarchy(root,activate_root=True)):
            if startup_root:selected.record(node['reader'])
            for component in node['components']:
                if component.type.name in ('MeshRenderer','SkinnedMeshRenderer','MeshFilter','LODGroup','Light'):selected.record(component)
                if component.type.name!='MonoBehaviour':continue
                cname=source.class_name(component)
                if cname=='attachgripscript':
                    # attachgripscript.Start instantiates GripPrefab: the grip is its own prototype.
                    prefab=source.resolve(component,source.read(component)['GripPrefab'])
                    if prefab:add(prefab,'nested')
                if cname in RULE_CLASSES or key(component) in EDIBLE_COMPONENTS:
                    rule=selected.record(component)
                    selected.record(node['reader'])
                    for link in rule['links']:
                        target=source.get(link['target']['file'],link['target']['path_id'])
                        if target.type.name=='Transform':selected.record(target)
                    if cname in ('undergroundSpawnScript','spawnInBoxScript'):
                        for item in source.read(component)['spawn']:
                            catalog_items(item['category'],bool(item['itemNameInsteadOfCategory']))
                    if cname=='newRandomStuffSpawnScript':
                        for index,item in enumerate(source.read(component)['items']):
                            target=source.resolve(component,item['G'])
                            if target:
                                reason,excluded_actions=scope(target)
                                if reason:
                                    excluded.append({'source':source_id(component),'field':f'items[{index}].G','target':source_id(target),'reason':reason,'catalog':''})
                                else:add(target,'nested')
                            elif item['prefab']:
                                catalog_items(item['prefab'],bool(item['isitem']))
        for item_id in sorted(required_items):
            item=catalog['items'][item_id]
            if item['prototype'] is not None or item['exclusion']:continue
            target=source.get(item['source']['file'],item['source']['path_id'])
            reason,excluded_actions=scope(target)
            item['compatible_slots']=part_matches.get(key(_source_root(source,target)),[])
            item['exclusion']=reason
            item['excluded_actions']=excluded_actions
            if not reason:item['prototype']=add(target,'nested')
    for item in catalog['items']:
        if item['index'] not in required_items:item['exclusion']='outside_selected_pools'
    print(f'Selected {len(roots)} prototypes and {len(selected.records)} caller records',flush=True)
    source.world_prototype_roots=set(roots)
    from .world_biome_materials import scene_component, recover_sets, SETS_CLASS
    source.biome_material_sets=recover_sets(source,scene_component(source,reader,SETS_CLASS))
    source.biome_sets_used=set()
    source.world_material_source=copy.copy(source)
    source.world_material_source.lighting_textures={}
    source.world_material_export=lambda material:source_material(source,selected,material,output,source.world_material_source)
    prototypes=export_world_prototypes(source,reader,selected,output,roots,roles,scope_cache,first_asset)
    physics=next(owner for owner in source.objects.values() if owner.type.name=='PhysicsManager')
    selected.record(physics)
    collision_rows=source.read(physics)['m_LayerCollisionMatrix']
    if len(collision_rows)!=32:raise ValueError('Physics layer matrix must have 32 masks')
    collision_rules={'source':source_id(physics),'record_sha256':selected.receipts[key(physics)]['sha256'],
        'layers':collision_rows}
    startup_holes=[]
    for component_id,filter_id in ((68463,46406),(77620,49294)):
        component=source.get('level1',component_id);mesh_filter=source.get('level1',filter_id)
        selected.record(component);selected.record(mesh_filter)
        owner=source.read(mesh_filter);mesh=source.resolve(mesh_filter,owner['m_Mesh'])
        if source.read(component)['m_GameObject']!=owner['m_GameObject']:
            raise ValueError(f'level1:{component_id} startup hole owner mismatch')
        selected.receipt(mesh)
        doc,directory,mesh_index=reader.mesh(ObjectId(*key(mesh)))
        points=GltfExporter.rows(None,doc,directory,doc.meshes[mesh_index].primitives[0].attributes.POSITION)
        if len(points)<4:raise ValueError(f'{key(mesh)} startup hole needs four vertices')
        startup_holes.append({'component':source_id(component),'filter':source_id(mesh_filter),
            'mesh':source_id(mesh),'mesh_sha256':selected.receipts[key(mesh)]['sha256'],
            'vertices':[[x,y,-z] for x,y,z in points[:4]]})
    grass_displacement=recover_grass_displacement(source,selected,prototypes)
    lod=recover_lod(source,selected)
    road_fields=('length','roadBoneNum','roadBoneLength','roadSegmentLength','roadAboveTerrain','width','scale','XmaxAngle','YmaxAngle','startStraight','straightModifier','alignEdgeOfTheWorld','aliignRoad','alignBuildings','forceHomesick','noises','defPointRange','defPointStep','addPoint','buildingBoneNum')
    draw_settings=source.get('level0',16865)
    from .vehicles import vehicle_globals
    world={'library':provenance(reader.receipt),
        'spawn_context':SPAWN_CONTEXT,'vehicle_globals':vehicle_globals(source),'generation_frame':IDENTITY,'magnetic_field':magnetic_field,'electronics_cutoff':0.2,'draw_distance':draw_distance.record(selected.receipt(draw_settings),source.read(draw_settings)),'mirrors':mirrors.record(source,selected),'source_hashes':[],
        'road':{'source':source_id(road_owner),'prototypes':road_roots,**generation_counts(road),**{k:road[k] for k in road_fields}},
        'terrain':{'source':source_id(settings_owner),'prefabs':terrain_prefabs,'materials':terrain_materials,'textures':list(terrain_source.lighting_textures.values()),'game_settings':{'source':source_id(main_owner),'homesickMode':main['GSettings']['homesickMode']},
            'tiles':[{k:settings[t][k] for k in ('tileSize','resolution','drawDistance','YCoord')} for t in ('Terrain','DistantTerrain','DistantTerrain2')],
            **{k:settings[k] for k in ('edgeOfWorld','edgeOfWorldGradient','edgeOfWorldGenerationDist','edgeOfWorldWarpDist','edgeOfWorldWarpOffset','randomSeed','seed')}},
        'buildings':{'source':source_id(building_owner),'types':building_types,**{k:buildings[k] for k in ('groups','defaultSides','minStartDist','maxStartDist','distModifier')}},
        'part_scope_slots':part_scope_slots,'catalog':catalog,'generators':generators,'startup':startup,'startup_holes':startup_holes,'grass_displacement':grass_displacement,'lod':lod,'initial_materials':recover_initial_materials(source,selected),'collision_rules':collision_rules,'prototypes':prototypes,'records':list(selected.records.values()),
        'receipts':list(selected.receipts.values()),'exclusions':excluded,'material_textures':list(source.world_material_source.lighting_textures.values())}
    files={row['source']['file'] for row in world['receipts']}
    world['source_hashes']=[{'file':f,'sha256':reader.source_files[f'TheLongDrive_Data/{f}'].sha256} for f in sorted(files)]
    from .world_spawn import extend_spawn_closure
    extend_spawn_closure(source,reader,output,world)
    from .world_mechanisms import recover_world_mechanisms
    world['mechanisms'] = recover_world_mechanisms(source,selected,world)
    from .world_paintings import export_paintings
    from .world_biome_materials import export_sets
    world['biome_material_sets'] = export_sets(source.biome_material_sets,source.biome_sets_used,selected)
    marked=sum(n['biome_materials'] for p in world['prototypes'] for n in p['nodes'])
    print(f'Biome materials: {len(world["biome_material_sets"])} of {len(source.biome_material_sets)} sets used; {marked} marked nodes',flush=True)
    world['painting_materials'] = export_paintings(source,reader,selected,world,output,max(p['asset'] for p in world['prototypes'])+1)
    receipts={tuple(r['source'].values()):r for r in world['receipts']}
    receipts.update(selected.receipts)
    world['receipts']=list(receipts.values())
    records={tuple(r['source'].values()):r for r in world['records']}
    for identity,record in selected.records.items():
        if identity not in records:records[identity]=record
    world['records']=list(records.values())
    bind_tune_icons(source,world)
    bind_light_cookies(source,world,output)
    from .world_spawn import project_runtime_receipts
    from .world_fields import project_runtime_fields
    project_runtime_receipts(world)
    project_runtime_fields(world)
    return world

def bind_tune_icons(source, world):
    """mainscript.CrossHairStuff: GCMMB shows the img of a looked tuneAble usable or canTune cap."""
    for r in world['records']:
        flag={'usablescript':'tuneAble','tankcapscript':'canTune'}.get(r['class_name'])
        img=r['fields'].get('img') if flag else None
        if img and img['kind']=='link' and r['fields'][flag]['value']:
            source.crosshair_icons.add(tuple(img['value'].values()))

def bind_light_cookies(source, world, output):
    """Bind each world spot Light cookie to its lighting texture. The renderer packs these textures."""
    from .lighting import texture
    bindings={}
    for r in world['records']:
        cookie=r['fields'].get('m_Cookie') if r['class_name']=='Light' else None
        if not cookie or cookie['kind']!='link':continue
        identity=tuple(cookie['value'].values())
        if r['fields']['m_Type']['value']!=0:raise ValueError(f"{r['source']} world light cookie is not on a spot light")
        bindings[identity]={'source':cookie['value'],'texture':texture(source,source.get(*identity),output)}
    world['light_cookies']=[bindings[i] for i in sorted(bindings)]

def register_mirrors(source, cars):
    """Disabled mirrors of each car show the source noMirrorMaterial glass."""
    main=source.get('level1',64919)
    glass=source.resolve(main,source.read(main)['noMirrorMaterial'])
    source.mirror_renderers={}
    for identity in sorted(cars):
        for node in source.hierarchy(source.get(*identity),activate_root=True):
            for reader in node['components']:
                if reader.type.name=='MonoBehaviour' and source.class_name(reader)=='mirrorscript':
                    data=source.read(reader)['M']
                    if data['R']['m_PathID'] and data['C']['m_PathID']:
                        source.mirror_renderers[key(source.resolve(reader,data['R']))]=glass

def recover_catalog(source, reader, selected, database):
    """Read source initializer tables without retaining method bodies in runtime output."""
    import re
    from .library.reader import checked
    path=reader.records_root/'managed/Assembly-CSharp.cs'
    text=checked(reader.records_root,{'file':'managed/Assembly-CSharp.cs','bytes':path.stat().st_size,
        'sha256':'22ddc62b91954205d90ff513232f39becebbc58a71b718685075f91f64b1f3ee'}).decode('utf-8-sig').replace('\r\n','\n')
    start=text.index('public class itemdatabase ')
    text=text[start:text.index('public class jointHelpScript',start)]
    def body(marker):
        pos=text.index(marker);start=text.index('{',pos);depth=1;end=start+1
        while depth:
            depth += (text[end]=='{')-(text[end]=='}');end+=1
        return text[start+1:end-1]
    def names(marker):
        result=[s.strip() for s in body(marker).split(',') if s.strip()]
        if any(not re.fullmatch(r'\w+',n) for n in result):raise ValueError('level1:66024 enum format changed')
        return result
    item_names=names('public enum i\n')
    category_names=names('public enum k\n')
    fields=dict((int(index),field) for index,field in re.findall(r'items\[(\d+)\]\s*=\s*(\w+)\s*;',body('public void ItemInit()')))
    db=source.read(database)
    items=[]
    for i,name in enumerate(item_names):
        field=fields.get(i,'gerror')
        required_record(source,database,(field,))
        target=source.resolve(database,db[field])
        if target is None:raise ValueError(f'level1:66024 {field}: missing item target')
        items.append({'index':i,'name':name,'field':field,'compatible_slots':[],'source':selected.receipt(target),'prototype':None,'exclusion':'','excluded_actions':[]})
    categories=[]
    for index,args in re.findall(r'categories\[(\d+)\]\s*=\s*new category\((.*?)\);',body('public void CategoryInit()'),re.S):
        arrays=[]
        def array(match):
            values=[int(s.strip()) for s in match[2].split(',') if s.strip()]
            if len(values)!=int(match[1]):raise ValueError('level1:66024 category array length')
            arrays.append(values);return '@'+str(len(arrays)-1)
        args=re.sub(r'new int\[(\d+)\]\s*\{(.*?)\}',array,args,flags=re.S)
        args=[a.strip() for a in args.split(',')]
        def arr(arg):
            if not re.fullmatch(r'@\d+',arg):raise ValueError(f'level1:66024 category argument {arg}')
            return arrays[int(arg[1:])]
        row={'index':int(index),'name':category_names[int(index)],'items':None,'categories':None,'item_chances':None,'category_chances':None,'depend_on_car':False,'initialized':True}
        if args[1].startswith('kategoria:'):
            category=args[1].split(':')[1].strip()=='true'
            row['categories' if category else 'items']=arr(args[0])
            if len(args)==3:
                if args[2].startswith('pDependOnCar:'):row['depend_on_car']=args[2].split(':')[1].strip()=='true'
                else:row['category_chances']=arr(args[2])
            elif len(args)!=2:raise ValueError('level1:66024 category overload')
        else:
            row['items']=arr(args[0]);row['categories']=arr(args[1])
            if len(args)==4:row['item_chances']=arr(args[2]);row['category_chances']=arr(args[3])
            elif len(args)!=2:raise ValueError('level1:66024 category overload')
        categories.append(row)
    if len({c['index'] for c in categories})!=len(categories):raise ValueError('level1:66024 duplicate category initialization')
    for i,name in enumerate(category_names):
        if not any(c['index']==i for c in categories):
            categories.append({'index':i,'name':name,'items':None,'categories':None,'item_chances':None,'category_chances':None,'depend_on_car':False,'initialized':False})
    for row in categories:
        row['invalid_category_indices']=[i for i in row['categories'] or [] if i>=len(category_names)]
    return {'source':source_id(database),'items':items,'categories':sorted(categories,key=lambda c:c['index'])}

def home_rope(source):
    """The home buildingscript connectToPole ropeScript, its data and its root Transform."""
    home=source.get('level1',HOME_BUILDING)
    if source.class_name(home)!='buildingscript':raise ValueError(f'level1:{HOME_BUILDING} is not buildingscript')
    rope=source.resolve(home,source.read(home)['connectToPole'])
    if rope is None or source.class_name(rope)!='ropeScript':raise ValueError(f'level1:{HOME_BUILDING} connectToPole is not ropeScript')
    data=source.read(rope)
    return rope,data,source.transform_for(source.resolve(rope,data['m_GameObject']))

def prepare_home_cable(source):
    """The home rope root is inactive until it connects. Keep the meshes of its subtree."""
    _,_,root=home_rope(source)
    source.home_cable_geometry_nodes={key(n['reader']) for n in source.hierarchy(root,activate_root=True)}

def recover_home_cable(source, nodes):
    """Map the home buildingscript connectToPole rope to manifest node ids."""
    rope,data,root=home_rope(source)
    node_ids={(n['source']['file'],n['source']['path_id']):n['id'] for n in nodes}
    def node(transform, field):
        if transform is None or key(transform) not in node_ids:raise ValueError(f'{key(rope)} {field}: rope node is not exported')
        return node_ids[key(transform)]
    return {'root':node(root,'m_GameObject'),'ropes':[node(source.resolve(rope,p),f'ropes[{i}]') for i,p in enumerate(data['ropes'])]}


def recover_grass_displacement(source, selected, prototypes):
    """Retain the checked displacement camera and selected renderer writers."""
    camera=source.get('level1',40217);follow=source.get('level1',65072);writer=source.get('level1',65073)
    for owner in (camera,follow,writer):
        selected.records.pop(key(owner),None)
        selected.record(owner)
    c=required_record(source,camera,('m_GameObject','m_TargetTexture','orthographic','orthographic size','m_CullingMask'))
    f=required_record(source,follow,('follow','followHeight'));w=required_record(source,writer,('borderSmoothingArea',))
    game_object=source.resolve(camera,c['m_GameObject']);transform=source.transform_for(game_object)
    target=source.resolve(camera,c['m_TargetTexture']);follow_target=source.resolve(follow,f['follow'])
    for owner in (game_object,transform,target,follow_target):selected.record(owner)
    writers=[];seen=set();transforms=[source.get('level1',29482),source.get('level1',24842)]
    transforms.extend(source.get(node['source']['file'],node['source']['path_id']) for prototype in prototypes for node in prototype['nodes'] if node['layer']==18)
    for node in transforms:
        if key(node) in seen:continue
        seen.add(key(node));go=source.resolve(node,source.read(node)['m_GameObject']);data=source.read(go)
        components=[source.resolve(go,row['component']) for row in data['m_Component']]
        renderer=next((x for x in components if x.type.name=='MeshRenderer'),None)
        mesh_filter=next((x for x in components if x.type.name=='MeshFilter'),None)
        if renderer is None or mesh_filter is None:continue
        rd=source.read(renderer);mesh=source.resolve(mesh_filter,source.read(mesh_filter)['m_Mesh'])
        materials=[source.resolve(renderer,link) for link in rd['m_Materials']]
        for owner in (node,go,renderer,mesh_filter,*materials):
            selected.records.pop(key(owner),None);selected.record(owner)
        selected.receipt(mesh)
        writers.append({'transform':source_id(node),'owner':source_id(go),'renderer':source_id(renderer),
            'filter':source_id(mesh_filter),'mesh':source_id(mesh),'materials':[source_id(x) for x in materials],
            'layer':data['m_Layer'],'enabled':bool(rd['m_Enabled'])})
    return {'camera':source_id(camera),'follow':source_id(follow),'displacement':source_id(writer),
        'transform':source_id(transform),'follow_target':source_id(follow_target),'target':source_id(target),
        'follow_height':f['followHeight'],'border':w['borderSmoothingArea'],'writers':writers}

def recover_lod(source, selected):
    """Retain checked quality inputs and the statically recovered engine dither mask."""
    owner=source.get('globalgamemanagers',12)
    data=required_record(source,owner,('m_CurrentQuality','m_QualitySettings'))
    selected.record(owner)
    index=data['m_CurrentQuality'];profile=data['m_QualitySettings'][index]
    if profile['lodBias']!=1 or profile['maximumLODLevel']!=0:
        raise ValueError('Checked LOD quality context changed')
    return {'quality':source_id(owner),'profile':index,'lod_bias':profile['lodBias'],'maximum_lod':profile['maximumLODLevel'],
        'dither':{'engine_sha256':'553c1e7560e1b42bd4638c53d0168dcd1b4b9eb5b9df324b0a44f14b3f18bc86',
            'creator_rva':0x372b80,'creator_sha256':'397f40191129798e57cb4beaa1ba9d881f25c140987f656a6b24c7614dfbbd1d',
            'binding_rva':0x374580,'binding_sha256':'13b5c5a6eb2c0427b7a3181e1e4cce6678312f6e7523b04d14ad5b1fa30df7be',
            'property_slot':18,'property_name':'unity_DitherMask','alpha':[0,192,48,240,128,64,176,112,32,224,16,208,160,96,144,80]}}


def recover_initial_materials(source, selected):
    """Checked headlightscript.Start/RefreshLights applies MOFF to shared Lamp1 renderer."""
    controller=source.get('sharedassets1.assets',134437)
    data=required_record(source,controller,('MOFF','currentState','states'))
    renderer=source.get('sharedassets1.assets',99000)
    node=source.get('sharedassets1.assets',88898)
    material=source.resolve(controller,data['MOFF'])
    if data['currentState']!=0 or len(data['states'])!=3: raise ValueError('Changed checked Lamp1 initialization')
    for owner in (controller,renderer,node,material): selected.record(owner)
    return [{'controller':source_id(controller),'renderer':source_id(renderer),'node':source_id(node),'material':source_id(material)}]


def skin_record(source, exporter, component, node, bone_id):
    """SkinnedMeshRenderer vertex streams. `bone_id` maps a bone Transform to its owner id."""
    data=source.read(component)
    mesh=source.resolve(component,data['m_Mesh']);doc,directory,mesh_index=exporter.accepted_mesh(mesh)
    primitive=doc.meshes[mesh_index].primitives[0]
    return {'source':source_id(component),'mesh':source_id(mesh),'node':node,
        'bones':[bone_id(source.resolve(component,p)) for p in data['m_Bones']],
        'bind_poses':convert_bind_poses(source.read(mesh)['m_BindPose']),
        'positions':exporter.rows(doc,directory,primitive.attributes.POSITION),
        'normals':exporter.rows(doc,directory,primitive.attributes.NORMAL),
        'tangents':exporter.rows(doc,directory,primitive.attributes.TANGENT) if primitive.attributes.TANGENT is not None else [],
        'uv':exporter.rows(doc,directory,primitive.attributes.TEXCOORD_0),
        'joints':exporter.rows(doc,directory,primitive.attributes.JOINTS_0),
        'weights':exporter.rows(doc,directory,primitive.attributes.WEIGHTS_0),
        'triangles':exporter.geometry(mesh)[2]}


def recover_help_texts(source,hierarchy,mapping):
    """Source `helptextscript` nodes. fpscontroller shows the set of the box that the use ray hits."""
    result=[]
    for n in hierarchy:
        for c in n['components']:
            if c.type.name!='MonoBehaviour' or source.class_name(c)!='helptextscript':continue
            d=source.read(c)
            # isFix boxes do not join mainscript.helpTexts and keep an empty text.
            if d['isFix'] or not d['m_Enabled']:raise ValueError(f'{key(c)}: fixed or disabled help text is not ported')
            result.append({'source':source_id(c),'node':mapping[key(n['reader'])],'force_auto':bool(d['ForceAuto']),
                'motorcycle':bool(d['isMotorcycle']),'bike':bool(d['isBike']),'always':bool(d['showifmenudisabledtoo'])})
    return result


def export_world_prototypes(source,reader,selected,output,roots,roles,scope_cache,first_asset):
    prototypes=[]
    for index,(identity,root) in enumerate(roots.items(),first_asset):
        name=f'world-{identity[0].replace(".","-")}-{identity[1]}'
        exporter=GltfExporter(source,output/f'{name}.gltf',reader,index)
        report=exporter.export([root])
        from .world_biome_materials import check_exported, swaps
        check_exported(source.biome_material_sets,exporter.biome_sets,{tuple(m['source'].values()) for m in report['materials']+report['deferred_materials']},name)
        source.biome_sets_used.update(exporter.biome_sets)
        for material in report['materials']:
            selected.record(source.get(material['source']['file'],material['source']['path_id']))
            for texture in material['properties']['textures']:
                selected.receipt(source.get(texture['source']['file'],texture['source']['path_id']))
        for material in report['deferred_materials']:
            selected.record(source.get(material['source']['file'],material['source']['path_id']))
        hierarchy=source.hierarchy(root,activate_root=True)
        for node in hierarchy:
            selected.record(node['reader'])
            selected.record(node['game_object'])
        mapping={key(node['reader']):i for i,node in enumerate(hierarchy)}
        nodes=[{'source':r['source'],'name':source.read(n['game_object'])['m_Name'],'parent':mapping[key(n['parent'])] if n['parent'] else None,
                'active':bool(source.read(n['game_object'])['m_IsActive']),'renderer_enabled':bool(exporter.document.nodes[r['gltf_node']].extras.get('source_renderer_enabled',False)),'layer':source.read(n['game_object'])['m_Layer'],'transform':r['transform'],'gltf_node':r['gltf_node'],'biome_materials':swaps(source,n['components'])} for r,n in zip(report['nodes'],hierarchy)]
        local=copy.copy(source)
        ancestor_count=len(source.chain(root))
        local.chain=lambda transform: source.chain(transform)[:-ancestor_count]
        bodies=[];body_nodes={}
        centers=centers_of_mass(source,hierarchy)
        for n in hierarchy:
            for component in n['components']:
                if component.type.name!='Rigidbody':continue
                data=source.read(component);selected.record(component)
                body_nodes[key(n['reader'])]=len(bodies)
                bodies.append({'source':source_id(component),'node':mapping[key(n['reader'])],
                    'mass_kg':data['m_Mass'],'linear_damping':data['m_Drag'],'angular_damping':data['m_AngularDrag'],
                    'kinematic':bool(data['m_IsKinematic']),'use_gravity':bool(data['m_UseGravity']),
                    'constraints':data['m_Constraints'],'pose':world_pose(local,n['reader']),
                    'center_of_mass':centers.pop(key(component),None)})
        if centers:raise ValueError(f'{name}: setcenterofmass Rigidbody outside its prototype')
        skins=[];colliders=[];controls=[]
        for n in hierarchy:
            for component in n['components']:
                data=source.read(component)
                if component.type.name=='MonoBehaviour' and source.class_name(component)=='roadscript':
                    controls=[mapping[key(source.resolve(component,p))] for p in data['bones']]
                # Only road deformation reads skin streams. The glTF skin draws other skins.
                if component.type.name=='SkinnedMeshRenderer' and data['m_Mesh']['m_PathID'] and roles[identity]=='road':
                    skins.append(skin_record(source,exporter,component,mapping[key(n['reader'])],lambda bone:mapping[key(bone)]))
                if component.type.name in ('BoxCollider','MeshCollider','SphereCollider','CapsuleCollider'):
                    selected.record(component)
                    physical_material=source.resolve(component,data['m_Material'])
                    if physical_material:selected.record(physical_material)
                    # A road collider receives the deformed skin at runtime.
                    skin_collider=roles[identity]=='road' and component.type.name=='MeshCollider' and not data['m_Mesh']['m_PathID']
                    empty_mesh=None
                    if component.type.name=='MeshCollider' and data['m_Mesh']['m_PathID']:
                        collider_mesh=source.resolve(component,data['m_Mesh'])
                        if source.read(collider_mesh)['m_VertexData']['m_VertexCount']==0:
                            selected.record(collider_mesh);empty_mesh=source_id(collider_mesh)
                    owner_node=n['reader']
                    while key(owner_node) not in body_nodes and key(owner_node)!=key(root):
                        owner_node=source.resolve(owner_node,source.read(owner_node)['m_Father'])
                    body=body_nodes.get(key(owner_node))
                    body_pose=bodies[body]['pose'] if body is not None else IDENTITY
                    shape=None if skin_collider or empty_mesh else geometry(local,exporter,component,n['reader'],body_pose,body is not None and not bodies[body]['kinematic'])
                    role=recover_trigger_role(local,n['reader'],root,body_pose) if data['m_IsTrigger'] else None
                    colliders.append({'source':source_id(component),'node':mapping[key(n['reader'])],'enabled':bool(data['m_Enabled']),
                        'body':body,'trigger':bool(data['m_IsTrigger']),'material':selected.receipt(physical_material) if physical_material else None,'deformed_skin':0 if skin_collider else None,'source_empty_mesh':empty_mesh,'geometry':shape,'role':role,
                        'step_surface':step_surface(source,n['game_object']),'tag':collider_tag(source,n['game_object'])})
        sled=recover_sled(source,hierarchy,mapping,bodies,colliders)
        pushes=recover_world_pushes(local,hierarchy,mapping,bodies,colliders)
        from .vehicles import Prototype, light_records
        trailer=recover_trailer(source,hierarchy,mapping,bodies,lambda transform:world_pose(local,transform)['translation'],
            lambda:light_records(source,Prototype(source,hierarchy),None,None,index,report['material_indices'])[0])
        for node in hierarchy:
            for component in node['components']:
                if component.type.name=='MonoBehaviour' and source.class_name(component) in ('szankoScript','setcenterofmass','utanfutoscript','attachTargetscript','attachablescript','followspringscript','followavgposrot','followposrotscript'):selected.record(component)
                selected.component_record(component)
        prototype={'source':source_id(root),'role':roles[identity],'asset':index,'nodes':nodes,'controls':controls,
            'deferred_materials':report['deferred_materials'],'excluded_actions':scope_cache.get(identity,('',[]))[1],'skins':skins,'bodies':bodies,'colliders':colliders,'prefab_pose':converted_transform(source.read(root)),
            'asset_record':{'id':index,'mesh_count':report['mesh_count'],'material_count':report['material_count'],'usage':'world_prototype','path':f'reference/{name}.gltf','sha256':report['sha256'],'root':source_id(root),'dependencies':report['dependencies']},
            'materials':[dict(material,asset=index) for material in report['materials']],'sled':sled,'pushes':pushes,
            'trailer':trailer,'hitches':recover_hitches(source,hierarchy,mapping,colliders),'help_texts':recover_help_texts(source,hierarchy,mapping)}
        from .vehicles import vehicle_record, part_records
        vehicle=vehicle_record(source,hierarchy,bodies,colliders,index,report['material_indices'],output)
        if vehicle:prototype['vehicle']=vehicle
        prototype['followers']=recover_followers(source,hierarchy,mapping,vehicle,trailer)
        # Vehicle and sled records link source assets outside the hierarchy: particle materials,
        # shaders, plate fonts and clips. Each linked record needs its receipt.
        receipt_links(source,selected,[vehicle,sled])
        engines,radiators,gauges=part_records(source,hierarchy,index,report['material_indices'])
        if engines:prototype['engines']=engines
        if radiators:prototype['radiators']=radiators
        if gauges:prototype['gauges']=gauges
        prototypes.append(prototype)
        print(f'Exported {name}: {len(nodes)} nodes, {len(colliders)} colliders',flush=True)
    return prototypes

def receipt_links(source, selected, value):
    """Receipt every source identity ({file, path_id}) in `value`."""
    if isinstance(value,dict):
        if set(value)=={'file','path_id'}:selected.receipt(source.get(value['file'],value['path_id']))
        else:
            for child in value.values():receipt_links(source,selected,child)
    elif isinstance(value,list):
        for child in value:receipt_links(source,selected,child)

def generation_counts(road):
    result = {name: road[name] for name in ('genAtStart', 'genInOneFrame')}
    if any(type(value) is not int for value in result.values()) or result != {'genAtStart': 100, 'genInOneFrame': 1}:
        raise ValueError('level1:64771 unsupported generation counts')
    return result
