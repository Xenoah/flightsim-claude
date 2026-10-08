"""Offline transport/identity rejection tests; fixtures are never hosted evidence."""
import gzip
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import urllib.error

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('hosted', Path(__file__).with_name('check-hosted-source-archives.py'))
hosted = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(hosted)
PUBLIC = {'id': 1318879072, 'full_name': 'Xenoah/flightsim-claude',
          'private': False, 'visibility': 'public'}


class Response(io.BytesIO):
    def __init__(self, body=b'abc', url='https://api.github.com/example', headers=None, status=200):
        super().__init__(body)
        self.url, self.headers, self.status = url, headers or {}, status

    def geturl(self):
        return self.url


class TransportTests(unittest.TestCase):
    def fetch(self, response, limit=3):
        with patch.object(hosted.urllib.request, 'build_opener') as create:
            create.return_value.open.return_value = response
            return hosted.fetch('https://api.github.com/example', limit)

    def test_exact_limit_and_missing_length_are_allowed(self):
        self.assertEqual(self.fetch(Response()), (b'abc', 'https://api.github.com/example'))

    def test_rejects_oversized_undeclared_empty_and_truncated_bodies(self):
        for response in (Response(b'abcd'), Response(b''),
                         Response(headers={'Content-Length': '4'}),
                         Response(b'a', headers={'Content-Length': '3'}),
                         Response(headers={'Content-Length': '-1'}),
                         Response(headers={'Content-Encoding': 'gzip'}),
                         Response(url='https://evil.example/'), Response(status=206)):
            with self.subTest(response=response), self.assertRaises(ValueError):
                self.fetch(response)

    def test_checks_deadline_after_a_slow_read(self):
        with patch.object(hosted.time, 'monotonic', side_effect=[0, 0, 121]):
            with self.assertRaisesRegex(ValueError, 'time limit'):
                self.fetch(Response())

    def test_redirect_is_exact_and_never_forwards_credentials(self):
        source = 'https://github.com/Xenoah/flightsim-claude/archive/' + 'a' * 40 + '.zip'
        target = 'https://codeload.github.com/Xenoah/flightsim-claude/zip/' + 'a' * 40
        redirect = urllib.error.HTTPError(source, 302, 'Found', {'Location': target}, io.BytesIO())
        with patch.object(hosted.urllib.request, 'build_opener') as create:
            create.return_value.open.side_effect = [redirect, Response(url=target)]
            self.assertEqual(hosted.fetch(source, 3, target), (b'abc', target))
            for call in create.return_value.open.call_args_list:
                self.assertFalse(call.args[0].has_header('Authorization'))
            self.assertEqual(create.call_args.args[0].proxies, {})

    def test_rejects_redirect_changes_before_contacting_target(self):
        good = 'https://codeload.github.com/Xenoah/flightsim-claude/zip/' + 'a' * 40
        for target in (good + '?token=secret', good + '#fragment', good.replace('https:', 'http:'),
                       good.replace('codeload.github.com', 'evil.example'), good[:-1] + 'b'):
            error = urllib.error.HTTPError('https://github.com/example', 302, 'Found',
                                          {'Location': target}, io.BytesIO())
            with self.subTest(target=target), patch.object(hosted.urllib.request, 'build_opener') as create:
                create.return_value.open.side_effect = error
                with self.assertRaises(ValueError):
                    hosted.fetch('https://github.com/example', 3, good)
                self.assertEqual(create.return_value.open.call_count, 1)

    def test_metadata_rejects_array_and_oversize(self):
        with patch.object(hosted, 'fetch', return_value=(b'[]', 'unused')) as fetch:
            with self.assertRaises(ValueError):
                hosted.metadata()
            self.assertEqual(fetch.call_args.args[1], 1024 * 1024)


class HostedArchiveTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temporary = tempfile.TemporaryDirectory()
        cls.repo = Path(cls.temporary.name)
        cls.git('init', '--quiet')
        required = ('LICENSE-MIT', 'LICENSE-APACHE', 'ATTRIBUTION.md',
                    'docs/release/asset-rights-manifest.json',
                    'assets/aircraft/swift_sport.glb', 'assets/aircraft/swift_sport.json',
                    'assets/aircraft/light_single.glb')
        for name in required:
            path = cls.repo / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b'synthetic fixture')
        (cls.repo / 'scripts').mkdir()
        (cls.repo / 'scripts/check-source-archive.py').write_bytes((ROOT / 'scripts/check-source-archive.py').read_bytes())
        (cls.repo / '.gitattributes').write_text('/assets/aircraft/light_single.glb export-ignore\n')
        cls.git('add', '.')
        cls.git('-c', 'user.name=Test', '-c', 'user.email=test@example.invalid', 'commit', '--quiet', '-m', 'fixture')
        cls.commit = cls.git('rev-parse', 'HEAD').decode().strip()
        cls.tree = cls.git('rev-parse', 'HEAD^{tree}').decode().strip()
        cls.archives = {'.zip': cls.git('archive', '--format=zip', '--prefix=hosted/', 'HEAD'),
                        '.tar.gz': gzip.compress(cls.git('archive', '--format=tar', '--prefix=hosted/', 'HEAD'))}

    @classmethod
    def tearDownClass(cls):
        cls.temporary.cleanup()

    @classmethod
    def git(cls, *args):
        return subprocess.check_output(['git', '-C', str(cls.repo), *args])

    def metadata(self, path=''):
        if not path:
            return dict(PUBLIC)
        if path == '/git/commits/' + self.commit:
            return {'sha': self.commit, 'tree': {'sha': self.tree}}
        if path == '/git/ref/tags/v1.2.3':
            return {'ref': 'refs/tags/v1.2.3', 'object': {'type': 'commit', 'sha': self.commit}}
        self.fail('unexpected metadata request: ' + path)

    def fetch(self, url, limit, redirect):
        self.assertEqual(limit, 64 * 1024 * 1024)
        extension = '.zip' if url.endswith('.zip') else '.tar.gz'
        return self.archives[extension], redirect

    def verify(self, tag=''):
        with patch.object(hosted, 'metadata', side_effect=self.metadata), patch.object(hosted, 'fetch', side_effect=self.fetch):
            return hosted.verify(self.repo, self.commit, tag)

    def test_commit_and_existing_tag_both_formats_use_real_checker(self):
        result = self.verify('v1.2.3')
        self.assertEqual(len(result['archives']), 4)
        self.assertEqual(result['commit'], self.commit)
        self.assertEqual(result['tree'], self.tree)
        self.assertEqual(result['tag_object_chain'], [self.commit])
        self.assertEqual({r['format'] for r in result['archives']}, {'zip', 'tar.gz'})
        self.assertEqual(len({r['members_sha256'] for r in result['archives']}), 1)
        self.assertFalse(result['publication_authorized'])
        self.assertLess(len(json.dumps(result, indent=2).encode()), 16 * 1024)

    def test_public_visibility_identity_and_commit_tree_are_required(self):
        for wrong in ({**PUBLIC, 'private': True}, {**PUBLIC, 'visibility': 'private'},
                      {**PUBLIC, 'id': 3}, {**PUBLIC, 'full_name': 'Other/repo'},
                      {'sha': self.commit, 'tree': {'sha': 'a' * 40}}):
            responses = [wrong] if 'id' in wrong else [PUBLIC, wrong]
            with self.subTest(wrong=wrong), patch.object(hosted, 'metadata', side_effect=responses), patch.object(hosted, 'fetch') as download:
                with self.assertRaises(ValueError):
                    hosted.verify(self.repo, self.commit)
                download.assert_not_called()

    def test_wrong_archive_bytes_and_wrong_formats_are_rejected(self):
        for payload in (b'<html>error</html>', self.archives['.tar.gz'],
                        self.archives['.zip'][:-20]):
            with self.subTest(payload=payload[:8]), patch.object(hosted, 'metadata', side_effect=self.metadata), patch.object(hosted, 'fetch', return_value=(payload, 'unused')):
                with self.assertRaises(Exception):
                    hosted.verify(self.repo, self.commit)

    def test_tag_requires_exact_ref_and_commit_not_same_named_branch(self):
        for value in ({'ref': 'refs/heads/v1.2.3', 'object': {'type': 'commit', 'sha': self.commit}},
                      {'ref': 'refs/tags/v1.2.3', 'object': {'type': 'commit', 'sha': 'a' * 40}},
                      {'ref': 'refs/tags/v1.2.3', 'object': {'type': 'tree', 'sha': self.tree}}):
            with self.subTest(value=value), patch.object(hosted, 'metadata', return_value=value), self.assertRaises(ValueError):
                hosted.resolve_tag('v1.2.3', self.commit)

    def test_annotated_tag_identity_and_depth(self):
        root = {'ref': 'refs/tags/v1.2.3', 'object': {'type': 'tag', 'sha': 'a' * 40}}
        value = {'sha': 'a' * 40, 'object': {'type': 'commit', 'sha': self.commit}}
        with patch.object(hosted, 'metadata', side_effect=[root, value]):
            self.assertEqual(hosted.resolve_tag('v1.2.3', self.commit), ['a' * 40, self.commit])
        with patch.object(hosted, 'metadata', side_effect=[root, {**value, 'sha': 'b' * 40}]), self.assertRaises(ValueError):
            hosted.resolve_tag('v1.2.3', self.commit)
        cycle = {'sha': 'a' * 40, 'object': root['object']}
        with patch.object(hosted, 'metadata', side_effect=[root] + [cycle] * 5), self.assertRaisesRegex(ValueError, 'depth'):
            hosted.resolve_tag('v1.2.3', self.commit)

    def test_tag_object_change_during_download_fails_even_at_same_commit(self):
        with patch.object(hosted, 'resolve_tag', side_effect=[[self.commit], ['a' * 40, self.commit]]):
            with self.assertRaisesRegex(ValueError, 'tag changed'):
                self.verify('v1.2.3')

    def test_rejects_unbounded_or_ambiguous_input_before_network(self):
        for commit, tag in (('main', ''), (self.commit, '../main'), (self.commit, 'v1?token=x'), (self.commit, 'v1\n')):
            with self.subTest(commit=commit, tag=tag), patch.object(hosted, 'metadata') as read:
                with self.assertRaises(ValueError):
                    hosted.verify(self.repo, commit, tag)
                read.assert_not_called()

    def test_failure_never_creates_success_receipt(self):
        with tempfile.TemporaryDirectory() as tmp:
            report = Path(tmp) / 'receipt.json'
            target = {'commit': self.commit, 'tree': self.tree, 'tag': ''}
            with patch('sys.argv', ['check', '--evidence-commit', self.commit, '--report', str(report)]), patch.object(hosted, 'evidence_request', return_value=(target, {})), patch.object(hosted, 'verify', side_effect=ValueError('fixture')):
                with self.assertRaises(ValueError):
                    hosted.main()
            self.assertFalse(report.exists())

    def test_wrong_reviewed_tree_fails_before_network(self):
        with patch.object(hosted, 'metadata') as read:
            with self.assertRaisesRegex(ValueError, 'reviewed target'):
                hosted.verify(self.repo, self.commit, expected_tree='f' * 40)
            read.assert_not_called()

    def test_rejects_changed_committed_checker_before_import_or_network(self):
        with tempfile.TemporaryDirectory() as tmp:
            repo = Path(tmp) / 'source'
            subprocess.check_call(['git', 'clone', '--quiet', '--no-hardlinks', str(self.repo), str(repo)])
            checker = repo / 'scripts/check-source-archive.py'
            checker.write_text("raise RuntimeError('unreviewed checker executed')\n")
            subprocess.check_call(['git', '-C', str(repo), 'add', 'scripts/check-source-archive.py'])
            subprocess.check_call(['git', '-C', str(repo), '-c', 'user.name=Test', '-c',
                                   'user.email=test@example.invalid', 'commit', '--quiet', '-m', 'changed checker'])
            commit = subprocess.check_output(['git', '-C', str(repo), 'rev-parse', 'HEAD'], text=True).strip()
            with patch.object(hosted, 'metadata') as read:
                with self.assertRaisesRegex(ValueError, 'differs from reviewed checker'):
                    hosted.verify(repo, commit)
                read.assert_not_called()


