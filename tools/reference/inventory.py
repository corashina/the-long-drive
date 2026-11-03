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
    return result

