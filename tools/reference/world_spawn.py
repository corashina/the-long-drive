"""Retain nested caller inputs and export only missing supported pool prototypes."""
import copy
from .source import key, source_id
from .world_fields import PROVENANCE_ROOT
from .world import Selection, RULE_CLASSES, vehicle_exclusion, car_prototypes, EDIBLE_COMPONENTS, selected_actions, part_scope, _source_root, source_material, export_world_prototypes


def extend_spawn_closure(source, reader, output, world):
    selected=Selection(source,reader)
    selected.records={tuple(r['source'].values()):r for r in world['records']}
    selected.receipts={tuple(r['source'].values()):r for r in world['receipts']}
    baseline=set(selected.records)
    catalog=world['catalog']; categories=catalog['categories']; items=catalog['items']
    cars=car_prototypes(source,catalog)
    prototypes={tuple(p['source'].values()):p for p in world['prototypes']}
    pending=[]; queued=set(); processed=set(); new_roots={}; scopes={}; required=set()
    item_roots={i['index']:_source_root(source,source.get(i['source']['file'],i['source']['path_id'])) for i in items}
    items_by_root={}
    for i in items:items_by_root.setdefault(key(item_roots[i['index']]),[]).append(i)
    actors={'aiscript','newAiScript','nyulscript','vultureAI','movecreaturescript'}
    actions={'GunAim','GunShoot','bbgunscript','meleeweaponscript','weaponscript','weaponhandlingscript','wearable','ediblescript','survivalscript','cigiscript'}
    def add(target):
        root=_source_root(source,target)
        if key(root) not in queued:queued.add(key(root));pending.append(root)
        return root
    seen_categories=set()
    def category(index):
        if index in seen_categories or index>=len(categories):return
        seen_categories.add(index);row=categories[index]
        for i in row['items'] or []:item(i)
        for i in row['categories'] or []:category(i)
    def named(name,is_item):
        rows=items if is_item else categories
        row=next((r for r in rows if r['name']==name),None)
        if row is None:raise ValueError(f'Missing source spawn name {name}')
        (item if is_item else category)(row['index'])
    def item(index):
        if index in required:return
        required.add(index);row=items[index]
        add(item_roots[index])
    for p in world['prototypes']:add(source.get(p['source']['file'],p['source']['path_id']))
    for i in items:
        if i['exclusion']!='outside_selected_pools':item(i['index'])
    for e in world['exclusions']:
        if e['field'].startswith('items['):add(source.get(e['target']['file'],e['target']['path_id']))
    main=source.get('level1',64919)
    flyer=source.resolve(main,source.read(main)['randomFlyerPrefab'])
    if flyer:add(flyer)
    for r in list(selected.records.values()):
        if r['class_name'] in RULE_CLASSES and len(r['fields'])==1:
            selected.records.pop(tuple(r['source'].values()));selected.record(source.get(r['source']['file'],r['source']['path_id']))
    while pending:
        root=pending.pop(0);identity=key(root)
        if identity in processed:continue
        processed.add(identity)
        if len(processed)%25==0:print(f'Retaining spawn rule root {len(processed)}; queued {len(queued)}',flush=True)
        hierarchy=source.hierarchy(root,activate_root=True)
        scripts=[c for n in hierarchy for c in n['components'] if c.type.name=='MonoBehaviour']
        classes={source.class_name(c) for c in scripts}
        reason=vehicle_exclusion(root,classes,cars) or ('actor_behavior' if classes & actors else '')
        excluded_actions=selected_actions(source,scripts,actions)
        root_parts=[source.read(c) for c in hierarchy[0]['components'] if c.type.name=='MonoBehaviour' and source.class_name(c)=='partscript']
        matches,excluded_actions=part_scope(root_parts,world['part_scope_slots'],excluded_actions)
        scopes[identity]=(reason,excluded_actions)
        for row in items_by_root.get(identity,[]):
            if row['index'] in required:
                row.update(exclusion=reason,excluded_actions=excluded_actions,compatible_slots=matches,prototype=None if reason else source_id(root))
        if not reason and identity not in prototypes:new_roots[identity]=root
        if reason:
            initialization={'tosaveitemscript','newAiScript','nyulscript','partconditionscript','partslotscript','partscript','randomTypeSelector','randomScaleScript','spawnInBoxScript','paintingScript','paintingMain','newRandomStuffSpawnScript','ammoscript','foodscript','tankscript','massScript','childunparent','sprayscript','flashlightscript'}
            consumed={
                'tosaveitemscript': {'m_GameObject','m_Script','m_Enabled','started','partslotscripts','randomScales','painting','spawnBox','ammo','tanks','randomizetanks','food','engine','partconditions','manualStart','scaleDestroyOnStart','scaleDestroyOnStartIfReducedGibs'},
                'newAiScript': {'m_GameObject','m_Script','m_Enabled','sleeping','startSleepChange','voiceMinPitch','voiceMaxPitch','startPatrolTimeMin','startPatrolTimeMax'},
                'flashlightscript': {'m_GameObject','m_Script','m_Enabled','ON','randomStart'},
                'nyulscript': {'m_GameObject','m_Script','m_Enabled'},
                'partscript': {'m_GameObject','m_Script','tosaveitem','localRotationAfterMount','T'},
                'massScript': {'m_GameObject','m_Script','maxCountIfSpawnedInBox'},
                'GameObject': {'m_Component','m_IsActive','m_Name','m_Layer'},
                'Transform': {'m_GameObject','m_Father','m_Children','m_LocalPosition','m_LocalRotation','m_LocalScale'},
            }
            retained={key(c) for n in hierarchy for c in n['components'] if c.type.name in ('Transform','RectTransform') or (c.type.name=='MonoBehaviour' and source.class_name(c) in initialization)}
            for n in hierarchy:
                for c in n['components']:
                    if key(c) not in retained:selected.records.pop(key(c),None)
            for n in hierarchy:
                for owner in (n['reader'],n['game_object']):
                    record=selected.record(owner)
                    fields=consumed['Transform' if owner.type.name=='RectTransform' else owner.type.name]
                    record['fields']={k:v for k,v in record['fields'].items() if k in fields}
                    record['links']=[l for l in record['links'] if l['field'].split('[')[0].split('.')[0] in fields]
                    if owner.type.name=='GameObject':
                        record['links']=[l for l in record['links'] if not l['field'].startswith('m_Component[') or tuple(l['target'].values()) in retained]
                        record['fields']['m_Component']['value']=[v for v in record['fields']['m_Component']['value'] if tuple(v['value']['component']['value'].values()) in retained]
            for c in scripts:
                cname=source.class_name(c)
                if cname in consumed:
                    if key(c) in selected.records:selected.records.pop(key(c))
                    record=selected.record(c);fields=consumed[cname]
                    record['fields']={k:v for k,v in record['fields'].items() if k in fields}
                    record['links']=[l for l in record['links'] if l['field'].split('[')[0].split('.')[0] in fields]
        for node in hierarchy:
            selected.record(node['reader']);selected.record(node['game_object'])
            for component in node['components']:
                if component.type.name!='MonoBehaviour':
                    if not reason:selected.component_record(component)
                    continue
                cname=source.class_name(component)
                if (cname in RULE_CLASSES or key(component) in EDIBLE_COMPONENTS) and (not reason or cname in initialization):
                    if key(component) in selected.records and len(selected.records[key(component)]['fields'])==1:selected.records.pop(key(component))
                    selected.record(component);data=source.read(component)
                    script=source.resolve(component,data['m_Script'])
                    if script:selected.record(script)
                    if cname=='buildingscript':
                        ramp=source.resolve(component,data['rampPrefab'])
                        if ramp:add(ramp)
                    if cname=='newRandomStuffSpawnScript':
                        for row in data['items']:
                            target=source.resolve(component,row['G'])
                            if target:add(target)
                            elif row['prefab']:named(row['prefab'],bool(row['isitem']))
                    if cname=='partslotscript' and data['enableSpawn'] and not data['part']['m_PathID']:
                        for row in data['randomParts']:item(row['id'])
                        item(data['spawn_def']);item(data['selectedID'])
                    if cname in ('undergroundSpawnScript','spawnInBoxScript'):
                        for row in data['spawn']:named(row['category'],bool(row['itemNameInsteadOfCategory']))
                    for field in ('painting','spawnBox','randomScales','partconditions','partslotscripts'):
                        if field not in data:continue
                        pointers=data[field] if isinstance(data[field],list) else [data[field]]
                        for pointer in pointers:
                            if isinstance(pointer,dict) and pointer.get('m_PathID'):
                                target=source.resolve(component,pointer)
                                if key(target) in selected.records and len(selected.records[key(target)]['fields'])==1:selected.records.pop(key(target))
                                selected.record(target)
                elif not reason or cname=='sprayscript':selected.component_record(component)
        if reason:
            nodes={key(n['reader']):n for n in hierarchy}
            by_object={key(n['game_object']):key(n['reader']) for n in hierarchy}
            keep={identity}
            for node in hierarchy:
                if any(c.type.name=='MonoBehaviour' and source.class_name(c) in initialization for c in node['components']):keep.add(key(node['reader']))
            for node in hierarchy:
                for c in node['components']:
                    record=selected.records.get(key(c))
                    if record and c.type.name=='MonoBehaviour' and source.class_name(c) in initialization:
                        for link in record['links']:
                            target=tuple(link['target'].values())
                            if target in nodes:keep.add(target)
                            elif target in by_object:keep.add(by_object[target])
            for target in list(keep):
                current=target
                while current in nodes:
                    keep.add(current)
                    parent=source.resolve(nodes[current]['reader'],source.read(nodes[current]['reader'])['m_Father'])
                    if parent is None:break
                    current=key(parent)
            for nid,node in nodes.items():
                if nid not in keep:
                    selected.records.pop(nid,None);selected.records.pop(key(node['game_object']),None)
                    for c in node['components']:selected.records.pop(key(c),None)
                else:
                    record=selected.records[nid]
                    record['fields']['m_Children']['value']=[v for v in record['fields']['m_Children']['value'] if tuple(v['value'].values()) in keep]
                    record['links']=[l for l in record['links'] if not l['field'].startswith('m_Children[') or tuple(l['target'].values()) in keep]
    source.world_prototype_roots=set(prototypes)|set(new_roots)
    source.world_material_source=copy.copy(source);source.world_material_source.lighting_textures={tuple(t["source"].values()):t for t in world["material_textures"]}
    source.world_material_export=lambda material:source_material(source,selected,material,output,source.world_material_source)
    print(f'Spawn closure: {len(processed)} input roots, {len(new_roots)} new supported prototypes',flush=True)
    added=export_world_prototypes(source,reader,selected,output,new_roots,{k:'nested' for k in new_roots},scopes,max(p['asset'] for p in world['prototypes'])+1)
    world['prototypes'].extend(added)
    # Supported prototype consumers keep their hierarchy and component records.
    for prototype in added:
        for node in source.hierarchy(source.get(prototype['source']['file'],prototype['source']['path_id']),activate_root=True):
            baseline.add(key(node['reader']))
            baseline.add(key(node['game_object']))
            baseline.update(key(component) for component in node['components'])
    textures={tuple(t['source'].values()):t for t in world['material_textures']}
    textures.update({tuple(t['source'].values()):t for t in source.world_material_source.lighting_textures.values()})
    world['material_textures']=list(textures.values())
    world['records']=list(selected.records.values());world['receipts']=list(selected.receipts.values())
    files={r['source']['file'] for r in world['receipts']}
    world['source_hashes']=[{'file':f,'sha256':reader.source_files[f'TheLongDrive_Data/{f}'].sha256} for f in sorted(files)]
    compact_initialization(world,baseline,source,reader)
    return added

