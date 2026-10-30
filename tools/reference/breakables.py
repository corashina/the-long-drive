"""Checked breakablescript records of selected world and item prototypes."""
from .source import key, source_id

# Record field: breakablescript field. Values are floats.
NUMBERS = {'health': 'health', 'shoot_health': 'shootHealth', 'explosion_health': 'explosionHealth',
           'decrease_above_health': 'onlyDecreaseAfterAttackIfAboveHealth', 'child_modifier': 'childModifier'}
# Record field: breakablescript field. Values are flags.
FLAGS = {'shoot_different': 'shootDifferent', 'explosion_different': 'explosionDifferent', 'no_shoot': 'noShoot',
         'decrease_after_attack': 'decreaseAfterAttack', 'only_decrease_above': 'onlyDecreaseAfterAttackIfAbove',
         'only_velocity': 'onlyVelocity', 'no_trigger': 'noTrigger', 'no_collider': 'noCollider',
         'no_gib': 'noGib', 'no_destroy': 'noDestroy'}


def owners(source, manifest):
    """(kind, prototype, hierarchy, node ids by Transform) of each selected prototype."""
    result = []
    for p in manifest['world']['prototypes']:
        hierarchy = source.hierarchy(source.get(p['source']['file'], p['source']['path_id']), activate_root=True)
        if [source_id(n['reader']) for n in hierarchy] != [n['source'] for n in p['nodes']]:
            raise ValueError(f"World prototype hierarchy differs: {p['source']}")
        result.append(('world', p['source'], hierarchy, {key(n['reader']): i for i, n in enumerate(hierarchy)}))
    nodes = {(n['source']['file'], n['source']['path_id']): n['id'] for n in manifest['nodes']}
    for p in manifest['item_prototypes']:
        hierarchy = source.hierarchy(source.get(p['source']['file'], p['source']['path_id']), activate_root=True)
        result.append(('item', p['id'], hierarchy, {key(n['reader']): nodes[key(n['reader'])] for n in hierarchy}))
    return result


def breakable(source, c, actor=False) -> dict:
    """One breakablescript, as Start leaves it. `actor`: the creature runtime owns its AI link."""
    d = source.read(c)
    if not actor and (d['AI']['m_PathID'] or d['newAI']['m_PathID']):
        raise ValueError(f'breakablescript AI link is not supported before X03: {key(c)}')
    # flashlightscript.Start sets lamp. A set lamp field is not audited.
    if d['lamp']['m_PathID'] or d['destroyed']:
        raise ValueError(f'breakablescript start state is not audited: {key(c)}')

    def gib(field):
        target = source.resolve(c, d[field])
        if target is None:
            return None
        if target.type.name != 'GameObject':
            raise ValueError(f'breakablescript {field} is not GameObject: {key(c)}')
        return source_id(source.transform_for(target))
    clips = []
    for pointer in d['clips']:
        clip = source.resolve(c, pointer)
        if clip is None or clip.type.name != 'AudioClip':
            raise ValueError(f'breakablescript clip is not AudioClip: {key(c)}')
        source.item_clips.add(key(clip))
        clips.append(source_id(clip))
    children = []
    for pointer in d['childs']:
        child = source.resolve(c, pointer)
        if child is None or child.type.name != 'MonoBehaviour' or source.class_name(child) != 'breakchilds':
            raise ValueError(f'breakablescript child is not breakchilds: {key(c)}')
        f = source.read(child)
        parent = source.resolve(child, f['P'])
        if parent is None or key(parent) != key(c):
            raise ValueError(f'breakchilds parent differs: {key(child)}')
        children.append({'transform': source_id(source.transform_for(source.resolve(child, f['m_GameObject']))),
                         'own_modifier': bool(f['ownModifier']), 'modifier': f['modifier']})
    return {'source': source_id(c), 'owners': [], **{k: d[v] for k, v in NUMBERS.items()},
            **{k: bool(d[v]) for k, v in FLAGS.items()},
            # breakablescript.Start: massModifier 0 becomes 1.
            'mass_modifier': d['massModifier'] or 1., 'children': children,
            'gib': gib('Gib'), 'reduced_gib': gib('reducedGib'), 'clips': clips, 'lamp': None}


def breakable_records(source, prototypes, controls) -> list[dict]:
    """One record per breakablescript. `controls` holds the exported flashlightscript keys."""
    records = {}; lamps = {}
    for kind, prototype, hierarchy, node_ids in prototypes:
        for node in hierarchy:
            for c in node['components']:
                if c.type.name != 'MonoBehaviour':
                    continue
                name = source.class_name(c)
                if name == 'breakablescript':
                    if key(c) not in records:
                        records[key(c)] = breakable(source, c)
                    records[key(c)]['owners'].append({'kind': kind, 'prototype': prototype, 'node': node_ids[key(node['reader'])]})
                elif name == 'flashlightscript':
                    # flashlightscript.Break(p) breaks each lamp whose breaktrigger holds p.
                    for index, lamp in enumerate(source.read(c)['Lamps']):
                        for pointer in lamp['breaktrigger']:
                            target = source.resolve(c, pointer)
                            if target is None:
                                raise ValueError(f'flashlightscript breaktrigger is null: {key(c)}')
                            if key(c) not in controls:
                                raise ValueError(f'Lamp link to a flashlightscript outside the export: {key(c)}')
                            link = lamps.setdefault(key(target), {'control': source_id(c), 'lamps': []})
                            if link['control'] != source_id(c):
                                raise ValueError(f'breakablescript has two lamp controls: {key(target)}')
                            if index not in link['lamps']:
                                link['lamps'].append(index)
    for identity, link in lamps.items():
        if identity not in records:
            raise ValueError(f'Lamp breaktrigger outside selected prototypes: {identity}')
        records[identity]['lamp'] = {'control': link['control'], 'lamps': sorted(link['lamps'])}
    return list(records.values())


def check_gibs(records, prototypes) -> None:
    """Each Gib and reducedGib link is a gib prototype, and each gib prototype has a link."""
    gibs = [p['id'] for p in prototypes if p['item_id'] is None]
    links = [r[f] for r in records for f in ('gib', 'reduced_gib') if r[f] is not None]
    missing = [g for g in links if g not in gibs]
    if missing:
        raise ValueError(f'Breakable gibs are not item_prototypes.GIBS: {missing}')
    unused = [g for g in gibs if g not in links]
    if unused:
        raise ValueError(f'Gib prototypes without a breakable: {unused}')


def recover_breakables(source, manifest) -> list[dict]:
    controls = {(r['source']['file'], r['source']['path_id']) for r in manifest['world']['records']
                if r['class_name'] == 'flashlightscript'}
    records = breakable_records(source, owners(source, manifest), controls)
    check_gibs(records, manifest['item_prototypes'])
    return records


def recover_gib_constants(source) -> dict:
    """mainscript values that gibscript.Start reads."""
    main = source.get('level1', 64919)
    d = source.read(main)
    return {'source': source_id(main), 'random_rotation': d['gibMainChildsRandomRot'],
            'random_position': d['gibMainChildsRandomPos'], 'add_force': d['gibMainChildsAddForce']}
