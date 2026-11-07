import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace as NS
from unittest.mock import patch
from tools.reference.export import FAILED_RUNTIME, derive_reference, retry_package


class ExportStagingTests(unittest.TestCase):
    def test_failed_export_cleans_or_keeps_only_its_stage(self):
        for failure in ('export', 'verify', 'promote'):
            with self.subTest(failure=failure), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp).resolve()
                output = root / 'runtime' / 'reference'
                output.mkdir(parents=True)
                accepted = output / 'accepted'
                accepted.write_bytes(b'accepted')
                recovery = output.parent / 'runtime-existing-recovery'
                recovery.mkdir()
                (recovery / 'evidence').write_bytes(b'keep')
                snapshot = NS(manifest_sha256='a'*64, files=(), root=root/'source', manifest_path=root/'source-cache'/'manifest.json')
                receipt = NS(source_manifest_sha256='a'*64)
                reader = NS(snapshot=snapshot, root=root/'library', exports={})
                stages = []
                def selected(source, staged, library, selected_snapshot):
                    stages.append(staged)
                    (staged/'new').write_bytes(b'new')
                    if failure == 'export': raise ValueError('injected export')
                def verify(*args):
                    if failure == 'verify': raise ValueError('injected verify')
                promoted = []
                def promote(staged, accepted):
                    if failure == 'promote': raise ValueError('injected promote')
                    promoted.append(staged)
                before = accepted.stat().st_mtime_ns
                with patch('tools.reference.library.reader.LibraryReader', return_value=reader), patch('tools.reference.source.LibrarySource'), patch('tools.reference.library.storage.check_space'), patch('tools.reference.export._export_selected', side_effect=selected), patch('tools.reference.export.verify_reference', side_effect=verify), patch('tools.reference.runtime_package.publish_package'), patch('tools.reference.runtime_package.write_runtime_manifest'), patch('tools.reference.export.promote_reference', side_effect=promote):
                    with self.assertRaisesRegex(ValueError, 'injected '+failure):
                        derive_reference(snapshot, receipt, output)
                    failed = output.parent / FAILED_RUNTIME
                    self.assertEqual(failed.is_dir(), failure == 'verify')
                    if failure == 'verify':
                        self.assertEqual((failed/'new').read_bytes(), b'new')
                        (failed/'manifest.json.gz').write_bytes(b'')
                        failure = 'none'
                        retry_package(receipt, output)
                        self.assertEqual(promoted, [failed])
                self.assertEqual(accepted.read_bytes(), b'accepted')
                self.assertEqual(accepted.stat().st_mtime_ns, before)
                self.assertEqual((recovery/'evidence').read_bytes(), b'keep')
                self.assertEqual(stages[0].exists(), failure == 'promote')
                if failure == 'promote': self.assertEqual((stages[0]/'new').read_bytes(), b'new')


class PrunePackagesTests(unittest.TestCase):
    def test_keeps_current_and_newest_older_package(self):
        import os
        from tools.reference.runtime_package import prune_packages
        with tempfile.TemporaryDirectory() as tmp:
            runtime = Path(tmp)
            names = [c * 64 for c in 'abcd']
            for age, name in enumerate(names):
                (runtime / name).mkdir()
                os.utime(runtime / name, (1000 - age, 1000 - age))
            (runtime / '.package-pending').mkdir()
            prune_packages(runtime / names[2])
            self.assertEqual(sorted(p.name for p in runtime.iterdir()), sorted(['.package-pending', names[0], names[2]]))


if __name__ == '__main__':
    unittest.main()
