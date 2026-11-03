"""Checked acquisition inputs. Runtime draws and publication belong to gameplay."""
import hashlib
import json
import re
from .source import key, source_id
from .gltf import converted_transform
from .survival import component, managed_evidence
STATION_PREFABS = (26573, 27344, 41123)

ASSEMBLY_SHA256 = '9919a9eeddcce23bb4ff86e890437f32b4a3ef3c10af2efe3b8d889d894dd62e'
TEXT_SHA256 = '22ddc62b91954205d90ff513232f39becebbc58a71b718685075f91f64b1f3ee'
# Selected Lada contract, recovered from the checked build 11140890 source receipt.
CONTRACT_SHA256 = 'a3379b3188ac06004d0792f239f3a0e633a1487df61e5a113403fd9d742be3e1'


def start_car(manifest, group):
    """Source DestroyOthers keeps the pack of a start car group that has a car placement."""
    return any(s['prototype'] is not None and s['group']==group for s in manifest['world']['startup'])


def _block(code, start):
    opening = code.index('{', start)
    depth, end = 1, opening + 1
    while depth:
        depth += (code[end] == '{') - (code[end] == '}')
        end += 1
    return code[start:end]


def _database(code):
    database = _block(code, code.index('public class itemdatabase :'))
    enums = {}
    for name in ('i', 'k'):
        match = re.search(r'public enum ' + name + r'\s*\{([^}]+)\}', database)
        entries = [v.strip() for v in match[1].split(',') if v.strip()]
        if any(not re.fullmatch(r'\w+', v) for v in entries):
            raise ValueError('Acquisition enum requires explicit recovery')
        enums[name] = {v: i for i, v in enumerate(entries)}
    categories = {}
    for match in re.finditer(r'categories\[(\d+)\] = new category\((.*?)\);', database, re.S):
        arrays = re.findall(r'new int\[\d+\]\s*\{([^}]+)\}', match[2])
        if len(arrays) not in (1, 2) or 'kategoria: ' not in match[2]:
            raise ValueError('Acquisition category constructor requires explicit recovery')
        values = [[int(v.strip()) for v in a.split(',') if v.strip()] for a in arrays]
        is_category = 'kategoria: true' in match[2]
        supplied = values[1] if len(values) == 2 else None
        categories[int(match[1])] = {
            'id': int(match[1]), 'children': values[0], 'categories': is_category,
            'weights': supplied if is_category and supplied is not None else [1] * len(values[0]),
            'constructor_weights': supplied, 'depend_on_car': 'pDependOnCar: true' in match[2],
            'constructor_sha256': hashlib.sha256(match[0].encode()).hexdigest(),
        }
    return enums, categories


