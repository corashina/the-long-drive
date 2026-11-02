"""Checked acquisition inputs. Runtime draws and publication belong to gameplay."""
import hashlib
import json
import re
from .source import key, source_id
from .gltf import converted_transform
from .survival import component, finite_record, managed_evidence
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


def _integer(value, maximum=2**32-1):
    if type(value) is not int or not 0 <= value <= maximum:
        raise ValueError('Acquisition unsigned integer differs')


def _number(value):
    import math
    if type(value) not in (int,float) or not math.isfinite(value) or abs(value)>3.4028234663852886e38:
        raise ValueError('Acquisition finite float differs')


def _boolean(value):
    if type(value) is not bool:raise ValueError('Acquisition boolean differs')


def _source(value):
    _shape(value,'file path_id')
    if value['file'] not in ('sharedassets1.assets','sharedassets0.assets','level1','level0'):
        raise ValueError('Acquisition source container differs')
    _integer(value['path_id'],2**63-1)
    if not value['path_id']:raise ValueError('Acquisition source identity is null')


def _list(value):
    if type(value) is not list:raise ValueError('Acquisition array differs')
    return value


def _numbers(value, length):
    if len(_list(value))!=length:raise ValueError('Acquisition numeric array length differs')
    for v in value:_number(v)


def _pose(value):
    _shape(value,'source parent coordinate_space transform');_source(value['source'])
    if value['parent'] is not None:_source(value['parent'])
    if value['coordinate_space']!='source_parent_local':raise ValueError('Acquisition pose space differs')
    _transform(value['transform'])
    if value not in CHECKED_POSES:raise ValueError('Acquisition source pose differs')


def _transform(t):
    _shape(t,'translation rotation scale')
    for f,n in [('translation',3),('rotation',4),('scale',3)]:_numbers(t[f],n)
    if any(x==0 for x in t['scale']) or abs(sum(x*x for x in t['rotation'])-1)>.002:
        raise ValueError('Acquisition transform differs')


def source_contract(value):
    if isinstance(value, dict):
        return {k:source_contract(v) for k,v in value.items() if k not in ('asset','root','node')}
    if isinstance(value, list):
        return [source_contract(v) for v in value]
    return value


def validate_acquisition(a, manifest):
    try:
        _validate_acquisition(a, manifest)
        content = json.dumps(source_contract(a), sort_keys=True, separators=(',', ':'), allow_nan=False).encode()
        digest = hashlib.sha256(content).hexdigest()
        if digest != CONTRACT_SHA256:
            raise ValueError(f"Acquisition differs from the checked source contract: {digest}")
    except (KeyError, TypeError, IndexError, OverflowError, StopIteration) as error:
        raise ValueError("Malformed acquisition contract") from error


