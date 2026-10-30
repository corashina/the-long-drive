"""Recover source startup conditions, runtime variants, and cleaning tools."""
import hashlib
from .source import key, source_id


def roll(identity, purpose, count):
    digest=hashlib.sha256(f'11140890:{identity}:{purpose}'.encode()).digest()
    return int.from_bytes(digest[:4],'little')%count


def condition_material(source, reader, data, group, main_reader, selection):
    state = selection['state']; material = None; color = None
    def selected(name):
        return source.resolve(main_reader, group[name]) if group else source.resolve(reader, data['def' + name])
    if data['UseMatList']:
        if not 0 <= state < len(data['MatList']):
            raise ValueError(f'{source_id(reader)} MatList state {state}')
        material = source.resolve(reader, data['MatList'][state])
        if data['forceColor'] and not data['disableColor']: color = selection['color']
    if not data['DontUseWorn']:
        material = selected(('New', 'Used', 'Middle', 'Old', 'Rusty')[state])
        if material and not data['disableColor']:
            if state < 3 or data['forceColor']: color = selection['color']
            else:
                rusty = selected('Rusty')
                if rusty is None: raise ValueError(f'{source_id(reader)} defRusty color missing')
                color = dict(source.read(rusty)['m_SavedProperties']['m_Colors'])['_Color']
    if material is None and not data['disableColor']:
        colors = group['colors'] if group else data.get('colors', [])
        if len(colors) > state: color = colors[state]
    return material, color


