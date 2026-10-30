"""Recover the checked four-slot and right-hand source contract."""
import re
from .source import source_id
from .survival import managed_evidence

INPUTS = [
    ('equiprecenter', 19, 'Tab'), ('removeSunglasses', 30, 'X'),
    ('inventory1', 31, 'Alpha1'), ('inventory2', 32, 'Alpha2'),
    ('inventory3', 33, 'Alpha3'), ('inventory4', 34, 'Alpha4'),
    ('smoke', 29, 'CapsLock'),
]
EVIDENCE = [
    'private float rightHandLerp = 20f;',
    'private float rightHandTime = 1f;',
    'public static float holdButtonTime = 0.25f;',
    'inventorySwapValue += inventorySwapGain * dt;',
]


def recover_inventory(source, manifest):
    player = source.get('level1', 80125)
    fields = source.read(player)
    if source.class_name(player) != 'fpscontroller':
        raise ValueError('Inventory player must be fpscontroller')
    evidence = managed_evidence(source, EVIDENCE)
    code = (source.library.records_root / 'managed/Assembly-CSharp.cs').read_text(encoding='utf-8-sig')
    enum = code[code.index('public enum IN\n'):]
    actions = [entry.strip() for entry in enum[enum.index('{') + 1:enum.index('}')].split(',') if entry.strip()]
    inputs = []
    for _, index, _ in INPUTS:
        match = re.search(r'defaultInput\[' + str(index) + r'\]\.keys\[0\]\.Set\(KeyCode\.(\w+)\);', code)
        if not match:
            raise ValueError(f'Missing inventory input {index}')
        inputs.append({'action': actions[index], 'index': index, 'key': match.group(1)})
    hand = source.resolve(player, fields['RH'])
    if hand is None or hand.type.name != 'Transform':
        raise ValueError('Inventory right-hand anchor must be Transform')
    result = {
        'source': source_id(player),
        'slot_count': len(fields['inventory']),
        'initial_slots': [None if entry['m_PathID'] == 0 else source_id(source.resolve(player, entry)) for entry in fields['inventory']],
        'selected_slot': fields['selectedInv'],
        'swap_gain': fields['inventorySwapGain'],
        'starter_groups': [bool(fields[f'startWithItems{i}']) for i in (1, 2, 3)],
        'equip_hold_seconds': .25,
        'hand_interpolation_factor': 20.,
        'hand_interpolation_seconds': 1.,
        'right_hand_anchor': source_id(hand),
        'inputs': inputs,
        'evidence': evidence,
    }
    validate_inventory(result, manifest)
    return result


def validate_inventory(record, manifest):
    expected = {
        'source': {'file': 'level1', 'path_id': 80125},
        'slot_count': 4,
        'initial_slots': [None] * 4,
        'selected_slot': 0,
        'swap_gain': 5.,
        'starter_groups': [False] * 3,
        'equip_hold_seconds': .25,
        'hand_interpolation_factor': 20.,
        'hand_interpolation_seconds': 1.,
        'right_hand_anchor': {'file': 'level1', 'path_id': 23118},
    }
    if not isinstance(record, dict) or any(record.get(field) != value for field, value in expected.items()):
        raise ValueError('Checked inventory startup contract differs')
    inputs = [{'action': action, 'index': index, 'key': key} for action, index, key in INPUTS]
    if record.get('inputs') != inputs:
        raise ValueError('Checked inventory input contract differs')
    evidence = record.get('evidence')
    if evidence != {
        'assembly_sha256': '9919a9eeddcce23bb4ff86e890437f32b4a3ef3c10af2efe3b8d889d894dd62e',
        'methods': EVIDENCE,
    }:
        raise ValueError('Checked inventory managed evidence differs')
