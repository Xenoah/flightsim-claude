"""Critical S/P invocation and export boundaries of the one diagnostic workflow."""
from pathlib import Path
import re
import unittest

REPO = Path(__file__).resolve().parents[2]
WORKFLOW = REPO / '.github/workflows/analytical-runtime-fe-diagnostic.yml'


class DiagnosticWorkflowTests(unittest.TestCase):
    def test_only_exact_diagnostic_branch_and_original_main_source_ci(self):
        text = WORKFLOW.read_text()
        self.assertIn("branches: ['diagnostic/analytical-runtime-fe7373e']", text)
        self.assertIn('SOURCE_SHA: fe7373e96396131ef4a1c2a9af9cfabdb8aab82b', text)
        self.assertIn('--source-sha "$SOURCE_SHA"', text)
        for forbidden in ('workflow_dispatch:', 'workflow_run:', 'pull_request:', 'contents: write', 'actions/cache', 'continue-on-error:', 'gh release', 'RUST_LOG:'):
            self.assertNotIn(forbidden, text)
        self.assertEqual(text.count('actions: read'), 1)
        self.assertEqual(re.findall(r'^  ([a-z-]+):\n    (?:permissions|needs):', text, re.M), ['source-ci', 'runtime-diagnostic'])

    def test_separate_clean_checkout_and_unchanged_bounded_windows_runtime(self):
        text = WORKFLOW.read_text()
        runtime = text.split('  runtime-diagnostic:', 1)[1]
        self.assertIn('needs: source-ci', runtime)
        self.assertIn('runs-on: windows-2022', runtime)
        self.assertIn('timeout-minutes: 360', runtime)
        self.assertEqual(runtime.count('path: policy'), 1); self.assertEqual(runtime.count('path: source'), 1)
        self.assertIn('ref: ${{ env.SOURCE_SHA }}', runtime)
        self.assertEqual(runtime.count('persist-credentials: false'), 2)
        self.assertEqual(runtime.count('--phase runtime'), 1)
        self.assertIn("working-directory: source\n        run: |\n          python scripts/qualify-analytical-swift-windows.py --phase runtime --source-sha $env:SOURCE_SHA", runtime)
        self.assertIn('rustup toolchain install 1.93.0 --profile minimal --target x86_64-pc-windows-msvc', runtime)
        for mutation in ('Copy-Item', 'Move-Item', 'Set-Content', 'git apply', 'git checkout', 'git reset'):
            self.assertNotIn(mutation, runtime)

    def test_uploads_are_fixed_validated_json_and_existing_eight_pngs(self):
        text = WORKFLOW.read_text()
        uploads = re.findall(r'          path: \|\n((?:            .*\n)+)', text)
        self.assertEqual(len(uploads), 2)
        source = [line.strip().rsplit('/', 1)[-1] for line in uploads[0].splitlines()]
        diag = [line.strip().rsplit('/', 1)[-1] for line in uploads[1].splitlines()]
        self.assertEqual(source[:4], ['qualification.json', 'native-review.json', 'runtime-facts.json', 'ui-capabilities.json'])
        self.assertEqual(source[4:], ['default-swift.png', 'day-cockpit.png', 'low-sun.png', 'night-cockpit.png', 'fog-cockpit.png', 'cloud-high.png', 'water-high.png', 'tower-day.png'])
        self.assertEqual(diag, ['diagnostic-origin.json', 'runtime-facts.json', 'runtime-failure.json'])
        self.assertIn("if: always() && steps.source_evidence.outputs.validated == 'true'", text)
        self.assertIn("if: always() && steps.diagnostic_evidence.outputs.validated == 'true'", text)
        self.assertNotIn('*', ''.join(uploads))
        self.assertIn('--validate-evidence', text)


if __name__ == '__main__': unittest.main()
