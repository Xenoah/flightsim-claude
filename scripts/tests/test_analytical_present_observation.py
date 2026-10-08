"""P2 binding and audit-count boundaries using synthetic upstream authority."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location('present_observe', Path(__file__).parents[1] / 'observe-present-failure.py')
p = importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(p)


class PresentObservationTests(unittest.TestCase):
    def fixture(self, root):
        source_repo = root / 'source'; source_repo.mkdir()
        private = root / 'source-private'; private.mkdir(); export = root / 'source-export'; export.mkdir()
        policy_private = root / 'p2-private'
        source = {'source_sha': p.SOURCE_SHA, 'source_tree': p.SOURCE_TREE, 'files': []}
        p.capture.write_json(private / 'source.json', source)
        build, text = private / 'build-private', private / 'build-text'; build.mkdir(); text.mkdir()
        exe = build / 'target-analytic' / p.capture.check.TARGET / 'release/flightsim-app.exe'
        exe.parent.mkdir(parents=True); exe.write_bytes(b'synthetic audited executable')
        summary = {'status': p.capture.PASS, 'source_sha': p.SOURCE_SHA, 'source_tree': p.SOURCE_TREE,
                   'builds': {'analytic': {'executable': p.capture.file_record(exe)}}}
        p.capture.write_json(text / p.capture.EXPORT_NAME, summary); p.capture.write_json(build / 'progress.json', summary)
        for name in ('frozen-trees.json', 'source.json'): p.capture.write_json(build / name, {})
        bundle = private / 'extracted/swift-candidate'; bundle.mkdir(parents=True)
        (bundle / 'flightsim-app.exe').write_bytes(exe.read_bytes()); p.capture.write_json(bundle / 'bundle-manifest.json', {})
        p.capture.write_json(export / 'native-review.json', {})
        command = private / 'commands/default-swift'; command.mkdir(parents=True)
        (command / 'stdout').write_bytes(b'')
        (command / 'stderr').write_bytes(b'Error in Surface::present: Validation Error\n\nCaused by:\n  Parent device is lost\n\n')
        argv = [str(bundle / 'flightsim-app.exe'), '--screenshot', str(private / 'default-swift.png')]
        journal = {'command': argv, 'cwd': str(private / 'unrelated-cwd'), 'exit_code': 1, 'outcome': 'failed',
                   'elapsed_seconds': 38.703, 'timeout_seconds': 180,
                   'stdout': p.capture.file_record(command / 'stdout'), 'stderr': p.capture.file_record(command / 'stderr')}
        p.capture.write_json(command / 'journal.json', journal)
        report = {'phase': 'runtime', 'source_sha': p.SOURCE_SHA, 'source_tree': p.SOURCE_TREE,
                  'status': 'failed', 'bindings': {'source': p.capture.file_record(private / 'source.json')},
                  'native_projection': p.capture.file_record(export / 'native-review.json'),
                  'runtime_facts': {'bytes': 1, 'sha256': 'a' * 64},
                  'commands': [{'id': 'default-swift', **{key: journal[key] for key in ('outcome', 'exit_code', 'stdout', 'stderr')}}]}
        p.capture.write_json(private / 'result.json', report); p.capture.write_json(export / 'qualification.json', report)
        original = mock.Mock(EXPORT='qualification.json', MAX_JSON=2 * 1024 * 1024)
        original.validate_export.return_value = report
        original.command_specifications.return_value = {'default-swift': argv}
        context = (original, source, {'source_tree': 'b' * 40}, {'source_sha': p.SOURCE_SHA, 'policy_sha': 'c' * 40})
        args = (source_repo, p.SOURCE_SHA, private, export, 'c' * 40, policy_private)
        return args, context, report, exe

    def test_full_original_validation_runs_once_then_observation_checks_bindings(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); args, context, _, _ = self.fixture(root); evidence = root / 'evidence'
            with mock.patch.object(p, 'source_context', return_value=context), mock.patch.object(p.capture, 'validate_summary'), \
                 mock.patch.object(p.capture, 'validate_export', side_effect=AssertionError('no repeated frozen target audit')):
                receipt = p.validate_source(*args)
                self.assertEqual(context[0].validate_export.call_count, 1)
                p.observe(*args, evidence)
                p.validate_export(evidence, *args)
                self.assertEqual(context[0].validate_export.call_count, 1)
            self.assertEqual(receipt['native_failure_command'], 'default-swift')
            packet = json.loads((evidence / p.EXCERPT).read_bytes())
            self.assertEqual(packet['cause_block']['lines'][0]['text'], 'Parent device is lost')
            self.assertFalse(packet['runtime_accepted']); self.assertFalse(packet['release_authorized'])
            self.assertEqual({path.name for path in evidence.iterdir()}, {p.ORIGIN, p.EXCERPT, p.RECEIPT, p.failure.EXPORT})

    def test_current_executable_and_bound_raw_stream_mutations_are_rejected(self):
        for target in ('executable', 'stderr', 'journal'):
            with self.subTest(target=target), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary); args, context, _, exe = self.fixture(root)
                with mock.patch.object(p, 'source_context', return_value=context), mock.patch.object(p.capture, 'validate_summary'):
                    p.validate_source(*args)
                    path = exe if target == 'executable' else args[2] / 'commands/default-swift' / ('stderr' if target == 'stderr' else 'journal.json')
                    path.write_bytes(path.read_bytes() + b'CHANGED')
                    with self.assertRaises((ValueError, json.JSONDecodeError)): p.bound_context(*args)

    def test_validation_receipt_or_root_repoint_cannot_be_silently_resealed(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); args, context, _, _ = self.fixture(root)
            with mock.patch.object(p, 'source_context', return_value=context), mock.patch.object(p.capture, 'validate_summary'):
                p.validate_source(*args)
                receipt = p.read_json(args[-1] / p.RECEIPT, p.MAX_RECEIPT)
                receipt['runtime_accepted'] = True; (args[-1] / p.RECEIPT).write_bytes(p.canonical(receipt))
                with self.assertRaises(ValueError): p.bound_context(*args)
                receipt['runtime_accepted'] = False; (args[-1] / p.RECEIPT).write_bytes(p.canonical(receipt))
                (args[-1] / 'inputs.json').write_bytes(p.canonical({'source_sha': 'd' * 40}))
                with self.assertRaises(ValueError): p.bound_context(*args)

    def test_export_rejects_arbitrary_extra_payload_and_changed_selected_text(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); args, context, _, _ = self.fixture(root); evidence = root / 'evidence'
            with mock.patch.object(p, 'source_context', return_value=context), mock.patch.object(p.capture, 'validate_summary'):
                p.validate_source(*args); p.observe(*args, evidence)
                packet = p.read_json(evidence / p.EXCERPT, 16384)
                packet['cause_block']['lines'][0]['text'] = 'PRIVATE_SECRET'
                (evidence / p.EXCERPT).write_bytes(p.canonical(packet))
                with self.assertRaises(ValueError): p.validate_export(evidence, *args)
                (evidence / 'raw.log').write_bytes(b'PRIVATE')
                with self.assertRaises(ValueError): p.validate_export(evidence, *args)

    def test_prebuild_failure_remains_preservable_without_native_failure_claim(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary); args, context, report, _ = self.fixture(root)
            report.update(source_tree=None, bindings={}, commands=[], native_projection=None, runtime_facts=None)
            for path in (args[2] / 'result.json', args[3] / 'qualification.json'): p.capture.write_json(path, report)
            with mock.patch.object(p, 'source_context', return_value=context), mock.patch.object(p.capture, 'validate_summary'):
                receipt = p.validate_source(*args)
                self.assertIsNone(receipt['native_failure_command']); self.assertIsNone(receipt['original_source_tree'])
                with self.assertRaises(ValueError): p.project(*args)


if __name__ == '__main__': unittest.main()