def prepare_conditions(source, roots):
    main_reader=source.get('level1',64919)
    materials={entry['tipus']:entry for entry in source.read(main_reader)['conditionmaterials']}
    readers={}
    for root in roots:
        for node in source.hierarchy(root,activate_root=True):
            if not node['active']:continue
            for reader in node['components']:
                if reader.type.name=='MonoBehaviour' and source.class_name(reader)=='partconditionscript':readers[key(reader)]=reader
    states={identity:{'state':source.read(reader)['state'],'range':None,'color':source.read(reader)['color']} for identity,reader in readers.items()}
    selected_types = {}
    for identity, reader in readers.items():
        data = source.read(reader)
        selected_types[identity] = data['tipus']
        if not data['useRandomTipus']: continue
        choices = data['randomTipusok']
        selection = data['slectedRandomTipus'] if data['setRandom'] else 0
        if not data['setRandom'] and roll(identity, 'type-chance', 1 << 24) / (1 << 24) <= data['chanceToRandom']:
            total = sum(choice['chance'] for choice in choices)
            threshold = roll(identity, 'type-choice', 1 << 24) / (1 << 24) * total
            accumulated = 0.
            for index, choice in enumerate(choices):
                accumulated += choice['chance']
                if threshold <= accumulated:
                    selection = index + 1; break
        if selection:
            selected_types[identity] = choices[selection - 1]['tipus']
            group = materials.get(selected_types[identity])
            if group and not group['colors'] and not data['dontSetRandomColorTo1']:
                states[identity]['color'] = dict(r=1., g=1., b=1., a=1.)
    def randomize(identity,minimum,maximum,color,visiting=None):
        visiting=set() if visiting is None else visiting
        if identity in visiting:raise ValueError('Part condition cycle')
        visiting.add(identity)
        reader=readers[identity];data=source.read(reader)
        if data['isChild']:
            visiting.remove(identity)
            return
        low=data['overRideMin'] if data['enableOverrideMin'] else minimum
        high=data['overRideMax'] if data['enableOverrideMax'] else maximum
        if not 0<=low<=high<=4:raise ValueError(f'Invalid condition range: {identity}')
        state=low+roll(identity,'state',high-low+1)
        states[identity]={'state':state,'range':[low,high],'color':color}
        for pointer in data['childs']:
            child=source.resolve(reader,pointer);child_id=key(child)
            if child_id not in readers:continue
            child_data=source.read(child)
            if child_data['noInheritState'] or child_data['loaded']:continue
            randomize(child_id,minimum,maximum,color,visiting)
        visiting.remove(identity)
    for identity,reader in readers.items():
        data=source.read(reader)
        if not data['randomStart'] or data['isChild']:continue
        group=materials.get(data['materialTipus'] if data['useOnlyMaterialTipusForMaterial'] else selected_types[identity]);colors=group['colors'] if group else data['colors']
        color=colors[roll(identity,'color',len(colors))] if colors else data['color']
        randomize(identity,data['randomMin'],data['randomMax'],color)
    # Refresh copies all three states to child parts before selecting source variants.
    variants = {identity: [data['state2'], data['state3']] for identity, reader in readers.items()
                for data in [source.read(reader)]}
    renderer_meshes = {}; active_nodes = {}
    def refresh_children(identity, visiting=None):
        visiting = set() if visiting is None else visiting
        if identity in visiting: raise ValueError(f'{identity} condition child cycle')
        visiting.add(identity)
        reader = readers[identity]; data = source.read(reader)
        for pointer in data['childs']:
            child = source.resolve(reader, pointer); child_id = key(child)
            if child_id not in readers: continue
            child_data = source.read(child)
            if (child_data['isChild'] and not data['noInheritState']) or child_data['sameStateAsParent']:
                states[child_id]['state'] = states[identity]['state']
                variants[child_id][0] = variants[identity][0]
                if not child_data['randomStartState3InFStart']: variants[child_id][1] = variants[identity][1]
            if child_data['isChildnewColor'] and not child_data['loaded']:
                group = materials.get(child_data['materialTipus'] if child_data['useOnlyMaterialTipusForMaterial'] else selected_types[child_id])
                colors = group['colors'] if group else child_data['colors']
                if colors: states[child_id]['color'] = colors[roll(child_id, 'child-color', len(colors))]
                elif selected_types[child_id] != child_data['tipus'] and not child_data['dontSetRandomColorTo1']:
                    states[child_id]['color'] = dict(r=1., g=1., b=1., a=1.)
            refresh_children(child_id, visiting)
        visiting.remove(identity)
    child_ids = {key(source.resolve(reader, pointer)) for reader in readers.values()
                 for pointer in source.read(reader)['childs']}
    for identity, reader in readers.items():
        data = source.read(reader)
        if data['randomStart'] and not data['isChild']:
            for index, name in enumerate(('MesList', 'GoList')):
                if data[name]: variants[identity][index] = roll(identity, name, len(data[name]))
        if data['randomStartState3InFStart'] and not data['loaded'] and not data['setRandom'] and data['GoList']:
            variants[identity][1] = roll(identity, 'GoList', len(data['GoList']))
    for identity in readers:
        if identity not in child_ids: refresh_children(identity)
    renderer_states={};renderer_materials={};renderer_colors={};evidence=[]
    for identity,reader in readers.items():
        data=source.read(reader);selection=states[identity];state=selection['state'];group=materials.get(data['materialTipus'] if data['useOnlyMaterialTipusForMaterial'] else selected_types[identity])
        material,color=condition_material(source,reader,data,group,main_reader,selection)
        state2, state3 = variants[identity]
        mesh = None
        if data['UseMesList'] and data['MesList']:
            if not 0 <= state2 < len(data['MesList']): raise ValueError(f'{source_id(reader)} MesList state2')
            mesh = source.resolve(reader, data['MesList'][state2])
        if data['UseGoList'] and data['GoList']:
            if not 0 <= state3 < len(data['GoList']): raise ValueError(f'{source_id(reader)} GoList state3')
            selected = source.resolve(reader, data['GoList'][state3])
            if selected:
                for pointer in data['GoList']:
                    game_object = source.resolve(reader, pointer)
                    if game_object: active_nodes[key(source.transform_for(game_object))] = key(game_object) == key(selected)
        for pointer in data['renderers']:
            renderer=source.resolve(reader,pointer)
            if renderer is None:raise ValueError(f'Condition renderer missing: {identity}')
            renderer_states[key(renderer)]={'source':source_id(reader),'state':state,'quality':1.-state/4.,'range':selection['range']}
            if mesh:renderer_meshes[key(renderer)]=mesh
            if material:renderer_materials[key(renderer)]=material
            if color:renderer_colors[key(renderer)]=color
        evidence.append({'source':source_id(reader),**selection,'material':source_id(material) if material else None,
            'state2':state2,'state3':state3,'type':selected_types[identity],
            'mesh':source_id(mesh) if mesh else None,'rule':'partconditionscript.FStart and Refresh; fixed build hash seed.'})
    source.condition_meshes=renderer_meshes
    active_nodes.update(getattr(source,'wash_startup_active',{}))
    source.condition_active=active_nodes
    source.conditions=states
    source.condition_renderers=renderer_states
    source.condition_materials=renderer_materials
    source.condition_colors=renderer_colors
    source.condition_evidence=evidence
    source.condition_readers=readers
    source.condition_variants={}
    source.condition_types=selected_types
    source.condition_groups=materials
    source.condition_main=main_reader

