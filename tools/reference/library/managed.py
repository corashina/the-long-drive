"""Index archived assemblies and retain separate dynamic-analysis evidence."""
import hashlib
import json
import subprocess
from collections import Counter, defaultdict
from dataclasses import asdict
from pathlib import Path

from .archive import _check_path, sha256, validate_paths
from .records import _file, _json
from .types import Catalog, DATA_DIRECTORY, FileRecord, ObjectId, SCHEMA_VERSION, Snapshot

SCRIPT = Path(__file__).resolve().parents[1] / 'decompile.ps1'


def preflight_managed() -> None:
    script = SCRIPT.with_name('preflight-managed.ps1')
    try:
        result = subprocess.run(['pwsh', '-NoProfile', '-File', str(script)], capture_output=True, text=True)
    except OSError as error:
        raise ValueError('Managed tools need host pwsh; see tools/reference/README.md') from error
    if result.returncode:
        raise ValueError(f'Managed tool preflight failed: {result.stderr.strip()}; see tools/reference/README.md')


def index_managed(snapshot: Snapshot, output: Path) -> tuple[FileRecord, ...]:
    assemblies = tuple(item for item in snapshot.files if item.file.startswith(f'{DATA_DIRECTORY}/Managed/')
                       and item.file.lower().endswith('.dll'))
    files = export_managed(snapshot.root, assemblies, output, snapshot.manifest_sha256)
    report = json.loads((output / 'managed/coverage.json').read_bytes())
    if not report['accounting_complete']:
        raise ValueError('Managed method accounting incomplete. See managed/coverage.json')
    return files


def _source_assemblies(source_root, assemblies):
    assemblies = tuple(sorted(assemblies, key=lambda item: item.file))
    if not assemblies:
        raise ValueError('No archived managed assemblies')
    validate_paths([item.file for item in assemblies])
    for item in assemblies:
        path = source_root / item.file
        _check_path(path, source_root.resolve())
        if path.stat().st_size != item.bytes or sha256(path) != item.sha256:
            raise ValueError(f'Managed source hash mismatch: {item.file}')
    return assemblies


def _receipt(output, name):
    path = output / name
    validate_paths([name])
    _check_path(path, output.resolve())
    return FileRecord(name, path.stat().st_size, sha256(path))


def _method_reason(method):
    if method['flags'] & 0x400:
        return 'abstract'
    if method['flags'] & 0x2000:
        return 'pinvoke'
    if method['implementation_flags'] & 0x1000:
        return 'internal_call'
    if method['implementation_flags'] & 3 == 3:
        return 'runtime'
    return 'missing_il_body'


def _verified_resolution(row, method, methods, proof):
    keys = {'kind', 'method_token', 'caller_il_hex', 'caller_il_sha256', 'callee_token',
            'callee_signature', 'callee_il_hex', 'callee_il_sha256', 'exception_constructor_token',
            'exception_constructor_type', 'exception_constructor_signature'}
    if (row['resolution'] != proof or set(proof) != keys or proof['kind'] != 'verified_nonreturning_stub'
            or proof['method_token'] != method['token'] or row['status'] != 'decompiled'
            or row['diagnostics'] or row['reason'] is not None
            or row['resolved_diagnostics'] != ['IL_0007: Expected Obj, but got I4']):
        raise ValueError('Invalid resolved managed warning')
    caller, callee = bytes.fromhex(proof['caller_il_hex']), bytes.fromhex(proof['callee_il_hex'])
    target = methods.get(proof['callee_token'])
    if (len(caller) != 7 or caller[0] != 0x28 or caller[5:] != b'\x16\x2a'
            or int.from_bytes(caller[1:5], 'little') != proof['callee_token']
            or len(callee) != 6 or callee[0] != 0x73 or callee[-1] != 0x7a
            or int.from_bytes(callee[1:5], 'little') != proof['exception_constructor_token']
            or hashlib.sha256(caller).hexdigest() != proof['caller_il_sha256']
            or proof['caller_il_sha256'] != method['il_sha256']
            or hashlib.sha256(callee).hexdigest() != proof['callee_il_sha256']
            or target is None or target['type'] != 'Unity.ThrowStub'
            or target['name'] != 'ThrowNotSupportedException' or not target['rva']
            or target['signature'] != proof['callee_signature'] or proof['callee_signature'] != '000001'
            or target['il_sha256'] != proof['callee_il_sha256']
            or proof['exception_constructor_type'] != 'System.PlatformNotSupportedException'
            or proof['exception_constructor_signature'] != '200001'):
        raise ValueError('Managed nonreturning-stub proof mismatch')


