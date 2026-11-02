"""Checked computer (computerscript) and TV (displayscript) inputs."""
import math
from .source import key, source_id
from .survival import managed_evidence

# Catalog item index, catalog name, prefab script, and script class.
COMPUTER=(73,'gep01',133932,'computerscript')
DISPLAY=(152,'teve01',144977,'displayscript')
EVIDENCE=[
    'WriteLn("Ready.");',
    'line = new string(screenText, num, num2 - num);',
    'WriteLn2("?Syntax  Error");',
    'yield return new WaitForSeconds(0.05f);',
    'yield return new WaitForSeconds(0.01f);',
    'if (Time.time - lastCursorSwitched >= 0.35f && !musicMode)',
    'float num = Mathf.Pow(2f, (float)(note + (octave - 4) * 12) / 12f);',
    'sounds[num].pitch = num + ((UnityEngine.Random.Range(0f, 1f) < pitchDifferenceChance) ? UnityEngine.Random.Range((0f - pitchDifference) * 0.5f, pitchDifference * 0.5f) : 0f);',
    'musicSelectedOctave = Mathf.Clamp(--musicSelectedOctave, 0, 9);',
    'material.SetTexture("_EmissionMap", input);',
    'ON = !ON;',
    'CursorRect.anchoredPosition = new Vector2(CursorPos.x * CursorPosM.x, (0f - CursorPos.y) * CursorPosM.y);',
    'ScreenText.text += screenText[j + i * xSize];']
# Printable ASCII cells of the glyph atlas.
FIRST_GLYPH,GLYPH_COUNT=32,95


def owner(source, manifest, spec):
    """The catalog prototype of a prefab script, its reader and fields, and a link check."""
    index,name,script,cname=spec
    item=manifest['world']['catalog']['items'][index]
    if item['name']!=name or item['prototype'] is None:raise ValueError(f'Catalog item {index} {name} is not a world prototype')
    prototype=next(p for p in manifest['world']['prototypes'] if p['source']==item['prototype'])
    reader=source.get('sharedassets1.assets',script)
    if reader.type.name!='MonoBehaviour' or source.class_name(reader)!=cname:raise ValueError(f'{key(reader)} is not {cname}')
    nodes={(n['source']['file'],n['source']['path_id']) for n in prototype['nodes']}
    def node(target):
        if target.type.name not in ('Transform','RectTransform'):
            if target.type.name!='GameObject':target=source.resolve(target,source.read(target)['m_GameObject'])
            target=source.transform_for(target)
        if key(target) not in nodes:raise ValueError(f'{key(target)} lies outside catalog item {name}')
        return target
    node(reader)
    def link(fields, field, kind):
        target=source.resolve(reader,fields[field])
        if target is None or target.type.name!=kind:raise ValueError(f'{key(reader)} {field} is not {kind}')
        return target
    return {'item':index,'prototype':item['prototype'],'source':source_id(reader)},prototype,reader,source.read(reader),node,link


def display_switch(source, prototype, display, node):
    """The usablescript whose turnedEvent calls displayscript.Turned."""
    found=[]
    for n in prototype['nodes']:
        transform=source.get(n['source']['file'],n['source']['path_id'])
        for c in source.components(transform)[1]:
            if c.type.name!='MonoBehaviour' or source.class_name(c)!='usablescript':continue
            for call in source.read(c)['turnedEvent']['m_PersistentCalls']['m_Calls']:
                if call['m_CallState']==0:continue
                target=source.resolve(c,call['m_Target'])
                if target is not None and key(target)==key(display):
                    if call['m_MethodName']!='Turned' or call['m_Mode']!=1:raise ValueError(f'{key(c)} display call differs')
                    found.append(c)
    if len(found)!=1:raise ValueError('Display needs one Turned caller')
    usable=found[0];d=source.read(usable)
    if not d['turnable'] or d['slideTurnable'] or d['rotateAble'] or d['slideAble'] or d['maxTurnState']!=0 or len(d['turnClips'])!=1:
        raise ValueError(f'{key(usable)} display switch differs')
    clip=source.resolve(usable,d['turnClips'][0])
    if clip is None or clip.type.name!='AudioClip':raise ValueError(f'{key(usable)} turn clip is not AudioClip')
    source.item_clips.add(key(clip))
    return {'source':source_id(usable),'node':source_id(node(usable)),'clip':source_id(clip),'volume':d['turnVolume']}


