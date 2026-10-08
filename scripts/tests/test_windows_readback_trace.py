#!/usr/bin/env python3
"""Adversarial tests for the isolated, single-launch diagnostic harness."""
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[2]


def module(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


trace = module(ROOT / "scripts/run-windows-readback-trace.py", "windows_readback_trace")
ordinary = module(ROOT / "scripts/check-swift-windows-candidate.py", "windows_ordinary_candidate")


def event(seq=0, name="capture_prepare", us=1, a=0, b=0, c=0):
    return f"FS_READBACK_TRACE schema=1 seq={seq} us={us} event={name} a={a} b={b} c={c}\n"


def status(reserved=1, drained=1, dropped=0, io_errors=0):
    return f"FS_READBACK_TRACE_STATUS schema=1 reserved={reserved} drained={drained} dropped={dropped} io_errors={io_errors} capacity=2048\n"


def failed_report():
    return {"schema_version": 1, "identity": trace.IDENTITY, **trace.QUALIFICATION,
            "baseline_commit": trace.BASELINE, "baseline_tree": trace.BASELINE_TREE,
            "diagnostic_commit": "a" * 40, "target": trace.TARGET, "toolchain": trace.TOOLCHAIN,
            "baseline_observation": trace.BASELINE_OBSERVATION,
            "features": trace.FEATURES, "default_features": True, "profile": "release",
            "scene_attempts": 0, "informational_attempts": 0, "informational_invocations": 0, "status": "failed", "failure": "build failed"}


class TraceWireTests(unittest.TestCase):
    def test_adapter_requires_complete_baseline_identity(self):
        line = 'INFO bevy_render::renderer: AdapterInfo { name: "Microsoft Basic Render Driver", vendor: 5140, device: 140, device_type: Cpu, driver: "10.0.26100.33438", driver_info: "", backend: Dx12 }'
        self.assertEqual(trace.adapter_observation(line)["status"], "matches_baseline")
        self.assertEqual(trace.adapter_observation(line.replace("33438", "33439"))["status"], "mismatch")
        self.assertEqual(trace.adapter_observation("")["status"], "unobserved")
        self.assertEqual(trace.adapter_observation(line + "\n" + line)["status"], "unparsed_or_ambiguous")

    def test_exact_events_match_committed_instrumentation_schema(self):
        table = json.loads((ROOT / "diagnostics/windows-readback/events.json").read_text())
        self.assertEqual(set(table["events"]), trace.EVENT_NAMES)
        self.assertEqual(table["capacity"], trace.MAX_EVENTS)

    def test_partial_concurrent_events_do_not_claim_completion_or_timestamp_order(self):
        text = event(0, "channel_send_enter", 20) + event(1, "receive_resumed", 19) + event(2, "channel_send_return", 21) + status(4, 3)
        parsed = trace.parse_trace(text)
        self.assertEqual(parsed["event_count"], 3)
        self.assertEqual(parsed["last_observed_event"], "channel_send_return")
        self.assertFalse(parsed["complete_trace_claimed"])
        self.assertTrue(all(parsed[key] is False for key in trace.QUALIFICATION))

    def test_sequence_gaps_and_overflow_preserve_explicit_unknowns(self):
        parsed = trace.parse_trace(event(2) + status(2050, 3, 2, 2))
        self.assertEqual(parsed["missing_sequence_count"], 2)
        self.assertFalse(parsed["complete_trace_claimed"])
        self.assertEqual(parsed["statuses"][-1]["dropped"], 2)
        self.assertEqual(trace.parse_trace("ordinary log only")["events"], [])

    def test_malformed_unknown_duplicate_overflow_and_embedded_events_rejected(self):
        cases = [event().replace("seq=0", "seq=00"), event().replace("a=0", "a=-1"),
                 event(name="invented"), event(a=2**64), event(2048), event() + event(),
                 event(2) + event(1), "prefix " + event(), event().replace(" c=0", " c=0 extra=1"),
                 status(drained=2), status().replace("capacity=2048", "capacity=512"),
                 status(2, 2) + status(1, 1)]
        for value in cases:
            with self.subTest(value=value), self.assertRaises(ValueError):
                trace.parse_trace(value)


class IntegrityTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.addCleanup(self.temporary.cleanup)

    def test_git_tree_projection_matches_real_git_baseline(self):
        rows = [{"path": name, **record} for name, record in trace.git_files(ROOT, trace.BASELINE).items()]
        self.assertEqual(trace.tree_identity(rows), trace.BASELINE_TREE)
        rows[0]["git_blob"] = "0" * 40
        self.assertNotEqual(trace.tree_identity(rows), trace.BASELINE_TREE)

    def test_source_copy_rejects_changed_bytes_and_symlink(self):
        path = self.root / "input.rs"
        path.write_bytes(b"ordinary\n")
        blob = hashlib.sha1(b"blob 9\0ordinary\n").hexdigest()
        record = {"git_mode": "100644", "git_blob": blob}
        self.assertEqual(trace.verify_checkout_file(self.root, "input.rs", record)["bytes"], 9)
        path.write_bytes(b"ordinary\r\n")
        with self.assertRaisesRegex(ValueError, "canonical Git bytes"):
            trace.verify_checkout_file(self.root, "input.rs", record)
        for bad in ("../escape", "a/../escape", "/absolute", "a\\b", "C:foo", "a//b", "a/./b"):
            with self.subTest(path=bad), self.assertRaises(ValueError):
                trace.safe_relative(bad)

    def test_deterministic_lock_allows_only_path_overlays_and_helper(self):
        # Use the real pinned lock and independently construct only approved deltas.
        original = (ROOT / "Cargo.lock").read_text(encoding="utf-8")
        parts = original.split("[[package]]")
        for index, block in enumerate(parts):
            if 'name = "bevy_render"\n' in block or 'name = "wgpu-core"\n' in block:
                lines = [line for line in block.splitlines(keepends=True)
                         if not line.startswith(("source = ", "checksum = "))]
                parts[index] = "".join(lines).replace("dependencies = [\n", 'dependencies = [\n "flightsim-readback-trace",\n')
        changed = "[[package]]".join(parts) + '[[package]]\nname = "flightsim-readback-trace"\nversion = "0.0.0"\n'
        trace.validate_lock_delta(original, changed)
        for mutation in (changed.replace('version = "0.0.0"', 'version = "0.1.0"'),
                         changed.replace('name = "wgpu-core"', 'name = "wgpu-core-other"'),
                         changed.replace(' "flightsim-readback-trace",\n', "", 1),
                         changed.replace('version = "27.0.3"', 'version = "27.0.4"')):
            with self.subTest(mutation=mutation[-120:]), self.assertRaises(ValueError):
                trace.validate_lock_delta(original, mutation)

    def test_metadata_rejects_feature_or_dependency_drift(self):
        identities = [("flightsim-app", "1"), ("bevy_render", "0.18.1"), ("wgpu-core", "27.0.3"), ("bevy_core_pipeline", "0.18.1")]
        packages = [{"id": str(index), "name": name, "version": version} for index, (name, version) in enumerate(identities)]
        nodes = [{"id": "0", "features": ["default", "commercial-staging"], "dependencies": ["1", "2", "3"]},
                 {"id": "1", "features": [], "dependencies": ["2"]},
                 {"id": "2", "features": [], "dependencies": []},
                 {"id": "3", "features": ["tonemapping_luts"], "dependencies": ["1"]}]
        before = {"packages": packages, "resolve": {"nodes": nodes}}
        after = copy.deepcopy(before)
        after["packages"].append({"id": "helper", "name": trace.HELPER[0], "version": trace.HELPER[1]})
        after["resolve"]["nodes"].append({"id": "helper", "features": [], "dependencies": []})
        for row in after["resolve"]["nodes"][1:3]:
            row["dependencies"].append("helper")
        trace.validate_metadata_delta(before, after)
        for index, field, value in ((0, "features", ["commercial-staging"]), (2, "dependencies", []),
                                    (3, "features", []), (4, "features", ["default"])):
            mutation = copy.deepcopy(after)
            mutation["resolve"]["nodes"][index][field] = value
            with self.subTest(index=index, field=field), self.assertRaises(ValueError):
                trace.validate_metadata_delta(before, mutation)


class CaptureTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.addCleanup(self.temporary.cleanup)
        self.repo, self.work, self.evidence = [self.root / name for name in ("repo", "work", "evidence")]
        for directory in (self.repo, self.work, self.evidence):
            directory.mkdir()
        self.app = self.work / "extracted/swift-candidate/flightsim-app.exe"
        self.app.parent.mkdir(parents=True)
        self.app.write_bytes(b"diagnostic-executable-fixture")
        self.run = trace.Commands(self.repo, self.work, self.evidence, {"TRACE_RUST_LOG": ordinary.CAPTURE_TRACE, "RUSTFLAGS": "-D warnings"})
        build_command = [*ordinary.candidate_commands()["build"], "--config", "<private-work>/dependencies/patch-config.toml"]
        self.run.records.extend([
            {"command": build_command, "exit_code": 0},
            {"command": ["python", "<source>/scripts/stage-commercial-candidate.py"], "exit_code": 1},
        ])
        self.report = failed_report()
        self.report.update(executable_sha256=trace.digest(self.app), informational_attempts=1, informational_invocations=1,
                           commands=self.run.records, runtime_environment={"WGPU_BACKEND": "dx12", "WGPU_FORCE_FALLBACK_ADAPTER": "1",
                               "BEVY_ASSET_ROOT": "<source>", "CARGO_MANIFEST_DIR": "<source>", "RUST_LOG": ordinary.CAPTURE_TRACE},
                           compiler_flags={"RUSTFLAGS": "-D warnings", "CARGO_TARGET_DIR": "<private-work>/target"},
                           build_command=build_command)
        self.input_fixture()

    def input_fixture(self):
        def row(path, data=b"fixture"):
            return {"path": path, "bytes": len(data), "sha256": hashlib.sha256(data).hexdigest()}

        def manifest(rows):
            encoded = json.dumps(rows, sort_keys=True, separators=(",", ":")).encode()
            return {"files": rows, "tree_sha256": hashlib.sha256(encoded).hexdigest()}

        prefix = "diagnostics/windows-readback/"
        baseline = ["Cargo.lock", "assets/aircraft/swift_sport.glb", "assets/aircraft/swift_sport.json"]
        extra = sorted([*trace.NEW_FILES, *(prefix + name for name in trace.RECIPE_FILES)])
        files = []
        for name in sorted(baseline + extra):
            record = row(name)
            record.update(git_mode="100644", git_blob=hashlib.sha1(b"blob 7\0fixture").hexdigest())
            files.append(record)
        baseline_tree = trace.tree_identity([record for record in files if record["path"] in baseline])
        self.addCleanup(patch.stopall)
        patch.object(trace, "BASELINE_TREE", baseline_tree).start()
        self.report.update(baseline_tree=baseline_tree, diagnostic_tree=trace.tree_identity(files), final_lock_sha256="f" * 64)
        source = {"schema_version": 1, "identity": trace.IDENTITY, "baseline_commit": trace.BASELINE,
                  "baseline_tree": baseline_tree, "diagnostic_commit": self.report["diagnostic_commit"],
                  "diagnostic_tree": self.report["diagnostic_tree"], "files": files,
                  "baseline_paths": sorted(baseline), "diagnostic_paths": extra}
        recipe = [row(name) for name in sorted(trace.RECIPE_FILES)]
        lock_hash = hashlib.sha256(b"fixture").hexdigest()
        preparation = {"diagnostic_only": True, "ordinary_qualification": False,
                       "baseline_lock_sha256": lock_hash, "generated_lockfile_sha256": "f" * 64,
                       "cargo_config_sha256": "c" * 64, "recipe_files": manifest(recipe),
                       "helper": manifest([row("Cargo.toml"), row("src/lib.rs")]),
                       "dependencies": [{"name": name, "version": version, "archive_sha256": "e" * 64,
                           "patch_sha256": lock_hash, "original": manifest([row("Cargo.toml", b"original")]),
                           "patched": manifest([row("Cargo.toml", b"patched")])} for name, version in sorted(trace.PATCHED)]}
        dependencies = {"schema_version": 1, "identity": trace.IDENTITY, **trace.QUALIFICATION,
                        "preparation": preparation, "original_lock_sha256": lock_hash,
                        "final_lock_sha256": "f" * 64, "cargo_config_sha256": "c" * 64}
        package = {"schema_version": 1, "identity": trace.IDENTITY, **trace.QUALIFICATION,
                   "baseline_readiness": "blocked", "bundle_manifest_sha256": lock_hash,
                   "files": [row(name) for name in [*baseline[1:], "bundle-manifest.json", "third-party/dependency-inventory.json",
                              "commercial-readiness.json", "distribution-info.json"]]
                            + [{"path": "flightsim-app.exe", **trace.file_record(self.app)}]}
        for name, value in (("source-inputs", source), ("dependency-inputs", dependencies), ("package-inputs", package)):
            path = self.evidence / (name + ".json")
            trace.write_json(path, value)
            self.report[name.replace("-", "_") + "_sha256"] = trace.digest(path)

    def timeout(self):
        failure = subprocess.TimeoutExpired([str(self.app)], 180, output=b"partial stdout", stderr=event().encode())
        with patch.object(trace.subprocess, "run", side_effect=failure) as run:
            with self.assertRaises(subprocess.TimeoutExpired) as caught:
                trace.capture_once(self.run, ordinary, self.app, self.work, self.work, self.evidence, self.report)
            self.assertIs(caught.exception, failure)
        self.assertEqual(run.call_count, 1)
        args, kwargs = run.call_args
        self.assertEqual(args[0], ordinary.capture_command(self.app, self.work / "default-swift.png"))
        self.assertEqual(kwargs["timeout"], 180)
        self.assertEqual(kwargs["env"]["WGPU_BACKEND"], "dx12")
        self.assertEqual(kwargs["env"]["WGPU_FORCE_FALLBACK_ADAPTER"], "1")
        self.assertEqual(kwargs["env"]["RUST_LOG"], ordinary.CAPTURE_TRACE)
        self.assertNotIn("creationflags", kwargs)
        self.assertNotIn("startupinfo", kwargs)
        self.assertNotIn("--windows-readback-diagnostic", args[0])
        trace.seal_report(self.evidence, self.report)
        return run

    def test_timeout_preserves_partial_events_and_never_retries(self):
        self.timeout()
        self.assertEqual(self.report["scene_attempts"], 1)
        self.assertEqual(self.report["capture"]["status"], "timeout")
        self.assertIn("partial stdout", (self.evidence / "runtime.log").read_text())
        self.assertEqual(trace.read_json(self.evidence / "trace-events.json")["event_count"], 1)
        trace.validate_evidence(self.evidence)
        with self.assertRaisesRegex(ValueError, "reruns forbidden"):
            trace.capture_once(self.run, ordinary, self.app, self.work, self.work, self.evidence, self.report)
        self.assertFalse(list(self.evidence.glob("*.png")))

    def test_timeout_cannot_drop_its_source_dependency_or_package_bindings(self):
        self.timeout()
        for name in ("source-inputs.json", "dependency-inputs.json", "package-inputs.json"):
            path = self.evidence / name
            contents = path.read_bytes()
            path.unlink()
            trace.seal_report(self.evidence, self.report)
            with self.subTest(name=name), self.assertRaises(ValueError):
                trace.validate_evidence(self.evidence)
            path.write_bytes(contents)

    def test_stager_failure_does_not_claim_informational_process_started(self):
        report = {"informational_attempts": 0, "informational_invocations": 0}
        with patch.object(trace.subprocess, "run", side_effect=OSError("failed before staging")):
            with self.assertRaises(OSError):
                trace.stage_package(self.run, ordinary, self.repo, self.app, self.work, self.work / "notices", report)
        self.assertEqual(report["informational_attempts"], 1)
        self.assertIsNone(report["informational_invocations"])

    def test_resealed_false_capture_environment_count_and_trace_claims_rejected(self):
        self.timeout()
        for field, value in (("scene_attempts", 2), ("scene_attempts", True),
                             ("qualifies_release", True), ("release_authorized", 0),
                             ("baseline_commit", "b" * 40), ("target", "x86_64-unknown-linux-gnu"),
                             ("runtime_environment", {}), ("informational_invocations", 0)):
            changed = copy.deepcopy(self.report)
            changed[field] = value
            trace.seal_report(self.evidence, changed)
            with self.subTest(field=field), self.assertRaises(ValueError):
                trace.validate_evidence(self.evidence)
        for field, value in (("status", "captured_diagnostic_only"), ("timeout_seconds", 181), ("rust_log", "info"),
                             ("command", [str(self.app), "--headless-screenshot", "foo.png"])):
            changed = copy.deepcopy(self.report)
            changed["capture"][field] = value
            trace.seal_report(self.evidence, changed)
            with self.subTest(field=field), self.assertRaises(ValueError):
                trace.validate_evidence(self.evidence)
        changed = copy.deepcopy(self.report)
        projected = trace.read_json(self.evidence / "trace-events.json")
        projected["qualifies_release"] = 0
        trace.write_json(self.evidence / "trace-events.json", projected)
        changed["capture"]["trace_sha256"] = trace.digest(self.evidence / "trace-events.json")
        trace.seal_report(self.evidence, changed)
        with self.assertRaisesRegex(ValueError, "projection"):
            trace.validate_evidence(self.evidence)

    def test_malformed_partial_trace_cannot_hide_primary_timeout(self):
        failure = subprocess.TimeoutExpired("app", 180, stderr=b"FS_READBACK_TRACE malicious payload\n")
        with patch.object(trace.subprocess, "run", side_effect=failure):
            with self.assertRaises(subprocess.TimeoutExpired) as caught:
                trace.capture_once(self.run, ordinary, self.app, self.work, self.work, self.evidence, self.report)
        self.assertIs(caught.exception, failure)
        self.assertIn("trace_error", self.report["capture"])
        self.assertFalse((self.evidence / "trace-events.json").exists())
        trace.seal_report(self.evidence, self.report)
        trace.validate_evidence(self.evidence)

    def test_size_cutoff_preserves_whole_trace_lines_and_marks_truncation(self):
        text = event() + "long ordinary log\n" * 200
        with patch.object(trace, "MAX_FILE_BYTES", 200):
            trace.bounded_log(self.evidence / "runtime.log", text)
        result = trace.parse_trace((self.evidence / "runtime.log").read_text())
        self.assertEqual(result["event_count"], 1)
        self.assertTrue(result["runtime_log_truncated"])
        self.assertLessEqual((self.evidence / "runtime.log").stat().st_size, 200)


class EvidenceAndWorkflowTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.addCleanup(self.temporary.cleanup)
        (self.root / "commands.log").write_text("build failed\n")
        self.report = failed_report()
        trace.seal_report(self.root, self.report)

    def test_failed_before_build_evidence_is_valid(self):
        trace.validate_evidence(self.root)

    def test_unknown_files_directories_binary_json_duplicates_and_sizes_rejected(self):
        for name, contents in (("flight.png", b"PNG"), ("model.glb", b"glTF"), ("private.exe", b"MZ"),
                               ("commands.log", b"hello\0binary"), ("source-inputs.json", b'{"x":1,"x":2}')):
            path = self.root / name
            prior = path.read_bytes() if path.exists() else None
            path.write_bytes(contents)
            trace.seal_report(self.root, self.report)
            with self.subTest(name=name), self.assertRaises(ValueError):
                trace.validate_evidence(self.root)
            if prior is None:
                path.unlink()
            else:
                path.write_bytes(prior)
        extra = self.root / "assets"
        extra.mkdir()
        with self.assertRaises(ValueError):
            trace.validate_evidence(self.root)
        extra.rmdir()
        trace.seal_report(self.root, self.report)
        with patch.object(trace, "MAX_FILE_BYTES", 2), self.assertRaises(ValueError):
            trace.validate_evidence(self.root)

    def test_workflow_is_isolated_one_attempt_text_only_and_failure_upload_safe(self):
        workflow = (ROOT / ".github/workflows/windows-readback-trace.yml").read_text()
        for required in ("branches: [diagnostic/windows-readback-afff4a7c]", "contents: read", "github.run_attempt == 1",
                         "persist-credentials: false", "ref: ${{ github.sha }}", "fetch-depth: 0", "if: always()",
                         "steps.evidence.outputs.validated == 'true'", "timeout-minutes: 90", "--validate-evidence"):
            self.assertIn(required, workflow)
        for forbidden in ("workflow_dispatch:", "workflow_run:", "contents: write", "actions/cache", "rust-cache",
                          "--diagnose-readback", "gh release", "git tag", "continue-on-error:"):
            self.assertNotIn(forbidden, workflow)
        block = workflow.split("          path: |\n", 1)[1].split("          if-no-files-found:", 1)[0]
        names = {line.strip().rsplit("/", 1)[1] for line in block.splitlines() if line.strip()}
        self.assertEqual(names, trace.TEXT_EVIDENCE)
        self.assertNotIn("*", block)
        self.assertNotIn(".png", block)
        ordinary_workflow = (ROOT / ".github/workflows/swift-windows-candidate.yml").read_text()
        for action in (line.strip() for line in workflow.splitlines() if "uses:" in line):
            self.assertIn(action, ordinary_workflow)


if __name__ == "__main__":
    unittest.main()