# This group becomes active in terrainGenerator.Start. Child transforms stay authored.
CLEANING_ROOTS=((66615,10842,68227,77707),(66617,10846,68224,77709),
                (66616,10838,68226,71298),(66619,10849,68225,71299),
                (73834,10841,78225,73209),(73835,10840,78223,73210))
MANAGED_SHA256='22ddc62b91954205d90ff513232f39becebbc58a71b718685075f91f64b1f3ee'


def prepare_cleaning_roots(source, roots) -> None:
    terrain=source.get('level1',64898)
    enabled={key(source.resolve(terrain,p)) for p in source.read(terrain)['EnableAfterStart']}
    group=source.get('level1',117)
    if key(group) not in enabled or source.read(group)['m_IsActive']:
        raise ValueError('level1:64898 EnableAfterStart level1:117 differs')
    parent=source.transform_for(group)
    source.cleaning_startup=[]
    source.condition_geometry_nodes=set()
    source.wash_startup_active={}
    source.cleaning_colors={}
    source.cleaning_color_selections={}
    for controller,go_id,first,second in CLEANING_ROOTS:
        reader=source.get('level1',controller);d=source.read(reader)
        go=source.resolve(reader,d['m_GameObject']);root=source.transform_for(go)
        fields=('slide','ownCondition') if controller<70000 else ('ammo','color')
        if (key(go)!=('level1',go_id) or not source.read(go)['m_IsActive']
                or key(source.resolve(root,source.read(root)['m_Father']))!=key(parent)
                or [key(source.resolve(reader,d[f])) for f in fields]!=[('level1',first),('level1',second)]):
            raise ValueError(f'level1:{controller} startup owner/controller links differ')
        roots['items'].append(root)
        source.cleaning_startup.append({'source':source_id(reader),'game_object':source_id(go),
            'parent':source_id(group),'serialized_active':True,'enabled_by':source_id(terrain)})
        if controller>70000:
            color_reader=source.resolve(reader,d['color']);color=source.read(color_reader)
            selected=color['color'];palette_name=None;color_index=None
            if not color['set']:
                main=source.get('level1',64919)
                groups={g['tipus']:g for g in source.read(main)['conditionmaterials']}
                own=bool(d['colors']) and (not d['colorTypes'] or roll(key(reader),'paint-palette',10)<=1)
                if own:
                    color_index=roll(key(reader),'paint-color',len(d['colors']));selected=d['colors'][color_index]
                else:
                    if not d['colorTypes']:raise ValueError(f'{key(reader)} colorTypes has no startup choice')
                    palette_name=d['colorTypes'][roll(key(reader),'paint-type',len(d['colorTypes']))]
                    if palette_name not in groups:raise ValueError(f'{key(reader)} colorTypes missing palette {palette_name}')
                    palette=groups[palette_name]['colors']
                    if not palette:raise ValueError(f'{key(reader)} colorTypes empty palette {palette_name}')
                    color_index=roll(key(reader),'paint-color',len(palette));selected=palette[color_index]
                selected=dict(selected,a=1.)
            source.cleaning_colors[key(source.resolve(reader,d['R']))]=selected
            source.cleaning_color_selections[key(reader)]={'startup_palette':palette_name,'startup_color_index':color_index}

    for readers in roots.values():
        for root in readers:
            for node in source.hierarchy(root,activate_root=True):
                for reader in node['components']:
                    if reader.type.name!='MonoBehaviour' or source.class_name(reader)!='washtriggerscript':continue
                    d=source.read(reader);chance=d['chanceToStartDisabled']
                    source.wash_startup_active[key(node['reader'])]=bool(source.read(node['game_object'])['m_IsActive']) and not (chance>0 and roll(key(reader),'wash-disabled',1<<24)/(1<<24)<chance)
                    source.condition_geometry_nodes.update(key(n['reader']) for n in source.hierarchy(node['reader'],activate_root=True))


