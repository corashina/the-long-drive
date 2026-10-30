"""fpscontroller outside view rig and mainscript map view values."""
RIG_FIELDS = ('CarCamParent', 'CarCamRot', 'CarCamMov', 'CarCam')
MAP_FIELDS = dict(time_add='mapCamViewTimeAdd', min_height='mapCamViewMinHeight', max_height='mapCamViewMaxHeight',
    min_pan='mapCamViewMinLean', max_pan='mapCamViewMaxLean', min_pan_speed='mapCamViewMinLeanSpeed',
    max_pan_speed='mapCamViewMaxLeanSpeed', height_rate='mapCamViewChange', rotation_rate='mapCamViewRotLerp',
    look_time='mapCamViewLookLerp')


def recover_car_camera(source, player_reader, player, main):
    """The rig is CarCamParent -> CarCamRot -> CarCamMov -> CarCam. Only CarCam has a local offset (-z)."""
    rig = [source.resolve(player_reader, player[field]) for field in RIG_FIELDS]
    data = [source.read(reader) for reader in rig]
    for i, (reader, value) in enumerate(zip(rig, data)):
        if reader.type.name != 'Transform':raise ValueError(f'Car camera {RIG_FIELDS[i]} is not a transform')
        if i and source.resolve(reader, value['m_Father']).path_id != rig[i-1].path_id:raise ValueError('Car camera rig hierarchy differs')
        rotation = value['m_LocalRotation']
        if (abs(rotation['x']), abs(rotation['y']), abs(rotation['z']), rotation['w']) != (0., 0., 0., 1.):raise ValueError('Car camera rig rotation differs')
        position = value['m_LocalPosition']
        offset = (position['x'], position['y'], position['z'] if i < 3 else 0.)
        if offset != (0., 0., 0.):raise ValueError('Car camera rig offset differs')
    distance = -data[3]['m_LocalPosition']['z']
    if not 1. <= distance <= 30.:raise ValueError('Car camera distance differs')
    return {'default_pitch_degrees':player['defcarcamrotX'],
        'pan_limits':[player['CarCamLimitX'], player['CarCamLimitY'], player['CarCamLimitZ']],
        'pan_sensitivity':player['CarCamPosSens'], 'default_distance':distance,
        'recenter_rate':player['FduckLerp'], 'follow_rate':player['carCamLerpCarSpeed'],
        'follow_speed_kmh':[player['carCamLerpCarSpeedMin'], player['carCamLerpCarSpeedMax']],
        'follow_angle_speed_kmh':[player['carCamLerpCarAngleSpeedMin'], player['carCamLerpCarAngleSpeedMax']],
        'map':{name:main[field] for name, field in MAP_FIELDS.items()}}
