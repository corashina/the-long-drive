"""Vehicle records of car world prototypes and engine and radiator records of part nodes.

Ids are local to one world prototype: node, body and collider indices.
"""
import math
from .source import key, source_id
from .wheels import wheel_collider_record
from .driving import car_driving, road_wheels, hub_record, wheel_graphics, recover_lever
from .instruments import instrument_record, gauge_limits, Meters
from .lighting import vehicle_light_links, vehicle_states
from .particles import recover_coolant_steam,recover_exhaust
from .wheel_effects import recover_wheel_effects, effect_gains
from .plates import prototype_plates
from .render_lighting import probe_settings

ENGINE_FIELDS = {'start_chance':'startChance','starter_rpm':'starterRpm','starter_rpm_lerp':'starterRpmLerp',
    'idle_rpm':'alapRpm','max_rpm':'maxRpm','shutdown_rpm':'shutdownRpm','max_torque_nm':'maxNm',
    'consumption_modifier':'consumptionM','oil_consumption_modifier':'OilConsumptionModifier',
    'engine_brake_modifier':'motorBrakeModifier','rpm_change_modifier':'rpmChangeModifier',
    'engine_start_time_modifier':'engineStartTimeModifier','heat_gain_min':'engineHeatGainMin',
    'heat_gain_max':'engineHeatGainMax','heat_loss':'engineHeatLoss','initial_temperature':'temp',
    'heat_loss_speed_modifier':'engineHeatLossSpeedModifier','max_rpm_difference':'maxRpmDifference',
    'torque_modifier':'modifier','oil_fluid':'Oilfluid','auto_shift_clutch_rpm':'autoShiftClutchRpm',
    'auto_shift_clutch_rpm_range':'autoShiftClutchRpmRange',
    'oil_toleration_min':'oilTolerationMin','oil_toleration_max':'oilTolerationMax'}
RADIATOR_FIELDS = {'fan_temperature_on':'fanOnTemp','fan_temperature_off':'fanOffTemp','max_value':'maxValue',
    'max_fan_value':'maxFanValue','max_fan_speed':'maxFanSpeed','max_fluid_loss':'maxFluidLoss'}


def smoke_rate(engine: dict) -> float:
    """Read enginescript.defSmokeRate."""
    rate = engine['defSmokeRate']
    if not math.isfinite(rate) or rate < 0:
        raise ValueError('Invalid engine smoke rate')
    return rate


def curve_record(curve: dict) -> dict:
    """Unity AnimationCurve keys and wrap modes."""
    return {'keys':[{'time':k['time'],'value':k['value'],'in_tangent':k['inSlope'],'out_tangent':k['outSlope'],
        'in_weight':k['inWeight'],'out_weight':k['outWeight'],'weighted_mode':k['weightedMode']} for k in curve['m_Curve']],
        'pre_wrap_mode':curve['m_PreInfinity'],'post_wrap_mode':curve['m_PostInfinity']}


BALANCE_FIELDS = {'lean_min_speed':'balanceLeanMinSpeed','lean_max_speed':'balanceLeanMaxSpeed',
    'lean_min_angle':'balanceLeanMinAngle','lean_max_angle':'balanceLeanMaxAngle','lean_angle_lerp':'balanceLeanAngleLerp',
    'steer_angle_min':'balanceSteerAngleMin','steer_angle_max':'balanceSteerAngleMax',
    'steer_angle_speed_min':'balanceSteerAngleSpeedMin','steer_angle_speed_max':'balanceSteerAngleSpeedMax',
    'steer_angle_lerp':'balanceSteerAngleLerp','p':'balanceP','i':'balanceI','d':'balanceD',
    'set_angle':'balanceSetAngle','min_angle':'balanceMinAngle','max_angle':'balanceMaxAngle','max_force':'balanceMaxForce'}


def balance_record(car: dict) -> dict | None:
    """carscript balance values (source Balance). None when the car is not balanced."""
    if not car['balanced']:return None
    return {'lean':bool(car['balanceLean']),'lean_curve':curve_record(car['balanceLeanCurve']),
        'use_angle_limit':bool(car['balanceUseAngleLimit']),**{field:car[unity] for field,unity in BALANCE_FIELDS.items()}}


