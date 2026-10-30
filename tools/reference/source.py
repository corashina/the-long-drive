"""Resolve Unity identities and verify complete typed records."""
from pathlib import Path


def source_id(reader):
    return {'file': reader.assets_file.source_container_path, 'path_id': reader.path_id}


def key(reader):
    return reader.assets_file.source_container_path, reader.path_id


def discover_source(directory: Path):
    """Find serialized files by their header, including both Unity built-ins."""
    from UnityPy.enums import FileType
    from UnityPy.helpers.ImportHelper import check_file_type
    from .library.archive import _paths
    directory = directory.absolute()
    paths, _ = _paths(directory)
    serialized, streams = [], []
    for path in paths:
        name = path.relative_to(directory).as_posix()
        if path.suffix.casefold() in ('.ress', '.resource'):
            streams.append(name)
            continue
        with path.open('rb') as stream:
            kind, _ = check_file_type(stream)
        if kind == FileType.AssetsFile:
            serialized.append(name)
        elif kind in (FileType.BundleFile, FileType.WebFile):
            raise ValueError(f'Unsupported source container: {name}')
    return tuple(sorted(serialized)), tuple(sorted(streams))


def serialized_version(path: Path):
    """Read the Unity version from the serialized file metadata header."""
    import struct
    with path.open('rb') as stream:
        header = stream.read(20)
        version = struct.unpack('>I', header[8:12])[0]
        if version != 22:
            raise ValueError(f'Unsupported serialized header: {path}: {version}')
        stream.seek(48)
        raw = stream.read(64).split(b'\x00', 1)[0]
    return raw.decode('ascii')


def vector(value, width=3):
    return tuple(value[name] for name in ('x', 'y', 'z', 'w')[:width])


def normalize_arrays(node):
    """Restore array container types without changing scalar string layout."""
    if node.m_Type == 'pair' and [child.m_Name for child in node.m_Children] != ['first', 'second']:
        node.m_Type = 'managed_pair'
    if node.m_Children and node.m_Children[0].m_Type == 'Array':
        array = node.m_Children[0]
        scalar_string = node.m_Type == 'string' and len(array.m_Children) > 1 and array.m_Children[1].m_Type == 'char'
        if not scalar_string:
            node.m_Type = 'vector'
    for child in node.m_Children:
        normalize_arrays(child)


