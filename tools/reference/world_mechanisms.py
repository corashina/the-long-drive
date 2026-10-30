"""Project existing rotation and slide rules for generated source instances."""
from .source import key, source_id
from .world import _source_root
from .mechanisms import recover_mechanisms

def recover_world_mechanisms(source, selected, world):
    result=[]
    main=source.read(source.get('level1',64919))
    for prototype in world['prototypes']:
        root=source.get(prototype['source']['file'],prototype['source']['path_id'])
        hierarchy=source.hierarchy(root,activate_root=True)
        mapping={key(n['reader']):i+1 for i,n in enumerate(hierarchy)}
        nodes=[dict(active=True,parent=mapping.get(key(n['parent'])) if n['parent'] else None) for n in hierarchy]
        components={}
        for i,node in enumerate(hierarchy,1):
            group={}
            for c in node['components']:
                if c.type.name!='MonoBehaviour':continue
                cls=source.class_name(c)
                if cls=='flashlightscript':continue
                if cls=='usablescript':
                    data=source.read(c)
                    if not (data['rotateAble'] or data['slideAble']):continue
                group.setdefault(cls,[]).append(c)
            components[i]=group
        def pointer(owner,p):
            target=source.resolve(owner,p)
            return mapping[key(_source_root(source,target))]
        recipes=recover_mechanisms(source,nodes,components,pointer,None,main)
        recipes=[r for r in recipes if r['rotation'] or r['slide']]
        by_source={(r['source']['file'],r['source']['path_id']):r['use_node'] for r in recipes}
        for r in recipes:
            reaction = r['reaction']
            if reaction['kind'] == 'rotation' and not 1 <= reaction['node'] <= len(hierarchy):
                raise ValueError('Generated endpoint control is outside its prototype')
            node=r.pop('use_node');child=r.pop('child_source')
            parent=node
            while parent and not components[parent].get('pickupable'):parent=nodes[parent-1]['parent']
            r.update(item=node,parent_item=parent,child=by_source.get(child),disable_on_craft=False,disable_on_uncraft=False,turn=None)
            for group in components.values():
                for pick in group.get('pickupable',[]):
                    data=source.read(pick)
                    usable=source.resolve(pick,data['usable'])
                    if usable and key(usable)==(r['source']['file'],r['source']['path_id']):
                        r['disable_on_craft']=bool(data['disableOnCraft'])
                        r['disable_on_uncraft']=bool(data['disableOnUnCraft']) and not data['DONTdisableUsableOnUnCraft']
            reader=source.get(r['source']['file'],r['source']['path_id']);selected.record(reader)
            if r['slide'] and r['slide']['animation_source']:
                a=r['slide']['animation_source'];selected.record(source.get(a['file'],a['path_id']))
            result.append(dict(prototype=prototype['source'],node=node-1,recipe=r))
    return result
