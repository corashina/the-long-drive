"""Offline export for the selected Steam build."""
import argparse
import hashlib
import json
import re
import subprocess
from pathlib import Path
from .library.types import Snapshot, LibraryReceipt
from .plates import plate_resources

APP_ID = 1017180
BUILD_ID = 11140890
SOURCE_WORK_ROOT = Path('.cache/reference-library')
NATIVE_ENGINE_SHA256 = '553c1e7560e1b42bd4638c53d0168dcd1b4b9eb5b9df324b0a44f14b3f18bc86'


def native_engine_evidence(steam_root: Path):
    with (steam_root / 'UnityPlayer.dll').open('rb') as stream:
        digest = hashlib.file_digest(stream, 'sha256').hexdigest()
    if digest != NATIVE_ENGINE_SHA256:
        raise ValueError('UnityPlayer.dll differs from the checked native cloud noise source')
    return {'file': 'UnityPlayer.dll', 'version': '2021.3.18f1 (3129e69bc0c7)', 'sha256': digest}


def validate_build(steam_root: Path):
    path = steam_root.parents[1] / f'appmanifest_{APP_ID}.acf'
    values = dict(re.findall(r'"([^"\r\n]+)"\s+"([^"\r\n]+)"', path.read_text(encoding='utf-8')))
    if values.get('appid') != str(APP_ID):
        raise ValueError(f'Steam app mismatch: expected {APP_ID}')
    if values.get('buildid') != str(BUILD_ID):
        raise ValueError(f'Steam build mismatch: expected {BUILD_ID}, got {values.get("buildid")}')
    return values



def derive_reference(snapshot: Snapshot, library: LibraryReceipt, output: Path) -> None:
    import tempfile
    from .library.reader import LibraryReader
    from .source import LibrarySource
    if snapshot.manifest_sha256 != library.source_manifest_sha256:
        raise ValueError('Runtime source provenance mismatch')
    reader = LibraryReader(library)
    if snapshot.files != reader.snapshot.files or snapshot.root.resolve() != reader.snapshot.root.resolve():
        raise ValueError('Runtime source storage mismatch')
    target = output.resolve()
    for protected in (snapshot.manifest_path.parent.resolve(), reader.root):
        if target.is_relative_to(protected) or protected.is_relative_to(target):
            raise ValueError('Runtime output overlaps accepted library storage')
    from .library.storage import RUNTIME_BYTES, check_space, tree_bytes
    audio_bound = sum(f['bytes'] for entry in reader.exports.values()
        if entry['details']['type'] == 'AudioClip' for f in entry['files'])
    check_space('runtime: selected scene and audio staging', target.parent,
                RUNTIME_BYTES + audio_bound, retained=tree_bytes(output))
    output.parent.mkdir(parents=True, exist_ok=True)
    staged = Path(tempfile.mkdtemp(prefix='runtime-', dir=target.parent))
    try:
        _export_selected(LibrarySource(reader), staged, reader, snapshot)
        verify_reference(staged, library)
        from .runtime_package import publish_package
        publish_package(staged, output.parent)
    except BaseException:
        import shutil
        shutil.rmtree(staged)
        raise
    promote_reference(staged, output)


def promote_reference(staged: Path, accepted: Path) -> None:
    from .library.validation import _publish_directory
    _publish_directory(staged, accepted)


def provenance(library):
    return {'schema_version':1, 'manifest_sha256':library.manifest_sha256,
            'catalog_sha256':library.catalog_sha256, 'source_manifest_sha256':library.source_manifest_sha256}


def runtime_resources(manifest):
    resources = {}
    def collect(value):
        if isinstance(value, dict):
            if 'path' in value and 'sha256' in value:
                path = value['path']
                if path in resources and resources[path]['sha256'] != value['sha256']:
                    raise ValueError('Conflicting source resource hashes')
                resources[path] = value
            for child in value.values(): collect(child)
        elif isinstance(value, list):
            for child in value: collect(child)
    collect(manifest.get('lighting', {}))
    collect(manifest.get('particles', []))
    collect(manifest.get('weather', {}))
    collect(manifest.get('sky', {}))
    collect(manifest.get('conditions', {}))
    collect(manifest.get('clothing', {}))
    for p in manifest.get('item_prototypes',[]):collect(p.get('consumable'))
    collect(manifest.get('skid',{}).get('particles',[]))
    collect(manifest.get('survival',{}).get('food_particles',[]))
    for item in manifest.get('render_lighting', {}).get('dependencies', []):
        collect({'path':item['file'],'sha256':item['sha256']})
    world=manifest.get('world', {})
    collect(world.get('terrain', {}))
    collect(world.get('material_textures', []))
    for prototype in world.get('prototypes', []):
        collect(prototype.get('deferred_materials', []))
        vehicle=prototype.get('vehicle') or {}
        collect(vehicle.get('exhaust', []))
        collect(vehicle.get('coolant_steam'))
        collect([[w['dust'],w['grass']] for w in vehicle.get('wheel_effects',[])])
    return list(resources.values())


