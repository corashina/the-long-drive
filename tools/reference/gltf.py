"""Export selected Unity hierarchies through pygltflib."""
import hashlib
import math
import struct
from pathlib import Path

from .source import key, source_id, vector
from typing import TYPE_CHECKING
if TYPE_CHECKING:
    from .library.reader import LibraryReader
from .materials import recover_material, export_material_textures, material_source_evidence
from .visibility import renderer_visible, recover_startup_inactive_nodes


def convert_point(value: tuple[float, float, float]) -> tuple[float, float, float]:
    return value[0], value[1], -value[2]


def convert_rotation(value):
    return -value[0], -value[1], value[2], value[3]


def convert_triangles(values):
    return [(a, c, b) for a, b, c in values]


def convert_bind_poses(matrices):
    """Reflect Unity inverse bind matrices and store glTF column order."""
    signs = (1, 1, -1, 1)
    return [tuple(matrix[f'e{row}{column}'] * signs[row] * signs[column]
                  for column in range(4) for row in range(4)) for matrix in matrices]


def transform_point(point, translation, rotation, scale):
    x, y, z = [point[i] * scale[i] for i in range(3)]
    qx, qy, qz, qw = rotation
    tx, ty, tz = 2*(qy*z-qz*y), 2*(qz*x-qx*z), 2*(qx*y-qy*x)
    return (x+qw*tx+qy*tz-qz*ty+translation[0],
            y+qw*ty+qz*tx-qx*tz+translation[1],
            z+qw*tz+qx*ty-qy*tx+translation[2])


def converted_transform(data):
    return {'translation': list(convert_point(vector(data['m_LocalPosition']))),
            'rotation': list(convert_rotation(vector(data['m_LocalRotation'], 4))),
            'scale': list(vector(data['m_LocalScale']))}


class GltfBuilder:
    def __init__(self, source, output):
        import pygltflib as gl
        self.gl = gl
        self.source = source
        self.output = output
        self.document = gl.GLTF2(asset=gl.Asset(generator='The Long Slop reference exporter'))
        self.binary = bytearray()
        self.meshes = {}
        self.materials = {}
        self.textures = {}
        self.samplers = {}
        self.mesh_data = {}
        self.material_report = []
        self.material_records = []
        self.material_dependencies = {}
        self.deferred_materials = {}

    def accessor(self, values, shape, component=5126, target=34962, buffer_index=0):
        gl = self.gl
        width = {'SCALAR': 1, 'VEC2': 2, 'VEC3': 3, 'VEC4': 4, 'MAT4':16}[shape]
        rows = [tuple(value) if width > 1 else (value,) for value in values]
        if not rows or any(len(row) != width or not all(math.isfinite(v) for v in row) for row in rows):
            raise ValueError(f'Invalid {shape} mesh attribute')
        self.binary.extend(b'\0' * (-len(self.binary) % 4))
        offset = len(self.binary)
        flat = [v for row in rows for v in row]
        code = {5126:'f',5125:'I',5123:'H'}[component]
        self.binary.extend(struct.pack(f'<{len(flat)}{code}', *flat))
        view = len(self.document.bufferViews)
        self.document.bufferViews.append(gl.BufferView(buffer=buffer_index, byteOffset=offset, byteLength=len(self.binary)-offset, target=target))
        accessor = len(self.document.accessors)
        self.document.accessors.append(gl.Accessor(bufferView=view, componentType=component, count=len(rows), type=shape,
            min=[min(row[i] for row in rows) for i in range(width)], max=[max(row[i] for row in rows) for i in range(width)]))
        return accessor

