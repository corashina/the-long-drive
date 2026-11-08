"""Checked wearable templates, source anchors, and complete selected variants."""
from .source import key, source_id
from .gltf import converted_transform
from .survival import managed_evidence

FAMILIES=((146739,70812,217,0),(147668,53838,218,1),(147667,53837,221,2),
          (131348,63605,245,1),(148817,63259,92,3),(142834,54495,230,4))
ANCHORS=(26477,28766,35477,36615,39162)
ANCESTRY=(39843,39851,39811,22249,39820,25362)


def recover_clothing(source,manifest,output):
    from .postprocessing import recover_glasses_profiles,recover_dead_profile
    from .ragdoll import recover_ragdoll
    evidence=managed_evidence(source,['w.sunglass = this;', 'mainscript.M.disableSunFlare = disableLensFlare;',
        'mainscript.M.PPVSunGlasses.profile = PP;', 'tipusok[i].objects[j].SetActive(value: false);',
        'tipusok[rtipus].objects[k].SetActive(value: true);'])
    nodes={key(source.get(n['source']['file'],n['source']['path_id'])):n for n in manifest['nodes']}
    nodes_by_id={n['id']:n for n in manifest['nodes']}
    def node(reader):
        if reader.type.name=='GameObject':reader=source.transform_for(reader)
        elif reader.type.name not in ('Transform','RectTransform'):reader=source.transform_for(source.resolve(reader,source.read(reader)['m_GameObject']))
        return nodes[key(reader)]['id']
    def links(reader,values):return [node(source.resolve(reader,p)) for p in values]
    result={'wearable_type_count':5,'player_root':{'file':'level1','path_id':39843},'anchor_nodes':[],
        'anchors':[{'file':'level1','path_id':p} for p in ANCHORS],'wearables':[],
        'glasses_profiles':recover_glasses_profiles(source,manifest,output),
        'dead_profile':recover_dead_profile(source,manifest,output),'evidence':evidence}
    player=source.get('level1',80125)
    root=source.transform_for(source.resolve(player,source.read(player)['m_GameObject']))
    if key(root)!=('level1',39843):raise ValueError('Player Transform root differs')
    for pid in (*ANCESTRY,*ANCHORS):
        reader=source.get('level1',pid);d=source.read(reader);parent=source.resolve(reader,d['m_Father'])
        result['anchor_nodes'].append({'source':source_id(reader),'parent':source_id(parent) if parent else None,
            'coordinate_space':'source_parent_local','transform':converted_transform(d)})
    for pid,root_id,item_id,type_index in FAMILIES:
        reader=source.get('sharedassets1.assets',pid);d=source.read(reader)
        prototype=next(p for p in manifest['item_prototypes'] if p['id']=={'file':'sharedassets1.assets','path_id':root_id})
        if d['tipus']!=type_index or d['sunglass']['m_PathID']:raise ValueError('Wearable source type or startup differs')
        local_colliders={(c['source']['file'],c['source']['path_id']):c['id'] for c in prototype['colliders']}
        root=source.get('sharedassets1.assets',root_id)
        records=source.hierarchy(root,activate_root=True)
        selectors=[];lenses=[]
        for record in records:
            for c in record['components']:
                if c.type.name!='MonoBehaviour':continue
                name=source.class_name(c);v=source.read(c)
                if name=='szbglasscript':
                    pp=source.resolve(c,v['PP'])
                    if key(source.resolve(c,v['w']))!=key(reader):raise ValueError('Foreign glasses startup owner')
                    lenses.append({'source':source_id(c),'node':node(c),'wearable':source_id(reader),
                        'profile':next((i for i,p in enumerate(result['glasses_profiles']) if p['source']==source_id(pp)),None) if pp else None,
                        'disable_lens_flare':bool(v['disableLensFlare'])})
        for record in records:
            for c in record['components']:
                if c.type.name!='MonoBehaviour':continue
                name=source.class_name(c);v=source.read(c)
                if name not in ('randomTypeSelector','partconditionscript'):continue
                selection={'source':source_id(c),'node':node(c),'kind':'random' if name=='randomTypeSelector' else 'material',
                    'authored_index':v['rtipus'] if name=='randomTypeSelector' else v['state'],
                    'random_start':bool(v['forceStart'] if name=='randomTypeSelector' else v['randomStart']),
                    'save_index':v['inTosaveId'] if name=='randomTypeSelector' else None,
                    'children':[source_id(source.resolve(c,p)) for p in v['childs']],
                    'material_rules':None,'variants':[]}
                if name=='partconditionscript':
                    if not v['UseMatList'] or v['UseGoList'] or v['UseMesList'] or v['useRandomTipus']:raise ValueError('Unsupported wearable material selector')
                    if any(v[k] for k in ('isChildnewColor','forceColor','dontSetRandomColorTo1','useOnlyMaterialTipusForMaterial','randomStartState3InFStart','setRandom','started','refresh','chanceToRandom','repairValue','RepairProperty','state2','state3','slectedRandomTipus','randomChildsSet','randomTipusok','colors')):raise ValueError('Unaudited material selector branch')
                    if any(v[k]['m_PathID'] for k in ('defMiddle','defNew','defOld','defRusty','defUsed','mMiddle','mNew','mOld','mRusty','mUsed','tosave')):raise ValueError('Unaudited material selector resource')
                    selection['material_rules']={k:(v[k] if k in ('randomMin','randomMax','overRideMin','overRideMax') else bool(v[k])) for k in ('randomMin','randomMax','isChild','noInheritState','sameStateAsParent','DontUseWorn','disableColor','useMatListDontCreateNewMaterial','enableOverrideMin','enableOverrideMax','overRideMin','overRideMax','loaded')}
                    choices=[{'objects':[],'Move':[],'Target':[],'partslots':[],'parts':[],'tank':{'m_PathID':0,'m_FileID':0},
                        'fluid':{'all':0.,'f':0.,'fluids':[],'maxC':0.},'R':v['renderers'],'M':p,'chance':None} for p in v['MatList']]
                else:choices=v['tipusok']
                for choice in choices:
                    selected=links(c,choice['objects']);material=source.resolve(c,choice['M']);tank=source.resolve(c,choice['tank'])
                    # These selected source payloads contain no transforms, mounts, or fluids.
                    if choice['Move'] or choice['Target'] or choice['partslots'] or choice['parts'] or tank or choice['fluid']['fluids'] or any(choice['fluid'][f] for f in ('all','f','maxC')):raise ValueError('Unaudited wearable variant payload')
                    candidates=[l for l in lenses if any(descendant(nodes_by_id, l['node'], n) for n in selected)]
                    lens=candidates[0] if len(candidates)==1 else None
                    selection['variants'].append({'nodes':selected,'weight':choice['chance'],'material':source_id(material) if material else None,
                        'renderers':[{'node':n,'slot':0} for n in links(c,choice['R'])],
                        'moves':[],'targets':[],'part_slots':[],'parts':[],'tank':None,'fluid':{'all':0.,'f':0.,'fluids':[],'max_c':0.},
                        'lens':lens['source'] if lens else None,'profile':lens['profile'] if lens else None,
                        'disable_lens_flare':lens['disable_lens_flare'] if lens else None})
                selectors.append(selection)
        center=source.resolve(reader,d['CenterObj'])
        result['wearables'].append({'source':source_id(reader),'prototype':prototype['id'],'type_index':type_index,
            'colliders':[local_colliders[key(source.resolve(reader,p))] for p in d['c']],
            'renderers':links(reader,d['Renderers']),'use_center_object':bool(d['useCenterObj']),
            'center':node(center) if center else None,'selectors':selectors,'lenses':lenses})
    from .player_pose import recover_player_pose
    result['characters'],result['selected_character']=recover_characters(source,manifest,output)
    result['pose']=recover_player_pose(source,manifest)
    outfit=source.get('level1',70544)
    result['outfit_source']=source_id(outfit)
    result['invisible_material']=source_id(source.resolve(outfit,source.read(outfit)['defInvisible']))
    result['ragdoll']=recover_ragdoll(source,manifest,output)
    from .appearance_preview import recover_preview
    result['preview']=recover_preview(source)
    result['bulging_eyes']=recover_bulging_eyes(source,manifest,result['characters'])
    return result


