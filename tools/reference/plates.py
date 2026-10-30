"""Recover the plate font and material, and the car prototype plate labels."""
import hashlib
import json
import math
from .source import key, source_id
from .gltf import converted_transform
from .materials import shader_defaults, export_material_textures

def _resource(reader, output, suffix, payload):
    name=f'plate-{reader.assets_file.name}-{reader.path_id}.{suffix}'
    (output/name).write_bytes(payload)
    return {'source':source_id(reader),'path':f'reference/{name}','sha256':hashlib.sha256(payload).hexdigest()}

def _required(source, owner, pointer, label):
    if not pointer['m_PathID']:raise ValueError(f'{owner.assets_file.name}:{owner.path_id} {label}: required link')
    try:return source.resolve(owner,pointer)
    except (KeyError,IndexError) as error:raise ValueError(f'{owner.assets_file.name}:{owner.path_id} {label}: missing record') from error

def _font(source, reader, atlas, output):
    f=source.read(reader);face=f['m_FaceInfo']
    numeric={k.removeprefix('m_'):v for k,v in face.items() if isinstance(v,(int,float))}
    glyphs=[]
    for g in f['m_GlyphTable']:
        metric=g['m_Metrics'];rect=g['m_GlyphRect']
        glyphs.append({'index':g['m_Index'],'atlas_index':g['m_AtlasIndex'],'scale':g['m_Scale'],
            'metrics':[metric[k] for k in ('m_Width','m_Height','m_HorizontalBearingX','m_HorizontalBearingY','m_HorizontalAdvance')],
            'rect':[rect[k] for k in ('m_X','m_Y','m_Width','m_Height')]})
    characters=[{'unicode':c['m_Unicode'],'glyph':c['m_GlyphIndex'],'scale':c['m_Scale']} for c in f['m_CharacterTable']]
    pairs=[]
    for p in f['m_FontFeatureTable']['m_GlyphPairAdjustmentRecords']:
        values=[]
        for side in ('m_FirstAdjustmentRecord','m_SecondAdjustmentRecord'):
            adjustment=p[side];v=adjustment['m_GlyphValueRecord']
            values.append({'glyph':adjustment['m_GlyphIndex'],'values':[v[k] for k in ('m_XPlacement','m_YPlacement','m_XAdvance','m_YAdvance')]})
        pairs.append({'first':values[0],'second':values[1],'flags':p['m_FeatureLookupFlags']})
    return {'source':source_id(reader),'name':f['m_Name'],'version':f['m_Version'],'family':face['m_FamilyName'],
        'style':face['m_StyleName'],'face':numeric,'glyphs':glyphs,'characters':characters,'pair_adjustments':pairs,
        'atlas':atlas,'atlas_channel':'alpha','atlas_extent':[f['m_AtlasWidth'],f['m_AtlasHeight']],'padding':f['m_AtlasPadding'],
        'atlas_render_mode':f['m_AtlasRenderMode'],'normal_style':f['normalStyle'],'normal_spacing':f['normalSpacingOffset'],
        'bold_style':f['boldStyle'],'bold_spacing':f['boldSpacing']}

