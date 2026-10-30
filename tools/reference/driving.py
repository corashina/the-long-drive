"""Driving controls and wheel records from checked build 11140890 sources."""
import hashlib
import re
from .source import key, source_id

ASSEMBLY_SHA256 = '9919a9eeddcce23bb4ff86e890437f32b4a3ef3c10af2efe3b8d889d894dd62e'
DECOMPILED_SHA256 = '22ddc62b91954205d90ff513232f39becebbc58a71b718685075f91f64b1f3ee'
# inputscript.IN actions the port leaves out: 3D screenshots, VR, multiplayer and voice chat.
LEFT_OUT = {'screenshot3D', 'screenshot360', 'camera360', 'vrrecenter', 'multiplayerchat', 'voicechat'}
KEY_NAMES = {'LeftControl':'ControlLeft', 'RightControl':'ControlRight', 'LeftShift':'ShiftLeft', 'RightShift':'ShiftRight',
    'LeftAlt':'AltLeft', 'RightAlt':'AltRight', 'UpArrow':'ArrowUp', 'DownArrow':'ArrowDown', 'LeftArrow':'ArrowLeft',
    'RightArrow':'ArrowRight', 'KeypadPlus':'NumpadAdd', 'KeypadMinus':'NumpadSubtract', 'Space':'Space', 'Tab':'Tab',
    'CapsLock':'CapsLock', 'Backspace':'Backspace'}
# oneKey.Set(axis, invert) mouse axes, named like oneKey.GetName.
MOUSE_AXES = ('Mouse X', 'Mouse Y', 'Mouse ScrollWheel')


def key_code(name):
    """Unity KeyCode name to the browser KeyboardEvent.code (mouse buttons stay Mouse0-2)."""
    if re.fullmatch(r'[A-Z]',name):return 'Key'+name
    if match:=re.fullmatch(r'Alpha(\d)',name):return 'Digit'+match[1]
    if match:=re.fullmatch(r'Keypad(\d)',name):return 'Numpad'+match[1]
    if re.fullmatch(r'F\d{1,2}|Mouse[0-2]',name):return name
    if name in KEY_NAMES:return KEY_NAMES[name]
    raise ValueError(f'Unmapped input key: {name}')


def method(text, declaration):
    start = text.index(declaration)
    opening = text.index('{', start)
    depth = 1; end = opening + 1
    while depth:
        if text[end] == '{':depth += 1
        elif text[end] == '}':depth -= 1
        end += 1
    body = text[start:end]
    return body, {'declaration':declaration, 'first_line':text.count('\n',0,start)+1,
        'last_line':text.count('\n',0,end)+1, 'sha256':hashlib.sha256(body.encode()).hexdigest()}