def _validate_acquisition(a, manifest):
    _shape(a,'stations categories box packs startup_packs holiday_pumpkin holiday_startup pack_item_id cigarette_item_id evidence rules')
    finite_record(a)
    validate_rules(a['rules'])
    if type(a['pack_item_id']) is not int or a['pack_item_id']!=74 or type(a['cigarette_item_id']) is not int or a['cigarette_item_id']!=230:
        raise ValueError('Acquisition item identities differ')
    _source(a['holiday_pumpkin'])
    if a['holiday_pumpkin']!={'file':'level1','path_id':82}:raise ValueError('Acquisition holiday identity differs')
    nodes={(p['asset'],i):n for p in manifest['world']['prototypes'] for i,n in enumerate(p['nodes'])}
    prototypes={p['item_id']:p for p in manifest['item_prototypes'] if p['item_id'] is not None}
    by_root={(p['id']['file'],p['id']['path_id']):p for p in manifest['item_prototypes']}
    categories={c['id']:c for c in _list(a['categories'])}
    if len(categories)!=len(a['categories']):raise ValueError('Duplicate acquisition category')
    def prototype(identity,item=None):
        _source(identity)
        p=by_root.get((identity['file'],identity['path_id']))
        if p is None or (item is not None and p['item_id']!=item):raise ValueError('Acquisition prototype link differs')
        return p
    def marker(v,asset):
        _shape(v,'source node transform');_source(v['source']);_integer(v['node']);_transform(v['transform'])
        n=nodes.get((asset,v['node']),{})
        if n.get('source')!=v['source'] or n.get('transform')!=v['transform']:
            raise ValueError('Acquisition marker link or pose differs')
    def selection(s):
        if type(s) is not dict:raise ValueError('Acquisition selector differs')
        kind=s.get('kind')
        if kind=='category':
            _shape(s,'kind name category_id');_integer(s['category_id'])
            if type(s['name']) is not str or not s['name'] or s['category_id'] not in categories:
                raise ValueError('Acquisition category link differs')
        elif kind in ('item','prefab'):
            _shape(s,'kind name item_id prototype'+(' source' if kind=='prefab' else ''))
            if type(s['name']) is not str:raise ValueError('Acquisition selector name differs')
            if s['item_id'] is not None:_integer(s['item_id'])
            elif kind=='item':raise ValueError('Missing direct item ID')
            expected=prototypes.get(s['item_id'])
            if kind=='prefab':
                _source(s['source'])
                if s['source']['file']!='sharedassets1.assets' or SOURCE_PREFABS.get(s['source']['path_id'])!=(s['name'],s['item_id']):raise ValueError('Acquisition direct prefab source item link differs')
                expected_root=SUPPORTED_PREFABS.get((s['source']['file'],s['source']['path_id']))
                expected=by_root.get(expected_root) if expected_root else None
            if s['prototype'] is not None:_source(s['prototype'])
            if s['prototype']!=(expected['id'] if expected else None):raise ValueError('Acquisition selected prototype differs')
            if kind=='item' and ITEM_NAMES.get(s['name'])!=s['item_id']:raise ValueError('Acquisition item enum differs')
        else:raise ValueError('Acquisition selector operation differs')
    def table(entries):
        threshold=0.
        for row in _list(entries):
            _shape(row,'selection weight threshold');selection(row['selection'])
            _number(row['weight']);_number(row['threshold']);threshold+=row['weight']
            if row['weight']<=0 or row['threshold']!=threshold:raise ValueError('Acquisition table threshold differs')
        if not entries:raise ValueError('Empty acquisition table')
    for c in categories.values():
        _shape(c,'id children categories weights constructor_weights depend_on_car constructor_sha256 prototypes invalid_children')
        _integer(c['id']);_boolean(c['categories']);_boolean(c['depend_on_car'])
        for x in _list(c['children']):_integer(x)
        for x in _list(c['weights']):_integer(x)
        if c['constructor_weights'] is not None:
            for x in _list(c['constructor_weights']):_integer(x)
        if len(_list(c['prototypes']))!=len(c['children']):raise ValueError('Acquisition leaf prototype count differs')
        if not c['children'] or len(c['weights']) != len(c['children']):
            raise ValueError('Acquisition category weight count differs')
        expected = c['constructor_weights'] if c['categories'] and c['constructor_weights'] is not None else [1] * len(c['children'])
        if c['weights'] != expected or any(w == 0 for w in c['weights']):
            raise ValueError('Acquisition constructor weight semantics differ')
        invalid=[]
        for index,(child,pid) in enumerate(zip(c['children'],c['prototypes'],strict=True)):
            if pid is not None:_source(pid)
            if c['categories']:
                if pid is not None:raise ValueError('Category must not resolve as prototype')
                if child not in categories:
                    if child not in {12:(186,265,303,387),13:(187,266,304,386)}.get(c['id'],()):
                        raise ValueError('Missing acquisition category')
                    invalid.append({'ordinal':index,'target':child,'failure':'category_index_out_of_range'})
            elif pid!=(prototypes[child]['id'] if child in prototypes else None):
                raise ValueError('Acquisition leaf prototype differs')
        for v in _list(c['invalid_children']):
            _shape(v,'ordinal target failure');_integer(v['ordinal']);_integer(v['target'])
        if c['invalid_children']!=invalid:raise ValueError('Native invalid-category terminal differs')
    if len(categories) != 86 or list(categories) != sorted(categories):
        raise ValueError('Selected category closure or order differs')
    if len(_list(a['stations']))!=3:raise ValueError('Selected station count differs')
    for index,s in enumerate(a['stations']):
        station=next(p for p in manifest['world']['prototypes'] if p['asset']==s['asset'])
        _shape(s,'source building asset root item_spawn_distance building_chance initial_items_spawned to_start ordinary underground misc4 flyers')
        _source(s['source']);_source(s['building']);_integer(s['asset']);_integer(s['root'])
        if s['building']!={'file':'sharedassets1.assets','path_id':(158348,155446,156988)[index]}:raise ValueError('Station building owner differs')
        f=s['flyers'];_shape(f,'force halloween vertical_offset vertical_scale horizontal prefab supported')
        for field in ('force','halloween','supported'):_boolean(f[field])
        for field in ('vertical_offset','vertical_scale'):_number(f[field])
        _numbers(f['horizontal'],2);_source(f['prefab'])
        if f!=STATION_FLYERS[index]:raise ValueError('Station flyer contract differs')
        record=next(r for r in manifest['world']['records'] if r['source']==station['source'])
        owner=next(f['value'] for f in record['fields'] if f['name']=='m_GameObject')
        if s['source']!={'file':'sharedassets1.assets','path_id':STATION_PREFABS[index]} or s['root']!=0 or owner!={'kind':'link','value':s['source']}:
            raise ValueError('Acquisition station asset link differs')
        _boolean(s['initial_items_spawned']);_numbers(s['building_chance'],2)
        if s['initial_items_spawned'] or s['building_chance']!=[0.,0.] or s['to_start']!=[]:raise ValueError('Station startup differs')
        d=s['item_spawn_distance'];_shape(d,'source draw_distance item_distance authored_draw_distance');_source(d['source'])
        _numbers(d['draw_distance'],2);_numbers(d['item_distance'],2);_number(d['authored_draw_distance'])
        if d!={'source':{'file':'level1','path_id':65886},'draw_distance':[500.,25000.],'item_distance':[140.,340.],'authored_draw_distance':2500.}:
            raise ValueError('Station source distance differs')
        ordinary=_list(s['ordinary'])
        if len(ordinary) != (33,37,236)[index]:raise ValueError('Station spawner count differs')
        misc=[]
        for o in ordinary:
            _shape(o,'source marker dont_spawn holiday building_affects_this chance spawned entries')
            _source(o['source']);marker(o['marker'],s['asset'])
            for f in ('dont_spawn','building_affects_this','spawned'):_boolean(o[f])
            _integer(o['holiday']);_number(o['chance'])
            if o['holiday']>2 or not 0<=o['chance']<=1 or o['spawned']:raise ValueError('Ordinary spawn guard differs')
            threshold=0
            for e in _list(o['entries']):
                _shape(e,'selection weight threshold places random_rotation disable_random initialization');selection(e['selection'])
                _integer(e['weight']);_integer(e['threshold']);threshold+=e['weight'] or 1
                if e['threshold']!=threshold:raise ValueError('Ordinary item threshold differs')
                for f in ('random_rotation','disable_random'):_boolean(e[f])
                pt=0
                for place in _list(e['places']):
                    _shape(place,'weight threshold marker');_integer(place['weight']);_integer(place['threshold']);pt+=place['weight'] or 1
                    if pt!=place['threshold']:raise ValueError('Ordinary place threshold differs')
                    if place['marker'] is not None:marker(place['marker'],s['asset'])
                v=e['initialization'];_shape(v,'integer_range float_range override_fluid_amount fluid_range override_fluid_type fluids no_mixed_fluids override_mix_chance mix_chance')
                for f in ('integer_range','float_range','fluid_range'):_numbers(v[f],2)
                for x in v['integer_range']:_integer(x)
                for f in ('override_fluid_amount','override_fluid_type','no_mixed_fluids','override_mix_chance'):_boolean(v[f])
                _number(v['mix_chance'])
                for fluid in _list(v['fluids']):
                    _shape(fluid,'type weight authored_threshold');_integer(fluid['type']);_number(fluid['weight']);_number(fluid['authored_threshold'])
            if any(e['selection'].get('category_id')==35 for e in o['entries']):misc.append(o['source'])
        for identity in _list(s['misc4']):_source(identity)
        if s['misc4']!=misc or len(misc)!=(18,6,38)[index]:raise ValueError('misc4 subset differs')
        if len(_list(s['underground']))!=1:raise ValueError('Underground spawner count differs')
        u=s['underground'][0];_shape(u,'source center chance count radius vertical table');_source(u['source']);marker(u['center'],s['asset'])
        for f in ('count','radius','vertical'):_numbers(u[f],2)
        for x in u['count']:_integer(x)
        _number(u['chance'])
        if u['source']!={'file':'sharedassets1.assets','path_id':(154992,132529,157204)[index]} or u['count']!=[1,5] or u['radius']!=([3.,9.],[5.,11.],[13.,20.])[index] or u['vertical']!=[-.15000000596046448,-.4000000059604645] or u['chance']!=.4000000059604645:
            raise ValueError('Underground source bounds differ')
        table(u['table'])
    _validate_objects(a,manifest,prototype,table)