def compact_initialization(world, baseline, source, reader):
    """Store added caller hierarchy and fields without duplicate dynamic graph links."""
    records={tuple(r['source'].values()):r for r in world['records']}
    selection=Selection(source,reader)
    extra={identity for identity in records if identity not in baseline}
    nodes=[];components=[];moved=set()
    for identity in sorted(extra):
        record=records[identity];f=record['fields'];kind=record['class_name']
        if kind not in ('Transform','RectTransform'):continue
        owner=f['m_GameObject']['value'];oid=tuple(owner.values())
        go=records.get(oid)
        if go is None:go=selection.record(source.get(owner['file'],owner['path_id']))
        def vec(name,axes):return [f[name]['value'][a]['value'] for a in axes]
        p=vec('m_LocalPosition','xyz');q=vec('m_LocalRotation','xyzw')
        ordered=[v['value']['component']['value'] for v in go['fields']['m_Component']['value']]
        ordered=[v for v in ordered if tuple(v.values()) in records and records[tuple(v.values())]['class_name'] not in ('Transform','RectTransform')]
        nodes.append(dict(source=record['source'],owner=owner,parent=f['m_Father'].get('value'),children=[v['value'] for v in f['m_Children']['value']],components=ordered,active=bool(go['fields']['m_IsActive']['value']),pose=dict(translation=[p[0],p[1],-p[2]],rotation=[-q[0],-q[1],q[2],q[3]],scale=vec('m_LocalScale','xyz'))))
        moved.add(identity);moved.add(tuple(owner.values()))
    for identity in sorted(extra-moved):
        r=records[identity];f=r['fields']
        if 'm_GameObject' not in f:continue
        script=f.get('m_Script',{}).get('value');order=0
        if script:
            script_record=records.get(tuple(script.values()))
            if script_record:order=script_record['fields'].get('m_ExecutionOrder',{}).get('value',0)
        components.append(dict(source=r['source'],owner=f['m_GameObject']['value'],class_name=r['class_name'],execution_order=order,fields=[dict(name=k,value=v) for k,v in f.items() if k!='m_GameObject']))
        moved.add(identity)
    for component in components:project_component_fields(component)
    world['initialization']=dict(nodes=nodes,components=components)
    ancestors=retain_initialization_ancestors(world,source,reader)
    if ancestors.records:
        from .world_fields import project_runtime_fields
        projection=dict(library=world['library'],source_hashes=world['source_hashes'],records=list(ancestors.records.values()),initialization=dict(nodes=[],components=[]),receipts=list(ancestors.receipts.values()))
        project_runtime_fields(projection)
    world['records']=[r for r in world['records'] if tuple(r['source'].values()) not in moved]
    print(f'Compact initialization: {len(nodes)} nodes, {len(components)} components; original graph {len(world["records"])} records',flush=True)

