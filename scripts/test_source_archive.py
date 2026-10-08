#!/usr/bin/env python3
import importlib.util
import io
import os
from pathlib import Path
import stat
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import warnings
import zipfile

spec = importlib.util.spec_from_file_location('archive_policy', Path(__file__).with_name('check-source-archive.py'))
policy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(policy)

def zipped(entries):
    stream = io.BytesIO()
    with zipfile.ZipFile(stream, 'w') as archive:
        with warnings.catch_warnings():
            warnings.simplefilter('ignore', UserWarning)
            for name, data in entries:
                archive.writestr(name, data)
    return stream.getvalue()

class ArchivePolicyTests(unittest.TestCase):
    def test_zip_root_is_normalized(self):
        self.assertEqual(policy.normalized_members(zipped([('any-root/LICENSE', b'x')])), {'LICENSE': b'x'})

    def test_reject_unsafe_paths(self):
        for name in ('root/../outside', '/root/a', 'root//a', 'root/./a', 'root\\a', 'C:/a'):
            with self.subTest(name=name), self.assertRaises(ValueError):
                policy.normalized_members(zipped([(name, b'x')]))

    def test_reject_duplicate(self):
        with self.assertRaises(ValueError):
            policy.normalized_members(zipped([('r/a', b'1'), ('r/a', b'2')]))

    def test_reject_multiple_roots(self):
        with self.assertRaises(ValueError):
            policy.normalized_members(zipped([('a/x', b'1'), ('b/x', b'2')]))

    def test_reject_flat_archive(self):
        with self.assertRaises(ValueError):
            policy.normalized_members(zipped([('a', b'1')]))

    def test_reject_empty(self):
        with self.assertRaises(ValueError):
            policy.normalized_members(zipped([]))

    def test_reject_zip_symlink(self):
        stream = io.BytesIO()
        with zipfile.ZipFile(stream, 'w') as archive:
            info = zipfile.ZipInfo('root/link')
            info.create_system = 3
            info.external_attr = (stat.S_IFLNK | 0o777) << 16
            archive.writestr(info, b'target')
        with self.assertRaises(ValueError):
            policy.normalized_members(stream.getvalue())

    def test_reject_nonempty_directory(self):
        with self.assertRaises(ValueError):
            policy.normalized_members(zipped([('root/', b'hidden'), ('root/file', b'ok')]))

    def test_reject_directory_named_symlink(self):
        stream = io.BytesIO()
        with zipfile.ZipFile(stream, 'w') as archive:
            info = zipfile.ZipInfo('root/link/')
            info.create_system = 3
            info.external_attr = (stat.S_IFLNK | 0o777) << 16
            archive.writestr(info, b'')
            archive.writestr('root/file', b'ok')
        with self.assertRaises(ValueError):
            policy.normalized_members(stream.getvalue())

    def test_reject_tar_hardlink(self):
        stream = io.BytesIO()
        with tarfile.open(fileobj=stream, mode='w') as archive:
            info = tarfile.TarInfo('root/link')
            info.type = tarfile.LNKTYPE
            info.linkname = 'root/a'
            archive.addfile(info)
        with self.assertRaises(ValueError):
            policy.normalized_members(stream.getvalue())

    def test_size_limit(self):
        old = policy.MAX_TOTAL
        try:
            policy.MAX_TOTAL = 3
            with self.assertRaises(ValueError):
                policy.read_members(zipped([('a', b'12'), ('b', b'34')]))
        finally:
            policy.MAX_TOTAL = old

    def test_nested_identical_denied_bytes(self):
        old = policy.DENIED_SHA256
        try:
            policy.DENIED_SHA256 = policy.sha(b'denied')
            with self.assertRaises(ValueError):
                policy.reject_identical_model({'copy.zip': zipped([('renamed.dat', b'denied')])})
        finally:
            policy.DENIED_SHA256 = old

    def test_nested_depth_limit(self):
        payload = b'leaf'
        for _ in range(5):
            payload = zipped([('nested.zip', payload)])
        with self.assertRaises(ValueError):
            policy.reject_identical_model({'nested.zip': payload})

    def test_ambient_attributes_cannot_change_expected_zip_or_tar(self):
        for override in ('info', 'local-file', 'global-file', 'environment'):
            with self.subTest(override=override), tempfile.TemporaryDirectory() as tmp:
                repo = Path(tmp) / 'repo'
                repo.mkdir()
                def git(*args):
                    return subprocess.check_output(['git', '-C', str(repo), *args], stderr=subprocess.DEVNULL)
                git('init')
                for name in policy.REQUIRED + (policy.DENIED_PATH, 'vendor/example/NOTICE'):
                    path = repo / name
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_bytes(b'fixture')
                (repo / '.gitattributes').write_text('/' + policy.DENIED_PATH + ' export-ignore\n')
                git('add', '.')
                git('-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '-m', 'fixture')
                baseline = policy.verify(repo, 'HEAD')['member_count']
                attrs = Path(tmp) / 'untracked-attributes'
                attrs.write_text('vendor/** export-ignore\n')
                environment = {}
                if override == 'info':
                    (repo / '.git/info/attributes').write_bytes(attrs.read_bytes())
                elif override == 'local-file':
                    git('config', 'core.attributesFile', str(attrs))
                elif override == 'environment':
                    environment.update(GIT_CONFIG_COUNT='1', GIT_CONFIG_KEY_0='core.attributesFile', GIT_CONFIG_VALUE_0=str(attrs))
                else:
                    config = Path(tmp) / 'global-config'
                    git('config', '--file', str(config), 'core.attributesFile', str(attrs))
                    environment['GIT_CONFIG_GLOBAL'] = str(config)
                with patch.dict(os.environ, environment):
                    for fmt in ('zip', 'tar'):
                        checked = policy.verify(repo, 'HEAD', fmt=fmt)
                        self.assertEqual(checked['member_count'], baseline)
                        tampered = git('archive', '--format=' + fmt, '--prefix=hosted/', 'HEAD')
                        self.assertNotIn('vendor/example/NOTICE', policy.normalized_members(tampered))
                        with self.assertRaises(ValueError):
                            policy.verify(repo, 'HEAD', tampered, fmt)
                self.assertEqual(git('status', '--porcelain'), b'')

    def test_exact_commit_export_both_formats(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp)
            def git(*args):
                return subprocess.check_output(['git', '-C', str(repo), *args], stderr=subprocess.DEVNULL)
            git('init')
            for name in policy.REQUIRED + (policy.DENIED_PATH,):
                path = repo / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b'fixture')
            (repo / '.gitattributes').write_text('/' + policy.DENIED_PATH + ' export-ignore\n')
            git('add', '.')
            git('-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '-m', 'fixture')
            for fmt in ('zip', 'tar'):
                result = policy.verify(repo, 'HEAD', fmt=fmt)
                self.assertFalse(result['publication_authorized'])
                self.assertFalse(result['hosted_origin_attested'])
                supplied = git('archive', '--format=' + fmt, '--prefix=hosted-name/', 'HEAD')
                policy.verify(repo, 'HEAD', supplied, fmt)
            bad = zipped([('root/' + name, b'wrong') for name in policy.REQUIRED])
            with self.assertRaises(ValueError):
                policy.verify(repo, 'HEAD', bad)
            (repo / '.gitattributes').write_text('')
            git('add', '.')
            git('-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '-m', 'bad-policy')
            with self.assertRaises(ValueError):
                policy.verify(repo, 'HEAD')

if __name__ == '__main__':
    unittest.main()
