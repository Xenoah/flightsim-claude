"""Portable staging guards: no compilation, graphics, network or release needed."""
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch


SCRIPT = Path(__file__).parents[1] / "stage-commercial-candidate.py"
spec = importlib.util.spec_from_file_location("stage_commercial", SCRIPT)
stage = importlib.util.module_from_spec(spec)
spec.loader.exec_module(stage)


class CommercialCandidateTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.repo = self.root / "repo"
        self.notices = self.root / "notices"
        self.output = self.root / "candidate"
        self.executable = self.root / "flightsim-app"
        self.executable.write_bytes(b"synthetic executable fixture, never executed")
        for relative in stage.SOURCE_FILES + ("scripts/check-commercial-readiness.py",):
            path = self.repo / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(("fixture: " + relative).encode())
        # A recursive assets/ copy would leak this unverified asset and secret.
        (self.repo / "assets/aircraft/light_single.glb").write_bytes(b"EXCLUDED MESHy")
        for relative in ("assets/aircraft/light_single.json", "assets/aircraft/swift_sport.blend"):
            (self.repo / relative).write_bytes(b"original source excluded from commercial candidate")
        (self.repo / ".env").write_bytes(b"DO NOT PACKAGE")
        license_path = self.notices / "licenses/example-1.0/LICENSE"
        license_path.parent.mkdir(parents=True)
        license_path.write_text("synthetic license fixture", encoding="utf-8")
        (self.notices / "README.txt").write_text("Dependency notices, not legal approval", encoding="utf-8")
        (self.notices / stage.INVENTORY).write_text(json.dumps({
            "target": "x86_64-unknown-linux-gnu", "review_status": "not_reviewed",
            "packages": [{"notices": [{"path": "licenses/example-1.0/LICENSE",
                "sha256": hashlib.sha256(license_path.read_bytes()).hexdigest()}]}],
        }), encoding="utf-8")
        self.info = {
            "schema_version": 1, "package": "flightsim-app", "package_version": "0.6.0-alpha.21",
            "profile": "commercial-staging", "default_aircraft": "swift-sport",
            "default_model": "aircraft/swift_sport.glb", "bundled_aircraft": ["swift-sport"],
            "release_authorized": False, "target_os": "linux", "target_arch": "x86_64", "target_env": "gnu",
        }
        self.report = {
            "schema_version": 1, "status": "blocked", "warnings": [],
            "blockers": [{"code": "review", "category": "review", "message": "Review needed"}],
        }
        self.gate_exit = 2

    def run_process(self, command, **_kwargs):
        if command[-1] == "--distribution-info":
            return subprocess.CompletedProcess(command, 0, json.dumps(self.info).encode(), b"")
        self.assertIn("--json", command)
        self.assertIn("--repo", command)
        bundle = Path(command[command.index("--bundle") + 1])
        self.assertTrue((bundle / "assets/aircraft/swift_sport.glb").is_file())
        self.assertFalse((bundle / "assets/aircraft/light_single.glb").exists())
        return subprocess.CompletedProcess(command, self.gate_exit, json.dumps(self.report), "")

    def assemble(self):
        with patch.object(stage.subprocess, "run", side_effect=self.run_process):
            return stage.stage(self.repo, self.executable, self.notices, self.output)

    def test_blocked_review_candidate_keeps_all_hashes_and_excludes_unapproved_assets(self):
        self.assertTrue(self.assemble())
        self.assertFalse((self.output / "assets/aircraft/light_single.glb").exists())
        self.assertFalse((self.output / "assets/aircraft/light_single.json").exists())
        self.assertFalse((self.output / "assets/aircraft/swift_sport.blend").exists())
        self.assertFalse((self.output / ".env").exists())
        self.assertTrue((self.repo / "assets/aircraft/light_single.glb").is_file())
        manifest = json.loads((self.output / "bundle-manifest.json").read_text())
        self.assertEqual(manifest["readiness_gate"], "blocked")
        self.assertFalse(manifest["release_authorized"])
        indexed = {item["path"] for item in manifest["files"]}
        actual = {p.relative_to(self.output).as_posix() for p in self.output.rglob("*") if p.is_file()}
        self.assertEqual(indexed, actual - {"bundle-manifest.json"})
        for item in manifest["files"]:
            contents = (self.output / item["path"]).read_bytes()
            self.assertEqual(item["sha256"], hashlib.sha256(contents).hexdigest())
            self.assertEqual(item["bytes"], len(contents))
        self.assertIn("READINESS GATE BLOCKED", (self.output / "LOCAL-CANDIDATE.txt").read_text())

    def test_successful_mechanical_gate_never_claims_release_authorization(self):
        self.gate_exit = 0
        self.report.update(status="checks_passed", blockers=[])
        self.assertFalse(self.assemble())
        self.assertIn("release approval is still required", (self.output / "LOCAL-CANDIDATE.txt").read_text())
        manifest = json.loads((self.output / "bundle-manifest.json").read_text())
        self.assertFalse(manifest["release_authorized"])

    def test_development_executable_is_rejected(self):
        self.info["profile"] = "development"
        with self.assertRaisesRegex(ValueError, "commercial-staging build"):
            self.assemble()
        self.assertFalse(self.output.exists())

    def test_missing_required_font_notice_refuses_candidate(self):
        (self.repo / "docs/release/licenses/FiraMono-LICENSE").unlink()
        with self.assertRaisesRegex(ValueError, "required file is missing"):
            self.assemble()
        self.assertFalse(self.output.exists())

    def test_wrong_target_dependency_inventory_refuses_candidate(self):
        (self.notices / stage.INVENTORY).write_text('{"target":"x86_64-pc-windows-msvc"}')
        with self.assertRaisesRegex(ValueError, "inventory target"):
            self.assemble()
        self.assertFalse(self.output.exists())

    def test_windows_gnu_and_msvc_inventories_are_not_interchangeable(self):
        self.info["target_os"] = "windows"
        self.info["target_env"] = "gnu"
        (self.notices / stage.INVENTORY).write_text('{"target":"x86_64-pc-windows-msvc"}')
        with self.assertRaisesRegex(ValueError, "x86_64-pc-windows-gnu"):
            self.assemble()

    def test_wrongly_typed_metadata_and_inventory_are_rejected(self):
        self.info["release_authorized"] = 0
        with self.assertRaisesRegex(ValueError, "commercial-staging build"):
            self.assemble()
        self.info["release_authorized"] = False
        (self.notices / stage.INVENTORY).write_text('[]')
        with self.assertRaisesRegex(ValueError, "JSON object"):
            self.assemble()

    def test_failed_checker_cannot_create_a_candidate(self):
        self.gate_exit = 1
        with self.assertRaisesRegex(ValueError, "failed unexpectedly"):
            self.assemble()
        self.assertFalse(self.output.exists())

    def test_unknown_dependency_notice_file_is_not_silently_copied(self):
        (self.notices / "unreviewed.glb").write_bytes(b"not a notice")
        with self.assertRaisesRegex(ValueError, "unexpected dependency-notice file"):
            self.assemble()

    def test_unreferenced_renamed_mesh_under_license_tree_is_rejected(self):
        destination = self.notices / "licenses/extra/NOTICE"
        destination.parent.mkdir()
        destination.write_bytes((self.repo / "assets/aircraft/light_single.glb").read_bytes())
        with self.assertRaisesRegex(ValueError, "unexpected dependency-notice file"):
            self.assemble()
        self.assertFalse(self.output.exists())

    def test_unreferenced_text_notice_is_not_silently_included(self):
        (self.notices / "licenses/example-1.0/UNREVIEWED").write_text("unreviewed extra text")
        with self.assertRaisesRegex(ValueError, "unexpected dependency-notice file"):
            self.assemble()

    def test_collector_readme_cannot_hide_a_binary_asset(self):
        (self.notices / "README.txt").write_bytes(b"glTF\x00hidden binary")
        with self.assertRaisesRegex(ValueError, "binary/NUL"):
            self.assemble()

    def test_inventory_cannot_relabel_binary_asset_as_license_text(self):
        path = self.notices / "licenses/example-1.0/LICENSE"
        path.write_bytes(b"glTF\x00binary model fixture")
        inventory_path = self.notices / stage.INVENTORY
        inventory = json.loads(inventory_path.read_text())
        inventory["packages"][0]["notices"][0]["sha256"] = hashlib.sha256(path.read_bytes()).hexdigest()
        inventory_path.write_text(json.dumps(inventory))
        with self.assertRaisesRegex(ValueError, "binary/NUL"):
            self.assemble()

    def test_excluded_asset_bytes_are_fatal_even_if_checker_calls_them_review(self):
        self.report["blockers"][0]["code"] = "UNRESOLVED_ASSET_RIGHTS"
        with self.assertRaisesRegex(ValueError, "integrity error"):
            self.assemble()
        self.assertFalse(self.output.exists())

    def test_asset_symlink_is_rejected(self):
        model = self.repo / "assets/aircraft/swift_sport.glb"
        model.unlink()
        model.symlink_to(self.repo / "assets/aircraft/light_single.glb")
        with self.assertRaisesRegex(ValueError, "symlink"):
            self.assemble()

    def test_dependency_directory_symlink_is_rejected(self):
        (self.notices / "licenses/extra").symlink_to(self.repo / "assets", target_is_directory=True)
        with self.assertRaisesRegex(ValueError, "symlink"):
            self.assemble()

    def test_integrity_blocker_refuses_staging_instead_of_labeling_review_only(self):
        self.report["blockers"][0]["category"] = "integrity"
        with self.assertRaisesRegex(ValueError, "integrity error"):
            self.assemble()
        self.assertFalse(self.output.exists())

    def test_gate_exit_status_must_agree_with_json(self):
        self.report["status"] = "checks_passed"
        with self.assertRaisesRegex(ValueError, "status disagrees"):
            self.assemble()

    def test_existing_candidate_is_not_overwritten(self):
        self.output.mkdir()
        existing = self.output / "keep.txt"
        existing.write_text("previous review")
        with self.assertRaisesRegex(ValueError, "output already exists"):
            self.assemble()
        self.assertEqual(existing.read_text(), "previous review")

    def test_traversal_and_absolute_paths_are_rejected(self):
        for path in ("../secret", "/secret", "C:/secret", "nested\\secret"):
            with self.assertRaisesRegex(ValueError, "unsafe"):
                stage.safe_file(self.repo, path)


if __name__ == "__main__":
    unittest.main()
