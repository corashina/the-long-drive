import math
import unittest

from tools.reference.creatures import animator_actions, ray_direction


class CreatureExportTests(unittest.TestCase):
    def test_ray_direction_uses_unity_euler_order(self):
        # Unity +y yaw turns forward to +x. +x pitch turns it down. The port negates z.
        for euler, expected in (({'x': 0, 'y': 90, 'z': 0}, (1., 0., 0.)),
                                ({'x': 90, 'y': 0, 'z': 0}, (0., -1., 0.)),
                                ({'x': 0, 'y': 0, 'z': 70}, (0., 0., -1.))):
            for a, b in zip(ray_direction(euler), expected):
                self.assertAlmostEqual(a, b)
        self.assertAlmostEqual(math.hypot(*ray_direction({'x': 45, 'y': 30, 'z': 0})), 1.)

    def test_rabbit_animator_graph_must_match(self):
        names = {1: 'RunValue', 2: 'Roar', 3: 'Attack', 10: 'Idle,move', 11: 'Roar', 12: 'Jump'}
        values = [{'m_ID': 1, 'm_Type': 1}, {'m_ID': 2, 'm_Type': 9}, {'m_ID': 3, 'm_Type': 9}]
        states = [{'data': {'m_NameID': n}} for n in (10, 11, 12)]
        c = {'m_Values': {'data': {'m_ValueArray': values}},
             'm_StateMachineArray': [{'data': {'m_StateConstantArray': states, 'm_DefaultState': 0}}]}
        with self.assertRaisesRegex(ValueError, 'states differ'):
            animator_actions(None, None, c, names, [], None)


if __name__ == '__main__':
    unittest.main()