# `bulgingeyesScript` of the player characters, then of the `MPPlayer` prefab characters.
BULGING_EYES=(('level1',66030),('level1',77008),('sharedassets0.assets',1409),('sharedassets0.assets',1401))
BULGING_EYES_FIELDS=('startScale','endScale','animx','animy','animz','speedF','speedB')


def recover_bulging_eyes(source,manifest,characters):
    """`bulgingeyesScript`: the eye renderers of each player character and the shared scale curves."""
    from .source import vector
    from .vehicles import curve_record
    asset=next(a for a in manifest['assets'] if a['usage']=='player_prototype')
    nodes={key(source.get(n['source']['file'],n['source']['path_id'])):n['id'] for n in manifest['nodes'] if n['asset']==asset['id']}
    scripts=[source.get(*k) for k in BULGING_EYES]
    if any(source.class_name(s)!='bulgingeyesScript' for s in scripts):raise ValueError('Bulging eyes script differs')
    values=[source.read(s) for s in scripts]
    if any({f:v[f] for f in BULGING_EYES_FIELDS}!={f:values[0][f] for f in BULGING_EYES_FIELDS} or v['disabled'] or not v['m_Enabled'] for v in values):
        raise ValueError('Player bulging eyes differ')
    eyes=[]
    for script,v,character in zip(scripts,values,characters):
        pair=[nodes.get(key(source.resolve(script,p))) for p in v['T']]
        if len(pair)!=2 or any(n not in {r['node'] for r in character['renderers']} for n in pair):raise ValueError('Bulging eyes are not character renderers')
        eyes.append(pair)
    v=values[0]
    return {'source':source_id(scripts[0]),'eyes':eyes,
        'start_scale':list(vector(v['startScale'])),'end_scale':list(vector(v['endScale'])),
        'curves':[curve_record(v[f]) for f in ('animx','animy','animz')],
        'speed_forward':v['speedF'],'speed_back':v['speedB'],
        'evidence':managed_evidence(source,['active = fps.zooming;','active = mps.zooming;',
            'current = Mathf.Lerp(current, 1f, Time.deltaTime * speedF);',
            'current = Mathf.Lerp(current, 0f, Time.deltaTime * speedB);',
            'currentScale.x = Mathf.LerpUnclamped(startScale.x, endScale.x, animx.Evaluate(current));',
            'players[i].mpscript.zooming = _zooming;'])}