def _validate_objects(a,manifest,prototype,table):
    b=a['box'];_shape(b,'prototype source table disable_spawn spawn_before_open spawned opened spawn_position authored_spawn_position breakable save mount release disabled_starter_boxes content_counts')
    p=prototype(b['prototype'],62);behavior=p['behavior']
    for f in ('source','breakable','save'):_source(b[f])
    if b['source']!=behavior['spawn'] or b['breakable']!=behavior['breakable']:
        raise ValueError('Box behavior links differ')
    # The box breakables record holds its break rules, gibs and clips.
    record=[r for r in manifest['breakables'] if r['source']==b['breakable']]
    gibs=[{'file':'sharedassets1.assets','path_id':pid} for pid in (81642,81645)]
    if len(record)!=1 or [record[0][f] for f in ('health','no_trigger','no_gib','gib','reduced_gib')]!=[7.,True,False,*gibs]:
        raise ValueError('Box breakables record differs')
    table(b['table']);_pose(b['spawn_position'])
    if b['spawn_position']!=CHECKED_POSES[0]:raise ValueError('Box spawn pose owner differs')
    for f in ('disable_spawn','spawn_before_open','spawned','opened'):_boolean(b[f])
    if [b[f] for f in ('disable_spawn','spawn_before_open','spawned','opened')]!=[False,True,False,False] or b['authored_spawn_position'] is not None:
        raise ValueError('Box spawn guards differ')
    m=b['mount'];_shape(m,'source parent_save collider only_manual no_physics_lock dont_lock_if_picked_up')
    for f in ('source','parent_save','collider'):_source(m[f])
    for f in ('only_manual','no_physics_lock','dont_lock_if_picked_up'):_boolean(m[f])
    if m['source']!=behavior['mount'] or m['parent_save']!=b['save'] or not any(c['source']==m['collider'] for c in p['colliders']) or not m['only_manual'] or m['no_physics_lock'] or m['dont_lock_if_picked_up']:
        raise ValueError('Box content mount links differ')
    for e in _list(b['release']):
        _shape(e,'owner method target target_method');_source(e['owner']);_source(e['target'])
    if b['release']!=BOX_RELEASE:raise ValueError('Box release method or target differs')
    for c in _list(b['disabled_starter_boxes']):
        _shape(c,'source instance disable_spawn');_source(c['source']);_source(c['instance']);_boolean(c['disable_spawn'])
    if b['disabled_starter_boxes']!=DISABLED_BOXES:raise ValueError('Disabled starter Box source differs')
    for c in _list(b['content_counts']):
        _shape(c,'prototype source maximum');prototype(c['prototype']);_source(c['source']);_integer(c['maximum'])
    if b['content_counts']!=CONTENT_COUNTS:raise ValueError('Box content count rules differ')
    if len(_list(a['packs']))!=1:raise ValueError('Pack count differs')
    pack=a['packs'][0];_shape(pack,'prototype source ammo cigarette cigarette_prefab take_one_order wearable_type')
    p=prototype(pack['prototype'],74);prototype(pack['cigarette'],230)
    for f in ('source','cigarette_prefab'):_source(pack[f])
    if pack['source']!=p['behavior']['pack'] or pack['cigarette_prefab']!=p['behavior']['cigarette_prefab'] or pack['wearable_type']!=4 or type(pack['wearable_type']) is not int or pack['take_one_order']!=['decrement_integer_count','instantiate_at_pack_pose','initialize','wear_if_slot_empty']:
        raise ValueError('Pack Take one rules differ')
    def ammo(v,pid):
        _shape(v,'source count float_count maximum float_or_int type random_range');_source(v['source'])
        _integer(v['count']);_integer(v['type']);_number(v['float_count']);_number(v['maximum']);_boolean(v['float_or_int']);_numbers(v['random_range'],2)
        if v!={'source':{'file':'sharedassets1.assets' if pid==141757 else 'level1','path_id':pid},'count':20,'float_count':0.,'maximum':20.,'float_or_int':False,'type':5,'random_range':[20.,1.]}:
            raise ValueError('Source pack count or reversed range differs')
    ammo(pack['ammo'],141757)
    instance_fields='source pose active save item_id body pickup manual_start started loaded buried car fixed_in_car'
    def instance(v):
        for f in ('source','save','body','pickup'):_source(v[f])
        _pose(v['pose']);_integer(v['item_id']);_integer(v['fixed_in_car'])
        for f in ('active','manual_start','started','loaded','buried'):_boolean(v[f])
        if v['car'] is not None:_source(v['car'])
        if v['started'] or v['loaded'] or v['buried'] or v['car'] is not None or v['fixed_in_car']!=0:
            raise ValueError('Startup instance initialization differs')
    selected_packs=[(index,expected) for index,expected in enumerate(STARTUP_PACKS)
        if start_car(manifest,'StartCar01' if expected['source']==14079 else 'StartCar02')]
    if len(_list(a['startup_packs']))!=len(selected_packs):raise ValueError('Selected-car startup packs differ')
    for v,(index,expected) in zip(a['startup_packs'],selected_packs,strict=True):
        _shape(v,instance_fields+' prototype group group_pose group_active vehicle pack cigarette_prefab ammo');instance(v)
        prototype(v['prototype'],74);_pose(v['group_pose']);_boolean(v['group_active'])
        if [v['pose'],v['group_pose']]!=CHECKED_POSES[2+index*2:4+index*2]:raise ValueError('Startup pack pose owner differs')
        for f in ('group','vehicle','pack','cigarette_prefab'):_source(v[f])
        ammo(v['ammo'],expected['ammo'])
        for f in ('source','save','body','pickup','group','vehicle','pack'):
            if v[f]!={'file':'level1','path_id':expected[f]}:raise ValueError('Selected startup source link differs')
        if v['group_active'] or not v['active'] or not v['manual_start'] or v['item_id']!=74 or v['pose']['parent']!=v['group_pose']['source'] or v['cigarette_prefab']!=pack['cigarette_prefab']:
            raise ValueError('Selected startup group rule differs')
    h=a['holiday_startup'];_shape(h,instance_fields+' prototype wearable terrain start end inclusive local_calendar new_game_only multiplayer');instance(h)
    prototype(h['prototype'],92)
    if h!=HOLIDAY_STARTUP:raise ValueError('Holiday source startup fields differ')
    for f in ('wearable','terrain'):_source(h[f])
    for f in ('inclusive','local_calendar','new_game_only','multiplayer'):_boolean(h[f])
    for f in ('start','end'):
        if len(_list(h[f]))!=2:raise ValueError('Holiday date shape differs')
        for x in h[f]:_integer(x)
    if h['source']!=a['holiday_pumpkin'] or h['wearable']!={'file':'level1','path_id':79857} or h['terrain']!={'file':'level1','path_id':64898} or h['start']!=[10,15] or h['end']!=[11,15] or not h['inclusive'] or not h['local_calendar'] or not h['new_game_only'] or h['multiplayer']:
        raise ValueError('Holiday startup predicate differs')
    e=a['evidence'];_shape(e,'assembly_sha256 text_sha256 methods')
    for method in _list(e['methods']):_shape(method,'owner signature sha256')
    if e['assembly_sha256'] != ASSEMBLY_SHA256 or e['text_sha256'] != TEXT_SHA256:
        raise ValueError('Acquisition checked source differs')


