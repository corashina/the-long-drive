"""Decode one source clip at a time and validate its WAV samples."""
import hashlib
import math
import struct
from dataclasses import asdict
from pathlib import Path

from .images import typed_record
from .records import _file, _json, _stream
from .types import AssetExport, Catalog, DATA_DIRECTORY
from ..source import Source


def validate_wav(path: Path, clip_record: dict) -> dict:
    """Check RIFF boundaries and PCM or IEEE float sample metadata."""
    size = path.stat().st_size
    fmt = data_size = None
    with path.open('rb') as stream:
        header = stream.read(12)
        if len(header) != 12 or header[:4] != b'RIFF' or header[8:] != b'WAVE':
            raise ValueError(f'Invalid RIFF/WAVE header: {path}')
        if struct.unpack_from('<I', header, 4)[0] + 8 != size:
            raise ValueError(f'RIFF size mismatch or truncated WAV: {path}')
        while stream.tell() < size:
            chunk = stream.read(8)
            if len(chunk) != 8:
                raise ValueError(f'Truncated WAV chunk: {path}')
            name, length = struct.unpack('<4sI', chunk)
            end = stream.tell() + length + (length & 1)
            if end > size:
                raise ValueError(f'Truncated WAV payload: {path}')
            if name == b'fmt ':
                if fmt is not None or length < 16:
                    raise ValueError(f'Invalid WAV format chunk: {path}')
                fmt = struct.unpack('<HHIIHH', stream.read(16))
            elif name == b'data':
                if data_size is not None:
                    raise ValueError(f'Duplicate WAV data chunk: {path}')
                data_size = length
            stream.seek(end)
    if fmt is None or not data_size:
        raise ValueError(f'WAV has no format or samples: {path}')
    format, channels, frequency, byte_rate, block, bits = fmt
    if format not in (1, 3) or bits not in ((8, 16, 24, 32) if format == 1 else (32, 64)):
        raise ValueError(f'Unsupported WAV sample format: {path}')
    if channels <= 0 or frequency <= 0 or block != channels * bits // 8 or byte_rate != frequency * block:
        raise ValueError(f'Invalid WAV sample alignment: {path}')
    if data_size % block:
        raise ValueError(f'WAV contains a partial sample frame: {path}')
    if channels != clip_record['m_Channels'] or frequency != clip_record['m_Frequency']:
        raise ValueError(f'WAV channels or frequency mismatch: {path}')
    if bits != clip_record['m_BitsPerSample']:
        raise ValueError(f'WAV sample width mismatch: {path}')
    frames = data_size // block
    length = clip_record['m_Length']
    if not math.isfinite(length) or length <= 0:
        raise ValueError(f'Invalid source audio duration: {path}')
    expected_frames = length * frequency
    delta = frames - expected_frames
    # Unity stores duration in a binary32 value. Allow its rounding error and one frame.
    tolerance = max(1.0, abs(expected_frames) * 2 ** -23)
    if abs(delta) > tolerance:
        raise ValueError(f'WAV duration mismatch: {path}: {delta} frames')
    return {'format': 'pcm' if format == 1 else 'ieee_float', 'format_tag': format,
            'channels': channels, 'frequency': frequency, 'bits_per_sample': bits,
            'block_align': block, 'data_bytes': data_size, 'frames': frames,
            'duration_seconds': frames / frequency, 'source_duration_seconds': length,
            'duration_delta_seconds': delta / frequency, 'duration_delta_frames': delta,
            'duration_tolerance_frames': tolerance, 'duration_status': 'source_rounding'}


def _source_audio(source, data):
    resource = data.get('m_Resource')
    byte_range = _stream(source, 'StreamedResource', resource) if resource else None
    if byte_range:
        if not byte_range['bytes']:
            raise ValueError('AudioClip has no source samples')
        path = source.root / DATA_DIRECTORY / byte_range['file']
        digest = hashlib.sha256()
        with path.open('rb') as stream:
            stream.seek(byte_range['offset'])
            signature = stream.read(min(12, byte_range['bytes']))
            stream.seek(byte_range['offset'])
            remaining = byte_range['bytes']
            while remaining:
                chunk = stream.read(min(1024 * 1024, remaining))
                if not chunk:
                    raise ValueError(f'Truncated source audio stream: {path}')
                digest.update(chunk)
                remaining -= len(chunk)
        result = {**byte_range, 'sha256': digest.hexdigest(), 'storage': 'source_archive'}
    else:
        payload = bytes(data.get('m_AudioData', b''))
        if not payload:
            raise ValueError('AudioClip has no source samples')
        signature = payload[:12]
        result = {'bytes': len(payload), 'sha256': hashlib.sha256(payload).hexdigest(),
                  'storage': 'typed_record_binary'}
    result['format'] = {b'FSB5': 'FSB5', b'FSB4': 'FSB4', b'OggS': 'Ogg', b'RIFF': 'RIFF'}.get(
        signature[:4], 'unknown')
    result['signature'] = signature.hex()
    return result


def export_audio(source: Source, catalog: Catalog, output: Path) -> tuple[AssetExport, ...]:
    exports, paths = [], set()
    clips = [(identity, record) for identity, record in sorted(catalog.objects.items()) if record.type == 'AudioClip']
    for identity, _ in clips:
        prefix = f'audio/{identity.file}/{identity.path_id}'
        if prefix.casefold() in paths:
            raise ValueError(f'Audio output identity collision: {identity}')
        paths.add(prefix.casefold())
    for index, (identity, record) in enumerate(clips, 1):
        reader = source.get(identity.file, identity.path_id)
        data = source.read_record(reader)
        details = {'name': data['m_Name'], 'type': record.type, 'typed_record': typed_record(identity),
                   'source_audio': _source_audio(source, data), 'samples': [],
                   'conversion': {'decoder': 'UnityPy.AudioClip.samples', 'format': 'WAV'}}
        # Source.object caches parsed clips. Parse directly to release this clip after writing.
        obj = reader.parse_as_object(check_read=True)
        samples = obj.samples
        if not samples:
            raise ValueError(f'No decoded samples: {identity}')
        files = []
        for sample_index, (label, payload) in enumerate(sorted(samples.items())):
            name = f'audio/{identity.file}/{identity.path_id}/sample-{sample_index:03d}.wav'
            file = _file(output, name, payload)
            details['samples'].append({'label': label, 'file': file.file, **validate_wav(output / file.file, data)})
            files.append(file)
        exports.append(AssetExport(identity, 'audio', 'converted', 'unverified', '', tuple(files),
                                   catalog.dependencies[identity], details))
        del samples, payload, obj, data
        if index % 25 == 0:
            print(f'Exported audio: {index}/{len(clips)}', flush=True)
    _file(output, 'audio/index.json', _json([asdict(item) for item in exports]))
    return tuple(exports)
