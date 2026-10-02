"""Release workflow wiring: these guards need no Cargo, network or real approval.

These are focused contract assertions, not a replacement for GitHub/actionlint's
YAML/expression validator. Keep source CI separate from optional publication.
"""
from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/release.yml"


def jobs():
    text = WORKFLOW.read_text()
    parts = re.split(r"^  ([a-z_]+):\n", text.split("jobs:\n", 1)[1], flags=re.MULTILINE)
    return dict(zip(parts[1::2], parts[2::2]))


class ReleaseWorkflowTests(unittest.TestCase):
    def test_build_has_an_explicit_authorization_dependency(self):
        build = jobs()["build_windows"].split("    steps:", 1)[0]
        self.assertIn("needs: authorize", build)
        self.assertIn("needs.authorize.outputs.authorized == 'true'", build)

    def test_publish_has_an_explicit_authorization_dependency(self):
        publish = jobs()["publish"].split("    steps:", 1)[0]
        self.assertIn("needs: [authorize, build_windows]", publish)
        self.assertIn("needs.authorize.outputs.authorized == 'true'", publish)
        self.assertIn("needs: [build_windows, publish]", jobs()["cleanup_merged_branch"])

    def test_preflight_retains_same_repository_successful_main_push_guards(self):
        authorization = jobs()["authorize"]
        for guard in ("conclusion == 'success'", "event == 'push'", "head_branch == 'main'",
                      "head_sha == github.sha", "head_repository.full_name == github.repository"):
            self.assertIn(guard, authorization)
        self.assertIn("--allow-blocked", authorization)
        self.assertIn("--github-summary", authorization)
        self.assertIn("authorized: ${{ steps.gate.outputs.authorized }}", authorization)
        self.assertNotIn("contents: write", authorization)
        self.assertNotIn("upload-artifact", authorization)
        self.assertNotIn("cargo build", authorization)

    def test_exact_checkout_and_recheck_precede_the_build(self):
        for name in ("authorize", "build_windows"):
            self.assertIn("ref: ${{ github.event.workflow_run.head_sha }}", jobs()[name])
            self.assertIn("persist-credentials: false", jobs()[name])
        build = jobs()["build_windows"]
        self.assertLess(build.index("scripts/check-release-authorization.py"), build.index("cargo build"))
        self.assertNotIn("--allow-blocked", build)
        for field in ("release_inventory_sha256", "source_inventory_sha256", "authorization_sha256"):
            self.assertIn("$gate." + field + " -ne $env:EXPECTED_", build)

    def test_copy_plan_is_the_only_source_of_package_files(self):
        build = jobs()["build_windows"]
        self.assertIn("foreach ($file in $inventory.files)", build)
        self.assertNotIn("-Recurse", build)
        self.assertIn("--bundle $staging --executable $executable", build)
        self.assertIn("--bundle $packageRoot --executable $builtExecutable", build)

    def test_artifact_paths_are_explicit_and_exclude_raw_qa(self):
        build = jobs()["build_windows"]
        paths = re.findall(r"^          path: \|\n((?:            .+\n)+)", build, re.MULTILINE)
        self.assertEqual(len(paths), 2)
        for block in paths:
            self.assertNotIn("*", block)
            for forbidden in (".fsreplay", "qa/", ".csv", "target/", "assets/", "workspace"):
                self.assertNotIn(forbidden, block)
        self.assertIn("flightsim-windows-smoke.stdout.log", paths[0])
        self.assertIn("flightsim-windows-smoke-swift.stderr.log", paths[0])
        self.assertIn("release-artifact/${{ steps.version.outputs.bundle }}", paths[1])

    def test_publisher_only_runs_inline_verification_of_current_run_artifact(self):
        publish = jobs()["publish"]
        self.assertNotIn("actions/checkout", publish)
        self.assertNotIn("scripts/", publish)
        self.assertIn("windows-release-${{ github.run_id }}-${{ github.run_attempt }}", publish)
        for field in ("release_inventory_sha256", "authorization_sha256"):
            self.assertIn(f"jq -er '.{field}'", publish)
        self.assertIn("contents: write", publish)
        self.assertNotIn("contents: write", jobs()["build_windows"])


if __name__ == "__main__":
    unittest.main()