def recover_computer(source, manifest) -> dict:
    managed_evidence(source, EVIDENCE)
    record,prototype,reader,d,node,link=owner(source,manifest,COMPUTER)
    pickup=[c for n in [source.transform_for(source.resolve(reader,d['m_GameObject']))] for c in source.components(n)[1]
        if c.type.name=='MonoBehaviour' and source.class_name(c)=='pickupable']
    if len(pickup)!=1 or key(source.resolve(pickup[0],source.read(pickup[0])['computer']))!=key(reader):
        raise ValueError(f'{key(reader)} pickupable computer link differs')
    link(d,'musicClip','AudioClip')
    if d['ON'] or d['connected'] or d['display']['m_PathID'] or d['musicMode']:raise ValueError(f'{key(reader)} start state differs')
    computer={**record,'x_size':d['xSize'],'y_size':d['ySize'],'cursor_step':[d['CursorPosM']['x'],d['CursorPosM']['y']],
        'volume':d['musicVolume'],'pitch_difference_chance':d['pitchDifferenceChance'],'pitch_difference':d['pitchDifference'],
        'screen':recover_screen(source,manifest,d,link)}
    record,prototype,reader,d,node,link=owner(source,manifest,DISPLAY)
    renderer=link(d,'R','MeshRenderer');node(renderer)
    on,off=[link(d,f,'Material') for f in ('defMaterial','OffMaterial')]
    # R shows defMaterial. OffMaterial is the same material without _EMISSION: off is a black emission.
    if [key(source.resolve(renderer,m)) for m in source.read(renderer)['m_Materials']]!=[key(on)]:raise ValueError(f'{key(renderer)} does not show defMaterial')
    od,fd=source.read(on),source.read(off)
    keywords=[set(m.get('m_ValidKeywords',m.get('m_ShaderKeywords','').split())) for m in (od,fd)]
    if od['m_Shader']!=fd['m_Shader'] or od['m_SavedProperties']!=fd['m_SavedProperties'] or keywords[0]-keywords[1]!={'_EMISSION'} or keywords[1]-keywords[0]:
        raise ValueError(f'{key(off)} is not defMaterial without emission')
    color=dict(od['m_SavedProperties']['m_Colors'])['_EmissionColor']
    display={**record,'screen':source_id(node(renderer)),'renderer':source_id(renderer),'emission':[color[c] for c in 'rgba'],
        'startup_on':bool(d['ON']),'switch':display_switch(source,prototype,reader,node)}
    return {'computer':computer,'display':display}