def checked_methods(source):
    from .library.reader import checked
    import json
    library = source.library
    assembly = library.snapshot.root / 'TheLongDrive_Data/Managed/Assembly-CSharp.dll'
    digest = hashlib.sha256(assembly.read_bytes()).hexdigest()
    if digest != ASSEMBLY_SHA256:raise ValueError('Driving source assembly differs')
    manifest = json.loads((library.records_root/'managed/manifest.json').read_bytes())
    record = next(r for r in manifest['files'] if r['file']=='managed/Assembly-CSharp.cs')
    if record['sha256'] != DECOMPILED_SHA256:raise ValueError('Driving managed source receipt differs')
    text = checked(library.records_root,record).decode('utf-8-sig')
    declarations = ['public void DriveStuff()', 'private void InitDefault()', 'public void RefreshSettings()',
        'private void LoadSettings()', 'public IEnumerator DoReset(bool _force)',
        'public void FStart()', 'public void RefreshMenu()', 'public void ResetSettings(bool _force)',
        'public void ResetCustomRadioPath()', 'public void OnChangedAnything()',
        'public void ResetToDefault()', 'public void WheelSize()', 'public float GetRadius()',
        'public bool IsAutoShift()', 'public void ShiftTo(int pg, bool _tryanyway = false, bool multi = false)',
        'public void Shifted(bool playSoundAndAnimation = true)']
    # Scope declarations that also occur on unrelated classes.
    scopes = {'public void RefreshSettings()':'public class settingsscript :',
        'private void LoadSettings()':'public class settingsscript :',
        'public void OnChangedAnything()':'public class settingsscript :',
        'public void FStart()':'public class settingsscript :',
        'public void RefreshMenu()':'public class settingsscript :',
        'public void ResetToDefault()':'public class togglesetting :'}
    bodies={}; evidence=[]
    for declaration in declarations:
        scoped = text[text.index(scopes[declaration]):] if declaration in scopes else text
        body, entry = method(scoped,declaration)
        offset = len(text)-len(scoped)
        entry['first_line'] += text.count('\n',0,offset)
        entry['last_line'] += text.count('\n',0,offset)
        bodies[declaration] = body;evidence.append(entry)
    if 'private bool playTransmissionSoundsInAutomatic;' not in text:
        raise ValueError('Driving shift sound default differs')
    if re.search(r'playTransmissionSoundsInAutomatic\s*=', text):
        raise ValueError('Driving shift sound default has an assignment')
    minimum = re.search(r'public static float minWheelMotorTorque = ([\d.]+)f;',text)
    if minimum is None:raise ValueError('Missing wheel motor torque constant')
    return bodies, evidence, float(minimum[1]), text


LEVER_FIELDS = {'seconds':'TransmissionLerp', 'stage1':'transmissionStage1', 'stage2':'transmissionStage2'}
LEVER_LERPS = [f'Transmission.{part} = {kind}.Lerp(Transmission.{part}, gears[{gear} + 1].{target}.{part}, transMissionLerp3);'
    for gear,target in (('lastgear','Path'),('gear','Path'),('gear','Pos'))
    for part,kind in (('position','Vector3'),('rotation','Quaternion'))]


def lever_constants(text):
    """carscript lever timing from the private field initializers."""
    values = {}
    for name,field in LEVER_FIELDS.items():
        match = re.search(rf'private float {field} = ([\d.]+)f;', text)
        if match is None:raise ValueError(f'Missing lever constant: {field}')
        values[name] = float(match[1])
    if any(line not in text for line in LEVER_LERPS):raise ValueError('Lever lerp source differs')
    return values


def recover_lever(source, car_reader, car, root, node_ids):
    """carscript.Transmission and gear Path/Pos, posed in the lever parent space, or None
    when the car has no lever. `node_ids` maps source Transform keys to prototype node indices.
    The source writes world poses. The runtime writes the node local pose."""
    from .collision import world_pose
    from .particles import relative_pose
    lever = source.resolve(car_reader, car['Transmission'])
    if lever is None:return None
    def inside(reader):
        while reader is not None:
            if key(reader)==key(root):return True
            reader = source.resolve(reader, source.read(reader)['m_Father'])
        return False
    parent = source.resolve(lever, source.read(lever)['m_Father'])
    if parent is None or not inside(parent):raise ValueError(f'Lever parent is outside the car: {source_id(lever)}')
    if key(lever) not in node_ids:raise ValueError(f'Missing lever node: {key(lever)}')
    scale = world_pose(source, parent)['scale']
    if max(scale)-min(scale) > 1e-5 * max(abs(v) for v in scale):raise ValueError(f'Lever parent scale is not uniform: {source_id(lever)}')
    def pose(pointer):
        reader = source.resolve(car_reader, pointer)
        if reader is None:raise ValueError(f'Missing lever gear pose: {source_id(car_reader)}')
        if not inside(reader):raise ValueError(f'Lever gear pose is outside the car: {source_id(car_reader)}')
        value = relative_pose(source, reader, parent)
        value['translation'] = [v/scale[0] for v in value['translation']]
        return value
    return {'node':node_ids[key(lever)], 'gears':[{'path':pose(g['Path']), 'pos':pose(g['Pos'])} for g in car['gears']]}


