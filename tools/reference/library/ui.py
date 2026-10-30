"""Index complete source UI records and derive reference canvas coordinates."""
import math
from dataclasses import asdict
from pathlib import Path

from .images import typed_record
from .records import _file, _json, _pointer
from .types import AssetExport, Catalog, DATA_DIRECTORY
from ..source import Source

UI_NAMESPACES = ('UnityEngine.UI', 'TMPro', 'UnityEngine.EventSystems')


def _scale(scaler, viewport):
    mode = scaler.get('m_UiScaleMode', 0)
    if mode == 0:
        return scaler.get('m_ScaleFactor', 1)
    if mode == 2:
        units = (2.54, 25.4, 1, 72, 6)
        return scaler.get('m_FallbackScreenDPI', 96) / units[scaler.get('m_PhysicalUnit', 3)]
    reference = scaler['m_ReferenceResolution']
    width, height = viewport[0] / reference['x'], viewport[1] / reference['y']
    match = scaler.get('m_ScreenMatchMode', 0)
    if match == 1:
        return min(width, height)
    if match == 2:
        return max(width, height)
    weight = scaler.get('m_MatchWidthOrHeight', 0)
    return 2 ** ((1 - weight) * math.log2(width) + weight * math.log2(height))


def convert_layout(record: dict, ancestry: tuple[dict, ...]) -> dict:
    """Convert bottom-origin layout once, using this canvas's scaling context."""
    contexts = [item for item in ancestry if 'canvas' in item]
    context = contexts[-1] if contexts else {}
    canvas, scaler = context.get('canvas', {}), context.get('scaler', {})
    viewport = context.get('viewport')
    if canvas.get('m_RenderMode') == 2 or viewport is None:
        return {'browser': {'space': 'world' if canvas.get('m_RenderMode') == 2 else 'unresolved',
                            'rect': None, 'scale_factor': None, 'y_conversion_count': 0}}
    parents = tuple(item for item in ancestry if 'canvas' not in item)
    if any('m_AnchorMin' not in item for item in parents):
        return {'browser': {'space': 'unresolved', 'rect': None, 'scale_factor': None,
                            'y_conversion_count': 0, 'reason': 'ordinary_transform_ancestry'}}
    scale = _scale(scaler, viewport)
    if scale <= 0:
        raise ValueError('Canvas scale must be positive')
    canvas_size = (viewport[0] / scale, viewport[1] / scale)
    parent = [0, 0, *canvas_size]
    for current in (*parents, record):
        minimum, maximum = current['m_AnchorMin'], current['m_AnchorMax']
        pivot, position, delta = current['m_Pivot'], current['m_AnchoredPosition'], current['m_SizeDelta']
        size = [(maximum[axis] - minimum[axis]) * parent[2 + index] + delta[axis]
                for index, axis in enumerate(('x', 'y'))]
        lower = [parent[index] + (minimum[axis] + (maximum[axis] - minimum[axis]) * pivot[axis]) *
                 parent[2 + index] + position[axis] - pivot[axis] * size[index]
                 for index, axis in enumerate(('x', 'y'))]
        parent = [*lower, *size]
    return {'browser': {'space': 'canvas_reference', 'rect': [parent[0], canvas_size[1] - parent[1] - parent[3],
                         parent[2], parent[3]], 'scale_factor': scale, 'canvas_size': list(canvas_size),
                         'viewport': list(viewport), 'y_conversion_count': 1,
                         'geometry': 'anchor_rect_before_source_rotation_and_scale',
                         'scale_mode': scaler.get('m_UiScaleMode', 0),
                         'screen_match_mode': scaler.get('m_ScreenMatchMode'),
                         'reference_resolution': scaler.get('m_ReferenceResolution')}}