def validate_runtime_paths(manifest):
    assets = manifest.get('assets', [])
    world = manifest.get('world')
    if not world or not world.get('prototypes'):
        raise ValueError('Missing required source world dependency')
    from .item_prototypes import GIBS, SELECTED
    from .sky import PREFABS
    expected = {f'reference/{name}.gltf' for name in ('home','items','road_lamp','sky','food','player','ragdoll','creature_rabbit',*(p[3] for p in SELECTED),*(f'item_gib_{g}' for g in GIBS),*(p[0] for p in PREFABS))}
    prototype_ids={p['asset'] for p in world['prototypes']}
    material_ids={p['asset'] for p in world['painting_materials']}
    expected.update(a['path'] for a in manifest['assets'] if a['id'] in material_ids and a['usage']=='world_material')
    expected.update(a['path'] for a in manifest['assets'] if a['id'] in prototype_ids and a['usage']=='world_prototype')
    if {a['path'] for a in assets} != expected or len(assets) != len(expected):
        raise ValueError('Runtime must contain only selected scenes and world prototypes')
    resources = {item['path']:item['sha256'] for item in
                 [*runtime_resources(manifest), *plate_resources(manifest['plates'])]}
    for asset in assets:
        for item in [asset, *asset['dependencies']]:
            name = item.get('file', item.get('path'))
            if name in resources:
                if item['sha256'] != resources[name]:
                    raise ValueError('Runtime dependency hash differs from typed resource')
                continue
            if not name.startswith('reference/') or len(Path(name).parts) != 2 or Path(name).suffix not in ('.gltf','.bin','.png','.bc'):
                raise ValueError('Invalid selected runtime dependency')
    for item in runtime_resources(manifest):
        name = item['path']
        valid_prefix=name.startswith(('reference/lighting-','reference/texture-','reference/source-cloud.','reference/source-caster-dither-' ))
        if not valid_prefix or len(Path(name).parts) != 2 or Path(name).suffix not in ('.gltf','.bin','.png','.smip','.txt','.json'):
            raise ValueError('Invalid selected runtime lighting resource')
    for item in plate_resources(manifest['plates']):
        name=item['path']
        if not name.startswith('reference/plate-') or len(Path(name).parts)!=2 or Path(name).suffix not in ('.bin','.png','.json'):
            raise ValueError('Invalid selected runtime plate resource')
    sprites = manifest.get('ui', {}).get('sprites', [])
    paths = set()
    for sprite in sprites:
        source = sprite['source']
        expected = f"reference/ui-{source['file']}-{source['path_id']}.png"
        if sprite['path'] != expected or len(Path(expected).parts) != 2 or expected in paths:
            raise ValueError('Invalid selected runtime UI sprite')
        paths.add(expected)


def validate_runtime_manifest(content: bytes) -> None:
    """Use the game's contract before accepting runtime output."""
    root = Path(__file__).resolve().parents[2]
    namespace = hashlib.sha256(str(root.resolve()).rstrip('\\/').lower().encode()).hexdigest()
    command = ['docker', 'run', '--rm', '--pull=never', '-i',
        '--mount', f'type=bind,source={root},target=/workspace,readonly',
        '--mount', 'type=volume,source=tld-wasm-cargo,target=/cargo',
        '--mount', 'type=volume,source=tld-wasm-target,target=/target',
        '--env', f'CARGO_TARGET_DIR=/target/{namespace}',
        '--env', 'CARGO_BUILD_JOBS=2',
        'the-long-slop-toolchain', 'cargo', 'run', '--quiet', '--locked',
        '--bin', 'validate-reference']
    try:
        result = subprocess.run(command, input=content, capture_output=True, check=False)
    except OSError as error:
        raise ValueError('Runtime manifest contract validation needs Docker and the project toolchain image') from error
    if result.returncode:
        detail = result.stderr.decode('utf-8', errors='replace').strip()
        raise ValueError(f'Runtime manifest contract validation failed: {detail}')


