"""Source biome material sets (`materialpairscript`) and the prototype nodes that swap them (`materialChangeScript`)."""
from .source import key, source_id
from .library.types import ObjectId

SETS_CLASS = 'materialpairscript'
SWAP_CLASS = 'materialChangeScript'
RENDERERS = {'MeshRenderer', 'SkinnedMeshRenderer', 'ParticleSystemRenderer', 'LineRenderer', 'TrailRenderer', 'BillboardRenderer', 'SpriteRenderer'}


def scene_component(source, reader, class_name, scene='level'):
    """Find the one MonoBehaviour of a class in the scenes whose name starts with `scene`.
    The catalog dependencies name its MonoScript."""
    scripts = {key(o) for o in source.objects.values() if o.type.name == 'MonoScript' and source.read(o)['m_ClassName'] == class_name}
    found = [o for o in source.objects.values()
             if o.type.name == 'MonoBehaviour' and o.assets_file.source_container_path.startswith(scene)
             and any((d['file'], d['path_id']) in scripts for d in reader.source_objects[ObjectId(*key(o))]['dependencies'])
             and source.class_name(o) == class_name]
    if len(found) != 1:
        raise ValueError(f'Expected one scene {class_name}, found {len(found)}')
    return found[0]


def recover_sets(source, owner):
    """Read `materials[].m` as [desert, winter, grass]. The first set of a desert material wins, as in the source loop."""
    sets = []
    deserts = set()
    for index, row in enumerate(source.read(owner)['materials']):
        if len(row['m']) < 3:
            raise ValueError(f'{SETS_CLASS} set {index}: needs desert, winter and grass entries, has {len(row["m"])}')
        materials = [source.resolve(owner, pointer) for pointer in row['m'][:3]]
        if materials[0] is None:
            raise ValueError(f'{SETS_CLASS} set {index}: missing desert material')
        if any(m is not None and m.type.name != 'Material' for m in materials):
            raise ValueError(f'{SETS_CLASS} set {index}: entry is not a Material')
        if key(materials[0]) in deserts:
            continue
        deserts.add(key(materials[0]))
        sets.append(materials)
    if not sets:
        raise ValueError(f'{SETS_CLASS} has no material sets')
    return sets


def swaps(source, components):
    """True when an enabled `materialChangeScript` is in the components."""
    return any(c.type.name == 'MonoBehaviour' and source.class_name(c) == SWAP_CLASS and source.read(c)['m_Enabled'] for c in components)


def used_sets(source, records, sets):
    """Indices of sets whose desert material is the first material of a renderer at or under a marked node."""
    children = {}
    for record in records:
        if record['parent'] is not None:
            children.setdefault(key(record['parent']), []).append(key(record['reader']))
    pending = [key(r['reader']) for r in records if swaps(source, r['components'])]
    under = set()
    while pending:
        identity = pending.pop()
        if identity not in under:
            under.add(identity)
            pending.extend(children.get(identity, []))
    deserts = {key(s[0]): i for i, s in enumerate(sets)}
    used = set()
    for record in records:
        if key(record['reader']) not in under:
            continue
        for component in record['components']:
            if component.type.name not in RENDERERS:
                continue
            pointers = source.read(component)['m_Materials']
            first = source.resolve(component, pointers[0]) if pointers else None
            if first is not None and key(first) in deserts:
                used.add(deserts[key(first)])
    return used


def check_exported(sets, used, exported, name):
    for index in used:
        for material in sets[index]:
            if material is not None and key(material) not in exported:
                raise ValueError(f'{name}: biome material set {index} material {key(material)} is not exported')


def export_sets(sets, used, selected):
    """Used sets as [desert, winter or None, grass or None] source ids, in source order."""
    result = []
    for index in sorted(used):
        for material in sets[index]:
            if material is not None:
                selected.receipt(material)
        result.append([source_id(m) if m is not None else None for m in sets[index]])
    return result
