"""Checked wearable templates, source anchors, and complete selected variants."""
from .source import key, source_id
from .gltf import converted_transform
from .survival import managed_evidence, finite_record

FAMILIES=((146739,70812,217,0),(147668,53838,218,1),(147667,53837,221,2),
          (131348,63605,245,1),(148817,63259,92,3),(142834,54495,230,4))
ANCHORS=(26477,28766,35477,36615,39162)
ANCESTRY=(39843,39851,39811,22249,39820,25362)
RANDOM_SELECTORS={135226:(70812,[10]*5),157851:(57452,[25,10,10,10,10,10]),154182:(49311,[25,10,10,10,10,10]),149977:(63724,[10]*5)}
MATERIAL_SELECTORS=(132932,150967)


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
    from .menu_figure import recover_menu_figure
    result['menu_figure']=recover_menu_figure(source)
    return result


def validate_clothing(clothing,manifest):
    from .postprocessing import validate_glasses_profiles,validate_dead_profile
    from .ragdoll import validate_ragdoll
    finite_record(clothing)
    from .player_pose import validate_player_pose
    validate_characters(clothing,manifest)
    if clothing['outfit_source']!={'file':'level1','path_id':70544} or clothing['invisible_material']!={'file':'sharedassets0.assets','path_id':56}:raise ValueError('Player outfit appearance source')
    if not any(m['asset']==clothing['characters'][0]['asset'] and m['source']==clothing['invisible_material'] for m in manifest['rendering']['materials']):raise ValueError('Player invisible material dependency')
    validate_player_pose(clothing['pose'],manifest)
    sid=lambda pid,file='sharedassets1.assets':{'file':file,'path_id':pid}
    if clothing['wearable_type_count']!=5 or clothing['player_root']!=sid(39843,'level1') or clothing['anchors']!=[sid(p,'level1') for p in ANCHORS]:raise ValueError('Wearable anchor identities')
    anchor_nodes=clothing['anchor_nodes'];expected=(*ANCESTRY,*ANCHORS)
    if [n['source'] for n in anchor_nodes]!=[sid(p,'level1') for p in expected]:raise ValueError('Anchor ancestry identities')
    for n,pid in zip(anchor_nodes,expected,strict=True):
        parent=None if pid==39843 else sid(ANCESTRY[ANCESTRY.index(pid)-1] if pid in ANCESTRY else 25362,'level1')
        t=n['transform']
        if n['parent']!=parent or n['coordinate_space']!='source_parent_local' or len(t['translation'])!=3 or len(t['scale'])!=3 or any(v==0 for v in t['scale']) or len(t['rotation'])!=4 or abs(sum(v*v for v in t['rotation'])-1)>.002:raise ValueError('Anchor ancestry pose')
    validate_glasses_profiles(clothing['glasses_profiles'],manifest)
    validate_dead_profile(clothing['dead_profile'],manifest)
    validate_ragdoll(clothing['ragdoll'],manifest,clothing)
    from .appearance_preview import validate_preview
    validate_preview(clothing['preview'],clothing,manifest)
    from .menu_figure import validate_menu_figure
    validate_menu_figure(clothing['menu_figure'],clothing,manifest)
    if len(clothing['wearables'])!=6:raise ValueError('Selected wearable count')
    nodes={n['id']:n for n in manifest['nodes']}
    for w,(pid,root,item,type_index) in zip(clothing['wearables'],FAMILIES,strict=True):
        p=next((p for p in manifest['item_prototypes'] if p['id']==w['prototype']),None)
        if w['source']!=sid(pid) or w['prototype']!=sid(root) or w['type_index']!=type_index or p is None or p['item_id']!=item:raise ValueError('Wearable prototype identity')
        if [next((c['source']['path_id'] for c in p['colliders'] if c['id']==i),None) for i in w['colliders']]!=WEARABLE_COLLIDERS[pid]:raise ValueError('Wearable collider links')
        if [nodes.get(i,{}).get('source') for i in w['renderers']]!=[sid(i) for i in WEARABLE_RENDERERS[pid]]:raise ValueError('Wearable source renderers')
        renderers={r['node']:r for r in p['renderers']}
        if len(set(w['renderers']))!=len(w['renderers']) or any(n not in renderers for n in w['renderers']):raise ValueError('Wearable renderer links')
        expected_center={131348:69634,142834:56826}.get(pid)
        if w['use_center_object']!=(expected_center is not None) or (nodes.get(w['center'],{}).get('source') if w['center'] is not None else None)!=(sid(expected_center) if expected_center else None):raise ValueError('Wearable center')
        def local(n):return nodes.get(n,{}).get('asset')==p['asset']
        if pid!=146739:
            if w['selectors'] or w['lenses']:raise ValueError('Unexpected wearable selectors')
            continue
        if [s['source']['path_id'] for s in w['selectors']]!=[135226, 132932, 157851, 154182, 149977, 150967]:raise ValueError('Complete wearable selectors')
        if len(w['lenses'])!=19 or len({l['source']['path_id'] for l in w['lenses']})!=19:raise ValueError('Complete glasses startup lenses')
        for lens in w['lenses']:
            if lens['source']['file']!='sharedassets1.assets' or not local(lens['node']) or lens['node'] not in renderers or lens['wearable']!=w['source'] or lens['profile'] not in range(6):raise ValueError('Glasses startup links')
        validate_selected_glasses(w,manifest,p)
        for s in w['selectors']:
            if not local(s['node']) or s['source']['file']!='sharedassets1.assets' or s['authored_index']!=0 or not s['random_start']:raise ValueError('Wearable selector owner')
            source=s['source']['path_id']
            if source in RANDOM_SELECTORS:
                root_id,weights=RANDOM_SELECTORS[source]
                if s['kind']!='random' or nodes[s['node']]['source']!=sid(root_id) or [v['weight'] for v in s['variants']]!=weights or not s['random_start'] or s['save_index']!=0 or s['children'] or s['material_rules'] is not None:raise ValueError('Wearable random selector')
            elif nodes[s['node']]['source']!=sid(70812 if source==132932 else 85646) or s['kind']!='material' or len(s['variants'])!=3 or s['save_index'] is not None or s['material_rules'] is None:raise ValueError('Wearable material selector')
            for child in s['children']:
                if not any(t['source']==child for t in w['selectors']):raise ValueError('Selector child')
            for v in s['variants']:
                if any(not local(n) for n in v['nodes']) or len(set(v['nodes']))!=len(v['nodes']):raise ValueError('Variant nodes')
                if v['moves'] or v['targets'] or v['part_slots'] or v['parts'] or v['tank'] or v['fluid']!={'all':0.,'f':0.,'fluids':[],'max_c':0.}:raise ValueError('Variant selection payload')
                for r in v['renderers']:
                    if r['node'] not in renderers or not 0<=r['slot']<len(renderers[r['node']]['materials']):raise ValueError('Variant renderer slot')
                if v['material'] and not any(m['asset']==p['asset'] and m['source']==v['material'] for m in manifest['rendering']['materials']):raise ValueError('Variant material dependency')
                lens=next((l for l in w['lenses'] if l['source']==v['lens']),None)
                candidates=[l for l in w['lenses'] if any(descendant(nodes,l['node'],n) for n in v['nodes'])]
                expected=candidates[0]['source'] if len(candidates)==1 else None
                if v['lens']!=expected:raise ValueError('Missing effective variant lens')
                if v['lens'] is None:
                    if v['profile'] is not None or v['disable_lens_flare'] is not None:raise ValueError('Variant startup profile')
                elif lens is None or not any(descendant(nodes,lens['node'],n) for n in v['nodes']) or lens['profile']!=v['profile'] or lens['disable_lens_flare']!=v['disable_lens_flare']:raise ValueError('Variant effective lens')

