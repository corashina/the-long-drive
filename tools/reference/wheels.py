"""Source WheelCollider records."""
from .gltf import convert_point
from .source import vector

CURVE_FIELDS = {'extremum_slip':'m_ExtremumSlip', 'extremum_value':'m_ExtremumValue',
    'asymptote_slip':'m_AsymptoteSlip', 'asymptote_value':'m_AsymptoteValue', 'stiffness':'m_Stiffness'}

def wheel_collider_record(value):
    """Map read WheelCollider fields to the runtime collider record."""
    spring=value['m_SuspensionSpring']
    return {'radius':value['m_Radius'],'suspension_distance':value['m_SuspensionDistance'],
        'center':list(convert_point(vector(value['m_Center']))),
        'spring':spring['spring'],'damper':spring['damper'],'target_position':spring['targetPosition'],
        'forward_friction':{out:value['m_ForwardFriction'][field] for out,field in CURVE_FIELDS.items()},
        'sideways_friction':{out:value['m_SidewaysFriction'][field] for out,field in CURVE_FIELDS.items()},
        'wheel_mass_kg':value['m_Mass'],'rotational_damping':value['m_WheelDampingRate'],
        'force_application_distance_m':value['m_ForceAppPointDistance']}