class Source:
    def __init__(self, steam_root: Path):
        import UnityPy
        from UnityPy.helpers import TypeTreeHelper
        from UnityPy.helpers.TypeTreeGenerator import TypeTreeGenerator
        from .library.types import DATA_DIRECTORY, expected_serialized_version
        self.root = steam_root.absolute()
        directory = self.root / DATA_DIRECTORY
        serialized, streams = discover_source(directory)
        self.env = UnityPy.Environment(path=str(directory))
        self.resource_streams = streams
        self.serialized_files = {}
        # Register all stream files before a decoder can request their bytes.
        for name in streams:
            self.env.load_file(str(directory / name))
        for name in serialized:
            asset = self.env.load_file(str(directory / name))
            if asset.unity_version != expected_serialized_version(name):
                raise ValueError(f'Unity version mismatch: {name}: {asset.unity_version}')
            asset.source_container_path = name
            self.serialized_files[name] = asset
        self.generator = TypeTreeGenerator('2021.3.18f1')
        self.generator.load_local_game(str(steam_root))
        self.env.typetree_generator = self.generator
        # The native reader caches type dispatch before generated-tree repairs.
        # Use UnityPy's Python reader so repaired array types control dispatch.
        TypeTreeHelper.read_typetree_boost = None
        self.objects = {}
        for asset in self.serialized_files.values():
            for obj in asset.objects.values():
                identity = key(obj)
                if identity in self.objects:
                    raise ValueError(f'Duplicate source identity: {identity}')
                self.objects[identity] = obj
        self.data_cache = {}
        self.object_cache = {}
        self.tree_cache = {}
        self.evidence = {}
        self.class_cache = {}

    def container_path(self, reader) -> str:
        return reader.assets_file.source_container_path

    def get(self, file, path_id):
        return self.objects[file, path_id]

    def transform_for(self, game_object):
        for part in self.read(game_object)['m_Component']:
            reader = self.resolve(game_object,part['component'])
            if reader.type.name in ('Transform','RectTransform'):
                return reader
        raise ValueError(f'GameObject has no Transform: {key(game_object)}')

    def chain(self, transform):
        result = []
        seen = set()
        while transform:
            if key(transform) in seen:
                raise ValueError('Transform parent cycle')
            seen.add(key(transform))
            result.append(self.read(transform))
            transform = self.resolve(transform,result[-1]['m_Father'])
        return result

    def resolve(self, owner, pointer):
        from UnityPy.classes.PPtr import PPtr
        if not pointer['m_PathID']:
            return None
        return PPtr(m_FileID=pointer['m_FileID'], m_PathID=pointer['m_PathID'], assetsfile=owner.assets_file).deref()

    def raw_data(self, reader):
        return reader.get_raw_data()

    def texture_object(self, reader):
        return self.object(reader)

    def sprite_image(self, reader):
        return self.object(reader).image

    def shader_resource(self, reader):
        return 'shader.txt', self.object(reader).export().encode('utf-8')

    def object(self, reader):
        identity = key(reader)
        if identity not in self.object_cache:
            self.object_cache[identity] = reader.parse_as_object(check_read=True)
        return self.object_cache[identity]

    def class_name(self, reader):
        identity = key(reader)
        if identity not in self.class_cache:
            head = reader.parse_monobehaviour_head()
            self.class_cache[identity] = head.m_Script.deref_parse_as_object().m_ClassName
        return self.class_cache[identity]

    def record_tree(self, reader):
        """Cache repaired type trees without caching decoded payloads."""
        if reader.type.name != 'MonoBehaviour':
            return reader._get_typetree_node()
        from UnityPy.helpers.Tpk import get_typetree_node
        standard = get_typetree_node(reader.class_id, reader.version)
        head = reader.parse_monobehaviour_head()
        if not head.m_Script.path_id:
            return standard
        tree_key = key(head.m_Script.deref()), reader.version
        if tree_key not in self.tree_cache:
            node = reader.generate_monobehaviour_node()
            header_names = {child.m_Name for child in standard.m_Children}
            node.m_Children = [*standard.m_Children, *[child for child in node.m_Children if child.m_Name not in header_names]]
            normalize_arrays(node)
            self.tree_cache[tree_key] = node
        return self.tree_cache[tree_key]

    def read_record(self, reader) -> dict:
        """Decode all bytes and verify MonoBehaviour header fields."""
        identity = key(reader)
        data = reader.parse_as_dict(node=self.record_tree(reader), check_read=True)
        if reader.type.name == 'MonoBehaviour':
            head = reader.parse_monobehaviour_head()
            for field in ('m_Script', 'm_GameObject'):
                pointer = getattr(head, field)
                if data[field] != {'m_FileID': pointer.file_id, 'm_PathID': pointer.path_id}:
                    raise ValueError(f'Header mismatch: {identity} {field}')
            for field in ('m_Enabled', 'm_Name'):
                if data[field] != getattr(head, field):
                    raise ValueError(f'Header mismatch: {identity} {field}')
        return data

    def read(self, reader):
        identity = key(reader)
        if identity not in self.data_cache:
            data = self.read_record(reader)
            self.data_cache[identity] = data
            if reader.type.name == 'MonoBehaviour':
                head = reader.parse_monobehaviour_head()
                name = self.class_name(reader) if head.m_Script.path_id else None
                self.evidence[f'{identity[0]}:{identity[1]}'] = {
                    'class': name, 'bytes': reader.byte_size, 'complete_byte_consumption': True,
                }
        return self.data_cache[identity]

    def components(self, transform):
        data = self.read(transform)
        game_object = self.resolve(transform, data['m_GameObject'])
        return game_object, [self.resolve(game_object, part['component']) for part in self.read(game_object)['m_Component']]

    def hierarchy(self, root, activate_root=False):
        result = []
        seen = set()
        def visit(reader, parent, path, active):
            if key(reader) in seen:
                raise ValueError(f'Duplicate hierarchy reference: {key(reader)}')
            seen.add(key(reader))
            game_object, components = self.components(reader)
            go = self.read(game_object)
            path = f'{path}/{go["m_Name"]}' if path else go['m_Name']
            active = active and (bool(go['m_IsActive']) or (parent is None and activate_root and key(reader) not in getattr(self,'flashlight_inactive_nodes',set())))
            result.append({'reader': reader, 'game_object': game_object, 'components': components, 'parent': parent, 'path': path, 'active': active})
            for child in self.read(reader)['m_Children']:
                visit(self.resolve(reader, child), reader, path, active)
        visit(root, None, '', True)
        return result