GLASSES_BRANCHES={135226: [[57452], [49311], [69685], [63724], [74215]], 157851: [[63552], [66918], [82244], [73036], [66333], [56341]], 154182: [[66684], [87658], [79878], [83500], [74142], [77647]], 149977: [[58013], [55142], [57753], [81212], [83913]]}
GLASSES_LENSES=[(154009, 63552, 0, True), (135972, 66918, 4, True), (150488, 82244, 2, True), (143500, 73036, 1, True), (156901, 66333, 5, True), (151834, 56341, 3, True), (140782, 66684, 0, True), (141256, 87658, 4, True), (153056, 79878, 2, True), (147364, 83500, 1, True), (147011, 74142, 5, True), (136426, 77647, 3, True), (157020, 86499, 0, True), (155894, 58013, 4, False), (152881, 55142, 2, False), (140470, 57753, 1, False), (131005, 81212, 5, False), (144723, 83913, 3, False), (151873, 87099, 4, False)]

def validate_selected_glasses(w,manifest,p):
    nodes={n['id']:n['source']['path_id'] for n in manifest['nodes'] if n['asset']==p['asset']}
    for s in w['selectors']:
        pid=s['source']['path_id']
        if pid in GLASSES_BRANCHES:
            if [[nodes.get(n) for n in v['nodes']] for v in s['variants']]!=GLASSES_BRANCHES[pid] or any(v['material'] or v['renderers'] for v in s['variants']):raise ValueError('Exact selected variant nodes')
        else:
            if s['material_rules']!=MATERIAL_RULES[pid] or s['children']!=([{'file':'sharedassets1.assets','path_id':150967}] if pid==132932 else []):raise ValueError('Exact material selector rules')
            expected_renderers=MATERIAL_RENDERERS[pid]
            if [v['material'] for v in s['variants']]!=[{'file':file,'path_id':pid} for file,pid in [('sharedassets1.assets',335),('sharedassets1.assets',253),('sharedassets0.assets',89)]]:raise ValueError('Exact selected material variants')
            for v in s['variants']:
                if v['nodes'] or v['weight'] is not None or [(nodes.get(r['node']),r['slot']) for r in v['renderers']]!=[(n,0) for n in expected_renderers]:raise ValueError('Exact selected material slots')
    if [(l['source']['path_id'],nodes.get(l['node']),l['profile'],l['disable_lens_flare']) for l in w['lenses']]!=GLASSES_LENSES:raise ValueError('Exact selected startup lenses')

