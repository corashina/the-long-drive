"""Export and verify all archived managed assemblies without replacing records."""
import argparse
import json
import shutil
import subprocess
import tempfile
from dataclasses import asdict
from pathlib import Path

from .library.archive import load_snapshot, verify_snapshot
from .library.managed import (SCRIPT, _checked_file, _checked_json, _coverage_report, _receipt,
                              _source_assemblies, export_managed, preflight_managed, verify_managed_export)
from .library.records import _file, _json
from .library.types import DATA_DIRECTORY, FileRecord


def write_report(report, output):
    summary = report['summary']
    lines = ['# Managed decompilation report', '',
             'Reference build: Steam 11140890. Scope: shipped managed assemblies.', '',
             f"Assemblies: {summary['assemblies']}. Methods: {summary['methods']}.",
             f"Accounted methods: {summary['accounted_methods']}.", '',
             f"Resolved warnings: {summary['resolved_warnings']}.", '',
             f"Method accounting complete: {report['accounting_complete']}.",
             f"IL decompilation complete: {report['il_complete']}.",
             f"Full source complete: {report['complete']}.", '',
             'Coverage checks compare MethodDef tokens and IL hashes with the metadata index.',
             'These checks do not prove behavior or recover original source files.',
             'Native engine code remains outside this managed export.', '',
             '## Gaps', '', '| Reason | Count |', '| --- | ---: |']
    lines.extend(f'| {reason} | {count} |' for reason, count in summary['gap_reasons'].items())
    lines.extend(['', '## Resolved warnings', '',
                  'Each resolution proves that a local helper always throws PlatformNotSupportedException.',
                  'The exported C# uses null for an unreachable reference return after that helper call.',
                  'Source DLL files remain unchanged. Coverage retains raw caller IL, helper IL, hashes, and original diagnostics.', ''])
    for row in report['resolved_warnings']:
        lines.append(f"- ``{Path(row['assembly']).name} / {row['type']}::{row['name']} / 0x{row['token']:08x}``")
    lines.extend(['', 'Read [coverage.json](managed/coverage.json) for each method, diagnostic, and failure.', '',
                  '## Decompiler diagnostics', ''])
    for gap in report['gaps']:
        if gap['kind'] not in ('module_diagnostic', 'process_diagnostic', 'assembly_failure') and gap.get('status') not in ('warning', 'failed'):
            continue
        name = Path(gap['assembly']).name
        if 'token' in gap:
            name += f" / {gap['type']}::{gap['name']} / 0x{gap['token']:08x}"
        lines.append(f'- ``{name}``')
        lines.extend('  ' + ' '.join(message.split()) for message in gap['diagnostics'])
    lines.extend(['', '## Assemblies', '', '| Assembly | Methods | Decompiled | Warnings | Failed | Declarations |',
                  '| --- | ---: | ---: | ---: | ---: | ---: |'])
    for assembly in report['assemblies']:
        counts = assembly.get('statuses', {})
        values = [counts.get(key, 0) for key in ('decompiled', 'warning', 'failed', 'declaration_only')]
        lines.append(f"| {assembly['name']} | {assembly['method_count']} | " + ' | '.join(map(str, values)) + ' |')
    (output / 'report.md').write_text('\n'.join(lines) + '\n', encoding='utf-8')