def verify_reference(output: Path, library) -> None:
    from .library.reader import checked
    from .gltf import validate_gltf
    content = (output / 'manifest.json').read_bytes()
    manifest = json.loads(content)
    if library is None or manifest.get('library') != provenance(library):
        raise ValueError('Missing or mismatched runtime library provenance')
    if manifest['library']['schema_version'] != 1 or any(not re.fullmatch('[0-9a-f]{64}', manifest['library'][k]) for k in ('manifest_sha256','catalog_sha256','source_manifest_sha256')):
        raise ValueError('Unsupported runtime provenance')
    world = manifest.get('world', {})
    if world.get('library') != provenance(library):
        raise ValueError('Missing or mismatched world current library provenance')
    from .acquisition import validate_acquisition
    validate_acquisition(manifest['acquisition'], manifest)
    validate_runtime_manifest(content)
    validate_runtime_paths(manifest)
    selection=json.loads((output/'library-selection.json').read_bytes())
    evidence_receipts=selection.get('evidence',[])
    if [record['file'] for record in evidence_receipts]!=['conditions-evidence.json']:
        raise ValueError('Missing condition evidence receipt')
    for record in evidence_receipts:checked(output,record)
    if json.loads((output/'conditions-evidence.json').read_bytes())['conditions']!=manifest['conditions']:
        raise ValueError('Condition evidence differs from runtime records')
    names = {'manifest.json','geometry-evidence.json','parameter-evidence.json','library-selection.json','lighting-evidence.json','material-evidence.json','plate-evidence.json','flashlights-evidence.json','storage-evidence.json','driving-evidence.json','conditions-evidence.json'}
    for asset in manifest['assets']:
        for record in [asset, *asset['dependencies']]:
            name = record.get('file', record.get('path')).removeprefix('reference/')
            path = output / name
            checked(output, {'file':name,'bytes':path.stat().st_size,'sha256':record['sha256']})
            names.add(name)
        scene = output / asset['path'].removeprefix('reference/')
        document = json.loads(scene.read_bytes())
        uris = {entry['uri'] for entry in [*document.get('buffers', []), *document.get('images', [])]}
        resources = {item['path'].removeprefix('reference/') for item in
                     [*runtime_resources(manifest), *plate_resources(manifest['plates'])]}
        declared = {entry['file'].removeprefix('reference/') for entry in asset['dependencies']}
        from .native_bc import texture_inputs, validate_storage
        files = {r['file']: r for r in manifest['file_catalog']['files']}
        selected = [t for m in manifest['rendering']['materials'] if m['asset'] == asset['id'] for t in m['properties']['textures']]
        for texture in selected: validate_storage(texture, files)
        material_mips = {Path(name).name for texture in selected for name in texture_inputs(texture)}
        required = uris | material_mips
        if not required <= declared or declared - required - resources:
            raise ValueError('Runtime scene dependencies differ from selected receipts')
        validate_gltf(scene)
    from .clothing import validate_player_export
    validate_player_export(manifest['clothing'],manifest,output)
    for sprite in [*manifest.get('ui', {}).get('sprites', []), *manifest.get('audio', {}).get('clips', [])]:
        name = sprite['path'].removeprefix('reference/')
        path = output / name
        checked(output, {'file':name, 'bytes':path.stat().st_size, 'sha256':sprite['sha256']})
        names.add(name)
    from .audio import validate_audio_export
    validate_audio_export(manifest['audio'], output, manifest)
    names.update(c['path'].removeprefix('reference/') for c in manifest['audio']['clips'])
    from .lighting import validate_lighting_export
    validate_lighting_export(manifest, output)
    names.update(Path(item['path']).name for item in runtime_resources(manifest))
    from .runtime_package import file_receipt, package_id
    if manifest['content_base'] != f'assets/runtime/{package_id(manifest)}/':
        raise ValueError('Runtime package identity differs')
    for receipt in manifest['file_catalog']['files']:
        name = receipt['file'].removeprefix('reference/')
        if file_receipt(output / name, receipt['file']) != receipt:
            raise ValueError('Runtime file receipt differs')
        names.add(name)
    if {p.relative_to(output).as_posix() for p in output.rglob('*') if p.is_file()} != names:
        raise ValueError('Unselected files in runtime')