MATERIAL_RENDERERS={132932: [81689, 60387, 83695], 150967: [70739]}

WEARABLE_RENDERERS={146739: [81689, 70739, 63552, 66918, 82244, 73036, 66333, 56341, 60387, 66684, 87658, 79878, 83500, 74142, 77647, 80742, 86499, 53868, 58013, 55142, 57753, 81212, 83913, 83695, 87099], 147668: [86632, 88813], 147667: [80368], 131348: [75397], 148817: [84062], 142834: []}
WEARABLE_COLLIDERS={146739: [112724, 113317, 121089], 147668: [117484, 117486], 147667: [117485], 131348: [112577, 119446, 118285, 113889, 112407, 110965, 119440, 111925, 121241, 116901, 114763, 114256], 148817: [111200, 116405, 124754], 142834: [112435]}


def descendant(nodes,node,root):
    for _ in range(len(nodes)):
        if node==root:return True
        node=nodes.get(node,{}).get('parent')
        if node is None:return False
    return False

MATERIAL_RULES={132932: {'DontUseWorn': 1, 'disableColor': 1, 'enableOverrideMax': 0, 'enableOverrideMin': 0, 'isChild': 0, 'loaded': 0, 'noInheritState': 0, 'overRideMax': 0, 'overRideMin': 0, 'randomMax': 2, 'randomMin': 0, 'sameStateAsParent': 0, 'useMatListDontCreateNewMaterial': 1}, 150967: {'DontUseWorn': 1, 'disableColor': 1, 'enableOverrideMax': 0, 'enableOverrideMin': 0, 'isChild': 1, 'loaded': 0, 'noInheritState': 0, 'overRideMax': 0, 'overRideMin': 0, 'randomMax': 2, 'randomMin': 0, 'sameStateAsParent': 0, 'useMatListDontCreateNewMaterial': 1}}

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
    import json
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