# Checked build 11140890 identities and branches. Recovery must reproduce these records.

ITEM_NAMES = {'box01': 62}


SUPPORTED_PREFABS = {('sharedassets1.assets', 32825): ('sharedassets1.assets', 75443),
 ('sharedassets1.assets', 9085): ('sharedassets1.assets', 51722),
 ('sharedassets1.assets', 18963): ('sharedassets1.assets', 61587),
 ('sharedassets1.assets', 39185): ('sharedassets1.assets', 81642),
 ('sharedassets1.assets', 39188): ('sharedassets1.assets', 81645),
 ('sharedassets1.assets', 43845): ('sharedassets1.assets', 70812),
 ('sharedassets1.assets', 24481): ('sharedassets1.assets', 53838),
 ('sharedassets1.assets', 24480): ('sharedassets1.assets', 53837),
 ('sharedassets1.assets', 20981): ('sharedassets1.assets', 63605),
 ('sharedassets1.assets', 20649): ('sharedassets1.assets', 63259),
 ('sharedassets1.assets', 11985): ('sharedassets1.assets', 54495),
 ('sharedassets1.assets', 41097): ('sharedassets1.assets', 83430)}

BOX_RELEASE = [{'owner': {'file': 'sharedassets1.assets', 'path_id': 135113},
  'method': 'Break',
  'target': {'file': 'sharedassets1.assets', 'path_id': 145573},
  'target_method': 'PreDestroy'},
 {'owner': {'file': 'sharedassets1.assets', 'path_id': 145573},
  'method': 'PreDestroy',
  'target': {'file': 'sharedassets1.assets', 'path_id': 146589},
  'target_method': 'Unstore'}]

