"""Read accepted library records and selected decoded payloads."""
import hashlib
import json
from pathlib import Path
from urllib.parse import unquote
from .types import LibraryReceipt, ObjectId
from .archive import validate_paths, _check_path


def checked(root, record):
    validate_paths([record['file']])
    path = root / record['file']
    _check_path(path, root.resolve())
    content = path.read_bytes()
    if len(content) != record['bytes'] or hashlib.sha256(content).hexdigest() != record['sha256']:
        raise ValueError(f"Library file hash mismatch: {record['file']}")
    return content


class LibraryReader:
    def __init__(self, receipt: LibraryReceipt):
        self.receipt = receipt
        self.root = receipt.manifest_path.parent.resolve()
        content = receipt.manifest_path.read_bytes()
        if hashlib.sha256(content).hexdigest() != receipt.manifest_sha256:
            raise ValueError('Library manifest hash mismatch')
        manifest = self.manifest = json.loads(content)
        if manifest.get('schema_version') != 1:
            raise ValueError('Unsupported library schema')
        for field in ('catalog_sha256', 'source_manifest_sha256'):
            if manifest[field] != getattr(receipt, field):
                raise ValueError(f'Library provenance mismatch: {field}')
        self.records_root = (self.root / manifest['records_root']).resolve()
        stage = self.records_root.parent
        if self.records_root.name != 'records':
            raise ValueError('Invalid shared records root')
        from .archive import load_snapshot
        snapshot = self.snapshot = load_snapshot(stage)
        if snapshot.manifest_sha256 != receipt.source_manifest_sha256:
            raise ValueError('Shared records source mismatch')
        catalog_bytes = (stage / 'catalog.json').read_bytes()
        if hashlib.sha256(catalog_bytes).hexdigest() != receipt.catalog_sha256:
            raise ValueError('Library catalog hash mismatch')
        self.catalog = json.loads(catalog_bytes)
        records = json.loads(checked(self.records_root, manifest['records_manifest']))
        if records.get('schema_version') != 1 or any(records.get(k) != manifest[k] for k in ('catalog_sha256','source_manifest_sha256')):
            raise ValueError('Typed records provenance mismatch')
        self.record_files = {r['file']:r for r in records['records']}
        self.exports = {}
        for item in manifest['category_indexes']:
            content = checked(self.root, item)
            if item['file'] not in ('images/index.json','models/index.json','audio/index.json'):
                continue
            data = json.loads(content)
            for value in data if isinstance(data,list) else data['exports']:
                identity = ObjectId(**value['source'])
                if identity in self.exports: raise ValueError('Duplicate library owner')
                self.exports[identity] = value
        self.source_files = {r.file:r for r in snapshot.files}
        self.source_objects = {ObjectId(**v['source']):v for v in self.catalog['objects']}
        self.verified_source_files = {}
        self.selected_records = set()
        self.selected_meshes = set()
        self.selected_textures = set()
        self.selected_sprites = set()
        self.selected_audio = set()

    def record(self, source: ObjectId) -> dict:
        name = f'{source.file}/{source.path_id}.json'
        value = json.loads(checked(self.records_root, self.record_files[name]))
        if value.get('schema_version') != 1 or value['source'] != {'file':source.file,'path_id':source.path_id}:
            raise ValueError('Typed source identity mismatch')
        self.selected_records.add(source)
        def decode(item):
            if isinstance(item,list): return [decode(v) for v in item]
            if isinstance(item,dict):
                if '$pair' in item: return tuple(decode(v) for v in item['$pair'])
                if '$float' in item: return float(item['$float'])
                return {k:decode(v) for k,v in item.items()}
            return item
        return decode(value['data'])

    def mesh(self, source: ObjectId):
        from ..gltf import validate_gltf
        entry = self.exports[source]
        if entry['status'] != 'converted' or entry['details']['type'] != 'Mesh':
            raise ValueError('Selected mesh is not converted')
        files = {f['file']:f for f in entry['files']}
        for record in files.values(): checked(self.root, record)
        name = next(name for name in files if name.endswith('/geometry.gltf'))
        path = self.root / name
        raw = json.loads(path.read_bytes())
        for buffer in raw['buffers']:
            target = (path.parent / unquote(buffer['uri'])).resolve()
            relative = target.relative_to(self.root).as_posix()
            if relative not in files: raise ValueError('Mesh dependency lacks receipt')
        document = validate_gltf(path)
        self.selected_meshes.add(source)
        return document, path.parent, 0

    def texture(self, source: ObjectId) -> Path:
        entry = self.exports[source]
        if entry['status'] != 'converted' or entry['details']['type'] != 'Texture2D':
            raise ValueError('Selected texture is not converted')
        record = next(f for f in entry['files'] if f['file'].endswith('/source.png'))
        checked(self.root, record)
        self.selected_textures.add(source)
        return self.root / record['file']

    def sprite(self, source: ObjectId) -> Path:
        entry = self.exports[source]
        if entry['status'] != 'converted' or entry['details']['type'] != 'Sprite':
            raise ValueError('Selected sprite is not converted')
        record = next(f for f in entry['files'] if f['file'].endswith('/sprite.png'))
        checked(self.root, record)
        self.selected_sprites.add(source)
        return self.root / record['file']

    def audio(self, source: ObjectId) -> tuple[Path, dict]:
        from .audio import validate_wav
        entry = self.exports.get(source)
        if entry is None or entry['status'] != 'converted' or entry['details']['type'] != 'AudioClip':
            raise ValueError(f'Selected audio is not converted: {source}')
        samples = entry['details']['samples']
        if len(samples) != 1 or len(entry['files']) != 1:
            raise ValueError(f'Selected clip must have one WAV receipt: {source}')
        receipt = entry['files'][0]
        if receipt['file'] != samples[0]['file']:
            raise ValueError('Selected WAV receipt differs from sample')
        checked(self.root, receipt)
        path = self.root / receipt['file']
        metadata = validate_wav(path, self.record(source))
        if any(samples[0].get(k) != v for k, v in metadata.items()):
            raise ValueError('Selected WAV metadata differs from accepted samples')
        self.selected_audio.add(source)
        return path, {**metadata, 'sha256': receipt['sha256'], 'bytes': receipt['bytes']}

    def source_range(self, name, offset, size):
        from .archive import sha256
        record = self.source_files[name]
        path = self.snapshot.root / name
        _check_path(path, self.snapshot.root.resolve())
        before = path.stat()
        signature = (before.st_size, before.st_mtime_ns)
        if self.verified_source_files.get(name) != signature:
            if before.st_size != record.bytes or sha256(path) != record.sha256:
                raise ValueError(f'Archived source hash mismatch: {name}')
            self.verified_source_files[name] = signature
        if offset < 0 or size < 0 or offset + size > record.bytes:
            raise ValueError(f'Archived source range exceeds file: {name}')
        with path.open('rb') as stream:
            stream.seek(offset)
            content = stream.read(size)
        after = path.stat()
        if (after.st_size, after.st_mtime_ns) != signature:
            raise ValueError('Archived source changed during read')
        if len(content) != size: raise ValueError('Incomplete archived source range')
        return content

    def raw_object(self, source: ObjectId):
        self.record(source)
        record = self.source_objects[source]
        name = f'TheLongDrive_Data/{source.file}'
        if self.source_files[name].sha256 != record['container_sha256']:
            raise ValueError('Archived object container mismatch')
        return self.source_range(name, record['offset'], record['bytes'])

    def texture_data(self, source: ObjectId):
        data = self.record(source)
        stream = data['m_StreamData']
        if stream['size']:
            return self.source_range(f"TheLongDrive_Data/{stream['path']}", stream['offset'], stream['size'])
        payload = data['image data']
        return checked(self.records_root, payload['$binary'])

    def typed_payload(self, source: ObjectId):
        self.record(source)
        name = f'{source.file}/{source.path_id}.json'
        return checked(self.records_root, self.record_files[name])