def pedal_record(source, proto, car_reader, car, wheels, seats) -> dict | None:
    """carscript.bikepedal (bikepedalscript). None for a null link.

    `driven_wheels` index `wheels`. `seat` indexes `seats`. `driven_slots` are partslotscript
    prototype node ids, as wheel `slot`: a slot with a part is a mounted driven wheel.
    `pedals`, `pedals_inverse`, `feet_holders` and `axes` are prototype node ids.
    """
    reader=source.resolve(car_reader,car['bikepedal'])
    if reader is None:return None
    if source.class_name(reader)!='bikepedalscript':raise ValueError(f'{key(car_reader)} bikepedal is not a bikepedalscript')
    proto.node(reader)
    value=source.read(reader)
    def targets(field):
        found=[source.resolve(reader,p) for p in value[field]]
        if any(r is None for r in found):raise ValueError(f'{key(reader)} {field} has a null link')
        return found
    wheel_index={(w['source']['file'],w['source']['path_id']):i for i,w in enumerate(wheels)}
    driven=[wheel_index.get(key(w)) for w in targets('WHDriven')]
    if None in driven:raise ValueError(f'{key(reader)} driven wheel is not a car wheel')
    slots=targets('DrivenWheelSlots')
    if any(source.class_name(s)!='partslotscript' for s in slots):raise ValueError(f'{key(reader)} driven wheel slot is not a partslotscript')
    seat=source.resolve(reader,value['seat'])
    seat_index=next((i for i,s in enumerate(seats) if seat is not None and s['source']==source_id(seat)),None)
    if seat_index is None:raise ValueError(f'{key(reader)} seat is not a primary car seat')
    nodes=lambda field:[proto.node(r) for r in targets(field)]
    return {'source':source_id(reader),'driven_wheels':driven,'driven_slots':[proto.node(s) for s in slots],'seat':seat_index,
        'pedals':nodes('pedals'),'pedals_inverse':nodes('pedalsInverse'),'feet_holders':nodes('pedalFeetHolder'),
        'axes':nodes('Axis'),'axis_index':list(value['pedalAxisIndex']),'ratio':value['ratio'],
        'speed_modifier':value['pedalingSpeedModifier'],'input_debounce':value['pedalInputDebounce'],
        'gas_tap':value['gas_tap'],'player_use':bool(value['playerUse'])}


def engine_fields(engine: dict) -> dict:
    """enginescript values. The car and mounted parts add tanks, starter delay and electronics."""
    return {**{field:engine[unity] for field,unity in ENGINE_FIELDS.items()},
        'starting_push_force':engine['maxNm']*9.,
        'optimal_temperature':[engine[k] for k in ('minOptimalTemp1','maxOptimalTemp1','minOptimalTemp2','maxOptimalTemp2')],
        'fuel_consumption':[sum(f['amount'] for f in engine['FuelConsumption']['fluids'] if f['type']==i) for i in range(6)],
        'two_stroke':bool(engine['twostroke']),'no_overheat':bool(engine['noOverHeat']),'kickstart':bool(engine['kickstart']),
        'start_chance_interval_seconds':1.,'smoke_rate':smoke_rate(engine),
        'torque_curve':curve_record(engine['torqueCurve'])}


def radiator_fields(radiator: dict) -> dict:
    """coolantTankscript values. Links to the fan and the coolant tank are added by the caller."""
    return {'thermostat_range':[radiator['minThermostatTemp'],radiator['maxThermostatTemp']],
        'fan_on_when_engine_runs':bool(radiator['fanOnWhenEngineRuns']),
        **{field:radiator[unity] for field,unity in RADIATOR_FIELDS.items()}}


def vehicle_globals(source) -> dict:
    """Shared engine values of mainscript. The world record holds them once for every car."""
    reader=source.get('level1',64919);main=source.read(reader)
    return {'source':source_id(reader),'fuel_consumption':main['defConsumption'],'oil_consumption':main['defOilConsumption'],
        'rpm_change':2000.,'car_temperature_bounds':[main['minCarTemp'],main['maxCarTemp']],
        'coolant_evaporate_rate':main['coolantEvaporateRate'],'temperature_difference_modifier':main['tempDifferenceModifier'],
        'temperature_move_modifier':main['tempChangeMoveModifier'],'max_car_velocity':main['maxCarVelocity'],
        'outside_temperature_modifier':main['outsideTempM'],'min_steam_temperature':main['minSteamTemp']}


def dry_mass(source, components) -> float:
    """massScript mass of one node. The runtime adds tank fluids at 1 kg per liter."""
    return sum(source.read(r)['mass'] for r in components
        if r.type.name=='MonoBehaviour' and source.class_name(r)=='massScript')


