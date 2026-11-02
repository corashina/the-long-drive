"""Publish a complete copy only after source and archive hashes match."""
from dataclasses import asdict
import hashlib
import json
import os
import re
import shutil
import stat
import tempfile
import unicodedata
from pathlib import Path, PurePosixPath

from .types import APP_ID, BUILD_ID, UNITY_VERSION, SCHEMA_VERSION, DATA_DIRECTORY, Snapshot, FileRecord, expected_serialized_version


def validate_paths(paths):
    seen = set()
    reserved = {'con', 'prn', 'aux', 'nul', *{f'com{i}' for i in range(1, 10)}, *{f'lpt{i}' for i in range(1, 10)}}
    for name in paths:
        path = PurePosixPath(name)
        if (not name or path.is_absolute() or str(path) != name or '\\' in name
                or any(part in ('.', '..') or part[-1:] in (' ', '.') or ':' in part
                       or part.split('.')[0].casefold() in reserved for part in path.parts)):
            raise ValueError(f'Unsafe Windows path: {name}')
        normalized = unicodedata.normalize('NFC', name).casefold()
        if normalized in seen:
            raise ValueError(f'Normalized path collision: {name}')
        seen.add(normalized)


def _check_path(path, root):
    attributes = path.lstat()
    if (path.is_symlink() or getattr(attributes, 'st_file_attributes', 0) & stat.FILE_ATTRIBUTE_REPARSE_POINT
            or not path.resolve().is_relative_to(root)):
        raise ValueError(f'Filesystem link or path escape: {path}')
    return attributes