def validate_character_renderer(renderer):
    """Check the inactive character renderer schema and source-specific state."""
    fields={'source','node','mesh','gltf_mesh','materials','state','enabled','skin',
        'update_when_offscreen','skinned_motion_vectors','quality'}
    state_fields={'source','bounds_center','bounds_extents','cast_shadows','receive_shadows',
        'reflection_probe_usage','motion_vectors','sorting_layer','sorting_order'}
    if type(renderer) is not dict or set(renderer)!=fields:
        raise ValueError('Character renderer fields')
    state=renderer['state']
    if type(state) is not dict or set(state)!=state_fields:
        raise ValueError('Character renderer state fields')
    if type(renderer['materials']) is not list or not renderer['materials']:
        raise ValueError('Character renderer material array')
    for identity in [renderer['source'],renderer['mesh'],state['source'],*renderer['materials']]:
        if (type(identity) is not dict or set(identity)!={'file','path_id'}
                or type(identity['file']) is not str or not identity['file']
                or type(identity['path_id']) is not int or not 0<identity['path_id']<2**63):
            raise ValueError('Character renderer source identity')
    finite_record(renderer)
    for field in ('bounds_center','bounds_extents'):
        value=state[field]
        if (type(value) is not list or len(value)!=3
                or any(type(v) not in (int,float) or abs(v)>3.4028234663852886e38 for v in value)):
            raise ValueError('Character renderer bounds vector')
    if any(v<0 for v in state['bounds_extents']):
        raise ValueError('Character renderer bounds extents')
    for field in ('node','gltf_mesh'):
        if type(renderer[field]) is not int or not 0<=renderer[field]<2**32:
            raise ValueError('Character renderer node or mesh index')
    pid=renderer['source']['path_id']
    shadow=pid in (58502,58511,58500,58523,58489,58535)
    skinned=pid not in (43612,44815,42694,41622)
    expected={'cast_shadows':3 if shadow else 0 if skinned else 1,
        'reflection_probe_usage':0 if skinned else 1,'motion_vectors':1,
        'sorting_layer':0,'sorting_order':0}
    if (state['source']!=renderer['source'] or renderer['enabled'] is not True
            or state['receive_shadows'] is not (not shadow)
            or any(type(state[f]) is not int or state[f]!=v for f,v in expected.items())):
        raise ValueError('Character renderer state')
    if skinned:
        if (type(renderer['skin']) is not int or not 0<=renderer['skin']<2**32
                or type(renderer['quality']) is not int or renderer['quality']!=0):
            raise ValueError('Character skinned renderer links')
    elif renderer['skin'] is not None or renderer['quality'] is not None:
        raise ValueError('Character eye renderer skin fields')
    if any(renderer[f] is not (True if skinned else None)
            for f in ('update_when_offscreen','skinned_motion_vectors')):
        raise ValueError('Character skinned renderer flags')


