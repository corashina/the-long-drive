import unittest
from types import SimpleNamespace

from tools.reference.appearance_preview import stage_pose


def reader(path_id):
    return SimpleNamespace(assets_file=SimpleNamespace(source_container_path='level1'), path_id=path_id)


class FakeSource:
    def __init__(self, records):
        self.records = records

    def read(self, r):
        return self.records[r.path_id]

    def resolve(self, _, pointer):
        return reader(pointer['m_PathID']) if pointer['m_PathID'] else None


def plain(father, position):
    return {'m_Father': {'m_PathID': father}, 'm_LocalPosition': dict(zip('xyz', position)),
            'm_LocalRotation': {'x': 0., 'y': 0., 'z': 0., 'w': 1.}, 'm_LocalScale': {'x': 1., 'y': 1., 'z': 1.}}


class AppearancePreviewTests(unittest.TestCase):
    def test_rect_transform_takes_its_anchored_position(self):
        # Source preview camera (level1 63057): stale m_LocalPosition y 0, m_AnchoredPosition y 0.851, plain parent.
        camera = plain(1, (0., 0., -1.634))
        camera.update({'m_AnchorMin': {'x': .5, 'y': .5}, 'm_AnchorMax': {'x': .5, 'y': .5}, 'm_Pivot': {'x': .5, 'y': .5},
                       'm_AnchoredPosition': {'x': 0., 'y': .851}, 'm_SizeDelta': {'x': 100., 'y': 100.}})
        source = FakeSource({1: plain(0, (0., 0., 0.)), 2: camera})
        pose = stage_pose(source, reader(2), reader(1))
        for a, b in zip(pose['translation'], (0., .851, 1.634)):
            self.assertAlmostEqual(a, b, places=5)


if __name__ == '__main__':
    unittest.main()