def bake_glyphs(rects, alpha, scale, ascent, advance, convert_case):
    """Text glyph quads of a bitmap font at `scale`, sampled bilinear at pixel centers (Unity UI Text).

    `rects` maps a character to its legacy CharacterInfo (uv, vert, flipped). `alpha` is the font texture
    alpha, rows top-down. A cell origin is the line top-left; y grows down. Returns cell size, offset, coverage."""
    height,tex_w=len(alpha),len(alpha[0])
    def sample(u,v):
        x,y=u*tex_w-.5,v*height-.5
        x0,y0=math.floor(x),math.floor(y);fx,fy=x-x0,y-y0
        def texel(i,j):
            i=min(max(i,0),tex_w-1);j=min(max(j,0),height-1)
            return alpha[height-1-j][i]
        low=texel(x0,y0)*(1-fx)+texel(x0+1,y0)*fx
        high=texel(x0,y0+1)*(1-fx)+texel(x0+1,y0+1)*fx
        return low*(1-fy)+high*fy
    quads=[]
    for code in range(FIRST_GLYPH,FIRST_GLYPH+GLYPH_COUNT):
        source_code=code-32 if convert_case==1 and ord('a')<=code<=ord('z') else code
        r=rects.get(source_code)
        if r is None:raise ValueError(f'Font glyph {source_code} is missing')
        if r['advance']!=advance:raise ValueError('Font is not monospace')
        uv,vert=r['uv'],r['vert']
        u0,v0,u1,v1=uv['x'],uv['y'],uv['x']+uv['width'],uv['y']+uv['height']
        # CharacterInfo: bottom-left is (uv.x, uv.y); a flipped glyph swaps the bottom-right and top-left uvs.
        bl=(u0,v0)
        br,tl=((u0,v1),(u1,v0)) if r['flipped'] else ((u1,v0),(u0,v1))
        x0,x1=vert['x']*scale,(vert['x']+vert['width'])*scale
        y0,y1=(ascent-vert['y'])*scale,(ascent-vert['y']-vert['height'])*scale
        quads.append((x0,x1,y0,y1,bl,br,tl) if x1>x0 and y1>y0 else None)
    live=[q for q in quads if q]
    first_x=math.ceil(min(q[0] for q in live)-.5);last_x=math.ceil(max(q[1] for q in live)-.5)
    first_y=math.ceil(min(q[2] for q in live)-.5);last_y=math.ceil(max(q[3] for q in live)-.5)
    coverage=[]
    for q in quads:
        for py in range(first_y,last_y):
            for px in range(first_x,last_x):
                cx,cy=px+.5,py+.5
                if q is None or not (q[0]<=cx<q[1] and q[2]<=cy<q[3]):
                    coverage.append(0);continue
                x0,x1,y0,y1,bl,br,tl=q
                a,b=(cx-x0)/(x1-x0),(y1-cy)/(y1-y0)
                u=bl[0]+a*(br[0]-bl[0])+b*(tl[0]-bl[0]);v=bl[1]+a*(br[1]-bl[1])+b*(tl[1]-bl[1])
                coverage.append(round(sample(u,v)))
    return {'first':FIRST_GLYPH,'cell':[last_x-first_x,last_y-first_y],'offset':[first_x,first_y],'coverage':coverage}


def _image_color(source, transform):
    _,components=source.components(transform)
    images=[c for c in components if c.type.name=='MonoBehaviour' and source.class_name(c)=='Image']
    if len(images)!=1:raise ValueError(f'{key(transform)} needs one Image')
    d=source.read(images[0])
    if d['m_Sprite']['m_PathID'] or d['m_Type']!=0 or d['m_Material']['m_PathID']:raise ValueError(f'{key(images[0])} is not a plain color')
    return [d['m_Color'][c] for c in 'rgba']


def _corners(d):
    return [d[n][a] for n in ('m_AnchorMin','m_AnchorMax') for a in 'xy']


def _stretched(source, transform):
    """A child rect at its anchors: no offsets, so the pivot does not matter."""
    d=source.read(transform)
    if any(d['m_SizeDelta'][c] or d['m_AnchoredPosition'][c] or d['m_LocalScale'][c]!=1 for c in 'xy'):
        raise ValueError(f'{key(transform)} rect is not stretched')
    return d