INITIALIZATION_FIELDS={
    'partconditionscript': {'started','isChild','loaded','randomStart','randomMin','randomMax','enableOverrideMin','enableOverrideMax','overRideMin','overRideMax','randomStartState3InFStart','useRandomTipus','setRandom','chanceToRandom','randomTipusok','slectedRandomTipus','randomChildsSet','tipus','useOnlyMaterialTipusForMaterial','materialTipus','colors','MesList','GoList','childs','noInheritState','state','state2','state3','color','isChildnewColor','sameStateAsParent','dontSetRandomColorTo1'},
    'partslotscript': {'randomTypeToStart','part','enableSpawn','spawnChance','randomChance','randomParts','selectedID','spawn_def','T'},
}
def project_component_fields(component):
    names=INITIALIZATION_FIELDS.get(component['class_name'])
    if names is not None:
        names=names|{'m_Script','m_Enabled'}
        component['fields']=[f for f in component['fields'] if f['name'] in names]

def project_runtime_receipts(world):
    """Preserve complete provenance before retaining the runtime input projection."""
    import hashlib
    import json
    original=json.dumps({"library":world["library"],"source_hashes":world["source_hashes"],"receipts":world["receipts"]},separators=(",",":")).encode()
    digest=hashlib.sha256(original).hexdigest()
    directory=PROVENANCE_ROOT
    directory.mkdir(parents=True,exist_ok=True)
    sidecar=directory/f"world-receipts-{digest}.json"
    if sidecar.exists():
        if sidecar.read_bytes()!=original:raise ValueError("Immutable world provenance differs")
    else:sidecar.write_bytes(original)
    required={}
    def visit(value,path):
        if isinstance(value,dict):
            if set(value)=={"file","path_id"}:required.setdefault((value["file"],value["path_id"]),path)
            else:
                for name,child in value.items():
                    if name!="receipts":visit(child,f"{path}.{name}")
        elif isinstance(value,list):
            for index,child in enumerate(value):visit(child,f"{path}[{index}]")
    visit(world,"world")
    receipts={(r["source"]["file"],r["source"]["path_id"]):r for r in world["receipts"]}
    missing=sorted(required.keys()-receipts.keys())
    if missing:raise ValueError(f"Runtime identities have no checked receipts: {[(i,required[i]) for i in missing[:20]]}")
    world["receipts"]=[r for identity,r in receipts.items() if identity in required]
    print(f"World provenance: {sidecar} SHA256 {digest}; runtime receipts {len(world['receipts'])}",flush=True)


