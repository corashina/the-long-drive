"""Checked painting material variants. Only the selected variant is admitted."""
import hashlib
from .source import source_id
from .gltf import GltfExporter

def export_paintings(source, reader, selected, world, output, first_asset):
    materials = {}
    for prototype in world['prototypes']:
        root = source.get(prototype['source']['file'],prototype['source']['path_id'])
        for node in source.hierarchy(root,activate_root=True):
            for component in node['components']:
                if component.type.name == 'MonoBehaviour' and source.class_name(component) == 'paintingScript':
                    data = source.read(component)
                    material = source.resolve(component,data['defmat'])
                    materials[(material.assets_file.name,material.path_id)] = material
    main = source.get('level1',64919)
    textures = [source.resolve(main,p) for p in source.read(main)['paintings']]
    result = []
    for material in materials.values():
        selected.record(material)
        for texture in textures:
            selected.receipt(texture)
            asset = first_asset + len(result)
            name = f'painting-{material.assets_file.name}-{material.path_id}-{texture.assets_file.name}-{texture.path_id}.gltf'
            exporter = GltfExporter(source,output/name,reader,asset)
            exporter.material(material,texture_overrides={'_MainTex':texture})
            exporter.document.save_json(str(output/name))
            result.append(dict(material=source_id(material),texture=source_id(texture),asset=asset,
                asset_record=dict(id=asset,usage='world_material',path='reference/'+name,sha256=hashlib.sha256((output/name).read_bytes()).hexdigest(),root=source_id(material),mesh_count=0,material_count=1,dependencies=list(exporter.material_dependencies.values())),
                materials=[dict(r,asset=asset) for r in exporter.material_records]))
    return result
