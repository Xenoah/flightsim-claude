"""CI screenshot provenance contracts; no Cargo, renderer or uploads required."""
from pathlib import Path
import subprocess
import tempfile
import unittest


WORKFLOW = Path(__file__).resolve().parents[2] / ".github/workflows/ci.yml"


class CiSmokeWorkflowTests(unittest.TestCase):
    def smoke(self):
        return WORKFLOW.read_text().split("  app-smoke:\n", 1)[1]

    def test_rendered_smoke_selects_original_swift_sport_explicitly(self):
        smoke = self.smoke()
        invocation = smoke.split("target/release/flightsim-app", 1)[1].split('>"$log"', 1)[0]
        self.assertIn("--aircraft swift-sport", invocation)
        self.assertNotIn("--no-model", invocation)
        self.assertNotIn("--model", invocation)
        cleanup = 'rm -f -- "$screenshot"'
        self.assertIn(cleanup, smoke)
        self.assertGreater(smoke.index(cleanup), smoke.index('screenshot="$RUNNER_TEMP/flightsim-smoke.png"'))
        self.assertLess(smoke.index(cleanup), smoke.index("xvfb-run -a env"))

    def test_complete_png_still_requires_selected_profile_model_and_fit(self):
        smoke = self.smoke()
        ready = smoke.split("ready=false", 1)[1].split("ready=true", 1)[0]
        for proof in ('complete_png', 'grep -Fq "(swift-sport)" "$log"',
                      'grep -Fq "aircraft/swift_sport.glb" "$log"',
                      'grep -Fq "aircraft model fitted:" "$log"'):
            self.assertIn(proof, ready)

    def test_actual_shell_condition_rejects_wrong_aircraft_model_or_incomplete_proof(self):
        condition = self.smoke().split("if complete_png", 1)[1].split("; then", 1)[0]
        valid_log = (
            "aircraft: Swift Sport (swift-sport)\n"
            "aircraft model: /repo/assets/aircraft/swift_sport.glb\n"
            "aircraft model fitted: 7.12 m along its length\n"
        )
        cases = [
            ("valid Swift", valid_log, 0, 0),
            ("wrong profile", valid_log.replace("(swift-sport)", "(light-single)"), 0, 1),
            ("wrong model", valid_log.replace("swift_sport.glb", "light_single.glb"), 0, 1),
            ("no model fit", valid_log.replace("aircraft model fitted:", "still loading:"), 0, 1),
            ("incomplete PNG", valid_log, 1, 1),
        ]
        with tempfile.TemporaryDirectory() as temporary:
            log = Path(temporary) / "synthetic-smoke.log"
            for label, contents, png_status, expected in cases:
                with self.subTest(label=label):
                    log.write_text(contents)
                    script = f"complete_png() {{ return {png_status}; }}; log=$1; if complete_png{condition}; then exit 0; else exit 1; fi"
                    result = subprocess.run(["bash", "-c", script, "fixture", str(log)], capture_output=True)
                    self.assertEqual(result.returncode, expected, result.stderr.decode())

    def test_png_upload_path_is_only_exposed_after_model_identity_checks(self):
        smoke = self.smoke()
        self.assertIn("id: smoke", smoke)
        marker = 'echo "verified_screenshot=$screenshot" >> "$GITHUB_OUTPUT"'
        self.assertIn(marker, smoke)
        failure_check = smoke.split('if [[ "$ready" != true ]]; then', 1)[1].split("fi", 1)[0]
        self.assertIn("exit 1", failure_check)
        self.assertGreater(smoke.index(marker), smoke.index('if [[ "$ready" != true ]]; then'))
        fatal_failure = smoke.split('echo "application logged an error during the smoke run"', 1)[1].split("fi", 1)[0]
        self.assertIn("exit 1", fatal_failure)
        self.assertGreater(smoke.index(marker), smoke.index('echo "application logged an error during the smoke run"'))
        upload = smoke.split("- name: Upload smoke screenshot and log", 1)[1]
        self.assertIn("if: always()", upload)
        self.assertIn("${{ steps.smoke.outputs.verified_screenshot }}", upload)
        self.assertNotIn("${{ runner.temp }}/flightsim-smoke.png", upload)
        self.assertIn("${{ runner.temp }}/flightsim-smoke.log", upload)
        self.assertNotIn("*", upload)


if __name__ == "__main__":
    unittest.main()
