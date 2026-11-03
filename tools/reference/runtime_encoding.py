"""Compact runtime forms of the largest manifest sections. Export code keeps its own shapes.

The Rust reference structs read only these forms:
- world values: plain JSON. A link is {"$":[file,path_id]}. Records are sorted objects.
- world receipts: source ids only. Hashes stay in the world provenance sidecar.
- convex and trimesh colliders: base64 of little-endian f32 points and u32 triangles.
- material textures: rendering.textures holds each unique storage. A texture storage is its index.
"""
import base64
import json
import struct


def world_value(value):
    kind = value['kind']
    if kind == 'empty':
        return None
    if kind == 'link':
        return {'$': [value['value']['file'], value['value']['path_id']]}
    if kind == 'array':
        return [world_value(v) for v in value['value']]
    if kind == 'record':
        return world_fields(value['value'])
    if kind == 'number':
        return float(value['value'])
    return value['value']


def world_fields(fields):
    return {f['name']: world_value(f['value']) for f in fields}


def packed(rows, code):
    return base64.b64encode(struct.pack(f'<{3 * len(rows)}{code}', *(x for row in rows for x in row))).decode()


def pack_colliders(value):
    """Pack collider point lists in place. A shared geometry dict is packed once."""
    if isinstance(value, dict):
        if value.get('kind') in ('convex', 'trimesh') and isinstance(value.get('points'), list):
            value['points'] = packed(value['points'], 'f')
            if 'triangles' in value:
                value['triangles'] = packed(value['triangles'], 'I')
            return
        for child in value.values():
            pack_colliders(child)
    elif isinstance(value, list):
        for child in value:
            pack_colliders(child)


def material_textures(manifest):
    """Pairs of material and texture entry, with the storage read from the runtime table."""
    table = manifest['rendering']['textures']
    for material in manifest['rendering']['materials']:
        for texture in material['properties']['textures']:
            yield material, {**texture, 'storage': table[texture['storage']]}


def encode_runtime(manifest):
    """Replace the large export shapes with their runtime forms. Shared input objects stay unchanged."""
    world = manifest['world']
    world['records'] = [{**r, 'fields': world_fields(r['fields'])} for r in world['records']]
    world['initialization'] = {**world['initialization'], 'components': [
        {**c, 'fields': [{**f, 'value': world_value(f['value'])} for f in c['fields']]}
        for c in world['initialization']['components']]}
    world['receipts'] = [r['source'] for r in world['receipts']]
    pack_colliders(manifest)
    table, index = [], {}
    def indexed(texture):
        key = json.dumps(texture['storage'], sort_keys=True)
        if key not in index:
            index[key] = len(table)
            table.append(texture['storage'])
        return {**texture, 'storage': index[key]}
    materials = [{**m, 'properties': {**m['properties'], 'textures': [indexed(t) for t in m['properties']['textures']]}}
                 for m in manifest['rendering']['materials']]
    manifest['rendering'] = {'textures': table, 'materials': materials}