def _export_selected(source, output: Path, library, snapshot) -> None:
    from .gltf import export_gltf
    from .lighting import recover_lighting, road_source, validate_lighting_export
    from .source import key, source_id
    from .parameters import recover_parameters
    from .manifest import build_manifest
    from .conditions import prepare_conditions, prepare_cleaning_roots, bind_condition_audio
    from .visibility import recover_camera_visibility
    from .ui import export_ui
    from .audio import recover_audio
    from .render_lighting import prepare_startup_lighting, recover_lighting as recover_render_lighting
    if output.resolve().is_relative_to(source.root.resolve()):
        raise ValueError('Keep generated files outside the source installation')
    def transform(go):return source.transform_for(source.get('level1',go))
    roots = {'home':[source.get('level1',21400)],
        'items':[transform(go) for go in (13023,13027,18878,13019,15637)]}
    spare=source.get('level1',71403)
    from .flashlights import prepare_flashlight_roots, validate_flashlight_mesh_export
    prepare_flashlight_roots(source, roots)
    prepare_cleaning_roots(source, roots)
    roots['items'].append(source.transform_for(source.resolve(spare,source.read(spare)['m_GameObject'])))
    roots['items'].append(transform(3542))
    from .survival import prepare_food, recover_survival, bind_survival
    from .particles import recover_particles, recover_outlet_particles, validate_particles_export
    from .audio import recover_ambient
    from .weather import recover_weather, validate_weather_export
    prepare_food(source, roots)
    food_roots=roots.pop('food')
    from .world import LADA
    prefab=source.get(*LADA)
    if source.read(source.get('level1',64896))['FirstHome']['m_PathID']!=21400:
        raise ValueError('Selected home link differs from the required home')
    prepare_conditions(source,[root for readers in roots.values() for root in readers])
    output.mkdir(parents=True, exist_ok=True)
    native_engine = native_engine_evidence(snapshot.root)
    source.lighting_probe_prototype=prefab
    # Guns, caps and world usables add GCMMB and weapon crosshair sprites. ui exports them.
    source.crosshair_icons=set()
    source.lighting_root_assets={key(root):i for i,readers in enumerate(roots.values(),1) for root in readers}
    _,pole=road_source(source)
    clouds=source.get('level1',27217)
    source.lighting_prototype_roots={key(pole),key(clouds)}
    source.lighting_pole_root=key(pole)
    source.lighting_root_assets[key(pole)]=len(roots)+1
    source.lighting_root_assets[key(clouds)]=len(roots)+2
    recover_lighting(source,[root for readers in roots.values() for root in readers],output)
    prepare_startup_lighting(source,[root for readers in roots.values() for root in readers])
    source.render_lighting=recover_render_lighting(source,[root for readers in roots.values() for root in readers],output)
    roots['road_lamp']=[pole]
    roots['sky']=[clouds]
    roots['food']=food_roots
    source.lighting_prototype_roots.update(key(root) for root in food_roots)
    from .item_prototypes import prepare_item_prototypes, recover_item_prototypes, validate_item_prototypes
    prepare_item_prototypes(source, roots)
    recover_survival(source,roots,output)
    particles=recover_particles(source,output)
    weather=recover_weather(source,output)
    roots['player']=[source.get('level1',39843)]
    source.lighting_prototype_roots.add(('level1',39843))
    from .ragdoll import prepare_ragdoll
    prepare_ragdoll(source, roots)
    from .sky import prepare_sky, recover_sky, recover_flyer, validate_sky
    prepare_sky(source, roots)
    from .creatures import prepare_creatures, recover_creatures, validate_creatures
    prepare_creatures(source, roots)
    from .world import prepare_home_cable
    prepare_home_cable(source)
    report = {}
    for name, readers in roots.items():
        report[name] = export_gltf((source,readers),output/f'{name}.gltf',library,list(roots).index(name)+1,name!='sky')
        if name == 'items': validate_flashlight_mesh_export(source, output/'items.gltf')
        print(f'Exported {name}: {report[name]["node_count"]} nodes',flush=True)
    (output/'material-evidence.json').write_text(json.dumps(list(source.material_evidence.values()), indent=2, allow_nan=False), encoding='utf-8')
    (output/'geometry-evidence.json').write_text(json.dumps(report,indent=2,allow_nan=False),encoding='utf-8')
    parameter_roots={f'{name}-{i}':root for name,readers in roots.items() for i,root in enumerate(readers)}
    parameter_roots['required-prefab']=prefab
    parameters = recover_parameters(source,parameter_roots)
    parameters['conditions']=source.condition_evidence
    parameters['camera_visibility']=recover_camera_visibility(source)
    (output/'parameter-evidence.json').write_text(json.dumps(parameters,indent=2,allow_nan=False),encoding='utf-8')
    if parameters['errors']:
        raise ValueError(f'Incomplete parameter recovery: {parameters["errors"]}')
    hashes=[{'file':record.file.removeprefix('TheLongDrive_Data/'),'sha256':record.sha256}
            for record in snapshot.files if record.file.startswith('TheLongDrive_Data/')]
    manifest=build_manifest(source,roots,report,hashes,library,prefab,output)
    (output/'driving-evidence.json').write_text(json.dumps(source.driving_evidence,indent=2,allow_nan=False),encoding='utf-8')
    from .plates import plate_source_evidence
    (output/'plate-evidence.json').write_text(json.dumps(plate_source_evidence(source,manifest['plates']),indent=2,allow_nan=False),encoding='utf-8')
    from .world import recover_world, recover_home_cable
    from .manifest import attach_world
    attach_world(manifest,recover_world(source,library,output,len(manifest['assets'])+1))
    manifest['world']['home_cable']=recover_home_cable(source,manifest['nodes'])
    from .storage import recover_prototype_storage
    recover_prototype_storage(source,manifest,output)
    roles=[c['role'] for p in manifest['world']['prototypes'] for c in p['colliders'] if c['role']]
    print(f"Exported trigger roles: {sum('ladder' in r for r in roles)} ladders, {sum('hazard' in r for r in roles)} hazards",flush=True)
    report['world']=[{'source':p['source'],'asset':p['asset'],'node_count':len(p['nodes']),'body_count':len(p['bodies']),'collider_count':len(p['colliders']),'control_count':len(p['controls']),'skin_bone_counts':[len(s['bones']) for s in p['skins']]} for p in manifest['world']['prototypes']]
    (output/'geometry-evidence.json').write_text(json.dumps(report,indent=2,allow_nan=False),encoding='utf-8')
    (output/'material-evidence.json').write_text(json.dumps(list(source.material_evidence.values()),indent=2,allow_nan=False),encoding='utf-8')
    manifest['library']=provenance(library.receipt)
    from .inventory import recover_inventory
    manifest['inventory'] = recover_inventory(source, manifest)
    bind_survival(source,manifest)
    particles.extend(recover_outlet_particles(source,manifest,output))
    manifest['particles']=particles
    manifest['ambient']=recover_ambient(source)
    manifest['lighting']['textures']=list(source.lighting_textures.values())
    validate_particles_export(particles,manifest)
    manifest['weather']=weather
    validate_weather_export(weather,manifest)
    manifest['sky']=recover_sky(source,manifest,output)
    manifest['sky']['flyer']=recover_flyer(source,manifest)
    manifest['item_prototypes'] = recover_item_prototypes(source, manifest, output)
    from .small_items import recover_small_items, validate_small_items
    manifest['small_items'] = recover_small_items(source, manifest, output)
    from .conditions import recover_prototype_tools
    manifest['conditions']['prototype_tools'] = recover_prototype_tools(source, manifest, output)
    from .computer import recover_computer, validate_computer
    manifest['computer'] = recover_computer(source, manifest)
    from .melee import recover_melee, validate_melee
    manifest['melee'] = recover_melee(source, manifest, output)
    from .particles import recover_food_particles
    manifest['survival']['food_particles'] = recover_food_particles(source, output)
    from .guns import recover_guns, validate_guns
    # Before audio and UI: gun clips join source.item_clips, crosshair icons join the UI sprites.
    manifest['guns'] = recover_guns(source, manifest, output)
    from .explosions import recover_explosions, validate_explosions
    # Before audio: explosion and ear ring clips join source.item_clips.
    manifest['explosions'] = recover_explosions(source, manifest, output)
    from .crash import recover_crash, validate_crash
    manifest['crash'] = recover_crash(source, manifest, output)
    from .breakables import recover_breakables, recover_gib_constants
    # Before audio: breakable clips join source.item_clips.
    manifest['breakables'] = recover_breakables(source, manifest)
    manifest['gib_constants'] = recover_gib_constants(source)
    # PhysicsManager: PhysX puts a body to sleep below this mass-normalized kinetic energy.
    physics=source.get('globalgamemanagers',10)
    if physics.type.name!='PhysicsManager':raise ValueError('globalgamemanagers 10 is not PhysicsManager')
    manifest['physics']={'source':source_id(physics),'sleep_threshold':source.read(physics)['m_SleepThreshold']}
    # Before audio: the rabbit die clips get clip ids there.
    manifest['creatures'] = recover_creatures(source, manifest, output)
    from .wheel_effects import recover_skid, validate_skid
    manifest['skid'] = recover_skid(source, output)
    from .clothing import recover_clothing,validate_clothing
    manifest['clothing'] = recover_clothing(source,manifest,output)
    manifest['lighting']['textures']=list(source.lighting_textures.values())
    resources={entry['file'] for entry in manifest['assets'][0]['dependencies']}
    manifest['assets'][0]['dependencies'].extend({'file':r['path'],'sha256':r['sha256']} for r in runtime_resources(manifest) if r['path'] not in resources)
    manifest['ui']=export_ui(source,output,library)
    from .language import recover_language
    manifest['language']=recover_language(source)
    manifest['audio']=recover_audio(source,{name:readers for name,readers in roots.items() if not name.startswith(('item_','sky_','creature_')) and name not in ('player','ragdoll')},manifest,library,output)
    bind_condition_audio(manifest['conditions'],manifest['audio'])
    from .acquisition import recover_acquisition, validate_acquisition
    manifest['acquisition'] = recover_acquisition(source, manifest)
    validate_acquisition(manifest['acquisition'], manifest)
    validate_item_prototypes(manifest['item_prototypes'], manifest)
    validate_sky(manifest['sky'], manifest)
    validate_creatures(manifest['creatures'], manifest)
    validate_small_items(manifest['small_items'], manifest)
    validate_computer(manifest['computer'], manifest)
    validate_melee(manifest['melee'], manifest)
    validate_guns(manifest['guns'], manifest)
    validate_explosions(manifest['explosions'], manifest)
    validate_crash(manifest['crash'], manifest)
    validate_skid(manifest['skid'])
    validate_clothing(manifest['clothing'],manifest)
    evidence=json.loads((output/'conditions-evidence.json').read_bytes())
    evidence['conditions']=manifest['conditions']
    (output/'conditions-evidence.json').write_text(json.dumps(evidence,indent=2,allow_nan=False),encoding='utf-8')
    validate_lighting_export(manifest,output)
    (output/'lighting-evidence.json').write_text(json.dumps({'lighting':manifest['lighting'],'render_lighting':manifest['render_lighting'],'byte_checks':source.evidence,'native_engine':native_engine,'fog_setting':source.lighting_fog_evidence,'library':manifest['library']},indent=2,allow_nan=False),encoding='utf-8')
    parameters['controls']={
        'source':{'file':'level0','path_id':16865},
        'slider_multiplier_rule':'slidersettingscript.FStart changes zero valueMultipler to one; GetValue returns value * valueMultipler.',
        'carry_minimum_rule':'Distance range derives from the authored carry marker and maxPickupDist offset sphere.',
    }
    parameters['player_markers']=[{'source':{'file':'level1','path_id':pid},'fields':source.read(source.get('level1',pid))} for pid in (39823,23339)]
    (output/'parameter-evidence.json').write_text(json.dumps(parameters,indent=2,allow_nan=False),encoding='utf-8')
    from .runtime_package import prepare_package
    # Package preparation also checks and catalogs texture batch descriptors.
    prepare_package(manifest, output)
    (output/'manifest.json').write_text(json.dumps(manifest,separators=(',',':'),allow_nan=False),encoding='utf-8')
    from dataclasses import asdict
    selection = {name:[asdict(v) for v in sorted(values)] for name,values in (
        ('records',library.selected_records),('meshes',library.selected_meshes),
        ('textures',library.selected_textures),('sprites',library.selected_sprites),('audio',library.selected_audio))}
    evidence_path=output/'conditions-evidence.json'
    selection['evidence']=[{'file':evidence_path.name,'bytes':evidence_path.stat().st_size,'sha256':hashlib.sha256(evidence_path.read_bytes()).hexdigest()}]
    (output/'library-selection.json').write_text(json.dumps(selection,indent=2),encoding='utf-8')
    print(f'Exported manifest: {len(manifest["colliders"])} colliders, {len(manifest["items"])} items',flush=True)