def recover_station_graphs(source, manifest, code):
    enums, all_categories = _database(code)
    nodes = {}
    prototypes = {p['item_id']: p['id'] for p in manifest['item_prototypes'] if p['item_id'] is not None}
    prototype_roots = {(p['id']['file'], p['id']['path_id']): p for p in manifest['item_prototypes']}
    selected_categories = set()

    def marker(reader):
        if reader.type.name == 'GameObject':
            reader = source.transform_for(reader)
        elif reader.type.name not in ('Transform', 'RectTransform'):
            reader = source.transform_for(source.resolve(reader, source.read(reader)['m_GameObject']))
        n = nodes[key(reader)]
        return {'source': source_id(reader), 'node': n['id'], 'transform': converted_transform(source.read(reader))}

    def selection(owner, name, is_item, prefab=None):
        if prefab is not None:
            transform = source.transform_for(prefab)
            accepted = prototype_roots.get(key(transform))
            saves = [c for c in source.components(transform)[1]
                     if c.type.name == 'MonoBehaviour' and source.class_name(c) == 'tosaveitemscript']
            item_id = source.read(saves[0])['id'] if len(saves) == 1 else None
            return {'kind': 'prefab', 'name': name, 'source': source_id(prefab),
                    'item_id': item_id, 'prototype': accepted['id'] if accepted else None}
        if is_item:
            item_id = enums['i'][name]
            return {'kind': 'item', 'name': name, 'item_id': item_id, 'prototype': prototypes.get(item_id)}
        category_id = enums['k'][name]
        selected_categories.add(category_id)
        return {'kind': 'category', 'name': name, 'category_id': category_id}

    def table(reader, values):
        result, threshold = [], 0.
        for entry in values:
            threshold += entry['chance']
            result.append({'selection': selection(reader, entry['category'], entry['itemNameInsteadOfCategory']),
                           'weight': entry['chance'], 'threshold': threshold})
        return result

    stations = []
    main = source.get('level1', 64919)
    settings = source.get('level1', 65886)
    sd = source.read(settings)
    distance = {'source': source_id(settings), 'draw_distance': [sd['DDSmin']['drawDistance'], sd['DDSmax']['drawDistance']],
                'item_distance': [sd['DDSmin']['ItemSpawnDist'], sd['DDSmax']['ItemSpawnDist']],
                'authored_draw_distance': sd['S']['FDrawDistance']}
    for go_id in STATION_PREFABS:
        go = source.get('sharedassets1.assets', go_id)
        root = source.transform_for(go)
        building = component(source, root, 'buildingscript')
        bd = source.read(building)
        station = next(p for p in manifest['world']['prototypes'] if p['source'] == source_id(root))
        nodes = {(n['source']['file'],n['source']['path_id']): dict(n,id=i) for i,n in enumerate(station['nodes'])}
        ordinary, underground = [], []
        for record in source.hierarchy(root, activate_root=True):
            for reader in record['components']:
                if reader.type.name != 'MonoBehaviour':
                    continue
                kind = source.class_name(reader)
                if kind not in ('newRandomStuffSpawnScript', 'undergroundSpawnScript'):
                    continue
                d = source.read(reader)
                if kind == 'undergroundSpawnScript':
                    underground.append({'source': source_id(reader), 'center': marker(source.resolve(reader, d['center'])),
                        'chance': d['chance'], 'count': [d['minCount'], d['maxCount']],
                        'radius': [d['minRadius'], d['maxRadius']], 'vertical': [d['minOffsetty'], d['maxOffsetty']],
                        'table': table(reader, d['spawn'])})
                    continue
                entries, threshold = [], 0
                for item in d['items']:
                    threshold += item['chance'] or 1
                    places, place_threshold = [], 0
                    for place in item['places']:
                        place_threshold += place['chance'] or 1
                        target = source.resolve(reader, place['T'])
                        places.append({'weight': place['chance'], 'threshold': place_threshold,
                                       'marker': marker(target) if target else None})
                    entries.append({'selection': selection(reader, item['prefab'], item['isitem'], source.resolve(reader, item['G'])),
                        'weight': item['chance'], 'threshold': threshold, 'places': places,
                        'random_rotation': bool(item['randomRot']), 'disable_random': bool(item['disableRandom']),
                        'initialization': {'integer_range': [item['ir1'], item['ir2']], 'float_range': [item['fr1'], item['fr2']],
                            'override_fluid_amount': bool(item['overrideFluidAmount']), 'fluid_range': [item['minFluid'], item['maxFluid']],
                            'override_fluid_type': bool(item['overrideFluidType']), 'fluids': [
                                {'type': f['type'], 'weight': f['chance'], 'authored_threshold': f['chance2']} for f in item['fluids']],
                            'no_mixed_fluids': bool(item['noMixedFluids']), 'override_mix_chance': bool(item['overrideMixChance']),
                            'mix_chance': item['mixChance']}})
                ordinary.append({'source': source_id(reader), 'marker': marker(record['reader']),
                    'dont_spawn': bool(d['dontSpawn']), 'holiday': d['holiday'],
                    'building_affects_this': bool(d['buildingAffectsThis']), 'chance': d['chanceToSpawn'],
                    'spawned': bool(d['spawned']), 'entries': entries})
        stations.append({'source': source_id(go), 'building': source_id(building), 'asset': station['asset'],
            'root': 0, 'item_spawn_distance': distance,
            'building_chance': [bd['minRandomSpawn'], bd['maxRandomSpawn']],
            'initial_items_spawned': bool(bd['itemsSpawned']),
            'flyers': {'force':bool(bd['forceRandomFlyer']), 'halloween':bool(bd['randomFlyerOnHalloween']),
                'vertical_offset':bd['randomFlyerOffsetty'], 'vertical_scale':bd['randomFlyerVertical'],
                'horizontal':[bd['randomFlyerHorizontalMin'],bd['randomFlyerHorizontalMax']],
                'prefab':source_id(source.resolve(main,source.read(main)['randomFlyerPrefab'])), 'supported':True},
            'to_start': [source_id(source.resolve(building, p)) for p in bd['toStart']],
            'ordinary': ordinary, 'underground': underground,
            'misc4': [s['source'] for s in ordinary if any(i['selection'].get('category_id') == 35 for i in s['entries'])]})
    box = source.get('sharedassets1.assets', 158204)
    box_table = table(box, source.read(box)['spawn'])
    def include(cid):
        if cid not in all_categories:
            raise ValueError(f'Missing checked category {cid}')
        if cid in closure:
            return
        closure.add(cid)
        if all_categories[cid]['categories']:
            for child in all_categories[cid]['children']:
                if child not in all_categories:
                    expected = {12: (186,265,303,387), 13: (187,266,304,386)}
                    if child not in expected.get(cid, ()):
                        raise ValueError(f'Unreviewed native invalid category {cid}:{child}')
                else:
                    include(child)
    closure = set()
    for cid in sorted(selected_categories):
        include(cid)
    categories = [all_categories[cid] for cid in sorted(closure)]
    for c in categories:
        c['prototypes'] = [None if c['categories'] else prototypes.get(i) for i in c['children']]
        c['invalid_children'] = [{'ordinal':i, 'target':child, 'failure':'category_index_out_of_range'}
            for i,child in enumerate(c['children']) if c['categories'] and child not in all_categories]
    return stations, categories, box_table

