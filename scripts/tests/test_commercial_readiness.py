"""No Rust builds, external services, or real approvals in these tests."""

import importlib.util
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SCRIPTS = Path(__file__).resolve().parents[1]


def load_script(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), SCRIPTS / f"{name}.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


gate = load_script("check-commercial-readiness")
collector = load_script("collect-dependency-notices")


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value))


class StagingChecks(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / "repo"
        self.bundle = self.root / "bundle"
        self.repo.mkdir()
        self.bundle.mkdir()
        self.asset = "assets/aircraft/swift_sport.glb"
        self.legacy = "assets/aircraft/light_single.glb"
        for relative, data in [(self.asset, b"original test geometry"), (self.legacy, b"unverified test asset"), ("LICENSE-MIT", b"test license"), ("Cargo.lock", b"test lock")]:
            path = self.repo / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        self.manifest = {
            "schema_version": 1,
            "assets": [{"path": relative, "review_state": state, "sha256": gate.digest(self.repo / relative)} for relative, state in [(self.asset, "original_source_recorded"), (self.legacy, "unresolved")]],
            "commercial_external_assets": [self.asset],
            "required_bundle_files": ["LICENSE-MIT", "docs/release/asset-rights-manifest.json"],
        }
        self.manifest_path = self.repo / "docs/release/asset-rights-manifest.json"
        write_json(self.manifest_path, self.manifest)
        for relative in [self.asset, *self.manifest["required_bundle_files"]]:
            path = self.bundle / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(self.repo / relative, path)
        write_json(self.bundle / "distribution-info.json", {"schema_version": 1, "profile": "commercial-staging", "region_downloads": False, "default_aircraft": "swift-sport", "bundled_aircraft": ["swift-sport"], "target_os": "windows", "target_arch": "x86_64", "target_env": "msvc"})
        self.inventory_path = self.bundle / "third-party/dependency-inventory.json"
        notice = self.bundle / "third-party/licenses/example/LICENSE"
        notice.parent.mkdir(parents=True)
        notice.write_text("Test-only notice")
        self.inventory = {"schema_version": 1, "kind": "cargo-dependency-notices", "target": "x86_64-pc-windows-msvc", "root_package": "flightsim-app", "cargo_lock_sha256": gate.digest(self.repo / "Cargo.lock"), "asset_manifest_sha256": gate.digest(self.manifest_path), "packages": [{"id": "example@1.0", "notices": [{"path": "licenses/example/LICENSE", "sha256": gate.digest(notice)}]}], "embedded_assets": [], "unresolved": []}
        write_json(self.inventory_path, self.inventory)
        self.inventory["packages"].append({"id": "flightsim-app@1.0", "name": "flightsim-app", "features": ["commercial-staging"], "notices": []})
        write_json(self.inventory_path, self.inventory)
        self.review_path = self.root / "synthetic-review.json"
        self.refresh_review()

    def refresh_review(self):
        write_json(self.review_path, {"schema_version": 1, "status": "reviewed", "reviewed_by": "UNIT TEST FIXTURE, NOT REAL APPROVAL", "reviewed_at": "2000-01-01", "scope": "Synthetic fixture only", "inventory_sha256": gate.digest(self.inventory_path), "resolutions": []})

    def result(self, review=True):
        return gate.check(self.repo, self.bundle, self.inventory_path, self.review_path if review else None)

    def codes(self, **kwargs):
        return {x["code"] for x in self.result(**kwargs)["blockers"]}

    def test_complete_synthetic_record_passes_mechanical_checks(self):
        self.assertEqual(self.result()["status"], "checks_passed")

    def test_collection_without_review_stays_blocked(self):
        self.assertIn("DEPENDENCY_REVIEW_REQUIRED", self.codes(review=False))

    def test_offline_and_network_binary_inventory_combinations(self):
        path = self.bundle / "distribution-info.json"
        info = json.loads(path.read_text())
        for binary_network in (False, True):
            for inventory_network in (False, True):
                with self.subTest(binary=binary_network, inventory=inventory_network):
                    info["region_downloads"] = binary_network
                    write_json(path, info)
                    features = ["default", "commercial-staging"]
                    if inventory_network:
                        features.append("region-downloads")
                    self.inventory["packages"][1]["features"] = features
                    write_json(self.inventory_path, self.inventory)
                    self.refresh_review()
                    codes = self.codes()
                    self.assertEqual("DISTRIBUTION_FEATURE_UNVERIFIED" in codes, binary_network)
                    self.assertEqual("DEPENDENCY_FEATURE_MISMATCH" in codes, inventory_network)
                    self.assertEqual(not codes, not binary_network and not inventory_network)

    def test_missing_or_invalid_feature_identity_stays_blocked(self):
        path = self.bundle / "distribution-info.json"
        info = json.loads(path.read_text())
        for value in (None, 0, 0.0, "false", [], {}):
            with self.subTest(value=value):
                write_json(path, {**info, "region_downloads": value})
                self.assertIn("DISTRIBUTION_FEATURE_UNVERIFIED", self.codes())
        del info["region_downloads"]
        write_json(path, info)
        self.assertIn("DISTRIBUTION_FEATURE_UNVERIFIED", self.codes())
        path.unlink()
        self.assertIn("DISTRIBUTION_IDENTITY_MISSING", self.codes())

    def test_invalid_identity_document_fails_closed(self):
        path = self.bundle / "distribution-info.json"
        for value in (None, [], False, 0, "not an object"):
            with self.subTest(value=value):
                write_json(path, value)
                with self.assertRaisesRegex(ValueError, "JSON object"):
                    self.result()
        path.write_text("invalid JSON")
        with self.assertRaises(ValueError):
            self.result()

    def test_release_only_assets_are_neither_required_nor_allowed_in_commercial_candidate(self):
        release_only = ["assets/aircraft/light_single.json", "assets/aircraft/swift_sport.blend"]
        for relative in release_only:
            path = self.repo / relative
            path.write_bytes(("original source fixture: " + relative).encode())
            self.manifest["assets"].append({"path": relative, "review_state": "original_source_recorded", "sha256": gate.digest(path)})
        self.manifest["release_external_assets"] = [self.asset, *release_only]
        write_json(self.manifest_path, self.manifest)
        shutil.copyfile(self.manifest_path, self.bundle / "docs/release/asset-rights-manifest.json")
        self.inventory["asset_manifest_sha256"] = gate.digest(self.manifest_path)
        write_json(self.inventory_path, self.inventory)
        self.refresh_review()
        self.assertEqual(self.result()["status"], "checks_passed")
        for relative in release_only:
            destination = self.bundle / relative
            shutil.copyfile(self.repo / relative, destination)
            self.assertTrue(any(b["code"] == "UNAPPROVED_ASSET" and relative in b["message"] for b in self.result()["blockers"]))
            destination.unlink()

    def test_source_only_does_not_claim_bundle_passed(self):
        result = gate.check(self.repo, None)
        self.assertIn("BUNDLE_NOT_CHECKED", {b["code"] for b in result["blockers"]})

    def test_historical_meshy_hash_stays_denied_after_current_asset_replacement(self):
        historical = dict(next(a for a in self.manifest['assets'] if a['path'] == self.legacy))
        self.manifest['historical_excluded_assets'] = [historical]
        current = next(a for a in self.manifest['assets'] if a['path'] == self.legacy)
        (self.bundle/'renamed-historical.dat').write_bytes((self.repo/self.legacy).read_bytes())
        (self.repo/self.legacy).write_bytes(b'current original asset')
        current.update(review_state='original_source_recorded', sha256=gate.digest(self.repo/self.legacy))
        write_json(self.manifest_path, self.manifest)
        self.assertIn('UNRESOLVED_ASSET_RIGHTS', self.codes())

    def test_meshy_renamed_outside_assets_is_still_blocked(self):
        shutil.copyfile(self.repo / self.legacy, self.bundle / "unrelated.dat")
        self.assertIn("UNRESOLVED_ASSET_RIGHTS", self.codes())
        blocker = next(b for b in self.result()["blockers"] if b["code"] == "UNRESOLVED_ASSET_RIGHTS")
        self.assertEqual(blocker["category"], "integrity")

    def test_renamed_excluded_asset_in_unreferenced_notice_tree_is_fatal(self):
        path = self.bundle / "third-party/licenses/extra/NOTICE"
        path.parent.mkdir(parents=True)
        shutil.copyfile(self.repo / self.legacy, path)
        blocker = next(b for b in self.result()["blockers"] if b["code"] == "UNRESOLVED_ASSET_RIGHTS")
        self.assertEqual(blocker["category"], "integrity")

    def test_notice_cannot_be_binary_even_with_matching_hash(self):
        path = self.bundle / "third-party/licenses/example/LICENSE"
        path.write_bytes(b"binary\x00notice")
        self.inventory["packages"][0]["notices"][0]["sha256"] = gate.digest(path)
        write_json(self.inventory_path, self.inventory)
        self.refresh_review()
        self.assertIn("DEPENDENCY_NOTICE_NOT_TEXT", self.codes())

    def test_unknown_external_asset_blocks(self):
        (self.bundle / "assets/unknown.txt").write_text("unapproved")
        self.assertIn("UNAPPROVED_ASSET", self.codes())

    def test_renamed_binary_asset_directory_blocks(self):
        (self.bundle / "unknown.glb").write_text("unapproved")
        self.assertIn("UNINVENTORIED_BINARY_ASSET", self.codes())

    def test_osm_database_anywhere_is_not_silently_included(self):
        (self.bundle / "region.fsairports").write_bytes(b"FSAP")
        self.assertIn("UNAPPROVED_GEODATA", self.codes())

    def test_scenery_database_outside_assets_is_blocked(self):
        (self.bundle / "region.fsscenery").write_bytes(b"FSSC\x01\x00")
        self.assertIn("UNAPPROVED_GEODATA", self.codes())

    def test_renamed_scenery_database_cannot_hide_in_a_notice(self):
        (self.bundle / "README-extra.txt").write_bytes(b"FSSC\x01\x00")
        self.assertIn("UNAPPROVED_GEODATA", self.codes())

    def test_source_hash_change_requires_review(self):
        (self.repo / self.asset).write_bytes(b"replacement")
        self.assertIn("SOURCE_ASSET_CHANGED", self.codes())

    def test_bundled_asset_mutation_blocks(self):
        (self.bundle / self.asset).write_bytes(b"replacement")
        self.assertIn("BUNDLE_ASSET_CHANGED", self.codes())

    def test_required_notice_mutation_blocks(self):
        (self.bundle / "LICENSE-MIT").write_text("shortened")
        self.assertIn("REQUIRED_NOTICE_MISSING_OR_CHANGED", self.codes())

    def test_dependency_notice_mutation_blocks(self):
        (self.bundle / "third-party/licenses/example/LICENSE").write_text("shortened")
        self.assertIn("DEPENDENCY_NOTICE_CHANGED", self.codes())

    def test_dependency_review_must_bind_inventory_hash(self):
        self.inventory["unresolved"] = [{"id": "example@1.0", "reason": "new question"}]
        write_json(self.inventory_path, self.inventory)
        self.assertIn("DEPENDENCY_REVIEW_INVALID", self.codes())
        self.assertIn("DEPENDENCY_UNRESOLVED", self.codes())

    def test_unresolved_record_requires_evidence_even_after_review(self):
        self.inventory["unresolved"] = [{"id": "example@1.0", "reason": "grant not recorded"}]
        write_json(self.inventory_path, self.inventory)
        self.refresh_review()
        self.assertIn("DEPENDENCY_UNRESOLVED", self.codes())

    def test_truncated_summary_cannot_erase_package_unresolved_evidence(self):
        self.inventory["packages"][0]["unresolved"] = ["Known missing grant"]
        self.inventory["unresolved"] = []
        write_json(self.inventory_path, self.inventory)
        self.refresh_review()
        codes = self.codes()
        self.assertIn("DEPENDENCY_UNRESOLVED_SUMMARY_INCONSISTENT", codes)
        self.assertIn("DEPENDENCY_UNRESOLVED", codes)

    def test_truncated_summary_cannot_erase_enabled_manifest_asset_evidence(self):
        self.manifest["dependency_assets"] = [{"id": "known-lut", "package": "engine", "version": "1.0", "feature": "lut", "sha256": "a" * 64, "review_state": "unresolved", "reason": "Known LUT grant missing"}]
        write_json(self.manifest_path, self.manifest)
        shutil.copyfile(self.manifest_path, self.bundle / "docs/release/asset-rights-manifest.json")
        self.inventory["asset_manifest_sha256"] = gate.digest(self.manifest_path)
        self.inventory["packages"].append({"id": "engine@1.0", "name": "engine", "version": "1.0", "features": ["lut"], "notices": []})
        self.inventory["embedded_assets"] = [{"id": "known-lut", "observed_sha256": "a" * 64, "notices": []}]
        self.inventory["unresolved"] = []
        write_json(self.inventory_path, self.inventory)
        self.refresh_review()
        codes = self.codes()
        self.assertIn("DEPENDENCY_UNRESOLVED_SUMMARY_INCONSISTENT", codes)
        self.assertIn("DEPENDENCY_UNRESOLVED", codes)

    def test_resolution_requires_packaged_digest_and_source(self):
        self.inventory["unresolved"] = [{"id": "example@1.0", "reason": "grant not recorded"}]
        write_json(self.inventory_path, self.inventory)
        self.refresh_review()
        review = json.loads(self.review_path.read_text())
        review["resolutions"] = [{"id": "example@1.0", "reason": "Synthetic evidence", "evidence": [{"path": "licenses/example/LICENSE", "sha256": gate.digest(self.bundle / "third-party/licenses/example/LICENSE"), "source": "https://example.invalid/test-only"}]}]
        write_json(self.review_path, review)
        self.assertEqual(self.result()["status"], "checks_passed")

    def test_wrong_target_blocks(self):
        self.inventory["target"] = "x86_64-unknown-linux-gnu"
        write_json(self.inventory_path, self.inventory)
        self.refresh_review()
        self.assertIn("DEPENDENCY_TARGET_MISMATCH", self.codes())

    def test_gnu_metadata_cannot_substitute_for_msvc(self):
        self.inventory["target"] = "x86_64-pc-windows-gnu"
        write_json(self.inventory_path, self.inventory)
        self.refresh_review()
        self.assertIn("DEPENDENCY_TARGET_MISMATCH", self.codes())

    def test_development_features_cannot_substitute_for_commercial(self):
        self.inventory["packages"][1]["features"] = []
        write_json(self.inventory_path, self.inventory)
        self.refresh_review()
        self.assertIn("DEPENDENCY_FEATURE_MISMATCH", self.codes())

    def test_path_traversal_and_absolute_notices_rejected(self):
        for value in ("../outside", "/etc/passwd", "a\\b", "a/./b", "a//b"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                gate.safe_path(self.bundle, value)

    def test_symlink_notice_rejected(self):
        path = self.bundle / "linked"
        path.symlink_to(self.repo / "LICENSE-MIT")
        with self.assertRaises(ValueError):
            self.result()


class DependencyCollection(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.repo = self.root / "repo"
        self.repo.mkdir()
        write_json(self.repo / "docs/release/asset-rights-manifest.json", {"dependency_assets": []})
        (self.repo / "Cargo.lock").write_text('version = 4\n[[package]]\nname = "flightsim-app"\nversion = "1.0.0"\n')
        for name in ("LICENSE-MIT", "LICENSE-APACHE"):
            (self.repo / name).write_text(f"Test-only {name}\n")
        self.packages = []
        self.nodes = []
        for name in ("flightsim-app", "normal", "build", "dev"):
            directory = self.repo / name
            directory.mkdir()
            (directory / "Cargo.toml").write_text("test-only")
            (directory / "LICENSE").write_text(f"Copyright test {name}\nPermission test\n")
            self.packages.append({"id": name, "name": name, "version": "1.0.0", "manifest_path": str(directory / "Cargo.toml"), "license": "MIT", "source": None if name == "flightsim-app" else "registry+test", "license_file": None})
            self.nodes.append({"id": name, "features": [], "deps": []})
        self.nodes[0]["deps"] = [{"pkg": name, "dep_kinds": [{"kind": kind}]} for name, kind in [("normal", None), ("build", "build"), ("dev", "dev")]]
        self.metadata_path = self.root / "metadata.json"
        self.metadata = {"workspace_root": str(self.repo), "workspace_members": ["flightsim-app"], "packages": self.packages, "resolve": {"nodes": self.nodes}}
        self.output = self.root / "notices"

    def run_collect(self):
        write_json(self.metadata_path, self.metadata)
        return collector.collect(self.metadata_path, self.repo, self.output, "x86_64-pc-windows-msvc", "flightsim-app")

    def test_normal_and_build_closure_excludes_dev_only(self):
        result = self.run_collect()
        self.assertEqual({p["name"] for p in result["packages"]}, {"flightsim-app", "normal", "build"})
        self.assertEqual(result["review_status"], "not_reviewed")

    def test_copied_notices_are_byte_exact(self):
        result = self.run_collect()
        normal = next(p for p in result["packages"] if p["name"] == "normal")
        self.assertEqual((self.output / normal["notices"][0]["path"]).read_bytes(), (self.repo / "normal/LICENSE").read_bytes())

    def test_missing_license_text_remains_unresolved(self):
        (self.repo / "normal/LICENSE").unlink()
        self.assertIn("normal@1.0.0", {p["id"] for p in self.run_collect()["unresolved"]})

    def test_nested_third_party_notice_does_not_substitute_primary_license(self):
        (self.repo / "normal/LICENSE").unlink()
        path = self.repo / "normal/third-party/NOTICE"
        path.parent.mkdir()
        path.write_text("Some other component's notice")
        result = self.run_collect()
        record = next(p for p in result["packages"] if p["name"] == "normal")
        self.assertEqual(len(record["notices"]), 1)
        self.assertTrue(record["unresolved"])

    def test_and_expression_not_reduced_to_allowlist(self):
        self.packages[1]["license"] = "MIT AND Unicode-3.0"
        result = self.run_collect()
        self.assertEqual(next(p for p in result["packages"] if p["name"] == "normal")["license_expression"], "MIT AND Unicode-3.0")

    def test_no_reuse_of_stale_output(self):
        self.run_collect()
        with self.assertRaises(ValueError):
            self.run_collect()

    def test_external_license_file_not_read(self):
        outside = self.root / "secret"
        outside.write_text("not a license")
        self.packages[1]["license_file"] = str(outside)
        self.assertTrue(any("escapes source root" in p["reason"] for p in self.run_collect()["unresolved"]))

    def test_incomplete_graph_rejected(self):
        self.nodes.pop(1)
        with self.assertRaises(ValueError):
            self.run_collect()


@unittest.skipUnless(shutil.which("git"), "Git required for checkout simulation")
class WindowsCheckoutBytes(unittest.TestCase):
    def test_autocrlf_checkout_preserves_asset_and_upstream_license_bytes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source"
            clone = root / "windows-style"
            source.mkdir()
            subprocess.run(["git", "init", "-q", str(source)], check=True)
            repo = SCRIPTS.parent
            for relative in (".gitattributes", "assets/aircraft/.gitattributes", "docs/release/.gitattributes"):
                destination = source / relative
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(repo / relative, destination)
            fixtures = {
                "assets/aircraft/profile.json": b'{"test": true}\n',
                "assets/aircraft/model.glb": b"glTF\x00\r\n\x01",
                "docs/release/record.json": b'{"evidence": true}\n',
                "docs/release/licenses/lf/LICENSE": b"Upstream LF with trailing space \n",
                "docs/release/licenses/crlf/LICENSE": b"Upstream CRLF\r\nNotice line\r\n",
            }
            for relative, data in fixtures.items():
                path = source / relative
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(data)
            subprocess.run(["git", "-C", str(source), "add", "."], check=True)
            subprocess.run(["git", "-C", str(source), "-c", "user.name=Test Fixture", "-c", "user.email=test@example.invalid", "commit", "-qm", "Byte fixtures only"], check=True)
            subprocess.run(["git", "-c", "core.autocrlf=true", "clone", "-q", "--no-hardlinks", str(source), str(clone)], check=True)
            for relative, data in fixtures.items():
                with self.subTest(path=relative):
                    self.assertEqual((clone / relative).read_bytes(), data)


if __name__ == "__main__":
    unittest.main()