def stage_source(steam_root: Path | None, verify: bool = False) -> Snapshot:
    from .library.archive import load_snapshot, snapshot_source, verify_snapshot
    from .library.inventory import inventory_source, catalog_data
    from .source import Source
    work_root = SOURCE_WORK_ROOT
    if verify:
        snapshot = load_snapshot(work_root)
        verify_snapshot(snapshot)
    else:
        if steam_root is None:
            raise ValueError('Source staging needs --steam-root')
        snapshot = snapshot_source(steam_root, work_root)
    source = Source(snapshot.root)
    catalog = inventory_source(source, snapshot)
    if len(source.serialized_files) != 9 or len(catalog.objects) != 262784:
        raise ValueError(f'Installed inventory mismatch: {len(source.serialized_files)} serialized files, {len(catalog.objects)} objects')
    data = catalog_data(catalog, source, snapshot)
    catalog_path = work_root / 'catalog.json'
    if verify or catalog_path.exists():
        accepted = json.loads(catalog_path.read_text(encoding='utf-8'))
        for item in accepted['objects']:
            item['dependencies'] = []
        if accepted != data:
            raise ValueError('Archived catalog differs from the complete inventory')
    else:
        temporary = work_root / '.catalog.json'
        temporary.write_text(json.dumps(data, indent=2, allow_nan=False) + '\n', encoding='utf-8')
        temporary.replace(catalog_path)
    action = 'Verified' if verify else 'Archived'
    print(f'{action} source: {len(source.serialized_files)} serialized files, {len(source.resource_streams)} streams, {len(catalog.objects):,} objects', flush=True)
    return snapshot