class GltfExporter(GltfBuilder):
    def __init__(self, source, output, library: "LibraryReader", asset_id=0, with_materials=True):
        super().__init__(source, output)
        self.library = library
        self.asset_id = asset_id
        self.with_materials = with_materials
        self.material_indices = {}

    def world_variants(self, records):
        """Keep selected component material recipes beside their prototype owner."""
        from .materials import FAMILIES
        materials = {}
        common = self.source.read(self.source.get('level1', 64919))['conditionmaterials']
        def add(owner, pointer):
            if pointer['m_PathID']:
                material = self.source.resolve(owner, pointer)
                materials[key(material)] = material
        for record in records:
            for component in record['components']:
                if component.type.name != 'MonoBehaviour': continue
                name = self.source.class_name(component)
                data = self.source.read(component)
                if name == 'randomRock':
                    for pointer in data['materials']: add(component, pointer)
                elif name == 'partconditionscript':
                    if data['UseMatList']:
                        for pointer in data['MatList']: add(component, pointer)
                    if not data['DontUseWorn']:
                        for field in ('defNew','defUsed','defMiddle','defOld','defRusty'): add(component, data[field])
                        kinds = {data['tipus'], data['materialTipus']}
                        kinds.update(row['tipus'] for row in data['randomTipusok'])
                        for row in common:
                            if row['tipus'] in kinds:
                                for field in ('New','Used','Middle','Old','Rusty'): add(self.source.get('level1',64919), row[field])
                elif name == 'flashlightscript':
                    for field in ('mON','mOFF'):
                        if field in data: add(component, data[field])
                    for row in data['Lamps']:
                        for field in ('mON','mOFF'): add(component,row[field])
                elif name == 'radioscript':
                    for field in ('SOn','SOff','MutatoOn','MutatoOff'): add(component, data[field])
                elif name == 'mirrorscript' and data['M']['R']['m_PathID'] and data['M']['C']['m_PathID']:
                    # mirrorscript.FStart: a live mirror shows a copy of defMirrorMaterial.
                    main = self.source.get('level1', 64919)
                    add(main, self.source.read(main)['defMirrorMaterial'])
        from .world_biome_materials import used_sets
        sets = self.source.biome_material_sets
        self.biome_sets = used_sets(self.source, records, sets)
        for index in self.biome_sets:
            for material in sets[index]:
                if material is not None: materials[key(material)] = material
        for material in materials.values():
            shader = self.source.resolve(material, self.source.read(material)['m_Shader'])
            if self.source.read(shader)['m_ParsedForm']['m_Name'] in FAMILIES:
                self.material(material)
            elif key(material) not in self.deferred_materials:
                self.deferred_materials[key(material)] = self.source.world_material_export(material)

    def material(self, reader, color_override=None, condition=None, texture_overrides=None):
        if reader is None: raise ValueError('Required renderer material has no source')
        import json
        record = recover_material(self.source, reader, color_override, condition, texture_overrides)
        identity = json.dumps(record, sort_keys=True, separators=(',', ':'))
        if identity in self.materials:
            index = self.materials[identity]
            if color_override is None and condition is None and not texture_overrides:
                self.material_indices[f"{reader.assets_file.name}:{reader.path_id}"] = index
            return index
        material_source_evidence(self.source, reader, record)
        dependencies = export_material_textures(self.source, reader, record, self.output.parent)
        self.material_dependencies.update({entry['file']: entry for entry in dependencies})
        gl = self.gl
        floats = record['properties']['floats']
        color = record['color']
        alpha = record['state']['alpha_mode']
        material = gl.Material(name=self.source.read(reader)['m_Name'],
            alphaMode={'opaque':'OPAQUE','mask':'MASK','blend':'BLEND','premultiply':'BLEND','refract':'BLEND'}[alpha],
            alphaCutoff=floats['_Cutoff'] if alpha == 'mask' else None,
            doubleSided=record['state']['cull'] == 0,
            pbrMetallicRoughness=gl.PbrMetallicRoughness(baseColorFactor=color,
                metallicFactor=floats.get('_Metallic',0.),roughnessFactor=1.-floats.get('_Glossiness',.5)),
            extras={'source': record['source'], 'asset': self.asset_id,
                    'gltf_material': len(self.material_records)})
        if '_EMISSION' in record['keywords']:
            factors = record['properties']['colors']['_EmissionColor'][:3]
            strength = max(1.,*factors)
            material.emissiveFactor = [v/strength for v in factors]
            if strength > 1:
                extension = 'KHR_materials_emissive_strength'
                material.extensions[extension] = {'emissiveStrength':strength}
                if extension not in self.document.extensionsUsed:self.document.extensionsUsed.append(extension)
        index = len(self.document.materials)
        self.materials[identity] = index; record['gltf_material'] = index
        self.material_records.append(record); self.document.materials.append(material)
        if color_override is None and condition is None and not texture_overrides:
            self.material_indices[f"{reader.assets_file.name}:{reader.path_id}"] = index
        self.material_report.append({'source': record['source'], 'shader': record['shader_family'],
            'keywords': record['keywords'], 'rule': ('Retained imported PBR; rope sag and sway remain unimplemented.'
                if record['shader_family'] == 'amplify_rope' else 'Source material record controls final binding.')})
        return index

    def accepted_mesh(self, reader):
        from .library.types import ObjectId
        identity = key(reader)
        if identity not in self.mesh_data:
            self.mesh_data[identity] = self.library.mesh(ObjectId(*identity))
        return self.mesh_data[identity]

    def rows(self, document, directory, index):
        from urllib.parse import unquote
        accessor = document.accessors[index]
        view = document.bufferViews[accessor.bufferView]
        content = (directory / unquote(document.buffers[view.buffer].uri)).read_bytes()
        code,size = {5126:('f',4),5125:('I',4),5123:('H',2),5121:('B',1)}[accessor.componentType]
        width = {'SCALAR':1,'VEC2':2,'VEC3':3,'VEC4':4,'MAT4':16}[accessor.type]
        start = (view.byteOffset or 0)+(accessor.byteOffset or 0)
        stride = view.byteStride or size*width
        return [struct.unpack_from('<'+str(width)+code,content,start+i*stride) for i in range(accessor.count)]

    def geometry(self, reader):
        document,directory,index = self.accepted_mesh(reader)
        mesh = document.meshes[index]
        positions = {p.attributes.POSITION for p in mesh.primitives}
        if len(positions) != 1: raise ValueError('Collision mesh requires shared vertices')
        points = self.rows(document,directory,next(iter(positions)))
        groups = []
        for primitive in mesh.primitives:
            if primitive.mode != 4: raise ValueError('Collision mesh requires triangles')
            values = [r[0] for r in self.rows(document,directory,primitive.indices)]
            groups.append(list(zip(values[::3],values[1::3],values[2::3])))
        return None,points,groups

    def mesh(self, mesh_reader, material_readers, color_override=None, condition=None):
        import copy
        from urllib.parse import unquote
        import json
        identity = key(mesh_reader), tuple(key(r) if r else None for r in material_readers), tuple(color_override[c] for c in ("r", "g", "b", "a")) if color_override is not None else None, json.dumps(condition, sort_keys=True)
        if identity in self.meshes: return self.meshes[identity]
        document,directory,index = self.accepted_mesh(mesh_reader)
        mesh = copy.deepcopy(document.meshes[index])
        accessors = {}
        views = {}
        def accessor(old):
            if old in accessors: return accessors[old]
            value = copy.deepcopy(document.accessors[old])
            if value.bufferView not in views:
                view = copy.deepcopy(document.bufferViews[value.bufferView])
                raw = (directory / unquote(document.buffers[view.buffer].uri)).read_bytes()
                start = view.byteOffset or 0
                self.binary.extend(b'\0' * (-len(self.binary)%4))
                offset = len(self.binary)
                self.binary.extend(raw[start:start+view.byteLength])
                view.buffer = 0; view.byteOffset = offset
                views[value.bufferView] = len(self.document.bufferViews)
                self.document.bufferViews.append(view)
            value.bufferView = views[value.bufferView]
            accessors[old] = len(self.document.accessors)
            self.document.accessors.append(value)
            return accessors[old]
        for primitive in mesh.primitives:
            submesh = primitive.extras['submesh']
            if submesh >= len(material_readers): raise ValueError('Missing submesh material')
            material = material_readers[submesh]
            deferred=False
            if getattr(self,'world_prototype',False) and material is None:primitive.extras['world_source_material']=None
            if material and getattr(self,'world_prototype',False):
                from .materials import FAMILIES
                data=self.source.read(material);shader=self.source.resolve(material,data['m_Shader'])
                shader_name=self.source.read(shader)['m_ParsedForm']['m_Name']
                if shader_name not in FAMILIES:
                    identity_material=key(material)
                    if identity_material not in self.deferred_materials:
                        self.deferred_materials[identity_material]=self.source.world_material_export(material)
                    primitive.extras['world_source_material']=source_id(material)
                    deferred=True
            primitive.material = self.material(material,color_override,condition) if material and self.with_materials and not deferred else None
            if primitive.indices is not None: primitive.indices = accessor(primitive.indices)
            for name,value in vars(primitive.attributes).items():
                if isinstance(value,int): setattr(primitive.attributes,name,accessor(value))
            primitive.targets = [{name:accessor(value) for name,value in target.items()} for target in primitive.targets]
        if mesh.extras.get('inverse_bind_accessor') is not None:
            mesh.extras['inverse_bind_accessor'] = accessor(mesh.extras['inverse_bind_accessor'])
        result = len(self.document.meshes)
        self.document.meshes.append(mesh)
        self.meshes[identity] = result
        return result

    def export(self, roots):
        gl = self.gl
        from .clothing import player_renderers, character_renderers, character_outfit
        outfit = character_outfit(self.source, roots[0]) if len(roots)==1 else None
        player_asset = outfit is not None
        if player_asset:
            selected_player_renderers={key(r) for r in (player_renderers(self.source) if key(roots[0])==('level1',39843) else character_renderers(self.source,outfit))}
            self.material(self.source.resolve(outfit,self.source.read(outfit)['defInvisible']))
        self.output.parent.mkdir(parents=True, exist_ok=True)
        records = []
        mapping = {}
        asset = getattr(self.source,'lighting_assets',{}).get(key(roots[0]))
        for material in getattr(self.source,'lighting_extra_materials',{}).get(asset,[]):self.material(material)
        # State materials that no renderer uses at start, by asset root. Their glTF indices go back to the caller.
        for material in getattr(self.source,'extra_root_materials',{}).get(key(roots[0]),[]):
            self.source.extra_material_indices[key(roots[0]),key(material)]=self.material(material)
        for root in roots:
            records.extend(self.source.hierarchy(root, activate_root=True))
        world_prototype=self.world_prototype=all(key(root) in getattr(self.source,'world_prototype_roots',set()) for root in roots)
        condition_active = getattr(self.source, 'condition_active', {})
        effective_active = {}
        for record in records:
            identity = key(record['reader'])
            local_active = getattr(self.source,'food_active',{}).get(identity,condition_active.get(identity, bool(self.source.read(record['game_object'])['m_IsActive'])))
            parent_active = effective_active[key(record['parent'])] if record['parent'] else True
            record['active'] = parent_active and (local_active or (record['parent'] is None and identity not in getattr(self.source,'flashlight_inactive_nodes',set())))
            effective_active[identity] = record['active']
        inactive_nodes = recover_startup_inactive_nodes(self.source, records)
        for record in records:
            identity = key(record['reader'])
            if identity in inactive_nodes or (record['parent'] and key(record['parent']) in inactive_nodes):
                inactive_nodes.add(identity)
                record['active'] = False
        hidden_lods=set()
        for record in records:
            for component in record['components']:
                if component.type.name != 'LODGroup':continue
                lods=self.source.read(component)['m_LODs']
                if not lods:continue
                highest={key(self.source.resolve(component,p['renderer'])) for p in lods[0]['renderers'] if p['renderer']['m_PathID']}
                for lod in lods[1:]:
                    hidden_lods.update(key(self.source.resolve(component,p['renderer'])) for p in lod['renderers'] if p['renderer']['m_PathID'])
                hidden_lods.difference_update(highest)
        for index, record in enumerate(records):
            identity = key(record['reader'])
            if identity in mapping:
                raise ValueError(f'Duplicate export root: {identity}')
            mapping[identity] = index
        missing_bones = []
        for record in records:
            reader = record['reader']
            identity = key(reader)
            data = self.source.read(reader)
            transform = converted_transform(data)
            if record['parent'] is None:
                from .collision import world_pose
                from .lighting import IDENTITY
                # Spawn code replaces the root position and rotation of these prefabs.
                if identity in getattr(self.source, 'item_geometry_nodes', set()) or identity in getattr(self.source, 'sky_roots', set()) or identity == getattr(self.source, 'ragdoll_root', None):
                    transform['translation'] = [0., 0., 0.]
                    transform['rotation'] = [0., 0., 0., 1.]
                elif key(reader)==getattr(self.source,'lighting_pole_root',None) or key(reader) in getattr(self.source,'world_prototype_roots',set()):
                    transform=IDENTITY.copy()
                elif key(reader) not in getattr(self.source,'lighting_prototype_roots',set()):
                    transform=world_pose(self.source,reader)
            node = gl.Node(name=self.source.read(record['game_object'])['m_Name'],
                extras={'reference_id': source_id(reader), 'active':record['active'], 'hierarchy_path':record['path'],
                    'layer':self.source.read(record['game_object'])['m_Layer']}, **transform)
            if world_prototype:node.extras['source_active']=bool(self.source.read(record['game_object'])['m_IsActive'])
            if player_asset:
                for r in record['components']:
                    if r.type.name.endswith('Renderer') and key(r) not in selected_player_renderers:
                        self.material_report.append({'source':source_id(r),'reason':'Auxiliary player renderer outside characterscript membership; authored transform retained.'})
            components = {r.type.name:r for r in record['components']}
            renderer = components.get('MeshRenderer') or components.get('SkinnedMeshRenderer')
            mesh_filter = components.get('MeshFilter')
            if player_asset and renderer and key(renderer) not in selected_player_renderers:
                renderer=None
            if renderer:
                renderer_data = self.source.read(renderer)
                if world_prototype:node.extras['source_renderer_enabled']=bool(renderer_data['m_Enabled'])
                node.extras['sorting_layer'] = renderer_data['m_SortingLayer']
                node.extras['sorting_order'] = renderer_data['m_SortingOrder']
            if renderer and (mesh_filter or renderer.type.name == 'SkinnedMeshRenderer'):
                component_classes={self.source.class_name(r) for r in record['components'] if r.type.name=='MonoBehaviour' and self.source.read(r)['m_Script']['m_PathID']}
                render_enabled=renderer_visible(bool(renderer_data['m_Enabled']), component_classes) and key(renderer) not in hidden_lods and key(reader) not in inactive_nodes
                if component_classes.intersection({'TextMeshPro','TMP_SubMesh'}):
                    selected_text = key(reader) in {('level1',61809),('level1',62878)}
                    selected_submesh = self.source.read(reader)['m_Father']['m_PathID'] in {61809,62878}
                    self.material_report.append({'source':source_id(renderer),
                        'adaptation':'Runtime plate glyph binding uses the checked font contract.' if selected_text or selected_submesh else 'Omitted unrelated TMP editor glyphs.'})
                    if selected_text:
                        node.extras['plate_text_source'] = source_id(next(r for r in record['components'] if r.type.name=='MonoBehaviour' and self.source.class_name(r)=='TextMeshPro'))
                        node.extras['runtime_glyph_binding'] = True
                node.extras['renderer_enabled'] = render_enabled
                node.extras['cast_shadows'] = renderer_data['m_CastShadows']
                node.extras['receive_shadows'] = bool(renderer_data['m_ReceiveShadows'])
                owner = mesh_filter or renderer
                mesh_reader = getattr(self.source, 'condition_meshes', {}).get(key(renderer)) or self.source.resolve(owner, self.source.read(owner)['m_Mesh'])
                retain_world_geometry=world_prototype and renderer_visible(True,component_classes) and key(reader) not in inactive_nodes
                if world_prototype and mesh_reader and self.source.read(mesh_reader)['m_VertexData']['m_VertexCount']==0:
                    if self.source.read(mesh_reader)['m_IndexBuffer']:
                        raise ValueError(f'{source_id(mesh_reader)} empty mesh has indices')
                    node.extras['source_empty_mesh']=source_id(mesh_reader)
                    mesh_reader=None
                if mesh_reader and (((record['active'] or identity in getattr(self.source,'food_geometry_nodes',set()) or identity in getattr(self.source,'flashlight_inactive_nodes',set()) or identity in getattr(self.source,'condition_geometry_nodes',set()) or identity in getattr(self.source,'home_cable_geometry_nodes',set())) and render_enabled) or retain_world_geometry or player_asset or identity in getattr(self.source,'item_geometry_nodes',set())):
                    materials=[self.source.resolve(renderer,p) for p in renderer_data['m_Materials']]
                    selected=getattr(self.source,'lighting_startup_materials',{}).get(key(renderer)) or getattr(self.source,'condition_materials',{}).get(key(renderer))
                    if selected:materials=[selected for _ in materials]
                    color=getattr(self.source,'condition_colors',{}).get(key(renderer)) or getattr(self.source,'cleaning_colors',{}).get(key(renderer))
                    condition=getattr(self.source,'condition_renderers',{}).get(key(renderer))
                    if condition:node.extras['condition']=condition
                    mirror = getattr(self.source, 'mirror_renderers', {}).get(key(renderer))
                    if mirror:
                        materials = [mirror]
                    if not materials or (not world_prototype and any(r is None for r in materials)):
                        raise ValueError(f'{source_id(renderer)} m_Materials: unresolved startup material')
                    if len(materials) < len(self.geometry(mesh_reader)[2]):
                        # Unity reuses the last renderer material for remaining submeshes.
                        materials += [materials[-1]] * (len(self.geometry(mesh_reader)[2]) - len(materials))
                    node.mesh = self.mesh(mesh_reader,materials,color,condition)
                    if (self.with_materials or world_prototype) and not player_asset:
                        from .render_lighting import recover_renderer
                        node.extras["source_renderer"] = recover_renderer(self.source,renderer,mesh_reader,"item_prototype" if identity in getattr(self.source,"item_geometry_nodes",set()) else "scene")
                    if renderer.type.name == 'SkinnedMeshRenderer':
                        bones = [self.source.resolve(renderer, pointer) for pointer in renderer_data['m_Bones']]
                        if bones:
                            if any(bone is not None and key(bone) not in mapping for bone in bones):
                                raise ValueError(f'Skin joint outside selected hierarchy: {key(reader)}')
                            matrices = list(self.source.read(mesh_reader)['m_BindPose'])
                            if len(matrices) != len(bones):
                                raise ValueError(f'Skin bind pose count: {key(reader)}')
                            # A missing bone leaves its vertices in place: a new child of the renderer node
                            # with an identity bind pose. Each one is a new node, because glTF joints are unique.
                            joints = []
                            for i, bone in enumerate(bones):
                                if bone is None:
                                    joints.append(len(records) + len(missing_bones))
                                    missing_bones.append(mapping[identity])
                                    matrices[i] = {f'e{r}{c}': float(r == c) for r in range(4) for c in range(4)}
                                else:
                                    joints.append(mapping[key(bone)])
                            converted = convert_bind_poses(matrices)
                            node.skin = len(self.document.skins)
                            root_bone = self.source.resolve(renderer,renderer_data['m_RootBone'])
                            self.document.skins.append(gl.Skin(joints=joints,
                                inverseBindMatrices=self.accessor(converted,'MAT4',target=None),
                                skeleton=mapping[key(root_bone)] if root_bone else None))
            self.document.nodes.append(node)
        for parent in missing_bones:
            self.document.nodes[parent].children.append(len(self.document.nodes))
            self.document.nodes.append(gl.Node(name='missing bone', extras={'missing_bone': True}))
        root_nodes = []
        for record in records:
            index = mapping[key(record['reader'])]
            if record['parent'] is None:
                root_nodes.append(index)
            else:
                self.document.nodes[mapping[key(record['parent'])]].children.append(index)
        for record in records:
            if key(record['reader']) not in getattr(self.source,'item_geometry_nodes',set()):continue
            for component in record['components']:
                if component.type.name=='MonoBehaviour' and self.source.class_name(component)=='partconditionscript':
                    for pointer in self.source.read(component)['MatList']:
                        self.material(self.source.resolve(component,pointer))
        # Cache all instrument, radio and vehicle lamp material variants in the same imported glTF.
        for record in records:
            for component in record['components']:
                if component.type.name!='MonoBehaviour' or not self.source.read(component)['m_Script']['m_PathID']:continue
                cname=self.source.class_name(component)
                if cname not in ('meterscript','mutatoscript','ledkijelzoscript','forgoszamlap','radioscript','headlightscript','rearlights'):continue
                data=self.source.read(component)
                pointers=[]
                if cname in ('meterscript','mutatoscript'):pointers=[data['OnM'],data['OffM']]
                elif cname=='radioscript':pointers=[data[k] for k in ('SOn','SOff','MutatoOn','MutatoOff')]
                elif cname=='ledkijelzoscript':pointers=[v[k] for v in data['leds'] for k in ('on','off')]
                elif cname=='headlightscript':pointers=[*(s['M'] for s in data['states']),data['MOFF']]
                elif cname=='rearlights':pointers=[data[p+k] for p in 'HFR' for k in ('LampOn','LampOff')]
                else:pointers=[v[k] for v in data['szamok'] for k in ('MOn','MOff')]
                for ptr in pointers:
                    if ptr['m_PathID']:self.material(self.source.resolve(component,ptr))
        from .conditions import export_condition_variants
        export_condition_variants(self, records, mapping)
        if world_prototype:
            self.world_variants(records)
        self.document.scenes.append(gl.Scene(nodes=root_nodes))
        self.document.scene = 0
        # A root without geometry (a particle-only gib) has no buffer.
        if self.binary:
            self.document.buffers.append(gl.Buffer(uri=self.output.with_suffix('.bin').name,byteLength=len(self.binary)))
            self.output.with_suffix('.bin').write_bytes(self.binary)
        self.document.save_json(str(self.output))
        validate_gltf(self.output)
        paths={buffer.uri for buffer in self.document.buffers}|{image.uri for image in self.document.images}
        from .runtime_package import file_receipt
        dependencies=[file_receipt(self.output.parent/path, f'reference/{path}') for path in sorted(paths)]
        dependencies = list({entry['file']: entry for entry in [*dependencies, *self.material_dependencies.values()]}.values())
        return {'path': self.output.name, 'sha256': hashlib.sha256(self.output.read_bytes()).hexdigest(),
                'dependencies':dependencies, 'materials': self.material_records, 'deferred_materials':list(self.deferred_materials.values()),
                'material_count':len(self.document.materials), 'material_indices':self.material_indices,
                'node_count':len(records), 'mesh_count':len(self.meshes), 'material_adaptations':self.material_report,
                'nodes':[{'source':source_id(r['reader']),'gltf_node':mapping[key(r['reader'])],
                    'parent':source_id(r['parent']) if r['parent'] else None,'hierarchy_path':r['path'],
                    'active':r['active'],'transform':{'translation':self.document.nodes[mapping[key(r['reader'])]].translation,
                        'rotation':self.document.nodes[mapping[key(r['reader'])]].rotation,'scale':self.document.nodes[mapping[key(r['reader'])]].scale}} for r in records]}