def retain_initialization_ancestors(world, source, reader):
    """Retain ancestor poses required by the projected initialization hierarchy."""
    selection=Selection(source,reader)
    nodes=world['initialization']['nodes']
    known={tuple(n['source'].values()) for n in nodes}|{tuple(r['source'].values()) for r in world['records']}
    pending=[(n['parent'],False) for n in nodes if n['parent'] and tuple(n['parent'].values()) not in known]
    pending += [(child,True) for n in nodes for child in n['children'] if tuple(child.values()) not in known]
    while pending:
        identity,descendant=pending.pop(); key_id=tuple(identity.values())
        if key_id in known:continue
        record=selection.record(source.get(identity['file'],identity['path_id']));f=record['fields']
        owner=f['m_GameObject']['value'];go=selection.record(source.get(owner['file'],owner['path_id']))
        def vec(name,axes):return [f[name]['value'][a]['value'] for a in axes]
        p=vec('m_LocalPosition','xyz');q=vec('m_LocalRotation','xyzw');parent=f['m_Father'].get('value')
        children=[v['value'] for v in f['m_Children']['value']] if descendant else []
        nodes.append(dict(source=identity,owner=owner,parent=parent,children=children,components=[],active=bool(go['fields']['m_IsActive']['value']),pose=dict(translation=[p[0],p[1],-p[2]],rotation=[-q[0],-q[1],q[2],q[3]],scale=vec('m_LocalScale','xyz'))))
        known.add(key_id)
        if parent and tuple(parent.values()) not in known:pending.append((parent,False))
        pending.extend((child,True) for child in children if tuple(child.values()) not in known)
    # Only retained descendants can be called. Original complete records remain in provenance.
    for node in nodes:
        if tuple(node['source'].values()) in selection.records and not node['children']:
            node['children']=[n['source'] for n in nodes if n['parent']==node['source']]
    receipts={tuple(r['source'].values()):r for r in world['receipts']};receipts.update(selection.receipts)
    world['receipts']=list(receipts.values())
    return selection
