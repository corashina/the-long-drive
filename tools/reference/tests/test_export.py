import math
import struct
import tempfile
import unittest
from pathlib import Path

from tools.reference.export import validate_build
from tools.reference.gltf import convert_point, convert_rotation, convert_triangles, transform_point
from tools.reference.source import normalize_arrays
from tools.reference.collision import bake_points

class ReferenceExportTests(unittest.TestCase):
    def test_build_and_conversion(self):
        cache = Path('.cache/reference-test')
        cache.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(dir=cache) as directory:
            root = Path(directory) / 'steamapps' / 'common' / 'The Long Drive'
            root.mkdir(parents=True)
            manifest = root.parents[1] / 'appmanifest_1017180.acf'
            manifest.write_text('"AppState" { "appid" "1017180" "buildid" "11140890" }')
            validate_build(root)
            manifest.write_text('"AppState" { "appid" "1017180" "buildid" "11140891" }')
            with self.assertRaisesRegex(ValueError, 'build'):
                validate_build(root)
        self.assertEqual(convert_point((0.0, 0.0, 1.0)), (0.0, 0.0, -1.0))
        self.assertEqual(convert_point((100.0, 0.0, 0.0)), (100.0, 0.0, 0.0))
        rotation = (0.0, math.sin(0.4), 0.0, math.cos(0.4))
        translation = (10.0, 2.0, 5.0)
        for scale in ((100.0, 100.0, 100.0), (-1.0, 2.0, 1.0)):
            visual = (0.25, 0.5, 1.0)
            expected = convert_point(transform_point(visual, translation, rotation, scale))
            actual = transform_point(convert_point(visual), convert_point(translation), convert_rotation(rotation), scale)
            for a, b in zip(actual, expected):
                self.assertAlmostEqual(a, b)
            chain=[{'m_LocalPosition':dict(zip(('x','y','z'),translation)),
                'm_LocalRotation':dict(zip(('x','y','z','w'),rotation)),
                'm_LocalScale':dict(zip(('x','y','z'),scale))}]
            collider=bake_points([visual],chain,{'translation':[0.,0.,0.],'rotation':[0.,0.,0.,1.]})[0]
            for a,b in zip(actual,collider):self.assertAlmostEqual(a,b)

    def test_normals_and_collision_use_one_conversion(self):
        points = [(0., 0., 0.), (1., 0., 0.), (0., 1., 0.)]
        converted = list(map(convert_point, points))
        a, b, c = [converted[i] for i in convert_triangles([(0, 1, 2)])[0]]
        u = [b[i] - a[i] for i in range(3)]
        v = [c[i] - a[i] for i in range(3)]
        normal = (u[1]*v[2]-u[2]*v[1], u[2]*v[0]-u[0]*v[2], u[0]*v[1]-u[1]*v[0])
        self.assertEqual(normal, convert_point((0., 0., 1.)))
        self.assertEqual(converted, [convert_point(p) for p in points])

if __name__ == '__main__':
    unittest.main()
