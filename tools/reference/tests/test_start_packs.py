import gzip
import tempfile
import unittest
from pathlib import Path

from tools.reference import runtime_package
from tools.reference.runtime_package import build_start_packs, file_receipt, start_files, verify_package


class StartPackTests(unittest.TestCase):
    def test_packs_split_by_entry_bytes_and_round_trip(self):
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp)
            sizes = {'a.bin': 6, 'b.bin': 5, 'c.bin': 12, 'd.bin': 4}
            files = {}
            for index, (name, size) in enumerate(sizes.items()):
                (output / name).write_bytes(bytes([index]) * size)
                files[f'reference/{name}'] = file_receipt(output / name, f'reference/{name}')
            old = runtime_package.PACK_BYTES
            runtime_package.PACK_BYTES = 11
            try:
                packs = build_start_packs(list(files), files, output)
            finally:
                runtime_package.PACK_BYTES = old
            # c.bin is over the limit and stays a single file.
            self.assertEqual([p['files'] for p in packs], [['reference/a.bin', 'reference/b.bin'], ['reference/d.bin']])
            for pack in packs:
                content = gzip.decompress((output / pack['file']).read_bytes())
                self.assertEqual(content, b''.join((output / n.removeprefix('reference/')).read_bytes() for n in pack['files']))
                self.assertEqual(file_receipt(output / pack['file'], pack['file'])['sha256'], pack['sha256'])

    def test_start_files_follow_scene_roots(self):
        manifest = {
            'ui': {'sprites': [{'path': 'reference/ui-a.png'}]},
            'assets': [
                {'usage': 'unique_scene', 'path': 'reference/home.gltf', 'dependencies': [{'file': 'reference/home.bin'}]},
                {'usage': 'world_prototype', 'path': 'reference/tree.gltf', 'dependencies': [{'file': 'reference/tree.bin'}]},
            ],
            'lighting': {'textures': [{'chunks': [{'data': {'path': 'reference/l.smip'}}]}]},
            'render_lighting': {'dependencies': [{'file': 'reference/home.bin'}, {'file': 'reference/dither.bin'}]},
        }
        plans = {'reference/home.gltf': 'reference/home.plan.json', 'reference/tree.gltf': 'reference/tree.plan.json'}
        self.assertEqual(start_files(manifest, plans), [
            'reference/ui-a.png', 'reference/home.plan.json', 'reference/home.gltf', 'reference/home.bin',
            'reference/l.smip', 'reference/dither.bin'])

    def test_verify_package_checks_pack_files(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            (root / 'reference').mkdir()
            (root / 'reference/a.bin').write_bytes(b'abc')
            files = {'reference/a.bin': file_receipt(root / 'reference/a.bin', 'reference/a.bin')}
            (root / 'packs').mkdir()
            (root / 'packs/start-0.gz').write_bytes(gzip.compress(b'abc', mtime=0))
            receipt = file_receipt(root / 'packs/start-0.gz', 'packs/start-0.gz')
            manifest = {'file_catalog': {'files': list(files.values()), 'packs': [
                {'file': 'packs/start-0.gz', 'bytes': receipt['bytes'], 'sha256': receipt['sha256'], 'files': ['reference/a.bin']}]}}
            manifest['content_base'] = f'assets/runtime/{runtime_package.package_id(manifest)}/'
            verify_package(manifest, root)
            (root / 'packs/start-0.gz').write_bytes(gzip.compress(b'abd', mtime=0))
            with self.assertRaisesRegex(ValueError, 'packs/start-0.gz'):
                verify_package(manifest, root)


if __name__ == '__main__':
    unittest.main()