# carscript player body frames while driving.
DRIVE_FRAMES=(('left_hand','LHDrivePos'),('right_hand','RHDrivePos'),('left_leg','LLDrivePos'),('right_leg','RLDrivePos'))
# carscript hand and foot targets of the car controls.
REACH_NODES=(('horn','HornHandPos'),('lights','LightHandPos'),('ignition','IgnitionHandPos'),('handbrake','HandBrakeHandPos'),
    ('transmission','TransmissionHandPos'),('gas','GasLegPos'),('brake','BrakeLegPos'),('clutch','ClutchLegPos'),
    ('default_left_leg','DefLLPos'),('default_right_leg','DefRLPos'))
# carscript limb flags. carscript.Start inverts the first five when Vinv is set.
REACH_FLAGS=(('leg_shift','VLegShift'),('hand_gas','VHandGas'),('hand_brake','VHandBrake'),('hand_clutch','VHandClutch'),
    ('leg_handbrake','VLegHandbrake'),('leg_ignition','VLegIgnition'),('no_clutch','VNoClutch'),
    ('dont_hold_ignition_when_start','VDontHoldIgnitionWhenStart'))


def reach_record(proto, car_reader, car) -> dict:
    """carscript control targets (null when unset) and limb flags after the Start Vinv inversion."""
    flags={name:bool(car[field])!=(bool(car['Vinv']) and i<5) for i,(name,field) in enumerate(REACH_FLAGS)}
    return {**{name:proto.link(car_reader,car[field]) for name,field in REACH_NODES},**flags}


def gear_stick_record(proto, engine_reader, engine) -> dict | None:
    """enginescript.gearstick hand targets, or None when the engine has no gear stick."""
    stick=proto.source.resolve(engine_reader,engine['gearstick'])
    if stick is None:return None
    value=proto.source.read(stick)
    return {'source':source_id(stick),'transmission':proto.link(stick,value['TransmissionHandPos']),
        'kick_ignition':proto.link(stick,value['KickNitionHandPos'])}


def seat_fields(source, reader, value, car_reader) -> dict:
    """Driver roles of a primary seat in the car of `car_reader`."""
    car=source.resolve(reader,value['Car'])
    own=car is not None and key(car)==key(car_reader)
    return {'driver':bool(value['driverSeat0']) and own,'driving2':bool(value['driverSeat1']) and own}


class Prototype:
    """Resolve source links to node indices of one world prototype hierarchy."""
    def __init__(self, source, hierarchy):
        self.source=source;self.hierarchy=hierarchy
        self.nodes={key(n['reader']):i for i,n in enumerate(hierarchy)}

    def scripts(self, name, active_only=False):
        """(node index, component) of each MonoBehaviour of class `name`."""
        source=self.source
        return [(i,c) for i,n in enumerate(self.hierarchy) if n['active'] or not active_only for c in n['components']
            if c.type.name=='MonoBehaviour' and source.read(c)['m_Script']['m_PathID'] and source.class_name(c)==name]

    def node(self, reader):
        source=self.source
        if reader.type.name not in ('Transform','RectTransform'):
            if reader.type.name!='GameObject':reader=source.resolve(reader,source.read(reader)['m_GameObject'])
            reader=source.transform_for(reader)
        if key(reader) not in self.nodes:raise ValueError(f'{key(reader)} link outside the prototype')
        return self.nodes[key(reader)]

    def link(self, owner, pointer):
        """Node index of a pointer target, or None for a null pointer."""
        reader=self.source.resolve(owner,pointer)
        return None if reader is None else self.node(reader)

    def links(self, owner, pointers):
        return [self.node(r) for p in pointers if (r:=self.source.resolve(owner,p))]


def blood_record(source, proto, blood):
    """carscript.blood washtriggerscript. GotDirty and GotClean set its node and objects active."""
    if blood is None:return None
    d=source.read(blood)
    # The port has no destroyed or start-disabled car blood trigger.
    if d['destroyOnRepair'] or d['chanceToStartDisabled']>0:raise ValueError(f'{key(blood)} car blood trigger differs')
    return {'node':proto.node(blood),'objects':proto.links(blood,d['objects']),'permission':d['CleanProperty'],
        'never_repair':bool(d['neverRepair']),'on':bool(d['ON'])}


