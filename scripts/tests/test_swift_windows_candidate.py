"""Candidate/evidence boundaries without compiling, graphics, network or approval."""
import importlib.util
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
import time
import tempfile
import unittest
from unittest.mock import Mock, patch
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


# Literal independent source-semantic mutations for the reviewed local packages.
# Altered sources are committed only in disposable fixtures and never compiled.
MODIFIED_SOURCE_MUTATIONS = (
    ('Cargo.toml', b'zune-jpeg = { path = "vendor/zune-jpeg" }', b'zune-jpeg = { path = "vendor/unreviewed-jpeg" }'),
    ('Cargo.toml', b'bevy_pbr = { path = "vendor/bevy_pbr" }', b'# use registry bevy_pbr'),
    ('Cargo.lock', b'name = "bevy_pbr"\nversion = "0.18.1"', b'name = "bevy_pbr"\nversion = "0.18.2"'),
    ('Cargo.lock', b'name = "zune-jpeg"\nversion = "0.5.15"', b'name = "zune-jpeg"\nversion = "0.5.15"\nsource = "registry+https://github.com/rust-lang/crates.io-index"'),
    ('vendor/zune-jpeg/Cargo.toml', b'build = false', b'build = true'),
    ('vendor/zune-jpeg/Cargo.toml', b'path = "src/lib.rs"', b'path = "src/unreviewed.rs"'),
    ('vendor/bevy_pbr/Cargo.toml', b'build = false', b'build = true'),
    ('vendor/bevy_pbr/Cargo.toml', b'path = "src/lib.rs"', b'path = "src/unreviewed.rs"'),
    ('vendor/zune-jpeg/Cargo.toml', b'    "x86",', b'    # x86 disabled,'),
    ('vendor/zune-jpeg/src/unsafe_utils_avx2.rs', b'#[target_feature(enable = "avx2")]\n#[inline]\npub unsafe fn transpose', b'#[inline]\npub unsafe fn transpose'),
    ('vendor/zune-jpeg/src/unsafe_utils_avx2.rs', b'let (a0, a1) = exchange_bit!(v0.mm256, v1.mm256, 0xb1, 0xaa);', b'let (a0, a1) = exchange_bit!(v0.mm256, v1.mm256, 0xb0, 0xaa);'),
    ('vendor/zune-jpeg/src/unsafe_utils_avx2.rs', b'let (b0, b2) = exchange_bit!(a0, a2, 0x4e, 0xcc);', b'let (b0, b2) = exchange_bit!(a0, a2, 0x4e, 0xcd);'),
    ('vendor/zune-jpeg/src/unsafe_utils_avx2.rs', b'v0.mm256 = _mm256_permute2x128_si256::<0x20>(b0, b4);', b'v0.mm256 = _mm256_permute2x128_si256::<0x31>(b0, b4);'),
    ('vendor/zune-jpeg/src/unsafe_utils_avx2.rs', b'v7.mm256 = _mm256_permute2x128_si256::<0x31>(b3, b7);', b'v7.mm256 = _mm256_permute2x128_si256::<0x31>(b2, b6);'),
    ('vendor/zune-jpeg/src/lib.rs', b'mod flightsim_transpose_tests;', b'// detached local transpose tests'),
    ('vendor/bevy_pbr/src/render/parallax_mapping.wgsl', b'return textureSampleLevel(', b'return textureSample('),
    ('vendor/bevy_pbr/src/render/parallax_mapping.wgsl', b'let projection_z = max(incidence, 0.0001);', b'let projection_z = incidence;'),
    ('vendor/bevy_pbr/src/render/parallax_mapping.wgsl', b'let maximum_layers = floor(min(max_layer_count, 1024.0));', b'let maximum_layers = floor(max_layer_count);'),
    ('vendor/bevy_pbr/src/render/parallax_mapping.wgsl', b'step < min(max_steps, 24u)', b'step < max_steps'),
    ('vendor/bevy_pbr/src/render/parallax_mapping.wgsl', b'vec2<f32>(direction.x, -direction.y)', b'vec2<f32>(direction.x, direction.y)'),
    ('vendor/bevy_pbr/src/render/parallax_mapping.wgsl', b'let start_height = clamp(sample_depth_map(original_uv, material_bind_group_slot), 0.0, 1.0);', b'let start_height = clamp(sample_depth_map(original_uv, 0u), 0.0, 1.0);'),
    ('vendor/bevy_pbr/src/render/parallax_mapping.wgsl', b'if residual >= 0.0 {', b'if residual <= 0.0 {'),
    ('vendor/bevy_pbr/src/render/parallax_mapping.wgsl', b'return original_uv + hit_depth * ray_uv;', b'return original_uv;'),
    ('vendor/bevy_pbr/src/lib.rs', b'load_shader_library!(app, "render/parallax_mapping.wgsl");', b'// unregistered parallax module'),
    ('vendor/bevy_pbr/src/render/pbr_fragment.wgsl', b'uv = parallaxed_uv(', b'uv = disabled_parallaxed_uv('),
    ('vendor/bevy_pbr/src/lib.rs', b'include_bytes!("bluenoise/stbn.ktx2")', b'include_bytes!("bluenoise/unreviewed.ktx2")'),
    ('.gitattributes', b'/assets/aircraft/light_single.glb binary', b'/assets/aircraft/light_single.glb export-ignore'),
    ('scripts/check-source-archive.py', b'if name in seen:', b'if False:'),
    ('scripts/check-source-archive.py', b'if sha(payload) == DENIED_SHA256:', b'if False:'),
    ('scripts/check-source-archive.py', b'if observed.keys() != expected.keys():', b'if False:'),
    ('scripts/check-source-archive.py', b'if observed[name] != expected[name]:', b'if False:'),
    ('scripts/check-source-archive.py', b"if not key.upper().startswith('GIT_')", b'if True'),
    ('scripts/check-source-archive.py', b"[*git, '-C', str(bare), 'archive', '--format=' + fmt,", b"[*git, '-C', str(repo), 'archive', '--format=' + fmt,"),
    ('scripts/check-source-archive.py', b"'publication_authorized': False", b"'publication_authorized': True"),
    ('tools/validate-parallax-replacement/src/main.rs', b'assert!(matches!(level, naga::SampleLevel::Exact(_)));', b'assert!(true);'),
    ('tools/validate-parallax-replacement/src/main.rs', b'assert_eq!(loops(&function.body), if relief { 2 } else { 1 });', b'assert_eq!(loops(&function.body), loops(&function.body));'),
)