def sha256(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def _paths(root):
    root = root.absolute()
    _check_path(root, root.resolve())
    files, directories = [], []
    def visit(directory):
        for path in sorted(directory.iterdir()):
            attributes = _check_path(path, root.resolve())
            relative = path.relative_to(root).as_posix()
            if stat.S_ISDIR(attributes.st_mode):
                directories.append(relative)
                visit(path)
            elif stat.S_ISREG(attributes.st_mode):
                files.append(path)
            else:
                raise ValueError(f'Unsupported source file: {path}')
    visit(root)
    validate_paths([*(path.relative_to(root).as_posix() for path in files), *directories])
    return files, sorted(directories)


def scan_files(root: Path):
    root = root.absolute()
    files, _ = _paths(root)
    result = []
    for path in files:
        try:
            before = path.stat()
            digest = sha256(path)
            after = path.stat()
        except OSError as error:
            raise OSError(f'Cannot hash source file: {path}: {error}') from error
        if (before.st_size, before.st_mtime_ns) != (after.st_size, after.st_mtime_ns):
            raise ValueError(f'Source changed while hashing: {path}')
        result.append(FileRecord(path.relative_to(root).as_posix(), after.st_size, digest))
    return tuple(sorted(result, key=lambda item: item.file))


def _steam_values(raw):
    values = dict(re.findall(r'"([^"\r\n]+)"\s+"([^"\r\n]+)"', raw.decode('utf-8')))
    if values.get('appid') != str(APP_ID) or values.get('buildid') != str(BUILD_ID):
        raise ValueError(f'Steam identity mismatch: expected app {APP_ID}, build {BUILD_ID}')
    return values


def _unity_identity(root):
    from ..source import discover_source, serialized_version
    files, _ = discover_source(root / DATA_DIRECTORY)
    if not files:
        raise ValueError('Source has no serialized files')
    versions = []
    for name in files:
        version = serialized_version(root / DATA_DIRECTORY / name)
        if version != expected_serialized_version(name):
            raise ValueError(f'Unity identity mismatch: {name}: {version}')
        versions.append({'path': name, 'unity_version': version})
    return versions


def load_snapshot(work_root: Path) -> Snapshot:
    directory = work_root.absolute()
    path = directory / 'source-manifest.json'
    data = json.loads(path.read_text(encoding='utf-8'))
    files = tuple(FileRecord(**record) for record in data['files'])
    return Snapshot(directory / 'source', files, path, sha256(path))


def _read_manifest(snapshot):
    expected_root = snapshot.manifest_path.parent / 'source'
    if snapshot.root.absolute() != expected_root.absolute():
        raise ValueError('Snapshot root differs from its manifest')
    _check_path(snapshot.manifest_path, snapshot.manifest_path.parent.resolve())
    if sha256(snapshot.manifest_path) != snapshot.manifest_sha256:
        raise ValueError('Source manifest hash changed')
    manifest = json.loads(snapshot.manifest_path.read_text(encoding='utf-8'))
    if (manifest.get('schema_version'), manifest.get('app_id'), manifest.get('build_id'), manifest.get('unity_version')) != (
            SCHEMA_VERSION, APP_ID, BUILD_ID, UNITY_VERSION):
        raise ValueError('Source manifest schema or build identity mismatch')
    if manifest.get('steam_manifest', {}).get('path') != 'steam-manifest.acf':
        raise ValueError('Invalid Steam manifest evidence path')
    validate_paths([record['file'] for record in manifest['files']] + manifest['directories'])
    if tuple(FileRecord(**record) for record in manifest['files']) != snapshot.files:
        raise ValueError('Snapshot files differ from the source manifest')
    return manifest


def verify_snapshot(snapshot: Snapshot) -> None:
    manifest = _read_manifest(snapshot)
    evidence = snapshot.manifest_path.parent / 'steam-manifest.acf'
    _check_path(evidence, evidence.parent.resolve())
    _steam_values(evidence.read_bytes())
    if sha256(evidence) != manifest['steam_manifest']['sha256']:
        raise ValueError('Steam manifest evidence hash changed')
    actual = scan_files(snapshot.root)
    if actual != snapshot.files:
        raise ValueError('Snapshot file list, size, or hash changed')
    if _paths(snapshot.root)[1] != manifest['directories']:
        raise ValueError('Snapshot directory list changed')
    if _unity_identity(snapshot.root) != manifest['serialized_versions']:
        raise ValueError('Archived serialized Unity versions changed')


def snapshot_source(steam_root: Path, work_root: Path) -> Snapshot:
    steam_root = steam_root.absolute()
    work_root = work_root.absolute()
    if (work_root.resolve().is_relative_to(steam_root.resolve())
            or steam_root.resolve().is_relative_to(work_root.resolve())):
        raise ValueError('Keep the source archive separate from the Steam installation')
    evidence_path = steam_root.parents[1] / f'appmanifest_{APP_ID}.acf'
    evidence = evidence_path.read_bytes()
    _steam_values(evidence)
    before = scan_files(steam_root)
    directories = _paths(steam_root)[1]
    versions = _unity_identity(steam_root)
    manifest_path = work_root / 'source-manifest.json'
    if manifest_path.exists():
        snapshot = load_snapshot(work_root)
        verify_snapshot(snapshot)
        manifest = _read_manifest(snapshot)
        if before != snapshot.files or directories != manifest['directories'] or evidence != (
                snapshot.manifest_path.parent / 'steam-manifest.acf').read_bytes():
            raise ValueError('Installed source changed from the verified snapshot')
        return snapshot
    if (work_root / 'source').exists():
        raise ValueError(f'Unaccepted source tree: {work_root / "source"}')
    work_root.mkdir(parents=True, exist_ok=True)
    _check_path(work_root, work_root.resolve())
    required = sum(record.bytes for record in before) + len(evidence) + 1024 * 1024
    available = shutil.disk_usage(work_root).free
    print(f'Storage estimate [source archive staging]: additional={required} bytes; '
          f'retained=0 bytes; peak={required} bytes; available={available} bytes', flush=True)
    if available < required:
        raise OSError(f'Insufficient archive space: need {required} bytes, have {available}')
    with tempfile.TemporaryDirectory(prefix='.source-', dir=work_root) as temporary:
        stage = Path(temporary)
        copied = stage / 'source'
        copied.mkdir()
        for name in directories:
            (copied / name).mkdir(parents=True, exist_ok=True)
        for record in before:
            source = steam_root / record.file
            destination = copied / record.file
            try:
                _check_path(source, steam_root.resolve())
                shutil.copy2(source, destination)
            except OSError as error:
                raise OSError(f'Cannot copy source file: {source}: {error}') from error
        if (scan_files(steam_root) != before or _paths(steam_root)[1] != directories
                or evidence_path.read_bytes() != evidence):
            raise ValueError('Installed source changed while copying')
        if scan_files(copied) != before or _paths(copied)[1] != directories:
            raise ValueError('Copied source list, size, or hash changed')
        (stage / 'steam-manifest.acf').write_bytes(evidence)
        manifest = {'schema_version': SCHEMA_VERSION, 'app_id': APP_ID, 'build_id': BUILD_ID,
                    'unity_version': UNITY_VERSION, 'serialized_versions': versions, 'files': [asdict(record) for record in before], 'directories': directories,
                    'steam_manifest': {'path': 'steam-manifest.acf', 'sha256': hashlib.sha256(evidence).hexdigest()}}
        (stage / 'source-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
        staged = load_snapshot(stage)
        verify_snapshot(staged)
        os.rename(copied, work_root / 'source')
        (stage / 'steam-manifest.acf').replace(work_root / 'steam-manifest.acf')
        # Publish acceptance after the source tree and Steam evidence exist.
        (stage / 'source-manifest.json').replace(manifest_path)
    return load_snapshot(work_root)