def gauge_records(source, proto, asset, material_indices) -> list:
    """Gauge part records: each meterscript on a part node that is not a built-in car meter."""
    source=proto.source
    built_in={key(source.resolve(c,p)) for _,c in proto.scripts('carscript') for p in source.read(c)['meters'] if p['m_PathID']}
    build=Meters(source,proto.nodes,asset,material_indices)
    records=[]
    for node,reader in proto.scripts('meterscript'):
        if key(reader) in built_in:continue
        # The meter belongs to the nearest partscript node at or above it.
        current=proto.hierarchy[node]['reader'];part=False
        while current is not None and key(current) in proto.nodes:
            if any(c.type.name=='MonoBehaviour' and source.read(c)['m_Script']['m_PathID'] and source.class_name(c)=='partscript'
                   for c in proto.hierarchy[proto.nodes[key(current)]]['components']):
                part=True;break
            current=source.resolve(current,source.read(current)['m_Father'])
        if part:records.append(build.gauge(reader,node))
    return records


def kmh_meters(source, proto, car_reader, car) -> list:
    """carscript.kmhmeters: gauge slots and how their meters read."""
    rows=[]
    for k in car['kmhmeters']:
        slots=[source.resolve(car_reader,p) for p in k['slots'] if p['m_PathID']]
        for slot in slots:
            if source.class_name(slot)!='partslotscript':raise ValueError(f'{key(car_reader)} kmhmeters slot is not a partslotscript')
            proto.node(slot)
        rows.append({'force':bool(k['force']),'modifier':k['modifier'],'types':list(k['types']),'slots':[source_id(s) for s in slots]})
    return rows


def selector_engines(source, proto):
    """Each engine part that a randomTypeSelector mounts (Car06) must be a prototype node, so it has an engine record."""
    for _,reader in proto.scripts('randomTypeSelector'):
        for row in source.read(reader)['tipusok']:
            for part in filter(None,(source.resolve(reader,p) for p in row.get('parts') or [])):
                owner=part if part.type.name=='GameObject' else source.resolve(part,source.read(part)['m_GameObject'])
                nodes=source.hierarchy(source.transform_for(owner))
                if any(c.type.name=='MonoBehaviour' and source.read(c)['m_Script']['m_PathID'] and source.class_name(c)=='enginescript' for n in nodes for c in n['components']):
                    proto.node(owner)

def check_collider_bakes(source, proto):
    """colliderscaleapplyscript (Car10k) is an editor bake helper. With `run` off it does nothing at runtime."""
    for _,reader in proto.scripts('colliderscaleapplyscript'):
        if source.read(reader)['run']:raise ValueError(f'{key(reader)} colliderscaleapplyscript.run is set: its bake is not exported')


def fuel_tank_slot(source, proto, car_reader, car):
    """carscript.tankslot. Its part holds the fuel tank when the root has no tankscript."""
    slot=source.resolve(car_reader,car['tankslot'])
    if slot is None:return None
    if source.class_name(slot)!='partslotscript':raise ValueError(f'{key(car_reader)} tankslot is not a partslotscript')
    proto.node(slot)
    return source_id(slot)


def part_records(source, hierarchy, asset, material_indices) -> tuple[list, list, list]:
    """Engine, radiator and gauge records of the part nodes of one prototype."""
    proto=Prototype(source,hierarchy)
    selector_engines(source,proto)
    engines=[{'source':source_id(reader),'node':node,'oil_tank':proto.link(reader,value['T']),
        'gear_stick':gear_stick_record(proto,reader,value),**engine_fields(value)}
        for node,reader in proto.scripts('enginescript') for value in (source.read(reader),)]
    radiators=[{'source':source_id(reader),'node':node,'coolant_tank':proto.link(reader,value['coolant']),
        'fan_node':proto.link(reader,value['Fan']),**radiator_fields(value)}
        for node,reader in proto.scripts('coolantTankscript') for value in (source.read(reader),)]
    return engines,radiators,gauge_records(source,proto,asset,material_indices)


def material_link(source, owner, pointer, asset, material_indices):
    reader=source.resolve(owner,pointer)
    if reader is None:return None
    identity=f'{reader.assets_file.name}:{reader.path_id}'
    if identity not in material_indices:raise ValueError(f'Missing vehicle material: {identity}')
    return {'source':source_id(reader),'asset':asset,'gltf_material':material_indices[identity]}


