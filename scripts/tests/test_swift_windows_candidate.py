"""Candidate/evidence boundaries without compiling, graphics, network or approval."""
import importlib.util
import json
from pathlib import Path
import struct
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import zlib


ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("swift_candidate", ROOT / "scripts/check-swift-windows-candidate.py")
candidate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(candidate)


def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data))


def png_bytes(extra=b"", pixel_tail=b""):
    header = struct.pack(">IIBBBBB", 640, 360, 8, 2, 0, 0, 0)
    pixels = (b"\0" + b"\x20\x40\x60" * 640) * 360
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + extra
            + chunk(b"IDAT", zlib.compress(pixels) + pixel_tail) + chunk(b"IEND", b""))


SWIFT_LOG = ("INFO aircraft: Swift Sport (generic) (swift-sport)\n"
             + candidate.SWIFT_MODEL_LOG + "\n"
             + "INFO aircraft model fitted: 7.12 m along its length → scale 1.0000\n"
             + "INFO Screenshot saved to capture.png\nBatch capture complete: status 0\n")
LEGACY_LOG = ("INFO aircraft: Light Single (generic) (light-single)\n"
              "INFO Screenshot saved to capture.png\nBatch capture complete: status 0\n")


class CandidateAcceptanceTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)

    def test_build_and_test_share_supported_release_msvc_flags(self):
        commands = candidate.candidate_commands()
        for name in ("build", "identity_test"):
            command = commands[name]
            self.assertEqual(command[:2], ["cargo", "+1.93.0"])
            self.assertIn("--release", command)
            self.assertIn("--locked", command)
            self.assertEqual(command[command.index("--target") + 1], candidate.TARGET)
            self.assertEqual(command[command.index("--features") + 1], "commercial-staging")
            self.assertNotIn("--no-default-features", command)
        metadata = commands["metadata"]
        self.assertEqual(metadata[metadata.index("--filter-platform") + 1], candidate.TARGET)
        self.assertEqual(metadata[metadata.index("--features") + 1], "flightsim-app/commercial-staging")

    def test_all_frozen_legacy_sources_match_independent_baseline(self):
        for relative, expected in candidate.LEGACY_SOURCE_HASHES.items():
            self.assertEqual(candidate.digest(ROOT / relative), expected, relative)
            blob = subprocess.check_output(["git", "show", "HEAD:" + relative], cwd=ROOT)
            self.assertEqual(candidate.hashlib.sha256(blob).hexdigest(), expected, relative)

    def source_fixture(self):
        repo = self.root / "source"
        repo.mkdir()
        subprocess.run(["git", "init", "-q", str(repo)], check=True)
        # Use the actual attributes: Rust is text=auto, profiles explicitly LF,
        # and verbatim license text explicitly bypasses all EOL conversion.
        for relative in (*candidate.LEGACY_SOURCE_HASHES, ".gitattributes",
                         "assets/aircraft/.gitattributes", "docs/release/.gitattributes"):
            target = repo / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes((ROOT / relative).read_bytes())
        notice = repo / "docs/release/licenses/upstream/NOTICE"
        notice.parent.mkdir(parents=True)
        notice.write_bytes(b"Verbatim upstream fixture\r\nKeep these CRLF bytes.\r\n")
        self.commit_source_fixture(repo)
        return repo, notice

    def commit_source_fixture(self, repo):
        subprocess.run(["git", "-c", "core.autocrlf=false", "add", "."], cwd=repo, check=True)
        subprocess.run(["git", "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
                        "commit", "-qm", "source fixture"], cwd=repo, check=True)
        return candidate.git(repo, "rev-parse", "HEAD")

    def checkout_source_fixture(self, repo, eol):
        # Keep the simulated platform policy active for status/diff as well as
        # checkout. These settings affect only this disposable fixture repo.
        subprocess.run(["git", "config", "core.autocrlf", "false"], cwd=repo, check=True)
        subprocess.run(["git", "config", "core.eol", eol], cwd=repo, check=True)
        # Force an actual checkout, independent of Git's cached stat metadata.
        for path in subprocess.check_output(["git", "ls-files", "-z"], cwd=repo).decode().split("\0"):
            if path:
                (repo / path).write_bytes(b"force fixture recheckout")
        subprocess.run(["git", "-c", "core.autocrlf=false", "-c", "core.eol=" + eol,
                        "checkout-index", "--force", "--all", "--index"], cwd=repo, check=True)

    def test_windows_native_checkout_failure_then_explicit_lf_preserves_raw_notices(self):
        repo, notice = self.source_fixture()
        notice_bytes = notice.read_bytes()
        expected = candidate.git(repo, "rev-parse", "HEAD")
        self.checkout_source_fixture(repo, "crlf")
        aircraft = repo / "crates/flightsim-fdm/src/aircraft.rs"
        self.assertEqual(aircraft.read_bytes().count(b"\r\n"), 799)
        self.assertEqual(candidate.digest(repo / "assets/aircraft/light_single.json"),
                         candidate.LEGACY_SOURCE_HASHES["assets/aircraft/light_single.json"])
        self.assertEqual(notice.read_bytes(), notice_bytes)
        self.assertEqual(candidate.git(repo, "status", "--porcelain"), "")
        with self.assertRaisesRegex(ValueError, "legacy checkout differs") as failure:
            candidate.source_inputs(repo, expected)
        self.assertIn("expected_sha256=72091944", str(failure.exception))
        self.assertIn("canonical_sha256=72091944", str(failure.exception))
        self.assertIn("checkout_sha256=5fb95408", str(failure.exception))

        self.checkout_source_fixture(repo, "lf")
        self.assertEqual(aircraft.read_bytes().count(b"\r\n"), 0)
        self.assertEqual(notice.read_bytes(), notice_bytes)
        evidence = candidate.source_inputs(repo, expected)
        self.assertEqual(evidence["schema_version"], 2)
        self.assertEqual(evidence["canonical_git_object_format"], "sha1")
        record = next(r for r in evidence["files"] if r["path"] == "docs/release/licenses/upstream/NOTICE")
        self.assertEqual(record["checkout_sha256"], candidate.digest(notice))
        self.assertEqual(record["checkout_bytes"], len(notice_bytes))
        self.assertEqual(record["canonical_git_blob"], candidate.git(repo, "rev-parse", "HEAD:" + record["path"]))
        for relative, pinned in candidate.LEGACY_SOURCE_HASHES.items():
            identity = evidence["legacy_source_evidence"][relative]
            self.assertEqual(identity["canonical_sha256"], pinned)
            self.assertEqual(identity["checkout_sha256"], pinned)
            self.assertEqual(identity["canonical_bytes"], identity["checkout_bytes"])

    def test_canonical_semantic_change_is_not_accepted_as_a_checkout_fix(self):
        repo, _ = self.source_fixture()
        path = repo / "assets/aircraft/light_single.json"
        original = path.read_bytes()
        changed = original.replace(b'"mass_kg": 1043.0', b'"mass_kg": 1044.0')
        self.assertNotEqual(original, changed)
        path.write_bytes(changed)
        expected = self.commit_source_fixture(repo)
        with self.assertRaisesRegex(ValueError, "legacy canonical baseline changed") as failure:
            candidate.source_inputs(repo, expected)
        self.assertIn("expected_sha256=8cf101b6", str(failure.exception))
        self.assertIn("canonical_sha256=" + candidate.digest(path), str(failure.exception))
        self.assertIn("checkout_sha256=" + candidate.digest(path), str(failure.exception))

    def test_legacy_header_records_actual_u16_version_and_fixed_identity(self):
        path = self.root / "fixture.fsreplay"
        name = b"Light Single (generic)"
        path.write_bytes(b"FSREPLAY" + struct.pack("<HI", 1, len(name)) + name
                         + struct.pack("<Q", int(candidate.LEGACY_FINGERPRINT, 16)))
        self.assertEqual(candidate.legacy_identity(path)["fingerprint"], "0505e6644bb29a53")
        contents = path.read_bytes()
        path.write_bytes(contents[:-8] + struct.pack("<Q", 42))
        with self.assertRaisesRegex(ValueError, "frozen baseline"):
            candidate.legacy_identity(path)
        path.write_bytes(b"FSREPLAY" + struct.pack("<HI", 1, 100000))
        with self.assertRaisesRegex(ValueError, "bounded legacy"):
            candidate.legacy_identity(path)

    def test_only_review_blockers_may_continue_engineering_acceptance(self):
        report = {"schema_version": 1, "status": "blocked", "blockers": [
            {"category": "review", "code": "DEPENDENCY_REVIEW_REQUIRED"}]}
        candidate.validate_readiness(report, 2)
        with self.assertRaisesRegex(ValueError, "unexpectedly"):
            candidate.validate_readiness(report, 1)
        with self.assertRaisesRegex(ValueError, "disagree"):
            candidate.validate_readiness(report, 0)
        report["blockers"][0]["category"] = "integrity"
        with self.assertRaisesRegex(ValueError, "integrity"):
            candidate.validate_readiness(report, 2)
        report["blockers"][0] = {"category": "review", "code": "UNRESOLVED_ASSET_RIGHTS"}
        with self.assertRaisesRegex(ValueError, "integrity"):
            candidate.validate_readiness(report, 2)

    def test_png_validates_crc_pixel_stream_and_rejects_hidden_payloads(self):
        path = self.root / "proof.png"
        path.write_bytes(png_bytes())
        info = candidate.validate_png(path)
        self.assertEqual((info["width"], info["height"]), (640, 360))
        cases = [png_bytes()[:-1], png_bytes() + b"MZ executable", b"MZ" + png_bytes(),
                 png_bytes(chunk(b"tEXt", b"MZ hidden payload")), png_bytes(pixel_tail=b"payload")]
        corrupt = bytearray(png_bytes())
        corrupt[29] ^= 1
        cases.append(bytes(corrupt))
        for content in cases:
            path.write_bytes(content)
            with self.assertRaises((ValueError, zlib.error)):
                candidate.validate_png(path)

    def test_swift_image_requires_success_model_fit_and_extracted_asset_path(self):
        candidate.validate_smoke(SWIFT_LOG, 0, model=True)
        for broken, code in ((SWIFT_LOG, 2),
                             (SWIFT_LOG.replace("extracted/swift-candidate/", "developer-copy/"), 0),
                             (SWIFT_LOG.replace("swift_sport.glb", "swift_sport.glb.backup"), 0),
                             (SWIFT_LOG.replace("7.12", "8.30"), 0),
                             (SWIFT_LOG.replace("1.0000", "0.8578"), 0),
                             (SWIFT_LOG.replace("Batch capture complete: status 0", ""), 0),
                             (SWIFT_LOG + "ERROR pipeline unavailable", 0),
                             (SWIFT_LOG + "thread 'main' panicked at x", 0),
                             (SWIFT_LOG + "WARN using the placeholder", 0)):
            with self.assertRaises(ValueError):
                candidate.validate_smoke(broken, code, model=True)
        candidate.validate_smoke(SWIFT_LOG + "WARN optional audio reports lowercase error", 0, model=True)
        candidate.validate_smoke(LEGACY_LOG, 0, model=False)
        with self.assertRaisesRegex(ValueError, "loaded a model"):
            candidate.validate_smoke(LEGACY_LOG + "aircraft model: secret.glb", 0, model=False)

    def test_sanitizes_windows_paths_in_logs_and_exception_repr(self):
        repo, work = Path("C:\\checkout"), Path("C:\\private")
        raw = ("aircraft model: \\\\?\\C:\\private\\extracted\\swift-candidate\\assets\\aircraft\\swift_sport.glb\n"
               + repr("C:\\private\\target\\app.exe") + " C:/checkout/file.rs")
        text = candidate.sanitize(raw, repo, work)
        self.assertIn(candidate.SWIFT_MODEL_LOG, text)
        self.assertNotIn("C:", text)
        self.assertIn("<source>/file.rs", text)

    def test_extracted_bundle_binds_members_assets_bytes_and_executable(self):
        bundle = self.root / "bundle"
        bundle.mkdir()
        exe = self.root / "built.exe"
        exe.write_bytes(b"MZ synthetic fixture")
        files = {"flightsim-app.exe": exe.read_bytes(), "assets/aircraft/swift_sport.glb": b"swift mesh",
                 "assets/aircraft/swift_sport.json": b"swift profile", "NOTICE": b"notice"}
        entries = []
        for name, content in files.items():
            path = bundle / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
            entries.append({"path": name, "bytes": len(content), "sha256": candidate.digest(path)})
        manifest = bundle / "bundle-manifest.json"
        candidate.write_json(manifest, {"release_authorized": False, "files": entries})
        expected = candidate.digest(manifest)
        candidate.verify_bundle(bundle, exe, expected)
        (bundle / "NOTICE").write_bytes(b"tampered")
        with self.assertRaisesRegex(ValueError, "integrity"):
            candidate.verify_bundle(bundle, exe, expected)
        (bundle / "NOTICE").write_bytes(b"notice")
        (bundle / "renamed.bin").write_bytes(b"excluded data")
        with self.assertRaisesRegex(ValueError, "membership"):
            candidate.verify_bundle(bundle, exe, expected)
        with self.assertRaisesRegex(ValueError, "manifest changed"):
            candidate.verify_bundle(bundle, exe, "0" * 64)

    def test_default_or_wrong_platform_inventory_cannot_attest_candidate(self):
        repo = self.root / "repo"
        (repo / "docs/release").mkdir(parents=True)
        (repo / "Cargo.lock").write_bytes(b"lock")
        (repo / "docs/release/asset-rights-manifest.json").write_bytes(b"manifest")
        metadata = {"resolve": {"nodes": [{"id": "engine", "features": ["tonemapping_luts"]}]},
                    "packages": [{"name": "bevy_core_pipeline", "id": "engine"}]}
        metadata_path = self.root / "metadata.json"
        candidate.write_json(metadata_path, metadata)
        inventory = {"target": candidate.TARGET, "review_status": "not_reviewed",
                     "metadata_sha256": candidate.digest(metadata_path),
                     "cargo_lock_sha256": candidate.digest(repo / "Cargo.lock"),
                     "asset_manifest_sha256": candidate.digest(repo / "docs/release/asset-rights-manifest.json"),
                     "packages": [{"name": "flightsim-app", "features": ["default", "commercial-staging"]}]}
        candidate.validate_inventory(inventory, metadata, repo, metadata_path)
        for key, value in (("target", "x86_64-pc-windows-gnu"), ("metadata_sha256", "old"),
                           ("asset_manifest_sha256", "old"), ("review_status", "reviewed")):
            with self.subTest(key=key), self.assertRaises(ValueError):
                candidate.validate_inventory({**inventory, key: value}, metadata, repo, metadata_path)
        inventory["packages"][0]["features"] = ["default"]
        with self.assertRaisesRegex(ValueError, "candidate app features"):
            candidate.validate_inventory(inventory, metadata, repo, metadata_path)
        inventory["packages"][0]["features"] = ["default", "commercial-staging"]
        metadata["resolve"]["nodes"][0]["features"] = []
        with self.assertRaisesRegex(ValueError, "full LUT bundle"):
            candidate.validate_inventory(inventory, metadata, repo, metadata_path)

    def evidence_report(self, directory, checks=None):
        report = {"schema_version": 1, "candidate": candidate.IDENTITY, "target": candidate.TARGET,
                  "features": candidate.FEATURES, "default_features": True, "release_authorized": False,
                  "status": "failed", "failure": "synthetic later failure", "checks": checks or {},
                  "evidence_files": {p.name: {"bytes": p.stat().st_size, "sha256": candidate.digest(p)}
                                     for p in directory.iterdir() if p.name != "acceptance.json"}}
        candidate.write_json(directory / "acceptance.json", report)
        return report

    def test_failure_evidence_is_text_only_and_hash_bound(self):
        evidence = self.root / "evidence"
        evidence.mkdir()
        (evidence / "commands.log").write_text("diagnostic", encoding="utf-8")
        self.evidence_report(evidence)
        candidate.validate_evidence(evidence)
        (evidence / "commands.log").write_text("changed", encoding="utf-8")
        with self.assertRaisesRegex(ValueError, "hash/size"):
            candidate.validate_evidence(evidence)
        (evidence / "commands.log").write_bytes(b"MZ\0renamed executable")
        self.evidence_report(evidence)
        with self.assertRaisesRegex(ValueError, "binary bytes"):
            candidate.validate_evidence(evidence)
        (evidence / "candidate.zip").write_bytes(b"zip")
        with self.assertRaises(ValueError):
            candidate.validate_evidence(evidence)

    def test_png_cannot_be_uploaded_without_matching_success_and_log(self):
        evidence = self.root / "evidence"
        evidence.mkdir()
        (evidence / candidate.PNG_NAME).write_bytes(png_bytes())
        (evidence / "default-swift.log").write_text(SWIFT_LOG, encoding="utf-8")
        self.evidence_report(evidence)
        with self.assertRaisesRegex(ValueError, "no successful Swift"):
            candidate.validate_evidence(evidence)
        checks = {"default_swift": {"status": "passed", "exit_code": 0,
                                   "png": candidate.validate_png(evidence / candidate.PNG_NAME),
                                   "log_sha256": candidate.digest(evidence / "default-swift.log")}}
        self.evidence_report(evidence, checks)
        candidate.validate_evidence(evidence)
        (evidence / "default-swift.log").write_text(SWIFT_LOG + "ERROR rendering failed", encoding="utf-8")
        checks["default_swift"]["log_sha256"] = candidate.digest(evidence / "default-swift.log")
        self.evidence_report(evidence, checks)
        with self.assertRaisesRegex(ValueError, "runtime logged"):
            candidate.validate_evidence(evidence)

    def test_success_cannot_be_declared_by_an_incomplete_report(self):
        evidence = self.root / "evidence"
        evidence.mkdir()
        report = self.evidence_report(evidence)
        report["status"] = "engineering_checks_passed"
        candidate.write_json(evidence / "acceptance.json", report)
        with self.assertRaisesRegex(ValueError, "lacks required checks"):
            candidate.validate_evidence(evidence)

    def test_unexpected_platform_fails_before_creating_a_candidate(self):
        with patch.object(candidate.sys, "platform", "linux"):
            with self.assertRaisesRegex(ValueError, "requires Windows"):
                candidate.run_candidate(ROOT, "a" * 40, self.root / "work", self.root / "evidence")
        self.assertFalse((self.root / "work").exists())