def permission_tables(source):
    import json,re
    from .library.reader import checked
    records={r['file']:r for r in source.library.manifest['managed_analysis']['files']}
    content=checked(source.library.records_root,records['managed/Assembly-CSharp.cs'])
    if hashlib.sha256(content).hexdigest()!=MANAGED_SHA256:
        raise ValueError('partconditionscript managed source SHA-256 differs')
    receipt=json.loads(checked(source.library.records_root,records['managed/decompilation.json']))
    if receipt['output']['sha256']!=MANAGED_SHA256 or not receipt['complete']:
        raise ValueError('Cleaning managed decompilation receipt differs')
    code=content.decode('utf-8-sig');start=code.index('public class itemdatabase : MonoBehaviour')
    end=code.find('\npublic class ',start+1);region=code[start:end]
    def rows(field,count):
        pattern=r'public '+(r'repair\[\]' if field=='repairProperties' else r'bool\[\]\[\]')+' '+field+r' = '
        m=re.search(pattern,region)
        if not m:raise ValueError(f'itemdatabase.{field} initializer missing')
        begin=region.index('{',m.end());depth=1;end=begin+1
        while depth:
            depth+=(region[end]=='{')-(region[end]=='}');end+=1
        values=[]
        for match in re.finditer(r'new bool\[7\](?:\s*\{([^}]+)\})?',region[begin:end]):
            row=[v.strip() for v in match[1].split(',')] if match[1] else ['false']*7
            if len(row)!=7 or any(v not in ('true','false') for v in row):
                raise ValueError(f'itemdatabase.{field} bool row differs')
            values.append([v=='true' for v in row])
        if len(values)!=count:raise ValueError(f'itemdatabase.{field} table dimensions differ')
        return values
    repair=rows('repairProperties',40);clean=rows('cleanProperties',4)
    return [repair[i:i+5] for i in range(0,40,5)],clean,records['managed/Assembly-CSharp.cs']


def export_condition_variants(exporter, records, mapping):
    source=exporter.source
    from .materials import required_pointer
    for identity,reader in source.condition_readers.items():
        d=source.read(reader);renderers=[]
        if not any(key(n['reader'])==key(source.transform_for(source.resolve(reader,d['m_GameObject']))) for n in records):continue
        group=source.condition_groups.get(d['materialTipus'] if d['useOnlyMaterialTipusForMaterial'] else source.condition_types[identity])
        for pointer in d['renderers']:
            rr=source.resolve(reader,pointer);rd=source.read(rr)
            root=source.transform_for(source.resolve(rr,rd['m_GameObject']))
            node=key(root)
            if node not in mapping:raise ValueError(f'{key(reader)} renderers outside selected asset: {node}')
            _,parts=source.components(root)
            owner=next((r for r in parts if r.type.name=='MeshFilter'),rr)
            base=required_pointer(source,owner,source.read(owner)['m_Mesh'],'m_Mesh')
            meshes=[source.resolve(reader,p) for p in d['MesList']] if d['UseMesList'] and d['MesList'] else [base]
            defaults=[required_pointer(source,rr,p,'m_Materials') for p in rd['m_Materials']]
            mesh_bindings=[];states=[]
            for mesh in meshes:
                if mesh is None:raise ValueError(f'{key(reader)} MesList unresolved')
                mesh_materials=list(defaults)
                count=len(exporter.geometry(mesh)[2])
                if len(mesh_materials)<count:mesh_materials.extend([mesh_materials[-1]]*(count-len(mesh_materials)))
                index=exporter.mesh(mesh,mesh_materials)
                mesh_bindings.append({'asset':exporter.asset_id,'gltf_mesh':index,'source':source_id(mesh)})
            for state in range(5):
                material,color=condition_material(source,reader,d,group,source.condition_main,
                    {'state':state,'color':source.conditions[identity]['color']})
                materials=[material] if material else defaults
                bindings=[]
                for mat in materials:
                    index=exporter.material(mat,color,{'source':source_id(reader),'state':state,'quality':1.-state/4.,'range':None})
                    bindings.append({'asset':exporter.asset_id,'gltf_material':index,'source':source_id(mat)})
                states.append({'materials':bindings,'color':None if color is None else [color[c] for c in 'rgba']})
            renderers.append({'source':source_id(rr),'node_source':source_id(root),'states':states,'meshes':mesh_bindings})
        source.condition_variants[identity]=renderers


def _linked(source,reader,pointer):return source.resolve(reader,pointer) if pointer['m_PathID'] else None


def _required(source,reader,pointer,field):
    target=_linked(source,reader,pointer)
    if target is None:raise ValueError(f'{key(reader)} {field} unresolved')
    return target


def _sid(reader):return source_id(reader) if reader else None


