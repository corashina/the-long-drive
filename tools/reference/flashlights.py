"""Checked portable flashlight bindings."""
import json
from .source import key, source_id
from .managed_code import managed_code, require

ROOTS=(32123,32124)

REQUIRED={
    ('flashlightscript','public void TURN()'):('if (ON)','TurnOff(bsound: true);','TurnOn(bsound: true);'),
    ('flashlightscript','public void Refresh(bool sendmulti = true)'):('if (lamp.broken)','else if (ON && Electronics())','r[i].material = lamp.mON;','r[i].material = lamp.mOFF;'),
    ('flashlightscript','public bool Electronics()'):('return electronics.output > mainscript.lightTurnOffValue;','return true;'),
}

def managed_contract(source):
    code,contract=managed_code(source,'Flashlight')
    require(code,REQUIRED)
    return contract

def prepare_flashlight_roots(source, roots):
    source.flashlight_root_keys={('level1',i) for i in ROOTS}
    source.flashlight_inactive_nodes={key(r['reader']) for r in source.hierarchy(source.get('level1',32124))}
    for i in ROOTS:roots['items'].append(source.get('level1',i))

def validate_flashlight_mesh_export(source, path):
    """Require each selected source renderer mesh in the exported scene."""
    document=json.loads(path.read_text(encoding='utf-8'))
    nodes=document['nodes']; meshes=document['meshes']
    exported={(key_from['file'],key_from['path_id']): (index,node)
              for index,node in enumerate(nodes)
              if (key_from:=node.get('extras',{}).get('reference_id'))}
    for root_id in ROOTS:
        records=source.hierarchy(source.get('level1',root_id))
        root_key=key(records[0]['reader'])
        root_index=exported[root_key][0]
        descendants=set();pending=[root_index]
        while pending:
            index=pending.pop()
            if index in descendants:continue
            descendants.add(index)
            pending.extend(nodes[index].get('children',[]))
        checked=0
        for record in records:
            components={part.type.name:part for part in record['components']}
            renderer=components.get('MeshRenderer'); mesh_filter=components.get('MeshFilter')
            if not renderer or not mesh_filter or not source.read(renderer)['m_Enabled']:continue
            mesh=source.resolve(mesh_filter,source.read(mesh_filter)['m_Mesh'])
            if mesh is None:raise ValueError(f'Flashlight source renderer mesh is missing: {source_id(renderer)}')
            index,node=exported.get(key(record['reader']),(None,None))
            if index not in descendants or node['extras'].get('hierarchy_path')!=record['path']:
                raise ValueError(f'Flashlight renderer left exported hierarchy: {source_id(renderer)}')
            mesh_index=node.get('mesh')
            if not isinstance(mesh_index,int) or not 0<=mesh_index<len(meshes):
                raise ValueError(f'Flashlight renderer mesh omitted: {source_id(renderer)}')
            if meshes[mesh_index].get('extras',{}).get('source')!=source_id(mesh):
                raise ValueError(f'Flashlight renderer mesh differs from source: {source_id(renderer)}')
            checked+=1
        if not checked:raise ValueError(f'Flashlight hierarchy has no source renderer mesh: level1:{root_id}')

