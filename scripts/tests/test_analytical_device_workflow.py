"""P3 remains an additive, branch-isolated observational successor of P2."""
from pathlib import Path
import unittest

ROOT=Path(__file__).resolve().parents[2]


class DeviceWorkflowTests(unittest.TestCase):
    def text(self):return (ROOT/'.github/workflows/analytical-device-fe-diagnostic.yml').read_text()

    def test_original_invocation_is_byte_identical_and_source_is_frozen(self):
        text=self.text(); prior=(ROOT/'.github/workflows/analytical-present-fe-diagnostic.yml').read_text()
        command=lambda t:next(line for line in t.splitlines() if 'python scripts/qualify-analytical-swift-windows.py' in line)
        self.assertEqual(command(text),command(prior));self.assertEqual(text.count('--phase runtime'),1)
        self.assertIn("branches: ['diagnostic/analytical-device-fe7373e']",text)
        self.assertIn('SOURCE_SHA: fe7373e96396131ef4a1c2a9af9cfabdb8aab82b',text)
        self.assertIn('runs-on: windows-2022',text);self.assertIn('timeout-minutes: 360',text)
        self.assertIn('ref: ${{ env.SOURCE_SHA }}',text)
        for forbidden in ('continue-on-error:','contents: write','actions/cache','workflow_dispatch:','Copy-Item','git apply','RUST_LOG:','GetDeviceRemovedReason'):
            self.assertNotIn(forbidden,text)
        original=(ROOT/'scripts/qualify-analytical-swift-windows.py').read_text()
        self.assertIn('timeout=180',original)

    def test_original_upload_is_required_and_validation_happens_once(self):
        text=self.text()
        self.assertLess(text.index('Preserve unchanged permitted S evidence'),text.index('Observe only the bound native failure'))
        self.assertEqual(text.count('--validate-source'),1)
        self.assertIn("steps.original_upload.outcome == 'success'",text)
        self.assertIn("steps.source_evidence.outputs.native_failure == 'true'",text)
        helper=(ROOT/'scripts/observe-device-failure.py').read_text()
        self.assertEqual(helper.count('p2.validate_source('),1)
        self.assertEqual(helper.count('p2.project('),1)
        self.assertNotIn('capture.validate_export(',helper)
        self.assertNotIn('collect_runtime_facts(',helper)
        self.assertNotIn('collect-analytical-runtime-facts.py',text)

    def test_fixed_six_file_allowlist_and_qualified_poststep(self):
        text=self.text().split('name: Preserve only validated diagnostic JSON sidecars',1)[1]
        self.assertIn("steps.diagnostic_evidence.outputs.validated == 'true'",text)
        files=[line.strip().split('/')[-1] for line in text.splitlines() if '/device-export/' in line]
        self.assertEqual(files,['present-origin.json','source-validation.json','runtime-failure.json','present-error.json','device-origin.json','device-error.json'])
        self.assertNotIn('*',text)


if __name__=='__main__':unittest.main()
