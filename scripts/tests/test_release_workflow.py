"""Release workflow wiring: these guards need no Cargo, network or real approval.

These are focused contract assertions, not a replacement for GitHub/actionlint's
YAML/expression validator. Keep source CI separate from optional publication.
"""
from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/release.yml"
PREPARATION = ROOT / 'scripts/prepare-ordinary-full-release.py'


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
        self.assertLess(build.index("scripts/check-release-authorization.py"), build.index("scripts/capture-analytical-swift-msvc.py"))
        self.assertNotIn('cargo build', build)
        self.assertNotIn("--allow-blocked", build)
        for field in ("release_inventory_sha256", "source_inventory_sha256", "authorization_sha256"):
            self.assertIn("$gate." + field + " -ne $env:EXPECTED_", build)

    def test_copy_plan_is_the_only_source_of_package_files(self):
        build, preparation = jobs()["build_windows"], PREPARATION.read_text()
        self.assertIn("for item in plan['files']:", preparation)
        self.assertNotIn("-Recurse", build)
        self.assertIn('release.verify_bundle(staged, plan, executable)', preparation)
        self.assertIn('release.verify_bundle(extracted, plan, executable)', preparation)
        self.assertIn("build_private / 'target-ordinary' / release.TARGET / 'release/flightsim-app.exe'", preparation)

    def test_offline_identity_runs_only_on_trusted_build_before_packaging(self):
        build = jobs()["build_windows"]
        preparation = PREPARATION.read_text().split('def prepare(', 1)[1]
        self.assertLess(build.index('scripts/capture-analytical-swift-msvc.py'), build.index('scripts/prepare-ordinary-full-release.py'))
        self.assertLess(preparation.index('capture.validate_export('), preparation.index('release.verify_distribution_info('))
        self.assertLess(preparation.index('release.verify_distribution_info('), preparation.index('shutil.copyfile(executable,'))
        self.assertNotIn("--verify-built-executable", jobs()["authorize"])
        self.assertNotIn("--verify-built-executable", jobs()["publish"])
        self.assertNotIn('--features', build)
        self.assertIn('--validate-only', build)

    def test_artifact_paths_are_explicit_and_exclude_raw_qa(self):
        build = jobs()["build_windows"]
        paths = re.findall(r"^          path: \|\n((?:            .+\n)+)", build, re.MULTILINE)
        self.assertEqual(len(paths), 3)
        for block in paths:
            self.assertNotIn("*", block)
            for forbidden in (".fsreplay", "qa/", ".csv", "target/", "assets/", "workspace"):
                self.assertNotIn(forbidden, block)
        self.assertIn('ordinary-failed-native-facts.json', paths[0])
        self.assertIn('ordinary-native-review.json', paths[1])
        self.assertIn('ordinary-release-evidence.json', paths[1])
        self.assertNotIn('.log', ''.join(paths))
        self.assertIn("ordinary-release-artifact/${{ steps.version.outputs.bundle }}", paths[2])
        self.assertIn('ordinary-release-evidence.json', paths[2])

    def test_failure_fact_export_revalidates_and_never_enters_publication_artifact(self):
        build = jobs()['build_windows']
        self.assertIn("if: failure() && steps.native_capture.outcome == 'success' && steps.version.outcome == 'success'", build)
        self.assertIn("if: failure() && steps.failure_native_facts.outcome == 'success'", build)
        block = build.split('- name: Revalidate bounded native facts after a failed final check', 1)[1]
        block = block.split('- name: Retain successful bounded ordinary native evidence', 1)[0]
        self.assertIn('scripts/project-ordinary-native-evidence.py', block)
        for arg in ('--expected-sha', '--build-private', '--build-text', '--bundle', '--runtime-facts-private'):
            self.assertIn(arg, block)
        self.assertIn('if ($LASTEXITCODE -ne 0)', block)
        self.assertNotIn('continue-on-error', block)
        self.assertNotIn('ordinary-release-artifact/', block)
        self.assertNotIn('ordinary-failed-native', jobs()['publish'])

    def test_publisher_only_runs_inline_verification_of_current_run_artifact(self):
        publish = jobs()["publish"]
        self.assertNotIn("actions/checkout", publish)
        self.assertNotIn("scripts/", publish)
        self.assertIn("windows-release-${{ github.run_id }}-${{ github.run_attempt }}", publish)
        for field in ("release_inventory_sha256", "authorization_sha256"):
            self.assertIn(f"jq -er '.{field}'", publish)
        self.assertIn("contents: write", publish)
        self.assertNotIn("contents: write", jobs()["build_windows"])

    def test_final_evidence_digest_rechecked_by_nonexecuting_publisher(self):
        publish = jobs()['publish']
        for binding in ('.evidence_sha256', '.bindings.archive.sha256', '.applicability.conditions_matched',
                        'ordinary_final_same_build_evidence_not_authority'):
            self.assertIn(binding, publish)
        self.assertIn('light-single-cockpit', publish)
        self.assertIn('swift-sport-chase', publish)
        self.assertLess(publish.index('evidence=release-artifact/ordinary-release-evidence.json'),
                        publish.index('Create the tag if absent'))

    def test_component_disclosure_is_bound_and_published_before_recipient_choice(self):
        build, publish = jobs()['build_windows'], jobs()['publish']
        self.assertIn('ordinary-release-artifact/release-notes.md', build)
        for field in ('.notes_sha256', '.bindings.publication_notes.sha256',
                      '.component_terms.recipient_assent_collected == false',
                      '.component_terms.dialog_tested_by_smoke == false'):
            self.assertIn(field, publish)
        self.assertIn("! grep -q '@SOURCE_SHA@'", publish)
        self.assertEqual(publish.count('--notes-file "$notes"'), 2)
        self.assertIn('"$swift_screenshot" "$evidence" --repo', publish)
        self.assertNotIn('--generate-notes', publish)
        self.assertNotIn('actions/checkout', publish)

    def test_windows_dialog_contract_is_separate_from_scene_smoke(self):
        test = './scripts/tests/test-component-terms-dialog.ps1'
        build = jobs()['build_windows']
        self.assertIn(test, build)
        self.assertLess(build.index('scripts/check-release-authorization.py'), build.index(test))
        self.assertLess(build.index(test), build.index('scripts/capture-analytical-swift-msvc.py'))
        ci = (ROOT / '.github/workflows/ci.yml').read_text()
        self.assertIn("if: runner.os == 'Windows'\n        shell: pwsh\n        run: " + test, ci)

    def test_native_capture_checkout_is_canonical_lf_on_windows(self):
        workflow = WORKFLOW.read_text()
        for expected in ("GIT_CONFIG_COUNT: '2'", 'GIT_CONFIG_KEY_0: core.autocrlf',
                         "GIT_CONFIG_VALUE_0: 'false'", 'GIT_CONFIG_KEY_1: core.eol',
                         "GIT_CONFIG_VALUE_1: 'lf'"):
            self.assertIn(expected, workflow.split('jobs:', 1)[0])


if __name__ == "__main__":
    unittest.main()