def stage_catalog(steam_root: Path | None, verify: bool = False):
    from .library.archive import load_snapshot, verify_snapshot
    from .library.inventory import catalog_data
    from .library.records import build_catalog, verify_catalog
    from .source import Source
    import shutil
    work_root = SOURCE_WORK_ROOT
    if not (work_root / 'source-manifest.json').exists():
        if verify:
            raise ValueError('No accepted source stage')
        stage_source(steam_root)
    snapshot = load_snapshot(work_root)
    verify_snapshot(snapshot)
    source = Source(snapshot.root)
    catalog_path = work_root / 'catalog.json'
    records_root = work_root / 'records'
    if verify or (records_root / 'manifest.json').exists():
        catalog = verify_catalog(source, snapshot, catalog_path, records_root)
    else:
        if records_root.exists():
            raise ValueError('Unaccepted records tree; keep it for diagnosis')
        from .library.storage import CATALOG_BYTES, check_space, tree_bytes
        check_space('catalog: typed records and catalog staging', work_root,
                    CATALOG_BYTES, retained=tree_bytes(work_root))
        pending = work_root / 'work' / 'catalog-records'
        if not pending.resolve().is_relative_to(work_root.resolve()):
            raise ValueError('Catalog work path escapes work root')
        if pending.exists():
            shutil.rmtree(pending)
        pending.mkdir(parents=True)
        catalog = build_catalog(source, snapshot, pending)
        data = catalog_data(catalog, source, snapshot)
        content = (json.dumps(data, indent=2, allow_nan=False) + '\n').encode('utf-8')
        receipt = json.loads((pending / 'manifest.json').read_bytes())
        receipt['catalog_sha256'] = hashlib.sha256(content).hexdigest()
        (pending / 'manifest.json').write_text(json.dumps(receipt, sort_keys=True, separators=(',', ':'), allow_nan=False) + '\n', encoding='utf-8')
        temporary = work_root / 'work' / 'catalog.json'
        temporary.write_bytes(content)
        # Publish acceptance only after every required record and graph check passes.
        if not records_root.resolve().is_relative_to(work_root.resolve()):
            raise ValueError('Records path escapes work root')
        from .library.validation import _publish_directory
        _publish_directory(pending, records_root)
        try:
            temporary.replace(catalog_path)
        except BaseException:
            # No previous accepted record stage exists on this path.
            records_root.replace(pending)
            raise
    action = 'Verified' if verify else 'Indexed'
    edges = sum(len(value) for value in catalog.dependencies.values())
    print(f'{action} catalog: {len(catalog.objects):,} records, {edges:,} dependency edges', flush=True)
    return catalog