def _assembly_coverage(output, assembly, methods, proofs):
    stem = Path(assembly['file']).stem
    receipt_name = 'decompilation.json' if stem == 'Assembly-CSharp' else f'{stem}.decompilation.json'
    receipt_file = _receipt(output, f'managed/{receipt_name}')
    receipt = _checked_json(output, receipt_file)
    expected_input = {'file': assembly['file'], 'sha256': assembly['sha256']}
    if receipt['input'] != expected_input or receipt['mode'] != 'whole_module':
        raise ValueError(f'Managed decompilation input mismatch: {stem}')
    if (receipt['method_count'] != assembly['method_count']
            or receipt['type_count'] != assembly['type_count']):
        raise ValueError(f'Managed decompilation count mismatch: {stem}')
    records = [receipt_file]
    expected_outputs = {'output': f'managed/{stem}.cs', 'coverage': f'managed/{stem}.coverage.json',
                        'supplement': f'managed/{stem}.methods.cs'}
    for key, name in expected_outputs.items():
        item = receipt.get(key)
        if item is None and key == 'supplement':
            continue
        record = FileRecord(**item)
        if record.file != name:
            raise ValueError(f'Managed output identity mismatch: {stem}.{key}')
        _checked_file(output, record)
        records.append(record)
    coverage = _checked_json(output, FileRecord(**receipt['coverage']))
    if coverage['input'] != expected_input:
        raise ValueError(f'Managed coverage input mismatch: {stem}')
    expected = {method['token']: method for method in methods}
    observed = {method['token']: method for method in coverage['methods']}
    if (len(expected) != len(methods) or len(observed) != len(coverage['methods'])
            or expected.keys() != observed.keys()):
        raise ValueError(f'Managed method token coverage mismatch: {stem}')
    fields = ('token', 'type', 'rva', 'flags', 'implementation_flags', 'signature', 'il_sha256')
    gaps, statuses, resolved = [], Counter(), []
    code_files = {item.file for item in records if item.file.endswith('.cs')}
    for token, method in expected.items():
        row = observed[token]
        if any(row[field] != method[field] for field in fields) or row['name'] != method['name']:
            raise ValueError(f'Managed method identity mismatch: {stem}:0x{token:08x}')
        status = row['status']
        if status not in ('decompiled', 'warning', 'failed', 'declaration_only'):
            raise ValueError(f'Unknown managed method status: {stem}:0x{token:08x}')
        if not isinstance(row['diagnostics'], list) or any(not isinstance(value, str) for value in row['diagnostics']):
            raise ValueError(f'Invalid managed method diagnostics: {stem}:0x{token:08x}')
        if method['rva']:
            if (status == 'declaration_only' or (status in ('decompiled', 'warning') and row['source'] not in code_files)
                    or (status == 'decompiled' and row['diagnostics'])):
                raise ValueError(f'Invalid managed body coverage: {stem}:0x{token:08x}')
        elif status != 'declaration_only' or row['reason'] != _method_reason(method):
            raise ValueError(f'Invalid managed declaration coverage: {stem}:0x{token:08x}')
        if row['source'] is not None and row['source'] not in code_files:
            raise ValueError(f'Unknown managed method source: {stem}:0x{token:08x}')
        if token in proofs and status == 'decompiled' and (
                'resolution' not in row or 'resolved_diagnostics' not in row):
            raise ValueError(f'Missing resolved managed warning proof: {stem}:0x{token:08x}')
        if 'resolution' in row or 'resolved_diagnostics' in row:
            if ('resolution' not in row or 'resolved_diagnostics' not in row or token not in proofs
                    or row['source'] != f'managed/{stem}.cs'):
                raise ValueError(f'Orphan resolved managed warning: {stem}:0x{token:08x}')
            _verified_resolution(row, method, expected, proofs[token])
            resolved.append({'assembly': assembly['file'], **row})
        statuses[status] += 1
        if status in ('warning', 'failed') or (not method['rva'] and row['reason'] != 'abstract'):
            gaps.append({'kind': 'method', 'assembly': assembly['file'], **row})
    gaps.extend({'kind': 'module_diagnostic', 'assembly': assembly['file'], 'diagnostics': [message]}
                for message in receipt['diagnostics'])
    il_complete = not receipt['diagnostics'] and all(
        observed[token]['status'] == 'decompiled' for token, method in expected.items() if method['rva'])
    entry = {key: assembly[key] for key in ('file', 'name', 'version', 'mvid', 'bytes', 'sha256', 'type_count', 'method_count')}
    entry.update({'decompilation': asdict(receipt_file), 'output': receipt['output'],
                  'coverage': receipt['coverage'], 'supplement': receipt.get('supplement'),
                  'accounting_complete': True, 'il_complete': il_complete,
                  'complete': receipt['complete'] is True and not gaps,
                  'statuses': dict(sorted(statuses.items())), 'diagnostics': receipt['diagnostics'],
                  'resolved_warnings': len(resolved)})
    return entry, records, gaps, resolved