DISABLED_BOXES = [{'source': {'file': 'level1', 'path_id': 79792}, 'instance': {'file': 'level1', 'path_id': 1904}, 'disable_spawn': True},
 {'source': {'file': 'level1', 'path_id': 79793}, 'instance': {'file': 'level1', 'path_id': 1902}, 'disable_spawn': True},
 {'source': {'file': 'level1', 'path_id': 79794}, 'instance': {'file': 'level1', 'path_id': 1903}, 'disable_spawn': True},
 {'source': {'file': 'level1', 'path_id': 79795}, 'instance': {'file': 'level1', 'path_id': 1909}, 'disable_spawn': True},
 {'source': {'file': 'level1', 'path_id': 79796}, 'instance': {'file': 'level1', 'path_id': 1906}, 'disable_spawn': True},
 {'source': {'file': 'level1', 'path_id': 79797}, 'instance': {'file': 'level1', 'path_id': 1905}, 'disable_spawn': True},
 {'source': {'file': 'level1', 'path_id': 79798}, 'instance': {'file': 'level1', 'path_id': 1908}, 'disable_spawn': True},
 {'source': {'file': 'level1', 'path_id': 79799}, 'instance': {'file': 'level1', 'path_id': 1907}, 'disable_spawn': True}]

CONTENT_COUNTS = [{'prototype': {'file': 'sharedassets1.assets', 'path_id': 75443},
  'source': {'file': 'sharedassets1.assets', 'path_id': 132605},
  'maximum': 10},
 {'prototype': {'file': 'sharedassets1.assets', 'path_id': 51722},
  'source': {'file': 'sharedassets1.assets', 'path_id': 136544},
  'maximum': 0},
 {'prototype': {'file': 'sharedassets1.assets', 'path_id': 61587},
  'source': {'file': 'sharedassets1.assets', 'path_id': 156405},
  'maximum': 0},
 {'prototype': {'file': 'sharedassets1.assets', 'path_id': 70812},
  'source': {'file': 'sharedassets1.assets', 'path_id': 134682},
  'maximum': 5},
 {'prototype': {'file': 'sharedassets1.assets', 'path_id': 53838},
  'source': {'file': 'sharedassets1.assets', 'path_id': 147662},
  'maximum': 3},
 {'prototype': {'file': 'sharedassets1.assets', 'path_id': 53837},
  'source': {'file': 'sharedassets1.assets', 'path_id': 147661},
  'maximum': 3},
 {'prototype': {'file': 'sharedassets1.assets', 'path_id': 63605},
  'source': {'file': 'sharedassets1.assets', 'path_id': 134243},
  'maximum': 3},
 {'prototype': {'file': 'sharedassets1.assets', 'path_id': 63259},
  'source': {'file': 'sharedassets1.assets', 'path_id': 141432},
  'maximum': 0},
 {'prototype': {'file': 'sharedassets1.assets', 'path_id': 54495},
  'source': {'file': 'sharedassets1.assets', 'path_id': 134663},
  'maximum': 0},
 {'prototype': {'file': 'sharedassets1.assets', 'path_id': 51557},
  'source': {'file': 'sharedassets1.assets', 'path_id': 137709},
  'maximum': 0},
 {'prototype': {'file': 'sharedassets1.assets', 'path_id': 51878},
  'source': {'file': 'sharedassets1.assets', 'path_id': 132739},
  'maximum': 0}]