def _material(source, reader, output):
    data=source.read(reader);shader=_required(source,reader,data['m_Shader'],'m_Shader');sd=source.read(shader)
    shader_name=sd['m_Name'] or sd['m_ParsedForm']['m_Name']
    if shader_name!='TextMeshPro/Distance Field (Surface)':raise ValueError(f'{key(reader)} required TMP surface shader')
    floats,colors,_=shader_defaults(sd,'plate');saved=data['m_SavedProperties']
    floats.update(dict(saved['m_Floats']));colors.update({n:[c[k] for k in ('r','g','b','a')] for n,c in saved['m_Colors']})
    textures=[]
    # Preserve all named source textures. Their presence does not enable optional effects.
    for slot,env in saved['m_TexEnvs']:
        tr=source.resolve(reader,env['m_Texture'])
        if not tr:continue
        td=source.read(tr);settings=td['m_TextureSettings']
        if slot=='_BumpMap' and (td['m_TextureFormat']!=29 or sd['platforms']!=[4]):raise ValueError(f'{key(tr)} required TMP D3D11 DXT5nm packing')
        name=f'plate-{tr.assets_file.name}-{tr.path_id}.png'
        mips=[{'file':f'reference/{name}' if level==0 else f'reference/{name[:-4]}-mip-{level}.png',
            'width':max(1,td['m_Width']>>level),'height':max(1,td['m_Height']>>level)} for level in range(td['m_MipCount'])]
        textures.append({'slot':slot,'source':source_id(tr),
            'color_space':'linear' if slot in ('_MainTex','_BumpMap') else 'srgb','encoding':'rgb' if slot=='_BumpMap' else 'rgba',
            'source_encoding':'ag' if slot=='_BumpMap' else 'rgba','uv_channel':0,
            'scale':[env['m_Scale'][k] for k in ('x','y')],'offset':[env['m_Offset'][k] for k in ('x','y')],
            'sampler':{'filter':settings['m_FilterMode'],'wrap_u':settings['m_WrapU'],'wrap_v':settings['m_WrapV'],
                'aniso':settings['m_Aniso'],'mip_bias':settings['m_MipBias'],'mip_count':td['m_MipCount']},'storage':{'kind':'png_chain','mips':mips}})
    passes=sd['m_ParsedForm']['m_SubShaders'][0]['m_Passes'];state=passes[0]['m_State']
    def value(field):return floats.get(field['name'],field['val'])
    material_state={'alpha_mode':'blend','cull':int(value(state['culling'])),'z_write':bool(value(state['zWrite'])),
        'z_test':int(value(state['zTest'])),'src_blend':int(value(state['rtBlend0']['srcBlend'])),
        'dst_blend':int(value(state['rtBlend0']['destBlend'])),'render_queue':data['m_CustomRenderQueue']}
    # Use the existing checked image and mip export independently of Standard material conversion.
    dependencies=export_material_textures(source,reader,{'source':source_id(reader),'properties':{'textures':textures}},output)
    suffix,payload=source.shader_resource(shader)
    shader_data=_resource(shader,output,suffix,payload);compiled=_resource(shader,output,'shader.bin',source.raw_data(shader))
    receipts=[{'source':t['source'],'path':m['file'],'sha256':next(d['sha256'] for d in dependencies if d['file']==m['file'])}
        for t in textures for m in t['storage']['mips']]
    unique={r['path']:r for r in receipts}
    return {'source':source_id(reader),'shader':shader_data,'compiled_shader':compiled,'shader_name':shader_name,
        'keywords':data['m_ValidKeywords'],'disabled_passes':data['disabledShaderPasses'],'state':material_state,
        'floats':floats,'colors':colors,'textures':textures,'resources':list(unique.values()),
        'passes':[dict(p['m_State']['m_Tags']['tags'])['LIGHTMODE'] for p in passes],
        'pass_states':[{'name':p['m_State']['m_Name'],'light_mode':dict(p['m_State']['m_Tags']['tags'])['LIGHTMODE'],
            'color_mask':int(value(p['m_State']['rtBlend0']['colMask'])),
            'state':dict(material_state,cull=int(value(p['m_State']['culling'])),z_write=bool(value(p['m_State']['zWrite'])),
                z_test=int(value(p['m_State']['zTest'])),src_blend=int(value(p['m_State']['rtBlend0']['srcBlend'])),
                dst_blend=int(value(p['m_State']['rtBlend0']['destBlend'])))} for p in passes]},next(r for r in receipts if r['source']['path_id']==366)