def descendant(nodes,node,root):
    for _ in range(len(nodes)):
        if node==root:return True
        node=nodes.get(node,{}).get('parent')
        if node is None:return False
    return False


CHARACTER_RENDERERS = ((58557,58502,43612,44815),
    (58487,58545,58525,58478,58512,58561,58491,58529,58531,58516,58540,58511,58500,58523,58489,58535,42694,41622))


def character_outfit(source,root):
    """playermodeloutfitscript of a character asset root, or None."""
    from .survival import component
    if key(root)==('level1',39843):return source.get('level1',70544)
    if key(root)==getattr(source,'ragdoll_root',None):return component(source,root,'playermodeloutfitscript')
    return None


def character_renderers(source,outfit):
    characters=[source.resolve(outfit,p) for p in source.read(outfit)['characters']]
    if not characters or any(c is None for c in characters):raise ValueError('Missing outfit character')
    return [source.resolve(c,p) for c in characters for p in source.read(c)['ch']['renderers']]


def player_renderers(source):
    result=[]
    for pid,expected in zip((71215,67763),CHARACTER_RENDERERS,strict=True):
        reader=source.get('level1',pid)
        values=[source.resolve(reader,p) for p in source.read(reader)['ch']['renderers']]
        if [key(r) for r in values]!=[('level1',i) for i in expected]:raise ValueError('Complete character renderer membership')
        result.extend(values)
    return result


