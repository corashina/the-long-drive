"""Build typed runtime records from selected scene components."""
import math
from .source import key, source_id, vector
from .gltf import GltfExporter, convert_rotation, transform_point
from .collision import world_pose, quaternion_product, bake_points, geometry, step_surface, collider_tag
from .actions import recover_masks, recover_bindings, recover_pickup, recover_pushables
from .caps import cap_flow, opening_metadata
from .mechanisms import recover_mechanisms, bind_mechanisms
from .visibility import recover_camera_visibility
from .parts import recover_part_slots
from .driving import recover_driving
from .settings import recover_settings
from .lighting import rgba
from .player_movement import recover_player_movement
from .car_camera import recover_car_camera


def component_mass(source, components) -> float:
    """massScript mass plus initial tankscript fluid of one node."""
    total=0.
    for reader in components:
        if reader.type.name!='MonoBehaviour':continue
        name=source.class_name(reader)
        if name=='massScript':total+=source.read(reader)['mass']
        elif name=='tankscript':total+=sum(f['amount'] for f in source.read(reader)['F']['fluids'])
    return total


def fluid_entries(value: dict) -> list[dict]:
    """Validate initial tank contents and retain source list order."""
    capacity = value['maxC']
    if not math.isfinite(capacity) or capacity <= 0:
        raise ValueError('Invalid fluid capacity')
    entries = []
    seen = set()
    for fluid in value['fluids']:
        kind, amount = fluid['type'], fluid['amount']
        if kind not in range(6) or kind in seen or not math.isfinite(amount) or amount < 0:
            raise ValueError('Invalid initial fluid entry')
        seen.add(kind)
        entries.append({'fluid_type': kind, 'amount': amount})
    if len(entries) > 6 or sum(entry['amount'] for entry in entries) > capacity:
        raise ValueError('Initial fluid exceeds capacity')
    return entries


def burn_colors(main: dict) -> list[list[float]]:
    """Read mainscript.fluids[i].burnColor in fluid type order."""
    colors = [rgba(fluid['burnColor']) for fluid in main['fluids']]
    if len(colors) != 6 or not all(math.isfinite(v) for c in colors for v in c):
        raise ValueError('Invalid fluid burn colors')
    return colors


