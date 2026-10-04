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
              "INFO Screenshot saved to capture.png\nBatch capture complete: status 0\n"
              + candidate.LEGACY_NOTICE + "\n")
SWIFT_DISTRIBUTION = {
    "schema_version": 1, "package": "flightsim-app", "package_version": "0.0.0-fixture",
    "profile": "commercial-staging", "region_downloads": False,
    "default_aircraft": "swift-sport", "default_model": "aircraft/swift_sport.glb",
    "bundled_aircraft": ["swift-sport"], "release_authorized": False,
    "target_os": "windows", "target_arch": "x86_64", "target_env": "msvc",
}


def readback_fixture(outcome):
    """Protocol examples with independent expected observations in the tests."""
    if outcome == "empty":
        return ""
    if outcome == "malformed":
        return "FS_READBACK_PROBE event=unknown ignored=true\n"
    lines = ["event=enabled", "event=armed", "event=submitted", "event=map_register_enter", "event=map_registered"]
    if outcome == "no_summary":
        lines.append("event=poll_enter gpu_timeout_ms=250 elapsed_ms=5000")
    elif outcome == "queue_only":
        lines.append("event=queue_callback cancelled=false phase=normal")
    elif outcome == "pixels_only":
        lines.extend(["event=map_callback result=ok cancelled=false phase=normal", "event=pixels valid=true count=16"])
    else:
        lines.extend(["event=async_started", "event=async_waiting", "event=async_signal"])
        resumed = outcome != "async_not_resumed"
        if resumed:
            lines.append("event=async_resumed")
        mapped = outcome not in ("gpu_timeout", "map_missing")
        map_result = "error" if outcome == "map_error" else "ok"
        if mapped:
            lines.append("event=map_callback result=" + map_result + " cancelled=false phase=normal")
        pixels = "valid" if mapped and map_result == "ok" else "missing"
        if pixels == "valid":
            lines.append("event=pixels valid=true count=16")
        queued = outcome != "gpu_timeout"
        if queued:
            lines.append("event=queue_callback cancelled=false phase=normal")
        poll = "timeout" if outcome == "gpu_timeout" else "wait_succeeded"
        lines.extend(["event=poll_enter gpu_timeout_ms=250 elapsed_ms=5000", "event=poll_return status=" + poll + " wall_ms=2"])
        lines.append("event=summary submitted=true queue_callback=" + str(queued).lower()
                     + " map_callback=" + (map_result if mapped else "missing") + " pixels=" + pixels
                     + " async_started=true async_waiting=true async_signal=true async_resumed=" + str(resumed).lower()
                     + " poll=" + poll + " cleanup=true render_frames=120 elapsed_ms=15000")
    return "".join("FS_READBACK_PROBE " + line + "\n" for line in lines)


