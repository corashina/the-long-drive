import unittest
from pathlib import Path

from tools.reference.native_bc import crunch_split

FILE = Path(__file__).resolve().parents[3] / 'crates/crunch/tests/UNITYCRUNCH_DXT1.crn'


class CrunchSplitTests(unittest.TestCase):
    def test_split_keeps_tables_in_the_start_file_and_level_0_alone(self):
        raw = FILE.read_bytes()
        level0, level1 = (int.from_bytes(raw[o:o + 4], 'big') for o in (70, 74))
        top, start = crunch_split(raw, 10)
        self.assertEqual(top, raw[level0:level1])
        self.assertEqual(start[:level0] + top + start[level0:], raw)
        with self.assertRaises(ValueError):
            crunch_split(raw, 9)
        with self.assertRaises(ValueError):
            crunch_split(raw + b'\0', 10)


if __name__ == '__main__':
    unittest.main()