def validate_gltf(path):
    """Check generated glTF references, byte ranges, geometry, and rigs."""
    import json
    from urllib.parse import unquote
    import pygltflib as gl
    from pygltflib.validator import validate
    text = path.read_text(encoding='utf-8')
    document = gl.GLTF2.gltf_from_json(text)
    import warnings
    with warnings.catch_warnings():
        warnings.filterwarnings('ignore',
            message=r'^pygltf\.utils\.validator is a provisional function and may not exist in future versions\.$',
            category=UserWarning, module=r'^pygltflib\.validator$')
        validate(document)
    raw_document = json.loads(text)
    def index(value, collection, label):
        if not isinstance(value, int) or not 0 <= value < len(collection):
            raise ValueError(f'Invalid {label} index: {value}')
        return collection[value]
    buffers = []
    for buffer in document.buffers:
        content = (path.parent / unquote(buffer.uri)).read_bytes()
        if buffer.byteLength <= 0 or len(content) != buffer.byteLength:
            raise ValueError(f'Buffer size mismatch: {buffer.uri}')
        buffers.append(content)
    for view in document.bufferViews:
        content = index(view.buffer, buffers, 'buffer')
        if view.byteLength < 0 or (view.byteOffset or 0) < 0 or (view.byteOffset or 0) + view.byteLength > len(content):
            raise ValueError('Buffer view range exceeds buffer')
    shapes = {'SCALAR':1, 'VEC2':2, 'VEC3':3, 'VEC4':4, 'MAT2':4, 'MAT3':9, 'MAT4':16}
    components = {5120:('b',1),5121:('B',1),5122:('h',2),5123:('H',2),5125:('I',4),5126:('f',4)}
    for accessor in document.accessors:
        view = index(accessor.bufferView, document.bufferViews, 'accessor buffer view')
        if accessor.sparse: raise ValueError('Generated accessors must be dense')
        if accessor.type not in shapes or accessor.componentType not in components:
            raise ValueError('Invalid accessor format')
        width = shapes[accessor.type]
        size = components[accessor.componentType][1]
        stride = view.byteStride or width * size
        offset = accessor.byteOffset or 0
        if accessor.count <= 0 or offset < 0 or offset % size or ((view.byteOffset or 0) + offset) % size:
            raise ValueError('Invalid accessor count or alignment')
        if stride < width * size or stride % size:
            raise ValueError('Invalid accessor stride')
        if offset + (accessor.count-1)*stride + width*size > view.byteLength:
            raise ValueError('Accessor range exceeds buffer view')
        if (accessor.min and len(accessor.min) != width) or (accessor.max and len(accessor.max) != width):
            raise ValueError('Invalid accessor bounds width')
    def rows(accessor_index):
        accessor = index(accessor_index,document.accessors,'accessor')
        view = document.bufferViews[accessor.bufferView]
        code,size = components[accessor.componentType]
        width = shapes[accessor.type]
        stride = view.byteStride or width*size
        content = buffers[view.buffer]
        start = (view.byteOffset or 0)+(accessor.byteOffset or 0)
        for row in range(accessor.count):
            values = struct.unpack_from('<'+str(width)+code,content,start+row*stride)
            if any(not math.isfinite(value) for value in values): raise ValueError('Non-finite accessor value')
            yield values
    # Check float payloads, including mesh attributes and inverse bind matrices.
    for i,accessor in enumerate(document.accessors):
        if accessor.componentType == 5126:
            for row in rows(i): pass
    for image in document.images:
        if image.uri is not None and not (path.parent / unquote(image.uri)).is_file():
            raise ValueError(f'Missing texture: {image.uri}')
        if image.bufferView is not None: index(image.bufferView,document.bufferViews,'image buffer view')
    for texture in document.textures:
        index(texture.source,document.images,'texture image')
        if texture.sampler is not None: index(texture.sampler,document.samplers,'texture sampler')
    for material in document.materials:
        if material.pbrMetallicRoughness:
            pbr = material.pbrMetallicRoughness
            if len(pbr.baseColorFactor) != 4 or any(not math.isfinite(v) or not 0 <= v <= 1 for v in (*pbr.baseColorFactor,pbr.metallicFactor,pbr.roughnessFactor)):
                raise ValueError('Invalid PBR material factor')
        strength = material.extensions.get('KHR_materials_emissive_strength', {}).get('emissiveStrength', 1.)
        if not math.isfinite(strength) or strength < 0:raise ValueError('Invalid emissive strength')
        if len(material.emissiveFactor) != 3 or any(not math.isfinite(v) or not 0 <= v <= 1 for v in material.emissiveFactor): raise ValueError('Invalid emissive factor')
        infos = [material.normalTexture,material.occlusionTexture,material.emissiveTexture]
        if material.pbrMetallicRoughness:
            infos.extend((material.pbrMetallicRoughness.baseColorTexture,material.pbrMetallicRoughness.metallicRoughnessTexture))
        for info in infos:
            if info is not None: index(info.index,document.textures,'material texture')
    for mesh in document.meshes:
        if not mesh.primitives: raise ValueError('Mesh has no primitives')
        target_counts = set()
        for primitive in mesh.primitives:
            attributes = {name:value for name,value in vars(primitive.attributes).items() if isinstance(value,int)}
            position = index(attributes.get('POSITION'),document.accessors,'position accessor')
            if position.type != 'VEC3' or position.componentType != 5126: raise ValueError('Invalid position accessor')
            for name,value in attributes.items():
                attribute = index(value,document.accessors,'attribute accessor')
                if attribute.count != position.count: raise ValueError('Attribute count mismatch')
                expected = 'VEC2' if name.startswith('TEXCOORD_') else {'NORMAL':'VEC3','TANGENT':'VEC4','JOINTS_0':'VEC4','WEIGHTS_0':'VEC4'}.get(name)
                if expected and attribute.type != expected: raise ValueError(f'Invalid {name} shape')
                if name.startswith('JOINTS_') and attribute.componentType not in (5121,5123): raise ValueError('Invalid joint component type')
            count = position.count
            if primitive.indices is not None:
                indices = index(primitive.indices,document.accessors,'primitive indices')
                if indices.type != 'SCALAR' or indices.componentType not in (5121,5123,5125): raise ValueError('Invalid index accessor')
                count = indices.count
                if any(row[0] >= position.count for row in rows(primitive.indices)): raise ValueError('Primitive vertex index exceeds vertex count')
            mode = primitive.mode if primitive.mode is not None else 4
            if mode not in range(7) or (mode == 1 and count % 2) or (mode == 4 and count % 3): raise ValueError('Invalid primitive topology')
            if (mode in (2,3) and count < 2) or (mode in (5,6) and count < 3): raise ValueError('Insufficient primitive vertices')
            if primitive.material is not None: index(primitive.material,document.materials,'primitive material')
            target_counts.add(len(primitive.targets or []))
            for target in primitive.targets or []:
                for semantic,value in target.items():
                    accessor = index(value,document.accessors,'morph accessor')
                    if semantic not in ('POSITION','NORMAL','TANGENT') or accessor.type != 'VEC3' or accessor.componentType != 5126 or accessor.count != position.count:
                        raise ValueError('Invalid morph target')
        if len(target_counts) > 1: raise ValueError('Morph count differs between primitives')
        if mesh.weights and target_counts and len(mesh.weights) != next(iter(target_counts)): raise ValueError('Mesh morph weight count mismatch')
    parents = {}
    for i,node in enumerate(document.nodes):
        if node.mesh is not None: index(node.mesh,document.meshes,'node mesh')
        for child in node.children:
            index(child,document.nodes,'child node')
            if child in parents: raise ValueError('Node has multiple parents')
            parents[child] = i
        weights = raw_document['nodes'][i].get('weights',[])
        node.weights = weights
        if weights:
            mesh = index(node.mesh,document.meshes,'morph node mesh')
            if len(weights) != len(mesh.weights) or any(not math.isfinite(v) for v in weights): raise ValueError('Node morph weight count mismatch')
        if node.skin is not None:
            skin = index(node.skin,document.skins,'node skin')
            if node.mesh is None: raise ValueError('Skinned node has no mesh')
            for primitive in document.meshes[node.mesh].primitives:
                joints = primitive.attributes.JOINTS_0
                weights_attribute = primitive.attributes.WEIGHTS_0
                if joints is None or weights_attribute is None: raise ValueError('Skinned primitive lacks joints or weights')
                if any(j >= len(skin.joints) for row in rows(joints) for j in row): raise ValueError('Skin joint attribute exceeds joint count')
                if any(any(w<0 or w>1 for w in row) or abs(sum(row)-1)>.002 for row in rows(weights_attribute)):raise ValueError('Invalid skin weights')
    for i in range(len(document.nodes)):
        seen = set()
        parent = i
        while parent in parents:
            if parent in seen: raise ValueError('Node hierarchy cycle')
            seen.add(parent)
            parent = parents[parent]
    for skin in document.skins:
        if not skin.joints or len(set(skin.joints))!=len(skin.joints): raise ValueError('Skin has no unique authored joints')
        for joint in skin.joints: index(joint,document.nodes,'skin joint')
        if skin.skeleton is not None: index(skin.skeleton,document.nodes,'skin skeleton')
        if skin.inverseBindMatrices is not None:
            accessor = index(skin.inverseBindMatrices,document.accessors,'inverse bind accessor')
            if accessor.type != 'MAT4' or accessor.componentType != 5126 or accessor.count != len(skin.joints): raise ValueError('Skin bind pose count mismatch')
    for scene in document.scenes:
        for root in scene.nodes:
            index(root,document.nodes,'scene root')
            if root in parents: raise ValueError('Scene root has a parent')
    if document.scene is not None: index(document.scene,document.scenes,'default scene')
    validate_animation_channels(document,rows)
    return document