MOUSE_DRIVE_LINES = ['currentMouseSteer = Mathf.Clamp(currentMouseSteer + Input.GetAxis("Mouse X") * 0.0125f * 0.25f * settingsscript.s.S.FMouseDriveSensX, -1f, 1f);',
    'currentMouseGas = Mathf.Clamp(currentMouseGas + Input.GetAxis("Mouse Y") * 0.0125f * settingsscript.s.S.FMouseDriveSensY, -1f, 1f);']


def slider_range(source, settings_reader, settings, tab, index):
    """slidersettingscript value range: clamp to minValue..maxValue, then GetValue multiplies."""
    reader = source.resolve(settings_reader, settings[tab]['sliders'][index])
    if reader is None:raise ValueError(f'Unresolved {tab} slider: {index}')
    value = source.read(reader)
    multiplier = value['valueMultipler'] if value['valueMultipler'] else 1.
    if not value['minValue'] <= value['defValue'] <= value['maxValue']:raise ValueError(f'{tab} slider default outside range: {index}')
    return {'default':value['defValue']*multiplier, 'minimum':value['minValue']*multiplier, 'maximum':value['maxValue']*multiplier}, reader


# DriveStuff lines that the Rust axis and gear rules port.
STEERING_WHEEL_LINES = ['float num = (Input.GetAxis(settingsscript.s.S.CSSteer) + settingsscript.s.S.CFSteerMiddle) * (settingsscript.s.S.CBinvertSteer ? (-1f) : 1f);',
    'value = Mathf.Lerp(-1f, 1f, Mathf.InverseLerp(settingsscript.s.S.CFGasDead - 1f, 1f, value));',
    'value = (value + 1f) * settingsscript.s.S.CFGasSens - 1f;',
    'handbrake = Mathf.Clamp(Mathf.InverseLerp(-1f, 1f, value4), 0f, 1f);',
    'gnDown = !grDown && !g1Down && !g2Down && !g3Down && !g4Down && !g5Down && !g6Down;']
# Rust `AxisSetting` order and the source S field names.
WHEEL_AXES = ('Steer', 'Gas', 'Brake', 'Clutch', 'HandBrake')


def steering_wheel_fields(refresh):
    """RefreshSettings: S.<field> = tabSteeringWheel.<toggles|sliders>[index]."""
    fields = {name:(kind,int(index)) for name,kind,index in re.findall(
        r'S\.(C\w+) = tabSteeringWheel\.(toggles|sliders)\[(\d+)\]\.(?:toggle\.isOn|GetValue\(\));', refresh)}
    required = {'CBHShifter':'toggles', 'CFSteerMiddle':'sliders',
        **{f'CB{flag}{axis}':'toggles' for axis in WHEEL_AXES for flag in ('enable','invert')},
        **{f'CF{axis}{part}':'sliders' for axis in WHEEL_AXES for part in ('Dead','Sens','Max')}}
    for name,kind in required.items():
        if fields.get(name,(None,))[0] != kind:raise ValueError(f'Steering wheel refresh differs: {name}')
    return {name:fields[name] for name in required}


def toggle_default(source, settings_reader, settings, tab, index):
    """togglesetting default (`def`), which the startup reset applies."""
    reader = source.resolve(settings_reader, settings[tab]['toggles'][index])
    if reader is None or source.class_name(reader)!='togglesetting':raise ValueError(f'Unresolved {tab} toggle: {index}')
    return bool(source.read(reader)['def']), reader


def recover_steering_wheel(source, settings_reader, settings, refresh):
    """tabSteeringWheel defaults: H-shifter and per axis enable, invert and slider ranges."""
    fields = steering_wheel_fields(refresh)
    def control(name):
        kind,index = fields[name]
        read = toggle_default if kind=='toggles' else slider_range
        return read(source, settings_reader, settings, 'tabSteeringWheel', index)[0]
    axes = [{'enabled':control(f'CBenable{axis}'), 'invert':control(f'CBinvert{axis}'),
        'middle':control('CFSteerMiddle') if axis=='Steer' else None,
        'dead':control(f'CF{axis}Dead'), 'sens':control(f'CF{axis}Sens'), 'max':control(f'CF{axis}Max')}
        for axis in WHEEL_AXES]
    return {'h_shifter':control('CBHShifter'), 'axes':axes}, {name:{'control':kind,'index':index} for name,(kind,index) in fields.items()}