class LibrarySource(Source):
    """Resolve the selected scene from accepted typed records only."""
    def __init__(self, library):
        from types import SimpleNamespace as NS
        self.library = library
        self.root = library.records_root.parent / 'source'
        self.serialized_files = {c['file']:NS(source_container_path=c['file'],name=Path(c['file']).name,externals=c['externals'],version=tuple(int(v) for v in c['unity_version'].replace('f','.').split('.')),platform=19) for c in library.catalog['containers']}
        self.objects = {}
        for item in library.catalog['objects']:
            identity = item['source']
            self.objects[identity['file'],identity['path_id']] = NS(assets_file=self.serialized_files[identity['file']],path_id=identity['path_id'],type=NS(name=item['type']),byte_size=item['bytes'])
        self.data_cache = {}
        self.class_cache = {}
        self.evidence = {}

    def resolve(self, owner, pointer):
        if not pointer['m_PathID']: return None
        file = owner.assets_file.source_container_path
        if pointer['m_FileID']:
            external = owner.assets_file.externals[pointer['m_FileID']-1]
            if external['file_id'] != pointer['m_FileID']: raise ValueError('External file identity mismatch')
            file = external['path'].replace('\\','/')
            if file == 'Library/unity default resources': file = 'Resources/unity default resources'
        return self.get(file,pointer['m_PathID'])

    def read(self, reader):
        from .library.types import ObjectId
        identity = key(reader)
        if identity not in self.data_cache:
            self.data_cache[identity] = self.library.record(ObjectId(*identity))
            if reader.type.name == 'MonoBehaviour':
                self.evidence[f'{identity[0]}:{identity[1]}'] = {'class':self.class_name(reader),'bytes':reader.byte_size,'complete_byte_consumption':True}
        return self.data_cache[identity]

    def class_name(self, reader):
        script = self.resolve(reader,self.read(reader)['m_Script'])
        return self.read(script)['m_ClassName'] if script else None

    def object(self, reader):
        raise TypeError('Runtime derivation requires typed library records')

    def raw_data(self, reader):
        from .library.types import ObjectId
        return self.library.raw_object(ObjectId(*key(reader)))

    def texture_object(self, reader):
        from types import SimpleNamespace as NS
        from PIL import Image
        from .library.types import ObjectId
        identity = ObjectId(*key(reader))
        data = self.read(reader)
        with Image.open(self.library.texture(identity)) as stored:
            image = stored.copy()
        fields = {name:value for name,value in data.items() if name.startswith('m_')}
        fields['m_TextureSettings'] = NS(**data['m_TextureSettings'])
        fields['object_reader'] = NS(assets_file=reader.assets_file,path_id=reader.path_id,version=reader.assets_file.version,platform=reader.assets_file.platform)
        return NS(**fields, image=image, get_image_data=lambda:self.library.texture_data(identity))

    def sprite_image(self, reader):
        from PIL import Image
        from .library.types import ObjectId
        with Image.open(self.library.sprite(ObjectId(*key(reader)))) as stored:
            return stored.copy()

    def shader_resource(self, reader):
        from .library.types import ObjectId
        return 'shader.json', self.library.typed_payload(ObjectId(*key(reader)))