def light_records(source, proto, car_reader, car, asset, material_indices):
    """headlightscript and rearlights controllers and the light switch of one car or trailer prototype."""
    scripts=[c for n in proto.hierarchy for c in n['components'] if c.type.name=='MonoBehaviour' and source.read(c)['m_Script']['m_PathID']]
    # A trailer has no carscript: no cabin routes, no switch and no electronics.
    routes,switches=vehicle_light_links(source,scripts,car_reader) if car_reader else ({},[])
    if len(switches)>1:raise ValueError(f'{key(car_reader)} has more than one light switch')
    def lights(owner,pointers):
        result=[]
        for p in pointers:
            r=source.resolve(owner,p)
            if not r:continue
            candidates=[r] if r.type.name=='Light' else [c for n in source.hierarchy(source.transform_for(r)) for c in n['components'] if c.type.name=='Light']
            result.extend(candidates)
        return result
    controllers=[]
    for node,reader in [*proto.scripts('headlightscript'),*proto.scripts('rearlights')]:
        f=source.read(reader)
        kind='rear' if source.class_name(reader)=='rearlights' else 'cabin' if key(reader) in routes else 'headlight'
        def state(signal,intensity,ls,rs,on,off,broken=False,working=(),not_working=(),dont_overwrite=False,reader=reader):
            return {'signal':signal,'lights':[proto.node(c) for c in lights(reader,ls)],'intensity':intensity,
                'renderers':proto.links(reader,rs),'on_material':material_link(source,reader,on,asset,material_indices),
                'off_material':material_link(source,reader,off,asset,material_indices),'broken':bool(broken),
                'working_nodes':proto.links(reader,working),'broken_nodes':proto.links(reader,not_working),'dont_overwrite':bool(dont_overwrite)}
        def intensity(pointers,reader=reader):
            found=lights(reader,pointers)
            return source.read(found[0])['m_Intensity'] if found else 0.
        if kind=='rear':owner,electronics=(car_reader,car['electronics']) if car_reader else (reader,{'m_PathID':0})
        else:owner,electronics=reader,f.get('electronics',{'m_PathID':0})
        record={'source':source_id(reader),'kind':kind,'node':node,
            'electronics':proto.link(owner,electronics),
            'startup_on':bool(f.get('ON',False)),'current_state':f.get('currentState',0),'requested_state':f.get('switchToState',0),
            'energy':f.get('energy',1.),'energy_move':f.get('energyMove',0.),'enabled':bool(f['m_Enabled']),
            'reverse_switch':bool(f.get('reverseSwitch',False)),'states':vehicle_states(f,kind,state,intensity),'cabin':None}
        if kind=='cabin':
            route=routes[key(reader)];rf=source.read(route)
            switch=source.resolve(route,rf['turnOverride'])
            record['cabin']={'source':source_id(route),'switch_node':proto.node(switch),'modes':rf['turnOverrideT'],
                'startup_mode':source.read(switch)['currentTurnState'],'door_slots':proto.links(route,rf['ISlotUsableV1To1']),
                'threshold':rf['iUsableV1To1Limit'],'include_children':bool(rf['includeUsablesChild1To1']),
                'missing_slot_is_same':bool(rf['ISlotUsableV1To1NoPartInSlotIsSame'])}
        controllers.append(record)
    switch=None
    if switches:
        switch={'usable':proto.node(switches[0]),'nodes':proto.links(car_reader,car['LightSwitch']),
            'state_nodes':[proto.links(car_reader,car[k]) for k in ('LightOff','LightLow','LightHigh')]}
    return controllers,switch


def probe_record(source, proto):
    """The car's realtime ReflectionProbe, or None. Its node is a prototype node index."""
    probes=[(i,c) for i,n in enumerate(proto.hierarchy) for c in n['components'] if c.type.name=='ReflectionProbe']
    if len(probes)>1:raise ValueError(f'{key(proto.hierarchy[0]["reader"])} has more than one reflection probe')
    if not probes:return None
    node,reader=probes[0]
    return {'source':source_id(reader),'node':node,**probe_settings(key(reader),source.read(reader))}


