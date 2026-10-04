"""Candidate/evidence boundaries without compiling, graphics, network or approval."""
import importlib.util
import json
from pathlib import Path
import struct
import subprocess
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace
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
              "INFO Screenshot saved to capture.png\nBatch capture complete: status 0\n"
              + candidate.LEGACY_NOTICE + "\n")


class CandidateAcceptanceTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)

    def test_build_and_test_share_supported_release_msvc_flags(self):
        commands = candidate.candidate_commands()
        for name in ("build", "identity_test", *candidate.REPLAY_ACCEPTANCE_TESTS):
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

    def test_reviewed_sources_and_independent_goldens_match_exact_committed_bytes(self):
        contract = candidate.load_replay_contract(ROOT)
        for relative, expected in {**contract["source_sha256"], **candidate.INDEPENDENT_REPLAY_HASHES}.items():
            self.assertEqual(candidate.digest(ROOT / relative), expected, relative)
            blob = subprocess.check_output(["git", "show", "HEAD:" + relative], cwd=ROOT)
            self.assertEqual(candidate.hashlib.sha256(blob).hexdigest(), expected, relative)
        self.assertEqual(candidate.LEGACY_SOURCE_HASHES["crates/flightsim-sim/src/replay.rs"],
                         "0b783ceed247b984729021ae57c74b061936d627c04275a850e59079266a18c1")
        self.assertNotEqual(contract["source_sha256"]["crates/flightsim-sim/src/replay.rs"],
                            candidate.LEGACY_SOURCE_HASHES["crates/flightsim-sim/src/replay.rs"])

    def test_independent_reference_encoders_keep_all_existing_bytes_and_yaw_ambiguity(self):
        for script in ("replay_identity_reference.py", "replay_v3_reference.py"):
            subprocess.run([candidate.sys.executable, str(ROOT / "docs/qa" / script)],
                           cwd=self.root, check=True, capture_output=True)

    def test_extracted_and_staged_identity_require_literal_offline_false(self):
        info = {"profile": "commercial-staging", "region_downloads": False}
        candidate.validate_distribution(info, dict(info))
        for value in (True, None, 0, 0.0, "false", [], {}):
            changed = {**info, "region_downloads": value}
            for actual, staged in ((changed, info), (info, changed), (changed, changed)):
                with self.subTest(actual=actual, staged=staged), self.assertRaisesRegex(ValueError, "region_downloads=false"):
                    candidate.validate_distribution(actual, staged)
        missing = {"profile": "commercial-staging"}
        with self.assertRaisesRegex(ValueError, "region_downloads=false"):
            candidate.validate_distribution(missing, missing)
        with self.assertRaisesRegex(ValueError, "identity changed"):
            candidate.validate_distribution(info, {**info, "profile": "development"})
        for value in (None, []):
            with self.assertRaisesRegex(ValueError, "must be an object"):
                candidate.validate_distribution(value, value)

    def source_fixture(self):
        repo = self.root / "source"
        repo.mkdir()
        subprocess.run(["git", "init", "-q", str(repo)], check=True)
        # Use the actual attributes: Rust is text=auto, profiles explicitly LF,
        # and verbatim license text explicitly bypasses all EOL conversion.
        for relative in (*candidate.REPLAY_CONTRACT_PATHS, *candidate.INDEPENDENT_REPLAY_HASHES,
                         candidate.REPLAY_CONTRACT_PATH, ".gitattributes",
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
        # Keep the contract LF in this fixture so the independent implementation
        # byte check is reached; another test covers a CRLF contract itself.
        with (repo / ".gitattributes").open("a", encoding="utf-8") as attributes:
            attributes.write("\nscripts/replay-candidate-contract.json text eol=lf\n")
        expected = self.commit_source_fixture(repo)
        self.checkout_source_fixture(repo, "crlf")
        aircraft = repo / "crates/flightsim-fdm/src/aircraft.rs"
        self.assertEqual(aircraft.read_bytes().count(b"\r\n"), 799)
        self.assertEqual(candidate.digest(repo / "assets/aircraft/light_single.json"),
                         candidate.LEGACY_SOURCE_HASHES["assets/aircraft/light_single.json"])
        self.assertEqual(notice.read_bytes(), notice_bytes)
        self.assertEqual(candidate.git(repo, "status", "--porcelain"), "")
        with self.assertRaisesRegex(ValueError, "reviewed replay checkout differs") as failure:
            candidate.source_inputs(repo, expected)
        relative = "crates/flightsim-app/src/aircraft_profile.rs"
        pinned = candidate.load_replay_contract(repo)["source_sha256"][relative]
        self.assertIn("expected_sha256=" + pinned, str(failure.exception))
        self.assertIn("canonical_sha256=" + pinned, str(failure.exception))
        self.assertIn("checkout_sha256=" + candidate.digest(repo / relative), str(failure.exception))

        self.checkout_source_fixture(repo, "lf")
        self.assertEqual(aircraft.read_bytes().count(b"\r\n"), 0)
        self.assertEqual(notice.read_bytes(), notice_bytes)
        evidence = candidate.source_inputs(repo, expected)
        self.assertEqual(evidence["schema_version"], 3)
        self.assertEqual(evidence["canonical_git_object_format"], "sha1")
        record = next(r for r in evidence["files"] if r["path"] == "docs/release/licenses/upstream/NOTICE")
        self.assertEqual(record["checkout_sha256"], candidate.digest(notice))
        self.assertEqual(record["checkout_bytes"], len(notice_bytes))
        self.assertEqual(record["canonical_git_blob"], candidate.git(repo, "rev-parse", "HEAD:" + record["path"]))
        for relative, pinned in candidate.load_replay_contract(repo)["source_sha256"].items():
            identity = evidence["reviewed_replay_source_evidence"][relative]
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
        with self.assertRaisesRegex(ValueError, "reviewed replay canonical baseline changed") as failure:
            candidate.source_inputs(repo, expected)
        self.assertIn("expected_sha256=8cf101b6", str(failure.exception))
        self.assertIn("canonical_sha256=" + candidate.digest(path), str(failure.exception))
        self.assertIn("checkout_sha256=" + candidate.digest(path), str(failure.exception))

    def test_reviewed_contract_cannot_shrink_or_repin_unchanged_legacy_inputs(self):
        repo, _ = self.source_fixture()
        path = repo / candidate.REPLAY_CONTRACT_PATH
        original = json.loads(path.read_text(encoding="utf-8"))
        for relative in candidate.REPLAY_CONTRACT_PATHS:
            changed = json.loads(json.dumps(original))
            del changed["source_sha256"][relative]
            candidate.write_json(path, changed)
            with self.subTest(relative=relative), self.assertRaisesRegex(ValueError, "boundary changed"):
                candidate.load_replay_contract(repo)
        changed = json.loads(json.dumps(original))
        changed["source_sha256"]["assets/aircraft/light_single.json"] = "0" * 64
        candidate.write_json(path, changed)
        with self.assertRaisesRegex(ValueError, "frozen legacy input changed"):
            candidate.load_replay_contract(repo)

    def test_all_expanded_helpers_and_independent_goldens_fail_on_committed_drift(self):
        repo, _ = self.source_fixture()
        for relative in (*candidate.REPLAY_CONTRACT_PATHS, *candidate.INDEPENDENT_REPLAY_HASHES):
            path = repo / relative
            original = path.read_bytes()
            path.write_bytes(original + b"\n")
            expected = self.commit_source_fixture(repo)
            with self.subTest(relative=relative), self.assertRaisesRegex(ValueError, "canonical baseline changed"):
                candidate.source_inputs(repo, expected)
            path.write_bytes(original)
            self.commit_source_fixture(repo)

    def test_contract_checkout_bytes_are_bound_even_when_git_reports_clean(self):
        repo, _ = self.source_fixture()
        self.checkout_source_fixture(repo, "crlf")
        self.assertEqual(candidate.git(repo, "status", "--porcelain"), "")
        with self.assertRaisesRegex(ValueError, "contract checkout differs"):
            candidate.source_inputs(repo, candidate.git(repo, "rev-parse", "HEAD"))

    def test_opt_in_and_full_limitation_are_required_only_for_positive_legacy_smoke(self):
        command = candidate.legacy_capture_command("app.exe", "legacy.fsreplay", "legacy.png")
        self.assertEqual(command.count("--legacy-replay-compatibility"), 1)
        self.assertIn("--no-model", command)
        candidate.validate_legacy_smoke(LEGACY_LOG, 0)
        for text in (LEGACY_LOG.replace(candidate.LEGACY_NOTICE, ""),
                     LEGACY_LOG.replace("not recorded or verified", "verified"),
                     LEGACY_LOG.replace("historical yaw_rate_p", "identity")):
            with self.assertRaisesRegex(ValueError, "full partial-identity limitation"):
                candidate.validate_legacy_smoke(text, 0)
        negative = "aircraft/FDM model mismatch: recorded legacy partial fingerprint 0505e6644bb29a53"
        candidate.validate_legacy_rejection(negative, 2)
        for log, code in ((negative, 0), (negative.replace("0505e6644bb29a53", "42"), 2),
                          (negative.replace("legacy partial fingerprint", "fingerprint"), 2)):
            with self.assertRaisesRegex(ValueError, "hexadecimal"):
                candidate.validate_legacy_rejection(log, code)
        text = (ROOT / "scripts/check-swift-windows-candidate.py").read_text(encoding="utf-8")
        self.assertIn('run([app, "--replay", fixture]', text)
        for name, test in candidate.REPLAY_ACCEPTANCE_TESTS.items():
            self.assertEqual(candidate.candidate_commands()[name][-3:], [test, "--", "--exact"])

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
        for features in (["default"], ["default", "commercial-staging", "region-downloads"]):
            inventory["packages"][0]["features"] = features
            with self.subTest(features=features), self.assertRaisesRegex(ValueError, "candidate app features"):
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

    def test_exported_legacy_success_requires_partial_notice_and_persistence_evidence(self):
        evidence = self.root / "evidence"
        evidence.mkdir()
        log = evidence / "legacy-no-model.log"
        log.write_text(LEGACY_LOG, encoding="utf-8")
        proof = {"status": "passed", "exit_code": 0, "identity_evidence": "legacy_partial",
                 "legacy_opt_in": True, "historical_yaw_verified": False,
                 "notice": candidate.LEGACY_NOTICE, "fingerprint": candidate.LEGACY_FINGERPRINT,
                 "log_sha256": candidate.digest(log)}
        report = self.evidence_report(evidence, {"legacy_no_model": proof})
        report["limits"] = [candidate.LEGACY_LIMIT]
        report["replay_tests"] = {
            name: {"status": "passed", "test": test} for name, test in candidate.REPLAY_ACCEPTANCE_TESTS.items()
        }
        self.seal_report(evidence, report)
        candidate.validate_evidence(evidence)
        for key, value in (("identity_evidence", "complete"), ("historical_yaw_verified", True),
                           ("historical_yaw_verified", 0), ("legacy_opt_in", 1), ("notice", "partial")):
            changed = json.loads(json.dumps(report))
            changed["checks"]["legacy_no_model"][key] = value
            self.seal_report(evidence, changed)
            with self.subTest(key=key, value=value), self.assertRaisesRegex(ValueError, "cannot claim"):
                candidate.validate_evidence(evidence)
        for field in ("limits", "replay_tests"):
            changed = json.loads(json.dumps(report))
            del changed[field]
            self.seal_report(evidence, changed)
            with self.assertRaisesRegex(ValueError, "limitation|persistent notice"):
                candidate.validate_evidence(evidence)
        log.write_text(LEGACY_LOG.replace(candidate.LEGACY_NOTICE, ""), encoding="utf-8")
        report["checks"]["legacy_no_model"]["log_sha256"] = candidate.digest(log)
        self.seal_report(evidence, report)
        with self.assertRaisesRegex(ValueError, "full partial-identity limitation"):
            candidate.validate_evidence(evidence)

    def successful_evidence_fixture(self):
        repo, _ = self.source_fixture()
        source_sha = candidate.git(repo, "rev-parse", "HEAD")
        source = candidate.source_inputs(repo, source_sha)
        evidence = self.root / "evidence"
        evidence.mkdir()
        for name in candidate.REQUIRED_TEXT_EVIDENCE - {"acceptance.json"}:
            (evidence / name).write_text("{}" if name.endswith(".json") else "fixture", encoding="utf-8")
        candidate.write_json(evidence / "source-inputs.json", source)
        (evidence / candidate.PNG_NAME).write_bytes(png_bytes())
        (evidence / "default-swift.log").write_text(SWIFT_LOG, encoding="utf-8")
        (evidence / "legacy-no-model.log").write_text(LEGACY_LOG, encoding="utf-8")
        (evidence / "default-rejects-legacy.log").write_text(
            "aircraft/FDM model mismatch: recorded legacy partial fingerprint 0505e6644bb29a53", encoding="utf-8")
        checks = {
            "default_swift": {"status": "passed", "exit_code": 0,
                              "png": candidate.validate_png(evidence / candidate.PNG_NAME),
                              "log_sha256": candidate.digest(evidence / "default-swift.log")},
            "absent_light_single": {"status": "passed", "exit_code": 2},
            "default_rejects_legacy": {"status": "passed", "exit_code": 2},
            "legacy_no_model": {"status": "passed", "exit_code": 0, "identity_evidence": "legacy_partial",
                                "legacy_opt_in": True, "historical_yaw_verified": False,
                                "notice": candidate.LEGACY_NOTICE, "fingerprint": candidate.LEGACY_FINGERPRINT,
                                "log_sha256": candidate.digest(evidence / "legacy-no-model.log")},
        }
        report = self.evidence_report(evidence, checks)
        report.update(status="engineering_checks_passed", source_sha=source_sha,
                      source_inputs_sha256=candidate.digest(evidence / "source-inputs.json"),
                      replay_contract=candidate.REPLAY_CONTRACT_ID,
                      replay_contract_sha256=source["replay_contract_sha256"],
                      legacy_replay={"fingerprint": candidate.LEGACY_FINGERPRINT},
                      distribution={"region_downloads": False}, limits=[candidate.LEGACY_LIMIT],
                      replay_tests={name: {"status": "passed", "test": test}
                                    for name, test in candidate.REPLAY_ACCEPTANCE_TESTS.items()})
        self.seal_report(evidence, report)
        return evidence, source, report

    def test_success_requires_complete_consistent_source_and_contract_evidence(self):
        evidence, source, report = self.successful_evidence_fixture()
        candidate.validate_evidence(evidence)
        mutations = []
        for field in source:
            changed = json.loads(json.dumps(source))
            del changed[field]
            mutations.append((field, changed))
        for path, value in (("source_sha", "0" * 40), ("replay_contract_sha256", "0" * 64),
                            ("replay_contract_text", "{}"), ("files", []),
                            ("reviewed_replay_source_evidence", {}), ("independent_replay_sha256", {})):
            mutations.append((path, {**source, path: value}))
        changed = json.loads(json.dumps(source))
        changed["replay_contract"]["source_sha256"].pop("crates/flightsim-sim/src/replay/player.rs")
        mutations.append(("removed helper", changed))
        changed = json.loads(json.dumps(source))
        changed["reviewed_replay_source_evidence"]["crates/flightsim-sim/src/replay.rs"]["checkout_bytes"] += 1
        mutations.append(("inconsistent bytes", changed))
        for name, changed in mutations:
            candidate.write_json(evidence / "source-inputs.json", changed)
            report["source_inputs_sha256"] = candidate.digest(evidence / "source-inputs.json")
            self.seal_report(evidence, report)
            with self.subTest(name=name), self.assertRaises(ValueError):
                candidate.validate_evidence(evidence)
        candidate.write_json(evidence / "source-inputs.json", source)
        report["source_inputs_sha256"] = candidate.digest(evidence / "source-inputs.json")
        for field in ("source_sha", "source_inputs_sha256", "replay_contract", "replay_contract_sha256"):
            changed = dict(report)
            del changed[field]
            self.seal_report(evidence, changed)
            with self.subTest(field=field), self.assertRaises(ValueError):
                candidate.validate_evidence(evidence)
        # Regression: absent hashes on both sides must never pass as None == None.
        source.pop("replay_contract_sha256")
        candidate.write_json(evidence / "source-inputs.json", source)
        report.pop("replay_contract_sha256")
        report["source_inputs_sha256"] = candidate.digest(evidence / "source-inputs.json")
        self.seal_report(evidence, report)
        with self.assertRaisesRegex(ValueError, "byte binding"):
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

    def capture_fixture(self, first="timeout", probe="success"):
        work, evidence = self.root / "capture-work", self.root / "capture-evidence"
        evidence.mkdir()
        app = work / "extracted/swift-candidate/flightsim-app.exe"
        app.parent.mkdir(parents=True)
        app.write_bytes(b"MZ synthetic executable, never run")
        unrelated = work / "unrelated-cwd"
        unrelated.mkdir()
        report = self.evidence_report(evidence)
        report["executable_sha256"] = candidate.digest(app)
        report["runtime_capture_rust_log"] = candidate.CAPTURE_TRACE
        calls = []
        primary = subprocess.TimeoutExpired(candidate.capture_command(app, work / candidate.PNG_NAME), 180)

        def run(command, **kwargs):
            calls.append((command, kwargs))
            self.assertEqual(kwargs["cwd"], unrelated)
            self.assertEqual(kwargs["timeout"], 180)
            self.assertIs(kwargs["runtime"], True)
            self.assertEqual(command[0], str(app))
            outcome = first if len(calls) == 1 else probe
            if len(calls) > 1:
                self.assertEqual(len(calls), 2, "probe must never loop")
                self.assertIs(kwargs["release_parity"], True)
            else:
                self.assertNotIn("release_parity", kwargs)
            if outcome == "timeout":
                kwargs["output"].write_text("INFO capturing a screenshot\nTRACE still waiting\n", encoding="utf-8")
                Path(command[2]).write_bytes(b"partial private screenshot")
                raise primary if len(calls) == 1 else subprocess.TimeoutExpired(command, 180)
            kwargs["output"].write_text(SWIFT_LOG, encoding="utf-8")
            if outcome == "success":
                Path(command[2]).write_bytes(png_bytes())
            return subprocess.CompletedProcess(command, 0 if outcome == "success" else 2), SWIFT_LOG

        return run, app, unrelated, work, evidence, report, calls, primary

    def seal_report(self, evidence, report):
        report["evidence_files"] = {
            p.name: {"bytes": p.stat().st_size, "sha256": candidate.digest(p)}
            for p in evidence.iterdir() if p.name != "acceptance.json"
        }
        candidate.write_json(evidence / "acceptance.json", report)

    def test_successful_probe_cannot_turn_primary_timeout_into_acceptance(self):
        run, app, cwd, work, evidence, report, calls, primary = self.capture_fixture()
        with self.assertRaises(subprocess.TimeoutExpired) as failed:
            candidate.check_default_capture(run, ROOT, app, cwd, work, evidence, report)
        self.assertIs(failed.exception, primary)
        self.assertEqual(len(calls), 2)
        self.assertEqual(report["status"], "failed")
        self.assertEqual(report["checks"], {})
        probe = report["diagnostics"]["release_launcher_probe"]
        self.assertEqual(probe["status"], "passed")
        self.assertFalse(probe["qualifies_acceptance"])
        self.assertFalse((evidence / candidate.PNG_NAME).exists())
        self.assertTrue((evidence / candidate.PROBE_PNG_NAME).exists())
        report["failure"] = report["primary_capture_failure"]["message"]
        self.seal_report(evidence, report)
        candidate.validate_evidence(evidence)
        report["checks"]["default_swift"] = {"status": "passed", "exit_code": 0}
        self.seal_report(evidence, report)
        with self.assertRaisesRegex(ValueError, "cannot replace or pass primary timeout"):
            candidate.validate_evidence(evidence)

    def test_probe_timeout_is_single_and_partial_images_never_enter_evidence(self):
        run, app, cwd, work, evidence, report, calls, primary = self.capture_fixture(probe="timeout")
        with self.assertRaises(subprocess.TimeoutExpired) as failed:
            candidate.check_default_capture(run, ROOT, app, cwd, work, evidence, report)
        self.assertIs(failed.exception, primary)
        self.assertEqual(len(calls), 2)
        self.assertEqual(report["diagnostics"]["release_launcher_probe"]["status"], "timed_out")
        self.assertFalse(any((evidence / name).exists() for name in candidate.PNG_EVIDENCE))
        self.assertTrue((work / candidate.PROBE_PNG_NAME).exists())
        report["failure"] = report["primary_capture_failure"]["message"]
        self.seal_report(evidence, report)
        candidate.validate_evidence(evidence)

    def test_primary_success_or_non_timeout_failure_never_launches_probe(self):
        run, app, cwd, work, evidence, report, calls, _ = self.capture_fixture(first="success")
        candidate.check_default_capture(run, ROOT, app, cwd, work, evidence, report)
        self.assertEqual(len(calls), 1)
        self.assertNotIn("diagnostics", report)
        self.assertIn("default_swift", report["checks"])
        # A fresh isolated fixture is needed because outputs are never replaced.
        self.root = self.root / "next"
        self.root.mkdir()
        run, app, cwd, work, evidence, report, calls, _ = self.capture_fixture(first="failure")
        with self.assertRaisesRegex(ValueError, "did not exit 0"):
            candidate.check_default_capture(run, ROOT, app, cwd, work, evidence, report)
        self.assertEqual(len(calls), 1)
        self.assertNotIn("diagnostics", report)

    def test_diagnostic_preparation_error_cannot_replace_original_timeout(self):
        run, app, cwd, work, evidence, report, calls, primary = self.capture_fixture()
        original_digest = candidate.digest

        def fail_probe_preparation(path):
            if path.name == "default-swift.log":
                raise OSError("injected diagnostic hash read failure")
            return original_digest(path)

        with patch.object(candidate, "digest", side_effect=fail_probe_preparation):
            with self.assertRaises(subprocess.TimeoutExpired) as failed:
                candidate.check_default_capture(run, ROOT, app, cwd, work, evidence, report)
        self.assertIs(failed.exception, primary)
        self.assertEqual(len(calls), 1)
        self.assertEqual(report["checks"], {})
        self.assertIn("hash read failure", report["diagnostics"]["release_launcher_probe"]["failure"])

    def test_nonzero_probe_is_failed_diagnostics_and_never_primary_success(self):
        run, app, cwd, work, evidence, report, calls, primary = self.capture_fixture(probe="failure")
        with self.assertRaises(subprocess.TimeoutExpired) as failed:
            candidate.check_default_capture(run, ROOT, app, cwd, work, evidence, report)
        self.assertIs(failed.exception, primary)
        self.assertEqual(len(calls), 2)
        probe = report["diagnostics"]["release_launcher_probe"]
        self.assertEqual((probe["status"], probe["exit_code"]), ("failed", 2))
        self.assertEqual(report["checks"], {})
        report["failure"] = report["primary_capture_failure"]["message"]
        self.seal_report(evidence, report)
        candidate.validate_evidence(evidence)

    def test_diagnostic_png_requires_own_valid_proof_and_unchanged_log(self):
        run, app, cwd, work, evidence, report, _, _ = self.capture_fixture()
        with self.assertRaises(subprocess.TimeoutExpired):
            candidate.check_default_capture(run, ROOT, app, cwd, work, evidence, report)
        report["failure"] = report["primary_capture_failure"]["message"]
        self.seal_report(evidence, report)
        candidate.validate_evidence(evidence)
        probe = report["diagnostics"]["release_launcher_probe"]
        probe["rust_log"] = "info"
        self.seal_report(evidence, report)
        with self.assertRaisesRegex(ValueError, "identity/launch differs"):
            candidate.validate_evidence(evidence)
        probe["rust_log"] = candidate.CAPTURE_TRACE
        probe["status"] = "failed"
        probe["failure"] = "injected failure"
        self.seal_report(evidence, report)
        with self.assertRaisesRegex(ValueError, "unproven diagnostic image"):
            candidate.validate_evidence(evidence)
        probe["status"] = "passed"
        (evidence / candidate.PROBE_LOG_NAME).write_text(SWIFT_LOG + "ERROR capture failed", encoding="utf-8")
        self.seal_report(evidence, report)
        with self.assertRaisesRegex(ValueError, "diagnostic log changed"):
            candidate.validate_evidence(evidence)
        probe["log_sha256"] = candidate.digest(evidence / candidate.PROBE_LOG_NAME)
        self.seal_report(evidence, report)
        with self.assertRaisesRegex(ValueError, "runtime logged"):
            candidate.validate_evidence(evidence)
        (evidence / candidate.PROBE_LOG_NAME).write_text(SWIFT_LOG, encoding="utf-8")
        probe["log_sha256"] = candidate.digest(evidence / candidate.PROBE_LOG_NAME)
        (evidence / candidate.PROBE_PNG_NAME).write_bytes(png_bytes() + b"hidden bytes")
        self.seal_report(evidence, report)
        with self.assertRaisesRegex(ValueError, "trailing bytes"):
            candidate.validate_evidence(evidence)

    def test_optional_probe_files_without_original_timeout_are_rejected(self):
        evidence = self.root / "evidence"
        evidence.mkdir()
        (evidence / candidate.PROBE_PNG_NAME).write_bytes(png_bytes())
        self.evidence_report(evidence)
        with self.assertRaisesRegex(ValueError, "lack a timeout/probe record"):
            candidate.validate_evidence(evidence)

    def test_release_parity_requests_only_one_time_console_and_show_flags(self):
        with patch.object(candidate.subprocess, "STARTUPINFO", SimpleNamespace, create=True), \
                patch.object(candidate.subprocess, "STARTF_USESHOWWINDOW", 1, create=True), \
                patch.object(candidate.subprocess, "CREATE_NEW_CONSOLE", 16, create=True):
            options = candidate.release_parity_startup()
        self.assertEqual(options["creationflags"], 16)
        self.assertEqual(vars(options["startupinfo"]), {"dwFlags": 1, "wShowWindow": 1})
        self.assertEqual(candidate.BASELINE_LAUNCH, {"creationflags": 0, "startupinfo": None})


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
        self.assertEqual(names, candidate.TEXT_EVIDENCE | candidate.PNG_EVIDENCE)
        self.assertNotIn("*", block)
        self.assertIn("steps.evidence.outputs.validated == 'true'", text)
        self.assertNotIn("release.yml", text)


if __name__ == "__main__":
    unittest.main()