def brush_tool(source,reader,d,slide_overrides,node_of,output):
    """drotkefescript and its objSlideOnColliderScript slide, then its nodes. `slide_overrides` replaces slide fields set at Start.
    `node_of(reader,pointer)` gives a node of a pointer."""
    from .particles import particle
    slide=_required(source,reader,d['slide'],'slide');sd={**source.read(slide),**slide_overrides}
    nodes={'scrub_node':node_of(reader,d['drot']) if d['drot']['m_PathID'] else None,
        'model':node_of(slide,sd['model']) if sd['model']['m_PathID'] else None,
        'model_equip':node_of(slide,sd['modelEquipPos']) if sd['modelEquipPos']['m_PathID'] else None,
        'model_default':node_of(slide,sd['modelDefPos']) if sd['modelDefPos']['m_PathID'] else None}
    root=source.transform_for(source.resolve(reader,d['m_GameObject']))
    return {'source':source_id(reader),'slide':source_id(slide),
        'cleaning_type':d['cleaningType'],'can_ruin':bool(d['canRuin']),'repair_modifier':d['repairModifier'],
        'min_force':d['minForce'],'wash_radius':d['washRadius'],'wash_mask':d['washMask']['m_Bits'],
        'text':d['text'],'show_text':not bool(d['dontshowtext']),
        'distance':sd['distance'],'mask':sd['LM']['m_Bits'],'position_lerp':sd['posLerp'],'rotation_lerp':sd['rotLerp'],
        'contact_offset':sd['modelDistfromCollider'],'minimum_displacement':sd['minSpeed'],'maximum_displacement':sd['maxSpeed'],
        'disabled':bool(sd['disabled']),'both_work':bool(sd['bothWork']),'sound_when_disabled':bool(sd['soundWhenDisabled']),'has_sound':bool(sd['hasSound']),
        'max_volume':sd['maxVolume'],'volume_lerp':sd['volumeLerp'],'voice_source':_sid(_linked(source,slide,sd['SSource'])),'voice':None,
        'particles':[particle(source,_required(source,slide,sd[field],field),f'cleaning-{reader.path_id}-{field}',sd[rate],output,root)
                for flag,field,rate in [('hasParticle1','particle','maxRate'),('hasParticle2','particle2','maxRate2')] if sd[flag]]},nodes


def spray_tool(source,reader,d,startup_color,selection,output):
    """sprayscript fields. `selection` holds startup_palette and startup_color_index."""
    from .particles import particle
    ammo=_required(source,reader,d['ammo'],'ammo');ad=source.read(ammo);color=_required(source,reader,d['color'],'color');cd=source.read(color)
    for name in d['colorTypes']:
        if name not in source.condition_groups or not source.condition_groups[name]['colors']:
            raise ValueError(f'{key(reader)} colorTypes missing or empty palette {name}')
    root=source.transform_for(source.resolve(reader,d['m_GameObject']))
    return {'source':source_id(reader),**selection,'ammo':source_id(ammo),'color_source':source_id(color),'float_quantity':bool(ad['floatOrInt']),
        'hide_look_at_count':bool(ad['hideLookAtCount']),'hide_carried_count':bool(ad['hideLookAtCountInInventory']),'no_per_show':bool(ad['noPerShow']),
        'startup_quantity':ad['fammo'] if ad['floatOrInt'] else float(ad['ammo']),'maximum_quantity':ad['max'],
        'startup_color':startup_color,'authored_color_set':bool(cd['set']),
        'color_types':d['colorTypes'],
        'color_palettes':{name:[[c[k] for k in 'rgba'] for c in source.condition_groups[name]['colors']] for name in d['colorTypes']},'colors':[[c[k] for k in 'rgba'] for c in d['colors']],
        'renderer':source_id(_required(source,reader,d['R'],'R')),'distance':d['distance'],'mask':d['lm']['m_Bits'],
        'color_lerp':d['colorLerp'],'paint_loss':d['paintLoss'],'valve_speed':d['valveSpeed'],
        'startup_valve':d['valve'],'rate':d['rate'],'max_volume':d['maxVolume'],'text':d['text'],
        'start_speed':d['startSpeed'],'voice_source':_sid(_required(source,reader,d['Saudio'],'Saudio')),'voice':None,
        'particles':[particle(source,_required(source,reader,d['particle'],'particle'),f'paint-{reader.path_id}',d['rate'],output,root)]}


