"""Collector ordering regressions; fixtures are not native/review evidence."""
import hashlib
import importlib.util
import json
from pathlib import Path, PurePosixPath, PureWindowsPath
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location(
    'dependency_notice_ordering', Path(__file__).resolve().parents[1] / 'collect-dependency-notices.py')
collector = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(collector)


class NoticeOrderingTests(unittest.TestCase):
    NAMES = [
        'fonts/FiraMono-LICENSE', 'LICENSE-MIT', 'LICENSE-APACHE',
        'third-party-notices/STANFORD-NOTICE.txt',
        'third-party-notices/libjpeg-turbo-LICENSE.ijg',
        'third-party-notices/libjpeg-turbo-acknowledgement-NOTICE.txt',
        'third-party-notices/stb-MIT-NOTICE.txt',
    ]

    def ordered(self, cls, root):
        source = cls(root)
        paths = [source / name for name in reversed(self.NAMES)]
        return [p.relative_to(source).as_posix() for p in sorted(
            paths, key=lambda path: collector.source_notice_order(path, source))]

    def test_actual_host_flavors_reproduce_old_order_difference(self):
        posix = [PurePosixPath(x) for x in self.NAMES]
        windows = [PureWindowsPath(x) for x in self.NAMES]
        self.assertNotEqual([p.as_posix() for p in sorted(posix)],
                            [p.as_posix() for p in sorted(windows)])

    def test_posix_and_windows_use_same_exact_relative_order(self):
        expected = sorted(self.NAMES)
        self.assertEqual(self.ordered(PurePosixPath, '/source/pkg'), expected)
        self.assertEqual(self.ordered(PureWindowsPath, r'C:\source\pkg'), expected)

    def test_order_does_not_depend_on_checkout_prefix(self):
        self.assertEqual(self.ordered(PurePosixPath, '/a'),
                         self.ordered(PurePosixPath, '/different/Z/root'))
        self.assertEqual(self.ordered(PureWindowsPath, r'C:\A'),
                         self.ordered(PureWindowsPath, r'D:\different\root'))

    def test_sort_key_preserves_case_and_spelling(self):
        root = PureWindowsPath(r'C:\source')
        self.assertEqual(collector.source_notice_order(root / 'X/NOTICE.txt', root), 'X/NOTICE.txt')
        self.assertNotEqual(collector.source_notice_order(root / 'X/NOTICE.txt', root),
                            collector.source_notice_order(root / 'x/notice.txt', root))

    def test_original_inputs_and_existing_outputs_are_not_rewritten(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); repo = root / 'repo'; package = repo / 'vendor/pkg'
            package.mkdir(parents=True)
            for name in self.NAMES:
                path = package / name; path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(('Exact original notice: ' + name + '\r\n').encode())
            for name in ('LICENSE-MIT', 'LICENSE-APACHE'):
                (repo / name).write_bytes((name + '\r\n').encode())
            (repo / 'Cargo.lock').write_text('version = 4\n[[package]]\nname="app"\nversion="1.0.0"\n[[package]]\nname="pkg"\nversion="1.0.0"\n')
            (repo / 'docs/release').mkdir(parents=True)
            (repo / 'docs/release/asset-rights-manifest.json').write_text('{"dependency_assets": []}')
            packages = [
                {'id': 'app', 'name': 'app', 'version': '1.0.0', 'manifest_path': str(repo / 'Cargo.toml'), 'license': 'MIT'},
                {'id': 'pkg', 'name': 'pkg', 'version': '1.0.0', 'manifest_path': str(package / 'Cargo.toml'), 'license': 'MIT'},
            ]
            metadata = {'workspace_root': str(repo), 'workspace_members': ['app'], 'packages': packages,
                        'resolve': {'nodes': [
                            {'id': 'app', 'features': [], 'deps': [{'pkg': 'pkg', 'dep_kinds': [{'kind': None}]}]},
                            {'id': 'pkg', 'features': [], 'deps': []}]}}
            metadata_path = root / 'metadata.json'; metadata_path.write_text(json.dumps(metadata))
            before = {p.relative_to(repo).as_posix(): p.read_bytes() for p in repo.rglob('*') if p.is_file()}
            output = root / 'notices'
            original_write_text = Path.write_text
            def windows_default_write_text(path, data, *args, **kwargs):
                # Emulate the Windows default only when production omits an
                # explicit newline. Exact upstream notices use write_bytes.
                kwargs.setdefault('newline', '\r\n')
                return original_write_text(path, data, *args, **kwargs)
            with patch.object(Path, 'write_text', windows_default_write_text):
                inventory = collector.collect(metadata_path, repo, output, 'x86_64-pc-windows-msvc', 'app')
            for name in ('README.txt', 'dependency-inventory.json'):
                self.assertNotIn(b'\r\n', (output / name).read_bytes())
                self.assertIn(b'\n', (output / name).read_bytes())
            record = next(p for p in inventory['packages'] if p['name'] == 'pkg')
            self.assertEqual([n['upstream_path'] for n in record['notices']], sorted(self.NAMES))
            for notice in record['notices']:
                original = (package / notice['upstream_path']).read_bytes()
                self.assertEqual((output / notice['path']).read_bytes(), original)
                self.assertEqual(notice['sha256'], hashlib.sha256(original).hexdigest())
                self.assertEqual(notice['bytes'], len(original))
            self.assertEqual(inventory['review_status'], 'not_reviewed')
            self.assertEqual(before, {p.relative_to(repo).as_posix(): p.read_bytes() for p in repo.rglob('*') if p.is_file()})
            original_output = (output / 'dependency-inventory.json').read_bytes()
            with self.assertRaisesRegex(ValueError, 'absent or empty'):
                collector.collect(metadata_path, repo, output, 'x86_64-pc-windows-msvc', 'app')
            self.assertEqual((output / 'dependency-inventory.json').read_bytes(), original_output)


if __name__ == '__main__':
    unittest.main()
