"""Synthetic capability outcomes; never actual Windows availability."""
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location('caps', Path(__file__).parents[1] / 'observe-analytical-ui-capabilities.py')
c = importlib.util.module_from_spec(SPEC); SPEC.loader.exec_module(c)


class CapabilityTests(unittest.TestCase):
    def fixture(self, root):
        repo = root / 'repo'; repo.mkdir(); private = root / 'private'; private.mkdir()
        planned = {'ocr': ['synthetic-ocr'], 'desktop': ['synthetic-desktop']}
        c.capture.write_json(private / 'capture.json', {'source_sha': 'a' * 40, 'cwd': str(repo), 'commands': planned})
        for name in c.NAMES:
            (private / (name + '.stdout')).write_bytes(b'')
            (private / (name + '.stderr')).write_bytes(b'private diagnostic only')
            c.capture.write_json(private / (name + '.command.json'), {'command': planned[name], 'cwd': str(repo),
                 'outcome': 'failed', 'exit_code': 1, 'stdout': c.capture.file_record(private / (name + '.stdout')),
                 'stderr': c.capture.file_record(private / (name + '.stderr'))})
        return repo, private, planned

    def test_unavailable_prerequisites_preserve_bounded_facts_without_approval(self):
        with tempfile.TemporaryDirectory() as temporary:
            repo, private, planned = self.fixture(Path(temporary))
            with mock.patch.object(c, 'commands', return_value=planned): value = c.project(private, repo, 'a' * 40)
            self.assertEqual([row['state'] for row in value['probes'].values()], ['unavailable', 'unavailable'])
            self.assertFalse(value['appearance_accepted']); self.assertFalse(value['lifecycle_accepted'])
            self.assertNotIn('private diagnostic', json.dumps(value))

    def test_available_requires_exact_probe_output_and_literal_true(self):
        actual = {'outcome': 'succeeded', 'exit_code': 0}
        raw = {'schema_version': 1, 'engine': 'Windows.Media.Ocr', 'language': 'en-US', 'available': True}
        self.assertEqual(c.state('ocr', actual, json.dumps(raw).encode()), 'available')
        raw['available'] = 1
        self.assertEqual(c.state('ocr', actual, json.dumps(raw).encode()), 'unknown')
        self.assertEqual(c.state('desktop', actual, b'unrelated output'), 'unknown')
        self.assertEqual(c.state('desktop', {'outcome': 'timed_out'}, b''), 'unknown')

    def test_failure_fields_cannot_be_arbitrary_export_payloads(self):
        with tempfile.TemporaryDirectory() as temporary:
            repo, private, planned = self.fixture(Path(temporary))
            with mock.patch.object(c, 'commands', return_value=planned): value = c.project(private, repo, 'a' * 40)
            for payload in ({'private': 'text'}, 'private', True, 2**40):
                value['probes']['ocr']['exit_code'] = payload
                with self.assertRaises(ValueError): c.validate_projection(value)

    def test_private_journal_or_stream_mutation_is_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            repo, private, planned = self.fixture(Path(temporary))
            (private / 'ocr.stdout').write_bytes(b'changed')
            with mock.patch.object(c, 'commands', return_value=planned), self.assertRaises(ValueError):
                c.project(private, repo, 'a' * 40)


if __name__ == '__main__': unittest.main()