def _category_stages(snapshot, catalog):
    """Reuse source-bound stages, or build missing category prerequisites once."""
    import tempfile
    import shutil
    from dataclasses import asdict
    from .source import Source
    from .library import (export_images, export_ui, export_audio, export_models,
                          export_animations, export_native_assets, index_source_text,
                          index_managed, write_analysis_report)
    from .library.archive import sha256
    from .library.records import _file, _json
    from .library.validation import CATEGORIES, load_exports, _receipt, _check_files, _verify_managed, _publish_directory
    root = snapshot.manifest_path.parent
    path = root / 'category-stages.json'
    bindings = {'schema_version': 1, 'source_manifest_sha256': snapshot.manifest_sha256,
                'catalog_sha256': sha256(root / 'catalog.json'),
                'records_manifest_sha256': sha256(root / 'records/manifest.json')}
    if path.exists():
        saved = json.loads(path.read_bytes())
        if any(saved.get(k) != v for k, v in bindings.items()):
            raise ValueError('Category stages differ from the accepted source or catalog')
    else:
        (root / 'work').mkdir(exist_ok=True)
        pending = Path(tempfile.mkdtemp(prefix='category-assets-', dir=root / 'work'))
        source = Source(snapshot.root)
        prior = (*export_images(source, catalog, pending), *export_ui(source, catalog, pending),
                 *export_audio(source, catalog, pending))
        models = export_models(source, catalog, pending)
        prior = (*prior, *models, *export_animations(source, catalog, models, pending))
        records = root / 'records'
        native_root = pending / 'record-inputs'
        native_root.mkdir()
        # This partial root supplies producers only. It is not an accepted record stage.
        shutil.copyfile(records / 'manifest.json', native_root / 'manifest.json')
        for identity, item in catalog.objects.items():
            if item.type in ('MonoScript', 'ResourceManager'):
                name = f'{identity.file}/{identity.path_id}.json'
                (native_root / name).parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(records / name, native_root / name)
        export_native_assets(source, catalog, native_root, frozenset(e.source for e in prior))
        index_source_text(snapshot, native_root)
        if not (records / 'managed/manifest.json').exists():
            managed = index_managed(snapshot, native_root)
            analysis = write_analysis_report(catalog, managed, native_root)
            _file(native_root, 'managed/manifest.json', _json({'schema_version': 1,
                  'files': [asdict(f) for f in (*managed, analysis)]}))
            _verify_managed(snapshot, native_root)
            _publish_directory(native_root / 'managed', records / 'managed')
        else:
            _verify_managed(snapshot, records)
        saved = {**bindings, 'categories': [{'category': category,
                 'root': (native_root if category == 'native' else pending).relative_to(root).as_posix(),
                 'index': asdict(_receipt(native_root if category == 'native' else pending, f'{category}/index.json'))}
                 for category in CATEGORIES]}
        _file(root, 'category-stages.json', _json(saved))
    if [e['category'] for e in saved['categories']] != list(CATEGORIES):
        raise ValueError('Missing or duplicate category stages')
    stages = []
    for entry in saved['categories']:
        from .library.types import FileRecord
        from .library.archive import validate_paths, _check_path
        validate_paths([entry['root']])
        directory = root / entry['root']
        _check_path(directory, root.resolve())
        record = FileRecord(**entry['index'])
        if record.file != f"{entry['category']}/index.json":
            raise ValueError('Category index identity mismatch')
        _check_files(directory, {record.file: record})
        stages.append((entry['category'], directory, load_exports(directory / record.file)))
    return stages