CAPTURE_ADMISSION_MUTATIONS = (
    ('crates/flightsim-app/src/capture_admission.rs', b'if startup.screenshot.is_none() || !startup.exit_after_screenshot {', b'if false {'),
    ('crates/flightsim-app/src/capture_admission.rs', b'.add_systems(ExtractSchedule, extract_admission);', b'.add_systems(Render, extract_admission);'),
    ('crates/flightsim-app/src/capture_admission.rs', b'prepare_view_admission.before(renderer),', b'prepare_view_admission.after(renderer),'),
    ('crates/flightsim-app/src/capture_admission.rs', b'restore_views.after(renderer),', b'restore_views.before(renderer),'),
    ('crates/flightsim-app/src/capture_admission.rs', b'.in_set(RenderSystems::Render),', b'.in_set(RenderSystems::Prepare),'),
    ('crates/flightsim-app/src/capture_admission.rs', b'snapshot: admission.state.snapshot(),', b'snapshot: AdmissionState::default().snapshot(),'),
    ('crates/flightsim-app/src/capture_admission.rs', b'flight_camera: flight_camera.single().ok(),', b'flight_camera: None,'),
    ('crates/flightsim-app/src/capture_admission.rs', b'screenshot_in_flight: !screenshots.is_empty(),', b'screenshot_in_flight: false,'),
    ('crates/flightsim-app/src/capture_admission.rs', b'if !admission.snapshot.draw_views && !admission.screenshot_in_flight {', b'if !admission.snapshot.draw_views {'),
    ('crates/flightsim-app/src/capture_admission.rs', b'frame.saved = Some(std::mem::take(&mut cameras.0));', b'cameras.0.clear();'),
    ('crates/flightsim-app/src/capture_admission.rs', b'let Some(epoch) = admission.snapshot.opportunity_epoch else {', b'let Some(epoch) = Some(1) else {'),
    ('crates/flightsim-app/src/capture_admission.rs', b'let Some(entity) = admission.flight_camera else {', b'let Some(entity) = cameras.0.first().map(|camera| camera.entity) else {'),
    ('crates/flightsim-app/src/capture_admission.rs', b'if !cameras.0.iter().any(|camera| camera.entity == entity) {', b'if false {'),
    ('crates/flightsim-app/src/capture_admission.rs', b'views: Query<(&ExtractedCamera, &ViewTarget), With<Camera3d>>,', b'views: Query<(&ExtractedCamera, &ViewTarget)>, '),
    ('crates/flightsim-app/src/capture_admission.rs', b'let Ok((camera, _target)) = views.get(entity) else {', b'let Ok((camera, _target)) = views.single() else {'),
    ('crates/flightsim-app/src/capture_admission.rs', b'window.physical_width > 0 && window.physical_height > 0', b'true'),
    ('crates/flightsim-app/src/capture_admission.rs', b'if target_valid {', b'if true {'),
    ('crates/flightsim-app/src/capture_admission.rs', b'frame.opportunity = Some((epoch, admission.rendered_epoch.clone()));', b'frame.opportunity = Some((1, admission.rendered_epoch.clone()));'),
    ('crates/flightsim-app/src/capture_admission.rs', b'cameras.0 = saved;', b'cameras.0.clear();'),
    ('crates/flightsim-app/src/capture_admission.rs', b'world.resource_mut::<SortedCameras>().0 = saved;', b'drop(saved);'),
    ('crates/flightsim-app/src/capture_admission.rs', b'admission.rendered_epoch.store(0, Ordering::Release);', b'admission.rendered_epoch.store(1, Ordering::Release);'),
    ('crates/flightsim-app/src/capture_admission.rs', b'std::panic::resume_unwind(failure);', b'drop(failure);'),
    ('crates/flightsim-app/src/capture_admission.rs', b'if let Some((epoch, rendered)) = frame.opportunity.take() {', b'if let Some((epoch, rendered)) = frame.opportunity.clone() {'),
    ('crates/flightsim-app/src/capture_admission.rs', b'rendered.store(epoch, Ordering::Release);', b'rendered.store(1, Ordering::Release);'),
    ('crates/flightsim-app/src/capture_admission.rs', b'if startup.screenshot.is_none() {\n        admission.finish();', b'if false {\n        admission.finish();'),
    ('crates/flightsim-app/src/capture_admission_state.rs', b'if !self.active || self.requested {', b'if false {'),
    ('crates/flightsim-app/src/capture_admission_state.rs', b'if scene_changed || (eligible && self.epoch == 0) {', b'if eligible && self.epoch == 0 {'),
    ('crates/flightsim-app/src/capture_admission_state.rs', b'self.epoch.checked_add(1)', b'Some(self.epoch.wrapping_add(1))'),
    ('crates/flightsim-app/src/capture_admission_state.rs', b'self.opened |= eligible;', b'self.opened = eligible;'),
    ('crates/flightsim-app/src/capture_admission_state.rs', b'Ok(eligible && self.epoch != 0 && rendered_epoch == self.epoch)', b'Ok(eligible)'),
    ('crates/flightsim-app/src/capture_admission_state.rs', b'draw_views: !self.active || self.opened || self.requested,', b'draw_views: self.eligible,'),
    ('crates/flightsim-app/src/capture_admission_state.rs', b'opportunity_epoch: (self.active && self.eligible && !self.requested)', b'opportunity_epoch: (self.active)'),
    ('crates/flightsim-app/src/capture_admission_state.rs', b'self.requested = true;', b'self.requested = false;'),
    ('crates/flightsim-app/src/capture_admission_state.rs', b'self.active = false;', b'self.active = true;'),
    ('crates/flightsim-app/src/screen_capture.rs', b'let eligible = state.elapsed >= startup.screenshot_delay && state.frames >= 30 && stable;', b'let eligible = true;'),
    ('crates/flightsim-app/src/screen_capture.rs', b'if !admission.observe(state.scene_changed, eligible) {', b'if false && !admission.observe(state.scene_changed, eligible) {'),
    ('crates/flightsim-app/src/screen_capture.rs', b'self.scene_changed = !stable && (ready.is_some() || self.previous_ready.is_some());', b'self.scene_changed = false;'),
    ('crates/flightsim-app/src/screen_capture.rs', b'admission.requested();', b'admission.finish();'),
    ('crates/flightsim-app/src/screen_capture.rs', b'if let Some(mut admission) = admission {\n        admission.finish();', b'if let Some(mut admission) = admission {\n        let _ = admission;'),
    ('crates/flightsim-app/src/capture_backpressure.rs', b'crate::capture_admission::with_restored_views(world, |world| {\n            world.run_schedule(Render);\n        });', b'world.run_schedule(Render);'),
    ('crates/flightsim-app/src/capture_backpressure.rs', b'crate::capture_admission::acknowledge_render(world);', b'// omit acknowledgement'),
    ('crates/flightsim-app/src/main.rs', b'#[cfg(not(target_family = "wasm"))]\nmod capture_admission;', b'mod capture_admission;'),
    ('crates/flightsim-app/src/screen_capture.rs', b'crate::capture_admission::configure(app);', b'let _ = app;'),
    ('crates/flightsim-app/src/capture_admission.rs', b'.physical_viewport_size\n        .is_none_or(|size| size.min_element() == 0)', b'.physical_viewport_size\n        .is_none_or(|_| false)'),
    ('crates/flightsim-app/src/capture_admission.rs', b'.physical_target_size\n            .is_none_or(|size| size.min_element() == 0)', b'.physical_target_size\n            .is_none_or(|_| false)'),
    ('crates/flightsim-app/src/capture_admission.rs', b'frame.opportunity = None;\n            frame.saved.take()', b'frame.saved.take()'),
    ('crates/flightsim-app/src/capture_admission.rs', b'self.state.finish();\n        self.rendered_epoch.store(0, Ordering::Release);', b'self.rendered_epoch.store(0, Ordering::Release);'),
    ('crates/flightsim-app/src/capture_admission_state.rs', b'self.finish();\n                return Err("screenshot admission generation exhausted");', b'return Ok(true);'),
    ('crates/flightsim-app/src/capture_admission_state.rs', b'opened: false,', b'opened: true,'),
    ('crates/flightsim-app/src/capture_backpressure.rs', b'world.run_schedule(Render);\n        });\n        crate::capture_admission::acknowledge_render(world);', b'crate::capture_admission::acknowledge_render(world);\n            world.run_schedule(Render);\n        });'),
)


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
        # Detached maintenance can outlive a fixture commit and race temp cleanup.
        # These short-lived repos need no maintenance; leave caller settings alone.
        for key, value in (("gc.auto", "0"), ("maintenance.auto", "false")):
            subprocess.run(["git", "config", "--local", key, value], cwd=repo, check=True)
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
        # Every mutation test starts from an admitted clean positive fixture.
        candidate.source_inputs(repo, candidate.git(repo, "rev-parse", "HEAD"))
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

    def test_source_fixture_commits_do_not_launch_auto_maintenance(self):
        # Prevent the cleanup race seen in CI job 111575494921 at .git/objects.
        trace = self.root / "git-trace.jsonl"
        with patch.dict("os.environ", {"GIT_TRACE2_EVENT": str(trace)}):
            repo, notice = self.source_fixture()
            notice.write_bytes(notice.read_bytes() + b"Another fixture commit.\r\n")
            self.commit_source_fixture(repo)
        events = [json.loads(line) for line in trace.read_text().splitlines()]
        auto_maintenance = [event["argv"] for event in events
                            if event.get("event") == "child_start"
                            and "--auto" in event.get("argv", [])
                            and any(command in event["argv"] for command in ("maintenance", "gc"))]
        self.assertEqual(auto_maintenance, [])

    def test_windows_native_checkout_failure_then_explicit_lf_preserves_raw_notices(self):
        repo, notice = self.source_fixture()
        notice_bytes = notice.read_bytes()
        # Keep the contract LF so the independent implementation byte check is
        # reached. Root .gitattributes is now pinned for archive policy; the
        # disposable local override leaves its canonical bytes unchanged.
        with (repo / ".git/info/attributes").open("w", encoding="utf-8") as attributes:
            attributes.write("scripts/replay-candidate-contract.json text eol=lf\n")
        expected = candidate.git(repo, "rev-parse", "HEAD")
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

    def test_turboprop_replay_review_rejects_export_and_old_codec_dispatch_drift(self):
        repo, _ = self.source_fixture()
        # Pure v5 is separately exported. It must not enter either existing
        # dispatch path or silently disappear from this reviewed sim root.
        # These exact source mutations exercise admission, not Rust execution.
        mutations = (
            ("crates/flightsim-sim/src/lib.rs", b'pub mod replay_v5;\n', b''),
            ("crates/flightsim-sim/src/lib.rs", b'pub mod turboprop_simulation;\n', b''),
            ("crates/flightsim-sim/src/replay_v4.rs",
             b'if version == MODEL_FORMAT_VERSION {',
             b'if version == MODEL_FORMAT_VERSION || version == 5 {'),
            ("crates/flightsim-sim/src/replay/current.rs",
             b'CURRENT_FORMAT_VERSION => Ok(Self::V3(',
             b'CURRENT_FORMAT_VERSION | 5 => Ok(Self::V3('),
        )
        candidate.source_inputs(repo, candidate.git(repo, "rev-parse", "HEAD"))
        for relative, old, new in mutations:
            path = repo / relative
            original = path.read_bytes()
            self.assertEqual(original.count(old), 1, relative)
            path.write_bytes(original.replace(old, new))
            expected = self.commit_source_fixture(repo)
            with self.subTest(relative=relative, mutation=old), self.assertRaisesRegex(
                    ValueError, "canonical baseline changed") as failure:
                candidate.source_inputs(repo, expected)
            self.assertIn(relative, str(failure.exception))
            path.write_bytes(original)
            self.commit_source_fixture(repo)

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
             b'!profile.is_jet()', b'false'),
            ("crates/flightsim-app/src/aircraft_picker_runtime.rs",
             b'!profile.is_legacy()', b'false'),
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

    def test_camera_rebase_helper_and_runtime_witness_reject_committed_drift(self):
        repo, _ = self.source_fixture()
        candidate.source_inputs(repo, candidate.git(repo, "rev-parse", "HEAD"))
        mutations = (
            ("crates/flightsim-input/src/camera.rs",
             b"self.smoothed = map(self.smoothed);", b"self.smoothed = Vec3::ZERO;"),
            ("crates/flightsim-app/src/runtime_tests.rs",
             b"(actual.0 - expected).length() < 0.001,",
             b"(actual.0 - expected).length() < 1.0,"),
        )
        for relative, old, new in mutations:
            self.assertIn(relative, candidate.REPLAY_CONTRACT_PATHS)
            path = repo / relative
            original = path.read_bytes()
            self.assertEqual(original.count(old), 1, relative)
            path.write_bytes(original.replace(old, new))
            expected = self.commit_source_fixture(repo)
            with self.subTest(relative=relative), self.assertRaisesRegex(
                    ValueError, "canonical baseline changed") as failure:
                candidate.source_inputs(repo, expected)
            self.assertIn(relative, str(failure.exception))
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

    def test_cockpit_instruments_reject_shared_width_and_actual_panel_drift(self):
        repo, _ = self.source_fixture()
        relative = "crates/flightsim-ui/src/instruments.rs"
        self.assertIn(relative, candidate.REPLAY_CONTRACT_PATHS)
        path = repo / relative
        original = path.read_bytes()
        # These committed semantic mutations check source admission only. The
        # real-font UI regressions and native pixels have their own evidence.
        mutations = (
            (b"const DIAL_GAP: f32 = 7.0;", b"const DIAL_GAP: f32 = 0.0;"),
            (b"DIAL_GAP * (Instrument::COLUMNS as f32 - 1.0)",
             b"DIAL_GAP * Instrument::COLUMNS as f32"),
            (b"margin: UiRect::left(Val::Px(-PANEL_WIDTH / 2.0)),",
             b"margin: UiRect::left(Val::Px(0.0)),"),
        )
        for old, new in mutations:
            self.assertEqual(original.count(old), 1, relative)
            path.write_bytes(original.replace(old, new))
            expected = self.commit_source_fixture(repo)
            with self.subTest(mutation=old), self.assertRaisesRegex(
                    ValueError, "reviewed replay canonical baseline changed") as failure:
                candidate.source_inputs(repo, expected)
            self.assertIn(relative, str(failure.exception))
            self.assertIn("canonical_sha256=" + candidate.digest(path), str(failure.exception))
            path.write_bytes(original)
            self.commit_source_fixture(repo)

    def test_exported_cockpit_instruments_cannot_be_omitted_or_rehashed(self):
        evidence, source, report = self.successful_evidence_fixture()
        candidate.validate_evidence(evidence)
        relative = "crates/flightsim-ui/src/instruments.rs"
        self.assertIn(relative, candidate.REPLAY_CONTRACT_PATHS)
        for mutation in ("missing file", "missing reviewed record", "changed canonical digest",
                         "changed checkout digests", "removed contract row"):
            changed = json.loads(json.dumps(source))
            if mutation == "missing file":
                changed["files"] = [r for r in changed["files"] if r["path"] != relative]
            elif mutation == "missing reviewed record":
                del changed["reviewed_replay_source_evidence"][relative]
            elif mutation == "changed canonical digest":
                changed["reviewed_replay_source_evidence"][relative]["canonical_sha256"] = "0" * 64
            elif mutation == "changed checkout digests":
                changed["reviewed_replay_source_evidence"][relative]["canonical_sha256"] = "0" * 64
                changed["reviewed_replay_source_evidence"][relative]["checkout_sha256"] = "0" * 64
                for record in changed["files"]:
                    if record["path"] == relative:
                        record["checkout_sha256"] = "0" * 64
            else:
                del changed["replay_contract"]["source_sha256"][relative]
                changed["replay_contract_text"] = json.dumps(changed["replay_contract"])
                changed["replay_contract_sha256"] = candidate.hashlib.sha256(
                    changed["replay_contract_text"].encode()).hexdigest()
                for record in changed["files"]:
                    if record["path"] == candidate.REPLAY_CONTRACT_PATH:
                        record["checkout_sha256"] = changed["replay_contract_sha256"]
                        record["checkout_bytes"] = len(changed["replay_contract_text"].encode())
            candidate.write_json(evidence / "source-inputs.json", changed)
            changed_report = {**report,
                              "source_inputs_sha256": candidate.digest(evidence / "source-inputs.json"),
                              "replay_contract_sha256": changed["replay_contract_sha256"]}
            self.seal_report(evidence, changed_report)
            with self.subTest(mutation=mutation), self.assertRaises(ValueError):
                candidate.validate_evidence(evidence)

    def test_capture_readiness_rejects_order_discovery_and_stability_drift(self):
        repo, _ = self.source_fixture()
        capture = "crates/flightsim-app/src/screen_capture.rs"
        selector = "crates/flightsim-render/src/terrain_selection.rs"
        # Committed semantic mutations exercise the exact source boundary, not
        # Rust execution or a claim that CPU readiness proves GPU completion.
        mutations = (
            ("crates/flightsim-app/src/main.rs",
             b".observe_readiness(capture_readiness.is_some());", b".observe_readiness(true);"),
            (capture, b"app.add_systems(Last, capture_screenshot);",
             b"app.add_systems(Update, capture_screenshot);"),
            (capture, b"ready.is_some() && self.previous_ready == ready", b"ready.is_some()"),
            (capture, b"availability_settled || live_matches_desired", b"true"),
            (capture, b") && !overlays.pending", b") && true"),
            (capture, b"pending_models.is_empty()", b"true"),
            (capture, b"(!runway_required || runway.is_some())", b"true"),
            (selector, b"if primary_reads_possible\n                    &&",
             b"if false\n                    &&"),
            (selector, b"matches!(outcome, LoadOutcome::Missing | LoadOutcome::Failed)",
             b"matches!(outcome, LoadOutcome::Loaded | LoadOutcome::Missing | LoadOutcome::Failed)"),
            (selector, b"frame_budget > 0", b"true"),
            (selector, b"&& state.next_preparation(&wanted, cache, camera).is_none()", b"&& true"),
            (selector, b"assert_eq!(a, b);\n            assert_eq!(ordinary.live, observed.live);",
             b"assert_eq!(a, a);\n            assert_eq!(ordinary.live, ordinary.live);"),
        )
        for relative, old, new in mutations:
            self.assertIn(relative, candidate.REPLAY_CONTRACT_PATHS)
            path = repo / relative
            original = path.read_bytes()
            self.assertEqual(original.count(old), 1, relative)
            path.write_bytes(original.replace(old, new))
            expected = self.commit_source_fixture(repo)
            with self.subTest(relative=relative, mutation=old), self.assertRaisesRegex(
                    ValueError, "reviewed replay canonical baseline changed") as failure:
                candidate.source_inputs(repo, expected)
            self.assertIn(relative, str(failure.exception))
            self.assertIn("canonical_sha256=" + candidate.digest(path), str(failure.exception))
            path.write_bytes(original)
            self.commit_source_fixture(repo)

    def test_exported_capture_readiness_sources_cannot_be_omitted_or_rehashed(self):
        evidence, source, report = self.successful_evidence_fixture()
        candidate.validate_evidence(evidence)
        for relative in ("crates/flightsim-app/src/main.rs",
                         "crates/flightsim-app/src/screen_capture.rs",
                         "crates/flightsim-render/src/terrain_selection.rs"):
            self.assertIn(relative, candidate.REPLAY_CONTRACT_PATHS)
            for mutation in ("missing file", "missing reviewed record", "changed canonical digest",
                             "changed checkout digests", "removed contract row"):
                changed = json.loads(json.dumps(source))
                if mutation == "missing file":
                    changed["files"] = [r for r in changed["files"] if r["path"] != relative]
                elif mutation == "missing reviewed record":
                    del changed["reviewed_replay_source_evidence"][relative]
                elif mutation == "changed canonical digest":
                    changed["reviewed_replay_source_evidence"][relative]["canonical_sha256"] = "0" * 64
                elif mutation == "changed checkout digests":
                    changed["reviewed_replay_source_evidence"][relative]["canonical_sha256"] = "0" * 64
                    changed["reviewed_replay_source_evidence"][relative]["checkout_sha256"] = "0" * 64
                    for record in changed["files"]:
                        if record["path"] == relative:
                            record["checkout_sha256"] = "0" * 64
                else:
                    del changed["replay_contract"]["source_sha256"][relative]
                    changed["replay_contract_text"] = json.dumps(changed["replay_contract"])
                    changed["replay_contract_sha256"] = candidate.hashlib.sha256(
                        changed["replay_contract_text"].encode()).hexdigest()
                    for record in changed["files"]:
                        if record["path"] == candidate.REPLAY_CONTRACT_PATH:
                            record["checkout_sha256"] = changed["replay_contract_sha256"]
                            record["checkout_bytes"] = len(changed["replay_contract_text"].encode())
                candidate.write_json(evidence / "source-inputs.json", changed)
                changed_report = {**report,
                                  "source_inputs_sha256": candidate.digest(evidence / "source-inputs.json"),
                                  "replay_contract_sha256": changed["replay_contract_sha256"]}
                self.seal_report(evidence, changed_report)
                with self.subTest(relative=relative, mutation=mutation), self.assertRaises(ValueError):
                    candidate.validate_evidence(evidence)

    def test_app_turboprop_admission_and_explicit_trim_cannot_drift(self):
        repo, _ = self.source_fixture()
        mutations = (
            ("crates/flightsim-app/src/distribution.rs",
             b'cfg!(feature = "commercial-staging") && profile.is_turboprop()',
             b'false && profile.is_turboprop()'),
            ("crates/flightsim-app/src/turboprop_session.rs",
             b') -> Result<FlightSession, String> {\n    crate::distribution::validate_profile_start(&startup.aircraft)?;',
             b') -> Result<FlightSession, String> {'),
            ("crates/flightsim-app/src/turboprop_session.rs",
             b') -> Result<Option<TurbopropReplayPlayer>, String> {\n    crate::distribution::validate_profile_start(&startup.aircraft)?;',
             b') -> Result<Option<TurbopropReplayPlayer>, String> {'),
            ("crates/flightsim-app/src/turboprop_session.rs",
             b') -> Result<(FlightSession, StartCondition), String> {\n    crate::distribution::validate_profile_start(&startup.aircraft)?;',
             b') -> Result<(FlightSession, StartCondition), String> {'),
            ("crates/flightsim-input/src/lib.rs", b'if self.value == 0.0 {', b'if false {'),
            ("crates/flightsim-input/src/lib.rs",
             b'pub const FINE_RATE: f64 = 0.002;', b'pub const FINE_RATE: f64 = 0.02;'),
            ("crates/flightsim-input/src/lib.rs",
             b'let lateral_trim_enabled = !keyboard.any_pressed(shortcut_modifiers);',
             b'let lateral_trim_enabled = keyboard.any_pressed(shortcut_modifiers);'),
        )
        candidate.source_inputs(repo, candidate.git(repo, "rev-parse", "HEAD"))
        for relative, old, new in mutations:
            path = repo / relative
            original = path.read_bytes()
            self.assertEqual(original.count(old), 1, relative)
            path.write_bytes(original.replace(old, new))
            expected = self.commit_source_fixture(repo)
            with self.subTest(relative=relative, mutation=old), self.assertRaisesRegex(
                    ValueError, "canonical baseline changed") as failure:
                candidate.source_inputs(repo, expected)
            self.assertIn(relative, str(failure.exception))
            self.assertIn("checkout_sha256=" + candidate.digest(path), str(failure.exception))
            path.write_bytes(original)
            self.commit_source_fixture(repo)

    def test_near_static_foundation_cannot_widen_law_identity_or_existing_app_dispatch(self):
        repo, _ = self.source_fixture()
        mutations = (
            ("crates/flightsim-fdm/src/turboprop/mod.rs", b'pub mod near_static;', b''),
            ("crates/flightsim-fdm/src/turboprop/near_static/propeller.rs",
             b'MAX_ADVERSE_INFLOW_RATIO: f64 = 0.10;', b'MAX_ADVERSE_INFLOW_RATIO: f64 = 0.11;'),
            ("crates/flightsim-fdm/src/turboprop/near_static/propeller.rs",
             b'MAX_TRANSVERSE_INFLOW_RATIO: f64 = 0.10;', b'MAX_TRANSVERSE_INFLOW_RATIO: f64 = 0.11;'),
            ("crates/flightsim-fdm/src/turboprop/near_static/propeller.rs",
             b'[-0.01, -0.005]', b'[-0.02, -0.005]'),
            ("crates/flightsim-sim/src/aircraft_profile/exact.rs",
             b'token.parse::<f64>()', b'token.trim_start_matches(\'-\').parse::<f64>()'),
            ("crates/flightsim-sim/src/aircraft_profile_v4/wire.rs",
             b'actual.get().to_bits() != expected.to_bits()', b'false'),
            ("crates/flightsim-sim/src/near_static_turboprop_identity.rs",
             b'&canonical_turboprop_bytes(config.forward_config())', b'&[]'),
            ("crates/flightsim-sim/src/near_static_turboprop_simulation/mod.rs",
             b'bytes.extend(physical);', b'bytes.extend([0_u8; 8]);'),
            ("crates/flightsim-sim/src/near_static_turboprop_simulation/mod.rs",
             b'let elapsed = self.committed.elapsed + NEAR_STATIC_TURBOPROP_FIXED_DT;',
             b'let elapsed = self.committed.elapsed;'),
            ("crates/flightsim-sim/src/replay_v6/mod.rs",
             b'.supported_near_static_turboprop()', b'.supported_turboprop()'),
            ("crates/flightsim-sim/src/replay_v6/mod.rs",
             b'report.origin.as_ref() == Some(&self.origin)', b'true'),
            ("crates/flightsim-sim/src/replay_v6/terminal.rs",
             b'R::OutsideNearStaticDomain(_) => mask == 0xfff,',
             b'R::OutsideNearStaticDomain(_) => mask == 0x1ff,'),
            ("crates/flightsim-app/src/aircraft_profile.rs",
             b'3 => AircraftProfileV3::from_bytes(bytes)',
             b'3 | 4 => AircraftProfileV3::from_bytes(bytes)'),
            ("crates/flightsim-sim/tests/cedar_near_static_qualification.rs",
             b'assert_eq!(total_parity_steps, 76_080);', b'assert!(total_parity_steps > 0);'),
        )
        candidate.source_inputs(repo, candidate.git(repo, "rev-parse", "HEAD"))
        for relative, old, new in mutations:
            path = repo / relative
            original = path.read_bytes()
            self.assertEqual(original.count(old), 1, relative)
            path.write_bytes(original.replace(old, new))
            expected = self.commit_source_fixture(repo)
            with self.subTest(relative=relative, mutation=old), self.assertRaisesRegex(
                    ValueError, "canonical baseline changed") as failure:
                candidate.source_inputs(repo, expected)
            self.assertIn(relative, str(failure.exception))
            self.assertIn("checkout_sha256=" + candidate.digest(path), str(failure.exception))
            path.write_bytes(original)
            self.commit_source_fixture(repo)

    def test_near_static_and_retained_independent_references_keep_original_bytes(self):
        scripts = (
            "subsonic_reference.py", "jet_identity_reference.py",
            "turboprop_profile_reference.py", "turboprop_identity_reference.py",
            "near_static_turboprop_identity_reference.py",
            "replay_v4_reference.py", "replay_v5_reference.py", "replay_v6_reference.py",
        )
        for script in scripts:
            with self.subTest(script=script):
                subprocess.run([candidate.sys.executable, str(ROOT / "docs/qa" / script)],
                               cwd=self.root, check=True, capture_output=True)

    def test_exported_near_static_sources_and_anchors_cannot_be_omitted_or_rehashed(self):
        evidence, source, report = self.successful_evidence_fixture()
        candidate.validate_evidence(evidence)
        paths = candidate.NEAR_STATIC_FOUNDATION_PATHS | set(candidate.NEAR_STATIC_INDEPENDENT_HASHES)
        for relative in sorted(paths):
            for mutation in ("missing file", "missing reviewed record", "changed canonical digest",
                             "removed contract row"):
                changed = json.loads(json.dumps(source))
                if mutation == "missing file":
                    changed["files"] = [r for r in changed["files"] if r["path"] != relative]
                elif mutation == "missing reviewed record":
                    del changed["reviewed_replay_source_evidence"][relative]
                elif mutation == "changed canonical digest":
                    changed["reviewed_replay_source_evidence"][relative]["canonical_sha256"] = "0" * 64
                elif relative in candidate.NEAR_STATIC_INDEPENDENT_HASHES:
                    del changed["independent_replay_sha256"][relative]
                else:
                    del changed["replay_contract"]["source_sha256"][relative]
                    changed["replay_contract_text"] = json.dumps(changed["replay_contract"])
                    changed["replay_contract_sha256"] = candidate.hashlib.sha256(
                        changed["replay_contract_text"].encode()).hexdigest()
                    for record in changed["files"]:
                        if record["path"] == candidate.REPLAY_CONTRACT_PATH:
                            record["checkout_sha256"] = changed["replay_contract_sha256"]
                            record["checkout_bytes"] = len(changed["replay_contract_text"].encode())
                candidate.write_json(evidence / "source-inputs.json", changed)
                changed_report = {**report,
                                  "source_inputs_sha256": candidate.digest(evidence / "source-inputs.json"),
                                  "replay_contract_sha256": changed["replay_contract_sha256"]}
                self.seal_report(evidence, changed_report)
                with self.subTest(relative=relative, mutation=mutation), self.assertRaises(ValueError):
                    candidate.validate_evidence(evidence)

    def test_help_reference_cannot_lose_replay_authority_or_scroll_bounds(self):
        repo, _ = self.source_fixture()
        mutations = (
            ("crates/flightsim-ui/src/pause.rs", b'let full = if replay {', b'let full = if false {'),
            ("crates/flightsim-ui/src/pause.rs",
             b'&& status.as_ref().is_none_or(|value| !value.is_changed())', b'&& true'),
            ("crates/flightsim-ui/src/pause.rs",
             b'if !paused.is_paused() {', b'if false {'),
            ("crates/flightsim-ui/src/pause.rs",
             b'(position.y + delta).clamp(0.0, max)', b'position.y + delta'),
            ("crates/flightsim-ui/src/tutorial.rs",
             b'&& replay.as_ref().is_none_or(|status| !status.active)', b'&& true'),
            ("crates/flightsim-app/src/turboprop_session.rs",
             b'Esc pause / complete controls', b'Esc pause'),
        )
        for relative, old, new in mutations:
            path = repo / relative
            original = path.read_bytes()
            self.assertEqual(original.count(old), 1, relative)
            path.write_bytes(original.replace(old, new))
            expected = self.commit_source_fixture(repo)
            with self.subTest(relative=relative, mutation=old), self.assertRaisesRegex(
                    ValueError, "canonical baseline changed") as failure:
                candidate.source_inputs(repo, expected)
            self.assertIn(relative, str(failure.exception))
            path.write_bytes(original)
            self.commit_source_fixture(repo)

    def test_exported_app_help_sources_cannot_be_omitted_or_rehashed_away(self):
        evidence, source, report = self.successful_evidence_fixture()
        paths = (
            "crates/flightsim-app/src/turboprop_session.rs",
            "crates/flightsim-app/src/turboprop_runtime_tests.rs",
            "crates/flightsim-app/src/turboprop_lifecycle_tests.rs",
            "crates/flightsim-input/src/lib.rs",
            "crates/flightsim-input/src/lateral_trim_regression.rs",
            "crates/flightsim-input/tests/fixtures/legacy-input-v1.json",
            "crates/flightsim-ui/src/pause.rs",
            "crates/flightsim-ui/src/attribution_layout_tests.rs",
        )
        for relative in paths:
            self.assertIn(relative, candidate.REPLAY_CONTRACT_PATHS)
            for mutation in ("missing file", "missing reviewed record", "changed canonical digest",
                             "removed contract row"):
                changed = json.loads(json.dumps(source))
                if mutation == "missing file":
                    changed["files"] = [r for r in changed["files"] if r["path"] != relative]
                elif mutation == "missing reviewed record":
                    del changed["reviewed_replay_source_evidence"][relative]
                elif mutation == "changed canonical digest":
                    changed["reviewed_replay_source_evidence"][relative]["canonical_sha256"] = "0" * 64
                else:
                    del changed["replay_contract"]["source_sha256"][relative]
                    changed["replay_contract_text"] = json.dumps(changed["replay_contract"])
                    changed["replay_contract_sha256"] = candidate.hashlib.sha256(
                        changed["replay_contract_text"].encode()).hexdigest()
                    for record in changed["files"]:
                        if record["path"] == candidate.REPLAY_CONTRACT_PATH:
                            record["checkout_sha256"] = changed["replay_contract_sha256"]
                            record["checkout_bytes"] = len(changed["replay_contract_text"].encode())
                candidate.write_json(evidence / "source-inputs.json", changed)
                changed_report = {**report,
                                  "source_inputs_sha256": candidate.digest(evidence / "source-inputs.json"),
                                  "replay_contract_sha256": changed["replay_contract_sha256"]}
                self.seal_report(evidence, changed_report)
                with self.subTest(relative=relative, mutation=mutation), self.assertRaises(ValueError):
                    candidate.validate_evidence(evidence)

    def test_responsive_map_rejects_scroll_modal_and_pointer_witness_drift(self):
        repo, _ = self.source_fixture()
        mutations = (
            ("crates/flightsim-ui/src/world_map.rs",
             b'    layout.update(&state);', b'    let _ = &mut layout;'),
            ("crates/flightsim-ui/src/world_map.rs",
             b'WorldMapBody, ScrollSurface::Map, ScrollPosition::default()',
             b'WorldMapBody, ScrollSurface::Credits, ScrollPosition::default()'),
            ("crates/flightsim-ui/src/world_map_layout.rs",
             b'size.x < 900.0 || size.y < 600.0', b'size.x < 640.0 || size.y < 480.0'),
            ("crates/flightsim-ui/src/world_map_layout.rs",
             b'AlignItems::Start', b'AlignItems::Center'),
            ("crates/flightsim-ui/src/world_map_layout.rs",
             b'* node.inverse_scale_factor()', b'* 1.0'),
            ("crates/flightsim-ui/src/world_map_layout.rs",
             b'let delta = if delta.is_finite() { delta } else { 0.0 };', b'let delta = delta;'),
            ("crates/flightsim-ui/src/world_map_layout.rs",
             b'state.visible.then_some(if state.weather_editor.is_some() {',
             b'true.then_some(if state.weather_editor.is_some() {'),
            ("crates/flightsim-ui/src/world_map_layout.rs",
             b'active != Some(*surface) && *surface != ScrollSurface::Map',
             b'active != Some(*surface)'),
            ("crates/flightsim-ui/src/world_map_layout.rs",
             b'if !opening {', b'if true {'),
            ("crates/flightsim-ui/src/world_map_layout.rs",
             b'(position.y + delta).clamp(0.0, max)', b'position.y + delta'),
            ("crates/flightsim-ui/src/regions.rs",
             b'ScrollSurface::Regions, ScrollPosition::default()',
             b'ScrollSurface::Map, ScrollPosition::default()'),
            ("crates/flightsim-ui/src/wind_settings.rs",
             b'super::ScrollSurface::Wind, ScrollPosition::default()',
             b'super::ScrollSurface::Map, ScrollPosition::default()'),
            ("crates/flightsim-ui/src/world_map_layout_tests.rs",
             b'assert!(!bounds.contains(point));', b'assert!(bounds.contains(point));'),
            ("crates/flightsim-ui/src/world_map_layout_tests.rs",
             b'assert_eq!(position, before);', b'assert!(position >= 0.0);'),
        )
        candidate.source_inputs(repo, candidate.git(repo, "rev-parse", "HEAD"))
        for relative, old, new in mutations:
            path = repo / relative
            original = path.read_bytes()
            # The pointer witness covers both clipped Start and clipped raster.
            expected_count = 2 if old == b'assert!(!bounds.contains(point));' else 1
            self.assertEqual(original.count(old), expected_count, relative)
            path.write_bytes(original.replace(old, new))
            expected = self.commit_source_fixture(repo)
            with self.subTest(relative=relative, mutation=old), self.assertRaisesRegex(
                    ValueError, "canonical baseline changed") as failure:
                candidate.source_inputs(repo, expected)
            self.assertIn(relative, str(failure.exception))
            self.assertIn("checkout_sha256=" + candidate.digest(path), str(failure.exception))
            path.write_bytes(original)
            self.commit_source_fixture(repo)

    def test_exported_responsive_map_sources_cannot_be_omitted_or_rehashed(self):
        evidence, source, report = self.successful_evidence_fixture()
        candidate.validate_evidence(evidence)
        paths = (
            "crates/flightsim-ui/src/world_map.rs",
            "crates/flightsim-ui/src/wind_settings.rs",
            "crates/flightsim-ui/src/regions.rs",
            "crates/flightsim-ui/src/world_map_layout.rs",
            "crates/flightsim-ui/src/world_map_layout_tests.rs",
        )
        for relative in paths:
            self.assertIn(relative, candidate.REPLAY_CONTRACT_PATHS)
            for mutation in ("missing file", "missing reviewed record", "changed canonical digest",
                             "removed contract row"):
                changed = json.loads(json.dumps(source))
                if mutation == "missing file":
                    changed["files"] = [r for r in changed["files"] if r["path"] != relative]
                elif mutation == "missing reviewed record":
                    del changed["reviewed_replay_source_evidence"][relative]
                elif mutation == "changed canonical digest":
                    changed["reviewed_replay_source_evidence"][relative]["canonical_sha256"] = "0" * 64
                else:
                    del changed["replay_contract"]["source_sha256"][relative]
                    changed["replay_contract_text"] = json.dumps(changed["replay_contract"])
                    changed["replay_contract_sha256"] = candidate.hashlib.sha256(
                        changed["replay_contract_text"].encode()).hexdigest()
                    for record in changed["files"]:
                        if record["path"] == candidate.REPLAY_CONTRACT_PATH:
                            record["checkout_sha256"] = changed["replay_contract_sha256"]
                            record["checkout_bytes"] = len(changed["replay_contract_text"].encode())
                candidate.write_json(evidence / "source-inputs.json", changed)
                changed_report = {**report,
                                  "source_inputs_sha256": candidate.digest(evidence / "source-inputs.json"),
                                  "replay_contract_sha256": changed["replay_contract_sha256"]}
                self.seal_report(evidence, changed_report)
                with self.subTest(relative=relative, mutation=mutation), self.assertRaises(ValueError):
                    candidate.validate_evidence(evidence)

    def test_authored_weather_rejects_exact_draft_atomicity_and_departure_drift(self):
        repo, _ = self.source_fixture()
        mutations = (
            ("crates/flightsim-ui/src/weather_settings.rs",
             b'visibility_dirty: false,', b'visibility_dirty: true,'),
            ("crates/flightsim-ui/src/weather_settings.rs",
             b'base_dirty: false,', b'base_dirty: true,'),
            ("crates/flightsim-ui/src/weather_settings.rs",
             b'value.is_finite() && (minimum..=maximum).contains(value)', b'true'),
            ("crates/flightsim-ui/src/weather_settings.rs",
             b'if !self.weather_settings.enabled || !self.new_flight_modal_ready() {',
             b'if !self.new_flight_modal_ready() {'),
            ("crates/flightsim-app/src/weather_draft.rs",
             b'self.background_visibility.is_some() || self.cloud_base.is_some()',
             b'self.background_visibility.is_some() && self.cloud_base.is_some()'),
            ("crates/flightsim-app/src/weather_draft.rs",
             b'!(10.0..=200_000.0).contains(&value.get())', b'false'),
            ("crates/flightsim-app/src/weather_draft.rs",
             b'!(0.0..=31_000.0 - thickness).contains(&value.get())', b'false'),
            ("crates/flightsim-app/src/weather_draft.rs",
             b'layer.base = reference.altitude + value;', b'layer.base = value;'),
            ("crates/flightsim-app/src/weather_draft.rs",
             b'layer.top = layer.base + (original.top - original.base);',
             b'layer.top = layer.base + Meters(1000.0);'),
            ("crates/flightsim-app/src/weather_draft.rs",
             b'&& left.seed == right.seed', b'&& true'),
            ("crates/flightsim-app/src/weather_draft.rs",
             b'&& cloud(left) == cloud(right)', b'&& true'),
            ("crates/flightsim-app/src/weather_draft.rs",
             b'&& optional_bits(self.cloud_base) == optional_bits(other.cloud_base)',
             b'&& self.cloud_base == other.cloud_base'),
            ("crates/flightsim-app/src/weather_runtime.rs",
             b'draft.preset() == preset && draft.template().seed == seed',
             b'draft.preset() == preset'),
            ("crates/flightsim-app/src/weather_runtime.rs",
             b'revision: pending.map_or(0, |p| p.revision),', b'revision: 0,'),
            ("crates/flightsim-app/src/weather_runtime.rs",
             b'let selectable = !replay && !lan && !startup.clouds_were_given;',
             b'let selectable = true;'),
            ("crates/flightsim-app/src/weather_runtime.rs",
             b'if selectable\n            && map.new_flight_modal_ready()',
             b'if selectable\n            && true'),
            ("crates/flightsim-app/src/weather_runtime.rs",
             b'if let Some(edit) = actions.weather.take() {\n        actions.invalidate_start();\n        pending.revision = pending.revision.wrapping_add(1);',
             b'if let Some(edit) = actions.weather.take() {\n        actions.invalidate_start();'),
            ("crates/flightsim-app/src/aircraft_picker_runtime.rs",
             b'&& weather_runtime::snapshot(world, current) == self.weather', b'&& true'),
            ("crates/flightsim-app/src/world_runtime.rs",
             b'super::weather_runtime::snapshot(world, startup).apply(&mut startup.weather);',
             b'let _ = super::weather_runtime::snapshot(world, startup);'),
            ("crates/flightsim-app/src/world_runtime.rs",
             b'super::weather_runtime::resolve_with_terrain(&mut startup, &mut terrain)?;',
             b'let _ = super::weather_runtime::resolve_with_terrain(&mut startup, &mut terrain);'),
            ("crates/flightsim-ui/src/world_map.rs",
             b'if keys.just_pressed(KeyCode::Escape) || cancel_weather || close_map {',
             b'if keys.just_pressed(KeyCode::Escape) || close_map {'),
            ("crates/flightsim-ui/src/world_map.rs",
             b'let mut weather_owned_frame = began_with_weather_editor;',
             b'let mut weather_owned_frame = false;'),
            ("crates/flightsim-ui/src/weather_settings.rs",
             b'super::ScrollSurface::Weather, ScrollPosition::default()',
             b'super::ScrollSurface::Map, ScrollPosition::default()'),
            ("crates/flightsim-app/src/weather_editor_tests.rs",
             b'assert_eq!(actual.draft, chosen.draft);', b'assert_eq!(actual.draft, actual.draft);'),
            ("crates/flightsim-ui/src/weather_settings_tests.rs",
             b'assert!(actions.generation > previous);', b'assert!(actions.generation >= previous);'),
            ("crates/flightsim-app/src/custom_weather_replay_tests.rs",
             b'assert_eq!(scalars(actual), scalars(expected));', b'assert_eq!(actual, expected);'),
        )
        candidate.source_inputs(repo, candidate.git(repo, "rev-parse", "HEAD"))
        for relative, old, new in mutations:
            path = repo / relative
            original = path.read_bytes()
            self.assertEqual(original.count(old), 1, relative)
            path.write_bytes(original.replace(old, new))
            expected = self.commit_source_fixture(repo)
            with self.subTest(relative=relative, mutation=old), self.assertRaisesRegex(
                    ValueError, "canonical baseline changed") as failure:
                candidate.source_inputs(repo, expected)
            self.assertIn(relative, str(failure.exception))
            self.assertIn("checkout_sha256=" + candidate.digest(path), str(failure.exception))
            path.write_bytes(original)
            self.commit_source_fixture(repo)

    def test_exported_authored_weather_sources_cannot_be_omitted_or_rehashed(self):
        evidence, source, report = self.successful_evidence_fixture()
        candidate.validate_evidence(evidence)
        for relative in sorted(candidate.AUTHORED_WEATHER_PATHS):
            self.assertIn(relative, candidate.REPLAY_CONTRACT_PATHS)
            for mutation in ("missing file", "missing reviewed record", "changed canonical digest",
                             "changed checkout digests", "removed contract row"):
                changed = json.loads(json.dumps(source))
                if mutation == "missing file":
                    changed["files"] = [r for r in changed["files"] if r["path"] != relative]
                elif mutation == "missing reviewed record":
                    del changed["reviewed_replay_source_evidence"][relative]
                elif mutation == "changed canonical digest":
                    changed["reviewed_replay_source_evidence"][relative]["canonical_sha256"] = "0" * 64
                elif mutation == "changed checkout digests":
                    changed["reviewed_replay_source_evidence"][relative]["canonical_sha256"] = "0" * 64
                    changed["reviewed_replay_source_evidence"][relative]["checkout_sha256"] = "0" * 64
                    for record in changed["files"]:
                        if record["path"] == relative:
                            record["checkout_sha256"] = "0" * 64
                else:
                    del changed["replay_contract"]["source_sha256"][relative]
                    changed["replay_contract_text"] = json.dumps(changed["replay_contract"])
                    changed["replay_contract_sha256"] = candidate.hashlib.sha256(
                        changed["replay_contract_text"].encode()).hexdigest()
                    for record in changed["files"]:
                        if record["path"] == candidate.REPLAY_CONTRACT_PATH:
                            record["checkout_sha256"] = changed["replay_contract_sha256"]
                            record["checkout_bytes"] = len(changed["replay_contract_text"].encode())
                candidate.write_json(evidence / "source-inputs.json", changed)
                changed_report = {**report,
                                  "source_inputs_sha256": candidate.digest(evidence / "source-inputs.json"),
                                  "replay_contract_sha256": changed["replay_contract_sha256"]}
                self.seal_report(evidence, changed_report)
                with self.subTest(relative=relative, mutation=mutation), self.assertRaises(ValueError):
                    candidate.validate_evidence(evidence)

    def test_near_static_app_rejects_admission_owner_clock_and_presentation_drift(self):
        repo, _ = self.source_fixture()
        # These committed semantic mutations exercise the source gate only.
        # No modified Rust is executed or credited as runtime qualification.
        mutations = (
            ("crates/flightsim-app/src/aircraft_profile.rs",
             b'4 => AircraftProfileV4::from_bytes(bytes)',
             b'4 => AircraftProfileV4::from_bytes(&[])'),
            ("crates/flightsim-app/src/distribution.rs",
             b'cfg!(feature = "commercial-staging") && profile.is_near_static_turboprop()',
             b'false && profile.is_near_static_turboprop()'),
            ("crates/flightsim-app/src/near_static_turboprop_session.rs",
             b') -> Result<FlightSession, String> {\n    crate::distribution::validate_profile_start(&startup.aircraft)?;',
             b') -> Result<FlightSession, String> {'),
            ("crates/flightsim-app/src/near_static_turboprop_session.rs",
             b') -> Result<Option<NearStaticTurbopropReplayPlayer>, String> {\n    crate::distribution::validate_profile_start(&startup.aircraft)?;',
             b') -> Result<Option<NearStaticTurbopropReplayPlayer>, String> {'),
            ("crates/flightsim-app/src/near_static_turboprop_session.rs",
             b') -> Result<(FlightSession, StartCondition), String> {\n    crate::distribution::validate_profile_start(&startup.aircraft)?;',
             b') -> Result<(FlightSession, StartCondition), String> {'),
            ("crates/flightsim-app/src/near_static_turboprop_session.rs",
             b'turbine_fraction: engine.turbine_fraction(),',
             b'turbine_fraction: flightsim_fdm::turboprop::TurbineFraction::new(1.0).unwrap(),'),
            ("crates/flightsim-app/src/near_static_turboprop_session.rs",
             b'NearStaticTurbopropRecording::read_from(&mut std::io::BufReader::new(file))',
             b'TurbopropRecording::read_from(&mut std::io::BufReader::new(file))'),
            ("crates/flightsim-app/src/near_static_turboprop_session.rs",
             b'if startup.weather.was_given || startup.weather.seed_was_given || startup.clouds_were_given {',
             b'if false {'),
            ("crates/flightsim-app/src/near_static_turboprop_session.rs",
             b'(startup.world.global_terrain || elevation != Meters::ZERO)', b'false'),
            ("crates/flightsim-app/src/near_static_turboprop_session.rs",
             b'candidate.weather.selection = environment.weather;',
             b'candidate.weather.selection = startup.weather.selection;'),
            ("crates/flightsim-app/src/flight_session.rs",
             b'crate::near_static_turboprop_session::prepare(startup, clock, start)',
             b'crate::turboprop_session::prepare(startup, clock, start)'),
            ("crates/flightsim-app/src/flight_session.rs",
             b'player: NearStaticTurbopropReplayPlayer,',
             b'player: TurbopropReplayPlayer,'),
            ("crates/flightsim-app/src/main.rs",
             b'near_static_turboprop_session::save_recording(&recorder.export())',
             b'near_static_turboprop_session::save_recording(&recorder.finish())'),
            ("crates/flightsim-app/src/replay_policy.rs",
             b'6 => "profile-v4 law-2 near-static turboprop aircraft",',
             b'6 => "profile-v3 law-1 turboprop aircraft",'),
            ("crates/flightsim-sim/src/near_static_turboprop_simulation/mod.rs",
             b'pub use presentation::NearStaticTurbopropPresentationSnapshot;', b''),
            ("crates/flightsim-sim/src/near_static_turboprop_simulation/presentation.rs",
             b'let environment = self.environment_at(self.state(), self.committed.ground, self.elapsed());',
             b'let environment = self.environment_at(self.state(), self.committed.ground, flightsim_core::Seconds::ZERO);'),
            ("crates/flightsim-sim/src/near_static_turboprop_simulation/presentation.rs",
             b'                    .is_ok()\n', b'                    .is_err()\n'),
            ("crates/flightsim-app/src/nearstatic_runtime_tests.rs",
             b'assert_eq!(record.conditions().identity.schema, 4);',
             b'assert!(record.conditions().identity.schema > 0);'),
            ("crates/flightsim-app/src/nearstatic_lifecycle_tests.rs",
             b'assert_eq!(record_bytes(session), self.record);',
             b'assert!(!record_bytes(session).is_empty());'),
        )
        candidate.source_inputs(repo, candidate.git(repo, "rev-parse", "HEAD"))
        for relative, old, new in mutations:
            path = repo / relative
            original = path.read_bytes()
            self.assertEqual(original.count(old), 1, relative)
            path.write_bytes(original.replace(old, new))
            expected = self.commit_source_fixture(repo)
            with self.subTest(relative=relative, mutation=old), self.assertRaisesRegex(
                    ValueError, "canonical baseline changed") as failure:
                candidate.source_inputs(repo, expected)
            self.assertIn(relative, str(failure.exception))
            self.assertIn("checkout_sha256=" + candidate.digest(path), str(failure.exception))
            path.write_bytes(original)
            self.commit_source_fixture(repo)

    def test_exported_near_static_app_sources_cannot_be_omitted_or_rehashed(self):
        evidence, source, report = self.successful_evidence_fixture()
        candidate.validate_evidence(evidence)
        self.assertEqual(len(candidate.NEAR_STATIC_APP_PATHS), 13)
        for relative in candidate.NEAR_STATIC_APP_PATHS:
            self.assertIn(relative, candidate.REPLAY_CONTRACT_PATHS)
            for mutation in ("missing file", "missing reviewed record", "changed canonical digest",
                             "removed contract row"):
                changed = json.loads(json.dumps(source))
                if mutation == "missing file":
                    changed["files"] = [r for r in changed["files"] if r["path"] != relative]
                elif mutation == "missing reviewed record":
                    del changed["reviewed_replay_source_evidence"][relative]
                elif mutation == "changed canonical digest":
                    changed["reviewed_replay_source_evidence"][relative]["canonical_sha256"] = "0" * 64
                else:
                    del changed["replay_contract"]["source_sha256"][relative]
                    changed["replay_contract_text"] = json.dumps(changed["replay_contract"])
                    changed["replay_contract_sha256"] = candidate.hashlib.sha256(
                        changed["replay_contract_text"].encode()).hexdigest()
                    for record in changed["files"]:
                        if record["path"] == candidate.REPLAY_CONTRACT_PATH:
                            record["checkout_sha256"] = changed["replay_contract_sha256"]
                            record["checkout_bytes"] = len(changed["replay_contract_text"].encode())
                candidate.write_json(evidence / "source-inputs.json", changed)
                changed_report = {**report,
                                  "source_inputs_sha256": candidate.digest(evidence / "source-inputs.json"),
                                  "replay_contract_sha256": changed["replay_contract_sha256"]}
                self.seal_report(evidence, changed_report)
                with self.subTest(relative=relative, mutation=mutation), self.assertRaises(ValueError):
                    candidate.validate_evidence(evidence)

    def test_replay_trim_source_rejects_fabricated_values_and_weakened_witnesses(self):
        repo, _ = self.source_fixture()
        # Mutations are committed only to disposable source-gate fixtures. They
        # are never compiled, executed or credited as behavioral qualification.
        mutations = (
            ("crates/flightsim-app/src/main.rs",
             b'let playback_active = playback.is_some() || simulation.0.is_replay();',
             b'let playback_active = playback.is_some();'),
            ("crates/flightsim-app/src/main.rs",
             b'trim: (!playback_active).then(|| controls.trim.value()),',
             b'trim: Some(controls.trim.value()),'),
            ("crates/flightsim-app/src/main.rs",
             b'trim: (!playback_active).then(|| controls.trim.value()),',
             b'trim: (!playback_active).then_some(0.0),'),
            ("crates/flightsim-ui/src/lib.rs",
             b'|| "  N/A".to_owned(),', b'|| " 0.00".to_owned(),'),
            ("crates/flightsim-ui/src/lib.rs",
             b'let hint = if trim > 0.02 {', b'let hint = if false {'),
            ("crates/flightsim-app/src/controls_runtime_tests.rs",
             b'(!replay).then_some(-0.12)', b'Some(-0.12)'),
            ("crates/flightsim-app/src/jet_runtime_tests.rs",
             b'assert_eq!(hud.trim, None);', b'assert_eq!(hud.trim, Some(0.0));'),
            ("crates/flightsim-app/src/turboprop_lifecycle_tests.rs",
             b'assert_eq!(hud.trim, None);', b'assert_eq!(hud.trim, Some(0.0));'),
            ("crates/flightsim-app/src/nearstatic_runtime_tests.rs",
             b'assert_eq!(hud.trim, None);', b'assert_eq!(hud.trim, Some(0.0));'),
        )
        candidate.source_inputs(repo, candidate.git(repo, "rev-parse", "HEAD"))
        for relative, old, new in mutations:
            path = repo / relative
            original = path.read_bytes()
            self.assertEqual(original.count(old), 1, relative)
            path.write_bytes(original.replace(old, new))
            expected = self.commit_source_fixture(repo)
            with self.subTest(relative=relative, mutation=old), self.assertRaisesRegex(
                    ValueError, "canonical baseline changed") as failure:
                candidate.source_inputs(repo, expected)
            self.assertIn(relative, str(failure.exception))
            self.assertIn("checkout_sha256=" + candidate.digest(path), str(failure.exception))
            path.write_bytes(original)
            self.commit_source_fixture(repo)

    def test_aircraft_package_review_rejects_admission_and_shared_policy_drift(self):
        repo, _ = self.source_fixture()
        # These committed mutations probe the source gate, not an executed app.
        # Ordinary package validation cannot silently admit commercial imports,
        # substitute a builtin profile, weaken content bounds or change axes/fit.
        mutations = (
            ("crates/flightsim-app/src/main.rs",
             b'if let Some(result) = aircraft_package_cli::run_cli(&arguments) {',
             b'if let Some(result) = None::<Result<(), String>> {'),
            ("crates/flightsim-app/src/aircraft_package_cli.rs",
             b'if cfg!(feature = "commercial-staging") {', b'if false {'),
            ("crates/flightsim-app/src/aircraft_package_cli.rs",
             b'SelectedAircraftProfile::from_bytes(bytes).map_err(Error::Invalid)?',
             b'SelectedAircraftProfile::builtin("swift-sport").map_err(Error::Invalid)?'),
            ("crates/flightsim-app/src/aircraft_package_cli.rs",
             b'manifest.model.path.strip_prefix("assets/") != Some(profile.model_path())',
             b'false'),
            ("crates/flightsim-content/src/aircraft/mod.rs",
             b'validate(&self.profile_bytes, &self.manifest, &self.geometry)?;',
             b'let _ = validate;'),
            ("crates/flightsim-content/src/aircraft/mod.rs",
             b'sha256(&bytes) != file.sha256', b'false'),
            ("crates/flightsim-content/src/archive.rs",
             b'PortablePaths::with_limit(limits.entries)', b'PortablePaths::default()'),
            ("crates/flightsim-content/src/archive.rs",
             b'entries: MAX_ARCHIVE_ENTRIES,', b'entries: usize::MAX,'),
            ("crates/flightsim-content/src/install.rs",
             b'Ok(_) => return Err(Error::AlreadyInstalled(target)),', b'Ok(_) => {},'),
            ("crates/flightsim-content/src/aircraft/glb.rs",
             b'|| doc.scenes[0].nodes.len() > MAX_NODES', b'|| false'),
            ("crates/flightsim-render/src/model.rs",
             b'target / along_forward', b'1.0'),
            ("crates/flightsim-app/src/aircraft_package_cli.rs",
             b'assert!(error.to_string().contains("commercial-staging"));',
             b'assert!(error.to_string().contains("package"));'),
        )
        candidate.source_inputs(repo, candidate.git(repo, "rev-parse", "HEAD"))
        for relative, old, new in mutations:
            path = repo / relative
            original = path.read_bytes()
            self.assertEqual(original.count(old), 1, relative)
            path.write_bytes(original.replace(old, new))
            expected = self.commit_source_fixture(repo)
            with self.subTest(relative=relative, mutation=old), self.assertRaisesRegex(
                    ValueError, "canonical baseline changed") as failure:
                candidate.source_inputs(repo, expected)
            self.assertIn(relative, str(failure.exception))
            path.write_bytes(original)
            self.commit_source_fixture(repo)

    def test_exported_aircraft_package_sources_cannot_be_omitted_or_rehashed(self):
        evidence, source, report = self.successful_evidence_fixture()
        candidate.validate_evidence(evidence)
        self.assertEqual(len(candidate.AIRCRAFT_PACKAGE_PATHS), 19)
        for relative in sorted(candidate.AIRCRAFT_PACKAGE_PATHS):
            self.assertIn(relative, candidate.REPLAY_CONTRACT_PATHS)
            for mutation in ("missing file", "missing reviewed record", "changed digests",
                             "removed contract row"):
                changed = json.loads(json.dumps(source))
                if mutation == "missing file":
                    changed["files"] = [r for r in changed["files"] if r["path"] != relative]
                elif mutation == "missing reviewed record":
                    del changed["reviewed_replay_source_evidence"][relative]
                elif mutation == "changed digests":
                    changed["reviewed_replay_source_evidence"][relative]["canonical_sha256"] = "0" * 64
                    changed["reviewed_replay_source_evidence"][relative]["checkout_sha256"] = "0" * 64
                    for record in changed["files"]:
                        if record["path"] == relative:
                            record["checkout_sha256"] = "0" * 64
                else:
                    del changed["replay_contract"]["source_sha256"][relative]
                    changed["replay_contract_text"] = json.dumps(changed["replay_contract"])
                    changed["replay_contract_sha256"] = candidate.hashlib.sha256(
                        changed["replay_contract_text"].encode()).hexdigest()
                    for record in changed["files"]:
                        if record["path"] == candidate.REPLAY_CONTRACT_PATH:
                            record["checkout_sha256"] = changed["replay_contract_sha256"]
                            record["checkout_bytes"] = len(changed["replay_contract_text"].encode())
                candidate.write_json(evidence / "source-inputs.json", changed)
                changed_report = {**report,
                                  "source_inputs_sha256": candidate.digest(evidence / "source-inputs.json"),
                                  "replay_contract_sha256": changed["replay_contract_sha256"]}
                self.seal_report(evidence, changed_report)
                with self.subTest(relative=relative, mutation=mutation), self.assertRaises(ValueError):
                    candidate.validate_evidence(evidence)

    def test_regional_coverage_review_rejects_budget_radius_source_and_physics_drift(self):
        repo, _ = self.source_fixture()
        # These are committed bad-source fixtures. They are never compiled or
        # presented as runtime/physical/native qualification.
        mutations = (
            ("crates/flightsim-world/src/coverage.rs",
             b"pub const MAX_COVERAGE_TILES: usize = 16_384;",
             b"pub const MAX_COVERAGE_TILES: usize = usize::MAX;"),
            ("crates/flightsim-world/src/coverage.rs",
             b"pub const MAX_COVERAGE_DIRECTORY_ENTRIES: usize = 32_768;",
             b"pub const MAX_COVERAGE_DIRECTORY_ENTRIES: usize = usize::MAX;"),
            ("crates/flightsim-world/src/coverage.rs",
             b"if entries > entry_limit {", b"if false {"),
            ("crates/flightsim-world/src/lod.rs",
             b"if id.level >= self.max_level {", b"if false {"),
            ("crates/flightsim-world/src/lod.rs",
             b"<= self.primary_coverage_radius.get()", b"<= f64::INFINITY"),
            ("crates/flightsim-world/src/lod.rs",
             b"if leaves.len().saturating_add(3) > self.max_tiles {", b"if false {"),
            ("crates/flightsim-world/src/global.rs",
             b"self.primary.primary_coverage()", b"None"),
            ("crates/flightsim-world/src/terrain.rs",
             b"for level in self.levels.clone().rev() {", b"for level in self.levels.clone() {"),
            ("crates/flightsim-world/src/tile.rs",
             b"pub const MAX_LEVEL: u8 = 24;", b"pub const MAX_LEVEL: u8 = 31;"),
            ("crates/flightsim-core/src/geodetic.rs",
             b".distance(self.0.clamp(minimum - padding, maximum + padding))",
             b".distance(self.0)"),
            ("crates/flightsim-world/src/draw_distance.rs",
             b".with_primary_coverage_radius(self.terrain_detail_radius)",
             b".with_primary_coverage_radius(Meters(400_000.0))"),
            ("crates/flightsim-world/src/draw_distance_tests.rs",
             b"assert_eq!(standard.terrain_detail_radius(), Meters(5500.0));",
             b"assert_eq!(standard.terrain_detail_radius(), Meters(400_000.0));"),
            ("crates/flightsim-content/src/install.rs",
             b"Some(&self.coverage)", b"None"),
            ("crates/flightsim-content/src/install.rs",
             b"Err(Error::ReplayUnsupported)", b"Ok(())"),
            ("crates/flightsim-render/src/terrain_selection.rs",
             b"while update.load_attempts < frame_budget {",
             b"while update.load_attempts < usize::MAX {"),
            ("crates/flightsim-render/src/terrain_selection.rs",
             b"while update.prepared.len() < frame_budget {",
             b"while update.prepared.len() < usize::MAX {"),
            ("crates/flightsim-app/src/main.rs",
             b"source: make_render_source(&startup),", b"source: make_source(&startup),"),
        )
        candidate.source_inputs(repo, candidate.git(repo, "rev-parse", "HEAD"))
        for relative, old, new in mutations:
            path = repo / relative
            original = path.read_bytes()
            self.assertEqual(original.count(old), 1, relative)
            path.write_bytes(original.replace(old, new))
            expected = self.commit_source_fixture(repo)
            with self.subTest(relative=relative, mutation=old), self.assertRaisesRegex(
                    ValueError, "canonical baseline changed") as failure:
                candidate.source_inputs(repo, expected)
            self.assertIn(relative, str(failure.exception))
            path.write_bytes(original)
            self.commit_source_fixture(repo)

    def test_exported_regional_coverage_sources_cannot_be_omitted_or_rehashed(self):
        evidence, source, report = self.successful_evidence_fixture()
        candidate.validate_evidence(evidence)
        self.assertEqual(len(candidate.REGIONAL_COVERAGE_PATHS), 11)
        for relative in sorted(candidate.REGIONAL_COVERAGE_PATHS):
            self.assertIn(relative, candidate.REPLAY_CONTRACT_PATHS)
            for mutation in ("missing file", "missing reviewed record", "changed digests",
                             "removed contract row"):
                changed = json.loads(json.dumps(source))
                if mutation == "missing file":
                    changed["files"] = [r for r in changed["files"] if r["path"] != relative]
                elif mutation == "missing reviewed record":
                    del changed["reviewed_replay_source_evidence"][relative]
                elif mutation == "changed digests":
                    changed["reviewed_replay_source_evidence"][relative]["canonical_sha256"] = "0" * 64
                    changed["reviewed_replay_source_evidence"][relative]["checkout_sha256"] = "0" * 64
                    for record in changed["files"]:
                        if record["path"] == relative:
                            record["checkout_sha256"] = "0" * 64
                else:
                    del changed["replay_contract"]["source_sha256"][relative]
                    changed["replay_contract_text"] = json.dumps(changed["replay_contract"])
                    changed["replay_contract_sha256"] = candidate.hashlib.sha256(
                        changed["replay_contract_text"].encode()).hexdigest()
                    for record in changed["files"]:
                        if record["path"] == candidate.REPLAY_CONTRACT_PATH:
                            record["checkout_sha256"] = changed["replay_contract_sha256"]
                            record["checkout_bytes"] = len(changed["replay_contract_text"].encode())
                candidate.write_json(evidence / "source-inputs.json", changed)
                changed_report = {**report,
                                  "source_inputs_sha256": candidate.digest(evidence / "source-inputs.json"),
                                  "replay_contract_sha256": changed["replay_contract_sha256"]}
                self.seal_report(evidence, changed_report)
                with self.subTest(relative=relative, mutation=mutation), self.assertRaises(ValueError):
                    candidate.validate_evidence(evidence)

    def test_capture_backpressure_keeps_independent_lifecycle_and_engine_anchors(self):
        # Literal review anchors supplement whole-file pins. This checks source
        # semantics and dependency identity, never native/GPU execution.
        import tomllib
        relative = "crates/flightsim-app/src/capture_backpressure.rs"
        production = (ROOT / relative).read_text().split("#[cfg(test)]", 1)[0]
        for anchor in (
            "const MAX_IN_FLIGHT_FRAMES: usize = 2;",
            "const SERVICE_WAIT: Duration = Duration::from_millis(100);",
            "self.0.store(false, Ordering::Release);",
            ".is_some_and(|complete| complete.load(Ordering::Acquire))",
            "if self.pending.len() < MAX_IN_FLIGHT_FRAMES {",
            "service()?;",
            "let complete = Arc::new(AtomicBool::new(false));",
            "self.pending.push_back(complete.clone());",
            "submission_index: None,",
            "timeout: Some(SERVICE_WAIT),",
            "Ok(_) | Err(PollError::Timeout) => Ok(()),",
            "Err(error) => Err(error),",
            '.unwrap_or_else(|error| panic!("screenshot GPU backpressure failed: {error}"));',
            ".on_submitted_work_done(move || complete.store(true, Ordering::Release));",
        ):
            self.assertEqual(production.count(anchor), 1, anchor)
        configure = production.split("pub(super) fn configure", 1)[1].split("fn cancel_removed_request", 1)[0]
        self.assertLess(configure.index(".is_none()"), configure.index("let session = CaptureSession::default();"))
        self.assertIn("return;", configure[:configure.index("let session = CaptureSession::default();")])
        self.assertIn("render_app.update_schedule = Some(CaptureRender.intern());", configure)
        self.assertIn(".add_systems(Last, cancel_removed_request);", configure)
        wrapper = production.split("fn render_capture_frame", 1)[1]
        order = [wrapper.index(value) for value in (
            ".wait_for_credit(&session, || {", "world.run_schedule(Render);",
            "if session.active() {", "let complete = credits.submitted();",
            ".on_submitted_work_done(move || complete.store(true, Ordering::Release));",
        )]
        self.assertEqual(order, sorted(order))
        self.assertEqual(production.count("world.run_schedule(Render);"), 1)
        for forbidden in (".set_extract(", ".submit(", ".run_if("):
            self.assertNotIn(forbidden, production)
        main = (ROOT / "crates/flightsim-app/src/main.rs").read_text()
        self.assertIn('#[cfg(not(target_family = "wasm"))]\nmod capture_backpressure;', main)
        capture = (ROOT / "crates/flightsim-app/src/screen_capture.rs").read_text().split("#[cfg(test)]", 1)[0]
        self.assertIn('    #[cfg(not(target_family = "wasm"))]\n    crate::capture_backpressure::configure(app);', capture)
        self.assertIn("state.elapsed < startup.screenshot_delay || state.frames < 30 || !stable", capture)
        self.assertIn("let stable = state.observe(ready);", capture)
        saver = capture.split("fn save_capture(", 1)[1].split("fn save_capture_image", 1)[0]
        self.assertLess(saver.index("session.finish();"), saver.index("let Some(path) = startup.screenshot.as_ref()"))
        self.assertLess(saver.index("session.finish();"), saver.index("let result = save_capture_image"))
        self.assertIn("finish_batch_capture(startup.exit_after_screenshot, &result);", saver)
        manifest = tomllib.loads((ROOT / "crates/flightsim-app/Cargo.toml").read_text())
        self.assertEqual(manifest["dependencies"]["wgpu-types"],
                         {"version": "=27.0.1", "default-features": False})
        workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())
        self.assertEqual(workspace["workspace"]["dependencies"]["bevy"]["version"], "0.18.1")
        locked = tomllib.loads((ROOT / "Cargo.lock").read_text())["package"]
        # These exact upstream packages supplied the independently reviewed
        # extraction/Render/poll/callback lifecycle; no engine patch was used.
        for name, version, checksum in (
            ("bevy_render", "0.18.1", "243523e33fe5dfcebc4240b1eb2fc16e855c5d4c0ea6a8393910740956770f44"),
            ("bevy_app", "0.18.1", "def9f41aa5bf9b9dec8beda307a332798609cffb9d44f71005e0cfb45164f2f6"),
            ("wgpu", "27.0.1", "bfe68bac7cde125de7a731c3400723cadaaf1703795ad3f4805f187459cd7a77"),
            ("wgpu-core", "27.0.3", "27a75de515543b1897b26119f93731b385a19aea165a1ec5f0e3acecc229cae7"),
            ("wgpu-types", "27.0.1", "afdcf84c395990db737f2dd91628706cb31e86d72e53482320d368e52b5da5eb"),
        ):
            matches = [entry for entry in locked if entry["name"] == name and entry["version"] == version]
            self.assertEqual(len(matches), 1, name)
            self.assertEqual(matches[0]["checksum"], checksum, name)

    def test_capture_backpressure_rejects_credit_lifecycle_error_and_scope_drift(self):
        repo, _ = self.source_fixture()
        helper = "crates/flightsim-app/src/capture_backpressure.rs"
        capture = "crates/flightsim-app/src/screen_capture.rs"
        # Each altered complete file is committed, then rejected before a build.
        # These fixtures do not run altered Rust or claim renderer acceptance.
        mutations = (
            (helper, b"const MAX_IN_FLIGHT_FRAMES: usize = 2;",
             b"const MAX_IN_FLIGHT_FRAMES: usize = usize::MAX;"),
            (helper, b"Duration::from_millis(100)", b"Duration::from_millis(60_000)"),
            (helper, b"self.0.store(false, Ordering::Release);", b"self.0.store(true, Ordering::Release);"),
            (helper, b".is_some_and(|complete| complete.load(Ordering::Acquire))", b".is_some_and(|_| true)"),
            (helper, b"if !session.active() {", b"if false {"),
            (helper, b"self.pending.clear();", b"// retain cancelled credit debt"),
            (helper, b"if self.pending.len() < MAX_IN_FLIGHT_FRAMES {", b"if true {"),
            (helper, b"service()?;", b"let _ = service();"),
            (helper, b"service()?;", b"service()?; self.pending.pop_front();"),
            (helper, b"let complete = Arc::new(AtomicBool::new(false));", b"let complete = Arc::new(AtomicBool::new(true));"),
            (helper, b"self.pending.push_back(complete.clone());",
             b"self.pending.push_back(Arc::new(AtomicBool::new(true)));"),
            (helper, b".screenshot\n        .is_none()", b".screenshot\n        .is_some()"),
            (helper, b"render_app.update_schedule = Some(CaptureRender.intern());",
             b"render_app.update_schedule = Some(Render.intern());"),
            (helper, b".wait_for_credit(&session, || {\n                match device.poll(PollType::Wait {",
             b".wait_for_credit(&CaptureSession::default(), || {\n                match device.poll(PollType::Wait {"),
            (helper, b"timeout: Some(SERVICE_WAIT),", b"timeout: None,"),
            (helper, b"Ok(_) | Err(PollError::Timeout) => Ok(()),", b"Ok(_) | Err(_) => Ok(()),"),
            (helper, b"Err(error) => Err(error),", b"Err(_) => Ok(()),"),
            (helper, b'.unwrap_or_else(|error| panic!("screenshot GPU backpressure failed: {error}"));',
             b".unwrap_or_else(|_| ());"),
            (helper, b"world.run_schedule(Render);", b"// omit the original Render lifecycle"),
            (helper, b"if session.active() {", b"if true {"),
            (helper, b".on_submitted_work_done(move || complete.store(true, Ordering::Release));",
             b".on_submitted_work_done(move || drop(complete));"),
            (helper, b"assert_eq!(render_app.update_schedule, Some(Render.intern()));",
             b"assert_eq!(render_app.update_schedule, render_app.update_schedule);"),
            ("crates/flightsim-app/src/main.rs",
             b'#[cfg(not(target_family = "wasm"))]\nmod capture_backpressure;', b"mod capture_backpressure;"),
            (capture, b"crate::capture_backpressure::configure(app);", b"let _ = app;"),
            (capture, b"if let Some(session) = session {\n        session.finish();\n    }",
             b"if let Some(session) = session {\n        let _ = session;\n    }"),
            (capture, b"state.elapsed < startup.screenshot_delay || state.frames < 30 || !stable",
             b"state.elapsed < startup.screenshot_delay || state.frames < 1 || !stable"),
            (capture, b"let stable = state.observe(ready);", b"let stable = true;"),
            ("crates/flightsim-app/Cargo.toml",
             b'wgpu-types = { version = "=27.0.1", default-features = false }',
             b'wgpu-types = { version = "=27.0.1", default-features = true }'),
            ("Cargo.toml", b'bevy = { version = "0.18.1",', b'bevy = { version = "0.19.0",'),
            ("Cargo.lock", b'afdcf84c395990db737f2dd91628706cb31e86d72e53482320d368e52b5da5eb', b'0' * 64),
        )
        candidate.source_inputs(repo, candidate.git(repo, "rev-parse", "HEAD"))
        for relative, old, new in mutations:
            self.assertIn(relative, candidate.REPLAY_CONTRACT_PATHS)
            path = repo / relative
            original = path.read_bytes()
            self.assertEqual(original.count(old), 1, relative)
            path.write_bytes(original.replace(old, new))
            expected = self.commit_source_fixture(repo)
            with self.subTest(relative=relative, mutation=old), self.assertRaisesRegex(
                    ValueError, "canonical baseline changed") as failure:
                candidate.source_inputs(repo, expected)
            self.assertIn(relative, str(failure.exception))
            path.write_bytes(original)
            self.commit_source_fixture(repo)

    def test_exported_capture_backpressure_sources_cannot_be_omitted_or_rehashed(self):
        evidence, source, report = self.successful_evidence_fixture()
        candidate.validate_evidence(evidence)
        paths = candidate.CAPTURE_BACKPRESSURE_PATHS | {
            "crates/flightsim-app/src/main.rs", "crates/flightsim-app/src/screen_capture.rs",
        }
        self.assertEqual(len(paths), 6)
        for relative in sorted(paths):
            self.assertIn(relative, candidate.REPLAY_CONTRACT_PATHS)
            for mutation in ("missing file", "missing reviewed record", "changed canonical digest",
                             "changed checkout digests", "removed contract row"):
                changed = json.loads(json.dumps(source))
                if mutation == "missing file":
                    changed["files"] = [r for r in changed["files"] if r["path"] != relative]
                elif mutation == "missing reviewed record":
                    del changed["reviewed_replay_source_evidence"][relative]
                elif mutation == "changed canonical digest":
                    changed["reviewed_replay_source_evidence"][relative]["canonical_sha256"] = "0" * 64
                elif mutation == "changed checkout digests":
                    changed["reviewed_replay_source_evidence"][relative]["canonical_sha256"] = "0" * 64
                    changed["reviewed_replay_source_evidence"][relative]["checkout_sha256"] = "0" * 64
                    for record in changed["files"]:
                        if record["path"] == relative:
                            record["checkout_sha256"] = "0" * 64
                else:
                    del changed["replay_contract"]["source_sha256"][relative]
                    changed["replay_contract_text"] = json.dumps(changed["replay_contract"])
                    changed["replay_contract_sha256"] = candidate.hashlib.sha256(
                        changed["replay_contract_text"].encode()).hexdigest()
                    for record in changed["files"]:
                        if record["path"] == candidate.REPLAY_CONTRACT_PATH:
                            record["checkout_sha256"] = changed["replay_contract_sha256"]
                            record["checkout_bytes"] = len(changed["replay_contract_text"].encode())
                candidate.write_json(evidence / "source-inputs.json", changed)
                changed_report = {**report,
                                  "source_inputs_sha256": candidate.digest(evidence / "source-inputs.json"),
                                  "replay_contract_sha256": changed["replay_contract_sha256"]}
                self.seal_report(evidence, changed_report)
                with self.subTest(relative=relative, mutation=mutation), self.assertRaises(ValueError):
                    candidate.validate_evidence(evidence)

    def test_capture_admission_has_complete_scope_and_prior_render_witnesses(self):
        paths = candidate.CAPTURE_ADMISSION_PATHS
        self.assertEqual(paths, {
            'crates/flightsim-app/src/capture_admission.rs',
            'crates/flightsim-app/src/capture_admission_state.rs',
        })
        self.assertTrue(paths <= candidate.REPLAY_CONTRACT_PATHS)
        for relative, old, _new in CAPTURE_ADMISSION_MUTATIONS:
            self.assertIn(relative, candidate.REPLAY_CONTRACT_PATHS)
            self.assertEqual((ROOT / relative).read_bytes().count(old), 1, (relative, old))
        helper = (ROOT / 'crates/flightsim-app/src/capture_admission.rs').read_text()
        production = helper.rsplit('\n#[cfg(test)]\nmod tests {', 1)[0]
        self.assertIn('#[path = "capture_admission_state.rs"]\nmod state;', production)
        self.assertIn('configure_frame_hooks(render_app, render_system.into_system_set().intern());', production)
        self.assertIn('views: Query<(&ExtractedCamera, &ViewTarget), With<Camera3d>>', production)
        self.assertIn('rendered_epoch: admission.rendered_epoch.clone(),', production)
        self.assertIn('frame.opportunity = Some((epoch, admission.rendered_epoch.clone()));', production)
        for forbidden in ('.set_extract(', '.submit(', '.run_if(', '.update_schedule =',
                          'is_active =', 'MAX_IN_FLIGHT_FRAMES', 'on_submitted_work_done'):
            self.assertNotIn(forbidden, production)
        configure = production.split('pub(super) fn configure', 1)[1].split('fn configure_frame_hooks', 1)[0]
        self.assertLess(configure.index('startup.screenshot.is_none() || !startup.exit_after_screenshot'),
                        configure.index('app.get_sub_app_mut(RenderApp)'))
        self.assertIn('.add_systems(Last, cancel_removed_request);', configure)
        restoration = production.split('fn restore_views(', 1)[1].split('pub(super) fn with_restored_views', 1)[0]
        self.assertIn('if let Some(saved) = frame.saved.take()', restoration)
        self.assertNotIn('snapshot.', restoration)
        self.assertNotIn('active()', restoration)
        wrapper = (ROOT / 'crates/flightsim-app/src/capture_backpressure.rs').read_text().split('fn render_capture_frame', 1)[1].split('#[cfg(test)]', 1)[0]
        order = [wrapper.index(anchor) for anchor in (
            '.wait_for_credit(&session, || {',
            'crate::capture_admission::with_restored_views(world, |world| {',
            'world.run_schedule(Render);',
            'crate::capture_admission::acknowledge_render(world);',
            'if session.active() {',
            'let complete = credits.submitted();',
            '.on_submitted_work_done(move || complete.store(true, Ordering::Release));',
        )]
        self.assertEqual(order, sorted(order))
        self.assertEqual(wrapper.count('world.run_schedule(Render);'), 1)
        capture = (ROOT / 'crates/flightsim-app/src/screen_capture.rs').read_text().split('\n#[cfg(test)]', 1)[0]
        self.assertIn('let stable = ready.is_some() && self.previous_ready == ready;', capture)
        self.assertIn('state.elapsed < startup.screenshot_delay || state.frames < 30 || !stable', capture)
        self.assertLess(capture.index('if !admission.observe(state.scene_changed, eligible)'),
                        capture.index('state.done = true;'))
        self.assertLess(capture.index('admission.requested();'), capture.index('let screenshot = target.map_or_else'))
        saver = capture.split('fn save_capture(', 1)[1].split('fn save_capture_image', 1)[0]
        self.assertLess(saver.index('admission.finish();'), saver.index('let Some(path)'))
        for witness in (
            'loading_keeps_preparation_and_cleanup_and_restores_the_exact_view_list',
            'an_extracted_screenshot_never_loses_its_view_draws',
            'restoration_does_not_depend_on_a_later_snapshot_or_empty_saved_list',
            'admission_with_no_valid_flight_view_does_not_acknowledge_a_render',
            'acknowledgement_uses_the_admitted_frame_token_not_later_main_state',
            'a_failed_render_restores_views_invalidates_ack_and_still_fails',
            'batch_configuration_extracts_a_deferred_snapshot_and_render_entity_mapping',
            'ordinary_and_non_batch_launches_install_no_admission_systems',
        ):
            self.assertEqual(helper.count('fn ' + witness + '('), 1, witness)

    def test_capture_admission_rejects_scope_epoch_lifecycle_and_restore_mutations(self):
        repo, _ = self.source_fixture()
        candidate.source_inputs(repo, candidate.git(repo, 'rev-parse', 'HEAD'))
        self.assertEqual(len(CAPTURE_ADMISSION_MUTATIONS), 50)
        for relative, old, new in CAPTURE_ADMISSION_MUTATIONS:
            self.assertIn(relative, candidate.REPLAY_CONTRACT_PATHS)
            path = repo / relative
            original = path.read_bytes()
            self.assertEqual(original.count(old), 1, (relative, old))
            path.write_bytes(original.replace(old, new))
            expected = self.commit_source_fixture(repo)
            with self.subTest(relative=relative, mutation=old), self.assertRaisesRegex(
                    ValueError, 'canonical baseline changed') as failure:
                candidate.source_inputs(repo, expected)
            self.assertIn(relative, str(failure.exception))
            path.write_bytes(original)
            self.commit_source_fixture(repo)

    def test_exported_capture_admission_sources_cannot_be_omitted_or_rehashed(self):
        evidence, source, report = self.successful_evidence_fixture()
        candidate.validate_evidence(evidence)
        paths = candidate.CAPTURE_ADMISSION_PATHS | {
            'crates/flightsim-app/src/main.rs',
            'crates/flightsim-app/src/screen_capture.rs',
            'crates/flightsim-app/src/capture_backpressure.rs',
        }
        self.assertEqual(len(paths), 5)
        for relative in sorted(paths):
            for mutation in ('missing file', 'missing reviewed record', 'changed canonical digest',
                             'changed checkout digests', 'removed contract row'):
                changed = json.loads(json.dumps(source))
                if mutation == 'missing file':
                    changed['files'] = [row for row in changed['files'] if row['path'] != relative]
                elif mutation == 'missing reviewed record':
                    del changed['reviewed_replay_source_evidence'][relative]
                elif mutation == 'changed canonical digest':
                    changed['reviewed_replay_source_evidence'][relative]['canonical_sha256'] = '0' * 64
                elif mutation == 'changed checkout digests':
                    changed['reviewed_replay_source_evidence'][relative]['canonical_sha256'] = '0' * 64
                    changed['reviewed_replay_source_evidence'][relative]['checkout_sha256'] = '0' * 64
                    for row in changed['files']:
                        if row['path'] == relative:
                            row['checkout_sha256'] = '0' * 64
                else:
                    del changed['replay_contract']['source_sha256'][relative]
                    changed['replay_contract_text'] = json.dumps(changed['replay_contract'])
                    changed['replay_contract_sha256'] = candidate.hashlib.sha256(changed['replay_contract_text'].encode()).hexdigest()
                    for row in changed['files']:
                        if row['path'] == candidate.REPLAY_CONTRACT_PATH:
                            row['checkout_sha256'] = changed['replay_contract_sha256']
                            row['checkout_bytes'] = len(changed['replay_contract_text'].encode())
                candidate.write_json(evidence / 'source-inputs.json', changed)
                self.seal_report(evidence, {**report,
                    'source_inputs_sha256': candidate.digest(evidence / 'source-inputs.json'),
                    'replay_contract_sha256': changed['replay_contract_sha256']})
                with self.subTest(relative=relative, mutation=mutation), self.assertRaises(ValueError):
                    candidate.validate_evidence(evidence)

    def test_local_package_closure_provenance_and_semantic_anchors_are_complete(self):
        import hashlib
        import tomllib
        self.assertEqual(len(candidate.VENDORED_PACKAGE_PATHS), 207)
        self.assertEqual(len(candidate.REPLACEMENT_WITNESS_PATHS), 20)
        self.assertEqual(len(candidate.MODIFIED_SOURCE_POLICY_PATHS), 9)
        self.assertEqual(len(candidate.HISTORICAL_REPLAY_CONTRACT_PATHS), 404)
        self.assertEqual(len(candidate.CORE_PIPELINE_SOURCE_PATHS), 55)
        self.assertEqual(len(candidate.REPLAY_CONTRACT_PATHS), 956)
        component_additions = {
            "LICENSE-MIT", "LICENSE-APACHE",
            "crates/flightsim-app/src/component_terms.rs",
            "crates/flightsim-app/src/component_terms_dialog.ps1",
            "docs/release/components/MICROSOFT-COMPONENT-TERMS.txt",
            "docs/release/components/MICROSOFT-COMPONENT-TERMS.ja.txt",
            "docs/release/components/MICROSOFT-COMPONENT-NOTICE.txt",
            "scripts/component-terms-source-migration.json",
            "scripts/history/4d40f9a-flightsim-app-main.rs",
        }
        self.assertEqual(candidate.COMPONENT_TERMS_SOURCE_PATHS
                         | {candidate.COMPONENT_TERMS_MIGRATION_PATH, candidate.COMPONENT_TERMS_HISTORY_PATH},
                         component_additions)
        self.assertTrue(component_additions <= candidate.REPLAY_CONTRACT_PATHS)
        terrain_additions = (candidate.TERRAIN_CENTROID_RUNTIME_PATHS
                             | {candidate.TERRAIN_CENTROID_MIGRATION_PATH, candidate.TERRAIN_CENTROID_HISTORY_PATH})
        self.assertEqual(len(candidate.REPLAY_CONTRACT_PATHS - component_additions - terrain_additions
                             - candidate.TERRAIN_STITCH_SOURCE_PATHS - candidate.ALPHA22_SOURCE_PATHS
                             - candidate.COCKPIT_SOURCE_PATHS - candidate.ALPHA23_SOURCE_PATHS), 918)
        for prefix, paths in candidate.MODIFIED_SOURCE_BOUNDARIES:
            actual = {path.relative_to(ROOT).as_posix() for path in (ROOT / prefix).rglob('*') if path.is_file()}
            self.assertEqual(actual, {p for p in paths if p.startswith(prefix)}, prefix)
        cargo = tomllib.loads((ROOT / 'Cargo.toml').read_text())
        self.assertEqual(cargo['patch']['crates-io'], {
            'zune-jpeg': {'path': 'vendor/zune-jpeg'},
            'bevy_pbr': {'path': 'vendor/bevy_pbr'},
            'bevy_core_pipeline': {'path': 'vendor/bevy_core_pipeline'},
        })
        self.assertEqual(len(cargo['workspace']['members']), 13)
        self.assertEqual(cargo['workspace']['exclude'], ['vendor/osmpbf', 'vendor/zune-jpeg', 'vendor/bevy_pbr', 'vendor/bevy_core_pipeline'])
        lock = tomllib.loads((ROOT / 'Cargo.lock').read_text())['package']
        for name, version, directory in (
            ('zune-jpeg', '0.5.15', 'vendor/zune-jpeg'),
            ('bevy_pbr', '0.18.1', 'vendor/bevy_pbr'),
        ):
            packages = [entry for entry in lock if entry['name'] == name]
            self.assertEqual(len(packages), 1, name)
            self.assertEqual(packages[0]['version'], version)
            self.assertNotIn('source', packages[0])
            self.assertNotIn('checksum', packages[0])
            manifest = tomllib.loads((ROOT / directory / 'Cargo.toml').read_text())
            self.assertEqual(manifest['package']['name'], name)
            self.assertEqual(manifest['package']['version'], version)
            self.assertEqual(manifest['package']['license'], 'MIT OR Apache-2.0')
            self.assertIs(manifest['package']['build'], False)
            self.assertEqual(manifest['lib']['path'], 'src/lib.rs')
        zune = tomllib.loads((ROOT / 'vendor/zune-jpeg/Cargo.toml').read_text())
        self.assertEqual(zune['features']['default'], ['x86', 'neon', 'std'])
        provenance = json.loads((ROOT / 'docs/release/analytical-modified-source-provenance.json').read_text())
        rows = {}
        for package in provenance['packages']:
            self.assertIn(package['source_root'], ('vendor/zune-jpeg', 'vendor/bevy_pbr'))
            for record in package['files']:
                path = package['source_root'] + '/' + record['path']
                self.assertNotIn(path, rows)
                rows[path] = record
                data = (ROOT / path).read_bytes()
                self.assertEqual(len(data), record['bytes'], path)
                self.assertEqual(hashlib.sha256(data).hexdigest(), record['sha256'], path)
        self.assertEqual(set(rows), candidate.VENDORED_PACKAGE_PATHS)
        headers = json.loads((ROOT / 'docs/release/analytical-source-header-evidence.json').read_text())['records']
        self.assertEqual(len(headers), 510)
        roots = {'zune-jpeg@0.5.15': 'vendor/zune-jpeg', 'bevy_pbr@0.18.1': 'vendor/bevy_pbr'}
        for record in headers:
            if record['package_id'] not in roots:
                continue
            path = roots[record['package_id']] + '/' + record['source_relative_path']
            self.assertIn(path, candidate.VENDORED_PACKAGE_PATHS)
            data = (ROOT / path).read_bytes()
            self.assertEqual(hashlib.sha256(data).hexdigest(), record['source_sha256'], path)
            start, length = record['start_byte'], record['byte_length']
            self.assertGreaterEqual(start, 0)
            self.assertGreater(length, 0)
            self.assertLessEqual(start + length, len(data))
            self.assertEqual(hashlib.sha256(data[start:start + length]).hexdigest(), record['excerpt_sha256'])
        # Whole-file pins remain authoritative; these literal source semantics
        # independently prevent a no-op/feature-removal migration being called equivalent.
        for relative, old, _new in MODIFIED_SOURCE_MUTATIONS:
            self.assertEqual((ROOT / relative).read_bytes().count(old), 1, relative)
        self.assertFalse((ROOT / 'vendor/bevy_pbr/src/meshlet/meshlet_preview.png').exists())
        for root in roots.values():
            self.assertFalse((ROOT / root / '.cargo_vcs_info.json').exists())

    def test_modified_source_exact_sets_reject_missing_extra_and_case_aliases(self):
        paths = set(candidate.REPLAY_CONTRACT_PATHS)
        candidate.validate_modified_source_boundaries(paths)
        candidate.validate_modified_source_boundaries(paths | {'docs/unrelated-source-note.md'})
        for prefix, declared in candidate.MODIFIED_SOURCE_BOUNDARIES:
            original = sorted(path for path in declared if path.startswith(prefix))[0]
            alias = prefix.upper() + original[len(prefix):]
            for changed in (paths - {original}, paths | {prefix + 'unreviewed.rs'},
                            (paths - {original}) | {alias}, paths | {alias}):
                with self.subTest(prefix=prefix), self.assertRaisesRegex(ValueError, 'modified source boundary changed'):
                    candidate.validate_modified_source_boundaries(changed)

    def test_ignored_vendor_and_witness_inputs_cannot_escape_the_live_boundary(self):
        repo, _ = self.source_fixture()
        candidate.validate_modified_source_checkout(repo)
        excludes = repo / '.git/info/exclude'
        excludes.write_text('build.rs\n.cargo/\n', encoding='utf-8')
        for prefix, _declared in candidate.MODIFIED_SOURCE_BOUNDARIES:
            for suffix in ('build.rs', '.cargo/config.toml'):
                path = repo / prefix / suffix
                self.assertFalse(path.exists())
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('unreviewed ignored fixture\n', encoding='utf-8')
                self.assertEqual(candidate.git(repo, 'status', '--porcelain', '--untracked-files=all'), '')
                with self.subTest(prefix=prefix, suffix=suffix), self.assertRaisesRegex(
                        ValueError, 'unreviewed modified source checkout input'):
                    candidate.validate_modified_source_checkout(repo)
                path.unlink()
        candidate.validate_modified_source_checkout(repo)
        # Exercise every ancestor/member guard without requiring Windows
        # symlink-creation privileges in the ordinary source-only CI suite.
        for prefix, declared in candidate.MODIFIED_SOURCE_BOUNDARIES:
            relative = sorted(path for path in declared if path.startswith(prefix))[0]
            for alias in (repo / Path(prefix).parts[0], repo / prefix, repo / relative):
                with self.subTest(alias=alias), patch.object(
                        Path, 'is_symlink', autospec=True, side_effect=lambda path: path == alias):
                    with self.assertRaisesRegex(ValueError, 'modified source (root|symlink)'):
                        candidate.validate_modified_source_checkout(repo)

    def test_windows_reparse_inputs_cannot_escape_vendor_or_witness_roots(self):
        from types import SimpleNamespace
        repo, _ = self.source_fixture()
        candidate.validate_modified_source_checkout(repo)
        original_lstat = Path.lstat
        for prefix, _declared in candidate.MODIFIED_SOURCE_BOUNDARIES:
            for reparse in (repo / Path(prefix).parts[0], repo / prefix,
                            repo / sorted(path for path in _declared if path.startswith(prefix))[0]):
                self.assertTrue(reparse.exists())
                def reparse_details(path, *args, **kwargs):
                    details = original_lstat(path, *args, **kwargs)
                    if path == reparse:
                        return SimpleNamespace(st_mode=details.st_mode, st_file_attributes=0x400)
                    return details
                with self.subTest(reparse=reparse), patch.object(
                        Path, 'lstat', autospec=True, side_effect=reparse_details):
                    with self.assertRaisesRegex(ValueError, 'reparse'):
                        candidate.validate_modified_source_checkout(repo)
                candidate.validate_modified_source_checkout(repo)

    def test_local_package_semantic_mutations_reject_before_build(self):
        repo, _ = self.source_fixture()
        candidate.source_inputs(repo, candidate.git(repo, 'rev-parse', 'HEAD'))
        for relative, old, new in MODIFIED_SOURCE_MUTATIONS:
            self.assertIn(relative, candidate.REPLAY_CONTRACT_PATHS)
            path = repo / relative
            original = path.read_bytes()
            self.assertEqual(original.count(old), 1, relative)
            path.write_bytes(original.replace(old, new))
            expected = self.commit_source_fixture(repo)
            with self.subTest(relative=relative, mutation=old), self.assertRaisesRegex(ValueError, 'canonical baseline changed') as failure:
                candidate.source_inputs(repo, expected)
            self.assertIn(relative, str(failure.exception))
            path.write_bytes(original)
            self.commit_source_fixture(repo)

    def test_local_package_provenance_notice_and_asset_mutations_reject(self):
        repo, _ = self.source_fixture()
        paths = (
            'vendor/bevy_pbr/src/bluenoise/stbn.ktx2',
            'vendor/bevy_pbr/third-party-notices/EA-fastnoise-LICENSE.txt',
            'docs/release/analytical-modified-source-provenance.json',
            'docs/release/analytical-source-header-evidence.json',
        )
        for relative in paths:
            path = repo / relative
            original = path.read_bytes()
            if relative.endswith('.ktx2'):
                changed = bytes([original[0] ^ 1]) + original[1:]
            elif relative.endswith('LICENSE.txt'):
                changed = original + b'\n'
            elif relative.endswith('analytical-modified-source-provenance.json'):
                value = json.loads(original)
                package = next(entry for entry in value['packages'] if entry['id'] == 'bevy_pbr@0.18.1')
                old_count = len(package['files'])
                package['files'] = [entry for entry in package['files'] if entry['path'] != 'src/render/parallax_mapping.wgsl']
                self.assertEqual(len(package['files']), old_count - 1)
                changed = json.dumps(value).encode()
            else:
                value = json.loads(original)
                entry = next(entry for entry in value['records'] if entry['package_id'] == 'zune-jpeg@0.5.15')
                entry['source_sha256'] = '0' * 64
                changed = json.dumps(value).encode()
            self.assertNotEqual(changed, original)
            path.write_bytes(changed)
            expected = self.commit_source_fixture(repo)
            with self.subTest(relative=relative), self.assertRaisesRegex(ValueError, 'canonical baseline changed') as failure:
                candidate.source_inputs(repo, expected)
            self.assertIn(relative, str(failure.exception))
            path.write_bytes(original)
            self.commit_source_fixture(repo)

    def test_committed_vendor_and_witness_members_cannot_expand_or_shrink(self):
        repo, _ = self.source_fixture()
        candidate.source_inputs(repo, candidate.git(repo, 'rev-parse', 'HEAD'))
        for prefix, declared in candidate.MODIFIED_SOURCE_BOUNDARIES:
            extra = repo / (prefix + 'unreviewed.rs')
            self.assertFalse(extra.exists())
            extra.write_bytes(b'unreviewed source fixture\n')
            expected = self.commit_source_fixture(repo)
            with self.subTest(prefix=prefix, operation='addition'), self.assertRaisesRegex(ValueError, 'modified source boundary changed'):
                candidate.source_inputs(repo, expected)
            extra.unlink()
            self.commit_source_fixture(repo)
            relative = sorted(path for path in declared if path.startswith(prefix))[0]
            path = repo / relative
            original = path.read_bytes()
            path.unlink()
            expected = self.commit_source_fixture(repo)
            with self.subTest(prefix=prefix, operation='omission'), self.assertRaisesRegex(ValueError, 'modified source boundary changed'):
                candidate.source_inputs(repo, expected)
            path.write_bytes(original)
            self.commit_source_fixture(repo)

    def test_exported_modified_sources_cannot_be_omitted_or_rehashed(self):
        evidence, source, report = self.successful_evidence_fixture()
        candidate.validate_evidence(evidence)
        paths = (candidate.VENDORED_PACKAGE_PATHS | candidate.REPLACEMENT_WITNESS_PATHS
                 | candidate.MODIFIED_SOURCE_POLICY_PATHS)
        self.assertEqual(len(paths), 236)
        for relative in sorted(paths):
            for mutation in ('missing file', 'missing reviewed record', 'changed canonical digest',
                             'changed checkout digests', 'removed contract row'):
                changed = json.loads(json.dumps(source))
                if mutation == 'missing file':
                    changed['files'] = [row for row in changed['files'] if row['path'] != relative]
                elif mutation == 'missing reviewed record':
                    del changed['reviewed_replay_source_evidence'][relative]
                elif mutation == 'changed canonical digest':
                    changed['reviewed_replay_source_evidence'][relative]['canonical_sha256'] = '0' * 64
                elif mutation == 'changed checkout digests':
                    changed['reviewed_replay_source_evidence'][relative]['canonical_sha256'] = '0' * 64
                    changed['reviewed_replay_source_evidence'][relative]['checkout_sha256'] = '0' * 64
                    for row in changed['files']:
                        if row['path'] == relative:
                            row['checkout_sha256'] = '0' * 64
                else:
                    del changed['replay_contract']['source_sha256'][relative]
                    changed['replay_contract_text'] = json.dumps(changed['replay_contract'])
                    changed['replay_contract_sha256'] = candidate.hashlib.sha256(changed['replay_contract_text'].encode()).hexdigest()
                    for row in changed['files']:
                        if row['path'] == candidate.REPLAY_CONTRACT_PATH:
                            row['checkout_sha256'] = changed['replay_contract_sha256']
                            row['checkout_bytes'] = len(changed['replay_contract_text'].encode())
                candidate.write_json(evidence / 'source-inputs.json', changed)
                self.seal_report(evidence, {**report,
                    'source_inputs_sha256': candidate.digest(evidence / 'source-inputs.json'),
                    'replay_contract_sha256': changed['replay_contract_sha256']})
                with self.subTest(relative=relative, mutation=mutation), self.assertRaises(ValueError):
                    candidate.validate_evidence(evidence)

    def test_exported_vendor_and_witness_sets_reject_extra_rows_and_aliases(self):
        evidence, source, report = self.successful_evidence_fixture()
        candidate.validate_evidence(evidence)
        for prefix, declared in candidate.MODIFIED_SOURCE_BOUNDARIES:
            original = sorted(path for path in declared if path.startswith(prefix))[0]
            for mutation in ('extra member', 'case alias', 'resealed additional contract member'):
                changed = json.loads(json.dumps(source))
                if mutation == 'case alias':
                    for row in changed['files']:
                        if row['path'] == original:
                            row['path'] = prefix.upper() + original[len(prefix):]
                else:
                    relative = prefix + 'unreviewed.rs'
                    record = {'path': relative, 'canonical_git_blob': candidate.hashlib.new(
                        changed['canonical_git_object_format'], b'blob 0\0').hexdigest(),
                        'git_mode': '100644', 'checkout_bytes': 0,
                        'checkout_sha256': candidate.hashlib.sha256(b'').hexdigest()}
                    changed['files'].append(record)
                    if mutation == 'resealed additional contract member':
                        changed['replay_contract']['source_sha256'][relative] = record['checkout_sha256']
                        changed['reviewed_replay_source_evidence'][relative] = {
                            'canonical_sha256': record['checkout_sha256'], 'canonical_bytes': 0,
                            'canonical_git_blob': record['canonical_git_blob'],
                            'checkout_sha256': record['checkout_sha256'], 'checkout_bytes': 0}
                        changed['replay_contract_text'] = json.dumps(changed['replay_contract'])
                        changed['replay_contract_sha256'] = candidate.hashlib.sha256(changed['replay_contract_text'].encode()).hexdigest()
                        for row in changed['files']:
                            if row['path'] == candidate.REPLAY_CONTRACT_PATH:
                                row['checkout_sha256'] = changed['replay_contract_sha256']
                                row['checkout_bytes'] = len(changed['replay_contract_text'].encode())
                candidate.write_json(evidence / 'source-inputs.json', changed)
                self.seal_report(evidence, {**report,
                    'source_inputs_sha256': candidate.digest(evidence / 'source-inputs.json'),
                    'replay_contract_sha256': changed['replay_contract_sha256']})
                with self.subTest(prefix=prefix, mutation=mutation), self.assertRaises(ValueError):
                    candidate.validate_evidence(evidence)

    def test_all_protected_sources_and_notices_survive_committed_archives(self):
        spec = importlib.util.spec_from_file_location(
            'reviewed_local_package_archive', ROOT / 'scripts/check-source-archive.py')
        policy = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(policy)
        protected = candidate.REPLAY_CONTRACT_PATHS | set(candidate.INDEPENDENT_REPLAY_HASHES)
        for fmt in ('zip', 'tar'):
            commit, payload = policy.committed_archive(ROOT, 'HEAD', fmt)
            self.assertEqual(commit, candidate.git(ROOT, 'rev-parse', 'HEAD'))
            members = policy.normalized_members(payload)
            self.assertIn(policy.DENIED_PATH, set(members))
            self.assertEqual(candidate.hashlib.sha256(members[policy.DENIED_PATH]).hexdigest(), policy.ORIGINAL_HIGHWING_SHA256)
            policy.verify(ROOT, 'HEAD', payload, fmt, policy=policy.ORIGINAL_HIGHWING_POLICY)
            self.assertTrue(protected <= set(members), sorted(protected - set(members)))
            for relative in protected:
                self.assertEqual(candidate.hashlib.sha256(members[relative]).hexdigest(), candidate.digest(ROOT / relative), relative)

    def test_reviewed_source_archive_policy_suite_runs_in_source_ci(self):
        result = subprocess.run([candidate.sys.executable, str(ROOT / 'scripts/test_source_archive.py')],
                                cwd=ROOT, text=True, capture_output=True, timeout=60, check=False)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn('Ran 16 tests', result.stderr)
        self.assertIn('\nOK\n', result.stderr)

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


