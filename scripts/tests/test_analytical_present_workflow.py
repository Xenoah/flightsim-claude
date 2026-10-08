"""The P2 run must remain isolated, failure-only, bounded and nonpublishing."""
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]


class PresentWorkflowTests(unittest.TestCase):
    def test_exact_isolated_source_and_single_unchanged_reproducer(self):
        text = (ROOT / '.github/workflows/analytical-present-fe-diagnostic.yml').read_text()
        self.assertIn("branches: ['diagnostic/analytical-present-fe7373e']", text)
        self.assertIn('SOURCE_SHA: fe7373e96396131ef4a1c2a9af9cfabdb8aab82b', text)
        self.assertIn('--source-sha "$SOURCE_SHA"', text)
        self.assertIn('runs-on: windows-2022', text); self.assertIn('timeout-minutes: 360', text)
        self.assertEqual(text.count('--phase runtime'), 1)
        self.assertIn('ref: ${{ env.SOURCE_SHA }}', text)
        for forbidden in ('continue-on-error:', 'contents: write', 'actions/cache', 'workflow_dispatch:', 'Copy-Item', 'git apply', 'RUST_LOG:'):
            self.assertNotIn(forbidden, text)

    def test_original_evidence_is_saved_before_excerpt_and_no_microsoft_recollection(self):
        text = (ROOT / '.github/workflows/analytical-present-fe-diagnostic.yml').read_text()
        self.assertLess(text.index('Preserve unchanged permitted S evidence'), text.index('Observe only the bound native failure'))
        self.assertIn("steps.source_evidence.outputs.native_failure == 'true'", text)
        self.assertIn("steps.original_upload.outcome == 'success'", text)
        self.assertIn('id: original_upload', text)
        self.assertEqual(text.count('--validate-source'), 1)
        self.assertNotIn('collect-analytical-runtime-facts.py', text)
        self.assertNotIn('observe-analytical-diagnostic-sidecars.py', text)
        helper = (ROOT / 'scripts/observe-present-failure.py').read_text()
        self.assertEqual(helper.count('original.validate_export('), 1)
        self.assertNotIn('capture.validate_export(', helper)
        self.assertNotIn('collect_runtime_facts(', helper)

    def test_fixed_excerpt_export_set_and_no_wildcards(self):
        text = (ROOT / '.github/workflows/analytical-present-fe-diagnostic.yml').read_text()
        final = text.split('name: Preserve only validated diagnostic JSON sidecars', 1)[1]
        self.assertIn("steps.diagnostic_evidence.outputs.validated == 'true'", final)
        files = [line.strip().split('/')[-1] for line in final.splitlines() if '/present-export/' in line]
        self.assertEqual(files, ['present-origin.json', 'source-validation.json', 'runtime-failure.json', 'present-error.json'])
        self.assertNotIn('*', final)


if __name__ == '__main__': unittest.main()