STARTUP_PACKS = [{'source': 14079, 'save': 72692, 'body': 50432, 'pickup': 75069, 'group': 298, 'vehicle': 69517, 'pack': 70704, 'ammo': 70998},
 {'source': 14082, 'save': 72693, 'body': 50434, 'pickup': 75072, 'group': 206, 'vehicle': 77839, 'pack': 70705, 'ammo': 70999}]


STATION_FLYERS = [{'force': False,
  'halloween': True,
  'horizontal': [6.0, 16.0],
  'prefab': {'file': 'sharedassets1.assets', 'path_id': 32394},
  'supported': True,
  'vertical_offset': 10.0,
  'vertical_scale': 5.0},
 {'force': False,
  'halloween': True,
  'horizontal': [6.0, 16.0],
  'prefab': {'file': 'sharedassets1.assets', 'path_id': 32394},
  'supported': True,
  'vertical_offset': 10.0,
  'vertical_scale': 5.0},
 {'force': False,
  'halloween': True,
  'horizontal': [6.0, 16.0],
  'prefab': {'file': 'sharedassets1.assets', 'path_id': 32394},
  'supported': True,
  'vertical_offset': 10.0,
  'vertical_scale': 5.0}]

# These records describe checked calls. They do not execute source control flow.


def recover_rules(code):
    database=_block(code,code.index('public class itemdatabase :'))
    # CarType has 23 source values; unassigned index17 retains the CLR zero value.
    car_map=[0]*23
    for i,v in re.findall(r'carTypeToCarID\[(\d+)\] = (\d+);',database):car_map[int(i)]=int(v)
    return {'integer_upper_exclusive':True,'float_upper_inclusive':True,
            'car_type_to_item_index':car_map}