class CandidateWorkflowTests(unittest.TestCase):
    def test_workflow_is_exact_successful_main_ci_diagnostics_only(self):
        text = (ROOT / ".github/workflows/swift-windows-candidate.yml").read_text(encoding="utf-8")
        for guard in ("workflow_run:", "workflows: [CI]", "conclusion == 'success'", "event == 'push'",
                      "head_branch == 'main'", "head_sha == github.sha",
                      "head_repository.full_name == github.repository", "persist-credentials: false",
                      "ref: ${{ github.event.workflow_run.head_sha }}", "timeout-minutes: 90"):
            self.assertIn(guard, text)
        for setting in ("GIT_CONFIG_COUNT: '2'", "GIT_CONFIG_KEY_0: core.autocrlf",
                        "GIT_CONFIG_VALUE_0: 'false'", "GIT_CONFIG_KEY_1: core.eol", "GIT_CONFIG_VALUE_1: 'lf'"):
            self.assertIn(setting, text)
        for forbidden in ("contents: write", "uses: Swatinem/rust-cache", "uses: actions/cache",
                          "gh release", "git tag", "workflow_dispatch:"):
            self.assertNotIn(forbidden, text)
        block = text.split("          path: |\n", 1)[1].split("          if-no-files-found:", 1)[0]
        names = {line.strip().rsplit("/", 1)[1] for line in block.splitlines() if line.strip()}
        self.assertEqual(names, candidate.TEXT_EVIDENCE | {candidate.PNG_NAME})
        self.assertNotIn("*", block)
        self.assertIn("steps.evidence.outputs.validated == 'true'", text)
        self.assertNotIn("release.yml", text)


if __name__ == "__main__":
    unittest.main()