METHODS = {
    'itemdatabase': ['public class category', 'private void Awake()'],
    'buildingscript': ['public void CheckDist()', 'public void SpawnStuff(int pId)', 'private IEnumerator SPAWN()', 'public static void RandomFlyerSpawn(bool _forceRandomFlyer, bool _randomFlyerOnHalloween, Vector3 _pos, float _offsetty, float _vertical, float _horizontalMin, float _horizontalMax)'],
    'newRandomStuffSpawnScript': ['public void Spawn()', 'public void InitSpawn()', 'public void SelectSpawn()', 'public void SpawnItem()', 'public static int SelectItem(string _categoryName, bool _isOneItem)', 'public static int selectId(int categoryId)'],
    'undergroundSpawnScript': ['private void Init()', 'public void Spawn()', 'public Vector3 RandomPoint()'],
    'spawnInBoxScript': ['public void FStart()', 'public void Spawn()', 'public void Opened()'],
    'breakablescript': ['private void Start()', 'private void OnCollisionEnter(Collision c)', 'public void TryBreak(float force)', 'public void Break()'],
    'mountStuff': ['public void Unstore()'],
    'tosaveitemscript': ['public void Bury()', 'public void UnBury()', 'public bool IsRecentlyUnburied()', 'public void UpdBuried(float _shovelSpeed)', 'public bool IsBuried()', 'public bool IsBuriedRay()', 'public void PreDestroy()', 'public void RandomSpawned(newRandomStuffSpawnScript.item _item)', 'public void RandomSpawned()'],
    'pickupable': ['public bool CanPickup()'],
    'meleeweaponscript': ['private void Update()'],
    'weaponhandlingscript': ['private void Update()', 'public void AttackStart()', 'public void AttackEnd()', 'private void StartAttack(meleeweaponscript.attackType tipus)'],
    'animationEventScript': ['public override void OnStateEnter(Animator animator, AnimatorStateInfo stateInfo, int layerIndex)', 'public override void OnStateExit(Animator animator, AnimatorStateInfo stateInfo, int layerIndex)'],
    'fpscontroller': ['public void usePickedUpStuff(pickupable pP, bool hand)'],
    'TerrainGenerationSettings': ['public void FStart()', 'public void DestroyOthers(GameObject selectedCar)', 'public void CheckCar(GameObject selectedCar, GameObject other)'],
    'mainscript': ['public static bool CheckHoliday(holiday _h)'],
    'settingsscript': ['private void RefreshInMain(bool _changeRes)'],
}