def validate_characters(clothing,manifest):
    import colorsys
    nodes={n['id']:n for n in manifest['nodes']}
    characters=clothing['characters']
    if clothing['selected_character']!=0 or len(characters)!=2:raise ValueError('Character selection')
    for index,c in enumerate(characters):
        if type(c['renderers']) is not list:raise ValueError('Character renderer array')
        for renderer in c['renderers']:validate_character_renderer(renderer)
        asset=next((a for a in manifest['assets'] if a['id']==c['asset']),None)
        if asset is None or asset['usage']!='player_prototype' or c['source']!={'file':'level1','path_id':(71215,67763)[index]}:raise ValueError('Character identity')
        def local(n):return n in nodes and nodes[n]['asset']==c['asset'] and descendant(nodes,n,c['root'])
        if not local(c['root']) or nodes[c['root']]['source']!={'file':'level1','path_id':(29438,31145)[index]}:raise ValueError('Character root')
        if [r['source'] for r in c['renderers']]!=[{'file':'level1','path_id':i} for i in CHARACTER_RENDERERS[index]]:raise ValueError('Character renderer order')
        if c['gltf_accessor_count']<=0 or c['gltf_mesh_count']<=0 or any(not 0<=r['gltf_mesh']<c['gltf_mesh_count'] for r in c['renderers']):raise ValueError('Character glTF counts')
        renderers={r['node']:r for r in c['renderers']}
        for r,expected in zip(c['renderers'],CHARACTER_BINDINGS[index],strict=True):
            transform_id,mesh_id,materials=expected
            if nodes.get(r['node'],{}).get('source')!={'file':'level1','path_id':transform_id} or r['mesh']!={'file':'sharedassets0.assets','path_id':mesh_id} or r['materials']!=[{'file':'sharedassets0.assets','path_id':p} for p in materials]:raise ValueError('Exact character renderer bindings')
        if len(renderers)!=len(c['renderers']) or any(not local(r['node']) or not r['materials'] for r in c['renderers']):raise ValueError('Character renderer nodes')
        for r in c['renderers']:
            for material in r['materials']:
                if not any(m['asset']==c['asset'] and m['source']==material for m in manifest['rendering']['materials']):raise ValueError('Character material dependency')
        if len(c['skins'])!=(2,16)[index] or len({s['index'] for s in c['skins']})!=len(c['skins']):raise ValueError('Complete character skins')
        for s in c['skins']:
            if not 0<=s['index']<18 or not 0<=s['inverse_bind']['index']<c['gltf_accessor_count']:raise ValueError('Character glTF skin and accessor index')
            if s['node'] not in renderers or renderers[s['node']]['skin']!=s['index'] or len(s['joints'])!=53 or len(set(s['joints']))!=53:raise ValueError('Character skin links')
            if not local(s['skeleton']) or any(not local(j) or not descendant(nodes,j,s['skeleton']) for j in s['joints']):raise ValueError('Foreign character joint')
            if [nodes[j]['source'] for j in s['joints']]!=[{'file':'level1','path_id':p} for p in CHARACTER_JOINTS[index]]:raise ValueError('Ordered character joints')
            a=s['inverse_bind']
            if a['shape']!='MAT4' or a['component_type']!=5126 or a['count']!=53 or a['byte_stride']!=64 or a['view_byte_length']<53*64:raise ValueError('Character inverse bind accessor')
            if len(s['attributes'])!=len(renderers[s['node']]['materials']):raise ValueError('Character skin primitive count')
            for pair in s['attributes']:
                j,w=pair['joints'],pair['weights']
                if any(not 0<=a['index']<c['gltf_accessor_count'] for a in (j,w)):raise ValueError('Character glTF attribute index')
                if j['shape']!='VEC4' or w['shape']!='VEC4' or j['component_type'] not in (5121,5123) or w['component_type']!=5126 or not j['count'] or j['count']!=w['count']:raise ValueError('Character joint and weight accessors')
        names=('Hair','Eyebrows','Eyes','Mustache','Skin','Shirt','Pants','Shoes') if index==0 else ('Hair','Eyebrows','Eyelashes','Eyes','Skin','Shirt','Pants','Shoes')
        if [s['name'] for s in c['segments']]!=list(names):raise ValueError('Character segment order')
        for segment,expected in zip(c['segments'],CHARACTER_SEGMENTS[index],strict=True):
            label,material,optional,color,pairs=expected
            actual_pairs=[(next((r['source']['path_id'] for r in c['renderers'] if r['node']==b['node']),None),b['slot']) for b in segment['renderers']]
            if segment['language_name']!=label or segment['default_material']!={'file':'sharedassets0.assets','path_id':material} or segment['default_enabled']!=optional or segment['disable_able']!=optional or segment['enabled'] or segment['default_color']!=color or segment['color']!=([0.,0.,0.,0.] if index==0 else [0.,0.,0.,1.]) or actual_pairs!=pairs:raise ValueError('Exact character appearance defaults')
            if len(segment['default_hsv'])!=3 or any(abs(a-b)>1e-6 for a,b in zip(segment['default_hsv'],colorsys.rgb_to_hsv(*segment['default_color'][:3]),strict=True)):raise ValueError('Character HSV order')
            if not segment['renderers']:raise ValueError('Empty character segment')
            for binding in segment['renderers']:
                r=renderers.get(binding['node'])
                if r is None or not 0<=binding['slot']<len(r['materials']):raise ValueError('Character segment material slot')
            if len(segment['default_color'])!=4 or len(segment['color'])!=4 or len(segment['default_hsv'])!=3 or any(not 0<=v<=1 for f in ('default_color','color','default_hsv') for v in segment[f]):raise ValueError('Character appearance values')
    for anchor in clothing['anchor_nodes']:
        n=next((n for n in nodes.values() if n['source']==anchor['source']),None)
        if n is None or n['asset']!=characters[0]['asset'] or n['transform']!=anchor['transform'] or (nodes[n['parent']]['source'] if n['parent'] else None)!=anchor['parent']:raise ValueError('Retained wearable anchor hierarchy')


