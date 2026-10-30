"""Recover source UI geometry and sprite pixels."""
import hashlib
from .source import key, source_id, vector

def convert_anchor(anchor):
    return anchor[0], 1-anchor[1]

def resolve_rect(rect, parent):
    x,y,w,h=parent
    a,b=rect['anchor_min'],rect['anchor_max']
    size=[w*(b[0]-a[0])+rect['size'][0],h*(b[1]-a[1])+rect['size'][1]]
    pivot=rect['pivot']
    point=[x+w*(a[0]+(b[0]-a[0])*pivot[0])+rect['position'][0],
           y+h*(a[1]+(b[1]-a[1])*pivot[1])+rect['position'][1]]
    size=[size[i]*abs(rect['scale'][i]) for i in range(2)]
    return point[0]-size[0]*pivot[0],point[1]-size[1]*pivot[1],size[0],size[1]

def export_ui(source, output, library):
    output.mkdir(parents=True,exist_ok=True)
    sprites=[]; sprite_ids={}; layouts={}
    def sprite(reader,role):
        identity=key(reader)
        if identity not in sprite_ids:
            from PIL import Image
            from .library.types import ObjectId
            data=source.read(reader)
            payload=library.sprite(ObjectId(*identity))
            with Image.open(payload) as image:
                size=list(image.size)
            path=output/f'ui-{reader.assets_file.name}-{reader.path_id}.png'
            path.write_bytes(payload.read_bytes())
            border=data['m_Border']; rect=data['m_Rect']
            sprite_ids[identity]=len(sprites)
            sprites.append({'role':role,'source':source_id(reader),'path':f'reference/{path.name}',
                'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),
                'crop':[rect[n] for n in ('x','y','width','height')],
                'border':[border[n] for n in ('x','y','z','w')],'size':size})
        return sprite_ids[identity]
    def components(go):
        return [source.resolve(go,c['component']) for c in source.read(go)['m_Component']]
    def rect_for(go):
        return next(c for c in components(go) if c.type.name=='RectTransform')
    def layout(reader,role,parent_button=False):
        data=source.read(reader)
        go=source.resolve(reader,data['m_GameObject']); rt=rect_for(go)
        textdata=data
        if parent_button:
            parent=source.resolve(rt,source.read(rt)['m_Father']);go=source.resolve(parent,source.read(parent)['m_GameObject']);rt=parent
        ancestry=[]; canvas=None
        current=rt
        while current:
            gd=source.resolve(current,source.read(current)['m_GameObject'])
            cs=components(gd)
            if any(c.type.name=='Canvas' for c in cs):
                scaler=next((c for c in cs if c.type.name=='MonoBehaviour' and source.class_name(c)=='CanvasScaler'),None)
                scale=source.read(scaler) if scaler else None
                canvas={'resolution':list(vector(scale['m_ReferenceResolution'],2)) if scale and scale['m_UiScaleMode']==1 else [1920.,1080.],
                    'mode':scale['m_UiScaleMode'] if scale else 0,
                    'match':scale['m_MatchWidthOrHeight'] if scale else 0.,
                    'screen_match':scale['m_ScreenMatchMode'] if scale else 0}
                break
            d=source.read(current)
            a=convert_anchor(vector(d['m_AnchorMin'],2)); b=convert_anchor(vector(d['m_AnchorMax'],2))
            ancestry.append({'source':source_id(current),'anchor_min':[a[0],b[1]],'anchor_max':[b[0],a[1]],
                'position':[d['m_AnchoredPosition']['x'],-d['m_AnchoredPosition']['y']],
                'size':list(vector(d['m_SizeDelta'],2)),'pivot':list(convert_anchor(vector(d['m_Pivot'],2))),
                'scale':list(vector(d['m_LocalScale'],2))})
            current=source.resolve(current,d['m_Father'])
        if canvas is None:raise ValueError(f'Missing UI canvas: {role}')
        screen=(0.,0.,*canvas['resolution'])
        for r in reversed(ancestry):screen=resolve_rect(r,screen)
        image=next((c for c in components(go) if c.type.name=='MonoBehaviour' and source.class_name(c)=='Image'),None)
        im=source.read(image) if image else None
        color=textdata.get('m_fontColor',textdata.get('m_Color',{'r':1.,'g':1.,'b':1.,'a':1.}))
        font=textdata.get('m_FontData',{})
        layouts[role]={'source':source_id(reader),'rect':list(screen),'ancestry':list(reversed(ancestry)),'canvas':canvas,
            'color':[color[channel] for channel in 'rgba'],'font_size':textdata.get('m_fontSize',font.get('m_FontSize',40)),
            'font':'Liberation Sans' if 'm_fontSize' in textdata else 'Arial',
            'alignment':textdata.get('m_textAlignment',font.get('m_Alignment',0)),
            'text':textdata.get('m_text',textdata.get('m_Text','')),
            'sprite':sprite(source.resolve(image,im['m_Sprite']),role) if im and im['m_Sprite']['m_PathID'] else None,
            'image_color':[im['m_Color'][channel] for channel in 'rgba'] if im else [1.,1.,1.,1.]}
    menu=source.get('level1',74691);md=source.read(menu)
    main=source.get('level0',16795);main_data=source.read(main)
    def label_of(go):
        rt=rect_for(go)
        for ptr in source.read(rt)['m_Children']:
            child=source.resolve(rt,ptr);cg=source.resolve(child,source.read(child)['m_GameObject'])
            text=next((c for c in components(cg) if c.type.name=='MonoBehaviour' and source.class_name(c) in ('Text','TextMeshProUGUI')),None)
            if text:return text
        raise ValueError(f'Missing source button label: {key(go)}')
    def find_button(go,method,arg=None,target_id=None):
        rt=rect_for(go)
        for record in source.hierarchy(rt):
            for c in record['components']:
                if c.type.name!='MonoBehaviour' or not source.read(c)['m_Script']['m_PathID'] or source.class_name(c)!='Button':continue
                for call in source.read(c)['m_OnClick']['m_PersistentCalls']['m_Calls']:
                    if call['m_MethodName']==method and (arg is None or call['m_Arguments']['m_StringArgument']==arg) and (target_id is None or call['m_Target']['m_PathID']==target_id):
                        return record['game_object']
        raise ValueError(f'Missing source menu button: {method}')
    start_go=source.resolve(main,main_data['MainMenuStuff'])
    pause_go=source.resolve(menu,md['DisappearWhenExit'])
    for role,go in [('start',find_button(start_go,'PressedLoadScene','SceneNewRandom')),
                    ('start-controls',find_button(start_go,'Open',target_id=main_data['Settings']['m_PathID'])),
                    ('continue',find_button(pause_go,'PressedContinue')),
                    ('controls',find_button(pause_go,'PressedSettings')),
                    ('restart',find_button(pause_go,'Open')),
                    ('recover',source.resolve(menu,md['ButtonExit']))]:
        layout(label_of(go),role,True)
    layout(source.get('level1',67604),'back',True)
    fields={'use':'TextE','pickup':'TextPickup','mount':'TextMount','fill':'LMB',
            'carry':'TextMoveRotateHelp','help':'ThelpText','fluid':'TTankCap','reticle':'GCDef','foot':'TextCF1','driving':'TextCC1'}
    for role,field in fields.items():layout(source.resolve(menu,md[field]),role)
    icons={}
    # mainscript.CrossHairStuff sets these sprites on the GCE, GCAdjust, GCMMB, GCF and GCLMB images.
    for role,field in [('door','IAdjust'),('slide','ISlide'),('slide-vertical','ISlideV'),('switch','ISwitch'),('push','IPush'),
                       ('mount','IMount'),('hose','IHose'),('attach','IAttach'),('fill','IFill'),('enter','ISit'),('exit','ISit2'),('cap','ICap')]:
        icons[role]=sprite(source.resolve(menu,md[field]),role)
    def image_sprite(image,role):return sprite(source.resolve(image,source.read(image)['m_Sprite']),role)
    for role,field in [('default','GCDef'),('pickup','GCCanPickup'),('dismount','GCDismount'),('carrying','GCPickup'),('too-heavy','GCTooHeavy')]:
        icons[role]=image_sprite(source.resolve(menu,md[field]),role)
    # mainscript.OldCrossHairStuff: GC tinted by CrosshairColor, GC2 tinted by the carried weight.
    layout(source.resolve(menu,md['GC']),'old-crosshair')
    layout(source.resolve(menu,md['GC2']),'old-crosshair-carry')
    colors=source.read(source.get('level1',64919))
    crosshair_colors={key:[colors[field][channel] for channel in 'rgba'] for key,field in
        [('default','defCrosshairColor'),('e','canECrosshairColor'),('f','canFCrosshairColor'),('mmb','canMMBCrosshairColor')]}
    # GCPickups: the children of GCPickup that fade in with the carried weight.
    pickups=md['GCPickups']
    if len(pickups)!=2:raise ValueError('GCPickups must hold the medium and heavy images')
    for role,pointer in zip(('carrying-medium','carrying-heavy'),pickups):icons[role]=image_sprite(source.resolve(menu,pointer),role)
    need_sliders={}
    for role,field in [('food','SFood'),('water','SWater'),('hp','SHp'),('bladder','SPiss'),('bowel','SShit'),('sleep','SSleepValue')]:
        slider=source.resolve(menu,md[field]);d=source.read(slider)
        layout(slider,'need-'+role)
        need_sliders[role]={'source':source_id(slider),'minimum':d['m_MinValue'],'maximum':d['m_MaxValue'],
            'direction':d['m_Direction'],'whole_numbers':bool(d['m_WholeNumbers'])}
    for role,field in [('food','Food'),('water','Water'),('hp','HP'),('bladder','Piss'),('bowel','Shit')]:
        for suffix,label in [('', 'fill'),('BG','background'),('Icon','icon')]:
            layout(source.resolve(menu,md['survivalSlider'+field+suffix]),'need-'+role+'-'+label)
    sleep_slider=source.resolve(menu,md['SSleepValue'])
    sleep_data=source.read(sleep_slider)
    fill_rect=source.resolve(sleep_slider,sleep_data['m_FillRect'])
    fill_go=source.resolve(fill_rect,source.read(fill_rect)['m_GameObject'])
    fill_image=next(c for c in components(fill_go) if c.type.name=='MonoBehaviour' and source.class_name(c)=='Image')
    layout(fill_image,'need-sleep-fill')
    slider_go=source.resolve(sleep_slider,sleep_data['m_GameObject'])
    background=[]
    for node in source.hierarchy(rect_for(slider_go),activate_root=True):
        for component in node['components']:
            if component.type.name=='MonoBehaviour' and source.class_name(component)=='Image':
                data=source.read(component)
                if data['m_Enabled'] and key(component)!=key(fill_image):background.append(component)
    if len(background)!=1:raise ValueError('Sleep slider must have one active background')
    layout(background[0],'need-sleep-background')
    for role,field in [('can-sleep','GCanSleep'),('cannot-sleep','GCantSleep'),('sleep-hint','GV')]:
        go=source.resolve(menu,md[field])
        for c in components(go):
            if c.type.name=='MonoBehaviour' and source.class_name(c) in ('Text','TextMeshProUGUI','Image'):
                layout(c,role);break
    pairs=[{'background':sprite(source.resolve(menu,a),'menu-background'),'frame':sprite(source.resolve(menu,b),'menu-frame')}
           for a,b in zip(md['menuOverlayBGs'],md['menuOverlayBGsKeret'],strict=True)]
    if not pairs:raise ValueError('Missing source menu frames')
    for identity in sorted(getattr(source, 'pickup_icons', set())):
        sprite(source.get(*identity), 'inventory-item')
    for identity in sorted(source.crosshair_icons):
        sprite(source.get(*identity), 'crosshair')
    return {'crosshair_colors':crosshair_colors,'need_sliders':need_sliders,'sprites':sprites,'pairs':pairs,'icons':icons,'layouts':layouts,
            'overlay_color':[180/255,128/255,70/255,64/255],'frame_color':[180/255,128/255,70/255,1.]}