def _coverage_report(source_root, assemblies, output, source_manifest_sha256, strict, executions=()):
    assemblies = _source_assemblies(source_root, assemblies)
    index_file = _receipt(output, 'managed/index.json')
    index = _checked_json(output, index_file)
    expected = {item.file: asdict(item) for item in assemblies}
    indexed = {item['file']: item for item in index['assemblies']}
    if (len(indexed) != len(index['assemblies']) or expected.keys() != indexed.keys()
            or any(any(indexed[name][key] != item[key] for key in ('bytes', 'sha256'))
                   for name, item in expected.items())):
        raise ValueError('Managed index source mismatch')
    methods = defaultdict(list)
    for method in index['methods']:
        if method['assembly'] not in indexed:
            raise ValueError('Managed index has an unknown method assembly')
        methods[method['assembly']].append(method)
    proofs = defaultdict(dict)
    method_tokens = {name: {method['token'] for method in values} for name, values in methods.items()}
    for item in index['verified_throw_stubs']:
        if set(item) != {'assembly', 'proof'} or item['assembly'] not in indexed:
            raise ValueError('Invalid indexed nonreturning-stub proof')
        token = item['proof']['method_token']
        if token in proofs[item['assembly']]:
            raise ValueError('Duplicate indexed nonreturning-stub proof')
        if token not in method_tokens.get(item['assembly'], set()):
            raise ValueError('Unknown indexed nonreturning-stub method')
        proofs[item['assembly']][token] = item['proof']
    rows, records, gaps, failures, statuses, resolved = [], [index_file], [], [], Counter(), []
    for name, assembly in indexed.items():
        try:
            if len(methods[name]) != assembly['method_count']:
                raise ValueError(f'Managed indexed method count mismatch: {name}')
            entry, outputs, assembly_gaps, assembly_resolved = _assembly_coverage(output, assembly, methods[name], proofs[name])
            records.extend(outputs)
            gaps.extend(assembly_gaps)
            statuses.update(entry['statuses'])
            resolved.extend(assembly_resolved)
        except (OSError, ValueError, KeyError, TypeError) as error:
            if strict:
                raise ValueError(f'Managed coverage invalid: {name}: {error}') from error
            failures.append({'assembly': name, 'error': str(error)})
            gaps.append({'kind': 'assembly_failure', 'assembly': name, 'diagnostics': [str(error)]})
            entry = {key: assembly[key] for key in ('file', 'name', 'version', 'mvid', 'bytes', 'sha256', 'type_count', 'method_count')}
            stem = Path(name).stem
            receipt_name = 'decompilation.json' if stem == 'Assembly-CSharp' else f'{stem}.decompilation.json'
            retained = [_receipt(output, f'managed/{filename}') for filename in (
                receipt_name, f'{stem}.cs', f'{stem}.coverage.json', f'{stem}.methods.cs')
                if (output / 'managed' / filename).is_file()]
            records.extend(retained)
            entry.update({'accounting_complete': False, 'il_complete': False, 'complete': False,
                          'diagnostics': [str(error)], 'retained_files': [asdict(item) for item in retained]})
        rows.append(entry)
    execution_failures = [execution for execution in executions if execution['exit_code'] != 0]
    gaps.extend({'kind': 'process_diagnostic', 'assembly': execution['source'],
                 'exit_code': execution['exit_code'], 'diagnostics': [execution['stderr']]}
                for execution in execution_failures)
    report = {'schema_version': SCHEMA_VERSION, 'source_manifest_sha256': source_manifest_sha256,
              'index': asdict(index_file), 'assemblies': rows, 'gaps': gaps, 'failures': failures,
              'resolved_warnings': resolved,
              'accounting_complete': index['complete'] is True and not failures,
              'il_complete': not execution_failures and all(row['il_complete'] for row in rows),
              'complete': index['complete'] is True and not execution_failures and all(row['complete'] for row in rows),
              'summary': {'assemblies': len(rows), 'types': sum(row['type_count'] for row in rows),
                          'methods': sum(row['method_count'] for row in rows),
                          'accounted_methods': sum(statuses.values()), 'statuses': dict(sorted(statuses.items())),
                          'gaps': len(gaps), 'failed_assemblies': len(failures),
                          'resolved_warnings': len(resolved),
                          'failed_executions': len(execution_failures),
                          'gap_reasons': dict(sorted(Counter(gap.get('reason') or gap['kind'] for gap in gaps).items()))},
              'index_diagnostics': index['diagnostics']}
    validate_paths([record.file for record in records])
    return report, records