def recover_characters(source,manifest,output,outfit=None):
    """Characters of the player outfit, or of `outfit` (the ragdoll outfit)."""
    import colorsys
    from pathlib import Path
    from .player_pose import resolved_transform
    from .gltf import validate_gltf, convert_point
    from .source import vector
    asset=next(a for a in manifest['assets'] if a['usage']==('player_prototype' if outfit is None else 'ragdoll_prototype'))
    path=(Path(output) if output is not None else Path('web/assets/reference'))/Path(asset['path']).name
    document=validate_gltf(path)
    nodes={key(source.get(n['source']['file'],n['source']['path_id'])):n for n in manifest['nodes'] if n['asset']==asset['id']}
    def node(r):
        r=resolved_transform(source,r)
        if key(r) not in nodes:raise ValueError('Character source outside player asset')
        return nodes[key(r)]
    accessor=lambda index:skin_accessor(document,index)
    if outfit is None:
        outfit=source.get('level1',70544)
        if [key(source.resolve(outfit,p)) for p in source.read(outfit)['characters']]!=[('level1',71215),('level1',67763)]:raise ValueError('Outfit character order')
        player_renderers(source)
    od=source.read(outfit)
    characters=[]
    for pointer in od['characters']:
        reader=source.resolve(outfit,pointer);d=source.read(reader)['ch'];root=node(reader)
        character={'source':source_id(reader),'asset':asset['id'],'root':root['id'],'gltf_accessor_count':len(document.accessors),'gltf_mesh_count':len(document.meshes),'renderers':[],'skins':[],'segments':[]}
        for pointer in d['renderers']:
            r=source.resolve(reader,pointer);v=source.read(r);n=node(r);gn=document.nodes[n['gltf_node']]
            mesh_owner=r if r.type.name=='SkinnedMeshRenderer' else next(c for c in source.components(resolved_transform(source,r))[1] if c.type.name=='MeshFilter')
            mesh=source.resolve(mesh_owner,source.read(mesh_owner)['m_Mesh'])
            if mesh is None or gn.mesh is None:raise ValueError('Required character geometry')
            materials=[source_id(source.resolve(r,p)) for p in v['m_Materials']]
            bounds=v['m_AABB'] if r.type.name=='SkinnedMeshRenderer' else source.read(mesh)['m_LocalAABB']
            state={'source':source_id(r),'bounds_center':list(convert_point(vector(bounds['m_Center']))),'bounds_extents':list(vector(bounds['m_Extent'])),
                'cast_shadows':v['m_CastShadows'],'receive_shadows':bool(v['m_ReceiveShadows']),
                'reflection_probe_usage':v['m_ReflectionProbeUsage'],'motion_vectors':v['m_MotionVectors'],
                'sorting_layer':v['m_SortingLayer'],'sorting_order':v['m_SortingOrder']}
            skin=None
            if r.type.name=='SkinnedMeshRenderer':
                if gn.skin is None:raise ValueError('Missing intact character skin')
                actual=document.skins[gn.skin]
                joints=[node(source.resolve(r,p))['id'] for p in v['m_Bones']]
                skeleton=node(source.resolve(r,v['m_RootBone']))['id']
                lookup={n['gltf_node']:n['id'] for n in nodes.values()}
                if [lookup[j] for j in actual.joints]!=joints or lookup[actual.skeleton]!=skeleton:raise ValueError('Authored character skin joints')
                skin={'node':n['id'],'index':gn.skin,'joints':joints,'skeleton':skeleton,
                    'inverse_bind':accessor(actual.inverseBindMatrices),
                    'attributes':[{'joints':accessor(p.attributes.JOINTS_0),'weights':accessor(p.attributes.WEIGHTS_0)} for p in document.meshes[gn.mesh].primitives]}
                character['skins'].append(skin)
            character['renderers'].append({'source':source_id(r),'node':n['id'],'mesh':source_id(mesh),
                'gltf_mesh':gn.mesh,'materials':materials,'state':state,'enabled':bool(v['m_Enabled']),
                'skin':gn.skin,'update_when_offscreen':bool(v['m_UpdateWhenOffscreen']) if skin else None,
                'skinned_motion_vectors':bool(v['m_SkinnedMotionVectors']) if skin else None,
                'quality':v['m_Quality'] if skin else None})
            exported=document.meshes[gn.mesh].primitives
            if len(exported)!=len(materials) or [document.materials[p.material].extras['source'] for p in exported]!=materials:raise ValueError('Exact character renderer material slots')
        for seg in d['segments']:
            if len(seg['Renderer'])!=len(seg['materialSlotNums']):raise ValueError('Unequal character renderer-slot arrays')
            color=[seg['defColor'][c] for c in 'rgba'];current=[seg['Ccolor'][c] for c in 'rgba']
            character['segments'].append({'name':seg['name'],'language_name':seg['languageName'],
                'renderers':[{'node':node(source.resolve(reader,p))['id'],'slot':slot} for p,slot in zip(seg['Renderer'],seg['materialSlotNums'],strict=True)],
                'default_material':source_id(source.resolve(reader,seg['defMat'])),
                'default_enabled':bool(seg['defEnabled']),'enabled':bool(seg['BEnabled']),'disable_able':bool(seg['disableAble']),
                'default_color':color,'color':current,'default_hsv':list(colorsys.rgb_to_hsv(*color[:3]))})
        characters.append(character)
    return characters,od['selectedCharacter']