def validate_character_skins(c,manifest,local):
    """Structure of one character: local renderers, material dependencies, and skin links."""
    renderers={r['node']:r for r in c['renderers']}
    if not renderers or len(renderers)!=len(c['renderers']) or any(not local(r['node']) or not r['materials'] or not 0<=r['gltf_mesh']<c['gltf_mesh_count'] for r in c['renderers']):raise ValueError('Character renderer nodes')
    for r in c['renderers']:
        for material in r['materials']:
            if not any(m['asset']==c['asset'] and m['source']==material for m in manifest['rendering']['materials']):raise ValueError('Character material dependency')
    if len({s['index'] for s in c['skins']})!=len(c['skins']):raise ValueError('Duplicate character skin')
    for s in c['skins']:
        if s['node'] not in renderers or renderers[s['node']]['skin']!=s['index'] or not local(s['skeleton']) or any(not local(j) for j in s['joints']):raise ValueError('Character skin links')
        if s['inverse_bind']['count']!=len(s['joints']) or len(s['attributes'])!=len(renderers[s['node']]['materials']):raise ValueError('Character skin accessors')
    for segment in c['segments']:
        for binding in segment['renderers']:
            r=renderers.get(binding['node'])
            if r is None or not 0<=binding['slot']<len(r['materials']):raise ValueError('Character segment material slot')


def skin_accessor(document,index):
    a=document.accessors[index];v=document.bufferViews[a.bufferView]
    return {'index':index,'count':a.count,'component_type':a.componentType,'shape':a.type,
        'buffer_view':a.bufferView,'byte_offset':(v.byteOffset or 0)+(a.byteOffset or 0),
        'byte_stride':v.byteStride or {'MAT4':64,'VEC4':4*{5121:1,5123:2,5126:4}[a.componentType]}[a.type],
        'view_byte_length':v.byteLength}


def validate_player_export(clothing,manifest,output):
    """Bind typed skin records to actual glTF nodes, accessors, and binary validation."""
    validate_characters(clothing,manifest)
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