def recover_conditions(source, nodes, components, pointer_node, item_nodes, colliders, output) -> dict:
    from .particles import particle
    import json
    repair,clean,managed=permission_tables(source)
    result={'surfaces':[],'brushes':[],'player_brush':None,'sprays':[],'prototype_tools':[],'washes':[],'collider_bindings':[],
            'repair_tables':repair,'clean_tables':clean,'repair_rate':source.read(source.get('level1',64919))['repairValueIncrease'],
            'repair_decay_rate':source.read(source.get('level1',64919))['repairValueDecrease']}
    surface_ids={identity:i+1 for i,identity in enumerate(source.condition_readers)}
    node_sources={(n['source']['file'],n['source']['path_id']):n['id'] for n in nodes}
    def linked(reader,pointer):return source.resolve(reader,pointer) if pointer['m_PathID'] else None
    def required(reader,pointer,field):
        target=linked(reader,pointer)
        if target is None:raise ValueError(f'{key(reader)} {field} unresolved')
        return target
    def ancestor(node,name):
        while node is not None:
            if components[node].get(name):return components[node][name][0]
            node=nodes[node-1]['parent']
        return None
    def sid(reader):return source_id(reader) if reader else None
    def cid(reader,field):
        if reader is None:return None
        if key(reader) not in surface_ids:raise ValueError(f'{key(reader)} {field} condition owner outside selection')
        return surface_ids[key(reader)]
    def tool_owner(reader,d):
        node=pointer_node(reader,d['m_GameObject']);item_id=item_nodes.get(node)
        if item_id is None:raise ValueError(f'{key(reader)} item missing')
        item=source.condition_items[item_id-1]
        pickups=components[node].get('pickupable',[])
        if len(pickups)!=1:raise ValueError(f'{key(reader)} pickup missing')
        body=source.condition_bodies[item['body']-1]
        rigidbodies=components[node].get('Rigidbody',[])
        if len(rigidbodies)!=1 or body['source']!=source_id(rigidbodies[0]):raise ValueError(f'{key(reader)} body owner link differs')
        if 'P' in d and key(required(reader,d['P'],'P'))!=key(pickups[0]):raise ValueError(f'{key(reader)} P pickup link differs')
        if 'RB' in d and key(required(reader,d['RB'],'RB'))!=key(rigidbodies[0]):raise ValueError(f'{key(reader)} RB body link differs')
        saves=components[node].get('tosaveitemscript',[])
        if len(saves)!=1:raise ValueError(f'{key(reader)} tosaveitemscript owner missing')
        return {'source':source_id(reader),'item':item_id,'body':item['body'],'pickup':source_id(pickups[0]),'save':source_id(saves[0])}
    for identity,reader in source.condition_readers.items():
        d=source.read(reader);node=pointer_node(reader,d['m_GameObject']);selection=source.conditions[identity]
        evidence=next(e for e in source.condition_evidence if (e['source']['file'],e['source']['path_id'])==identity)
        group=source.condition_groups.get(d['materialTipus'] if d['useOnlyMaterialTipusForMaterial'] else evidence['type'])
        colors=group['colors'] if group else d['colors']
        renderers=[]
        for r in source.condition_variants[identity]:
            value=dict(r);value['node']=node_sources[(r['node_source']['file'],r['node_source']['path_id'])];del value['node_source'];renderers.append(value)
        result['surfaces'].append({'id':surface_ids[identity],'source':source_id(reader),'node':node,'item':item_nodes.get(node),
            'startup':{'state':selection['state'],'state2':evidence['state2'],'state3':evidence['state3'],
                'repair_progress':d['repairValue'],'color':[selection['color'][c] for c in 'rgba']},
            'permission_index':d['RepairProperty'],'selected_type':evidence['type'],'material_type':d['materialTipus'],
            'colors':[[c[k] for k in 'rgba'] for c in colors],
            'use_material_type':bool(d['useOnlyMaterialTipusForMaterial']),
            'disable_color':bool(d['disableColor']),'force_color':bool(d['forceColor']),
            'use_mat_list':bool(d['UseMatList']),'dont_use_worn':bool(d['DontUseWorn']),
            'shared_mat_list':bool(d['useMatListDontCreateNewMaterial']),
            'use_mesh_list':bool(d['UseMesList']),'use_object_list':bool(d['UseGoList']),
            'objects':[pointer_node(reader,p) for p in d['GoList']],
            'children':[cid(required(reader,p,'childs'),'childs') for p in d['childs']],
            'is_child':bool(d['isChild']),'no_inherit_state':bool(d['noInheritState']),
            'same_state_as_parent':bool(d['sameStateAsParent']),'child_new_color':bool(d['isChildnewColor']),
            'random_start_state3':bool(d['randomStartState3InFStart']),'loaded':bool(d['loaded']),
            'renderers':renderers})
    def node_of(reader,pointer):return pointer_node(reader,pointer)
    for node,group in components.items():
        for reader in group.get('drotkefescript',[]):
            if key(reader) not in {('level1',r[0]) for r in CLEANING_ROOTS}:continue
            d=source.read(reader);record=tool_owner(reader,d);del record['source']
            record['own_condition']=cid(required(reader,d['ownCondition'],'ownCondition'),'ownCondition')
            record['tool'],record['nodes']=brush_tool(source,reader,d,{},node_of,output)
            result['brushes'].append(record)
        for reader in group.get('sprayscript',[]):
            if key(reader) not in {('level1',r[0]) for r in CLEANING_ROOTS}:continue
            d=source.read(reader);record=tool_owner(reader,d);del record['source']
            selected=source.cleaning_colors[key(source.resolve(reader,d['R']))]
            record['nozzle']=pointer_node(reader,d['hole']);record['renderer_node']=pointer_node(reader,d['R'])
            record['tool']=spray_tool(source,reader,d,[selected[c] for c in 'rgba'],source.cleaning_color_selections[key(reader)],output)
            result['sprays'].append(record)
        for reader in group.get('washtriggerscript',[]):
            d=source.read(reader);effects=[];prefab=linked(reader,d['particleOnDestroy'])
            if prefab:
                root=source.transform_for(prefab)
                for n in source.hierarchy(root):
                    for r in n['components']:
                        if r.type.name=='ParticleSystem':effects.append(particle(source,r,f'wash-{reader.path_id}-{r.path_id}',source.read(r)['EmissionModule']['rateOverTime']['scalar'],output,root))
                if not effects:raise ValueError(f'{key(reader)} particleOnDestroy has no particle')
            chance=d['chanceToStartDisabled']
            active=nodes[node-1]['active'] and not (chance>0 and roll(key(reader),'wash-disabled',1<<24)/(1<<24)<chance)
            result['washes'].append({'id':len(result['washes'])+1,'source':source_id(reader),'node':node,
                'colliders':[c['id'] for c in colliders if ancestor(c['node'],'washtriggerscript') is reader],
                'objects':[pointer_node(reader,p) for p in d['objects']], 'permission_index':d['CleanProperty'],
                'repair_progress':d['repairValue'],'active':active,'on':bool(d['ON']),
                'never_repair':bool(d['neverRepair']),'destroy_on_repair':bool(d['destroyOnRepair']),
                'wash_on_shoot':bool(d['washOnShoot']),'chance_to_start_disabled':chance,'particles':effects})
    # fpscontroller.pointKefe: the player brush. fpscontroller.Start sets slide.LM and slide.distance.
    player=source.get('level1',80125);pd=source.read(player)
    point=required(player,pd['pointKefe'],'pointKefe');point_data=source.read(point)
    if point_data['ownCondition']['m_PathID']:raise ValueError(f'{key(point)} player brush ownCondition is not exported')
    tool,brush_nodes=brush_tool(source,point,point_data,{'LM':pd['pointingLayer'],'distance':pd['pointRange']},node_of,output)
    result['player_brush']={'tool':tool,'nodes':brush_nodes}
    washes={key(source.get(w['source']['file'],w['source']['path_id'])):w['id'] for w in result['washes']}
    slots={key(source.get(s['source']['file'],s['source']['path_id'])):s['id'] for s in source.condition_part_slots}
    anchors={a['id']:a for a in source.condition_anchors}
    tire_slots={}
    for part in source.condition_wheel_parts:
        if part['tire_slot_anchor'] is None:continue
        reader=source.get(part['source']['file'],part['source']['path_id'])
        node=pointer_node(reader,source.read(reader)['m_GameObject'])
        wheelscripts=components[node].get('wheelscript',[])
        if len(wheelscripts)!=1:raise ValueError(f'{key(reader)} tire_slot_anchor wheelscript missing')
        wheel=wheelscripts[0];slot=required(wheel,source.read(wheel)['gumi'],'gumi')
        if pointer_node(slot,source.read(slot)['T'])!=anchors[part['tire_slot_anchor']]['node']:
            raise ValueError(f'{key(slot)} T differs from wheel_parts.tire_slot_anchor')
        tire_slots[key(slot)]=part['item']
    for collider in colliders:
        node=collider['node'];direct=ancestor(node,'partconditionscript');pick=ancestor(node,'pickupable')
        explicit=linked(pick,source.read(pick)['condition']) if pick else None
        forward=ancestor(node,'partconditionforward');forwarded=linked(forward,source.read(forward)['condition']) if forward else None
        slot=ancestor(node,'partslotscript');wash=ancestor(node,'washtriggerscript')
        if not any((direct,explicit,forwarded,slot,wash)):continue
        slot_id=key(slot) if slot else None
        if slot and sum(slot_id in table for table in (slots,tire_slots))!=1:
            raise ValueError(f'{key(slot)} installed_slot ownership unresolved or ambiguous')
        result['collider_bindings'].append({'collider':collider['id'],'source':collider['source'],
            'ancestor_condition':cid(direct,'ancestor_condition'),'pickup_condition':cid(explicit,'pickup_condition'),
            'forwarded_condition':cid(forwarded,'forwarded_condition'),
            'installed_slot_source':sid(slot),'installed_part_slot':slots.get(slot_id),
            'installed_tire_slot':tire_slots.get(slot_id),'wash':washes[key(wash)] if wash else None})
    source.condition_record=result
    physics=source.get('globalgamemanagers',10)
    queries_hit_triggers=source.read(physics)['m_QueriesHitTriggers']
    if queries_hit_triggers is not True:raise ValueError('Selected source tool rays require m_QueriesHitTriggers=true')
    (output/'conditions-evidence.json').write_text(json.dumps({'managed':managed,
        'physics_queries':{'source':source_id(physics),'queries_hit_triggers':queries_hit_triggers},'startup':source.cleaning_startup,
        'selections':source.condition_evidence,'conditions':result,'random_policy':'Fixed build hash seed. Native Unity random sequence is not matched.'},indent=2,allow_nan=False),encoding='utf-8')
    return result