def _retained_warning_inputs(snapshot, assemblies, output):
    """Check retained receipt hashes before choosing assemblies for a refresh."""
    assemblies = _source_assemblies(snapshot.root, assemblies)
    report = _checked_json(output, _receipt(output, 'managed/coverage.json'))
    diagnostics = _checked_json(output, _receipt(output, 'managed/diagnostics.json'))
    if (report['source_manifest_sha256'] != snapshot.manifest_sha256
            or diagnostics['source_manifest_sha256'] != snapshot.manifest_sha256
            or diagnostics['source_files'] != [asdict(item) for item in assemblies]
            or report['accounting_complete'] is not True or diagnostics['complete'] is not True):
        raise ValueError('Retained managed source receipts differ')
    if report['index']['file'] != 'managed/index.json':
        raise ValueError('Retained metadata index identity differs')
    index = _checked_json(output, FileRecord(**report['index']))
    expected = {item.file: item for item in assemblies}
    rows = {row['file']: row for row in report['assemblies']}
    indexed = {row['file']: row for row in index['assemblies']}
    if (len(rows) != len(report['assemblies']) or len(indexed) != len(index['assemblies'])
            or expected.keys() != rows.keys() or expected.keys() != indexed.keys()):
        raise ValueError('Retained managed assembly identities differ')
    files = ['managed/index.json', 'managed/coverage.json', 'managed/diagnostics.json']
    selected = []
    for name, source in expected.items():
        row = rows[name]
        if any(row[key] != getattr(source, key) or indexed[name][key] != getattr(source, key)
               for key in ('bytes', 'sha256')):
            raise ValueError(f'Retained assembly source differs: {name}')
        stem = Path(name).stem
        receipt_name = 'decompilation.json' if stem == 'Assembly-CSharp' else f'{stem}.decompilation.json'
        if row['decompilation']['file'] != f'managed/{receipt_name}':
            raise ValueError(f'Retained assembly receipt differs: {name}')
        receipt = _checked_json(output, FileRecord(**row['decompilation']))
        if receipt['input'] != {'file': name, 'sha256': source.sha256}:
            raise ValueError(f'Retained decompiler input differs: {name}')
        files.append(row['decompilation']['file'])
        for key, suffix in (('output', '.cs'), ('coverage', '.coverage.json'), ('supplement', '.methods.cs')):
            record = receipt.get(key)
            if record != row[key] or (record is None and key != 'supplement'):
                raise ValueError(f'Retained decompiler output differs: {name}')
            if record is not None:
                if record['file'] != f'managed/{stem}{suffix}':
                    raise ValueError(f'Retained output identity differs: {name}')
                _checked_file(output, FileRecord(**record))
                files.append(record['file'])
        coverage = _checked_json(output, FileRecord(**receipt['coverage']))
        if coverage['input'] != receipt['input']:
            raise ValueError(f'Retained method input differs: {name}')
        if receipt['diagnostics'] or any(method['status'] == 'warning' for method in coverage['methods']):
            selected.append(name)
    actual = {path.relative_to(output).as_posix() for path in (output / 'managed').rglob('*') if path.is_file()}
    if len(set(files)) != len(files) or actual != set(files):
        raise ValueError('Untracked retained managed output files')
    executions = {item['source']: item for item in diagnostics['executions']}
    if len(executions) != len(diagnostics['executions']) or executions.keys() != expected.keys() | {'index'}:
        raise ValueError('Retained managed execution receipts differ')
    return selected, executions


