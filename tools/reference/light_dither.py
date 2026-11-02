"""Source `lightdither` on item prototypes: Perlin flicker of the pumpkin light."""
from .lighting import light_payload, validate_light_payload
from .source import key, source_id

# Light id in its own table. The cigarette ember light is 1.
LIGHT_ID = 2
DITHER_FIELDS = (('speed', 'speed'), ('speed2', 'speed2'), ('min_threshold', 'minThershold'),
    ('max_threshold', 'maxThershold'), ('min_value', 'minValue'), ('max_value', 'maxValue'), ('value', 'value'))


def recover_light_dither(source, records, node):
    """The one `lightdither` of a prototype hierarchy, or None. `node` maps a Transform to its node id."""
    found = [c for r in records for c in r['components']
        if c.type.name == 'MonoBehaviour' and source.class_name(c) == 'lightdither']
    if not found: return None
    if len(found) != 1: raise ValueError(f'{key(found[0])}: one lightdither per prototype')
    reader = found[0]; d = source.read(reader)
    if not d['m_Enabled']: raise ValueError(f'{key(reader)}: disabled lightdither')
    # Start takes the Light of its own GameObject when `light` is null.
    light = source.resolve(reader, d['light'])
    if light is None:
        game_object = source.resolve(reader, d['m_GameObject'])
        light = next((c for c in source.components(source.transform_for(game_object))[1] if c.type.name == 'Light'), None)
    if light is None: raise ValueError(f'{key(reader)}: lightdither has no Light')
    ld = source.read(light)
    if ld['m_Type'] != 2 or ld['m_Cookie']['m_PathID'] or ld['m_Shadows']['m_Type']:
        raise ValueError(f'{key(light)}: lightdither light is not a plain point light')
    transform = source.transform_for(source.resolve(light, ld['m_GameObject']))
    return {'source': source_id(reader), 'light': light_payload(source, light, node(transform), LIGHT_ID, ld['m_Intensity']),
        'dithers': [{name: dither[field] for name, field in DITHER_FIELDS} for dither in d['dithers']],
        'max': bool(d['max']), 'min': bool(d['min'])}


def validate_light_dither(record):
    if record is None: return
    validate_light_payload(record['light'])
    if record['light']['id'] != LIGHT_ID or not record['dithers']: raise ValueError('Light dither record')
