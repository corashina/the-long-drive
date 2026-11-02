"""Bake original collider points into converted body-local meters."""
import math
from .gltf import convert_point, convert_rotation, transform_point
from .source import vector


def quaternion_product(a,b):
    x,y,z,w=a; i,j,k,l=b
    return (w*i+x*l+y*k-z*j,w*j-x*k+y*l+z*i,w*k+x*j-y*i+z*l,w*l-x*i-y*j-z*k)


def world_pose(source,reader):
    chain = source.chain(reader)
    position=(0.,0.,0.); rotation=(0.,0.,0.,1.); scale=(1.,1.,1.)
    for data in chain:
        position = transform_point(position,vector(data['m_LocalPosition']),vector(data['m_LocalRotation'],4),vector(data['m_LocalScale']))
        rotation = quaternion_product(vector(data['m_LocalRotation'],4),rotation)
        scale=tuple(scale[i]*vector(data['m_LocalScale'])[i] for i in range(3))
    return {'translation':list(convert_point(position)), 'rotation':list(convert_rotation(rotation)), 'scale':list(scale)}


def bake_points(points, chain, body_pose):
    rotation=body_pose['rotation']
    inverse=(-rotation[0],-rotation[1],-rotation[2],rotation[3])
    result=[]
    for point in points:
        for data in chain:
            point=transform_point(point,vector(data['m_LocalPosition']),vector(data['m_LocalRotation'],4),vector(data['m_LocalScale']))
        point=convert_point(point)
        offset=tuple(point[i]-body_pose['translation'][i] for i in range(3))
        result.append(list(transform_point(offset,(0.,0.,0.),inverse,(1.,1.,1.))))
    return result


def geometry(source, exporter, reader, transform, body_pose, dynamic):
    data=source.read(reader)
    chain=source.chain(transform)
    def bake(points): return bake_points(points,chain,body_pose)
    if reader.type.name=='BoxCollider':
        center=vector(data['m_Center']); half=tuple(x*0.5 for x in vector(data['m_Size']))
        points=[tuple(center[i]+sign[i]*half[i] for i in range(3)) for sign in [(x,y,z) for x in (-1,1) for y in (-1,1) for z in (-1,1)]]
        return {'kind':'convex','points':bake(points)}
    if reader.type.name=='MeshCollider':
        mesh=source.resolve(reader,data['m_Mesh'])
        if mesh is None: raise ValueError(f'MeshCollider {reader.path_id} has no mesh')
        _,points,groups=exporter.geometry(mesh)
        # MeshHandler points already use converted handedness. Undo that step
        # before the shared body-local baker applies the conversion once.
        original=[convert_point(p) for p in points]
        result={'kind':'convex' if dynamic or data['m_Convex'] else 'trimesh','points':bake(original)}
        if result['kind']=='trimesh':result['triangles']=[list(t) for group in groups for t in group]
        return result
    pose=world_pose(source,transform)
    scales=list(map(abs,pose['scale']))
    center=bake([vector(data['m_Center'])])[0]
    if reader.type.name=='SphereCollider':
        return {'kind':'sphere','center':center,'radius':data['m_Radius']*max(scales)}
    if reader.type.name=='CapsuleCollider':
        axis=data['m_Direction']; radius=data['m_Radius']*max(scales[i] for i in range(3) if i!=axis)
        half=max(0.,data['m_Height']*scales[axis]*0.5-radius)
        # Unity scales capsule radius by the maximum perpendicular scale.
        points=[]
        for sign in (-1,1):
            point=list(vector(data['m_Center']));point[axis]+=sign*half/scales[axis];points.append(point)
        a,b=bake(points)
        return {'kind':'capsule','a':a,'b':b,'radius':radius}
    raise ValueError(f'Unsupported collider: {reader.type.name}')


STEP_TIPS = ('road', 'desert', 'winter', 'grass', 'wood')


def step_surface(source, game_object):
    """Footstep surface of a hit collider GameObject, as stepSoundScript.Step reads it."""
    data = source.read(game_object)
    # The source looks for otherStepSoundScript on the collider's own GameObject only.
    for part in data['m_Component']:
        component = source.resolve(game_object, part['component'])
        if component.type.name == 'MonoBehaviour' and source.class_name(component) == 'otherStepSoundScript':
            return STEP_TIPS[source.read(component)['tip']]
    # The source checks sand before the Wood tag, so a Wood tag is not tip wood.
    return {'Wood': 'wood_tag', 'Sand': 'sand'}.get(collider_tag(source, game_object))


def collider_tag(source, game_object):
    """Custom tag of a collider GameObject, as CompareTag and detectTags read it."""
    data = source.read(game_object)
    # Player builds store a custom tag as 20000 plus its TagManager index.
    if data['m_Tag'] < 20000: return None
    if getattr(source, 'tag_names', None) is None:
        managers = [o for o in source.objects.values() if o.type.name == 'TagManager']
        if len(managers) != 1: raise ValueError('Source needs one TagManager')
        source.tag_names = source.read(managers[0])['tags']
    return source.tag_names[data['m_Tag'] - 20000]