def readback_v2_fixture(startup="complete", scene=True, late=True):
    """Independent v2 examples: elapsed times are local to their stated phase."""
    lines = ["event=enabled version=2", "event=prescene_begin",
             "event=prescene_submitted elapsed_ms=1",
             "event=prescene_map_register_enter elapsed_ms=1",
             "event=prescene_map_registered elapsed_ms=1",
             "event=prescene_poll_enter gpu_timeout_ms=5000 elapsed_ms=1"]
    if startup == "entered_without_return":
        return "".join("FS_READBACK_PROBE " + line + "\n" for line in lines)
    okay = startup == "complete"
    if okay:
        lines.append("event=prescene_map_callback result=ok cancelled=false phase=startup elapsed_ms=2")
    lines.append("event=prescene_poll_return status=" + ("queue_empty wall_ms=2 elapsed_ms=3" if okay else "timeout wall_ms=5000 elapsed_ms=5001"))
    if okay:
        lines.append("event=prescene_pixels valid=true count=16 elapsed_ms=3")
    else:
        lines.append("event=prescene_map_callback result=error cancelled=true phase=cleanup elapsed_ms=5001")
    lines.append("event=prescene_summary submitted=true map_callback=" + ("ok pixels=valid poll=queue_empty cleanup=true elapsed_ms=3" if okay else "missing pixels=missing poll=timeout cleanup=true elapsed_ms=5001"))
    if scene:
        for frame in (1, 2, 4, 8, 16, 30):
            lines.append(f"event=scene_register frame={frame} elapsed_ms={frame * 1000}")
            if okay:
                lines.append(f"event=scene_callback frame={frame} cancelled=false phase=active elapsed_ms={frame * 1000 + 1}")
    result = "".join("FS_READBACK_PROBE " + line + "\n" for line in lines)
    if late:
        result += readback_fixture("complete" if okay else "gpu_timeout").replace("FS_READBACK_PROBE event=enabled\n", "")
    if scene:
        result += "FS_READBACK_PROBE event=scene_summary registered=6 completed=" + ("6" if okay else "0") + " cleanup=true reason=" + ("late_probe elapsed_ms=45000\n" if late else "deadline elapsed_ms=60000\n")
    return result


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
        self.assertEqual(candidate.LEGACY_SOURCE_HASHES["crates/flightsim-fdm/src/lib.rs"],
                         "a956e3046e906304e23e675d440ed20875ea5e47d1a3552158b8356f16b8ccc9")
        self.assertNotEqual(candidate.REVIEWED_ADDITIVE_FDM_LIB_SHA256,
                            candidate.LEGACY_SOURCE_HASHES["crates/flightsim-fdm/src/lib.rs"])

    def test_independent_reference_encoders_keep_all_existing_bytes_and_yaw_ambiguity(self):
        for script in ("replay_identity_reference.py", "replay_v3_reference.py"):
            subprocess.run([candidate.sys.executable, str(ROOT / "docs/qa" / script)],
                           cwd=self.root, check=True, capture_output=True)

    def test_extracted_and_staged_identity_require_literal_offline_false(self):
        info = dict(SWIFT_DISTRIBUTION)
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
        pins = candidate.load_replay_contract(repo)["source_sha256"]
        profile = "crates/flightsim-app/src/aircraft_profile.rs"
        self.assertNotEqual(candidate.digest(repo / profile), pins[profile])
        # Newly extracted helpers can sort before the original profile path.
        # The first mismatched complete file must retain its exact diagnostic;
        # all sources are rechecked below after the explicit LF checkout.
        relative = next(path for path, pinned in pins.items() if candidate.digest(repo / path) != pinned)
        pinned = pins[relative]
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
        for relative in ("assets/aircraft/light_single.json", "crates/flightsim-fdm/src/aircraft.rs"):
            changed = json.loads(json.dumps(original))
            changed["source_sha256"][relative] = "0" * 64
            candidate.write_json(path, changed)
            with self.subTest(relative=relative), self.assertRaisesRegex(ValueError, "frozen legacy input changed"):
                candidate.load_replay_contract(repo)
        for value in ("0" * 64, candidate.LEGACY_SOURCE_HASHES["crates/flightsim-fdm/src/lib.rs"]):
            changed = json.loads(json.dumps(original))
            changed["source_sha256"]["crates/flightsim-fdm/src/lib.rs"] = value
            candidate.write_json(path, changed)
            with self.subTest(fdm_lib=value), self.assertRaisesRegex(ValueError, "frozen reviewed FDM module input changed"):
                candidate.load_replay_contract(repo)

    def test_additive_jet_review_does_not_admit_legacy_behavior_or_feature_drift(self):
        repo, _ = self.source_fixture()
        # Deliberate semantic mutations, committed in a disposable repository:
        # source qualification must reject these before any candidate is built.
        # The v2 mutation still targets the unchanged v1 decoder: explicit v2
        # admission belongs to SelectedAircraftProfile's separate decoder only.
        mutations = (
            ("crates/flightsim-sim/Cargo.toml", b'default-run = "flightsim-headless"\n', b''),
            ("crates/flightsim-sim/Cargo.toml", b'features = ["raw_value"]',
             b'features = ["raw_value", "float_roundtrip"]'),
            ("crates/flightsim-fdm/src/landing_gear.rs", b'BOTTOM_OUT_STIFFNESS_MULTIPLIER: f64 = 6.0',
             b'BOTTOM_OUT_STIFFNESS_MULTIPLIER: f64 = 7.0'),
            ("crates/flightsim-sim/src/replay/current.rs", b'CURRENT_FORMAT_VERSION => Ok(Self::V3(',
             b'CURRENT_FORMAT_VERSION | 4 => Ok(Self::V3('),
            ("crates/flightsim-app/src/aircraft_profile.rs", b'if self.version != 1 {',
             b'if self.version != 1 && self.version != 2 {'),
            ("crates/flightsim-app/src/replay_policy.rs", b'if !legacy_opt_in {', b'if false {'),
            ("crates/flightsim-sim/src/replay/identity.rs",
             b'Self::Complete(recorded) if recorded == AircraftIdentity::for_config(config) => {',
             b'Self::Complete(_) => {'),
        )
        for relative, old, new in mutations:
            path = repo / relative
            original = path.read_bytes()
            self.assertEqual(original.count(old), 1, relative)
            path.write_bytes(original.replace(old, new))
            expected = self.commit_source_fixture(repo)
            with self.subTest(relative=relative), self.assertRaisesRegex(ValueError, "canonical baseline changed"):
                candidate.source_inputs(repo, expected)
            path.write_bytes(original)
            self.commit_source_fixture(repo)

    def test_turboprop_review_rejects_export_and_existing_dispatch_drift(self):
        repo, _ = self.source_fixture()
        # These are independent source-admission mutations, not execution of
        # changed Rust. Public foundation exports do not enable profile v3 or
        # schema-3 identities in the unchanged app and replay-v4 dispatcher.
        mutations = (
            ("crates/flightsim-fdm/src/lib.rs", b'pub mod turboprop;\n', b''),
            ("crates/flightsim-sim/src/lib.rs", b'pub mod aircraft_profile_v3;\n', b''),
            ("crates/flightsim-sim/src/lib.rs", b'pub mod turboprop_identity;\n', b''),
            ("crates/flightsim-app/src/aircraft_profile.rs",
             b'2 => AircraftProfileV2::from_bytes(bytes)',
             b'2 | 3 => AircraftProfileV2::from_bytes(bytes)'),
            ("crates/flightsim-sim/src/replay_v4.rs",
             b'            self.identity.supported(),\n',
             b'            self.identity.supported() || self.identity.supported_turboprop(),\n'),
            ("crates/flightsim-sim/src/replay_v4.rs",
             b'            identity.supported(),\n',
             b'            identity.supported() || identity.supported_turboprop(),\n'),
        )
        for relative, old, new in mutations:
            path = repo / relative
            original = path.read_bytes()
            self.assertEqual(original.count(old), 1, relative)
            path.write_bytes(original.replace(old, new))
            expected = self.commit_source_fixture(repo)
            with self.subTest(relative=relative, mutation=old), self.assertRaisesRegex(ValueError, "canonical baseline changed"):
                candidate.source_inputs(repo, expected)
            path.write_bytes(original)
            self.commit_source_fixture(repo)

    def test_turboprop_review_cannot_restore_the_prior_additive_fdm_guard(self):
        repo, _ = self.source_fixture()
        path = repo / candidate.REPLAY_CONTRACT_PATH
        contract = json.loads(path.read_text(encoding="utf-8"))
        # The former jet-only root is historical evidence, not an alternative
        # accepted root after the exact one-line turboprop export was reviewed.
        contract["source_sha256"]["crates/flightsim-fdm/src/lib.rs"] = (
            "4d51cf4b5d62ce0f6bcc031df445225ac07c2bad0ed590c4b422ad6cca579462"
        )
        candidate.write_json(path, contract)
        with self.assertRaisesRegex(ValueError, "frozen reviewed FDM module input changed"):
            candidate.load_replay_contract(repo)

    def test_native_dispatch_review_rejects_model_routing_and_legacy_adapter_drift(self):
        repo, _ = self.source_fixture()
        mutations = (
            ("crates/flightsim-app/src/aircraft_profile.rs", b'1 => {', b'1 | 2 => {'),
            ("crates/flightsim-app/src/aircraft_profile.rs",
             b'AircraftProfile::builtin(id).map(Self::Legacy)',
             b'AircraftProfile::builtin("light-single").map(Self::Legacy)'),
            ("crates/flightsim-app/src/flight_session.rs", b'Self::Legacy(s) => s.airspeed(),',
             b'Self::Legacy(_) => MetersPerSecond::ZERO,'),
            ("crates/flightsim-app/src/flight_session.rs",
             b's.agl().get() < flightsim_sim::gear_height(s.config()).get() + 0.3',
             b's.agl().get() < flightsim_sim::gear_height(s.config()).get() + 0.6'),
            ("crates/flightsim-sim/src/replay_v4.rs", b'prefix.extend(version.to_le_bytes());',
             b'prefix.extend(version.to_be_bytes());'),
            ("crates/flightsim-sim/src/replay_v4.rs",
             b'recording.conditions.identity == ModelIdentity::for_jet(&config),', b'true,'),
            ("crates/flightsim-ui/src/lib.rs", b'tutorial_enabled: true,', b'tutorial_enabled: false,'),
            # A committed picker flight now resets both aircraft families. Keep
            # guarding admission and the actual call: neither restricting it to
            # jets nor silently skipping a pending reset is acceptable.
            ("crates/flightsim-app/src/main.rs",
             b'if let Some(mut reset) = jet_hud_reset\n        && reset.0',
             b'if simulation.0.is_jet()\n        && let Some(mut reset) = jet_hud_reset\n        && reset.0'),
            ("crates/flightsim-app/src/main.rs",
             b'&& reset.0\n        && let Some(mut smoothing) = smoothing',
             b'&& false\n        && let Some(mut smoothing) = smoothing'),
            ("crates/flightsim-app/src/main.rs", b'smoothing.reset(&hud);', b'let _ = &hud;'),
            ("crates/flightsim-app/src/main.rs",
             b'warning.update_jet(jet, flaps, angle, valid && !failed);',
             b'warning.update_jet(jet, flaps, angle, valid && !simulation.0.audio_paused());'),
            ("crates/flightsim-app/src/main.rs",
             b'warning.update_jet(jet, flaps, angle, valid && !failed);',
             b'warning.update_jet(jet, flaps, angle, valid);'),
            ("crates/flightsim-ui/src/replay.rs", b'display: Display::None,', b'display: Display::Flex,'),
            ("crates/flightsim-ui/src/replay.rs",
             b'flex_basis: Val::Px(240.0),\n            flex_grow: 1.0,\n            flex_shrink: 0.0,',
             b'flex_basis: Val::Px(240.0),\n            flex_grow: 1.0,\n            flex_shrink: 1.0,'),
        )
        for relative, old, new in mutations:
            path = repo / relative
            original = path.read_bytes()
            self.assertEqual(original.count(old), 1, relative)
            path.write_bytes(original.replace(old, new))
            expected = self.commit_source_fixture(repo)
            with self.subTest(relative=relative), self.assertRaisesRegex(ValueError, "canonical baseline changed"):
                candidate.source_inputs(repo, expected)
            path.write_bytes(original)
            self.commit_source_fixture(repo)

    def test_picker_review_rejects_commercial_replay_generation_and_scene_ownership_drift(self):
        repo, _ = self.source_fixture()
        mutations = (
            ("crates/flightsim-app/src/aircraft_picker_runtime.rs",
             b'if !cfg!(feature = "commercial-staging") {', b'if true {'),
            ("crates/flightsim-app/src/aircraft_picker_runtime.rs",
             b'if cfg!(feature = "commercial-staging") && file != "swift_sport.json" {', b'if false {'),
            ("crates/flightsim-app/src/aircraft_picker_runtime.rs",
             b'profile.is_jet() != (file == "kestrel_jet_trainer.json")', b'false'),
            ("crates/flightsim-app/src/aircraft_picker_runtime.rs",
             b'actions.generation == self.request.generation', b'true'),
            ("crates/flightsim-app/src/aircraft_picker_runtime.rs",
             b'if replay || !map.visible || map.regions.visible {',
             b'if !map.visible || map.regions.visible {'),
            ("crates/flightsim-app/src/aircraft_scene.rs",
             b'if !server.is_loaded_with_dependencies(handle.id()) {', b'if false {'),
            ("crates/flightsim-app/src/aircraft_scene.rs", b'if !spawned {', b'if false {'),
            ("crates/flightsim-app/src/aircraft_scene.rs",
             b'if staged {\n            Visibility::Hidden\n        } else {',
             b'if staged {\n            Visibility::Inherited\n        } else {'),
            ("crates/flightsim-audio/src/lib.rs", b'world.despawn(entity);', b'let _ = entity;'),
        )
        for relative, old, new in mutations:
            path = repo / relative
            original = path.read_bytes()
            self.assertEqual(original.count(old), 1, relative)
            path.write_bytes(original.replace(old, new))
            expected = self.commit_source_fixture(repo)
            with self.subTest(relative=relative), self.assertRaisesRegex(ValueError, "canonical baseline changed"):
                candidate.source_inputs(repo, expected)
            path.write_bytes(original)
            self.commit_source_fixture(repo)

    def test_wind_review_rejects_exact_value_default_seed_and_override_drift(self):
        repo, _ = self.source_fixture()
        mutations = (
            ("crates/flightsim-ui/src/wind_settings.rs",
             b'from_dirty: false,', b'from_dirty: true,'),
            ("crates/flightsim-ui/src/wind_settings.rs",
             b'speed_dirty: false,', b'speed_dirty: true,'),
            ("crates/flightsim-ui/src/wind_settings.rs",
             b'turbulence: None,', b'turbulence: Some(WorldMapTurbulence::Calm),'),
            ("crates/flightsim-ui/src/wind_settings.rs",
             b'value.is_finite() && (0.0..=maximum).contains(value)', b'true'),
            ("crates/flightsim-app/src/conditions_runtime.rs",
             b'if let Some(level) = edit.turbulence {',
             b'if let Some(level) = edit.turbulence {\n            self.turbulence.seed = 0;'),
            ("crates/flightsim-app/src/conditions_runtime.rs",
             b'self.wind.from.get().to_bits() == other.wind.from.get().to_bits()',
             b'self.wind.from == other.wind.from'),
            ("crates/flightsim-app/src/conditions_runtime.rs",
             b'&& self.turbulence.seed == other.turbulence.seed', b'&& true'),
            ("crates/flightsim-app/src/conditions_runtime.rs",
             b'&& self.wind_was_given == other.wind_was_given', b'&& true'),
            ("crates/flightsim-app/src/conditions_runtime.rs",
             b'&& self.turbulence_was_given == other.turbulence_was_given', b'&& true'),
            ("crates/flightsim-app/src/conditions_runtime.rs",
             b'self.wind_was_given = true;', b'self.wind_was_given = false;'),
            ("crates/flightsim-app/src/conditions_runtime.rs",
             b'self.turbulence_was_given = true;', b'self.turbulence_was_given = false;'),
        )
        for relative, old, new in mutations:
            path = repo / relative
            original = path.read_bytes()
            # Each wind component independently sets this same override flag.
            expected_count = 2 if old == b'self.wind_was_given = true;' else 1
            self.assertEqual(original.count(old), expected_count, relative)
            path.write_bytes(original.replace(old, new))
            expected = self.commit_source_fixture(repo)
            with self.subTest(relative=relative, mutation=old), self.assertRaisesRegex(ValueError, "canonical baseline changed"):
                candidate.source_inputs(repo, expected)
            path.write_bytes(original)
            self.commit_source_fixture(repo)

    def test_wind_review_rejects_replay_lan_snapshot_and_generation_drift(self):
        repo, _ = self.source_fixture()
        mutations = (
            ("crates/flightsim-app/src/conditions_runtime.rs",
             b'    !replay\n', b'    true\n'),
            ("crates/flightsim-app/src/conditions_runtime.rs",
             b'&& startup.replay.is_none()', b'&& true'),
            ("crates/flightsim-app/src/conditions_runtime.rs",
             b'&& startup.traffic.host.is_none()', b'&& true'),
            ("crates/flightsim-app/src/conditions_runtime.rs",
             b'&& startup.traffic.join.is_none()', b'&& true'),
            ("crates/flightsim-app/src/conditions_runtime.rs",
             b'if enabled && map.new_flight_modal_ready() {', b'if enabled {'),
            ("crates/flightsim-app/src/conditions_runtime.rs",
             b'actions.invalidate_start();', b'let _ = &actions;'),
            ("crates/flightsim-app/src/conditions_runtime.rs",
             b'.before(world_runtime::apply_world_map_start)',
             b'.after(world_runtime::apply_world_map_start)'),
            ("crates/flightsim-ui/src/wind_settings.rs",
             b'self.invalidate_start_pending = true;', b'self.invalidate_start_pending = false;'),
            ("crates/flightsim-ui/src/world_map.rs",
             b'if keys.just_pressed(KeyCode::Escape) || cancel_wind || close_map {',
             b'if keys.just_pressed(KeyCode::Escape) || close_map {'),
            ("crates/flightsim-app/src/aircraft_picker_runtime.rs",
             b'&& conditions_runtime::snapshot(world, current) == self.conditions', b'&& true'),
            ("crates/flightsim-app/src/aircraft_picker_runtime.rs",
             b'conditions: conditions_runtime::PhysicalConditions::from_startup(&startup),',
             b'conditions: conditions_runtime::PhysicalConditions::from_startup(world.resource::<Startup>()),'),
            ("crates/flightsim-app/src/world_runtime.rs",
             b'super::conditions_runtime::snapshot(world, startup).apply(startup);',
             b'let _ = super::conditions_runtime::snapshot(world, startup);'),
        )
        for relative, old, new in mutations:
            path = repo / relative
            original = path.read_bytes()
            self.assertEqual(original.count(old), 1, relative)
            path.write_bytes(original.replace(old, new))
            expected = self.commit_source_fixture(repo)
            with self.subTest(relative=relative, mutation=old), self.assertRaisesRegex(ValueError, "canonical baseline changed"):
                candidate.source_inputs(repo, expected)
            path.write_bytes(original)
            self.commit_source_fixture(repo)

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
                             (SWIFT_LOG.replace("(swift-sport)", "(kestrel-jet-trainer)"), 0),
                             (SWIFT_LOG.replace("extracted/swift-candidate/", "developer-copy/"), 0),
                             (SWIFT_LOG.replace("swift_sport.glb", "kestrel_jet_trainer.glb"), 0),
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
                      distribution=dict(SWIFT_DISTRIBUTION), limits=[candidate.LEGACY_LIMIT],
                      replay_tests={name: {"status": "passed", "test": test}
                                    for name, test in candidate.REPLAY_ACCEPTANCE_TESTS.items()})
        self.seal_report(evidence, report)
        return evidence, source, report

    def test_swift_evidence_cannot_qualify_jet_selection_or_another_distribution(self):
        evidence, _, report = self.successful_evidence_fixture()
        candidate.validate_evidence(evidence)
        mutations = [(field, None) for field in SWIFT_DISTRIBUTION if field != "region_downloads"]
        mutations.extend([
            ("default_aircraft", "kestrel-jet-trainer"),
            ("default_model", "aircraft/kestrel_jet_trainer.glb"),
            ("bundled_aircraft", ["swift-sport", "kestrel-jet-trainer"]),
            ("bundled_aircraft", ["kestrel-jet-trainer"]),
            ("profile", "development"), ("target_os", "linux"), ("target_env", "gnu"),
            ("target_arch", "aarch64"), ("schema_version", True), ("schema_version", 1.0),
            ("release_authorized", 0), ("release_authorized", True), ("package_version", ""),
        ])
        for field, value in mutations:
            changed = {**SWIFT_DISTRIBUTION, field: value}
            if value is None:
                del changed[field]
            with self.subTest(field=field, value=value), self.assertRaisesRegex(ValueError, "Swift-only"):
                candidate.validate_distribution(changed, dict(changed))
            altered = {**report, "distribution": changed}
            self.seal_report(evidence, altered)
            with self.subTest(exported_field=field, value=value), self.assertRaisesRegex(ValueError, "Swift-only"):
                candidate.validate_evidence(evidence)

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

    def test_cli_readback_diagnostic_requires_explicit_opt_in(self):
        args = ["--repo", str(ROOT), "--expected-source", "a" * 40,
                "--work", str(self.root / "work"), "--evidence", str(self.root / "evidence")]
        for flags, enabled in (([], False), (["--diagnose-readback"], True)):
            with self.subTest(enabled=enabled), patch.object(candidate, "run_candidate") as run:
                self.assertEqual(candidate.main(args + flags), 0)
                run.assert_called_once_with(ROOT.resolve(), "a" * 40, (self.root / "work").resolve(),
                                            (self.root / "evidence").resolve(), diagnose_readback=enabled)

    def capture_fixture(self, first="timeout", probe="success", probe_events="complete"):
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
        primary = (OSError("injected primary launcher failure") if first == "error" else
                   subprocess.TimeoutExpired(candidate.capture_command(app, work / candidate.PNG_NAME), 180))

        def run(command, **kwargs):
            calls.append((command, kwargs))
            self.assertEqual(kwargs["cwd"], unrelated)
            self.assertEqual(kwargs["timeout"], 180)
            self.assertIs(kwargs["runtime"], True)
            self.assertEqual(command[0], str(app))
            self.assertNotIn("release_parity", kwargs)
            self.assertNotIn("creationflags", kwargs)
            self.assertNotIn("startupinfo", kwargs)
            outcome = first if len(calls) == 1 else probe
            if len(calls) > 1:
                self.assertEqual(len(calls), 2, "probe must never loop")
                self.assertEqual(command, candidate.capture_command(app, work / candidate.PROBE_PNG_NAME)
                                 + ["--windows-readback-diagnostic"])
                self.assertIsNone(kwargs["accepted"])
                log = SWIFT_LOG + readback_fixture(probe_events)
            else:
                self.assertNotIn("--windows-readback-diagnostic", command)
                log = SWIFT_LOG
            if outcome == "error":
                raise primary if len(calls) == 1 else OSError("injected diagnostic launcher failure")
            if outcome == "timeout":
                log = "INFO capturing a screenshot\nTRACE still waiting\n" + (readback_fixture(probe_events) if len(calls) == 2 else "")
                kwargs["output"].write_text(log, encoding="utf-8")
                Path(command[2]).write_bytes(b"partial private screenshot")
                raise primary if len(calls) == 1 else subprocess.TimeoutExpired(command, 180)
            if outcome == "bad_log":
                log += "ERROR injected asset failure\n"
            kwargs["output"].write_text(log, encoding="utf-8")
            if outcome in ("success", "bad_log"):
                Path(command[2]).write_bytes(png_bytes())
            elif outcome == "invalid_png":
                Path(command[2]).write_bytes(b"invalid PNG")
            return subprocess.CompletedProcess(command, 2 if outcome == "failure" else 0), log

        return run, app, unrelated, work, evidence, report, calls, primary

    def seal_report(self, evidence, report):
        report["evidence_files"] = {
            p.name: {"bytes": p.stat().st_size, "sha256": candidate.digest(p)}
            for p in evidence.iterdir() if p.name != "acceptance.json"
        }
        candidate.write_json(evidence / "acceptance.json", report)

    def finish_failed_capture(self, evidence, report):
        report["failure"] = report["primary_capture_failure"]["message"]
        self.seal_report(evidence, report)
        candidate.validate_evidence(evidence)

    def test_default_primary_failure_does_not_launch_probe(self):
        root = self.root
        for first in ("timeout", "error", "failure", "bad_log", "invalid_png", "missing_png"):
            self.root = root / first
            self.root.mkdir()
            with self.subTest(first=first):
                run, app, cwd, work, evidence, report, calls, primary = self.capture_fixture(first=first)
                with patch.object(candidate, "record_readback_probe") as probe:
                    with self.assertRaises(candidate.CAPTURE_ERRORS) as failed:
                        candidate.check_default_capture(run, ROOT, app, cwd, work, evidence, report)
                    probe.assert_not_called()
                if first in ("timeout", "error"):
                    self.assertIs(failed.exception, primary)
                self.assertEqual(len(calls), 1)
                self.assertEqual(calls[0][1]["timeout"], 180)
                self.assertEqual(report["status"], "failed")
                self.assertEqual(report["checks"], {})
                self.assertEqual(report["primary_capture_failure"]["message"],
                                 candidate.sanitize(str(failed.exception), ROOT, work))
                self.assertNotIn("diagnostics", report)
                self.assertFalse((evidence / candidate.PNG_NAME).exists())
                self.assertFalse(any((directory / name).exists() for directory in (work, evidence) for name in (
                    candidate.PROBE_LOG_NAME, candidate.PROBE_JSON_NAME, candidate.PROBE_PNG_NAME)))
                self.finish_failed_capture(evidence, report)

    def test_primary_only_failure_evidence_keeps_identity_and_failure_guards(self):
        run, app, cwd, work, evidence, report, _, _ = self.capture_fixture()
        with self.assertRaises(subprocess.TimeoutExpired):
            candidate.check_default_capture(run, ROOT, app, cwd, work, evidence, report)
        self.finish_failed_capture(evidence, report)
        for key, value in (("timeout_seconds", 181), ("log_sha256", "0" * 64),
                           ("command", candidate.diagnostic_capture_command("other.exe", "other.png")),
                           ("message", "different failure")):
            changed = json.loads(json.dumps(report))
            changed["primary_capture_failure"][key] = value
            self.seal_report(evidence, changed)
            with self.subTest(key=key), self.assertRaises(ValueError):
                candidate.validate_evidence(evidence)
        self.seal_report(evidence, report)
        (evidence / candidate.PROBE_LOG_NAME).write_text("unexpected probe", encoding="utf-8")
        self.seal_report(evidence, report)
        with self.assertRaisesRegex(ValueError, "lack a primary failure/probe record"):
            candidate.validate_evidence(evidence)

    def test_explicit_opt_in_runs_one_bounded_probe_and_preserves_primary_failure(self):
        run, app, cwd, work, evidence, report, calls, primary = self.capture_fixture()
        with self.assertRaises(subprocess.TimeoutExpired) as failed:
            candidate.check_default_capture(run, ROOT, app, cwd, work, evidence, report, diagnose_readback=True)
        self.assertIs(failed.exception, primary)
        self.assertEqual(len(calls), 2)
        self.assertEqual(report["status"], "failed")
        self.assertEqual(report["checks"], {})
        probe = report["diagnostics"]["readback_probe"]
        self.assertEqual(probe["status"], "captured")
        self.assertEqual(probe["attempts"], 1)
        self.assertEqual(probe["timeout_seconds"], 180)
        self.assertEqual(probe["executable_sha256"], report["executable_sha256"])
        self.assertFalse(probe["qualifies_acceptance"])
        self.assertFalse((evidence / candidate.PNG_NAME).exists())
        self.assertTrue((evidence / candidate.PROBE_PNG_NAME).exists())
        self.finish_failed_capture(evidence, report)
        report["checks"]["default_swift"] = {"status": "passed", "exit_code": 0}
        self.seal_report(evidence, report)
        with self.assertRaisesRegex(ValueError, "cannot replace or pass primary failure"):
            candidate.validate_evidence(evidence)

    def test_probe_timeout_is_single_and_partial_images_never_enter_evidence(self):
        run, app, cwd, work, evidence, report, calls, primary = self.capture_fixture(probe="timeout", probe_events="no_summary")
        with self.assertRaises(subprocess.TimeoutExpired) as failed:
            candidate.check_default_capture(run, ROOT, app, cwd, work, evidence, report, diagnose_readback=True)
        self.assertIs(failed.exception, primary)
        self.assertEqual(len(calls), 2)
        self.assertEqual(report["diagnostics"]["readback_probe"]["status"], "timed_out")
        self.assertFalse(any((evidence / name).exists() for name in candidate.PNG_EVIDENCE))
        self.assertTrue((work / candidate.PROBE_PNG_NAME).exists())
        self.finish_failed_capture(evidence, report)
        document = candidate.readback_json(evidence / candidate.PROBE_JSON_NAME)
        self.assertIsNone(document["summary"])
        self.assertEqual(document["observations"]["poll"], "entered_without_return")
        self.assertFalse(document["observations"]["gpu_completion_observed"])

    def test_primary_success_never_launches_probe(self):
        root = self.root
        for enabled in (False, True):
            self.root = root / str(enabled)
            self.root.mkdir()
            with self.subTest(diagnose_readback=enabled):
                run, app, cwd, work, evidence, report, calls, _ = self.capture_fixture(first="success")
                candidate.check_default_capture(run, ROOT, app, cwd, work, evidence, report,
                                                diagnose_readback=enabled)
                self.assertEqual(len(calls), 1)
                self.assertNotIn("diagnostics", report)
                self.assertIn("default_swift", report["checks"])
                self.assertFalse(any((evidence / name).exists() for name in (
                    candidate.PROBE_LOG_NAME, candidate.PROBE_JSON_NAME, candidate.PROBE_PNG_NAME)))

    def test_non_timeout_primary_capture_failures_also_get_exactly_one_probe(self):
        root = self.root
        for first in ("failure", "bad_log", "invalid_png", "missing_png", "error"):
            self.root = root / first
            self.root.mkdir()
            with self.subTest(first=first):
                run, app, cwd, work, evidence, report, calls, primary = self.capture_fixture(first=first)
                with self.assertRaises((ValueError, OSError)) as failed:
                    candidate.check_default_capture(run, ROOT, app, cwd, work, evidence, report, diagnose_readback=True)
                if first == "error":
                    self.assertIs(failed.exception, primary)
                self.assertEqual(len(calls), 2)
                self.assertEqual(report["primary_capture_failure"]["kind"], "capture_failed")
                self.assertEqual(report["primary_capture_failure"]["message"], candidate.sanitize(str(failed.exception), ROOT, work))
                self.assertEqual(report["checks"], {})
                self.finish_failed_capture(evidence, report)

    def test_diagnostic_preparation_error_cannot_replace_original_timeout(self):
        run, app, cwd, work, evidence, report, calls, primary = self.capture_fixture()
        original_digest = candidate.digest

        def fail_probe_preparation(path):
            if path.name == "default-swift.log":
                raise OSError("injected diagnostic hash read failure")
            return original_digest(path)

        with patch.object(candidate, "digest", side_effect=fail_probe_preparation):
            with self.assertRaises(subprocess.TimeoutExpired) as failed:
                candidate.check_default_capture(run, ROOT, app, cwd, work, evidence, report, diagnose_readback=True)
        self.assertIs(failed.exception, primary)
        self.assertEqual(len(calls), 1)
        self.assertEqual(report["checks"], {})
        self.assertEqual(report["diagnostics"]["readback_probe"]["attempts"], 0)
        self.assertIn("hash read failure", report["diagnostics"]["readback_probe"]["failure"])

    def test_both_launcher_errors_without_output_still_export_honest_evidence(self):
        run, app, cwd, work, evidence, report, calls, primary = self.capture_fixture(first="error", probe="error")
        with self.assertRaises(OSError) as failed:
            candidate.check_default_capture(run, ROOT, app, cwd, work, evidence, report, diagnose_readback=True)
        self.assertIs(failed.exception, primary)
        self.assertEqual(len(calls), 2)
        self.finish_failed_capture(evidence, report)
        probe = report["diagnostics"]["readback_probe"]
        self.assertEqual(probe["attempts"], 1)
        self.assertEqual(probe["status"], "failed")
        self.assertNotIn("exit_code", probe)
        self.assertNotIn("exit_code", report["primary_capture_failure"])
        for name in ("default-swift.log", candidate.PROBE_LOG_NAME):
            self.assertIn("No process output was captured", (evidence / name).read_text())
        document = candidate.readback_json(evidence / candidate.PROBE_JSON_NAME)
        self.assertEqual(document["events"], [])
        self.assertIsNone(document["summary"])

    def test_probe_observations_separate_gpu_map_async_and_absent_summary(self):
        expected = {
            "complete": (True, "ok", "valid", True, "wait_succeeded"),
            "gpu_timeout": (False, "missing", "missing", True, "timeout"),
            "map_error": (True, "error", "missing", True, "wait_succeeded"),
            "map_missing": (True, "missing", "missing", True, "wait_succeeded"),
            "async_not_resumed": (True, "ok", "valid", False, "wait_succeeded"),
            "no_summary": (False, "missing", "missing", False, "entered_without_return"),
            "queue_only": (False, "missing", "missing", False, "not_entered"),
            "pixels_only": (True, "ok", "valid", False, "not_entered"),
            "empty": (False, "missing", "missing", False, "not_entered"),
        }
        root = self.root
        for outcome, values in expected.items():
            self.root = root / outcome
            self.root.mkdir()
            with self.subTest(outcome=outcome):
                run, app, cwd, work, evidence, report, calls, primary = self.capture_fixture(probe="failure", probe_events=outcome)
                with self.assertRaises(subprocess.TimeoutExpired) as failed:
                    candidate.check_default_capture(run, ROOT, app, cwd, work, evidence, report, diagnose_readback=True)
                self.assertIs(failed.exception, primary)
                self.assertEqual(len(calls), 2)
                probe = report["diagnostics"]["readback_probe"]
                self.assertEqual((probe["status"], probe["exit_code"]), ("failed", 2))
                self.assertIs(probe["qualifies_acceptance"], False)
                self.finish_failed_capture(evidence, report)
                observations = candidate.readback_json(evidence / candidate.PROBE_JSON_NAME)["observations"]
                self.assertEqual(tuple(observations[key] for key in (
                    "gpu_completion_observed", "map_callback", "pixels", "async_resumed", "poll")), values)

    def test_remaining_readback_outcomes_preserve_original_failure(self):
        incomplete = readback_fixture("gpu_timeout")
        never_started = "".join(line for line in incomplete.splitlines(keepends=True)
                                if not line.startswith("FS_READBACK_PROBE event=async_"))
        for event in ("async_started", "async_waiting", "async_signal", "async_resumed"):
            never_started = never_started.replace(event + "=true", event + "=false")
        cases = {
            "queue_empty": (incomplete.replace("status=timeout", "status=queue_empty")
                            .replace("poll=timeout", "poll=queue_empty"),
                            (True, "missing", "missing", True, "queue_empty")),
            "wrong_submission": (incomplete.replace("status=timeout", "status=wrong_submission")
                                  .replace("poll=timeout", "poll=wrong_submission"),
                                  (False, "missing", "missing", True, "wrong_submission")),
            "unexpected_poll": (incomplete.replace("status=timeout", "status=unexpected_poll")
                                 .replace("poll=timeout", "poll=unexpected_poll"),
                                 (False, "missing", "missing", True, "unexpected_poll")),
            "invalid_pixels": (readback_fixture("pixels_only").replace("valid=true", "valid=false"),
                               (False, "ok", "invalid", False, "not_entered")),
            "async_never_started": (never_started, (False, "missing", "missing", False, "timeout")),
        }
        root = self.root
        for outcome, (log, expected) in cases.items():
            self.root = root / outcome
            self.root.mkdir()
            with self.subTest(outcome=outcome), patch(__name__ + ".readback_fixture", return_value=log):
                run, app, cwd, work, evidence, report, calls, primary = self.capture_fixture(probe="failure")
                with self.assertRaises(subprocess.TimeoutExpired) as failed:
                    candidate.check_default_capture(run, ROOT, app, cwd, work, evidence, report, diagnose_readback=True)
                self.assertIs(failed.exception, primary)
                self.assertEqual(len(calls), 2)
                self.assertEqual(report["checks"], {})
                self.assertIs(report["diagnostics"]["readback_probe"]["qualifies_acceptance"], False)
                self.finish_failed_capture(evidence, report)
                observations = candidate.readback_json(evidence / candidate.PROBE_JSON_NAME)["observations"]
                self.assertEqual(tuple(observations[key] for key in (
                    "gpu_completion_observed", "map_callback", "pixels", "async_resumed", "poll")), expected)

    def test_no_summary_png_is_only_nonqualifying_capture_evidence(self):
        run, app, cwd, work, evidence, report, _, primary = self.capture_fixture(probe_events="empty")
        with self.assertRaises(subprocess.TimeoutExpired) as failed:
            candidate.check_default_capture(run, ROOT, app, cwd, work, evidence, report, diagnose_readback=True)
        self.assertIs(failed.exception, primary)
        self.finish_failed_capture(evidence, report)
        self.assertIsNone(candidate.readback_json(evidence / candidate.PROBE_JSON_NAME)["summary"])
        self.assertTrue((evidence / candidate.PROBE_PNG_NAME).exists())
        self.assertEqual(report["checks"], {})

    def test_v2_startup_partial_timeout_and_success_remain_separate_observations(self):
        for outcome, expected in (("complete", (True, "ok", "valid", "queue_empty")),
                                  ("timeout", (False, "missing", "missing", "timeout")),
                                  ("entered_without_return", (False, "missing", "missing", "entered_without_return"))):
            with self.subTest(outcome=outcome):
                parsed = candidate.parse_readback_log(readback_v2_fixture(outcome, scene=False, late=False))
                self.assertEqual((parsed["schema_version"], parsed["protocol"]), (2, "FS_READBACK_PROBE/v2"))
                self.assertEqual(tuple(parsed["observations"]["prescene"][key] for key in
                                       ("gpu_completion_observed", "map_callback", "pixels", "poll")), expected)
                self.assertFalse(parsed["observations"]["gpu_completion_observed"])
                self.assertIsNone(parsed["summary"])
                self.assertIsNone(parsed["scene_summary"])
        self.assertEqual(candidate.parse_readback_log(readback_fixture("complete"))["schema_version"], 1)

    def test_v2_sparse_callbacks_allow_slow_frames_and_both_cleanup_triggers(self):
        for late in (False, True):
            with self.subTest(late=late):
                parsed = candidate.parse_readback_log(readback_v2_fixture(late=late))
                self.assertEqual(parsed["observations"]["scene"]["registered_frames"], [1, 2, 4, 8, 16, 30])
                self.assertEqual(parsed["observations"]["scene"]["completed_frames"], [1, 2, 4, 8, 16, 30])
                self.assertEqual(parsed["observations"]["scene"]["completion_elapsed_ms"]["30"], 30001)
                self.assertEqual(parsed["scene_summary"]["reason"], "late_probe" if late else "deadline")
        partial = readback_v2_fixture().split("FS_READBACK_PROBE event=scene_register frame=4", 1)[0]
        parsed = candidate.parse_readback_log(partial)
        self.assertEqual(parsed["observations"]["scene"]["registered_frames"], [1, 2])
        self.assertIsNone(parsed["scene_summary"])
        self.assertIsNone(parsed["summary"])

    def test_v2_callbacks_can_be_synchronous_and_cancelled_callbacks_stay_inert(self):
        original = readback_v2_fixture(scene=False, late=False)
        callback = "FS_READBACK_PROBE event=prescene_map_callback result=ok cancelled=false phase=startup elapsed_ms=2\n"
        synchronous = original.replace(callback, "").replace("FS_READBACK_PROBE event=prescene_map_registered",
                          callback.replace("elapsed_ms=2", "elapsed_ms=1") + "FS_READBACK_PROBE event=prescene_map_registered")
        self.assertEqual(candidate.parse_readback_log(synchronous)["observations"]["prescene"]["pixels"], "valid")
        timed_out = readback_v2_fixture("timeout")
        late_map = "FS_READBACK_PROBE event=prescene_map_callback result=error cancelled=true phase=cleanup elapsed_ms=5001\n"
        timed_out = timed_out.replace(late_map, "") + late_map.replace("result=error", "result=ok").replace("elapsed_ms=5001", "elapsed_ms=50000")
        for frame in (1, 2, 4, 8, 16, 30):
            timed_out += f"FS_READBACK_PROBE event=scene_callback frame={frame} cancelled=true phase=owner_released elapsed_ms=50000\n"
        parsed = candidate.parse_readback_log(timed_out)
        self.assertEqual(parsed["observations"]["scene"]["completed_frames"], [])
        self.assertEqual(parsed["prescene_summary"]["map_callback"], "missing")
        self.assertFalse(parsed["observations"]["prescene"]["gpu_completion_observed"])
        cancelled_before_summary = timed_out.replace("FS_READBACK_PROBE event=scene_callback frame=1 cancelled=true phase=owner_released elapsed_ms=50000\n", "")
        cancelled_before_summary = cancelled_before_summary.replace("FS_READBACK_PROBE event=scene_summary", "FS_READBACK_PROBE event=scene_callback frame=1 cancelled=true phase=owner_released elapsed_ms=45000\nFS_READBACK_PROBE event=scene_summary")
        self.assertEqual(candidate.parse_readback_log(cancelled_before_summary)["scene_summary"]["completed"], 0)

    def test_v2_rejects_unknown_duplicate_unbounded_and_contradictory_events(self):
        base = readback_v2_fixture()
        changes = [
            base.replace("version=2", "version=3"),
            base.replace("version=2", "version=02"),
            base.replace("version=2", "version=2 arbitrary=true"),
            base.replace("event=prescene_begin", "event=prescene_unknown"),
            base.replace("event=prescene_submitted", "event=prescene_submitted arbitrary=true"),
            base.replace("gpu_timeout_ms=5000", "gpu_timeout_ms=5001"),
            base.replace("wall_ms=2 elapsed_ms=3", "wall_ms=5000 elapsed_ms=3"),
            base.replace("prescene_pixels valid=true count=16", "prescene_pixels valid=true count=15"),
            base.replace("prescene_summary submitted=true map_callback=ok", "prescene_summary submitted=true map_callback=missing"),
            base.replace("prescene_summary submitted=true map_callback=ok pixels=valid", "prescene_summary submitted=true map_callback=ok pixels=missing"),
            base.replace("event=prescene_poll_return status=queue_empty", "event=prescene_poll_return status=timeout"),
            base.replace("phase=startup", "phase=cleanup"),
            base.replace("prescene_pixels valid=true count=16 elapsed_ms=3", "prescene_pixels valid=true count=16 elapsed_ms=1"),
            base.replace("prescene_summary submitted=true map_callback=ok pixels=valid poll=queue_empty cleanup=true elapsed_ms=3", "prescene_summary submitted=true map_callback=ok pixels=valid poll=queue_empty cleanup=true elapsed_ms=2"),
            base.replace("reason=late_probe elapsed_ms=45000", "reason=late_probe elapsed_ms=29000"),
            base.replace("frame=30", "frame=31"),
            base.replace("frame=2 ", "frame=4 "),
            base.replace("registered=6 completed=6", "registered=6 completed=5"),
            base.replace("registered=6 completed=6", "registered=7 completed=6"),
            base.replace("reason=late_probe elapsed_ms=45000", "reason=deadline elapsed_ms=45000"),
            base.replace("scene_register frame=30 elapsed_ms=30000", "scene_register frame=30 elapsed_ms=60000"),
            base.replace("scene_callback frame=30 cancelled=false phase=active elapsed_ms=30001", "scene_callback frame=30 cancelled=false phase=active elapsed_ms=29999"),
            base.replace("scene_callback frame=30 cancelled=false", "scene_callback frame=30 cancelled=true"),
            base.replace("FS_READBACK_PROBE event=prescene_begin\n", ""),
            base.replace("FS_READBACK_PROBE event=prescene_map_registered elapsed_ms=1\n", ""),
            base.replace("FS_READBACK_PROBE event=prescene_poll_return status=queue_empty wall_ms=2 elapsed_ms=3\n", ""),
            base.replace("FS_READBACK_PROBE event=scene_register frame=1 elapsed_ms=1000\n", ""),
            base + "FS_READBACK_PROBE event=scene_register frame=1 elapsed_ms=46000\n",
            base + "FS_READBACK_PROBE event=prescene_begin\n",
            base + "FS_READBACK_PROBE event=scene_callback frame=1 cancelled=true phase=owner_released elapsed_ms=180001\n",
            readback_v2_fixture(late=False).replace("reason=deadline elapsed_ms=60000", "reason=late_probe elapsed_ms=60000"),
        ]
        for index, log in enumerate(changes):
            with self.subTest(index=index), self.assertRaises(ValueError):
                candidate.parse_readback_log(log)
        with self.assertRaisesRegex(ValueError, "count exceeds bound"):
            candidate.parse_readback_log("FS_READBACK_PROBE event=enabled version=2\n" * 65)

    def test_v2_cutoff_and_late_summary_clocks_cannot_gain_false_credit(self):
        prefix = readback_v2_fixture(scene=False, late=True)
        with self.assertRaisesRegex(ValueError, "cleanup lacks deadline"):
            candidate.parse_readback_log(prefix + "FS_READBACK_PROBE event=scene_summary registered=0 completed=0 cleanup=true reason=late_probe elapsed_ms=1\n")
        log = readback_v2_fixture(late=False)
        with self.assertRaisesRegex(ValueError, "active scene callback after deadline"):
            candidate.parse_readback_log(log.replace("scene_callback frame=30 cancelled=false phase=active elapsed_ms=30001", "scene_callback frame=30 cancelled=false phase=active elapsed_ms=60000"))
        late = log.replace("scene_callback frame=30 cancelled=false phase=active elapsed_ms=30001", "scene_callback frame=30 cancelled=true phase=deadline elapsed_ms=60000").replace("registered=6 completed=6", "registered=6 completed=5")
        parsed = candidate.parse_readback_log(late)
        self.assertEqual(parsed["observations"]["scene"]["completed_frames"], [1, 2, 4, 8, 16])
        self.assertEqual(parsed["scene_summary"]["reason"], "deadline")
        # Independent clocks use integer milliseconds; allow one ms of rounding.
        quantized = readback_v2_fixture(scene=False, late=False).replace("wall_ms=2 elapsed_ms=3", "wall_ms=3 elapsed_ms=3")
        self.assertEqual(candidate.parse_readback_log(quantized)["prescene_summary"]["poll"], "queue_empty")

    def test_v2_owner_teardown_and_released_phases_allow_partial_evidence(self):
        prefix = readback_v2_fixture(scene=False, late=False) + "FS_READBACK_PROBE event=scene_register frame=1 elapsed_ms=1000\n"
        for phase in ("owner_teardown", "owner_released"):
            log = prefix + f"FS_READBACK_PROBE event=scene_callback frame=1 cancelled=true phase={phase} elapsed_ms=1001\n"
            parsed = candidate.parse_readback_log(log)
            self.assertIsNone(parsed["scene_summary"])
            self.assertEqual(parsed["observations"]["scene"]["completed_frames"], [])
        for phase in ("active", "deadline", "late_probe", "unknown"):
            with self.subTest(phase=phase), self.assertRaises(ValueError):
                candidate.parse_readback_log(prefix + f"FS_READBACK_PROBE event=scene_callback frame=1 cancelled=true phase={phase} elapsed_ms=1001\n")
        base = readback_v2_fixture("timeout")
        for phase in ("owner_teardown", "deadline"):
            with self.subTest(phase=phase), self.assertRaisesRegex(ValueError, "cleanup reason"):
                candidate.parse_readback_log(base + f"FS_READBACK_PROBE event=scene_callback frame=1 cancelled=true phase={phase} elapsed_ms=60000\n")
        for phase in ("late_probe", "owner_released"):
            parsed = candidate.parse_readback_log(base + f"FS_READBACK_PROBE event=scene_callback frame=1 cancelled=true phase={phase} elapsed_ms=50000\n")
            self.assertEqual(parsed["scene_summary"]["completed"], 0)

    def test_v2_cannot_upgrade_primary_failure_or_upload_unproven_png(self):
        root = self.root
        cases = (("success", "complete"), ("timeout", "timeout"), ("timeout", "entered_without_return"), ("bad_log", "complete"))
        for index, (result, startup) in enumerate(cases):
            self.root = root / str(index)
            self.root.mkdir()
            log = readback_v2_fixture(startup)
            with self.subTest(result=result, startup=startup), patch(__name__ + ".readback_fixture", return_value=log):
                run, app, cwd, work, evidence, report, calls, primary = self.capture_fixture(probe=result)
                with self.assertRaises(subprocess.TimeoutExpired) as failed:
                    candidate.check_default_capture(run, ROOT, app, cwd, work, evidence, report, diagnose_readback=True)
                self.assertIs(failed.exception, primary)
                self.assertEqual(len(calls), 2)
                self.assertEqual(report["checks"], {})
                self.assertIs(report["diagnostics"]["readback_probe"]["qualifies_acceptance"], False)
                self.assertEqual((evidence / candidate.PROBE_PNG_NAME).exists(), result == "success")
                self.finish_failed_capture(evidence, report)
                self.assertEqual(candidate.readback_json(evidence / candidate.PROBE_JSON_NAME)["schema_version"], 2)

    def test_late_cancelled_callbacks_never_rewrite_summary_or_prove_completion(self):
        log = readback_fixture("gpu_timeout") + (
            "FS_READBACK_PROBE event=map_callback result=ok cancelled=true phase=cleanup\n"
            "FS_READBACK_PROBE event=queue_callback cancelled=true phase=cleanup\n")
        parsed = candidate.parse_readback_log(log)
        self.assertEqual(parsed["summary"]["map_callback"], "missing")
        self.assertFalse(parsed["summary"]["queue_callback"])
        self.assertFalse(parsed["observations"]["gpu_completion_observed"])
        self.assertEqual(parsed["observations"]["map_callback"], "missing")
        self.assertFalse(parsed["observations"]["queue_callback_observed"])
        with self.assertRaisesRegex(ValueError, "after terminal summary"):
            candidate.parse_readback_log(readback_fixture("gpu_timeout") + "FS_READBACK_PROBE event=pixels valid=true count=16\n")

    def test_cleanup_callbacks_before_summary_are_inert_and_freeze_active_events(self):
        log = readback_fixture("gpu_timeout")
        head, summary = log.split("FS_READBACK_PROBE event=summary", 1)
        cleanup = "FS_READBACK_PROBE event=map_callback result=error cancelled=true phase=cleanup\n"
        parsed = candidate.parse_readback_log(head + cleanup + "FS_READBACK_PROBE event=summary" + summary)
        self.assertEqual(parsed["observations"]["map_callback"], "missing")
        with self.assertRaisesRegex(ValueError, "after cleanup"):
            candidate.parse_readback_log(head + cleanup + "FS_READBACK_PROBE event=queue_callback cancelled=false phase=after_poll\n")

    def test_synchronous_map_and_poll_callback_phases_match_real_order(self):
        log = readback_fixture("complete")
        callback = "FS_READBACK_PROBE event=map_callback result=ok cancelled=false phase=normal\n"
        synchronous = log.replace(callback, "").replace("FS_READBACK_PROBE event=map_registered", callback + "FS_READBACK_PROBE event=map_registered")
        self.assertTrue(candidate.parse_readback_log(synchronous)["observations"]["gpu_completion_observed"])
        for phase, location in (("poll", "FS_READBACK_PROBE event=poll_return"),
                                ("after_poll", "FS_READBACK_PROBE event=summary")):
            moved = callback.replace("phase=normal", "phase=" + phase)
            # Move pixels after the map callback too; its causal proof must be prior.
            pixels = "FS_READBACK_PROBE event=pixels valid=true count=16\n"
            revised = log.replace(callback, "").replace(pixels, "").replace(location, moved + pixels + location)
            self.assertEqual(candidate.parse_readback_log(revised)["observations"]["pixels"], "valid")
            with self.subTest(phase=phase), self.assertRaisesRegex(ValueError, "outside poll|precedes poll return"):
                candidate.parse_readback_log(log.replace(callback, moved))

    def test_only_map_callback_can_precede_registration_return(self):
        base = readback_fixture("complete")
        for event in ("async_started", "queue_callback", "pixels", "summary"):
            lines = base.splitlines(keepends=True)
            line = next(line for line in lines if "event=" + event + " " in line or "event=" + event + "\n" in line)
            lines.remove(line)
            position = lines.index("FS_READBACK_PROBE event=map_registered\n")
            lines.insert(position, line)
            with self.subTest(event=event), self.assertRaisesRegex(ValueError, "precedes causal predecessor|precede map registration"):
                candidate.parse_readback_log("".join(lines))

    def test_readback_protocol_rejects_malformed_and_contradictory_evidence(self):
        base = readback_fixture("complete")
        mutations = [
            base.replace("event=armed", "event=unknown"),
            base + "FS_READBACK_PROBE event=enabled\n",
            base.replace("event=armed", "event=armed ignored=true"),
            base.replace("valid=true count=16", "valid=1 count=16"),
            base.replace("count=16", "count=15"),
            base.replace("cancelled=false phase=normal", "cancelled=true phase=normal"),
            base.replace("wall_ms=2", "wall_ms=180001"),
            base.replace("wall_ms=2", "wall_ms=02"),
            base.replace("event=armed", "event=armed event=armed"),
            base.replace("event=map_register_enter\nFS_READBACK_PROBE event=map_registered", "event=map_registered\nFS_READBACK_PROBE event=map_register_enter"),
            base.replace("event=async_waiting\nFS_READBACK_PROBE event=async_signal", "event=async_signal\nFS_READBACK_PROBE event=async_waiting"),
            base.replace("elapsed_ms=5000", "elapsed_ms=4999"),
            base.replace("elapsed_ms=5000", "elapsed_ms=15000"),
            base.replace("render_frames=120 elapsed_ms=15000", "render_frames=0 elapsed_ms=15000"),
            base.replace("render_frames=120 elapsed_ms=15000", "render_frames=120 elapsed_ms=14999"),
            base.replace("FS_READBACK_PROBE event=armed", "prefix FS_READBACK_PROBE event=armed"),
            base.replace("FS_READBACK_PROBE event=armed\n", ""),
            base.replace("FS_READBACK_PROBE event=poll_enter gpu_timeout_ms=250 elapsed_ms=5000\n", ""),
            base.replace("cleanup=true", "cleanup=false"),
            base.replace("summary submitted=true queue_callback=true", "summary submitted=true queue_callback=false"),
            base.replace("summary submitted=true", "summary submitted=false"),
            base.replace("summary submitted=true", "summary  submitted=true"),
        ]
        for malformed in mutations:
            with self.subTest(malformed=malformed), self.assertRaises(ValueError):
                candidate.parse_readback_log(malformed)

    def test_diagnostic_png_requires_own_valid_proof_and_unchanged_log(self):
        run, app, cwd, work, evidence, report, _, _ = self.capture_fixture()
        with self.assertRaises(subprocess.TimeoutExpired):
            candidate.check_default_capture(run, ROOT, app, cwd, work, evidence, report, diagnose_readback=True)
        self.finish_failed_capture(evidence, report)
        probe = report["diagnostics"]["readback_probe"]
        probe["rust_log"] = "info"
        self.seal_report(evidence, report)
        with self.assertRaisesRegex(ValueError, "identity/launch differs"):
            candidate.validate_evidence(evidence)
        probe["rust_log"] = candidate.CAPTURE_TRACE
        image_proof = {key: probe.pop(key) for key in ("png", "executable_sha256_after")}
        probe["status"] = "failed"
        probe["failure"] = "injected failure"
        self.seal_report(evidence, report)
        with self.assertRaisesRegex(ValueError, "unproven diagnostic image"):
            candidate.validate_evidence(evidence)
        probe["status"] = "captured"
        probe.update(image_proof)
        del probe["failure"]
        log = evidence / candidate.PROBE_LOG_NAME
        log.write_text(log.read_text() + "ERROR capture failed", encoding="utf-8")
        self.seal_report(evidence, report)
        with self.assertRaisesRegex(ValueError, "diagnostic log changed"):
            candidate.validate_evidence(evidence)
        probe["log_sha256"] = candidate.digest(log)
        candidate.write_json(evidence / candidate.PROBE_JSON_NAME, candidate.readback_document(log))
        probe["json_sha256"] = candidate.digest(evidence / candidate.PROBE_JSON_NAME)
        self.seal_report(evidence, report)
        with self.assertRaisesRegex(ValueError, "runtime logged"):
            candidate.validate_evidence(evidence)
        log.write_text(SWIFT_LOG + readback_fixture("complete"), encoding="utf-8")
        probe["log_sha256"] = candidate.digest(log)
        candidate.write_json(evidence / candidate.PROBE_JSON_NAME, candidate.readback_document(log))
        probe["json_sha256"] = candidate.digest(evidence / candidate.PROBE_JSON_NAME)
        (evidence / candidate.PROBE_PNG_NAME).write_bytes(png_bytes() + b"hidden bytes")
        self.seal_report(evidence, report)
        with self.assertRaisesRegex(ValueError, "trailing bytes"):
            candidate.validate_evidence(evidence)

    def test_diagnostic_json_tampering_is_rejected_even_after_rehash(self):
        run, app, cwd, work, evidence, report, _, _ = self.capture_fixture()
        with self.assertRaises(subprocess.TimeoutExpired):
            candidate.check_default_capture(run, ROOT, app, cwd, work, evidence, report, diagnose_readback=True)
        self.finish_failed_capture(evidence, report)
        path = evidence / candidate.PROBE_JSON_NAME
        original = path.read_text()
        for mutation in (original.replace('"gpu_completion_observed": true', '"gpu_completion_observed": false'),
                         original.replace('"gpu_completion_observed": true', '"gpu_completion_observed": 1'),
                         original.replace('"schema_version": 1', '"schema_version": 1, "schema_version": 1'),
                         original.replace('"schema_version": 1', '"schema_version": 1, "extra": "secret"')):
            path.write_text(mutation, encoding="utf-8")
            report["diagnostics"]["readback_probe"]["json_sha256"] = candidate.digest(path)
            self.seal_report(evidence, report)
            with self.subTest(mutation=mutation), self.assertRaisesRegex(ValueError, "log projection|duplicate diagnostic JSON"):
                candidate.validate_evidence(evidence)
        path.write_text(" " * (candidate.PROBE_MAX_JSON_BYTES + 1) + original, encoding="utf-8")
        report["diagnostics"]["readback_probe"]["json_sha256"] = candidate.digest(path)
        self.seal_report(evidence, report)
        with self.assertRaisesRegex(ValueError, "JSON exceeds size bound"):
            candidate.validate_evidence(evidence)

    def test_malformed_probe_log_cannot_hide_primary_error_or_export_png(self):
        run, app, cwd, work, evidence, report, calls, primary = self.capture_fixture(probe_events="malformed")
        with self.assertRaises(subprocess.TimeoutExpired) as failed:
            candidate.check_default_capture(run, ROOT, app, cwd, work, evidence, report, diagnose_readback=True)
        self.assertIs(failed.exception, primary)
        self.assertEqual(len(calls), 2)
        self.assertFalse((evidence / candidate.PROBE_PNG_NAME).exists())
        report["failure"] = report["primary_capture_failure"]["message"]
        self.seal_report(evidence, report)
        with self.assertRaisesRegex(ValueError, "malformed diagnostic evidence"):
            candidate.validate_evidence(evidence)

    def test_probe_identity_mutations_cannot_change_launcher_or_acceptance(self):
        run, app, cwd, work, evidence, report, _, _ = self.capture_fixture()
        with self.assertRaises(subprocess.TimeoutExpired):
            candidate.check_default_capture(run, ROOT, app, cwd, work, evidence, report, diagnose_readback=True)
        self.finish_failed_capture(evidence, report)
        for key, value in (("qualifies_acceptance", True), ("qualifies_acceptance", 0), ("attempts", 2),
                           ("attempts", True), ("timeout_seconds", 181), ("executable_sha256", "0" * 64),
                           ("launch", {"creationflags": 16, "startupinfo": None}),
                           ("launch", {"creationflags": False, "startupinfo": None}),
                           ("command", report["primary_capture_failure"]["command"])):
            changed = json.loads(json.dumps(report))
            changed["diagnostics"]["readback_probe"][key] = value
            self.seal_report(evidence, changed)
            with self.subTest(key=key), self.assertRaisesRegex(ValueError, "identity/launch differs"):
                candidate.validate_evidence(evidence)

    def test_optional_probe_files_without_original_failure_are_rejected(self):
        evidence = self.root / "evidence"
        evidence.mkdir()
        (evidence / candidate.PROBE_PNG_NAME).write_bytes(png_bytes())
        self.evidence_report(evidence)
        with self.assertRaisesRegex(ValueError, "lack a primary failure/probe record"):
            candidate.validate_evidence(evidence)

    def test_baseline_subprocess_has_no_alternate_console_path(self):
        text = (ROOT / "scripts/check-swift-windows-candidate.py").read_text(encoding="utf-8")
        for removed in ("CREATE_NEW_CONSOLE", "STARTF_USESHOWWINDOW", "release_parity"):
            self.assertNotIn(removed, text)
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
                          "gh release", "git tag", "workflow_dispatch:", "--diagnose-readback"):
            self.assertNotIn(forbidden, text)
        block = text.split("          path: |\n", 1)[1].split("          if-no-files-found:", 1)[0]
        names = {line.strip().rsplit("/", 1)[1] for line in block.splitlines() if line.strip()}
        self.assertEqual(names, candidate.TEXT_EVIDENCE | candidate.PNG_EVIDENCE)
        self.assertNotIn("*", block)
        self.assertIn("steps.evidence.outputs.validated == 'true'", text)
        self.assertNotIn("release.yml", text)


if __name__ == "__main__":
    unittest.main()
