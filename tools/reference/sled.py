"""Source szankoScript (sled) records of world prototypes."""
from .gltf import convert_point
from .source import key, source_id, vector
from .wheels import wheel_collider_record

# szankoScript field to record field.
VALUES = {'slideAboveMass':'slide_above_mass','moveForce':'move_force','maxSpeed':'max_speed',
    'velocityForwardLerp':'velocity_forward_lerp','turnForce':'turn_force','angularVelocityLerp':'angular_velocity_lerp',
    'soundMinSpeed':'sound_min_speed','soundMaxSpeed':'sound_max_speed','soundMaxVolume':'sound_max_volume',
    'soundPitchMinSpeed':'sound_pitch_min_speed','soundPitchMaxSpeed':'sound_pitch_max_speed',
    'soundMinPitch':'sound_min_pitch','soundMaxPitch':'sound_max_pitch'}
LINKS = ('source','rb','seats','wheels','cols')


def sled_values(data):
    """Map the szankoScript numbers. Stop on a missing or nonnumeric value."""
    for field in (*VALUES, *LINKS):
        if field not in data:raise ValueError(f'szankoScript {field}: missing value')
    values={}
    for field,name in VALUES.items():
        if type(data[field]) not in (int,float):raise ValueError(f'szankoScript {field}: missing value')
        values[name]=float(data[field])
    return values


def centers_of_mass(source, hierarchy):
    """setcenterofmass.Start sets R.centerOfMass = T.localPosition. Keys are Rigidbody identities."""
    centers={}
    for node in hierarchy:
        # Start never runs on an inactive node or a disabled component.
        if not node['active']:continue
        for component in node['components']:
            if component.type.name!='MonoBehaviour' or source.class_name(component)!='setcenterofmass':continue
            data=source.read(component)
            if not data['m_Enabled']:continue
            body=source.resolve(component,data['R'])
            if body is None:
                body=next((c for c in node['components'] if c.type.name=='Rigidbody'),None)
            target=source.resolve(component,data['T'])
            if body is None or target is None:continue
            if key(body) in centers:raise ValueError(f'{key(component)}: second setcenterofmass on one Rigidbody')
            centers[key(body)]=list(convert_point(vector(source.read(target)['m_LocalPosition'])))
    return centers


def recover_sled(source, hierarchy, mapping, bodies, colliders):
    """The first szankoScript of a world prototype, or None. `mapping` maps a Transform identity to its node."""
    scripts=[(n,c) for n in hierarchy for c in n['components']
        if c.type.name=='MonoBehaviour' and source.class_name(c)=='szankoScript']
    if not scripts:return None
    if len(scripts)>1:raise ValueError('World prototype has more than one szankoScript')
    node,script=scripts[0]
    data=source.read(script)
    identity=key(script)
    record={'source':source_id(script),**sled_values(data)}
    def required(pointer, field):
        target=source.resolve(script,pointer)
        if target is None:raise ValueError(f'{identity} {field}: missing value')
        return target
    rb=source.resolve(script,data['rb']) or next((c for c in node['components'] if c.type.name=='Rigidbody'),None)
    if rb is None:raise ValueError(f'{identity} rb: missing value')
    body=next((i for i,b in enumerate(bodies) if b['source']==source_id(rb)),None)
    if body is None:raise ValueError(f'{identity} rb: Rigidbody outside its prototype')
    if bodies[body]['center_of_mass'] is None:raise ValueError(f'{identity}: missing setcenterofmass')
    record['body']=body
    record['wheels']=[]
    for i,pointer in enumerate(data['wheels']):
        wheel=required(pointer,f'wheels[{i}]')
        if wheel.type.name!='WheelCollider':raise ValueError(f'{identity} wheels[{i}]: not a WheelCollider')
        transform=source.transform_for(source.resolve(wheel,source.read(wheel)['m_GameObject']))
        if key(transform) not in mapping:raise ValueError(f'{identity} wheels[{i}]: wheel outside its prototype')
        record['wheels'].append({'source':source_id(wheel),'node':mapping[key(transform)],
            'collider':wheel_collider_record(source.read(wheel))})
    owned={key(c) for n in hierarchy for c in n['components']}
    known={(c['source']['file'],c['source']['path_id']) for c in colliders}
    record['colliders']=[]
    for i,pointer in enumerate(data['cols']):
        collider=required(pointer,f'cols[{i}]')
        if key(collider) not in known:raise ValueError(f'{identity} cols[{i}]: collider outside its prototype')
        record['colliders'].append(source_id(collider))
    record['seats']=[]
    for i,pointer in enumerate(data['seats']):
        seat=required(pointer,f'seats[{i}]')
        if key(seat) not in owned or seat.type.name!='MonoBehaviour' or source.class_name(seat)!='seatscript':raise ValueError(f'{identity} seats[{i}]: not a seatscript')
        record['seats'].append(source_id(seat))
    if not record['wheels'] or not record['seats']:raise ValueError(f'{identity}: missing wheels or seats')
    audio_source=required(data['source'],'source')
    if key(audio_source) not in owned or audio_source.type.name!='AudioSource':raise ValueError(f'{identity} source: AudioSource outside its prototype')
    clip=source.resolve(audio_source,source.read(audio_source)['m_audioClip'])
    if clip is None:raise ValueError(f'{identity} source: missing clip')
    # audio.recover_audio replaces the clip source with its audio clip id.
    record['audio_source']=source_id(audio_source)
    record['clip']=source_id(clip)
    return record