class CandidateProcessTimeoutTests(unittest.TestCase):
    """Regression for run 37955865712: a dialog descendant retained pipe ends."""

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)

    def test_exited_parent_does_not_wait_for_descendant_output_handles(self):
        # The root exits normally, while a descendant inherits both output
        # handles. Waiting for pipe EOF would falsely time out this command.
        code = ("import subprocess, sys; "
                "subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(3)']); "
                "print('root complete', flush=True)")
        started = time.monotonic()
        result = candidate.run_bounded([sys.executable, '-c', code], cwd=self.root,
                                       env=os.environ.copy(), timeout=1, private=self.root)
        self.assertEqual(result.returncode, 0)
        self.assertIn(b'root complete', result.stdout)
        self.assertLess(time.monotonic() - started, 2.5)
        # Let this deliberately detached child close its inherited private file
        # handles before TemporaryDirectory cleanup on Windows.
        time.sleep(3.1)

    def test_late_descendant_writer_cannot_overwrite_output_during_readback(self):
        code = ("import subprocess, sys; "
                "subprocess.Popen([sys.executable, '-c', "
                "\"import sys, time; time.sleep(2); print('late child', flush=True)\"]); "
                "print('original root output', flush=True)")
        result = candidate.run_bounded([sys.executable, '-c', code], cwd=self.root,
                                       env=os.environ.copy(), timeout=1, private=self.root)
        self.assertEqual(result.stdout, b'original root output' + os.linesep.encode())
        time.sleep(2.2)
        logs = list(self.root.glob('command-*-stdout.log'))
        self.assertEqual(len(logs), 1)
        self.assertEqual(logs[0].read_bytes(), os.linesep.join(('original root output', 'late child', '')).encode())

    def test_timeout_retains_streams_and_stops_descendant_before_it_writes(self):
        marker = self.root / 'escaped.txt'
        child = "import time; from pathlib import Path; time.sleep(2); Path('escaped.txt').write_text('orphan')"
        code = ("import subprocess, sys, time; "
                f"subprocess.Popen([sys.executable, '-c', {child!r}]); "
                "print('stdout before timeout', flush=True); "
                "print('stderr before timeout', file=sys.stderr, flush=True); time.sleep(30)")
        started = time.monotonic()
        with self.assertRaises(subprocess.TimeoutExpired) as raised:
            candidate.run_bounded([sys.executable, '-c', code], cwd=self.root,
                                  env=os.environ.copy(), timeout=0.5, private=self.root)
        self.assertLess(time.monotonic() - started, 2.5)
        self.assertIn(b'stdout before timeout', raised.exception.stdout)
        self.assertIn(b'stderr before timeout', raised.exception.stderr)
        time.sleep(2.1)
        self.assertFalse(marker.exists(), 'timed-out descendant survived the process tree cleanup')

    def test_windows_cleanup_terminates_tree_before_bounded_root_wait(self):
        process = Mock(pid=123, poll=Mock(return_value=9))
        killer = Mock(wait=Mock(return_value=0))
        with patch.object(candidate.sys, "platform", "win32"), \
                patch.dict(candidate.os.environ, {"SystemRoot": "C:/Windows"}), \
                patch.object(candidate.subprocess, "Popen", return_value=killer) as launch:
            failure = candidate.stop_candidate_process_tree(process)
        self.assertEqual(failure, "")
        self.assertEqual(launch.call_args.args[0],
                         [str(Path("C:/Windows") / "System32/taskkill.exe"), "/PID", "123", "/T", "/F"])
        self.assertEqual(launch.call_args.kwargs,
                         {"stdout": subprocess.DEVNULL, "stderr": subprocess.DEVNULL})
        killer.wait.assert_called_once_with(timeout=10)
        process.wait.assert_called_once_with(timeout=10)
        process.kill.assert_not_called()

    def test_windows_cleanup_failure_has_only_bounded_waits_and_root_fallback(self):
        process = Mock(pid=123, poll=Mock(return_value=None))
        process.wait.side_effect = subprocess.TimeoutExpired("root", 10)
        killer = Mock()
        killer.wait.side_effect = subprocess.TimeoutExpired("taskkill", 10)
        with patch.object(candidate.sys, "platform", "win32"), \
                patch.dict(candidate.os.environ, {"SystemRoot": "C:/Windows"}), \
                patch.object(candidate.subprocess, "Popen", return_value=killer):
            failure = candidate.stop_candidate_process_tree(process)
        self.assertIn("process-tree termination failed", failure)
        self.assertIn("root-process cleanup failed", failure)
        self.assertEqual(killer.wait.call_count, 2)
        for call in killer.wait.call_args_list + process.wait.call_args_list:
            self.assertEqual(call.kwargs, {"timeout": 10})
        killer.kill.assert_called_once_with()
        process.kill.assert_called_once_with()

    def test_timeout_stays_failure_when_tree_cleanup_reports_failure(self):
        # Simulate Windows tree-cleanup failure while still reaping our real
        # disposable Linux child; logs must not silently claim clean shutdown.
        real_stop = candidate.stop_candidate_process_tree
        def failed_stop(process):
            real_stop(process)
            return "injected process-tree termination failure"
        with patch.object(candidate, "stop_candidate_process_tree", side_effect=failed_stop):
            with self.assertRaises(subprocess.TimeoutExpired) as raised:
                candidate.run_bounded([sys.executable, '-c', 'import time; time.sleep(30)'],
                                      cwd=self.root, env=os.environ.copy(), timeout=0.2, private=self.root)
        self.assertIn(b'injected process-tree termination failure', raised.exception.stderr)

    def test_windows_launcher_keeps_baseline_flags_and_file_backed_output(self):
        process = Mock(returncode=0)
        with patch.object(candidate.sys, "platform", "win32"), \
                patch.object(candidate.subprocess, "Popen", return_value=process) as launch:
            result = candidate.run_bounded(['app.exe'], cwd=self.root, env={},
                                           timeout=180, private=self.root)
        self.assertEqual(result.returncode, 0)
        self.assertNotIn('creationflags', launch.call_args.kwargs)
        self.assertNotIn('startupinfo', launch.call_args.kwargs)
        self.assertNotIn('start_new_session', launch.call_args.kwargs)
        self.assertNotEqual(launch.call_args.kwargs['stdout'], subprocess.PIPE)
        self.assertNotEqual(launch.call_args.kwargs['stderr'], subprocess.PIPE)
        process.wait.assert_called_once_with(timeout=180)
        process.communicate.assert_not_called()

    def test_final_log_bound_includes_both_streams_utf8_and_cleanup_notes(self):
        with patch.object(candidate, 'MAX_EVIDENCE_BYTES', 256):
            log, truncated = candidate.command_log(b'out' * 100, b'\xff' * 100 + b'cleanup failed',
                                                    self.root / 'source', self.root / 'private')
        self.assertTrue(truncated)
        self.assertLessEqual(len(log.encode('utf-8')), 256)
        self.assertIn('[harness] Output exceeded', log)
        self.assertTrue(log.endswith('cleanup failed'))

    def test_timeout_output_truncation_stays_bounded_and_explicit(self):
        with patch.object(candidate, 'MAX_EVIDENCE_BYTES', 256):
            with self.assertRaises(subprocess.TimeoutExpired) as raised:
                candidate.run_bounded(
                    [sys.executable, '-c', "import sys, time; sys.stderr.write('x' * 257); sys.stderr.flush(); time.sleep(30)"],
                    cwd=self.root, env=os.environ.copy(), timeout=0.2, private=self.root)
            log, truncated = candidate.command_log(raised.exception.stdout, raised.exception.stderr,
                                                    self.root / 'source', self.root / 'private')
        self.assertTrue(truncated)
        self.assertLessEqual(len(log.encode('utf-8')), 256)
        self.assertIn('truncated', log)

    def test_windows_taskkill_launch_and_nonzero_errors_are_reported(self):
        for error in (OSError('taskkill unavailable'), None):
            process = Mock(pid=123, poll=Mock(return_value=None))
            killer = Mock(wait=Mock(return_value=1))
            with self.subTest(error=error), patch.object(candidate.sys, 'platform', 'win32'), \
                    patch.dict(candidate.os.environ, {'SystemRoot': 'C:/Windows'}), \
                    patch.object(candidate.subprocess, 'Popen', return_value=killer, side_effect=error):
                failure = candidate.stop_candidate_process_tree(process)
            self.assertIn('termination', failure)
            process.kill.assert_called_once_with()
            process.wait.assert_called_once_with(timeout=10)

    def test_completed_nonzero_status_and_separate_byte_streams_are_preserved(self):
        result = candidate.run_bounded(
            [sys.executable, '-c', "import sys; sys.stdout.buffer.write(b'out\\x00'); sys.stderr.write('err'); sys.exit(7)"],
            cwd=self.root, env=os.environ.copy(), timeout=2, private=self.root)
        self.assertEqual((result.returncode, result.stdout, result.stderr), (7, b'out\x00', b'err'))