class EvidenceBindingTests(unittest.TestCase):
    def test_binds_distinct_source_target_and_evidence_files_and_rejects_edits(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            def git(*args):
                return subprocess.check_output(['git', '-C', str(root), *args])
            git('init', '--quiet')
            target = {'schema_version': 1, 'repository': 'Xenoah/flightsim-claude',
                      'commit': 'a' * 40, 'tree': 'b' * 40, 'tag': ''}
            for name in hosted.EVIDENCE_FILES:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(json.dumps(target) if name.endswith('.json') else 'reviewed fixture\n')
            git('add', '.')
            git('-c', 'user.name=Test', '-c', 'user.email=test@example.invalid',
                'commit', '--quiet', '-m', 'evidence fixture')
            head = git('rev-parse', 'HEAD').decode().strip()
            actual, evidence = hosted.evidence_request(root, head)
            self.assertEqual(actual, target)
            self.assertEqual(evidence['commit'], head)
            self.assertNotEqual(actual['commit'], head)
            self.assertEqual(set(evidence['files_sha256']), set(hosted.EVIDENCE_FILES))
            with self.assertRaisesRegex(ValueError, 'checkout differs'):
                hosted.evidence_request(root, 'f' * 40)
            for name in hosted.EVIDENCE_FILES:
                path = root / name
                original = path.read_bytes()
                path.write_bytes(original + b'\n')
                with self.subTest(name=name), self.assertRaisesRegex(ValueError, 'working files differ'):
                    hosted.evidence_request(root, head)
                path.write_bytes(original)


if __name__ == '__main__':
    unittest.main()
