"""Recover the part slots of the selected scene."""
from .source import source_id, vector
from .gltf import convert_point


def recover_part_slots(source,nodes,components,item_nodes,colliders,pointer_node,anchor,euler):
    """Part slots of the selected scene: the cover slots of wheel rims. Car parts belong to the car prototypes."""
    wheel_covers={source.read(group['partscript'][0])['cover_CantCraftIfCovered']['m_PathID'] for node,group in components.items() if nodes[node-1]['active'] and 'wheelscript' in group}
    slot_readers=[(node,r) for node,group in components.items() for r in group.get('partslotscript',[]) if r.path_id in wheel_covers and not {'felni','gumi'}.intersection(source.read(r)['tipus'])]
    def parent_item(node):
        node=nodes[node-1]['parent']
        while node is not None:
            if node in item_nodes and ('partscript' in components[node]):return item_nodes[node]
            node=nodes[node-1]['parent']
        return None
    slots=[]
    for node,r in sorted(slot_readers):
        v=source.read(r);part=source.resolve(r,v['part'])
        item=None
        # The pickup root can differ from part.T.
        if part:
            pickup=source.resolve(part,source.read(part)['p'])
            item=item_nodes[pointer_node(pickup,source.read(pickup)['m_GameObject'])]
        slots.append({'id':r.path_id,'source':source_id(r),'node':node,'owner':parent_item(node),
            'part':item,'mount_anchor':anchor(pointer_node(r,v['T'])),
            'detach_anchor':anchor(pointer_node(r,v['DT'])) if v['DT']['m_PathID'] else None,
            'fixed_detach':bool(v['fixDt']),'detach_position':list(convert_point(vector(v['VfixDt']))),
            'detach_rotation':euler(vector(v['VfixDtR'])),
            'main_types':v['maintipus'],'part_types':v['tipus'],
            'cant_craft':bool(v['cantCraft']),'cant_uncraft':bool(v['cantUnCraft']),
            'disable_colliders_on_craft':bool(v['disableCollidersOnCraft']),
            'colliders':[c['id'] for c in colliders if c['node']==node]})
    return slots