def mirror_records(source, proto, asset, material_indices):
    """Live mirrors: each mirrorscript with a renderer and a camera. Nodes are prototype node indices."""
    main=source.get('level1',64919)
    material=source.read(main)['defMirrorMaterial']
    records=[]
    for _,reader in proto.scripts('mirrorscript'):
        m=source.read(reader)['M']
        if not (m['R']['m_PathID'] and m['C']['m_PathID']):continue
        camera=source.resolve(reader,m['C']);c=source.read(camera)
        # FStart sets T to the script transform when it is null. MirrorsUpdate moves CT.
        plane=proto.link(reader,m['T']) if m['T']['m_PathID'] else proto.node(reader)
        if not m['CT']['m_PathID'] or key(source.transform_for(source.resolve(camera,c['m_GameObject'])))!=key(source.resolve(reader,m['CT'])):
            raise ValueError(f'{key(reader)} mirror CT is not the camera transform')
        if c['orthographic'] or c['m_NormalizedViewPortRect']!={'height':1.0,'width':1.0,'x':0.0,'y':0.0} or c['m_ClearFlags']!=1:
            raise ValueError(f'{key(camera)} mirror camera differs')
        records.append({'source':source_id(reader),'renderer':proto.link(reader,m['R']),'plane':plane,
            'camera':proto.link(reader,m['CT']),'near':c['near clip plane'],'mask':c['m_CullingMask']['m_Bits'],
            'material':material_link(source,main,material,asset,material_indices)})
    return records


