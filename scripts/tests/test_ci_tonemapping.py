"""The CI runner must not mistake unrelated compiler failures for mode guards."""
import contextlib
import importlib.util
import io
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    'ci_tonemapping', Path(__file__).resolve().parents[1] / 'ci-tonemapping.py')
CI = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CI)
DIAGNOSTIC = 'analytic-tonemapping requires --no-default-features; select one tone mode'


class CommandStatusTests(unittest.TestCase):
    def invoke(self, exit_code, diagnostics=(), success=False, expected=None):
        messages = [
            {'reason': 'compiler-message', 'message': {'level': 'error', 'message': text}}
            for text in diagnostics
        ] + [{'reason': 'build-finished', 'success': success}]
        payload = '\n'.join(json.dumps(message) for message in messages)
        command = [sys.executable, '-c',
                   'import sys; print(sys.argv[1]); sys.exit(int(sys.argv[2]))',
                   payload, str(exit_code)]
        with tempfile.TemporaryDirectory() as directory, \
                contextlib.redirect_stdout(io.StringIO()), \
                contextlib.redirect_stderr(io.StringIO()):
            evidence = Path(directory)
            outcome = 0
            try:
                CI.run(evidence, os.environ.copy(), 'fixture', command, expected)
            except SystemExit as error:
                outcome = error.code
            record = json.loads((evidence / 'fixture.command.json').read_text())
            self.assertEqual(record['exit_code'], exit_code)
            self.assertEqual(record['command'], command)
            self.assertEqual((evidence / 'fixture.stdout').read_text(), payload + '\n')
            return outcome, record

    def test_successful_command_retains_zero_exit(self):
        self.assertEqual(self.invoke(0, success=True)[0], 0)

    def test_failed_command_preserves_nonzero_exit(self):
        self.assertEqual(self.invoke(17)[0], 17)

    def test_exact_compile_guard_rejection_is_accepted(self):
        outcome, record = self.invoke(101, [DIAGNOSTIC], expected=DIAGNOSTIC)
        self.assertEqual(outcome, 0)
        self.assertTrue(record['matched_expected_rejection'])

    def test_guard_disappearance_fails_even_with_expected_text(self):
        self.assertNotEqual(self.invoke(0, [DIAGNOSTIC], success=True,
                                       expected=DIAGNOSTIC)[0], 0)

    def test_unrelated_compiler_error_is_not_suppressed(self):
        self.assertEqual(self.invoke(101, ['could not find dependency'],
                                     expected=DIAGNOSTIC)[0], 101)
        self.assertEqual(self.invoke(101, [DIAGNOSTIC, 'cannot find value'],
                                     expected=DIAGNOSTIC)[0], 101)

    def test_failed_process_needs_failed_cargo_completion(self):
        self.assertEqual(self.invoke(101, [DIAGNOSTIC], success=True,
                                     expected=DIAGNOSTIC)[0], 101)
        self.assertEqual(self.invoke(130, [DIAGNOSTIC], expected=DIAGNOSTIC)[0], 130)


if __name__ == '__main__':
    unittest.main()