def recover_screen(source, manifest, d, link):
    """The C64output canvas: Back fill, Front panel, Text grid and Cursor block, drawn by a screen-space camera."""
    if manifest['lighting']['color_space']!=1:raise ValueError('Computer screen expects linear color space')
    canvas_rt=link(d,'Canv','RectTransform')
    _,components=source.components(canvas_rt)
    canvas=[c for c in components if c.type.name=='Canvas']
    scaler=[c for c in components if c.type.name=='MonoBehaviour' and source.class_name(c)=='CanvasScaler']
    if len(canvas)!=1 or len(scaler)!=1:raise ValueError(f'{key(canvas_rt)} needs one Canvas and scaler')
    c,s=source.read(canvas[0]),source.read(scaler[0])
    camera=link(d,'cam','Camera');texture=link(d,'defRenderTexture','RenderTexture')
    if c['m_RenderMode']!=1 or not c['m_PixelPerfect'] or key(source.resolve(canvas[0],c['m_Camera']))!=key(camera):
        raise ValueError(f'{key(canvas[0])} is not a pixel-perfect camera canvas')
    if s['m_UiScaleMode']!=0 or s['m_ScaleFactor']!=1:raise ValueError(f'{key(scaler[0])} scale differs')
    cam=source.read(camera);rt=source.read(texture)
    if key(source.resolve(camera,cam['m_TargetTexture']))!=key(texture) or rt['m_SRGB'] or rt['m_TextureSettings']['m_FilterMode']!=0:
        raise ValueError(f'{key(texture)} screen texture differs')
    size=[rt['m_Width'],rt['m_Height']]
    text_reader=link(d,'ScreenText','MonoBehaviour')
    if source.class_name(text_reader)!='Text':raise ValueError(f'{key(text_reader)} is not Text')
    t=source.read(text_reader);f=t['m_FontData']
    if (f['m_Alignment'],f['m_HorizontalOverflow'],f['m_VerticalOverflow'],f['m_LineSpacing'],f['m_RichText'],f['m_BestFit'])!=(0,1,1,1,0,0):
        raise ValueError(f'{key(text_reader)} text layout differs')
    # Text fills Front, OS fills the canvas, Back fills OS under Front. Rects are top-down pixels.
    text_rt=source.transform_for(source.resolve(text_reader,t['m_GameObject']))
    td=_stretched(source,text_rt)
    front=source.resolve(text_rt,td['m_Father'])
    fd=_stretched(source,front)
    os_rt=source.resolve(front,fd['m_Father'])
    od=_stretched(source,os_rt)
    if key(source.resolve(os_rt,od['m_Father']))!=key(canvas_rt) or _corners(od)!=[0,0,1,1] or _corners(td)!=[0,0,1,1]:
        raise ValueError(f'{key(os_rt)} screen panels do not fill their parents')
    lo,hi=fd['m_AnchorMin'],fd['m_AnchorMax']
    front_rect=[lo['x']*size[0],(1-hi['y'])*size[1],(hi['x']-lo['x'])*size[0],(hi['y']-lo['y'])*size[1]]
    back=source.resolve(os_rt,od['m_Children'][0])
    bd=_stretched(source,back)
    if _corners(bd)!=[0,0,1,1] or not source.read(source.resolve(back,bd['m_GameObject']))['m_IsActive'] or key(back)==key(front):
        raise ValueError(f'{key(os_rt)} first child is not the Back fill')
    cursor=link(d,'CursorRect','RectTransform');cr=source.read(cursor)
    if key(source.resolve(cursor,cr['m_Father']))!=key(front) or _corners(cr)+[cr['m_Pivot'][a] for a in 'xy']!=[0,1,0,1,0,1]:
        raise ValueError(f'{key(cursor)} cursor is not anchored at the Front top-left')
    quad=link(d,'ScreenQuad','MeshRenderer')
    if source.read(source.resolve(quad,source.read(quad)['m_GameObject']))['m_IsActive']:raise ValueError(f'{key(quad)} ScreenQuad is drawn')
    font=source.resolve(text_reader,f['m_Font'])
    if font is None or font.type.name!='Font':raise ValueError(f'{key(text_reader)} font is not Font')
    fo=source.read(font)
    if fo['m_FontData'] or fo['m_KerningValues'] or fo['m_CharacterSpacing'] or fo['m_AsciiStartOffset']:raise ValueError(f'{key(font)} is not a plain bitmap font')
    atlas=source.resolve(font,fo['m_Texture'])
    if atlas is None or atlas.type.name!='Texture2D':raise ValueError(f'{key(font)} texture is not Texture2D')
    ad=source.read(atlas)
    if ad['m_TextureFormat']!=1 or ad['m_TextureSettings']['m_FilterMode']!=1 or ad['m_MipCount']!=1:raise ValueError(f'{key(atlas)} font texture differs')
    image=source.texture_object(atlas).image.getchannel('A')
    if list(image.size)!=[ad['m_Width'],ad['m_Height']]:raise ValueError(f'{key(atlas)} font texture size differs')
    pixels=list(image.getdata());alpha=[pixels[r*image.width:(r+1)*image.width] for r in range(image.height)]
    scale=f['m_FontSize']/fo['m_FontSize']
    advances={r['advance'] for r in fo['m_CharacterRects']}
    if len(advances)!=1:raise ValueError(f'{key(font)} is not monospace')
    advance=advances.pop()
    if abs(advance*scale-round(advance*scale))>1e-6:raise ValueError(f'{key(font)} advance is not whole pixels')
    glyphs=bake_glyphs({r['index']:r for r in fo['m_CharacterRects']},alpha,scale,fo['m_Ascent'],advance,fo['m_ConvertCase'])
    return {'size':size,'back':_image_color(source,back),'front':_image_color(source,front),'text':[t['m_Color'][c] for c in 'rgba'],
        'cursor':_image_color(source,cursor),'front_rect':front_rect,'cursor_size':[cr['m_SizeDelta']['x'],cr['m_SizeDelta']['y']],
        'advance':round(advance*scale),'line_height':fo['m_LineSpacing']*scale*f['m_LineSpacing'],'glyphs':glyphs}