def skin_accessor(document,index):
    a=document.accessors[index];v=document.bufferViews[a.bufferView]
    return {'index':index,'count':a.count,'component_type':a.componentType,'shape':a.type,
        'buffer_view':a.bufferView,'byte_offset':(v.byteOffset or 0)+(a.byteOffset or 0),
        'byte_stride':v.byteStride or {'MAT4':64,'VEC4':4*{5121:1,5123:2,5126:4}[a.componentType]}[a.type],
        'view_byte_length':v.byteLength}


def validate_player_export(clothing,manifest,output):
    """Bind typed skin records to actual glTF nodes, accessors, and binary validation."""
    from pathlib import Path
    from .gltf import validate_gltf
    asset=next(a for a in manifest['assets'] if a['usage']=='player_prototype')
    document=validate_gltf(Path(output)/Path(asset['path']).name)
    nodes={n['id']:n for n in manifest['nodes'] if n['asset']==asset['id']}
    if len(document.nodes)!=len(nodes) or len(document.skins)!=18:raise ValueError('Complete exported player hierarchy and skins')
    parents={child:i for i,n in enumerate(document.nodes) for child in n.children}
    for n in nodes.values():
        actual=document.nodes[n['gltf_node']]
        expected_parent=nodes[n['parent']]['gltf_node'] if n['parent'] is not None else None
        if actual.extras['reference_id']!=n['source'] or parents.get(n['gltf_node'])!=expected_parent or any(getattr(actual,f)!=n['transform'][f] for f in ('translation','rotation','scale')):raise ValueError('Exported authored player node differs')
    selected=set()
    for character in clothing['characters']:
        if character['gltf_accessor_count']!=len(document.accessors) or character['gltf_mesh_count']!=len(document.meshes):raise ValueError('Exported character glTF counts')
        for renderer in character['renderers']:
            gn=document.nodes[nodes[renderer['node']]['gltf_node']];selected.add(nodes[renderer['node']]['gltf_node'])
            if gn.mesh!=renderer['gltf_mesh'] or gn.skin!=renderer['skin']:raise ValueError('Exported character renderer binding')
            primitives=document.meshes[gn.mesh].primitives
            if [document.materials[p.material].extras['source'] for p in primitives]!=renderer['materials']:raise ValueError('Exported character material slots')
        for skin in character['skins']:
            gs=document.skins[skin['index']]
            if gs.joints!=[nodes[n]['gltf_node'] for n in skin['joints']] or gs.skeleton!=nodes[skin['skeleton']]['gltf_node'] or skin_accessor(document,gs.inverseBindMatrices)!=skin['inverse_bind']:raise ValueError('Exported character skin metadata')
            gn=document.nodes[nodes[skin['node']]['gltf_node']]
            expected=[{'joints':skin_accessor(document,p.attributes.JOINTS_0),'weights':skin_accessor(document,p.attributes.WEIGHTS_0)} for p in document.meshes[gn.mesh].primitives]
            if expected!=skin['attributes']:raise ValueError('Exported character skin accessors')
    if selected!={i for i,n in enumerate(document.nodes) if n.mesh is not None}:raise ValueError('Unselected player geometry')