CHARACTER_BINDINGS = [[(32922, 473, [75, 74, 59, 55, 58, 61, 57, 60, 73]), (25876, 473, [9, 9, 9, 9, 9, 9, 9, 9, 9]), (36602, 697, [54, 58]), (25048, 645, [58, 54])], [(38614, 690, [65]), (23205, 648, [66]), (27836, 627, [71]), (22144, 637, [72]), (30679, 375, [69]), (23000, 429, [62]), (31080, 618, [67]), (39486, 494, [68]), (32302, 700, [70]), (30051, 613, [63]), (29368, 705, [64]), (37355, 627, [9]), (39907, 637, [9]), (30182, 375, [9]), (30613, 494, [9]), (36960, 700, [9]), (23849, 697, [54, 58]), (40180, 645, [58, 54])]]

CHARACTER_JOINTS = [[26068, 29799, 36277, 36234, 27719, 38049, 32657, 37442, 38141, 26362, 32161, 28124, 24366, 33505, 31941, 28814, 25713, 40017, 33934, 39475, 28176, 25893, 40029, 26243, 25710, 34390, 29634, 40075, 36258, 35131, 24021, 23985, 35295, 36207, 29479, 23595, 23917, 36203, 37854, 27055, 28283, 26010, 25415, 37236, 35291, 26925, 26072, 35908, 39681, 38757, 28290, 36314, 36283], [25455, 30383, 37943, 37865, 28293, 39700, 32025, 36794, 39866, 24604, 32753, 27265, 22453, 31901, 32533, 27041, 24970, 38256, 35588, 38783, 27677, 25383, 38246, 24558, 24975, 35109, 30279, 38321, 37965, 34393, 22219, 22159, 33593, 37927, 31273, 23075, 22202, 37896, 36151, 28886, 27730, 25376, 26108, 36480, 33594, 28693, 25452, 37612, 38100, 39393, 27722, 36856, 37937]]

CHARACTER_SEGMENTS = [[(0, 59, True, [0.2745097875595093, 0.1921568512916565, 0.12549015879631042, 1.0], [(58557, 2)]), (0, 57, True, [0.501960813999176, 0.44705885648727417, 0.3490195870399475, 1.0], [(58557, 6)]), (0, 58, False, [0.5098038911819458, 0.31372547149658203, 0.07843134552240372, 1.0], [(58557, 4), (43612, 1), (44815, 0)]), (0, 60, True, [0.501960813999176, 0.44705885648727417, 0.3490195870399475, 1.0], [(58557, 7)]), (0, 61, False, [0.9528301358222961, 0.7646114826202393, 0.5842826962471008, 1.0], [(58557, 5)]), (0, 75, False, [0.5550907254219055, 0.6712933778762817, 0.7264150977134705, 1.0], [(58557, 0)]), (0, 74, False, [0.6698113083839417, 0.5059859752655029, 0.3159486949443817, 1.0], [(58557, 1)]), (0, 73, False, [0.5283018946647644, 0.29349106550216675, 0.0647917166352272, 1.0], [(58557, 8)])], [(101, 68, True, [0.5999999642372131, 0.4469999670982361, 0.14999999105930328, 1.0], [(58529, 0)]), (102, 65, True, [0.4399999976158142, 0.31679999828338623, 0.13199999928474426, 1.0], [(58487, 0)]), (429, 66, True, [0.5099999904632568, 0.3129359781742096, 0.08160001039505005, 1.0], [(58545, 0)]), (104, 67, False, [0.5099999904632568, 0.3129359781742096, 0.08160001039505005, 1.0], [(58491, 0), (42694, 1), (41622, 0)]), (108, 69, False, [0.949999988079071, 0.7622799873352051, 0.5889999866485596, 1.0], [(58512, 0)]), (105, 72, False, [0.8700000047683716, 0.4948560297489166, 0.44370001554489136, 1.0], [(58478, 0)]), (106, 71, False, [0.5224999785423279, 0.7960999608039856, 0.949999988079071, 1.0], [(58525, 0)]), (107, 70, False, [0.5999999642372131, 0.5999999642372131, 0.5999999642372131, 1.0], [(58531, 0)])]]
