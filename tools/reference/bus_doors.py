"""busdooropenscript sounds of one car prototype."""
from .source import source_id
from .particles import relative_pose


def door_audio(source, root, car_reader, shot):
    """One record per busdooropenscript below `root`, in hierarchy order.

    `shot(owner_reader, clip_pointer)` adds a mainscript.PlayClipAtPoint car voice and returns its record.
    Positions are the leaf positions in the chassis (`carscript.RB`) frame.
    """
    car = source.read(car_reader)
    body = source.resolve(car_reader, car['RB'])
    owner = source.transform_for(source.resolve(body, source.read(body)['m_GameObject']))
    records = []
    for node in source.hierarchy(root, activate_root=True):
        for c in node['components']:
            if c.type.name != 'MonoBehaviour' or source.class_name(c) != 'busdooropenscript':
                continue
            position = relative_pose(source, node['reader'], owner)['translation']
            sounds = []
            for pointer in source.read(c)['sounds']:
                s = shot(c, pointer)
                if s is None:
                    raise ValueError(f'busdooropenscript sound is null: {source_id(c)}')
                sounds.append({**s, 'position': position})
            records.append({'source': source_id(c), 'sounds': sounds})
    return records