def recover_bindings(text, defaults):
    """inputscript.IN actions and their three InitDefault keys, in enum order. None is null."""
    enum = re.search(r'public enum IN\s*\{([^}]+)\}',text)
    if enum is None:raise ValueError('Missing input enum')
    names=[name.strip() for name in enum[1].split(',') if name.strip()]
    if '= new oneKey[3];' not in defaults.replace('inputscript.',''):raise ValueError('Input key count differs')
    keys={(int(i),int(k)):argument for i,k,argument in re.findall(
        r'defaultInput\[(\d+)\]\.keys\[(\d)\]\.Set\((KeyCode\.\w+|"[^"]+", _invertAxis: (?:true|false))\);',defaults)}
    if len(keys)!=defaults.count('.Set('):raise ValueError('Unparsed input default')
    bindings=[];evidence=[]
    for index,name in enumerate(names):
        slots=[]
        for k in range(3):
            argument=keys.pop((index,k),None)
            if argument is None:slots.append(None)
            elif argument.startswith('KeyCode.'):slots.append(key_code(argument[8:]))
            else:
                axis,invert=re.fullmatch(r'"([^"]+)", _invertAxis: (true|false)',argument).groups()
                if axis not in MOUSE_AXES:raise ValueError(f'Unmapped input axis: {axis}')
                slots.append(('-' if invert=='true' else '')+axis)
        evidence.append({'source_input':name,'index':index,'keys':slots})
        if name not in LEFT_OUT:bindings.append({'action':name,'keys':slots})
    if keys:raise ValueError('Input default outside the enum')
    return bindings, evidence


def car_driving(car):
    """Per-car startup gear and transmission flags of one carscript."""
    initial=car['gear']+1
    if not 0<=initial<len(car['gears']):raise ValueError('Driving startup gear outside source gear list')
    return {'initial_gear':initial,'force_automatic':bool(car['forceAutomaticTransmission']),'play_shift_sounds_in_automatic':False}


def road_wheels(source, reader, car):
    """WheelCollider identities that carscript drives, steers or brakes."""
    return {key(source.resolve(reader,p)) for field in ('WHDriven','WHSteer','WHBrakeOnly') for p in car[field]}


def hub_record(hubs, car):
    """mainscript.kerekagyProperties[carscript.kerekagyType]."""
    index=car['kerekagyType']
    if not 0<=index<len(hubs):raise ValueError('Driving hub selection outside source properties')
    hub=hubs[index]
    return {'radius_m':hub['radius'],'mass_kg':hub['mass'],'forward_stiffness':hub['slip1'],'sideways_stiffness':hub['slip2']}


def wheel_graphics(source, nodes, road):
    """Active wheelgraphicsscript per road wheel identity, from hierarchy records."""
    graphics={}
    for node in nodes:
        if not node['active']:continue
        for component in node['components']:
            if component.type.name!='MonoBehaviour' or source.class_name(component)!='wheelgraphicsscript':continue
            raw=source.read(component);wheel=source.resolve(component,raw['W'])
            if wheel is None:raise ValueError('Unresolved driving wheel collider')
            if key(wheel) in road:
                if key(wheel) in graphics:raise ValueError('Duplicate driving wheel graphics owner')
                graphics[key(wheel)]=(component,raw)
    return graphics