def export_managed(source_root, assemblies, output, source_manifest_sha256=None):
    """Export every assembly before reporting failures. Preserve failed evidence."""
    assemblies = _source_assemblies(source_root, assemblies)
    managed = output / 'managed'
    source, target = source_root.resolve(), managed.resolve()
    if source.is_relative_to(target) or target.is_relative_to(source):
        raise ValueError('Managed output overlaps the source archive')
    managed.mkdir(parents=True, exist_ok=True)
    if any(managed.iterdir()):
        raise ValueError(f'Managed output must be empty: {managed}')
    commands = [(['-Mode', 'Index', '-AssemblyPath', str(source_root / DATA_DIRECTORY / 'Managed')], 'index')]
    commands.extend((['-AssemblyPath', str(source_root / item.file)], item.file) for item in assemblies)
    executions = []
    for arguments, name in commands:
        print(f'Managed export: {name}', flush=True)
        result = subprocess.run(['pwsh', '-NoProfile', '-File', str(SCRIPT), *arguments,
                                 '-OutputPath', str(managed)], capture_output=True, text=True)
        executions.append({'source': name, 'exit_code': result.returncode,
                           'stdout': result.stdout, 'stderr': result.stderr})
    try:
        report, records = _coverage_report(source_root, assemblies, output, source_manifest_sha256, False, executions)
    except (OSError, ValueError, KeyError, TypeError) as error:
        _file(output, 'managed/diagnostics.json', _json({'schema_version': SCHEMA_VERSION,
              'complete': False, 'source_manifest_sha256': source_manifest_sha256,
              'source_files': [asdict(item) for item in assemblies], 'executions': executions,
              'error': str(error)}))
        raise
    records.append(_file(output, 'managed/coverage.json', _json(report)))
    records.append(_file(output, 'managed/diagnostics.json', _json({'schema_version': SCHEMA_VERSION,
          'complete': report['accounting_complete'], 'source_manifest_sha256': source_manifest_sha256,
          'source_files': [asdict(item) for item in assemblies], 'executions': executions})))
    return tuple(sorted(records, key=lambda item: item.file))


