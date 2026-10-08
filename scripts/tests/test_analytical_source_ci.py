"""Synthetic source-CI responses only: no real API, build or publication calls."""
import contextlib
import copy
from email.message import Message
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock
import urllib.error


SCRIPT = Path(__file__).resolve().parents[1] / 'check-analytical-source-ci.py'
SPEC = importlib.util.spec_from_file_location('analytical_source_ci', SCRIPT)
gate = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(gate)
REPOSITORY = 'fixture/source'
SOURCE = 'a' * 40


def run_fixture():
    return {
        'id': 123, 'run_attempt': 2, 'workflow_id': 45, 'name': 'CI',
        'path': '.github/workflows/ci.yml', 'head_sha': SOURCE, 'head_branch': 'main',
        'event': 'push', 'status': 'completed', 'conclusion': 'success', 'pull_requests': [],
        'repository': {'id': 67, 'full_name': REPOSITORY},
        'head_repository': {'id': 67, 'full_name': REPOSITORY},
    }


def reader_fixture(runs=None):
    runs = [run_fixture()] if runs is None else runs
    reader = mock.Mock()
    reader.get.side_effect = [
        {'id': 45, 'name': 'CI', 'path': '.github/workflows/ci.yml', 'state': 'active'},
        {'total_count': len(runs), 'workflow_runs': runs},
        copy.deepcopy(max(runs, key=lambda run: run['id'])) if runs else {},
    ]
    return reader