def plate_layout(text_data, rect):
    """TextMeshPro layout of a plate label. `rect` is its RectTransform."""
    return {'bounds':[rect['m_SizeDelta'][k] for k in ('x','y')],'pivot':[rect['m_Pivot'][k] for k in ('x','y')],
            'anchor_min':[rect['m_AnchorMin'][k] for k in ('x','y')],'anchor_max':[rect['m_AnchorMax'][k] for k in ('x','y')],
            'font_size':text_data['m_fontSize'],'font_size_base':text_data['m_fontSizeBase'],
            'font_size_min':text_data['m_fontSizeMin'],'font_size_max':text_data['m_fontSizeMax'],
            'font_style':text_data['m_fontStyle'],'font_weight':text_data['m_fontWeight'],
            'horizontal_alignment':text_data['m_HorizontalAlignment'],'vertical_alignment':text_data['m_VerticalAlignment'],
            'auto_sizing':bool(text_data['m_enableAutoSizing']),'kerning':bool(text_data['m_enableKerning']),
            'word_wrapping':bool(text_data['m_enableWordWrapping']),'extra_padding':bool(text_data['m_enableExtraPadding']),
            'orthographic':bool(text_data['m_isOrthographic']),'rich_text':bool(text_data['m_isRichText']),
            'overflow_mode':text_data['m_overflowMode'],'max_visible_descender':bool(text_data['m_useMaxVisibleDescender']),
            'spacing':[text_data[k] for k in ('m_characterSpacing','m_wordSpacing','m_lineSpacing','m_paragraphSpacing')],
            'margin':[text_data['m_margin'][k] for k in ('x','y','z','w')],
            'color':[text_data['m_fontColor'][k] for k in ('r','g','b','a')],'transform':converted_transform(rect)}

def prototype_plates(source, proto):
    """rendszamscript labels of one car prototype. Node ids are prototype node indices.

    `same_as` is the node of the linked label. The font and material must be the shared plate contract.
    """
    records=[]
    for node,reader in proto.scripts('rendszamscript'):
        data=source.read(reader)
        text=_required(source,reader,data['text'],'text');text_data=source.read(text)
        font=_required(source,text,text_data['m_fontAsset'],'m_fontAsset')
        material=_required(source,text,text_data['m_sharedMaterial'],'m_sharedMaterial')
        text_node=proto.node(text)
        rect=source.read(proto.hierarchy[text_node]['reader'])
        renderers=[c for c in proto.hierarchy[text_node]['components'] if c.type.name=='MeshRenderer']
        if len(renderers)!=1:raise ValueError(f'{key(reader)} text: required renderer')
        renderer=source.read(renderers[0])
        same=source.resolve(reader,data['sameAsThis'])
        records.append({'source':source_id(reader),'node':node,'text_node':text_node,
            'same_as':proto.node(same) if same else None,'initial_text':data['S'],'initial_same':bool(data['same']),
            'font_source':source_id(font),'material_source':source_id(material),
            'sorting':[renderer['m_SortingLayer'],renderer['m_SortingOrder']],'layout':plate_layout(text_data,rect)})
    nodes={r['node'] for r in records}
    if len(nodes)!=len(records) or any(r['same_as'] is not None and r['same_as'] not in nodes-{r['node']} for r in records):
        raise ValueError('Car plate same_as: invalid sharing')
    return records

def recover_plates(source, prefab, output) -> dict:
    """Shared font and material of the rendszamscript labels of the car prototype `prefab`."""
    readers=set()
    for node in source.hierarchy(prefab,activate_root=True):
        for reader in node['components']:
            if reader.type.name!='MonoBehaviour' or source.class_name(reader)!='rendszamscript':continue
            text=_required(source,reader,source.read(reader)['text'],'text');text_data=source.read(text)
            readers.add((key(_required(source,text,text_data['m_fontAsset'],'m_fontAsset')),
                         key(_required(source,text,text_data['m_sharedMaterial'],'m_sharedMaterial'))))
    if len(readers)!=1:raise ValueError(f'{key(prefab)} plates: required one shared font and material')
    (font_key,material_key),=readers
    font_reader=source.get(*font_key);material_reader=source.get(*material_key)
    material,atlas=_material(source,material_reader,output)
    f=source.read(font_reader)
    if source.read(source.resolve(font_reader,f['m_AtlasTextures'][0]))['m_TextureFormat']!=1:raise ValueError('sharedassets0.assets:366 required alpha atlas')
    if source_id(source.resolve(font_reader,f['m_AtlasTextures'][0]))!=atlas['source']:raise ValueError(f'{key(font_reader)} atlas/material mismatch')
    result={'font':_font(source,font_reader,atlas,output),'material':material}
    validate_plate_data(result)
    return result