def stage_library(steam_root: Path | None, output: Path, verify: bool = False):
    import tempfile
    from .library.archive import load_snapshot
    from .library.validation import (load_catalog, check_installed_counts, assemble_library,
                                     publish_library, verify_library)
    library = output.parent / 'library'
    if verify:
        receipt = verify_library(library, SOURCE_WORK_ROOT / 'source')
        check_installed_counts(load_catalog(SOURCE_WORK_ROOT / 'catalog.json'))
        print(f'Verified library: {len(receipt.exports):,} owners; {receipt.manifest_sha256}', flush=True)
        return receipt
    from .library.managed import preflight_managed
    from .library.storage import (library_space, assembly_bytes, check_space, tree_bytes, with_headroom)
    if not (SOURCE_WORK_ROOT / 'category-stages.json').exists() and not (SOURCE_WORK_ROOT / 'records/managed/manifest.json').exists():
        preflight_managed()
    library_space(SOURCE_WORK_ROOT, library)
    if not (SOURCE_WORK_ROOT / 'source-manifest.json').exists():
        stage_source(steam_root)
    if not (SOURCE_WORK_ROOT / 'records/manifest.json').exists():
        stage_catalog(steam_root)
    snapshot = load_snapshot(SOURCE_WORK_ROOT)
    catalog = load_catalog(SOURCE_WORK_ROOT / 'catalog.json')
    check_installed_counts(catalog)
    stages = _category_stages(snapshot, catalog)
    check_space('library assembly: complete category receipts plus acceptance metadata', SOURCE_WORK_ROOT,
                with_headroom(assembly_bytes(stages)), retained=tree_bytes(SOURCE_WORK_ROOT) + tree_bytes(library))
    pending = Path(tempfile.mkdtemp(prefix='library-', dir=SOURCE_WORK_ROOT / 'work'))
    exports = assemble_library(snapshot, stages, pending)
    print(f'Assembled library: {len(exports):,} owners', flush=True)
    receipt = publish_library(snapshot, catalog, exports, pending, library)
    print(f'Published library: {len(exports):,} owners; {receipt.manifest_sha256}', flush=True)
    return receipt


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--steam-root', type=Path)
    parser.add_argument('--library-root', type=Path)
    parser.add_argument('--output', type=Path, default=Path('web/assets/reference'))
    parser.add_argument('--stage', choices=('runtime', 'source', 'catalog', 'library'), default='runtime')
    parser.add_argument('--verify', action='store_true')
    arguments = parser.parse_args(argv)
    if arguments.stage in ('library', 'runtime') and arguments.steam_root:
        steam = arguments.steam_root.resolve()
        destinations = (arguments.output.resolve(), (arguments.output.parent / 'library').resolve())
        if any(path.is_relative_to(steam) or steam.is_relative_to(path) for path in destinations):
            parser.error('Keep runtime and library output separate from the Steam installation')
    if arguments.library_root and arguments.stage != 'runtime':
        parser.error('--library-root requires --stage runtime')
    if arguments.stage == 'source':
        stage_source(arguments.steam_root, verify=arguments.verify)
    elif arguments.stage == 'catalog':
        stage_catalog(arguments.steam_root, verify=arguments.verify)
    elif arguments.stage == 'library':
        stage_library(arguments.steam_root, arguments.output, verify=arguments.verify)
    else:
        if arguments.library_root:
            from .library.validation import verify_library
            root=arguments.library_root.resolve()
            header=json.loads((root/'manifest.json').read_bytes())
            records=(root/header['records_root']).resolve()
            receipt=verify_library(root,records.parent/'source')
            source_work=records.parent
        else:
            library_path = arguments.output.parent / 'library/manifest.json'
            receipt = stage_library(arguments.steam_root, arguments.output,
                                    verify=arguments.verify or library_path.is_file())
            source_work=SOURCE_WORK_ROOT
        if arguments.verify:
            verify_reference(arguments.output, receipt)
            from .runtime_package import verify_package, package_id
            manifest = json.loads((arguments.output / 'manifest.json').read_bytes())
            verify_package(manifest, arguments.output.parent / 'runtime' / package_id(manifest))
        else:
            from .library.archive import load_snapshot
            derive_reference(load_snapshot(source_work), receipt, arguments.output)


if __name__ == '__main__':
    main()
