"""Recover cap controls and local visual marker links."""
import ast
import math
import re
from .source import key, source_id, vector
from .collision import world_pose, quaternion_product
from .gltf import transform_point


def parse_default_words(code: str) -> list[str]:
    """Return ls.FillDefWW: the default English text for each ls.w value."""
    body=code.split('private void FillDefWW()',1)[1].split('\n\t}',1)[0]
    words=[None]*int(re.search(r'defww = new string\[(\d+)\];',body)[1])
    for index,text in re.findall(r'defww\[(\d+)\] = ("(?:[^"\\]|\\.)*");',body):
        words[int(index)]=ast.literal_eval(text)
    if None in words:raise ValueError('Default language table is incomplete')
    return words


def parse_word_names(code: str) -> list[str]:
    """Return the ls.w enum names in index order. The enum has no explicit values."""
    body=code.split('public class ls : MonoBehaviour',1)[1].split('public enum w',1)[1].split('}',1)[0]
    return [name.strip() for name in body.strip(' \t\r\n{').split(',') if name.strip()]


def managed_code(source) -> str:
    if not hasattr(source,'managed_code'):
        from .library.reader import checked
        records={r['file']:r for r in source.library.manifest['managed_analysis']['files']}
        source.managed_code=checked(source.library.records_root,records['managed/Assembly-CSharp.cs']).decode('utf-8-sig')
    return source.managed_code


def default_words(source) -> list[str]:
    if not hasattr(source,'default_words'):source.default_words=parse_default_words(managed_code(source))
    return source.default_words


def cap_presentation(fields, words):
    """GetCapString shows ls.l.g(nameText): `word` is the ls.w index. The serialized nameString is not read."""
    if fields['hasName'] and not 0<=fields['nameText']<len(words):raise ValueError('Cap name word outside language table')
    return {'has_name':bool(fields['hasName']),'word':fields['nameText'] if fields['hasName'] else 0,'no_text':bool(fields['noText'])}


def opening_metadata(source, reader, node_ids, fluid_tune_speed):
    fields=source.read(reader)
    def node(pointer):
        if not pointer['m_PathID']:return None
        target=source.resolve(reader,pointer)
        if target.type.name=='GameObject':target=source.transform_for(target)
        if key(target) not in node_ids:raise ValueError(f'Cap visual outside selected hierarchy: {key(target)}')
        return node_ids[key(target)]
    # mainscript.CrossHairStuff: GCMMB shows img, or ICap when img is null.
    icon=source.resolve(reader,fields['img']) if fields['canTune'] else None
    if icon is not None:source.crosshair_icons.add(key(icon))
    def euler(value):
        x,y,z=vector(value)
        return [-x,-y,z]
    return {'valve':fields['valve'],'can_tune':bool(fields['canTune']),'tune_rate':fluid_tune_speed*fields['tuneSpeed'],
        'tune_icon':source_id(icon) if icon is not None else None,'presentation':cap_presentation(fields,default_words(source)),
        'visuals':{'open_nodes':[node(pointer) for pointer in fields['GOpen']],
            'closed_nodes':[node(pointer) for pointer in fields['GClosed']],
            'cap_node':node(fields['TCap']),'closed_node':node(fields['TClosed']),'open_node':node(fields['TOpen']),
            'euler_rotation':bool(fields['frot']),'closed_euler_degrees':euler(fields['closedRot']),
            'open_euler_degrees':euler(fields['openRot'])}}


def cap_flow(source, reader, cap_transform, pour_speed):
    """Flow, tilt, and particle fields of one tankcapscript, relative to its cap Transform."""
    value=source.read(reader)
    particle=source.resolve(reader,value['ps'])
    if particle is None or particle.type.name!='ParticleSystem':raise ValueError(f'Missing fluid emitter: {key(reader)}')
    emitted=source.read(particle);emitter=source.transform_for(source.resolve(particle,emitted['m_GameObject']))
    cap_pose=world_pose(source,cap_transform);emitter_pose=world_pose(source,emitter)
    rotation=cap_pose['rotation'];inverse=(-rotation[0],-rotation[1],-rotation[2],rotation[3])
    delta=[emitter_pose['translation'][axis]-cap_pose['translation'][axis] for axis in range(3)]
    particle_pose={'translation':list(transform_point(delta,(0.,0.,0.),inverse,(1.,1.,1.))),
        'rotation':list(quaternion_product(inverse,emitter_pose['rotation'])),
        'scale':[emitter_pose['scale'][axis]/cap_pose['scale'][axis] for axis in range(3)]}
    shape=emitted['ShapeModule'];collision=emitted['CollisionModule'];initial=emitted['InitialModule']
    if shape['type']!=4 or collision['type']!=1 or not collision['enabled'] or emitted['moveWithTransform']!=2:
        raise ValueError(f'Fluid particle motion differs: {key(reader)}')
    # tankcapscript.Start: flowA = radius * radius * PI * 0.025.
    return {'flow_liters_per_second':value['radius']**2*math.pi*0.025*pour_speed,'tilt_degrees':value['defPourAngle'],
        'tilt_axis':[0.,0.,-1.],'angle_disabled':bool(value['noAngle']),'spill_disabled':bool(value['noPour']),
        'tube_connected':bool(value['tube']),'cant_tube':bool(value['canttube']),'flow_radius':value['radius'],
        'can_toggle':bool(value['canToggle']),
        'particles':{'source':source_id(particle),'local_pose':particle_pose,
            'capacity':initial['maxNumParticles'],'cone_degrees':shape['angle'],'cone_radius':shape['radius']['value'],
            'lifetime_seconds':initial['startLifetime']['scalar'],'radius':initial['startSize']['scalar']*0.5*collision['radiusScale'],
            'gravity_multiplier':initial['gravityModifier']['scalar'],'collision_mask':collision['collidesWith']['m_Bits'],
            'bounce':value['bounce'],'dampen':value['dampen'],
            'max_speed':value['maxSpeed'],'min_rate':value['minRate'],'max_rate':value['maxRate']}}