def validate_plate_data(result):
    f=result['font'];width,height=f['atlas_extent'];glyphs={g['index']:g for g in f['glyphs']};chars={c['unicode']:c for c in f['characters']}
    if len(glyphs)!=len(f['glyphs']) or len(chars)!=len(f['characters']):raise ValueError('sharedassets0.assets:1366 duplicate glyph/character')
    for code in [*range(65,91),45,*range(48,58)]:
        if code not in chars:raise ValueError(f'sharedassets0.assets:1366 missing generated character {code}')
    for g in f['glyphs']:
        x,y,w,h=g['rect']
        if x<0 or y<0 or w<0 or h<0 or x+w>width or y+h>height or not all(math.isfinite(v) for v in g['metrics']+[g['scale']]):raise ValueError(f'sharedassets0.assets:1366 glyph {g["index"]}: invalid metrics/atlas rectangle')
    if any(c['glyph'] not in glyphs for c in f['characters']):raise ValueError('sharedassets0.assets:1366 missing mapped glyph')
    def finite_tree(value):
        if isinstance(value,float) and not math.isfinite(value):raise ValueError('Plate contract: nonfinite source value')
        if isinstance(value,dict):
            for child in value.values():finite_tree(child)
        elif isinstance(value,list):
            for child in value:finite_tree(child)
    finite_tree(result)
    for pair in f['pair_adjustments']:
        if any(pair[side]['glyph'] not in glyphs for side in ('first','second')):raise ValueError('sharedassets0.assets:1366 pair adjustment: missing glyph')

def plate_resources(contract):
    return [contract['material']['shader'],contract['material']['compiled_shader'],*contract['material']['resources']]

def plate_source_evidence(source, contract):
    from .library.reader import checked
    library=source.library;managed=library.manifest['managed_analysis']
    checked(library.records_root,managed['manifest'])
    receipts={r['file']:r for r in managed['files']}
    code=checked(library.records_root,receipts['managed/Assembly-CSharp.cs']).decode('utf-8')
    decomp=json.loads(checked(library.records_root,receipts['managed/decompilation.json']))
    assembly='TheLongDrive_Data/Managed/Assembly-CSharp.dll'
    if decomp['input']['sha256']!=library.source_files[assembly].sha256:raise ValueError('Plate managed source provenance mismatch')
    library.source_range(assembly,0,library.source_files[assembly].bytes)
    tmp_assembly='TheLongDrive_Data/Managed/Unity.TextMeshPro.dll'
    library.source_range(tmp_assembly,0,library.source_files[tmp_assembly].bytes)
    required=['Attach(attachAtStartTarget);','tosave.FStart();','target.attachs.Add(this);','target.attachs.Remove(this);',
        'transform.position += transform.TransformDirection(removeTargetPos);','rigidbody.mass = rbmass;',
        'RB.mass = mass.Mass();','RB.mass = 1f;','pickedUp.attach.onlyToPoint','!pickedUp.attach.ignorePoints && component2 != null',
        'if (input.mountUp || input.glueUp)','Random.Range(65, 91)','Random.Range(48, 58)']
    for expression in required:
        if expression not in code:raise ValueError(f'Plate executed source differs: {expression}')
    eligibility=code.split('public bool CanAttach(attachablescript attach)',1)[1].split('public class audioBasedOnInteriorsensscript',1)[0]
    if 'text == text2' not in eligibility or 'attachs' in eligibility:raise ValueError('Plate target eligibility differs')
    return {'managed':managed,'assembly':{'file':assembly,'sha256':decomp['input']['sha256']},
        'tmp_assembly':{'file':tmp_assembly,'sha256':library.source_files[tmp_assembly].sha256},
        'checked_expressions':required,
        'target_membership':'attachs is a list; CanAttach matches types and has no occupancy limit.',
        'player_target_path':'onlyToPoint=false; ignorePoints=false. Target branch does not use CanAttach result.',
        'mass_rule':'massScript=1 kg. Pickup applies 1 kg. Drop applies mass.Mass(). Detach restores cached rbmass before pickup.',
        'damping_rule':'No serialized plate Rigidbody. pickup_store_damping preserves RBStore zeros. Effective damping is unset for unity_add_component. Native defaults need separate evidence.',
        'layout_definitions':'Unity.TextMeshPro.dll: HorizontalAlignmentOptions.Center=2; VerticalAlignmentOptions.Middle=512; FontStyles.Bold=1.',
        'records':[library.record_files[f'{file}/{pid}.json'] for file,pid in sorted({(contract[k]['source']['file'],contract[k]['source']['path_id']) for k in ('font','material')})]}