def recover_methods(source, code):
    managed_evidence(source, ['return selectId(itemdatabase.d.categories[categoryId].categories[num9]);'])
    raw = (source.library.records_root/'managed/Assembly-CSharp.cs').read_bytes()
    if hashlib.sha256(raw).hexdigest() != TEXT_SHA256:
        raise ValueError('Acquisition decompilation differs')
    methods = []
    for owner, signatures in METHODS.items():
        owner_text = _block(code, code.index('public class '+owner+' :'))
        for signature in signatures:
            if signature not in owner_text:
                raise ValueError(f'Missing acquisition method {owner}.{signature}')
            text = _block(owner_text, owner_text.index(signature))
            methods.append({'owner':owner, 'signature':signature, 'sha256':hashlib.sha256(text.encode()).hexdigest()})
    return {'assembly_sha256':ASSEMBLY_SHA256, 'text_sha256':TEXT_SHA256, 'methods':methods}


def recover_acquisition(source, manifest):
    code = (source.library.records_root/'managed/Assembly-CSharp.cs').read_text(encoding='utf-8-sig')
    evidence = recover_methods(source, code)
    stations, categories, box_table = recover_station_graphs(source, manifest, code)
    prototypes = {p['item_id']:p for p in manifest['item_prototypes'] if p['item_id'] is not None}
    nodes = {(n['source']['file'],n['source']['path_id']):n for n in manifest['nodes']}
    def ptr(reader, d, field):
        target = source.resolve(reader,d[field])
        return source_id(target) if target else None
    def pose(reader):
        if reader.type.name=='GameObject':reader=source.transform_for(reader)
        d = source.read(reader)
        return {'source':source_id(reader), 'parent':ptr(reader,d,'m_Father'),
                'coordinate_space':'source_parent_local', 'transform':converted_transform(d)}
    def ammo(reader):
        d = source.read(reader)
        return {'source':source_id(reader), 'count':d['ammo'], 'float_count':d['fammo'], 'maximum':d['max'],
                'float_or_int':bool(d['floatOrInt']), 'type':d['tipus'], 'random_range':[d['ammoRmin'],d['ammoRmax']]}
    def startup(go):
        root = source.transform_for(go)
        save = component(source,root,'tosaveitemscript');d=source.read(save)
        pickup = component(source,root,'pickupable');pd=source.read(pickup)
        return {'source':source_id(go), 'pose':pose(root), 'active':bool(source.read(go)['m_IsActive']),
                'save':source_id(save), 'item_id':d['id'], 'body':ptr(save,d,'RB'), 'pickup':source_id(pickup),
                'manual_start':bool(d['manualStart']), 'started':bool(d['started']), 'loaded':bool(d['loaded']),
                'buried':bool(d['buried']), 'car':ptr(save,d,'car'), 'fixed_in_car':pd['fixedInCar']}
    boxp = prototypes[62]
    spawn=source.get('sharedassets1.assets',158204);sd=source.read(spawn)
    broken=source.get('sharedassets1.assets',135113)
    mount=source.resolve(spawn,sd['mount']);md=source.read(mount)
    box_root=source.get('sharedassets1.assets',51722)
    save=component(source,box_root,'tosaveitemscript');saved=source.read(save)
    box={'prototype':boxp['id'], 'source':source_id(spawn), 'table':box_table,
        'disable_spawn':bool(sd['disableSpawn']), 'spawn_before_open':bool(sd['spawnBeforeOpen']),
        'spawned':bool(sd['spawned']), 'opened':bool(sd['opened']), 'spawn_position':pose(source.resolve(spawn,sd['spawnPos']) or box_root),
        'authored_spawn_position':ptr(spawn,sd,'spawnPos'), 'breakable':source_id(broken), 'save':source_id(save),
        'mount':{'source':source_id(mount),'parent_save':ptr(mount,md,'parentTosave'),'collider':ptr(mount,md,'col'),
                 'only_manual':bool(md['onlyManual']),'no_physics_lock':bool(md['noPhysicsLock']),
                 'dont_lock_if_picked_up':bool(md['dontLockIfPickedUp'])},
        'release':[{'owner':source_id(broken),'method':'Break','target':source_id(save),'target_method':'PreDestroy'},
                   {'owner':source_id(save),'method':'PreDestroy','target':source_id(mount),'target_method':'Unstore'}],
        'disabled_starter_boxes':[], 'content_counts':[]}
    if saved['physlocks'] != [sd['mount']]:raise ValueError('Box release mount link differs')
    for pid in range(79792,79800):
        r=source.get('level1',pid);d=source.read(r);go=source.resolve(r,d['m_GameObject'])
        box['disabled_starter_boxes'].append({'source':source_id(r),'instance':source_id(go),'disable_spawn':bool(d['disableSpawn'])})
    for p in prototypes.values():
        # Spawn reads maxCountIfSpawnedInBox only when the item has a massScript.
        root=source.get(p['id']['file'],p['id']['path_id'])
        if not any(c.type.name=='MonoBehaviour' and source.class_name(c)=='massScript' for c in source.components(root)[1]):continue
        r=component(source,root,'massScript')
        box['content_counts'].append({'prototype':p['id'],'source':source_id(r),'maximum':source.read(r)['maxCountIfSpawnedInBox']})
    packp=prototypes[74];pack=source.get('sharedassets1.assets',141307)
    packs=[{'prototype':packp['id'], 'source':source_id(pack),'ammo':ammo(source.get('sharedassets1.assets',141757)),
            'cigarette':prototypes[230]['id'], 'cigarette_prefab':ptr(pack,source.read(pack),'prefab'),
            'take_one_order':['decrement_integer_count','instantiate_at_pack_pose','initialize','wear_if_slot_empty'],
            'wearable_type':4}]
    terrain=source.get('level1',64898);td=source.read(terrain)
    startup_packs=[]
    for field,pack_id,car_id in [('StartCar01',14079,69517),('StartCar02',14082,77839)]:
        if not start_car(manifest,field):continue
        group=source.resolve(terrain,td[field]);go=source.get('level1',pack_id)
        s=startup(go);root=source.transform_for(go)
        cigi=component(source,root,'cigiscript')
        s.update(prototype=packp['id'], group=source_id(group), group_pose=pose(group),
                 group_active=bool(source.read(group)['m_IsActive']),vehicle={'file':'level1','path_id':car_id},
                 pack=source_id(cigi),cigarette_prefab=ptr(cigi,source.read(cigi),'prefab'),
                 ammo=ammo(component(source,root,'ammoscript')))
        startup_packs.append(s)
    pumpkin=source.get('level1',82);holiday=startup(pumpkin)
    holiday.update(prototype=prototypes[92]['id'], wearable={'file':'level1','path_id':79857}, terrain=source_id(terrain),
                   start=[10,15],end=[11,15],inclusive=True,local_calendar=True,new_game_only=True,multiplayer=False)
    return {'stations':stations,'categories':categories,'box':box,'packs':packs,
        'startup_packs':startup_packs,'holiday_pumpkin':source_id(pumpkin),'holiday_startup':holiday,
        'pack_item_id':74,'cigarette_item_id':230,'evidence':evidence, 'rules':recover_rules(code)}


