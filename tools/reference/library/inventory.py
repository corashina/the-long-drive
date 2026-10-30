"""Index object identities and original serialized byte ranges."""
from collections import Counter
from dataclasses import asdict

from .archive import verify_snapshot
from .types import APP_ID, BUILD_ID, UNITY_VERSION, SCHEMA_VERSION, DATA_DIRECTORY, Catalog, Snapshot, ObjectId, ObjectRecord
from ..source import Source


def inventory_source(source: Source, snapshot: Snapshot) -> Catalog:
    if source.root.resolve() != snapshot.root.resolve():
        raise ValueError('Inventory source differs from the verified archive')
    verify_snapshot(snapshot)
    files = {record.file: record for record in snapshot.files}
    objects = {}
    counts = Counter()
    for reader in sorted(source.objects.values(), key=lambda item: (source.container_path(item), item.path_id)):
        container = source.container_path(reader)
        file = files[f'{DATA_DIRECTORY}/{container}']
        if reader.byte_start < 0 or reader.byte_size < 0 or reader.byte_start + reader.byte_size > file.bytes:
            raise ValueError(f'Object range escapes source file: {container}:{reader.path_id}')
        identity = ObjectId(container, reader.path_id)
        objects[identity] = ObjectRecord(identity, reader.type.name, reader.class_id,
                                         reader.byte_start, reader.byte_size, file.sha256)
        counts[reader.type.name] += 1
    return Catalog(objects, dict(sorted(counts.items())), {identity: () for identity in objects})


def catalog_data(catalog: Catalog, source: Source, snapshot: Snapshot) -> dict:
    """Write shared record descriptions and source file evidence in canonical order."""
    files = {record.file: record for record in snapshot.files}
    containers = []
    for name, asset in sorted(source.serialized_files.items()):
        record = files[f'{DATA_DIRECTORY}/{name}']
        containers.append({'file': name, 'bytes': record.bytes, 'sha256': record.sha256,
                           'unity_version': asset.unity_version, 'object_count': len(asset.objects),
                           'externals': [{'file_id': index, 'path': item.path,
                                          'guid': item.guid.hex() if item.guid else None, 'type': item.type}
                                         for index, item in enumerate(asset.externals, 1)]})
    streams = [{'file': name, 'bytes': files[f'{DATA_DIRECTORY}/{name}'].bytes,
                'sha256': files[f'{DATA_DIRECTORY}/{name}'].sha256} for name in source.resource_streams]
    objects = []
    for identity, record in sorted(catalog.objects.items()):
        data = asdict(record)
        data['dependencies'] = [asdict(item) for item in catalog.dependencies[identity]]
        objects.append(data)
    return {'schema_version': SCHEMA_VERSION, 'app_id': APP_ID, 'build_id': BUILD_ID,
            'unity_version': UNITY_VERSION, 'source_manifest_sha256': snapshot.manifest_sha256,
            'containers': containers, 'streams': streams, 'counts': catalog.counts, 'objects': objects}
