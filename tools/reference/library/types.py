"""Current source archive and catalog records."""
from dataclasses import dataclass
from pathlib import Path

SCHEMA_VERSION = 1
APP_ID = 1017180
BUILD_ID = 11140890
UNITY_VERSION = '2021.3.18f1'
DATA_DIRECTORY = 'TheLongDrive_Data'
# This Steam build contains a built-in file authored with an earlier editor.
SERIALIZED_VERSIONS = {'Resources/unity default resources': '2021.3.13f1'}


def expected_serialized_version(container: str) -> str:
    return SERIALIZED_VERSIONS.get(container, UNITY_VERSION)


@dataclass(frozen=True, order=True)
class ObjectId:
    file: str
    path_id: int


@dataclass(frozen=True)
class FileRecord:
    file: str
    bytes: int
    sha256: str


@dataclass(frozen=True)
class Snapshot:
    root: Path
    files: tuple[FileRecord, ...]
    manifest_path: Path
    manifest_sha256: str


@dataclass(frozen=True)
class ObjectRecord:
    source: ObjectId
    type: str
    class_id: int
    offset: int
    bytes: int
    container_sha256: str


@dataclass(frozen=True)
class Catalog:
    objects: dict[ObjectId, ObjectRecord]
    counts: dict[str, int]
    dependencies: dict[ObjectId, tuple[ObjectId, ...]]


@dataclass(frozen=True)
class AssetExport:
    source: ObjectId
    category: str
    status: str
    runtime: str
    reason: str
    files: tuple[FileRecord, ...]
    dependencies: tuple[ObjectId, ...]
    details: dict

    def __post_init__(self):
        if self.status not in ('converted', 'native_record', 'source_empty', 'failed'):
            raise ValueError(f'Invalid export status: {self.status}')
        if self.runtime not in ('verified', 'engine_dependent', 'unverified'):
            raise ValueError(f'Invalid runtime support: {self.runtime}')


@dataclass(frozen=True)
class LibraryReceipt:
    manifest_path: Path
    manifest_sha256: str
    catalog_sha256: str
    source_manifest_sha256: str
    exports: tuple[AssetExport, ...]


@dataclass(frozen=True)
class AnimationTrack:
    binding: dict
    times: tuple[float, ...]
    values: tuple[tuple[float, ...], ...]
    interpolation: str
    in_tangents: tuple[tuple[float, ...], ...] = ()
    out_tangents: tuple[tuple[float, ...], ...] = ()
    # Stored [key][component] triples (a, b, c) evaluate at time from the key.
    # Value = ((a * dt + b) * dt + c) * dt + values[key][component].
    coefficients: tuple[tuple[tuple[float, ...], ...], ...] = ()


@dataclass(frozen=True)
class AnimationRecord:
    source: ObjectId
    tracks: tuple[AnimationTrack, ...]
    events: tuple[dict, ...]
    settings: dict