def _shape(value, fields):
    if type(value) is not dict or set(value)!=set(fields.split()):raise ValueError('Computer record fields differ')


def _number(*values):
    if any(type(v) not in (int,float) or type(v) is bool or not math.isfinite(v) for v in values):raise ValueError('Computer number differs')


def validate_computer(record, manifest) -> None:
    _shape(record,'computer display')
    c=record['computer']
    _shape(c,'item prototype source x_size y_size cursor_step volume pitch_difference_chance pitch_difference screen')
    s=c['screen']
    _shape(s,'size back front text cursor front_rect cursor_size advance line_height glyphs')
    _shape(s['glyphs'],'first cell offset coverage')
    _number(*s['back'],*s['front'],*s['text'],*s['cursor'],*s['front_rect'],*s['cursor_size'],s['line_height'])
    g=s['glyphs']
    if g['first']!=FIRST_GLYPH or len(g['coverage'])!=GLYPH_COUNT*g['cell'][0]*g['cell'][1] or any(type(v) is not int or not 0<=v<=255 for v in g['coverage']):
        raise ValueError('Computer glyph atlas differs')
    if not (0<s['size'][0]<=4096 and 0<s['size'][1]<=4096 and s['advance']>0 and s['line_height']>0):raise ValueError('Computer screen values differ')
    _number(*c['cursor_step'],c['volume'],c['pitch_difference_chance'],c['pitch_difference'])
    if (c['item'],c['source']['path_id'])!=COMPUTER[::2] or not (0<c['x_size']<=256 and 0<c['y_size']<=256):raise ValueError('Computer values differ')
    if not (0<=c['volume']<=1 and 0<=c['pitch_difference_chance']<=1 and c['pitch_difference']>=0):raise ValueError('Computer music values differ')
    t=record['display']
    _shape(t,'item prototype source screen renderer emission startup_on switch')
    _shape(t['switch'],'source node clip volume')
    _number(t['switch']['volume'],*t['emission'])
    if (t['item'],t['source']['path_id'])!=DISPLAY[::2] or len(t['emission'])!=4:raise ValueError('Display values differ')
    clips={(x['source']['file'],x['source']['path_id']) for x in manifest['audio']['clips']}
    if (t['switch']['clip']['file'],t['switch']['clip']['path_id']) not in clips:raise ValueError('Display switch clip is not exported')