def vehicle_record(source, hierarchy, bodies, colliders, asset, material_indices, output):
    """`vehicle` record of a world prototype whose root has carscript, else None.

    `hierarchy` is source.hierarchy(root, activate_root=True). `bodies` and `colliders` are the
    prototype tables; their order gives the local ids.
    """
    proto=Prototype(source,hierarchy)
    cars=[c for i,c in proto.scripts('carscript') if i==0]
    if not cars:return None
    car_reader=cars[0];car=source.read(car_reader)
    check_collider_bakes(source,proto)
    rigidbody=source.resolve(car_reader,car['RB'])
    body=next((i for i,b in enumerate(bodies) if b['source']==source_id(rigidbody)),None) if rigidbody else None
    if body is None:raise ValueError(f'{key(car_reader)} chassis body outside the prototype')
    main=source.read(source.get('level1',64919))
    # Wheels.
    road=road_wheels(source,car_reader,car)
    if not road:raise ValueError(f'{key(car_reader)} has no wheels')
    roles={name:{key(source.resolve(car_reader,p)) for p in car[field]}
        for name,field in (('drive','WHDriven'),('steer','WHSteer'),('brake','WHBrakeOnly'),('handbrake','WHHandBrake'))}
    graphics=wheel_graphics(source,hierarchy,road)
    wheels=[]
    for identity in sorted(road):
        if identity not in graphics:raise ValueError(f'Wheel slot missing: {identity}')
        reader=source.get(*identity);graphic,raw=graphics[identity]
        linked=source.resolve(graphic,raw['car']);slot=source.resolve(graphic,raw['slot'])
        if linked is None or key(linked)!=key(car_reader) or slot is None:raise ValueError('Unresolved driving wheel hub link')
        wheels.append({'source':source_id(reader),'node':proto.node(reader),'collider':wheel_collider_record(source.read(reader)),
            'slot':proto.node(slot),'mount':proto.link(slot,source.read(slot)['T']) or proto.node(slot),
            **{name:identity in roles[name] for name in ('steer','drive','brake','handbrake')},
            'sideways_stiffness_modifier':raw['sideStiffnessModifier']})
    # Driver roles of primary seats. The runtime binds the seats from the prototype.
    seats=[]
    for node,reader in proto.scripts('seatscript',active_only=True):
        value=source.read(reader)
        if value['mainseat']['m_PathID']:continue
        seats.append({'source':source_id(reader),**seat_fields(source,reader,value,car_reader)})
    lights,light_switch=light_records(source,proto,car_reader,car,asset,material_indices)
    instruments=instrument_record(source,car_reader,proto.nodes,asset,material_indices)
    tanks=[c for c in hierarchy[0]['components'] if c.type.name=='MonoBehaviour' and source.class_name(c)=='tankscript']
    # carscript.DamageStuff: blood is the washtriggerscript that a hard crash makes dirty. SBlood01 plays there.
    blood=source.resolve(car_reader,car['blood'])
    if blood is not None and (blood.type.name!='MonoBehaviour' or source.class_name(blood)!='washtriggerscript'):raise ValueError(f'{key(car_reader)} blood is not washtriggerscript')
    blood_clip=source.resolve(car_reader,car['SBlood01'])
    if blood_clip is not None and blood_clip.type.name!='AudioClip':raise ValueError(f'{key(car_reader)} SBlood01 is not AudioClip')
    return {'source':source_id(car_reader),'body':body,'car_type':car['carType'],
        'colliders':[i for i,c in enumerate(colliders) if c['body']==body and c['enabled']],
        'mass_kg':sum(dry_mass(source,n['components']) for n in hierarchy if n['active']),
        'serialized_mass_kg':source.read(rigidbody)['m_Mass'],
        'fuel_tank':0 if tanks else None,'fuel_tank_slot':fuel_tank_slot(source,proto,car_reader,car),'electronics':proto.link(car_reader,car['electronics']),
        'starter_delay_seconds':car['startingTime'],'balance':balance_record(car),'driving':car_driving(car),'handbrake':bool(car['BhandBrake']),
        'hub':hub_record(main['kerekagyProperties'],car),'wheels':wheels,
        'gear_ratios':[g['ratio'] for g in car['gears']],'gear_free_run':[bool(g['freeRun']) for g in car['gears']],
        'final_drive_ratio':car['differentialRatio'],'max_steer_degrees':car['steerAngle'],
        'brake_torque_nm':car['brakePower'],'handbrake_torque_nm':car['maxHandBrake'],
        'wheel_slip':[main[k] for k in ('WheelSlip1Min','WheelSlip1Max','WheelSlip2Min','WheelSlip2Max')],
        'rim_slip':[main['WheelSlip1'],main['WheelSlip2']],
        'auto_shift_stop_speed':car['autoShiftStopSpeed'],'auto_shift_min_rpm':car['autoShiftMinRpm'],'auto_shift_max_rpm':car['autoShiftMaxRpm'],
        'dont_ignite_with_throttle_in_automatic':bool(car['dontIgniteWithThorttleInAutomatic']),'auto_move_min':0.3,
        'collision_gain':car['collisionSoundM'],
        'crash':{'multiplier':car['crashMultiplier'],'blood':car['crashBloodMultiplier'],'horn':car['crashHornMultiplier'],'fall_off':car['crashFallOffMultiplier']},
        'blood':blood_record(source,proto,blood),'blood_clip':None if blood_clip is None else source_id(blood_clip),
        'condition':proto.link(car_reader,car['condition']),
        'exhaust':recover_exhaust(source,car_reader,car,hierarchy[0]['reader'],car_reader.path_id,output),
        'coolant_steam':recover_coolant_steam(source,car_reader,car,hierarchy[0]['reader'],output),
        'wheel_effects':recover_wheel_effects(source,car_reader,car,hierarchy[0]['reader'],[source.get(*identity) for identity in sorted(road)],car_reader.path_id,output),
        'effect_gains':effect_gains(car),
        'lever':recover_lever(source,car_reader,car,hierarchy[0]['reader'],proto.nodes),
        # inputscript.DriveStuff uses the multiplier only when it is not zero.
        'key_steer_multiplier':car['keySteerSensMultiplier'],
        # carscript.GetPlayerInput replaces the multiplier from the car speed each step.
        'key_steer_speed':{'min_speed':car['keySensMinSpeed'],'max_speed':car['keySensMaxSpeed'],
            'min_multiplier':car['keySensMinSens'],'max_multiplier':car['keySensMaxSens']} if car['speedBasedKeyboardSens'] else None,
        # Source carscript.ledOilTestTwoStrokeDefault: the oil LED follows the two-stroke mix.
        'oil_led_two_stroke':bool(car['ledOilTestTwoStrokeDefault']),
        'seats':seats,'pedals':pedal_record(source,proto,car_reader,car,wheels,seats),
        'no_gas_while_pedaling':bool(car['noGasWhilePedaling']),'drive_frames':{n:proto.link(car_reader,car[f]) for n,f in DRIVE_FRAMES},
        'reach':reach_record(proto,car_reader,car),'lights':lights,'light_switch':light_switch,'instruments':instruments,
        'gauge_limits':gauge_limits(source,car_reader),'kmh_meters':kmh_meters(source,proto,car_reader,car),
        'plates':prototype_plates(source,proto),'probe':probe_record(source,proto),
        'mirrors':mirror_records(source,proto,asset,material_indices),
        'high_beams':[i for i,lamp in enumerate(instruments['lamps']) if lamp['signal']=='headlight2'] if instruments else []}
