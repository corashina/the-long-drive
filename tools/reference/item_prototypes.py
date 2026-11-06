"""Checked physical templates. All body and collider IDs are prototype-local."""
from .source import continuous, key, source_id
from .actions import recover_pickup
from .collision import world_pose, bake_points, quaternion_product, geometry, step_surface, collider_tag
from .gltf import GltfExporter
from .survival import component, managed_evidence

# Checked prefab root Transforms, database numbers, pickup counts, and asset names.
SELECTED = ((75443,74,1,'item_pack'),(51722,62,1,'item_box'),(61587,216,1,'item_shovel'),
            (70812,217,1,'item_sunglasses'),(53838,218,1,'item_hat'),(53837,221,1,'item_cap'),
            (63605,245,1,'item_cone'),(63259,92,1,'item_pumpkin'),(54495,230,1,'item_cigarette'),
            (83430,61,6,'item_siphon_hose'),(51557,312,1,'item_tumbleweed'),(51878,215,1,'item_smg_magazine'))
# Checked Gib and reducedGib GameObjects of selected breakablescripts: beer, box, cacti, vase, wc,
# then box, vase and wc reduced gibs. Box gib Transforms are 81642 and 81645.
GIBS = (14653,39185,24388,39184,39189,39188,39187,39186,23303,39984)


def selected(source):
    """(root Transform, database number, asset name) of each prototype. Gibs come last."""
    for pid, item_id, _, name in SELECTED:
        yield source.get('sharedassets1.assets', pid), item_id, name
    for go in GIBS:
        yield source.transform_for(source.get('sharedassets1.assets', go)), None, f'item_gib_{go}'


def recover_gib(source, records, node_ids) -> tuple:
    """gibscript of the gib root (or None), gibscripts of gib pieces and the nodes with scaleDestroyScript."""
    gib = None; scale_destroy = []; pieces = []
    parents = {key(r['reader']): key(r['parent']) if r['parent'] is not None else None for r in records}
    def subtree(top):
        result = []
        for identity in parents:
            node = identity
            while node is not None and node != top:node = parents[node]
            if node == top:result.append(node_ids[identity])
        return result
    for index, record in enumerate(records):
        for c in record['components']:
            if c.type.name != 'MonoBehaviour':
                continue
            name = source.class_name(c)
            if name == 'scaleDestroyScript':
                if node_ids[key(record['reader'])] in scale_destroy:
                    raise ValueError(f'Duplicate scaleDestroyScript: {key(c)}')
                scale_destroy.append(node_ids[key(record['reader'])])
            elif name == 'gibscript' and index:
                # A gib piece keeps its own life after the main gib detaches it.
                d = source.read(c)
                if d['gibMain'] or d['colsOff']:
                    raise ValueError(f'gib piece gibscript start state is not audited: {key(c)}')
                identity = key(record['reader'])
                pieces.append({'source': source_id(c), 'node': node_ids[identity], 'distance': bool(d['distance']),
                               'life': d['life'], 'nodes': subtree(identity)})
            elif name == 'gibscript':
                if gib is not None:
                    raise ValueError(f'gibscript is not the only one on the gib root: {key(c)}')
                d = source.read(c)
                # gibscript.Start sets startTime and life2. An authored colsOff is not audited.
                if d['colsOff']:
                    raise ValueError(f'gibscript start state is not audited: {key(c)}')
                children = []
                for pointer in d['childs']:
                    child = source.resolve(c, pointer)
                    if child is None or child.type.name != 'Transform' or key(child) not in node_ids:
                        raise ValueError(f'gibscript child is outside the gib: {key(c)}')
                    children.append(node_ids[key(child)])
                gib = {'source': source_id(c), 'gib_main': bool(d['gibMain']), 'dont_rot': bool(d['gibMainDontRot']),
                       'add_small_pos': bool(d['gibMainAddSmallPos']), 'distance': bool(d['distance']),
                       'life': d['life'], 'children': children}
    return gib, pieces, scale_destroy


def recover_gib_particles(source, root, records, output) -> list[dict]:
    """ParticleSystems of a gib. Break instantiates the gib, and each system plays on awake once."""
    from .particles import particle
    result = []
    for record in records:
        for c in record['components']:
            if c.type.name != 'ParticleSystem':
                continue
            d = source.read(c)
            if not d['playOnAwake']:
                continue
            # A looping system plays until the gib goes. That is not audited.
            if d['looping']:
                raise ValueError(f'Gib particle loops: {key(c)}')
            rate = d['EmissionModule']['rateOverTime']['scalar']
            result.append(particle(source, c, f'gib_{root.path_id}_{len(result)}', rate, output, root))
    return result


def prepare_item_prototypes(source, roots):
    source.item_geometry_nodes = set()
    for root, _, name in selected(source):
        roots[name] = [root]
        source.item_geometry_nodes.update(key(n['reader']) for n in source.hierarchy(root, activate_root=True))