def validate_rules(r):
    _shape(r,'integer_upper_exclusive float_upper_inclusive car_type_to_item_index')
    _boolean(r['integer_upper_exclusive']);_boolean(r['float_upper_inclusive'])
    if not r['integer_upper_exclusive'] or not r['float_upper_inclusive']:raise ValueError('Unity range endpoint contract differs')
    for v in _list(r['car_type_to_item_index']):_integer(v)
    if r['car_type_to_item_index']!=[1,1,15,98,29,94,105,122,140,153,173,178,222,243,257,275,309,0,337,356,371,390,396]:
        raise ValueError('Source car category index mapping differs')

SOURCE_PREFABS = {32825: ('', 74), 39986: ('', 54), 6496: ('', 71), 42570: ('', 60), 12981: ('', 86), 30814: ('', 72), 35111: ('', 76), 39989: ('', 57), 20979: ('', 75), 20649: ('', 92), 30548: ('', 254), 16468: ('', 80), 28749: ('', 66), 26645: ('', 85), 32753: ('', 83), 41097: ('', None), 9085: ('', 62), 9086: ('', 231), 24105: ('', 87), 26970: ('', 88), 35853: ('', 70), 36799: ('', 81), 35272: ('', 82), 7370: ('', 84)}


CHECKED_POSES = [{'coordinate_space': 'source_parent_local',
  'parent': None,
  'source': {'file': 'sharedassets1.assets', 'path_id': 51722},
  'transform': {'rotation': [-0.0, -0.0, 0.0, 1.0],
                'scale': [1.0, 1.0, 1.0],
                'translation': [0.0, 0.0, -0.0]}},
 {'coordinate_space': 'source_parent_local',
  'parent': {'file': 'level1', 'path_id': 21481},
  'source': {'file': 'level1', 'path_id': 21449},
  'transform': {'rotation': [-0.0, -1.0, 0.0, 0.0],
                'scale': [1.0, 1.0, 1.0],
                'translation': [1.4539967775344849, 6.00600004196167, 14.700002670288086]}},
 {'coordinate_space': 'source_parent_local',
  'parent': {'file': 'level1', 'path_id': 21655},
  'source': {'file': 'level1', 'path_id': 33859},
  'transform': {'rotation': [0.7058812379837036,
                             0.7058789730072021,
                             0.04163286089897156,
                             0.04163298010826111],
                'scale': [100.00006103515625, 100.00003051757812, 100.00003051757812],
                'translation': [-0.15660084784030914, -0.27971217036247253, -0.5606002807617188]}},
 {'coordinate_space': 'source_parent_local',
  'parent': {'file': 'level1', 'path_id': 21481},
  'source': {'file': 'level1', 'path_id': 21655},
  'transform': {'rotation': [0.0, 0.0, -0.7071056962013245, 0.7071078419685364],
                'scale': [1.0, 1.0, 1.0],
                'translation': [1.4890013933181763, 2.002000093460083, 11.198002815246582]}},
 {'coordinate_space': 'source_parent_local',
  'parent': {'file': 'level1', 'path_id': 21567},
  'source': {'file': 'level1', 'path_id': 33862},
  'transform': {'rotation': [0.0, 0.058877840638160706, -0.998265266418457, 9.126959810146218e-08],
                'scale': [100.00015258789062, 100.0001449584961, 100.0001220703125],
                'translation': [0.3570023775100708, 0.35050010681152344, -1.0168991088867188]}},
 {'coordinate_space': 'source_parent_local',
  'parent': {'file': 'level1', 'path_id': 21481},
  'source': {'file': 'level1', 'path_id': 21567},
  'transform': {'rotation': [0.0, -1.0, -0.0, 0.0],
                'scale': [1.0, 1.0, 1.0],
                'translation': [1.565999984741211, 1.8350000381469727, 9.581000328063965]}}]
HOLIDAY_STARTUP = {'active': True,
 'body': {'file': 'level1', 'path_id': 50268},
 'buried': False,
 'car': None,
 'end': [11, 15],
 'fixed_in_car': 0,
 'inclusive': True,
 'item_id': 92,
 'loaded': False,
 'local_calendar': True,
 'manual_start': True,
 'multiplayer': False,
 'new_game_only': True,
 'pickup': {'file': 'level1', 'path_id': 80624},
 'pose': {'coordinate_space': 'source_parent_local',
          'parent': {'file': 'level1', 'path_id': 21481},
          'source': {'file': 'level1', 'path_id': 21449},
          'transform': {'rotation': [-0.0, -1.0, 0.0, 0.0],
                        'scale': [1.0, 1.0, 1.0],
                        'translation': [1.4539967775344849, 6.00600004196167, 14.700002670288086]}},
 'prototype': {'file': 'sharedassets1.assets', 'path_id': 63259},
 'save': {'file': 'level1', 'path_id': 72331},
 'source': {'file': 'level1', 'path_id': 82},
 'start': [10, 15],
 'started': False,
 'terrain': {'file': 'level1', 'path_id': 64898},
 'wearable': {'file': 'level1', 'path_id': 79857}}
