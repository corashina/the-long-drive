"""Required JSON shapes shared by source reference owners."""
import math


def record(value, fields):
    if not isinstance(value, dict) or set(value) != set(fields):
        raise ValueError('Required record fields: ' + ', '.join(fields))
    for name, validate in fields.items():
        validate(value[name])


def number(value):
    if type(value) not in (int, float) or abs(value) > 3.4028234663852886e38 or not math.isfinite(value):
        raise ValueError('Expected finite f32')


def integer(low, high):
    def validate(value):
        if type(value) is not int or not low <= value <= high:
            raise ValueError('Expected bounded integer')
    return validate


def boolean(value):
    if type(value) is not bool:raise ValueError('Expected boolean')


def string(value):
    if not isinstance(value, str):raise ValueError('Expected string')


def array(validate, length=None):
    def check(value):
        if not isinstance(value, list) or (length is not None and len(value) != length):
            raise ValueError('Expected array shape')
        for child in value:validate(child)
    return check


def mapping(validate):
    def check(value):
        if not isinstance(value, dict):raise ValueError('Expected map')
        for key, child in value.items():
            string(key);validate(child)
    return check


def optional(validate):
    return lambda value: None if value is None else validate(value)


u32=integer(0, 2**32-1)
i32=integer(-2**31, 2**31-1)


def source(value):
    record(value, {'file':string, 'path_id':integer(-2**63, 2**63-1)})


def pose(value):
    record(value, {'translation':array(number,3),'rotation':array(number,4),'scale':array(number,3)})
    if any(v == 0 for v in value['scale']) or abs(sum(v*v for v in value['rotation'])-1) >= .002:
        raise ValueError('Invalid source transform')