def recover_item_prototypes(source, manifest, output) -> list[dict]:
    managed_evidence(source, ['disableThisWhenStored = transform;',
        'spawnedGib = UnityEngine.Object.Instantiate(reducedGib, transform.position, transform.rotation);',
        'spawnedGib = UnityEngine.Object.Instantiate(Gib, transform.position, transform.rotation);',
        'life2 = life * 0.95f;', 'scaleLength *= UnityEngine.Random.Range(1f - random, 1f + random);'])
    nodes = {(n['source']['file'], n['source']['path_id']):n for n in manifest['nodes']}
    matrix = source.read(next(r for r in source.objects.values() if r.type.name=='PhysicsManager'))['m_LayerCollisionMatrix']
    exporter = GltfExporter(source, None, source.library)
    source.item_clips = set()
    result = []
    for root, item_id, _ in selected(source):
        records = source.hierarchy(root, activate_root=True)
        by_node = {nodes[key(r['reader'])]['id']:r for r in records}
        root_pose = world_pose(source, root)
        inverse = [-v for v in root_pose['rotation'][:3]] + [root_pose['rotation'][3]]
        bodies = []; body_nodes = {}; poses = {}
        for node, record in by_node.items():
            rigid = [c for c in record['components'] if c.type.name=='Rigidbody']
            if not rigid: continue
            if len(rigid)!=1: raise ValueError('Duplicate prototype Rigidbody')
            reader = rigid[0]; d = source.read(reader)
            pose = world_pose(source, record['reader']); pose['scale'] = [1.,1.,1.]
            poses[node] = pose
            local_pose = {'translation':bake_points([(0.,0.,0.)],source.chain(record['reader']),root_pose)[0],
                'rotation':list(quaternion_product(inverse,pose['rotation'])), 'scale':[1.,1.,1.]}
            masses = [c for c in record['components'] if c.type.name=='MonoBehaviour' and source.class_name(c)=='massScript']
            # Gib parts may lack massScript. They keep the Rigidbody mass.
            if len(masses)>1 or (not masses and item_id is not None):raise ValueError(f'Prototype body requires one massScript: {key(reader)}')
            bodies.append({'id':len(bodies)+1,'source':source_id(reader),'node':node,'dynamic':not d['m_IsKinematic'],
                'mass_kg':source.read(masses[0])['mass'] if masses else d['m_Mass'],'linear_damping':d['m_Drag'],'angular_damping':d['m_AngularDrag'],
                'ccd':continuous(d),'pose':local_pose,'parent_body':None})
            body_nodes[node] = bodies[-1]['id']
        def body_for(node):
            while node is not None:
                if node in body_nodes:return body_nodes[node]
                node = manifest['nodes'][node-1]['parent']
            return None
        for body in bodies:body['parent_body'] = body_for(manifest['nodes'][body['node']-1]['parent'])
        colliders=[];pickups=[];renderers=[];variants=[]
        for node, record in by_node.items():
            group = {(source.class_name(c) if c.type.name=='MonoBehaviour' else c.type.name):c for c in record['components']}
            for c in record['components']:
                if c.type.name not in ('BoxCollider','SphereCollider','CapsuleCollider','MeshCollider'):continue
                d=source.read(c); owner=body_for(node)
                if owner is None:raise ValueError(f'Prototype collider lacks body: {key(c)}')
                body=bodies[owner-1];layer=source.read(record['game_object'])['m_Layer']
                material=source.resolve(c,d['m_Material'])
                properties=source.read(material) if material else {'dynamicFriction':.6,'bounciness':0.}
                colliders.append({'id':len(colliders)+1,'source':source_id(c),'node':node,'body':owner,
                    'sensor':bool(d['m_IsTrigger']),'enabled':bool(d['m_Enabled']), 'membership':1<<layer,'filter':matrix[layer],
                    'friction':properties['dynamicFriction'],'restitution':properties['bounciness'],
                    'geometry':geometry(source,exporter,c,record['reader'],poses[body['node']],body['dynamic']),
                    'step_surface':step_surface(source,record['game_object']),'tag':collider_tag(source,record['game_object'])})
            if 'pickupable' in group:
                c=group['pickupable'];d=source.read(c)
                clip=source.resolve(c,d['collideSound'])
                if clip:
                    if clip.type.name!='AudioClip':raise ValueError('Invalid pickup collision clip')
                    source.item_clips.add(key(clip))
                pickups.append({'node':node,'body':body_for(node),'pickup':recover_pickup(source,c),
                    'collision_clip':source_id(clip) if clip else None,'collision_gain':d['collisionSoundM']})
            if 'MeshRenderer' in group:
                c=group['MeshRenderer'];d=source.read(c);mesh=source.resolve(group['MeshFilter'],source.read(group['MeshFilter'])['m_Mesh'])
                from .render_lighting import recover_renderer
                renderers.append({'state':recover_renderer(source,c,mesh,'item_prototype'),'source':source_id(c),'node':node,'mesh':source_id(mesh),
                    'enabled':bool(d['m_Enabled']), 'materials':[source_id(source.resolve(c,p)) for p in d['m_Materials']]})
            if 'partconditionscript' in group and item_id not in (217,218,221,245,92,230):
                c=group['partconditionscript'];d=source.read(c)
                if d['UseMatList'] or d['UseMesList'] or d['useRandomTipus']:raise ValueError('Unaudited physical prototype selector')
                if d['UseGoList']:
                    variants.append({'source':source_id(c),'node':node,'nodes':[nodes[key(source.transform_for(source.resolve(c,p)))]['id'] for p in d['GoList']],
                        'authored_index':d['state3'],'random_start':bool(d['randomStart'])})
        def link(c, field, kind):
            target=source.resolve(c,source.read(c)[field])
            if target is None or target.type.name!=kind:raise ValueError(f'Invalid prototype {field}: {key(c)}')
            return target
        behavior={'pack':None,'ammo':None,'ammo_rules':None,'cigarette_prefab':None,'spawn':None,'breakable':None,'mount':None,
            'melee':None,'attack_target':None,'attack_trigger':None,'gib':None,'gib_pieces':[],'gib_particles':[],'scale_destroy':[],'clips':[],'contact_sound_source':None,'contact_clips':[]}
        if item_id==74:
            c=component(source,root,'cigiscript');behavior.update(pack=source_id(c),ammo=source_id(component(source,root,'ammoscript')),
                cigarette_prefab=source_id(link(c,'prefab','GameObject')))
        elif item_id==215:
            # geppityuscript.Reload(unload) spawns this magazine with the gun rounds.
            c=component(source,root,'ammoscript');d=source.read(c)
            if not d['isMag'] or d['w']['m_PathID']:raise ValueError(f'Magazine ammoscript is not a loose magazine: {key(c)}')
            behavior.update(ammo=source_id(c),ammo_rules={'kind':d['tipus'],'count':d['fammo'] if d['floatOrInt'] else float(d['ammo']),
                'maximum':d['max'],'float':bool(d['floatOrInt']),'is_mag':True})
        elif item_id==62:
            c=component(source,root,'breakablescript');spawn=component(source,root,'spawnInBoxScript')
            behavior.update(breakable=source_id(c),spawn=source_id(spawn),mount=source_id(link(spawn,'mount','MonoBehaviour')))
        elif any(c.type.name=='MonoBehaviour' and source.class_name(c)=='meleeweaponscript' for c in source.components(root)[1]):
            c=component(source,root,'meleeweaponscript')
            behavior.update(melee=source_id(c),attack_target=source_id(link(c,'T','Transform')),attack_trigger=source_id(link(c,'Trigger','Transform')))
            behavior['clips']=[source_id(source.resolve(c,p)) for p in source.read(c)['attackClips']]
            main=source.get('level1',64919)
            behavior['contact_sound_source']=source_id(main)
            behavior['contact_clips']=[[source_id(source.resolve(main,p)) for p in group['a']] for group in source.read(main)['meleeHitSounds']]
            managed_evidence(source,['mainscript.PlayClipAtPoint(mainscript.M.meleeHitSounds[(int)hitType].a[num], V, mainscript.hitVolume[(int)hitType], mainscript.AudioPriorities[7]);'])
        elif item_id is None:
            behavior['gib'],behavior['gib_pieces'],behavior['scale_destroy']=recover_gib(source,records,{key(r['reader']):nodes[key(r['reader'])]['id'] for r in records})
            behavior['gib_particles']=recover_gib_particles(source,root,records,output)
        for identity in [*behavior['clips'],*(c for group in behavior['contact_clips'] for c in group)]:
            c=source.get(identity['file'],identity['path_id'])
            if c.type.name!='AudioClip':raise ValueError('Invalid behavior clip')
            source.item_clips.add(key(c))
        from .survival import recover_cigarette
        consumable = recover_cigarette(source, root, manifest, output) if item_id==230 else None
        from .hose import recover_hose
        hose = recover_hose(source,by_node,bodies,poses,{k:n['id'] for k,n in nodes.items()},exporter,source.read(source.get('level1',64919)),output) if item_id==61 else None
        from .light_dither import recover_light_dither
        light_dither = recover_light_dither(source, records, lambda t: nodes[key(t)]['id'])
        result.append({'hose':hose,'consumable':consumable,'light_dither':light_dither,'id':source_id(root),'source':source_id(root),'item_id':item_id,
            'asset':nodes[key(root)]['asset'],'root':nodes[key(root)]['id'],'bodies':bodies,'colliders':colliders,
            'pickups':pickups,'variants':variants,'renderers':renderers,'behavior':behavior})
    return result