# Loot tool prefabs: catalog index and name, then their drotkefescript or sprayscript.
PROTOTYPE_TOOLS=((117,'spraypaint01',146992,'sprayscript'),(118,'drotkefe01',133845,'drotkefescript'),(119,'sponge01',133844,'drotkefescript'))


def recover_prototype_tools(source, manifest, output) -> list:
    """drotkefescript and sprayscript of world prototypes. Nodes are prototype node sources.
    recover_audio sets `sound`. The runtime selects the startup color of a spray without a set color."""
    catalog=manifest['world']['catalog']['items']
    prototypes={(p['source']['file'],p['source']['path_id']):p for p in manifest['world']['prototypes']}
    records=[]
    for index,name,script,cname in PROTOTYPE_TOOLS:
        item=catalog[index]
        if item['name']!=name or item['prototype'] is None:raise ValueError(f'Catalog item {index} {name} is not a world prototype')
        prototype=prototypes[(item['prototype']['file'],item['prototype']['path_id'])]
        nodes={(n['source']['file'],n['source']['path_id']) for n in prototype['nodes']}
        reader=source.get('sharedassets1.assets',script)
        if reader.type.name!='MonoBehaviour' or source.class_name(reader)!=cname:raise ValueError(f'{key(reader)} is not {cname}')
        d=source.read(reader)
        root=source.transform_for(source.resolve(reader,d['m_GameObject']))
        first=prototype['nodes'][0]['source']
        if key(root)!=(first['file'],first['path_id']):raise ValueError(f'{key(reader)} is not on the root of {name}')
        def node_of(owner,pointer):
            target=_required(source,owner,pointer,'node')
            if target.type.name not in ('Transform','GameObject'):target=source.resolve(target,source.read(target)['m_GameObject'])
            if target.type.name=='GameObject':target=source.transform_for(target)
            if key(target) not in nodes:raise ValueError(f'{key(owner)} node lies outside catalog item {name}')
            return source_id(target)
        if cname=='drotkefescript':
            tool,brush_nodes=brush_tool(source,reader,d,{},node_of,output)
            own=_required(source,reader,d['ownCondition'],'ownCondition')
            if source.class_name(own)!='partconditionscript':raise ValueError(f'{key(reader)} ownCondition is not partconditionscript')
            node_of(own,source.read(own)['m_GameObject'])
            kind={'brush':{'own_condition':source_id(own),'tool':tool,'nodes':brush_nodes,'sound':None}}
        else:
            color=source.read(_required(source,reader,d['color'],'color'))['color']
            tool=spray_tool(source,reader,d,[color[c] for c in 'rgba'],{'startup_palette':None,'startup_color_index':None},output)
            kind={'spray':{'nozzle':node_of(reader,d['hole']),'renderer_node':node_of(reader,d['R']),'tool':tool,'sound':None}}
        records.append({'prototype':item['prototype'],'kind':kind})
    print(f"Loot tools: {len(records)} prototypes ({', '.join(t[1] for t in PROTOTYPE_TOOLS)})",flush=True)
    return records


def bind_condition_audio(record,audio):
    voices={(v['source']['file'],v['source']['path_id']):v['id'] for v in audio['voices']}
    for tool in [*(b['tool'] for b in record['brushes']),record['player_brush']['tool'],*(s['tool'] for s in record['sprays'])]:
        source=tool['voice_source']
        if source:
            identity=(source['file'],source['path_id'])
            if identity not in voices:raise ValueError(f"{tool['source']} voice outside audio selection")
            tool['voice']=voices[identity]