def _shape(value, fields):
    if type(value) is not dict or set(value) != set(fields.split()):
        raise ValueError('Acquisition record fields differ')


def _number(value):
    import math
    if type(value) not in (int,float) or not math.isfinite(value) or abs(value)>3.4028234663852886e38:
        raise ValueError('Acquisition finite float differs')


def _list(value):
    if type(value) is not list:raise ValueError('Acquisition array differs')
    return value


def source_contract(value):
    if isinstance(value, dict):
        return {k:source_contract(v) for k,v in value.items() if k not in ('asset','root','node')}
    if isinstance(value, list):
        return [source_contract(v) for v in value]
    return value


def validate_acquisition(a, manifest):
    try:
        content = json.dumps(source_contract(a), sort_keys=True, separators=(',', ':'), allow_nan=False).encode()
        digest = hashlib.sha256(content).hexdigest()
        if digest != CONTRACT_SHA256:
            raise ValueError(f"Acquisition differs from the checked source contract: {digest}")
    except (KeyError, TypeError, IndexError, OverflowError, StopIteration) as error:
        raise ValueError("Malformed acquisition contract") from error


# Checked build 11140890 identities and branches. Recovery must reproduce these records.


# These records describe checked calls. They do not execute source control flow.


def recover_rules(code):
    database=_block(code,code.index('public class itemdatabase :'))
    # CarType has 23 source values; unassigned index17 retains the CLR zero value.
    car_map=[0]*23
    for i,v in re.findall(r'carTypeToCarID\[(\d+)\] = (\d+);',database):car_map[int(i)]=int(v)
    return {'integer_upper_exclusive':True,'float_upper_inclusive':True,
            'car_type_to_item_index':car_map}