def verify_managed_export(source_root, assemblies, output, source_manifest_sha256=None):
    """Hash retained evidence and recompute method coverage without decompilation."""
    coverage_file = _receipt(output, 'managed/coverage.json')
    diagnostics_file = _receipt(output, 'managed/diagnostics.json')
    diagnostics = _checked_json(output, diagnostics_file)
    if (diagnostics['complete'] is not True or diagnostics['source_manifest_sha256'] != source_manifest_sha256
            or diagnostics['source_files'] != [asdict(item) for item in sorted(assemblies, key=lambda item: item.file)]):
        raise ValueError('Managed diagnostics source mismatch')
    saved = _checked_json(output, coverage_file)
    report, records = _coverage_report(source_root, assemblies, output, source_manifest_sha256, True, diagnostics['executions'])
    if saved != report or not report['accounting_complete']:
        raise ValueError('Managed coverage differs from verified methods')
    records.extend((coverage_file, diagnostics_file))
    validate_paths([record.file for record in records])
    return report, tuple(sorted(records, key=lambda item: item.file))


def _checked_file(output, record):
    validate_paths([record.file])
    path = output / record.file
    _check_path(path, output.resolve())
    raw = path.read_bytes()
    if len(raw) != record.bytes or hashlib.sha256(raw).hexdigest() != record.sha256:
        raise ValueError(f'Managed output hash mismatch: {record.file}')
    return raw


def _checked_json(output, record):
    result = json.loads(_checked_file(output, record))
    if result['schema_version'] != SCHEMA_VERSION:
        raise ValueError('Obsolete managed schema')
    return result