def export_gltf(roots, output: Path, library, asset_id: int, materials=True) -> dict:
    """Roots are (Source, selected Transform readers). The sky prototype has no materials."""
    source, readers = roots
    return GltfExporter(source, output, library, asset_id, materials).export(readers)



def validate_animation_channels(document, rows):
    """Check animation accessors without reading shared geometry payloads."""
    def item(index,items,label):
        if not isinstance(index,int) or not 0<=index<len(items):raise ValueError(f'Invalid animation {label}')
        return items[index]
    for animation in document.animations:
        targets=set()
        for channel in animation.channels:
            sampler=item(channel.sampler,animation.samplers,'sampler')
            node=item(channel.target.node,document.nodes,'node')
            path=channel.target.path
            target=(channel.target.node,path)
            if target in targets:raise ValueError('Duplicate animation target')
            targets.add(target)
            if path not in ('translation','rotation','scale','weights'):raise ValueError('Invalid animation target path')
            incoming=item(sampler.input,document.accessors,'input')
            outgoing=item(sampler.output,document.accessors,'output')
            if incoming.type!='SCALAR' or incoming.componentType!=5126 or outgoing.componentType!=5126:
                raise ValueError('Invalid animation accessor format')
            for accessor in (incoming,outgoing):
                view=item(accessor.bufferView,document.bufferViews,'buffer view')
                if view.target is not None:raise ValueError('Animation buffer view has a vertex target')
            times=[r[0] for r in rows(sampler.input)]
            if not times or times[0]<0 or any(not math.isfinite(t) for t in times) or any(b<=a for a,b in zip(times,times[1:])):
                raise ValueError('Invalid animation key times')
            interpolation=sampler.interpolation or 'LINEAR'
            if interpolation not in ('STEP','LINEAR','CUBICSPLINE'):raise ValueError('Invalid animation interpolation')
            factor=3 if interpolation=='CUBICSPLINE' else 1
            count=len(times)*factor
            shape='VEC4' if path=='rotation' else 'VEC3'
            if path=='weights':
                mesh=item(node.mesh,document.meshes,'morph mesh')
                widths={len(p.targets or []) for p in mesh.primitives}
                if len(widths)!=1 or not next(iter(widths)):raise ValueError('Invalid animation morph targets')
                count*=next(iter(widths));shape='SCALAR'
            if outgoing.type!=shape or outgoing.count!=count:raise ValueError('Animation output dimensions differ')
            values=list(rows(sampler.output))
            if any(not math.isfinite(v) for row in values for v in row):raise ValueError('Non-finite animation output')
            if path=='rotation':
                quaternions=values[1::3] if factor==3 else values
                if any(abs(sum(v*v for v in q)-1)>1e-4 for q in quaternions):
                    raise ValueError('Invalid animation quaternion')