class SourceCITests(unittest.TestCase):
    def check(self, reader=None):
        return gate.check_source_ci(reader or reader_fixture(), REPOSITORY, SOURCE)

    def test_exact_success_receipt_is_canonical_bounded_and_not_release_authorization(self):
        reader = reader_fixture()
        receipt = self.check(reader)
        self.assertEqual(receipt, {
            'schema_version': 1, 'kind': 'analytical-source-ci', 'source_sha': SOURCE,
            'repository': REPOSITORY, 'head_repository': REPOSITORY, 'repository_id': 67,
            'run_id': 123, 'run_attempt': 2, 'workflow_id': 45, 'workflow_name': 'CI',
            'workflow_path': '.github/workflows/ci.yml', 'head_branch': 'main',
            'event': 'push', 'status': 'completed', 'conclusion': 'success',
            'release_authorized': False,
        })
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'source-ci.json'
            gate.write_receipt(output, receipt)
            payload = output.read_bytes()
        self.assertEqual(payload, (json.dumps(receipt, sort_keys=True, separators=(',', ':')) + '\n').encode())
        self.assertLessEqual(len(payload), gate.MAX_RECEIPT_BYTES)
        paths = [call.args[0] for call in reader.get.call_args_list]
        self.assertEqual(paths, [
            '/repos/fixture/source/actions/workflows/ci.yml',
            '/repos/fixture/source/actions/workflows/45/runs?head_sha=' + SOURCE
            + '&branch=main&event=push&per_page=100&page=1',
            '/repos/fixture/source/actions/runs/123',
        ])

    def test_ancestor_pr_foreign_workflow_and_nonterminal_runs_fail(self):
        changes = {
            'head_sha': ['b' * 40, SOURCE[:7], None],
            'head_branch': ['feature/qualification', 'refs/heads/main', None],
            'event': ['pull_request', 'pull_request_target', 'workflow_dispatch', 'workflow_run'],
            'path': ['.github/workflows/other.yml', '.github/workflows/ci.yml@main', None],
            'name': ['CI spoof', None], 'workflow_id': [46, True, '45'],
            'status': ['queued', 'in_progress', None],
            'conclusion': ['failure', 'cancelled', 'skipped', 'neutral', None],
            'pull_requests': [[{'number': 1}], None, {}],
            'id': [0, -1, True, '123', gate.MAX_ID + 1],
            'run_attempt': [0, True, '2', gate.MAX_ID + 1],
        }
        for key, values in changes.items():
            for value in values:
                with self.subTest(key=key, value=value):
                    run = run_fixture()
                    run[key] = value
                    with self.assertRaises(gate.GateError):
                        self.check(reader_fixture([run]))

    def test_same_name_different_repository_identity_is_rejected(self):
        for key in ('repository', 'head_repository'):
            for value in (None, {}, {'id': 67, 'full_name': 'foreign/source'},
                          {'id': 68, 'full_name': REPOSITORY},
                          {'id': True, 'full_name': REPOSITORY}):
                with self.subTest(key=key, value=value):
                    run = run_fixture()
                    run[key] = value
                    with self.assertRaises(gate.GateError):
                        self.check(reader_fixture([run]))

    def test_no_fallback_to_older_success_when_latest_fails_or_is_pending(self):
        for status, conclusion in (('in_progress', None), ('completed', 'failure')):
            latest = run_fixture()
            latest.update(id=124, status=status, conclusion=conclusion)
            with self.subTest(status=status), self.assertRaises(gate.GateError):
                self.check(reader_fixture([latest, run_fixture()]))

    def test_latest_run_selection_is_independent_of_api_order(self):
        latest = run_fixture()
        latest['id'] = 124
        self.assertEqual(self.check(reader_fixture([latest, run_fixture()]))['run_id'], 124)

    def test_all_returned_runs_must_belong_to_exact_query(self):
        foreign = run_fixture()
        foreign.update(id=122, head_sha='b' * 40)
        with self.assertRaises(gate.GateError):
            self.check(reader_fixture([foreign, run_fixture()]))

    def test_no_runs_or_duplicate_runs_fail(self):
        for runs in ([], [run_fixture(), run_fixture()]):
            with self.subTest(runs=len(runs)), self.assertRaises(gate.GateError):
                self.check(reader_fixture(runs))

    def test_workflow_must_be_the_active_expected_path_name_and_id(self):
        for field, value in (('id', True), ('id', 0), ('name', 'Other'),
                             ('path', 'ci.yml'), ('state', 'disabled_manually')):
            reader = reader_fixture()
            values = list(reader.get.side_effect)
            values[0][field] = value
            reader.get.side_effect = values
            with self.subTest(field=field), self.assertRaises(gate.GateError):
                self.check(reader)

    def test_incomplete_or_oversize_page_fails_without_pagination(self):
        for page in ({'total_count': 101, 'workflow_runs': [run_fixture()]},
                     {'total_count': 2, 'workflow_runs': [run_fixture()]},
                     {'total_count': True, 'workflow_runs': [run_fixture()]},
                     {'total_count': 1, 'workflow_runs': {}}, [], {}):
            reader = reader_fixture()
            values = list(reader.get.side_effect)
            values[1] = page
            reader.get.side_effect = values
            with self.subTest(page=page), self.assertRaises(gate.GateError):
                self.check(reader)
            self.assertEqual(reader.get.call_count, 2)

    def test_selected_run_is_reread_and_attempt_change_fails(self):
        for field, value in (('id', 124), ('run_attempt', 3), ('head_sha', 'b' * 40),
                             ('status', 'in_progress'), ('workflow_id', 99)):
            reader = reader_fixture()
            values = list(reader.get.side_effect)
            values[2][field] = value
            reader.get.side_effect = values
            with self.subTest(field=field), self.assertRaises(gate.GateError):
                self.check(reader)

    def test_untrusted_api_fields_never_enter_receipt(self):
        run = run_fixture()
        run.update(display_title='::error::SECRET', html_url='https://foreign.invalid/SECRET',
                   logs_url='SECRET', release_authorized=True, arbitrary='x' * 10000)
        receipt = self.check(reader_fixture([run]))
        self.assertNotIn('SECRET', json.dumps(receipt))
        self.assertIs(receipt['release_authorized'], False)

    def test_invalid_inputs_never_fetch(self):
        for repository, source in (('https://foreign.invalid/source', SOURCE),
                                   ('owner/repo?x=1', SOURCE), ('owner/..', SOURCE),
                                   ('owner/repo\n', SOURCE), ('a' * 40 + '/repo', SOURCE),
                                   (REPOSITORY, SOURCE[:7]), (REPOSITORY, 'A' * 40)):
            reader = mock.Mock()
            with self.subTest(repository=repository, source=source), self.assertRaises(gate.GateError):
                gate.check_source_ci(reader, repository, source)
            reader.get.assert_not_called()

    def test_cli_success_and_failure_have_safe_output(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'source-ci.json'
            args = ['--repository', REPOSITORY, '--source-sha', SOURCE, '--output', str(output)]
            stdout, stderr = io.StringIO(), io.StringIO()
            with mock.patch.dict(gate.os.environ, {'GH_TOKEN': 'SECRET'}, clear=True), \
                    mock.patch.object(gate, 'GitHubReader', return_value=reader_fixture()), \
                    contextlib.redirect_stdout(stdout), contextlib.redirect_stderr(stderr):
                self.assertEqual(gate.main(args), 0)
                original = output.read_bytes()
                self.assertEqual(gate.main(args), 1)
                self.assertEqual(output.read_bytes(), original)
            self.assertNotIn('SECRET', stdout.getvalue() + stderr.getvalue())
            self.assertIn('release_authorized=false', stdout.getvalue())

    def test_failed_ci_does_not_write_receipt(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'source-ci.json'
            run = run_fixture()
            run['conclusion'] = 'failure'
            with mock.patch.dict(gate.os.environ, {'GITHUB_TOKEN': 'SECRET'}, clear=True), \
                    mock.patch.object(gate, 'GitHubReader', return_value=reader_fixture([run])), \
                    contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(gate.main(['--repository', REPOSITORY, '--source-sha', SOURCE,
                                            '--output', str(output)]), 1)
            self.assertFalse(output.exists())


class TransportTests(unittest.TestCase):
    def response(self, payload=b'{}', content_type='application/json'):
        response = mock.MagicMock()
        response.__enter__.return_value = response
        response.status = 200
        response.headers = Message()
        response.headers['Content-Type'] = content_type
        body = io.BytesIO(payload)
        response.read1.side_effect = body.read1
        return response

    def reader(self, response=None):
        reader = gate.GitHubReader('SECRET_TOKEN')
        reader._opener = mock.Mock()
        reader._opener.open.return_value = response or self.response()
        return reader

    def test_fixed_origin_get_headers_timeout_and_request_budget(self):
        reader = self.reader()
        for _ in range(gate.MAX_REQUESTS):
            reader._opener.open.return_value = self.response()
            self.assertEqual(reader.get('/repos/fixture/source/actions/workflows/ci.yml'), {})
        request = reader._opener.open.call_args.args[0]
        self.assertEqual(request.full_url, 'https://api.github.com/repos/fixture/source/actions/workflows/ci.yml')
        self.assertEqual(request.get_method(), 'GET')
        self.assertEqual(request.get_header('Authorization'), 'Bearer SECRET_TOKEN')
        self.assertIsNone(request.data)
        self.assertLessEqual(reader._opener.open.call_args.kwargs['timeout'], gate.REQUEST_TIMEOUT)
        with self.assertRaises(gate.GateError):
            reader.get('/repos/fixture/source/actions/workflows/ci.yml')
        self.assertEqual(reader._opener.open.call_count, gate.MAX_REQUESTS)

    def test_redirects_never_receive_authorization(self):
        with self.assertRaisesRegex(gate.GateError, 'redirect refused'):
            gate.NoRedirects().redirect_request(None, None, 302, 'SECRET', {}, 'https://foreign.invalid')

    def test_deadline_fails_before_request(self):
        reader = self.reader()
        reader._deadline = 0
        with self.assertRaises(gate.GateError):
            reader.get('/repos/fixture/source/actions/workflows/ci.yml')
        reader._opener.open.assert_not_called()

    def test_response_size_and_json_shape_are_bounded(self):
        payloads = (b' ' * (gate.MAX_RESPONSE_BYTES + 1), b'[]', b'{', b'{"id":1,"id":2}',
                    b'{"id":NaN}', b'\xff', b'[' * 2000 + b']' * 2000)
        for payload in payloads:
            reader = self.reader(self.response(payload))
            with self.subTest(length=len(payload)), self.assertRaises(gate.GateError):
                reader.get('/repos/fixture/source/actions/workflows/ci.yml')

    def test_html_http_error_and_timeout_messages_do_not_leak_remote_content(self):
        for exception in (TimeoutError('SECRET'), urllib.error.URLError('SECRET'),
                          gate.http.client.BadStatusLine('SECRET'),
                          urllib.error.HTTPError('https://secret.invalid', 403, 'SECRET', {}, None)):
            reader = self.reader()
            reader._opener.open.side_effect = exception
            with self.assertRaises(gate.GateError) as caught:
                reader.get('/repos/fixture/source/actions/workflows/ci.yml')
            self.assertNotIn('SECRET', str(caught.exception))
        reader = self.reader(self.response(b'SECRET', 'text/html'))
        with self.assertRaises(gate.GateError) as caught:
            reader.get('/repos/fixture/source/actions/workflows/ci.yml')
        self.assertNotIn('SECRET', str(caught.exception))

    def test_token_and_origin_injection_rejected(self):
        for token in ('', 'SECRET\nInjected: true', 'SECRET token'):
            with self.assertRaises(gate.GateError):
                gate.GitHubReader(token)
        reader = self.reader()
        for path in ('https://foreign.invalid', '//foreign.invalid', '/repos/x/y\nInjected: true'):
            with self.assertRaises(gate.GateError):
                reader.get(path)
        reader._opener.open.assert_not_called()

    def test_receipt_bound_and_existing_file_are_enforced(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'source-ci.json'
            with self.assertRaises(gate.GateError):
                gate.write_receipt(output, {'unexpected': 'x' * gate.MAX_RECEIPT_BYTES})
            self.assertFalse(output.exists())
            output.write_text('existing')
            with self.assertRaises(FileExistsError):
                gate.write_receipt(output, {'release_authorized': False})
            self.assertEqual(output.read_text(), 'existing')


if __name__ == '__main__':
    unittest.main()
