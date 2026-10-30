"""Resolve source meterscript gauges: the built-in car meters and gauge parts."""
from .source import key,source_id,vector

def recover_material_switch(source, owner, fields, node, asset, material_indices):
    pointer = fields['R']
    if not pointer['m_PathID']:
        return {'kind': 'absent'}
    if source.resolve(owner, pointer) is None:
        raise ValueError('Missing instrument renderer')
    def material(pointer):
        reader = source.resolve(owner, pointer)
        if reader is None:
            raise ValueError('Missing instrument material')
        identity = f'{reader.assets_file.name}:{reader.path_id}'
        if identity not in material_indices:
            raise ValueError(f'Missing instrument material: {identity}')
        return {'source': source_id(reader), 'asset': asset,
                'gltf_material': material_indices[identity]}
    return {'kind': 'renderer', 'node': node(owner, pointer, True),
            'on': material(fields['OnM']), 'off': material(fields['OffM'])}

# meterscript szamlap names that carscript METERUPD2 moves by the driven distance.
DISTANCE_COUNTERS = ('kilometerszamlalo', 'napiszamlalo')

class Meters:
    """Build meterscript records. `nodes` maps Transform keys to prototype node ids."""
    def __init__(self, source, nodes, asset, material_indices):
        self.source=source;self.nodes=nodes;self.asset=asset;self.material_indices=material_indices

    def node(self, owner, ptr, renderer=False):
        source=self.source
        reader=source.resolve(owner,ptr)
        if reader is None:raise ValueError('Missing instrument node pointer')
        if renderer:reader=source.transform_for(source.resolve(reader,source.read(reader)['m_GameObject']))
        if key(reader) not in self.nodes:raise ValueError(f'Missing instrument node: {key(reader)}')
        return self.nodes[key(reader)]

    def material(self, owner, ptr):
        r=self.source.resolve(owner,ptr);identity=f'{r.assets_file.name}:{r.path_id}'
        return {'source':source_id(r),'asset':self.asset,'gltf_material':self.material_indices[identity]}

    def switch(self, owner, fields):
        return recover_material_switch(self.source,owner,fields,self.node,self.asset,self.material_indices)

    def counter(self, meter, count):
        """One szamlap and its forgoszamlap digits."""
        source=self.source
        r=source.resolve(meter,count['forgo']);d=source.read(r)
        digits=[]
        for digit in d['szamok']:
            axes=[digit['x'],digit['y'],digit['z']]
            digits.append({'node':self.node(r,digit['T']),'place':digit['helyiertekHatvany'],
                'axis':next((i for i,x in enumerate(axes) if x),0),'inv':bool(digit['inv']),'direct':bool(digit['direct']),
                'continuous':bool(digit['continous']),'initial_value':digit['alakiertek'],'initial_angle':digit['angle'],
                # forgoszamlap skips a null material and keeps the current one.
                'on':self.material(r,digit['MOn']) if digit['MOn']['m_PathID'] else None,
                'off':self.material(r,digit['MOff']) if digit['MOff']['m_PathID'] else None})
        return {'source':source_id(r),'initial_value':count['value'],'base':d['szamrendszer'],'decimals':d['tizedesjegyek'],
            'use_multiplier':bool(d['useMultiplier']),'multiplier':d['multiplier'],'digits':digits}

    def meter(self, meter, md):
        """Backlight, needles and lamps of one meterscript."""
        source=self.source
        needles=[]
        for entry in md['mutatok']:
            r=source.resolve(meter,entry['mutato']);d=source.read(r)
            axes=[d['X'],d['Y'],d['Z']]
            if sum(bool(x) for x in axes)!=1:raise ValueError(f'Invalid needle axes: {key(r)}')
            needles.append({'source':source_id(r),'node':self.node(r,d['mutato']),'signal':entry['tipus'],
                'min_value':d['minValue'],'max_value':d['maxValue'],'min_angle':d['minAngle'],'max_angle':d['maxAngle'],
                'axis':axes.index(1),'rest':[d['x'],d['y'],d['z']],'offset':d['inputValueOffset'],'inv':bool(d['inv']),
                'value_smoothing':8*(d['lerpSpeedMultiplier'] or 1),'signal_smoothing':5 if entry['tipus'] in ('fuel','temp') else 0,
                'no_value_lerp':bool(d['noValueLerp']),'lerp_angle':bool(d['lerpAngle']),
                'backlight':self.switch(r,d)})
        lamps=[]
        for ptr in md['leds']:
            r=source.resolve(meter,ptr);d=source.read(r)
            for led in d['leds']:
                lamps.append({'source':source_id(r),'node':self.node(r,led['r'],True),'signal':d['vname'],
                    'on':self.material(r,led['on']),'off':self.material(r,led['off']),'min_value':led['minValue'],
                    'max_value':led['maxValue'],'stay_on':bool(led['stayOnAfterMaximum'])})
        return {'backlight':self.switch(meter,md),'needles':needles,'lamps':lamps}

    def gauge(self, meter, node):
        """Gauge part record: a meterscript that carscript.kmhmeters updates through a slot."""
        md=self.source.read(meter)
        return {'source':source_id(meter),'node':node,**self.meter(meter,md),
            'counters':[self.counter(meter,c) for c in md['szamlapok'] if c['tipus'] in DISTANCE_COUNTERS]}


def instrument_record(source,car_reader,nodes,asset,material_indices):
    """Built-in meters of the car (carscript.meters). None without a meter."""
    cd=source.read(car_reader)
    meters=[(r,source.read(r)) for p in cd['meters'] if (r:=source.resolve(car_reader,p))]
    if not meters:return None
    build=Meters(source,nodes,asset,material_indices)
    records=[build.meter(meter,md) for meter,md in meters]
    odometers=[build.counter(meter,md['szamlapok'][0]) for meter,md in meters if md['szamlapok']]
    if len(odometers)>1:raise ValueError(f'Car has more than one odometer: {key(car_reader)}')
    return {'backlights':[r['backlight'] for r in records],
        'needles':[n for r in records for n in r['needles']],'lamps':[l for r in records for l in r['lamps']],
        'odometer':odometers[0] if odometers else None}


def gauge_limits(source, car_reader):
    """carscript lamp thresholds and the electronics output that gauges read."""
    cd=source.read(car_reader)
    electronic=source.resolve(car_reader,cd['electronics'])
    if electronic is None:electronic_output=1.
    else:
        e=source.read(electronic);maximum=e['overRideMaxJamValue'] if e['overRideMaxJamValue']>0 else 0.3
        electronic_output=e['maxOutput']*(1.-max(0.,min(1.,e['jamValue']/maximum)))
    return {'electronics_output':electronic_output,'electronics_off_threshold':0.2,
        'low_fuel':cd['lowFuelValue'],'low_oil':cd['lowOilValue'],
        'oil_test_rpm':cd['ledOilTestRpm'],'battery_test_rpm':cd['ledBatteryTestRpm']}