class CandidateWorkflowTests(unittest.TestCase):
    def test_windows_process_regressions_run_in_existing_native_ci_job(self):
        text = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        job = text.split("  content_download_smoke:\n", 1)[1].split("  world_source_tools:\n", 1)[0]
        self.assertIn("os: [windows-latest, ubuntu-latest]", job)
        setup = job.split("      - uses: actions/setup-python@", 1)[1].split("      - name:", 1)[0]
        self.assertIn("if: runner.os == 'Windows'", setup)
        self.assertIn("python-version: '3.12'", setup)
        step = job.split("      - name: Check Windows candidate process deadlines\n", 1)[1].split("      - uses:", 1)[0]
        self.assertIn("if: runner.os == 'Windows'", step)
        self.assertIn("python -m unittest scripts.tests.test_swift_windows_candidate.CandidateProcessTimeoutTests -v", step)

    def test_workflow_is_manual_exact_main_diagnostics_only(self):
        text = (ROOT / ".github/workflows/swift-windows-candidate.yml").read_text(encoding="utf-8")
        for guard in ("workflow_dispatch:", "github.ref == 'refs/heads/main'",
                      "persist-credentials: false", "ref: ${{ github.sha }}", "timeout-minutes: 90",
                      "EXPECTED_SOURCE_SHA: ${{ github.sha }}"):
            self.assertIn(guard, text)
        for setting in ("GIT_CONFIG_COUNT: '2'", "GIT_CONFIG_KEY_0: core.autocrlf",
                        "GIT_CONFIG_VALUE_0: 'false'", "GIT_CONFIG_KEY_1: core.eol", "GIT_CONFIG_VALUE_1: 'lf'"):
            self.assertIn(setting, text)
        for forbidden in ("contents: write", "uses: Swatinem/rust-cache", "uses: actions/cache",
                          "gh release", "git tag", "workflow_run:", "--diagnose-readback"):
            self.assertNotIn(forbidden, text)
        block = text.split("          path: |\n", 1)[1].split("          if-no-files-found:", 1)[0]
        names = {line.strip().rsplit("/", 1)[1] for line in block.splitlines() if line.strip()}
        self.assertEqual(names, candidate.TEXT_EVIDENCE | candidate.PNG_EVIDENCE)
        self.assertNotIn("*", block)
        self.assertIn("steps.evidence.outputs.validated == 'true'", text)
        self.assertNotIn("uses: ./.github/workflows/release.yml", text)


if __name__ == "__main__":
    unittest.main()