def write_analysis_report(catalog: Catalog, managed_files: tuple[FileRecord, ...], output: Path) -> FileRecord:
    """Keep proven serialized links and unproven code links in separate lists."""
    records = {record.file: record for record in managed_files}
    index = _checked_json(output, records['managed/index.json'])
    if not index['complete']:
        raise ValueError('Managed analysis incomplete')
    type_index = {(item['assembly'], item['full_name']): item for item in index['types']}
    assemblies = {item['name']: item for item in index['assemblies']}
    display_names, short_names, uses = defaultdict(list), defaultdict(list), Counter()
    for item in index['types']:
        display_names[item['assembly'], item['full_name'].replace('+', '.')].append(item)
        short_names[item['assembly'], item['full_name'].split('+')[-1].rsplit('.', 1)[-1].casefold()].append(item)
    for identity, entry in catalog.objects.items():
        if entry.type == 'MonoBehaviour':
            for target in catalog.dependencies[identity]:
                if catalog.objects[target].type == 'MonoScript':
                    uses[target] += 1
    scripts, resources, limits = [], [], []
    # Read only these small native records. Complete component payloads remain shared.
    for identity, entry in sorted(catalog.objects.items()):
        if entry.type not in ('MonoScript', 'ResourceManager'):
            continue
        path = output / f'{identity.file}/{identity.path_id}.json'
        payload = json.loads(path.read_bytes())['data']
        if entry.type == 'MonoScript':
            assembly_name = payload['m_AssemblyName']
            assembly = assemblies.get(assembly_name.removesuffix('.dll'))
            full_name = '.'.join(part for part in (payload['m_Namespace'], payload['m_ClassName']) if part)
            owner = type_index.get((assembly['file'], full_name)) if assembly else None
            item = {'source': asdict(identity), 'assembly_name': assembly_name, 'type': full_name,
                    'assembly': assembly['file'] if assembly else None,
                    'assembly_sha256': assembly['sha256'] if assembly else None,
                    'type_token': owner['token'] if owner else None,
                    'status': 'resolved' if owner else 'unresolved',
                    'stored_component_count': uses[identity], 'candidates': [],
                    'reason': '' if owner else 'declared_assembly_not_shipped' if assembly is None
                              else 'declared_type_not_in_archived_assembly'}
            if owner is None and assembly:
                candidates = list(display_names[assembly['file'], full_name])
                if not payload['m_Namespace'] and '.' not in payload['m_ClassName']:
                    candidates.extend(short_names[assembly['file'], payload['m_ClassName'].casefold()])
                candidates = {candidate['full_name']: candidate for candidate in candidates}
                item['candidates'] = [{'full_name': candidate['full_name'], 'token': candidate['token'],
                                       'status': 'unproven', 'reason': 'Namespace, nested name, or letter case differs.'}
                                      for _, candidate in sorted(candidates.items())]
            scripts.append(item)
            if owner is None:
                limits.append({'kind': 'MonoScript_type', **item})
        else:
            resources.append({'source': asdict(identity), 'typed_record': f'records/{identity.file}/{identity.path_id}.json',
                              'dependencies': [asdict(value) for value in catalog.dependencies[identity]]})
    links = json.loads((output / 'native/managed-links.json').read_bytes())
    if links['schema_version'] != SCHEMA_VERSION:
        raise ValueError('Obsolete native analysis schema')
    calls = index['asset_calls']
    proven, creation_sites, resolved_calls = [], [], set()
    assembly_path = f'{DATA_DIRECTORY}/Managed/Assembly-CSharp.dll'
    assembly = next((item for item in index['assemblies'] if item['file'] == assembly_path), None)
    pinned = '9919a9eeddcce23bb4ff86e890437f32b4a3ef3c10af2efe3b8d889d894dd62e'
    code = ''
    code_receipt = records.get('managed/Assembly-CSharp.cs')
    if code_receipt:
        code = _checked_file(output, code_receipt).decode('utf-8')
    if assembly and assembly['sha256'] != pinned:
        raise ValueError('Dynamic analysis evidence does not match assembly')
    def call_key(call):
        return (call['assembly'], call['method_token'], call['il_offset'])
    for call in calls:
        target = call['target']
        if call['assembly'] != assembly_path:
            continue
        if target['method'] in ('.ctor', 'Create', 'GetTemporary'):
            creation_sites.append({'call': call, 'assembly_sha256': pinned,
                                   'reason': 'IL proves an asset creation call; the runtime result has no serialized ObjectId.'})
        if target['type'] == 'UnityEngine.Resources' and target['method'] == 'Load':
            literals = call['preceding_string_literals']
            literal = literals[-1] if literals else None
            if literal and literal['il_offset'] + 5 == call['il_offset']:
                matches = [row for row in links['resources'] if row['target'] and row['name'].casefold() == literal['value'].casefold()]
                for row in matches:
                    if ObjectId(**row['target']) not in catalog.objects:
                        raise ValueError('Resources call has an unknown target')
                    proven.append({'kind': 'Resources.Load', 'call': call, 'resource': row,
                                   'assembly_sha256': pinned, 'argument_proof': 'The immediately preceding ldstr supplies the name.'})
                    resolved_calls.add(call_key(call))
    creation_rules = {
        ('computerscript', 'defRenderTexture'): ('computerscript', 'Start', 'UnityEngine.RenderTexture', 'output = new RenderTexture(defRenderTexture);'),
        ('mainscript', 'defMirrorTexture'): ('mirrorscript', 'FStart', 'UnityEngine.RenderTexture', 'M.rt = new RenderTexture(mainscript.M.defMirrorTexture);'),
        ('mainscript', 'defMirrorMaterial'): ('mirrorscript', 'FStart', 'UnityEngine.Material', 'M.mat = new Material(mainscript.M.defMirrorMaterial);'),
        ('tavcsoscript', 'defRt'): ('tavcsoscript', 'Start', 'UnityEngine.RenderTexture', 'rt = new RenderTexture(defRt);'),
        ('tavcsoscript', 'defM'): ('tavcsoscript', 'Start', 'UnityEngine.Material', 'M = new Material(defM);')}
    for binding in links['creation_bindings']:
        owner, method, asset_type, statement = creation_rules[binding['type'], binding['field']]
        matches = [call for call in calls if call['assembly'] == assembly_path and call['type'] == owner
                   and call['method'] == method and call['target']['type'] == asset_type and call['target']['method'] == '.ctor']
        if statement not in code or len(matches) != 1:
            raise ValueError(f'Creation source proof missing: {owner}.{method}')
        target = ObjectId(**binding['target'])
        component = ObjectId(**binding['component'])
        if target not in catalog.dependencies[component]:
            raise ValueError('Creation declaration does not match serialized dependency')
        proven.append({'kind': 'source_descriptor_creation', 'binding': binding, 'call': matches[0],
                       'statement': statement, 'code_file': asdict(code_receipt), 'assembly_sha256': pinned,
                       'selection': 'Source declaration; component lifetime and singleton selection remain runtime behavior.'})
    if links['controller_assignments']:
        controller_calls = [call for call in calls if call['assembly'] == assembly_path
                            and call['type'] == 'weaponhandlingscript' and call['method'] == 'Update'
                            and call['target']['method'] == 'set_runtimeAnimatorController']
        if len(controller_calls) != 1 or 'anim.runtimeAnimatorController = melee.AnimationController;' not in code:
            raise ValueError('Controller assignment source proof missing')
        for assignment in links['controller_assignments']:
            if assignment['evidence']['sha256'] != pinned:
                raise ValueError('Controller evidence hash mismatch')
            proven.append({'kind': 'controller_assignment', 'assignment': assignment, 'call': controller_calls[0],
                           'code_file': asdict(code_receipt)})
        resolved_calls.add(call_key(controller_calls[0]))
    for call in calls:
        if call_key(call) not in resolved_calls:
            limits.append({'kind': 'code_asset_call', 'reason': 'The call alone does not prove a serialized result or runtime argument.',
                           'call': call})
    bone_calls = [call for call in calls if call['target']['method'] in ('set_bones', 'set_rootBone')]
    for rig in links['source_unbound_rigs']:
        limits.append({'kind': 'source_unbound_rig', 'rig': rig, 'bone_assignment_calls': bone_calls,
                       'reason': 'No exact bone identities follow from null serialized slots or the managed call index.'})
    summary = {'total': len(scripts), 'stored_component_references': sum(uses.values()),
               'used_script_types': sum(uses[ObjectId(**s['source'])] > 0 for s in scripts), 'exact_types': sum(s['status'] == 'resolved' for s in scripts),
               'unmatched': sum(s['status'] == 'unresolved' for s in scripts),
               'missing_assembly': sum(s['reason'] == 'declared_assembly_not_shipped' for s in scripts),
               'missing_declared_type': sum(s['reason'] == 'declared_type_not_in_archived_assembly' for s in scripts),
               'unmatched_referenced_scripts': sum(s['status'] == 'unresolved' and s['stored_component_count'] > 0 for s in scripts),
               'unmatched_referenced_components': sum(s['stored_component_count'] for s in scripts if s['status'] == 'unresolved'),
               'unmatched_unused_scripts': sum(s['status'] == 'unresolved' and s['stored_component_count'] == 0 for s in scripts),
               'unproven_candidate_declarations': sum(bool(s['candidates']) for s in scripts)}
    typed_decode = None
    if scripts:
        manifest_path = output / 'manifest.json'
        typed_decode = asdict(FileRecord('manifest.json', manifest_path.stat().st_size, sha256(manifest_path)))
    report = {'schema_version': SCHEMA_VERSION, 'script_summary': summary,
              'typed_decode_evidence': typed_decode, 'managed_files': [asdict(item) for item in managed_files],
              'scripts': scripts, 'resources': links['resources'], 'resource_managers': resources, 'asset_calls': calls,
              'proven_dynamic_links': proven, 'proven_creation_sites': creation_sites,
              'limits': limits, 'serialized_dependency_graph_changed': False}
    return _file(output, 'managed/analysis.json', _json(report))