def recover_driving(source):
    """Resolve the shared driving controls; fail on missing records or links."""
    bodies, methods, minimum, text = checked_methods(source)
    constants = lever_constants(text)
    drive = bodies['public void DriveStuff()']
    rates = [float(value) for value in re.findall(r'B(?:Gas|Brake|Clutch) = Mathf.MoveTowards\([^;]+Time.deltaTime \* ([\d.]+)f\);',drive)]
    if len(rates)!=3 or len(set(rates))!=1:raise ValueError('Driving pedal rates differ')
    settings_reader = source.get('level0',16865)
    if source.class_name(settings_reader)!='settingsscript':raise ValueError('Driving settings owner differs')
    settings = source.read(settings_reader)
    toggles=[]
    for index in (0,1,7):
        reader=source.resolve(settings_reader,settings['tabControls']['toggles'][index])
        if reader is None or source.class_name(reader)!='togglesetting':raise ValueError('Unresolved driving assist toggle')
        value=source.read(reader)
        children=[source.resolve(reader,p) for p in value['childToggles']]
        if any(r is None for r in children):raise ValueError('Unresolved driving assist child')
        toggle=source.resolve(reader,value['toggle'])
        if toggle is None:raise ValueError('Unresolved driving toggle state')
        toggles.append({'source':source_id(reader),'default':bool(value['def']),
            'serialized_is_on':bool(source.read(toggle)['m_IsOn']),
            'reset_child_on_enable':bool(value['resetChildOnEnable']),
            'child_sources':[source_id(r) for r in children]})
    if toggles[0]['child_sources'] != [toggles[1]['source']]:raise ValueError('Driving assist dependency differs')
    if 'OnChangedAnything();' not in bodies['public void ResetCustomRadioPath()'] or 'RefreshSettings();' not in bodies['public void OnChangedAnything()']:
        raise ValueError('Driving startup refresh chain differs')
    refresh=bodies['public void RefreshSettings()']
    if 'S.BAutoShift = tabControls.toggles[1].toggle.isOn && S.BAutoClutch;' not in refresh:
        raise ValueError('Driving assist refresh differs')
    if any(line not in drive for line in MOUSE_DRIVE_LINES):raise ValueError('Mouse driving source differs')
    mouse_x,mouse_x_slider=slider_range(source,settings_reader,settings,'tabControls',8)
    mouse_y,mouse_y_slider=slider_range(source,settings_reader,settings,'tabControls',9)
    bindings,binding_evidence=recover_bindings(text,bodies['private void InitDefault()'])
    if any(line not in drive for line in STEERING_WHEEL_LINES):raise ValueError('Steering wheel source differs')
    steering_wheel,steering_wheel_evidence=recover_steering_wheel(source,settings_reader,settings,refresh)
    global_record={'source':source_id(settings_reader),'pedal_rate':rates[0],'force_manual':toggles[2]['default'],
        'min_motor_torque_nm':minimum,'default_assists':{'automatic_clutch':toggles[0]['default'],
            'automatic_gears':toggles[1]['default'] and toggles[0]['default']},'bindings':bindings,
        'mouse_drive':{'sensitivity_x':mouse_x,'sensitivity_y':mouse_y},'steering_wheel':steering_wheel,
        'lever':{'seconds':constants['seconds'],'stages':[constants['stage1'],constants['stage2']]}}
    return {'global':global_record,
        'evidence':{'assembly_sha256':ASSEMBLY_SHA256,'decompiled_sha256':DECOMPILED_SHA256,
            'methods':methods,'assist_toggles':toggles,'bindings':binding_evidence,
            'mouse_drive_sources':[source_id(mouse_x_slider),source_id(mouse_y_slider)],'steering_wheel_controls':steering_wheel_evidence,'serialized_assists':{name:bool(settings['S'][name]) for name in ('BAutoClutch','BAutoShift','BForceManual')},
            'startup':'FStart -> LoadSettings -> ResetSettings(true) -> DoReset (one yielded frame) -> tab.PressedReset -> togglesetting.ResetToDefault -> ResetCustomRadioPath -> OnChangedAnything -> RefreshSettings. The settled fresh reset applies true/true; serialized S contains true/false. Static source evidence, without a native startup measurement.',
            'shift_sounds':'Private bool has no initializer or assignment; CLR startup value is false.'}}
