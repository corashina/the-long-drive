"""Recover source startup visibility without changing hierarchy."""
from .source import key, source_id


def recover_startup_inactive_nodes(source, records):
    inactive = set()
    for record in records:
        for component in record['components']:
            if component.type.name != 'MonoBehaviour':
                continue
            name = source.class_name(component)
            if name == 'mountStuff':
                fields = source.read(component)
                if record['active'] and fields['m_Enabled'] and fields['visual']['m_PathID']:
                    visual = source.resolve(component, fields['visual'])
                    inactive.add(key(source.transform_for(visual)))
            elif name == 'terrainHeightAlignToBuildingScript':
                for pointer in source.read(component)['helpPosCoords']:
                    marker = source.resolve(component, pointer)
                    if marker is None:
                        raise ValueError(f'Missing startup marker: {source_id(component)} helpPosCoords')
                    inactive.add(key(marker))
    return inactive


def renderer_visible(serialized_enabled: bool, component_classes: set[str]) -> bool:
    return serialized_enabled and not component_classes.intersection({'digholescript2', 'TextMeshPro', 'TMP_SubMesh'})


def normal_view_mask(masks):
    result=0
    for mask in masks:
        if not 0<=mask<=0xffffffff:raise ValueError('Invalid source camera mask')
        result|=mask
    return result


def recover_camera_visibility(source):
    player=source.get('level1',80125);fields=source.read(player);cameras=[]
    for role in ('Cam','mainCam','farCamera','skyCamera'):
        reader=source.resolve(player,fields[role]);data=source.read(reader)
        cameras.append({'role':role,'source':source_id(reader),'mask':data['m_CullingMask']['m_Bits']})
    return {'cameras':cameras,'normal_view_mask':normal_view_mask(camera['mask'] for camera in cameras)}