def index_source_text(snapshot: Snapshot, output: Path) -> tuple[FileRecord, ...]:
    """Copy authored text files; the UI category owns installed Languages files."""
    files, entries = [], []
    for item in snapshot.files:
        if not item.file.lower().endswith('.txt') or item.file.startswith(f'{DATA_DIRECTORY}/Languages/'):
            continue
        payload = (snapshot.root / item.file).read_bytes()
        if len(payload) != item.bytes or hashlib.sha256(payload).hexdigest() != item.sha256:
            raise ValueError(f'Source text hash mismatch: {item.file}')
        receipt = _file(output, f'native/source-text/{item.file}', payload)
        files.append(receipt)
        entries.append({'source': asdict(item), 'output': asdict(receipt)})
    files.append(_file(output, 'native/source-text.json', _json({'schema_version': SCHEMA_VERSION, 'files': entries})))
    return tuple(sorted(files, key=lambda item: item.file))


def native_managed_links(source, catalog, output: Path) -> FileRecord:
    """Resolve serialized fields that the pinned module uses for dynamic creation."""
    from .records import _pointer
    from .animations import _dynamic_assignments
    rules = {'computerscript': ('defRenderTexture',), 'mainscript': ('defMirrorTexture', 'defMirrorMaterial'),
             'tavcsoscript': ('defRt', 'defM')}
    scripts, controllers, contexts, resources, bindings, rigs = {}, {}, [], [], [], []
    for identity, entry in sorted(catalog.objects.items()):
        if entry.type not in ('MonoScript', 'ResourceManager', 'Animator', 'AnimatorController', 'SkinnedMeshRenderer'):
            continue
        reader = source.get(identity.file, identity.path_id)
        data = source.read_record(reader)
        if entry.type == 'MonoScript':
            if data['m_ClassName'] in rules:
                scripts[identity] = data['m_ClassName']
        elif entry.type == 'ResourceManager':
            for name, pointer in data['m_Container']:
                target = _pointer(source, reader, pointer)
                if target is not None and target not in catalog.dependencies[identity]:
                    raise ValueError('Resource name has missing serialized dependency')
                resources.append({'name': name, 'manager': asdict(identity), 'target': asdict(target) if target else None})
        elif entry.type == 'AnimatorController':
            controllers[identity] = tuple(_pointer(source, reader, value) for value in data['m_AnimationClips'])
        elif entry.type == 'Animator':
            contexts.append({'owner': asdict(identity), 'clips': []})
        else:
            mesh = _pointer(source, reader, data['m_Mesh'])
            bones = tuple(_pointer(source, reader, value) for value in data['m_Bones'])
            root = _pointer(source, reader, data['m_RootBone'])
            if mesh and bones and not any(bones) and root is None:
                rigs.append({'renderer': asdict(identity), 'mesh': asdict(mesh), 'bone_slots': len(bones),
                             'root_bone': None, 'reason': 'Serialized bone slots and RootBone are null.'})
    for identity, entry in sorted(catalog.objects.items()):
        if entry.type != 'MonoBehaviour':
            continue
        owners = [value for value in catalog.dependencies[identity] if value in scripts]
        if not owners:
            continue
        if len(owners) != 1:
            raise ValueError('Component has multiple creation script owners')
        script = owners[0]
        reader = source.get(identity.file, identity.path_id)
        data = source.read_record(reader)
        for field in rules[scripts[script]]:
            target = _pointer(source, reader, data[field])
            if target is not None:
                if target not in catalog.dependencies[identity]:
                    raise ValueError('Creation field has missing serialized dependency')
                bindings.append({'component': asdict(identity), 'script': asdict(script), 'type': scripts[script],
                                 'field': field, 'target': asdict(target) if target else None})
    if contexts:
        _dynamic_assignments(source, catalog, controllers, contexts)
    assignments = [{'animator': context['owner'], **item} for context in contexts
                   for item in context.get('controller_assignments', [])]
    return _file(output, 'native/managed-links.json', _json({'schema_version': SCHEMA_VERSION,
           'resources': resources, 'creation_bindings': bindings, 'controller_assignments': assignments,
           'source_unbound_rigs': rigs}))
