"""Synthetic release approvals only; no real binaries, grants or network writes."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "check-release-authorization.py"
spec = importlib.util.spec_from_file_location("release_gate", SCRIPT)
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)


class ReleaseAuthorizationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / "repo"
        self.repo.mkdir()
        self.git("init", "-q")
        self.git("config", "user.name", "SYNTHETIC UNIT TEST")
        self.git("config", "user.email", "fixture@example.invalid")
        self.git("config", "core.autocrlf", "false")
        for relative in gate.SOURCE_FILES:
            self.write(relative, "synthetic source: " + relative)
        self.write("Cargo.toml", '[workspace.package]\nversion = "1.2.3"\n')
        self.write("Cargo.lock", "synthetic dependency lock")
        self.write("docs/release/.gitattributes", (SCRIPT.parents[1] / "docs/release/.gitattributes").read_text())
        self.write(".github/workflows/release.yml", "synthetic recipe, not a workflow")
        self.write("crates/flightsim-app/src/main.rs", "synthetic application source")
        self.manifest = {
            "schema_version": 1,
            "assets": [{"path": p, "review_state": "original_source_recorded", "sha256": gate.readiness.digest(self.repo / p)}
                       for p in gate.SOURCE_FILES if p.startswith("assets/")],
            "commercial_external_assets": [p for p in gate.SOURCE_FILES if p.startswith("assets/")],
            "required_bundle_files": ["LICENSE-MIT", "LICENSE-APACHE", gate.ASSET_MANIFEST],
        }
        self.write_json(gate.ASSET_MANIFEST, self.manifest)
        self.notice = Path(gate.DEPENDENCY_INVENTORY).parent / "licenses/test/LICENSE"
        self.write(self.notice, "Synthetic notice, not a real license")
        self.inventory = {
            "schema_version": 1, "kind": "cargo-dependency-notices", "target": gate.TARGET,
            "root_package": "flightsim-app", "cargo_lock_sha256": gate.readiness.digest(self.repo / "Cargo.lock"),
            "asset_manifest_sha256": gate.readiness.digest(self.repo / gate.ASSET_MANIFEST),
            "packages": [{"id": "flightsim-app@1.2.3", "name": "flightsim-app", "version": "1.2.3", "features": ["default"],
                          "notices": [{"path": "licenses/test/LICENSE", "sha256": gate.readiness.digest(self.repo / self.notice)}]}],
            "embedded_assets": [], "unresolved": [],
        }
        self.write_json(gate.DEPENDENCY_INVENTORY, self.inventory)
        self.review = {
            "schema_version": 1, "status": "reviewed", "reviewed_by": "UNIT TEST FIXTURE, NOT REAL APPROVAL",
            "reviewed_at": "2000-01-01", "scope": "Synthetic fixture only", "resolutions": [],
            "inventory_sha256": gate.readiness.digest(self.repo / gate.DEPENDENCY_INVENTORY),
        }
        self.write_json(gate.DEPENDENCY_REVIEW, self.review)
        self.commit()

    def git(self, *args):
        return subprocess.run(["git", "-C", str(self.repo), *args], capture_output=True, check=True)

    def write(self, relative, contents):
        path = self.repo / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents, encoding="utf-8")

    def write_json(self, relative, value):
        self.write(relative, json.dumps(value))

    def commit(self):
        self.git("add", ".")
        self.git("commit", "--allow-empty", "-qm", "Synthetic fixture only")

    def refresh_evidence(self):
        self.write_json(gate.ASSET_MANIFEST, self.manifest)
        self.inventory["asset_manifest_sha256"] = gate.readiness.digest(self.repo / gate.ASSET_MANIFEST)
        self.write_json(gate.DEPENDENCY_INVENTORY, self.inventory)
        self.review["inventory_sha256"] = gate.readiness.digest(self.repo / gate.DEPENDENCY_INVENTORY)
        self.write_json(gate.DEPENDENCY_REVIEW, self.review)
        self.commit()

    def receipt(self):
        result, _ = gate.inspect(self.repo)
        return {"schema_version": 1, "status": "authorized", "scope": "github-windows-prerelease",
                "authorized_by": "UNIT TEST FIXTURE, NOT REAL APPROVAL", "authorized_at": "2000-01-01T00:00:00Z",
                "evidence": ["Synthetic fixture only"], "version": result["version"], "target": gate.TARGET,
                "source_inventory_sha256": result["source_inventory_sha256"],
                "release_inventory_sha256": result["release_inventory_sha256"]}

    def authorize_fixture(self):
        self.write_json(gate.AUTHORIZATION, self.receipt())
        self.commit()

    def codes(self):
        result, _ = gate.inspect(self.repo)
        return {item["code"] for item in result["blockers"]}

    def test_missing_authorization_never_writes_a_copy_plan(self):
        output, summary, plan = (self.root / name for name in ("outputs", "summary", "inventory.json"))
        with contextlib.redirect_stdout(io.StringIO()):
            status = gate.main(["--repo", str(self.repo), "--allow-blocked", "--github-output", str(output),
                                "--github-summary", str(summary), "--inventory-file", str(plan)])
        self.assertEqual(status, 0)
        self.assertIn("authorized=false\n", output.read_text())
        self.assertIn("Binary publication: blocked", summary.read_text())
        self.assertFalse(plan.exists())
        self.assertIn("AUTHORIZATION_MISSING", self.codes())

    def test_strict_cli_returns_blocked_exit_two(self):
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(gate.main(["--repo", str(self.repo)]), 2)

    def test_valid_synthetic_authorization_is_reachable_without_permanent_false(self):
        before, _ = gate.inspect(self.repo)
        self.authorize_fixture()
        result, plan = gate.inspect(self.repo)
        self.assertTrue(result["authorized"], result)
        self.assertEqual(before["source_inventory_sha256"], result["source_inventory_sha256"])
        self.assertTrue(result["authorization_sha256"])
        self.assertIn("third-party/dependency-inventory.json", {x["path"] for x in plan["files"]})
        output = self.root / "plan.json"
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(gate.main(["--repo", str(self.repo), "--inventory-file", str(output)]), 0)
        self.assertEqual(json.loads(output.read_text()), plan)

    def test_untracked_receipt_cannot_authorize(self):
        self.write_json(gate.AUTHORIZATION, self.receipt())
        self.assertIn("AUTHORIZATION_MISSING", self.codes())

    def test_tracked_source_change_invalidates_even_same_version(self):
        self.authorize_fixture()
        self.write("crates/flightsim-app/src/main.rs", "changed flight implementation")
        self.commit()
        self.assertIn("AUTHORIZATION_STALE_OR_INVALID", self.codes())

    def test_release_recipe_change_invalidates_authorization(self):
        self.authorize_fixture()
        self.write(".github/workflows/release.yml", "different recipe or permissions")
        self.commit()
        self.assertIn("AUTHORIZATION_STALE_OR_INVALID", self.codes())

    def test_version_change_invalidates_receipt_and_dependency_build(self):
        self.authorize_fixture()
        self.write("Cargo.toml", '[workspace.package]\nversion = "1.2.4"\n')
        self.commit()
        self.assertTrue({"AUTHORIZATION_STALE_OR_INVALID", "DEPENDENCY_BUILD_MISMATCH"} <= self.codes())

    def test_dirty_source_fails_closed_before_outputs(self):
        self.authorize_fixture()
        self.write("Cargo.lock", "uncommitted dependency change")
        output = self.root / "output"
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(gate.main(["--repo", str(self.repo), "--allow-blocked", "--github-output", str(output)]), 1)
        self.assertFalse(output.exists())

    def test_current_unresolved_mesh_cannot_be_authorized_by_a_receipt(self):
        self.manifest["assets"][0]["review_state"] = "unresolved"
        self.manifest["commercial_external_assets"].remove(gate.SOURCE_FILES[0])
        self.refresh_evidence()
        self.authorize_fixture()
        self.assertTrue({"UNRESOLVED_ASSET_RIGHTS", "ASSET_NOT_ALLOWLISTED"} <= self.codes())

    def test_renamed_unresolved_asset_in_notice_is_rejected(self):
        # A text-like asset cannot hide under a referenced license path either.
        self.manifest["assets"][0]["review_state"] = "unresolved"
        data = (self.repo / gate.SOURCE_FILES[0]).read_text()
        self.write(self.notice, data)
        self.inventory["packages"][0]["notices"][0]["sha256"] = gate.readiness.digest(self.repo / self.notice)
        self.refresh_evidence()
        self.authorize_fixture()
        result, _ = gate.inspect(self.repo)
        self.assertTrue(any(b["code"] == "UNRESOLVED_ASSET_RIGHTS" and str(self.notice) in b["message"] for b in result["blockers"]))

    def test_missing_review_is_not_waived_by_authorization(self):
        (self.repo / gate.DEPENDENCY_REVIEW).unlink()
        self.commit()
        self.authorize_fixture()
        self.assertIn("DEPENDENCY_REVIEW_MISSING", self.codes())

    def test_unresolved_dependency_survives_truncated_summary_and_receipt(self):
        self.inventory["packages"][0]["unresolved"] = ["Missing primary grant"]
        self.refresh_evidence()
        self.authorize_fixture()
        self.assertTrue({"DEPENDENCY_UNRESOLVED", "DEPENDENCY_UNRESOLVED_SUMMARY_INCONSISTENT"} <= self.codes())

    def test_embedded_unresolved_asset_survives_truncated_inventory_summary(self):
        self.manifest["dependency_assets"] = [{"id": "unresolved-lut", "package": "engine", "version": "1.0",
            "feature": "lut", "sha256": "a" * 64, "review_state": "unresolved", "reason": "Exact LUT source grant absent"}]
        self.inventory["packages"].append({"id": "engine@1.0", "name": "engine", "version": "1.0", "features": ["lut"]})
        self.inventory["embedded_assets"] = [{"id": "unresolved-lut", "observed_sha256": "a" * 64}]
        self.refresh_evidence()
        self.authorize_fixture()
        self.assertIn("DEPENDENCY_UNRESOLVED", self.codes())

    def test_unresolved_embedded_source_asset_is_not_hidden_by_external_copy_plan(self):
        self.write("crates/world/data/unreviewed.bin", "Synthetic embedded source data")
        self.manifest["assets"].append({"path": "crates/world/data/unreviewed.bin", "delivery": "embedded",
            "review_state": "unresolved", "sha256": gate.readiness.digest(self.repo / "crates/world/data/unreviewed.bin")})
        self.refresh_evidence()
        self.authorize_fixture()
        self.assertIn("UNRESOLVED_EMBEDDED_ASSET", self.codes())

    def test_wrong_target_and_feature_are_rejected(self):
        self.inventory["target"] = "x86_64-pc-windows-gnu"
        self.inventory["packages"][0]["features"] = ["default", "commercial-staging"]
        self.refresh_evidence()
        self.authorize_fixture()
        self.assertTrue({"DEPENDENCY_TARGET_MISMATCH", "DEPENDENCY_BUILD_MISMATCH"} <= self.codes())

    def test_exact_upstream_crlf_notice_bytes_survive_git_commit(self):
        self.git("config", "core.autocrlf", "true")
        contents = b"Synthetic upstream notice\r\nCopyright fixture\r\n"
        (self.repo / self.notice).write_bytes(contents)
        self.inventory["packages"][0]["notices"][0]["sha256"] = gate.readiness.digest(self.repo / self.notice)
        self.refresh_evidence()
        stored = self.git("show", "HEAD:" + self.notice.as_posix()).stdout
        self.assertEqual(stored, contents)
        self.authorize_fixture()
        self.assertTrue(gate.inspect(self.repo)[0]["authorized"])

    def test_changed_notice_needs_new_evidence_even_with_fresh_receipt(self):
        self.write(self.notice, "unreviewed replacement text")
        self.commit()
        # inspect itself must reject the notice before a receipt can be prepared.
        with self.assertRaisesRegex(ValueError, "hash mismatch"):
            gate.inspect(self.repo)

    def test_symlink_input_fails_closed(self):
        path = self.repo / gate.SOURCE_FILES[0]
        path.unlink()
        path.symlink_to(Path(gate.SOURCE_FILES[1]).name)
        self.commit()
        with self.assertRaisesRegex(ValueError, "symlink"):
            gate.inspect(self.repo)

    def test_malformed_receipt_fails_closed(self):
        self.write(gate.AUTHORIZATION, "not JSON")
        self.commit()
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(gate.main(["--repo", str(self.repo), "--allow-blocked"]), 1)

    def test_boolean_schema_or_missing_human_evidence_cannot_authorize(self):
        receipt = self.receipt()
        receipt.update(schema_version=True, authorized_by=" ", authorized_at="bad date", evidence=[])
        self.write_json(gate.AUTHORIZATION, receipt)
        self.commit()
        self.assertTrue({"AUTHORIZATION_STALE_OR_INVALID", "AUTHORIZATION_EVIDENCE_MISSING", "AUTHORIZATION_DATE_INVALID"} <= self.codes())

    def test_untracked_telemetry_or_secret_is_never_included(self):
        self.authorize_fixture()
        self.write("assets/aircraft/raw-qa.fsreplay", "private telemetry")
        self.write("assets/secret.env", "do not publish")
        result, plan = gate.inspect(self.repo)
        self.assertTrue(result["authorized"])
        self.assertFalse(any(x["source"].endswith((".fsreplay", ".env")) for x in plan["files"]))

    def test_packaged_raw_telemetry_is_blocked_even_if_required_list_requests_it(self):
        self.write("raw-qa.fsreplay", "private telemetry")
        self.manifest["required_bundle_files"].append("raw-qa.fsreplay")
        self.refresh_evidence()
        self.authorize_fixture()
        self.assertIn("RAW_DATA_EXCLUDED", self.codes())

    def test_scenery_data_is_blocked_even_when_requested_by_manifest(self):
        self.write("region.fsscenery", "FSSC synthetic database")
        self.manifest["required_bundle_files"].append("region.fsscenery")
        self.refresh_evidence()
        self.authorize_fixture()
        self.assertIn("RAW_DATA_EXCLUDED", self.codes())

    def test_renamed_scenery_is_still_blocked_by_magic(self):
        self.write("notice-extra.txt", "FSSC synthetic database")
        self.manifest["required_bundle_files"].append("notice-extra.txt")
        self.refresh_evidence()
        self.authorize_fixture()
        self.assertIn("RAW_DATA_EXCLUDED", self.codes())

    def make_bundle(self):
        self.authorize_fixture()
        _, plan = gate.inspect(self.repo)
        bundle = self.root / "bundle"
        bundle.mkdir()
        for item in plan["files"]:
            destination = bundle / item["path"]
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(self.repo / item["source"], destination)
        executable = self.root / "flightsim-app.exe"
        executable.write_bytes(b"synthetic binary never executed")
        shutil.copyfile(executable, bundle / executable.name)
        return bundle, plan, executable

    def test_exact_staged_and_extracted_payload_can_be_verified(self):
        bundle, plan, executable = self.make_bundle()
        gate.verify_bundle(bundle, plan, executable)
        archive = shutil.make_archive(str(self.root / "bundle"), "zip", bundle)
        extracted = self.root / "extracted"
        shutil.unpack_archive(archive, extracted)
        gate.verify_bundle(extracted, plan, executable)

    def test_extra_changed_or_missing_bundle_file_is_rejected(self):
        bundle, plan, executable = self.make_bundle()
        raw = bundle / "raw-qa.fsreplay"
        raw.write_bytes(b"excluded recording")
        with self.assertRaisesRegex(ValueError, "extra or changed"):
            gate.verify_bundle(bundle, plan, executable)
        raw.unlink()
        (bundle / "LICENSE-MIT").write_text("changed")
        with self.assertRaisesRegex(ValueError, "extra or changed"):
            gate.verify_bundle(bundle, plan, executable)
        (bundle / "LICENSE-MIT").unlink()
        with self.assertRaisesRegex(ValueError, "exactly the authorized"):
            gate.verify_bundle(bundle, plan, executable)

    def test_changed_executable_or_symlink_is_rejected(self):
        bundle, plan, executable = self.make_bundle()
        (bundle / executable.name).write_bytes(b"other binary")
        with self.assertRaisesRegex(ValueError, "extra or changed"):
            gate.verify_bundle(bundle, plan, executable)
        (bundle / executable.name).unlink()
        (bundle / executable.name).symlink_to(executable)
        with self.assertRaisesRegex(ValueError, "symlink"):
            gate.verify_bundle(bundle, plan, executable)


if __name__ == "__main__":
    unittest.main()