def build_manifest(source, roots, reports, hashes, library, car_prefab, output):
    """`car_prefab` is the car prototype that holds the shared plate font and material."""
    nodes=[]; by_source={}; objects={}; components={}; node_readers={}
    assets=[]
    for asset_id,(name,readers) in enumerate(roots.items(),1):
        report=reports[name]
        assets.append({'id':asset_id,'mesh_count':report['mesh_count'],'material_count':report['material_count'],'usage':'player_prototype' if name=='player' else 'ragdoll_prototype' if name=='ragdoll' else 'item_prototype' if name.startswith('item_') else 'food_prototype' if name=='food' else 'road_lamp_prototype' if name=='road_lamp' else 'sky_prototype' if name=='sky' else 'sky_traffic' if name.startswith('sky_') else 'creature_prototype' if name.startswith('creature_') else 'unique_scene','path':f'reference/{name}.gltf','sha256':report['sha256'],'root':source_id(readers[0]),'dependencies':report['dependencies']})
        records=[]
        for root in readers:records.extend(source.hierarchy(root,activate_root=True))
        for entry,record in zip(report['nodes'],records,strict=True):
            identity=key(record['reader']);node_id=len(nodes)+1
            if identity in by_source:raise ValueError(f'Duplicate node identity: {identity}')
            by_source[identity]=node_id;node_readers[node_id]=record['reader'];objects[node_id]=record
            nodes.append({'id':node_id,'source':entry['source'],'name':source.read(record['game_object'])['m_Name'],
                'hierarchy_path':entry['hierarchy_path'],'asset':asset_id,'gltf_node':entry['gltf_node'],
                'parent':by_source[key(record['parent'])] if record['parent'] else None,'active':entry['active'],'transform':entry['transform']})
            group={}
            for reader in record['components']:
                name=source.class_name(reader) if reader.type.name=='MonoBehaviour' else reader.type.name
                group.setdefault(name,[]).append(reader)
            components[node_id]=group if assets[-1]["usage"]=="unique_scene" else {}
    def data(node,name):
        values=components[node].get(name,[])
        return source.read(values[0]) if values else None
    def node_of(reader):
        go=source.resolve(reader,source.read(reader)['m_GameObject'])
        return by_source[key(source.transform_for(go))]
    def pointer_node(owner,pointer):
        reader=source.resolve(owner,pointer)
        return by_source[key(reader)] if reader.type.name in ('Transform','RectTransform') else by_source[key(source.transform_for(reader))] if reader.type.name=='GameObject' else node_of(reader)
    def descendants(node):
        result={node}
        for entry in nodes:
            if entry['parent'] in result:result.add(entry['id'])
        return result
    def total_mass(node):
        total=0.
        for child in descendants(node):
            if nodes[child-1]['active']:total+=component_mass(source,[r for group in components[child].values() for r in group])
        return total
    home_root=by_source[key(roots['home'][0])]
    main_reader=source.get('level1',64919);main=source.read(main_reader)
    player_reader=source.get('level1',80125);player=source.read(player_reader)
    physics=next(reader for reader in source.objects.values() if reader.type.name=='PhysicsManager')
    matrix=source.read(physics)['m_LayerCollisionMatrix']
    if len(matrix)!=32:raise ValueError('Physics layer matrix must have 32 masks')
    body_roots={};bodies=[]
    def body(node,dynamic,mass,reader):
        if node in body_roots:return body_roots[node]
        pose=world_pose(source,node_readers[node]);pose['scale']=[1.,1.,1.]
        rigidbody=source.read(reader) if reader.type.name=='Rigidbody' else None
        value={'id':len(bodies)+1,'source':source_id(reader),'node':node,'dynamic':dynamic,'mass_kg':mass,'pose':pose,
            'linear_damping':rigidbody['m_Drag'] if rigidbody is not None else 0.,
            'angular_damping':rigidbody['m_AngularDrag'] if rigidbody is not None else 0.}
        bodies.append(value);body_roots[node]=value['id'];return value['id']
    body(home_root,False,0.,node_readers[home_root])
    for root in roots['items']:
        node=by_source[key(root)];rigidbody=components[node].get('Rigidbody',[])
        body(node,bool(rigidbody),source.read(components[node]['massScript'][0])['mass'] if key(root) in getattr(source,'flashlight_root_keys',set()) else total_mass(node),rigidbody[0] if rigidbody else root)
    mechanisms=recover_mechanisms(source,nodes,components,pointer_node,node_of,main)
    driven_nodes={node for mechanism in mechanisms for node in mechanism['driven_nodes']}
    for node in sorted(driven_nodes):body(node,False,0.,node_readers[node])
    def body_for(node):
        while node not in body_roots:
            node=nodes[node-1]['parent']
            if node is None:raise ValueError('Node has no body binding')
        return body_roots[node]
    for value in bodies:
        parent=nodes[value['node']-1]['parent']
        value['parent_body']=body_for(parent) if parent is not None else None
    items=[];item_nodes={};item_components={}
    item_classes={'tosaveitemscript','massScript','tankscript','enginescript','coolantTankscript','seatscript','tankcapscript','gumiscript','wheelscript','pickupable','usablescript','flashlightscript','pushablescript'}
    mechanism_nodes=driven_nodes
    for node,group in components.items():
        if (not nodes[node-1]['active'] and key(node_readers[node]) not in getattr(source,'flashlight_root_keys',set())) or not (item_classes.intersection(group) or node in mechanism_nodes):continue
        name=nodes[node-1]['name'];kind='decoration'
        if key(node_readers[node])==('level1',21482):name='Garage door right'
        elif key(node_readers[node])==('level1',21464):name='Garage door left'
        elif key(node_readers[node])==('level1',21450):name='Door'
        for class_name,kind_name in [('tankscript','container'),('enginescript','engine'),('coolantTankscript','radiator'),('gumiscript','tire'),('wheelscript','rim'),('seatscript','seat'),('tankcapscript','cap'),('ediblescript','food')]:
            if class_name in group:kind=kind_name
        if node in mechanism_nodes:kind='hood' if 'Hood' in nodes[node-1]['hierarchy_path'] else 'trunk' if 'Trunk' in nodes[node-1]['hierarchy_path'] else 'door'
        condition=data(node,'partconditionscript')
        raw=source.conditions[key(group['partconditionscript'][0])]['state'] if condition else 0
        mass=data(node,'massScript')
        value={'id':len(items)+1,'source':source_id(node_readers[node]),'node':node,'body':body_for(node),'name':name,'kind':kind,
            'condition':1.-raw*0.25,'mass_kg':mass['mass'] if mass else 0.,'attached_to':None,'pickup':recover_pickup(source,group['pickupable'][0]) if 'pickupable' in group else None,'collision_clip':None,'collision_gain':0.}
        if 'pickupable' in group:
            pickup=source.read(group['pickupable'][0]);clip=source.resolve(group['pickupable'][0],pickup['collideSound'])
            if clip and clip.type.name!='AudioClip':raise ValueError('Invalid pickup collision clip')
            # Audio export changes this source ID to the clip ID.
            value['collision_clip']=source_id(clip) if clip else None;value['collision_gain']=pickup['collisionSoundM']
        items.append(value);item_nodes[node]=value['id']
        for readers in group.values():
            for reader in readers:item_components[key(reader)]=value['id']
    for item in items:
        parent=nodes[item['node']-1]['parent']
        while parent and parent not in item_nodes:parent=nodes[parent-1]['parent']
        item['attached_to']=item_nodes.get(parent)
    anchors=[];anchor_nodes={}
    def anchor(node):
        if node not in anchor_nodes:
            body_id=body_for(node);body_pose=bodies[body_id-1]['pose'];pose=world_pose(source,node_readers[node])
            inverse=[-v for v in body_pose['rotation'][:3]]+[body_pose['rotation'][3]]
            pose['translation']=bake_points([(0.,0.,0.)],source.chain(node_readers[node]),body_pose)[0]
            pose['rotation']=list(quaternion_product(inverse,pose['rotation']))
            value={'id':len(anchors)+1,'source':source_id(node_readers[node]),'node':node,'body':body_id,'transform':pose}
            anchors.append(value);anchor_nodes[node]=value['id']
        return anchor_nodes[node]
    colliders=[];exporter=GltfExporter(source,None,library);wheel_parts_source=[];detachable_colliders=set()
    for node,group in components.items():
        if not nodes[node-1]['active'] or 'partscript' not in group:continue
        part=group['partscript'][0];part_data=source.read(part);pick=source.resolve(part,part_data['p']);pick_data=source.read(pick)
        identities=[]
        for pointer in pick_data['cols']:
            layer=source.resolve(pick,pointer)
            if source.class_name(layer)!='layerScript':raise ValueError('Wheel collider link must target layerScript')
            layer_data=source.read(layer)
            if layer_data['col']['m_PathID']:
                collider=source.resolve(layer,layer_data['col'])
            else:
                owner=source.resolve(layer,layer_data['m_GameObject']);transform=source.transform_for(owner)
                _,owner_components=source.components(transform)
                candidates=[component for component in owner_components if component.type.name in {'BoxCollider','SphereCollider','CapsuleCollider','MeshCollider'}]
                if not candidates:raise ValueError(f'Missing layerScript collider owner: {key(layer)}')
                collider=candidates[0]
            identities.append(key(collider));detachable_colliders.add(key(collider))
        if 'wheelscript' in group or 'gumiscript' in group:wheel_parts_source.append((node,part,part_data,identities))
    retained_colliders=detachable_colliders
    for node,group in components.items():
        food_geometry=key(node_readers[node]) in source.food_geometry_nodes
        if not nodes[node-1]['active'] and not food_geometry and key(node_readers[node]) not in getattr(source,'flashlight_inactive_nodes',set()) and key(node_readers[node]) not in getattr(source,'condition_geometry_nodes',set()) and not any(key(reader) in retained_colliders for readers in group.values() for reader in readers):continue
        layer=source.read(objects[node]['game_object'])['m_Layer']
        for class_name in ('BoxCollider','SphereCollider','CapsuleCollider','MeshCollider'):
            for reader in group.get(class_name,[]):
                value=source.read(reader)
                if not value['m_Enabled'] and key(reader) not in retained_colliders:continue
                material=source.resolve(reader,value['m_Material']);friction,restitution=0.6,0.
                if material:
                    properties=source.read(material);friction=properties['dynamicFriction'];restitution=properties['bounciness']
                body_id=body_for(node);owner=bodies[body_id-1]
                colliders.append({'id':len(colliders)+1,'source':source_id(reader),'node':node,'body':body_id,
                    'sensor':value['m_IsTrigger'],'enabled':bool(value['m_Enabled']) and (nodes[node-1]['active'] or food_geometry),'membership':1<<layer,'filter':matrix[layer],
                    'friction':friction,'restitution':restitution,'geometry':geometry(source,exporter,reader,node_readers[node],owner['pose'],owner['dynamic']),
                    'step_surface':step_surface(source,objects[node]['game_object']),'tag':collider_tag(source,objects[node]['game_object'])})
    def tank_explosion(source,reader,full):
        # tankscript.TryExplode destroys parentDest too. The runtime removes only the tank item.
        dest=source.resolve(reader,full['parentDest'])
        if dest is not None and by_source.get(key(dest))!=node_of(reader):raise ValueError(f'Tank parentDest outside the tank: {key(reader)}')
        return {'never_explode':bool(full['neverExplode']),'always_explode':bool(full['alwaysExplode']),'always_explode_force':full['alwaysExplodeForce'],
            'force_needed':full['explosionForceNeededToExplode'],'explosion_type':full['explosionType'],'no_shoot_hole':bool(full['noShootHole']),
            'no_shoot_explode':bool(full['noShootExplode']),'parent_dest':None}
    tanks=[];openings=[]
    for node,group in components.items():
        if not nodes[node-1]['active']:continue
        for reader in group.get('tankscript',[]):
            full=source.read(reader);value=full['F']
            owner=body_for(node)
            tanks.append({'item':item_components[key(reader)],'source':source_id(reader),'capacity_liters':value['maxC'],'fluids':fluid_entries(value),'openings':[],
                'no_drink':bool(full['noDrink']),'no_fill_this':bool(full['noFillThis']),'no_fill_from':bool(full['noFillFrom']),'slow_oil':bool(full['slowOil']),
                'role':'portable' if bodies[owner-1]['dynamic'] else 'fixed','explosion':tank_explosion(source,reader,full)})
    for node,group in components.items():
        if not nodes[node-1]['active']:continue
        for reader in group.get('tankcapscript',[]):
            value=source.read(reader);tank=source.resolve(reader,value['Tank'])
            if tank is None or key(tank) not in item_components:raise ValueError(f'Cap tank outside selected roots: {key(reader)}')
            opening={'id':len(openings)+1,'source':source_id(reader),'tank':item_components[key(tank)],'cap':item_nodes[node],
                'anchor':anchor(node),**cap_flow(source,reader,node_readers[node],main['pourSpeed'])}
            opening.update(opening_metadata(source,reader,by_source,main['fluidTuneSpeed']))
            openings.append(opening)
            tank_record=next(t for t in tanks if t['item']==opening['tank'])
            tank_record['openings'].append(opening['id'])
            if not value['canToggle'] and value['noAngle'] and not tank_record['no_fill_from'] and not bodies[body_for(node)-1]['dynamic']:
                tank_record['role']='supply'
    bind_mechanisms(source,mechanisms,nodes,components,items,item_nodes)
    collider_ids={(c['source']['file'],c['source']['path_id']):c['id'] for c in colliders}
    wheel_parts=[]
    def euler_quaternion(value):
        rotations=[]
        for axis,angle in enumerate(value):
            half=math.radians(angle)*0.5;q=[0.,0.,0.,math.cos(half)];q[axis]=math.sin(half);rotations.append(q)
        return list(convert_rotation(quaternion_product(rotations[1],quaternion_product(rotations[0],rotations[2]))))
    for node,part,value,identities in wheel_parts_source:
        rim=components[node].get('wheelscript',[]);tire_slot=None;tire_mount=None;tire_main=[];tire_types=[];tire_anchor=None
        if rim:
            spec=source.read(rim[0]);radius=spec['felniRadius'];tire_slot=source.resolve(rim[0],spec['gumi']);tire_data=source.read(tire_slot)
            target=pointer_node(tire_slot,tire_data['T']);tire_anchor=anchor(target)
            tire_part=source.resolve(tire_slot,tire_data['part']);tire_node=node_of(tire_part)
            pose=world_pose(source,node_readers[tire_node]);rim_pose=world_pose(source,node_readers[node])
            pose['translation']=bake_points([(0.,0.,0.)],source.chain(node_readers[tire_node]),rim_pose)[0]
            inverse=[-x for x in rim_pose['rotation'][:3]]+[rim_pose['rotation'][3]];pose['rotation']=list(quaternion_product(inverse,pose['rotation']))
            tire_mount=pose;tire_main=tire_data['maintipus'];tire_types=tire_data['tipus']
        else:radius=source.read(components[node]['gumiscript'][0])['radius']
        # mainscript.defFallOffChance has 18 members.
        if not 0<=value['fallOffCategory']<=17:raise ValueError(f'{key(part)} fallOffCategory is not a defFallOffChance member')
        wheel_parts.append({'item':item_nodes[node],'source':source_id(part),'radius':radius,'main_types':value['maintipus'],'part_types':value['tipus'],
            'colliders':[collider_ids[identity] for identity in identities],'tire_slot_anchor':tire_anchor,'tire_mount':tire_mount,
            'tire_main_types':tire_main,'tire_types':tire_types,'local_rotation_after_mount':euler_quaternion(vector(value['localRotationAfterMount'])),'cover_slot':value['cover_CantCraftIfCovered']['m_PathID'] or None,
            'fall_off_category':value['fallOffCategory']})
    # A tire owns its listed shapes if a parent rim lists the same shapes.
    tire_colliders={collider_id for part in wheel_parts if 'gumi' in part['part_types'] for collider_id in part['colliders']}
    for part in wheel_parts:
        if 'felni' in part['part_types']:part['colliders']=[collider_id for collider_id in part['colliders'] if collider_id not in tire_colliders]
    part_slots=recover_part_slots(source,nodes,components,item_nodes,colliders,pointer_node,anchor,euler_quaternion)
    driving=recover_driving(source)
    source.driving_evidence=driving
    capsule=source.get('level1',58456);camera=source.get('level1',40241)
    player_record={'source':source_id(player_reader),'capsule_source':source_id(capsule),'camera_source':source_id(camera),'walk_speed':player['FdefMaxSpeed'],'run_multiplier':player['FrunM'],'jump_force':player['FjumpForce'],'mass_kg':source.read(source.get('level1',50702))['m_Mass'],'duck_multiplier':player['FduckM'],'aim_reach':player['FrayRange']}
    player_record.update(recover_player_movement(source))
    player_record.update(exit_distance=player['CanGetOutDist'],exit_step_vertical=player['GetOutStepV'],exit_steps_vertical=player['GetOutStepTimesV'],
        exit_step_horizontal=player['GetOutStepH'],exit_steps_horizontal=player['GetOutStepTimesH'])
    head=source.resolve(player_reader,player['Th']);held=source.resolve(player_reader,player['TpickedUpPos'])
    held_offset=bake_points([(0.,0.,0.)],source.chain(held),world_pose(source,head))[0]
    player_record.update(carry_min_weight=player['minWeight'],carry_max_weight=player['maxWeight'],carry_anchor_offset=held_offset,carry_offset_radius=player['maxPickupDist'],carry_drop_distance=player['dropDist'],
        carry_force_multiplier=player['pickupForceM'],carry_max_force=player['maxPickupForce'])
    player_record.update(carry_min_distance=max(0.,math.sqrt(sum(value**2 for value in held_offset))-player['maxPickupDist']),
        carry_max_distance=math.sqrt(sum(value**2 for value in held_offset))+player['maxPickupDist'])
    player_record['car_camera']=recover_car_camera(source,player_reader,player,main)
    player_record['camera_culling_mask']=recover_camera_visibility(source)['normal_view_mask']
    player_record['action_masks']=recover_masks(player)
    time_reader=source.get('globalgamemanagers',8)
    if time_reader.type.name != 'TimeManager':raise ValueError('Source TimeManager differs')
    player_record.update(source_force_interval=source.read(time_reader)['Fixed Timestep'],
        throw_minimum=player['minThrowForce'],throw_gain=player['throwForceGain'],throw_multiplier=1000.,
        push_force=main['pushForce'],airborne_push_force=main['selfNotGroundedPushForce'],
        grip_force=player['pushgripForce'],grip_speed=player['FspeedM'],grip_distance=player['pushGripDistance'],
        pedaling_nm=player['PedalingNm'],pedaling_rpm=player['PedalingRpm'])
    player_record['get_out_layer_mask']=player['getOutLayer']['m_Bits']
    # fpscontroller pointing ray (input.point): range, near range and layers.
    player_record.update(point_range=player['pointRange'],point_range2=player['pointRange2'],pointing_layer=player['pointingLayer']['m_Bits'])
    # fpscontroller.CanGetOut uses literal capsule endpoints and radius.
    player_record['exit_capsule_half_height']=.568
    player_record['exit_capsule_radius']=.3-.05
    # The start Lada stands in the garage. The player spawn derives from its source spot.
    home_pose=world_pose(source,node_readers[home_root]);car_pose=world_pose(source,source.transform_for(source.get('level1',17687)))
    world_points=[]
    for collider in colliders:
        if collider['body']==body_roots[home_root] and not collider['sensor'] and 'points' in collider['geometry']:
            owner=bodies[collider['body']-1]['pose'];world_points.extend(transform_point(point,owner['translation'],owner['rotation'],[1.,1.,1.]) for point in collider['geometry']['points'])
    bounds={'min':[min(p[i] for p in world_points) for i in range(3)],'max':[max(p[i] for p in world_points) for i in range(3)]}
    # Sample authored floor triangles under the standing position. The garage spawn is an explicit project change.
    faces=[(0,1,3),(0,3,2),(4,6,7),(4,7,5),(0,4,5),(0,5,1),(2,3,7),(2,7,6),(0,2,6),(0,6,4),(1,5,7),(1,7,3)]
    def floor_at(x,z,ceiling):
        hits=[]
        for collider in colliders:
            if collider['body']!=body_roots[home_root] or collider['sensor']:continue
            shape=collider['geometry']
            if 'points' not in shape:continue
            reader=source.get(collider['source']['file'],collider['source']['path_id'])
            triangles=shape.get('triangles',faces if reader.type.name=='BoxCollider' else [])
            if not triangles and reader.type.name=='MeshCollider':
                mesh=source.resolve(reader,source.read(reader)['m_Mesh'])
                _,_,groups=exporter.geometry(mesh)
                triangles=[triangle for group in groups for triangle in group]
            pose=bodies[collider['body']-1]['pose'];points=[transform_point(p,pose['translation'],pose['rotation'],[1.,1.,1.]) for p in shape['points']]
            for indices in triangles:
                a,b,c=[points[i] for i in indices]
                denominator=(b[2]-c[2])*(a[0]-c[0])+(c[0]-b[0])*(a[2]-c[2])
                if abs(denominator)<1e-9:continue
                u=((b[2]-c[2])*(x-c[0])+(c[0]-b[0])*(z-c[2]))/denominator
                v=((c[2]-a[2])*(x-c[0])+(a[0]-c[0])*(z-c[2]))/denominator
                if min(u,v,1-u-v)<-1e-6:continue
                y=u*a[1]+v*b[1]+(1-u-v)*c[1]
                if y<=ceiling:hits.append((y,points))
        if not hits:raise ValueError(f'No authored garage floor under {x}, {z}')
        return max(hits,key=lambda hit:hit[0])
    position=list(transform_point((-1.5,0.,0.),car_pose['translation'],car_pose['rotation'],[1.,1.,1.]))
    floor,_=floor_at(position[0],position[2],car_pose['translation'][1])
    posture=player_record['posture']
    lower=min(posture['prone_head']['translation'][1], posture['feet']['center'][1]-posture['maximum_radius'])
    position[1]=floor-lower+source.read(physics)['m_DefaultContactOffset']
    spawn={'translation':position,'rotation':list(quaternion_product(car_pose['rotation'],[0.,-math.sqrt(.5),0.,math.sqrt(.5)])),'scale':[1.,1.,1.]}
    result={'driving':driving['global'],'settings':recover_settings(source),'app_id':1017180,'build_id':11140890,'source_hashes':hashes,'assets':assets,'nodes':nodes,'bodies':bodies,'colliders':colliders,'anchors':anchors,'mechanisms':mechanisms,'wheel_parts':wheel_parts,'parts':[],'part_slots':part_slots,'part_scan':{'source':source_id(main_reader),'mask':main['scanLayer']['m_Bits'],'radius':main['defScanRadius']},'items':items,'openings':openings,'tanks':tanks,
        'fluid_rules':{'source':source_id(main_reader),'pour_speed':main['pourSpeed'],'slow_oil_modifier':main['slowOilModifier'],'fluid_tune_speed':main['fluidTuneSpeed'],'burn_colors':burn_colors(main),
            'min_tube_vertical_height':main['minTubeVerticalHeight'],'max_tube_vertical_height':main['maxTubeVerticalHeight']},'player':player_record,
        'area':{'source':source_id(source.get('level1',64896)),'home_root':home_root,'home_pose':home_pose,'home_bounds':bounds,'player_spawn':spawn}}
    result['rendering'] = {'materials': [dict(record, asset=asset_id)
        for asset_id, name in enumerate(roots, 1) for record in reports[name]['materials']]}
    result['pushables']=recover_pushables(source,nodes,components,item_nodes,colliders,bodies,player_record['action_masks']['push_mask'])
    result['action_bindings']=recover_bindings(source,nodes,components,item_nodes,colliders,mechanisms,openings)
    from .plates import recover_plates, plate_resources
    result['plates']=recover_plates(source,car_prefab,output)
    assets[0]['dependencies'].extend({'file':r['path'],'sha256':r['sha256']} for r in plate_resources(result['plates']))
    result['lighting']=source.lighting
    result['render_lighting']=source.render_lighting
    collider_ids={(c['source']['file'],c['source']['path_id']):c['id'] for c in colliders}
    old_colliders={v:k for k,v in source.lighting_collider_ids.items()}
    for controller in result['lighting']['controllers']:
        missing=[old_colliders[i] for i in controller['aim_colliders'] if old_colliders[i] not in collider_ids]
        if missing and controller['kind']!='road':raise ValueError(f"{controller['source']} required aim collider outside export: {missing}")
        controller['aim_colliders']=[collider_ids[old_colliders[i]] for i in controller['aim_colliders'] if old_colliders[i] in collider_ids]
    from .flashlights import recover_flashlights
    result['flashlights']=recover_flashlights(source,pointer_node,item_nodes,result['lighting']['controllers'],output)
    from .storage import recover_storage
    result['storage']=recover_storage(source,item_nodes,components,output)
    from .conditions import recover_conditions
    source.condition_bodies=bodies
    source.condition_items=items
    source.condition_part_slots=part_slots
    source.condition_wheel_parts=wheel_parts
    source.condition_anchors=anchors
    result['conditions']=recover_conditions(source,nodes,components,pointer_node,item_nodes,colliders,output)
    from .export import runtime_resources
    assets[0]['dependencies'].extend({'file':item['path'],'sha256':item['sha256']} for item in runtime_resources(result))
    return result


def attach_world(manifest, world):
    """Move selected prototype resources into the shared asset and material tables."""
    for prototype in world['prototypes']:
        asset=prototype.pop('asset_record')
        if asset['id']!=len(manifest['assets'])+1 or asset['id']!=prototype['asset']:
            raise ValueError(f"{prototype['source']} world asset index")
        manifest['assets'].append(asset)
        manifest['rendering']['materials'].extend(prototype.pop('materials'))
    for binding in world['painting_materials']:
        manifest['assets'].append(binding.pop('asset_record'))
        manifest['rendering']['materials'].extend(binding.pop('materials'))
    manifest['world']=world
    from .export import runtime_resources
    world_resources=runtime_resources({'world':world})
    manifest['assets'][world['prototypes'][0]['asset']-1]['dependencies'].extend({'file':r['path'],'sha256':r['sha256']} for r in world_resources)