def recover_flashlights(source, pointer_node, item_nodes, controllers, output):
    def linked(reader,pointer):return source.resolve(reader,pointer) if pointer['m_PathID'] else None
    def sid(reader,pointer):
        target=linked(reader,pointer)
        return source_id(target) if target else None
    models=[];instances=[];evidence={'startup':{},'bodies':[],'prefabs':[],
                                      'managed':managed_contract(source)}
    terrain=source.get('level1',64898)
    if ('level1',117) not in [key(source.resolve(terrain,p)) for p in source.read(terrain)['EnableAfterStart']]:raise ValueError('StartSpawnItems activation link missing')
    spawn=source.transform_for(source.get('level1',117))
    expected_beams=((56881,12.,0.8500000238418579,90.,8.,(1.,0.8154796361923218,0.6009999513626099,1.)),
                    (56882,15.,1.5,80.,62.188602447509766,(1.,1.,1.,1.)))
    for transform_id,prefab_id,control_id,pickup_id,save_id,body_id,beam in zip(ROOTS,(28750,28749),(74834,74835),(69340,69341),(74314,74315),(50144,50143),expected_beams):
        root=source.get('level1',transform_id);go,components=source.components(root);active=bool(source.read(go)['m_IsActive'])
        if key(linked(root,source.read(root)['m_Father']))!=key(spawn):
            raise ValueError(f'level1:{transform_id} parent is not StartSpawnItems level1:117')
        ctrl=source.get('level1',control_id);cf=source.read(ctrl);pickup=source.get('level1',pickup_id);pf=source.read(pickup);save=source.get('level1',save_id)
        if any(key(source.resolve(r,source.read(r)['m_GameObject']))!=key(go) for r in (ctrl,pickup,save)):raise ValueError('Flashlight owner links differ')
        if key(linked(pickup,pf['lampa']))!=key(ctrl) or cf['ON'] or cf['randomStart'] or cf['SingleLight']['m_PathID'] or cf['electronics']['m_PathID'] or any(l['breaktrigger'] for l in cf['Lamps']):raise ValueError('Flashlight switch contract differs')
        body=source.get('level1',body_id);bf=source.read(body)
        if (bf['m_Mass'],bf['m_Drag'],bf['m_AngularDrag'],bool(bf['m_UseGravity']),bool(bf['m_IsKinematic']))!=(1.,0.,0.05000000074505806,True,False):raise ValueError('Flashlight body contract differs')
        node=pointer_node(ctrl,cf['m_GameObject']);control=next(c for c in controllers if c['source']==source_id(ctrl));control['enabled']=active and control['enabled']
        if len(control['states'])!=1 or len(control['states'][0]['lights'])!=1:
            raise ValueError(f'{key(ctrl)} flashlight state count differs')
        light=source.lighting_lights[control['states'][0]['lights'][0]-1]
        if (light['source']!=source_id(source.get('level1',beam[0]))
                or (light['kind'],light['range_meters'],light['serialized_intensity'],light['full_cone_degrees'],light['inner_cone_degrees'],tuple(light['color']))!=('spot',*beam[1:])
                or light['culling_mask']!=0xffffffff or light['cookie'] is None or light['shadows']['kind']!=1
                or (light['shadows']['strength'],light['shadows']['bias'],light['shadows']['normal_bias'],light['shadows']['near_plane'])!=(1.,0.05000000074505806,0.4000000059604645,0.20000000298023224)):
            raise ValueError(f'{key(ctrl)} flashlight beam differs')
        prefab=source.get('sharedassets1.assets',prefab_id);proot=source.transform_for(prefab);records=source.hierarchy(proot)
        meshes=[];materials=[];renderers=[];lights=[]
        for r in records:
            for c in r['components']:
                f=source.read(c)
                if c.type.name=='MeshFilter':meshes.append(sid(c,f['m_Mesh']))
                if c.type.name=='MeshRenderer':
                    renderers.append(source_id(c));materials.extend(sid(c,p) for p in f['m_Materials'])
                if c.type.name=='Light':lights.append(source_id(c))
        pc=source.get('sharedassets1.assets',148780 if prefab_id==28750 else 148779)
        if key(pc) not in {key(c) for r in records for c in r['components']}:
            raise ValueError(f'{source_id(pc)} is not in prefab sharedassets1.assets:{prefab_id}')
        models.append({'source':source_id(prefab),'controller':source_id(pc),'node':node,'meshes':meshes,'materials':materials,'renderers':renderers,'lights':lights})
        instances.append({'model':source_id(prefab),'item':item_nodes[node],'node':node,'pickup':source_id(pickup),'save':source_id(save),'controller':control['id'],'startup_active':active,'colliders':control['aim_colliders']})
        evidence['bodies'].append({'source':source_id(body),'fields':bf});evidence['prefabs'].append({'source':source_id(prefab),'controller':source.read(pc),'pickup':pf})
    if [i['startup_active'] for i in instances]!=[True,False]:raise ValueError('Flashlight startup activation differs')
    evidence['startup']={'terrain':source_id(terrain),'parent':source_id(source.get('level1',117))}
    (output/'flashlights-evidence.json').write_text(json.dumps(evidence,indent=2)+'\n')
    return {'models':models,'instances':instances}
