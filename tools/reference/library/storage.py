"""Storage bounds for the supported build and accepted category receipts."""
import json
import os
import shutil
from pathlib import Path

# Build 11140890 measured 0.53 GB source, 0.70 GB records, and 3.11 GB library.
# Rounded bounds include decoded media, native producer inputs, and metadata.
SOURCE_BYTES = 640 * 1024**2
CATALOG_BYTES = 1024**3
CATEGORY_BYTES = 5 * 1024**3
ASSEMBLY_BYTES = 4 * 1024**3
RUNTIME_BYTES = 128 * 1024**2


def tree_bytes(root: Path) -> int:
    return sum((Path(directory) / name).stat().st_size
               for directory, _, files in os.walk(root) for name in files)


def with_headroom(size: int) -> int:
    return (size * 5 + 3) // 4 + 64 * 1024**2


def check_space(scope: str, path: Path, additional: int, retained: int = 0) -> None:
    existing = path.resolve()
    while not existing.exists():
        existing = existing.parent
    available = shutil.disk_usage(existing).free
    print(f'Storage estimate [{scope}] on {existing}: additional={additional} bytes; '
          f'retained={retained} bytes; peak={retained + additional} bytes; available={available} bytes', flush=True)
    if available < additional:
        raise OSError(f'Insufficient {scope} space: need {additional} additional bytes, have {available}')


def library_space(root: Path, library: Path) -> None:
    # Existing allocations consume disk already. Only missing outputs need free bytes.
    additional = ASSEMBLY_BYTES
    if not (root / 'category-stages.json').exists():
        additional += CATEGORY_BYTES
    if not (root / 'records/manifest.json').exists():
        additional += CATALOG_BYTES
    if not (root / 'source-manifest.json').exists():
        additional += SOURCE_BYTES
    check_space('build 11140890: missing source/records, decoded categories, assembly staging',
                root, additional, retained=tree_bytes(root) + tree_bytes(library))


def assembly_bytes(stages) -> int:
    """Count the complete assembly inputs once, including standalone sidecars."""
    from .validation import _add
    from .types import FileRecord
    files = {}
    for category, root, exports in stages:
        def measured(name):
            _add(files, FileRecord(name, (root / name).stat().st_size, 'measured'))
        name = f'{category}/index.json'
        measured(name)
        for entry in exports:
            for record in entry.files:
                _add(files, record)
        if category in ('animations', 'native'):
            envelope = json.loads((root / name).read_bytes())
            key = 'ownership' if category == 'animations' else 'managed_links'
            _add(files, FileRecord(**envelope[key]))
        if category == 'ui':
            name = 'ui/languages.json'
            measured(name)
            for entry in json.loads((root / name).read_bytes()):
                _add(files, FileRecord(entry['file'], entry['bytes'], entry['sha256']))
        if category == 'native':
            name = 'native/source-text.json'
            measured(name)
            for entry in json.loads((root / name).read_bytes())['files']:
                _add(files, FileRecord(**entry['output']))
    return sum(record.bytes for record in files.values())