def refresh_warnings(snapshot, assemblies, output):
    """Refresh only warning assemblies. Publish after the complete staged export verifies."""
    from .library.validation import _publish_directory

    selected, executions = _retained_warning_inputs(snapshot, assemblies, output)
    if not selected:
        return verify_managed_export(snapshot.root, assemblies, output, snapshot.manifest_sha256)[0]
    source, target = snapshot.root.resolve(), output.resolve()
    if source.is_relative_to(target) or target.is_relative_to(source):
        raise ValueError('Managed refresh output overlaps the source archive')
    preflight_managed()
    stage = Path(tempfile.mkdtemp(prefix='managed-warning-refresh-', dir=output.parent)).resolve()
    if not stage.is_relative_to(output.parent.resolve()) or stage == output.resolve():
        raise ValueError('Managed refresh stage escapes its parent')
    print(f'Managed warning refresh stage: {stage}', flush=True)
    execution_rows = list(executions.values())
    try:
        shutil.copytree(output, stage, dirs_exist_ok=True)
        commands = [(['-Mode', 'Index', '-AssemblyPath', str(snapshot.root / DATA_DIRECTORY / 'Managed')], 'index')]
        for name in selected:
            stem = Path(name).stem
            receipt_name = 'decompilation.json' if stem == 'Assembly-CSharp' else f'{stem}.decompilation.json'
            for filename in (receipt_name, f'{stem}.cs', f'{stem}.coverage.json', f'{stem}.methods.cs'):
                path = (stage / 'managed' / filename).resolve()
                if not path.is_relative_to(stage / 'managed'):
                    raise ValueError('Managed refresh deletion escapes its stage')
                path.unlink(missing_ok=True)
            commands.append((['-AssemblyPath', str(snapshot.root / name)], name))
        for arguments, name in commands:
            print(f'Managed warning refresh: {name}', flush=True)
            result = subprocess.run(['pwsh', '-NoProfile', '-File', str(SCRIPT), *arguments,
                                     '-OutputPath', str(stage / 'managed')], capture_output=True, text=True)
            executions[name] = {'source': name, 'exit_code': result.returncode,
                                'stdout': result.stdout, 'stderr': result.stderr}
        execution_rows = [executions[name] for name in ('index', *sorted(item.file for item in assemblies))]
        _file(stage, 'managed/diagnostics.json', _json({'schema_version': 1, 'complete': False,
              'source_manifest_sha256': snapshot.manifest_sha256,
              'source_files': [asdict(item) for item in sorted(assemblies, key=lambda item: item.file)],
              'executions': execution_rows}))
        report, _ = _coverage_report(snapshot.root, assemblies, stage, snapshot.manifest_sha256, False, execution_rows)
        _file(stage, 'managed/coverage.json', _json(report))
        _file(stage, 'managed/diagnostics.json', _json({'schema_version': 1,
              'complete': report['accounting_complete'], 'source_manifest_sha256': snapshot.manifest_sha256,
              'source_files': [asdict(item) for item in sorted(assemblies, key=lambda item: item.file)],
              'executions': execution_rows}))
        write_report(report, stage)
        report, files = verify_managed_export(snapshot.root, assemblies, stage, snapshot.manifest_sha256)
        actual = {path.relative_to(stage).as_posix() for path in (stage / 'managed').rglob('*') if path.is_file()}
        if actual != {record.file for record in files} or not report['il_complete']:
            raise ValueError('Managed warning refresh remains incomplete')
        _publish_directory(stage, output)
        return report
    except BaseException as error:
        raise RuntimeError(f'Managed warning refresh failed. Retained stage: {stage}') from error


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path,
                        default=Path('.cache/reference-library/source/TheLongDrive_Data/Managed'))
    parser.add_argument('--output', type=Path, default=Path('.cache/reference-library/decompiled/managed'))
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument('--verify', action='store_true', help='Verify retained outputs without decompilation')
    mode.add_argument('--refresh-warnings', action='store_true', help='Stage and refresh warning assemblies only')
    arguments = parser.parse_args()
    source, managed = arguments.source.resolve(), arguments.output.resolve()
    if source.name != 'Managed' or source.parent.name != DATA_DIRECTORY or managed.name != 'managed':
        parser.error('Use the archived TheLongDrive_Data/Managed source and an output named managed')
    snapshot = load_snapshot(source.parents[2])
    if snapshot.root.resolve() != source.parents[1]:
        parser.error('Source does not match the archived snapshot')
    verify_snapshot(snapshot)
    assemblies = tuple(item for item in snapshot.files
                       if item.file.startswith(f'{DATA_DIRECTORY}/Managed/') and item.file.lower().endswith('.dll'))
    if arguments.refresh_warnings:
        report = refresh_warnings(snapshot, assemblies, managed.parent)
        print(json.dumps({key: report[key] for key in ('accounting_complete', 'il_complete', 'complete', 'summary')},
                         sort_keys=True, indent=2))
        return
    if not arguments.verify:
        preflight_managed()
        try:
            export_managed(snapshot.root, assemblies, managed.parent, snapshot.manifest_sha256)
        except (OSError, ValueError):
            if (managed / 'diagnostics.json').is_file():
                (managed.parent / 'report.md').write_text(
                    '# Managed decompilation report\n\n'
                    'Export failed before method coverage could be verified. Completion is unproven.\n\n'
                    'Read [diagnostics.json](managed/diagnostics.json) for assembly attempts and errors.\n',
                    encoding='utf-8')
            raise
        # Keep the gap report even when strict verification rejects a failed assembly.
        write_report(json.loads((managed / 'coverage.json').read_bytes()), managed.parent)
    report, files = verify_managed_export(snapshot.root, assemblies, managed.parent, snapshot.manifest_sha256)
    actual = {path.relative_to(managed.parent).as_posix() for path in managed.rglob('*') if path.is_file()}
    if actual != {record.file for record in files}:
        raise ValueError('Untracked managed output files')
    print(json.dumps({key: report[key] for key in ('accounting_complete', 'il_complete', 'complete', 'summary')},
                     sort_keys=True, indent=2))


if __name__ == '__main__':
    main()