def export_ui(source: Source, catalog: Catalog, output: Path) -> tuple[AssetExport, ...]:
    rects, canvases, owners, ui_components, scalers = {}, {}, {}, {}, {}
    for identity, entry in sorted(catalog.objects.items()):
        if entry.type in ('RectTransform', 'Canvas'):
            reader = source.get(identity.file, identity.path_id)
            data = source.read_record(reader)
            owner = _pointer(source, reader, data['m_GameObject'])
            owners[identity] = owner
            (rects if entry.type == 'RectTransform' else canvases)[identity] = data
    ui_owners = set(owners.values())
    scripts = {}
    for identity, entry in sorted(catalog.objects.items()):
        reader = source.get(identity.file, identity.path_id)
        if entry.type == 'MonoBehaviour':
            head = reader.parse_monobehaviour_head()
            if not head.m_Script.path_id:
                continue
            script_reader = head.m_Script.deref()
            script_key = (source.container_path(script_reader), script_reader.path_id)
            if script_key not in scripts:
                script = script_reader.parse_as_object()
                scripts[script_key] = (script.m_Namespace, script.m_ClassName)
            namespace, name = scripts[script_key]
            owner = _pointer(source, reader, {'m_FileID': head.m_GameObject.file_id,
                                             'm_PathID': head.m_GameObject.path_id})
            if namespace in UI_NAMESPACES or owner in ui_owners:
                owners[identity] = owner
                ui_components[identity] = (name, namespace)
                if name == 'CanvasScaler' and namespace == 'UnityEngine.UI':
                    scalers[owner] = (identity, source.read_record(reader))
        elif entry.type == 'CanvasRenderer':
            data = source.read_record(reader)
            owner = _pointer(source, reader, data['m_GameObject'])
            owners[identity] = owner
            ui_components[identity] = ('CanvasRenderer', 'UnityEngine')
    canvas_by_owner = {owners[identity]: identity for identity in canvases}
    canvas_by_transform = {identity: canvas_by_owner[owners[identity]] for identity in rects
                           if owners[identity] in canvas_by_owner}
    parents = {identity: _pointer(source, source.get(identity.file, identity.path_id), data['m_Father'])
               for identity, data in rects.items()}
    transforms = dict(rects)
    def transform_data(identity):
        if identity not in transforms:
            reader = source.get(identity.file, identity.path_id)
            transforms[identity] = source.read_record(reader)
            parents[identity] = _pointer(source, reader, transforms[identity]['m_Father'])
        return transforms[identity]
    exports = []
    for identity in sorted(set(rects) | set(canvases) | set(ui_components)):
        kind = catalog.objects[identity].type
        owner = owners[identity]
        details = {'type': kind, 'typed_record': typed_record(identity),
                   'game_object': asdict(owner) if owner else None,
                   'game_object_record': typed_record(owner) if owner else None}
        if identity in ui_components:
            details['class'], details['namespace'] = ui_components[identity]
        if identity in canvases:
            details['render_mode'] = canvases[identity]['m_RenderMode']
            if owner in scalers:
                details['scaler'] = asdict(scalers[owner][0])
                details['scaler_record'] = typed_record(scalers[owner][0])
        if identity in rects:
            chain, current, seen = [], identity, set()
            while current is not None and current in catalog.objects and catalog.objects[current].type in ('Transform', 'RectTransform'):
                if current in seen:
                    raise ValueError(f'UI ancestry cycle: {identity}')
                seen.add(current)
                transform_data(current)
                chain.append(current)
                current = parents[current]
            chain.reverse()
            canvas_positions = [index for index, item in enumerate(chain) if item in canvas_by_transform]
            details['ancestry'] = [asdict(item) for item in chain[:-1]]
            details['parent'] = asdict(parents[identity]) if parents[identity] else None
            details['source_layout_record'] = typed_record(identity)
            if canvas_positions:
                position = canvas_positions[-1]
                canvas_id = canvas_by_transform[chain[position]]
                canvas = canvases[canvas_id]
                scaler_id, scaler = scalers.get(owners[canvas_id], (None, {}))
                reference = scaler.get('m_ReferenceResolution')
                viewport = [reference['x'], reference['y']] if reference else [
                    rects[chain[position]]['m_SizeDelta']['x'], rects[chain[position]]['m_SizeDelta']['y']]
                viewport = viewport if min(viewport) > 0 else None
                context = {'canvas': canvas, 'scaler': scaler, 'viewport': viewport}
                # The canvas transform is the reference root, not another child rectangle.
                ancestors = tuple([context, *[transforms[item] for item in chain[position + 1:-1]]])
                details['canvas'] = asdict(canvas_id)
                details['scaler'] = asdict(scaler_id) if scaler_id else None
                if identity == chain[position] and viewport and canvas['m_RenderMode'] != 2:
                    details['browser'] = {'space': 'canvas_reference', 'rect': [0, 0, *viewport],
                                          'y_conversion_count': 0, 'scale_factor': _scale(scaler, viewport)}
                else:
                    details.update(convert_layout(rects[identity], ancestors))
            else:
                details.update(convert_layout(rects[identity], ()))
        exports.append(AssetExport(identity, 'ui', 'native_record', 'engine_dependent',
                                   'Source UI needs a browser layout and event adapter', (),
                                   catalog.dependencies[identity], details))
    languages = []
    language_root = source.root / DATA_DIRECTORY / 'Languages'
    if language_root.exists():
        for path in sorted(language_root.rglob('*')):
            if path.is_file():
                name = f'ui/languages/{path.relative_to(language_root).as_posix()}'
                receipt = _file(output, name, path.read_bytes())
                languages.append({'source_file': path.relative_to(source.root).as_posix(), **asdict(receipt)})
    _file(output, 'ui/languages.json', _json(languages))
    _file(output, 'ui/index.json', _json([asdict(item) for item in exports]))
    return tuple(exports)
