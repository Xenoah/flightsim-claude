#!/usr/bin/env python3
"""One nonqualifying Windows capture with byte-pinned dependency instrumentation.

All executable, model, LUT, archive and PNG bytes stay in the private work tree.
This runner never calls the ordinary candidate acceptance or release gates.
"""
from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import platform
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import sys
import tomllib
import zipfile

BASELINE = "afff4a7ccb35ce2f075ca6f404ffc2e664fceb4f"
BASELINE_TREE = "f1bdbb74f4546219c2e7eef2b6f9f9f99412dcd3"
IDENTITY = "windows-readback-dependency-trace-v1"
TARGET = "x86_64-pc-windows-msvc"
TOOLCHAIN = "1.93.0"
FEATURES = ["commercial-staging"]
CAPTURE_TRACE = ("info,wgpu_core::device::global=trace,wgpu_core::device::queue=trace,"
                 "wgpu_core::command::transfer=trace,wgpu_hal::dx12=debug,"
                 "bevy_app::task_pool_plugin=trace")
BRANCH = "diagnostic/windows-readback-afff4a7c"
HELPER = ("flightsim-readback-trace", "0.0.0")
PATCHED = {("bevy_render", "0.18.1"), ("wgpu-core", "27.0.3")}
NEW_FILES = {
    "scripts/run-windows-readback-trace.py",
    "scripts/tests/test_windows_readback_trace.py",
    ".github/workflows/windows-readback-trace.yml",
    "docs/qa/windows-readback-trace-2026-10-08.md",
}
RECIPE_FILES = {"prepare.py", "edits.json", "bevy_render.patch", "wgpu-core.patch",
                "events.json", "helper/Cargo.toml", "helper/src/lib.rs"}
TEXT_EVIDENCE = {
    "diagnostic-report.json", "source-inputs.json", "dependency-inputs.json",
    "package-inputs.json", "trace-events.json", "commands.log", "runtime.log",
}
MAX_FILE_BYTES = 32 * 1024 * 1024
MAX_TOTAL_BYTES = 64 * 1024 * 1024
TRUNCATION_MARKER = "[harness] log exceeded size bound; tail omitted"
MAX_EVENTS = 2048
EVENT_NAMES = frozenset("capture_prepare copy_encode task_spawn task_enter map_register_enter map_register_return "
    "bevy_callback_enter channel_send_enter channel_send_return receive_resumed mapped_range_enter "
    "mapped_range_return cpu_copy_return image_send_enter image_send_return main_receive event_queued "
    "buffer_selected core_map_enter core_map_return core_map_error callback_invoke callback_return "
    "dependency_assigned pending_promoted handle_map_enter handle_map_return buffer_map_enter map_state_waiting "
    "map_state_idle map_state_active backend_map_enter backend_map_return buffer_map_return fence_result "
    "maintain_return buffer_drop fence_enter".split())
QUALIFICATION = {
    "qualifies_ordinary_candidate": False, "qualifies_visual_acceptance": False,
    "qualifies_release": False, "release_authorized": False,
}
BASELINE_ADAPTER = {"name": "Microsoft Basic Render Driver", "vendor": 5140, "device": 140,
                    "device_type": "Cpu", "driver": "10.0.26100.33438", "driver_info": "", "backend": "Dx12"}
BASELINE_OBSERVATION = {"artifact_zip_sha256": "434712a7a03559186601ec264a4dc1a4fdc716cb0bc28b372c250a7756247317",
    "raw_log_sha256": "e878812a9eed4cb0a48085deda98a5b4b0618d7e49b8c036e47c0bd72c54a5ec",
    "image": "windows-2025-vs2026", "image_version": "20260925.250.1",
    "windows": "Windows Server 2025 Datacenter 10.0.26100", "runner_version": "2.337.0", "provisioner": "20260901.588"}
UINT = r"(0|[1-9][0-9]{0,19})"
EVENT = re.compile(r"FS_READBACK_TRACE schema=1 seq=" + UINT + r" us=" + UINT
                   + r" event=([a-z][a-z0-9_]{0,63}) a=" + UINT + r" b=" + UINT + r" c=" + UINT)
STATUS = re.compile(r"FS_READBACK_TRACE_STATUS schema=1 reserved=" + UINT + r" drained=" + UINT
                    + r" dropped=" + UINT + r" io_errors=" + UINT + r" capacity=2048")
ANSI = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")
ADAPTER = re.compile(r'AdapterInfo \{ name: ("(?:[^"\\]|\\.)*"), vendor: ([0-9]+), device: ([0-9]+), '
                     r'device_type: ([A-Za-z]+), driver: ("(?:[^"\\]|\\.)*"), driver_info: ("(?:[^"\\]|\\.)*"), backend: ([A-Za-z0-9]+) \}')


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def file_record(path):
    return {"bytes": path.stat().st_size, "sha256": digest(path)}


def hex_value(value, length):
    return isinstance(value, str) and re.fullmatch(r"[0-9a-f]{" + str(length) + r"}", value) is not None


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + "\n", encoding="utf-8")


def unique_pairs(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON key")
        result[key] = value
    return result


def read_json(path):
    require(path.stat().st_size <= MAX_FILE_BYTES, "JSON exceeds size bound")
    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_pairs,
                      parse_constant=lambda value: (_ for _ in ()).throw(ValueError("non-finite JSON")))


def safe_relative(relative):
    require(isinstance(relative, str) and relative and "\\" not in relative and ":" not in relative,
            "unsafe relative path")
    path = PurePosixPath(relative)
    require(not path.is_absolute() and str(path) == relative and all(p not in (".", "..") for p in path.parts),
            "unsafe relative path")
    return relative


def regular_file(root, relative):
    safe_relative(relative)
    path = root
    for part in PurePosixPath(relative).parts:
        path = path / part
        require(not path.is_symlink(), "symlink input: " + relative)
    require(path.is_file(), "missing regular input: " + relative)
    return path


def git(repo, *args):
    return subprocess.check_output(["git", *args], cwd=repo).decode("utf-8").strip()


def git_files(repo, revision):
    data = subprocess.check_output(["git", "ls-tree", "-r", "-z", revision], cwd=repo).decode("utf-8")
    records = {}
    for entry in data.split("\0"):
        if not entry:
            continue
        metadata, relative = entry.split("\t", 1)
        mode, kind, blob = metadata.split()
        safe_relative(relative)
        require(kind == "blob" and mode in ("100644", "100755"), "non-regular Git input")
        records[relative] = {"git_mode": mode, "git_blob": blob}
    return records


def verify_checkout_file(repo, relative, record):
    path = regular_file(repo, relative)
    data = path.read_bytes()
    git_hash = hashlib.sha1(b"blob " + str(len(data)).encode("ascii") + b"\0" + data).hexdigest()
    require(git_hash == record["git_blob"], "checkout differs from canonical Git bytes: " + relative)
    return {"path": relative, **record, **file_record(path)}


def source_inputs(repo, expected):
    require(hex_value(expected, 40), "expected diagnostic source must be a full SHA")
    require(git(repo, "rev-parse", "HEAD") == expected, "wrong diagnostic checkout")
    require(git(repo, "rev-parse", "--show-object-format") == "sha1", "unexpected Git object format")
    require(not git(repo, "status", "--porcelain", "--untracked-files=all"), "diagnostic checkout must be clean")
    require(git(repo, "rev-parse", BASELINE + "^{tree}") == BASELINE_TREE, "ordinary baseline tree changed")
    baseline = git_files(repo, BASELINE)
    current = git_files(repo, "HEAD")
    require(all(current.get(path) == record for path, record in baseline.items()),
            "ordinary source changed, removed or renamed")
    extra = set(current) - set(baseline)
    require(NEW_FILES <= extra and all(path in NEW_FILES or path.startswith("diagnostics/windows-readback/")
                                      for path in extra), "unapproved diagnostic source additions")
    records = [verify_checkout_file(repo, path, current[path]) for path in sorted(current)]
    return {"schema_version": 1, "identity": IDENTITY, "baseline_commit": BASELINE,
            "baseline_tree": BASELINE_TREE, "diagnostic_commit": expected,
            "diagnostic_tree": git(repo, "rev-parse", "HEAD^{tree}"),
            "baseline_paths": sorted(baseline), "diagnostic_paths": sorted(extra), "files": records}


def tree_identity(records):
    root = {}
    for record in records:
        parts = safe_relative(record["path"]).split("/")
        current = root
        for part in parts[:-1]:
            require(not isinstance(current.get(part), tuple), "source file/directory collision")
            current = current.setdefault(part, {})
        require(parts[-1] not in current, "duplicate source path")
        require(record["git_mode"] in ("100644", "100755") and hex_value(record["git_blob"], 40), "invalid Git source identity")
        current[parts[-1]] = (record["git_mode"], record["git_blob"])

    def encode_tree(tree):
        entries = []
        for name, value in tree.items():
            if isinstance(value, dict):
                mode, blob = "40000", encode_tree(value)
                sort_name = name + "/"
            else:
                mode, blob = value
                sort_name = name
            entries.append((sort_name.encode("utf-8"), mode.encode() + b" " + name.encode("utf-8") + b"\0" + bytes.fromhex(blob)))
        data = b"".join(value for _, value in sorted(entries))
        return hashlib.sha1(b"tree " + str(len(data)).encode() + b"\0" + data).hexdigest()

    return encode_tree(root)


def validate_source_manifest(source, report):
    require(source.get("schema_version") == 1 and source.get("identity") == IDENTITY, "invalid source manifest")
    require(source.get("diagnostic_commit") == report["diagnostic_commit"]
            and source.get("diagnostic_tree") == report.get("diagnostic_tree")
            and source.get("baseline_commit") == BASELINE and source.get("baseline_tree") == BASELINE_TREE,
            "source identity changed")
    files = source["files"]
    baseline, extra = source["baseline_paths"], source["diagnostic_paths"]
    require(isinstance(files, list) and isinstance(baseline, list) and isinstance(extra, list), "invalid source lists")
    require(baseline == sorted(set(baseline)) and extra == sorted(set(extra)) and not set(baseline) & set(extra), "duplicate/overlapping source paths")
    require({row["path"] for row in files} == set(baseline) | set(extra)
            and NEW_FILES <= set(extra) and all(path in NEW_FILES or path.startswith("diagnostics/windows-readback/") for path in extra),
            "source membership changed")
    for record in files:
        require(set(record) == {"path", "git_mode", "git_blob", "bytes", "sha256"}
                and type(record["bytes"]) is int and record["bytes"] >= 0 and hex_value(record["sha256"], 64), "invalid source file record")
    require(tree_identity([row for row in files if row["path"] in set(baseline)]) == BASELINE_TREE,
            "source manifest does not reconstruct the ordinary baseline tree")
    require(tree_identity(files) == report["diagnostic_tree"], "source manifest does not reconstruct diagnostic tree")


def directory_manifest(root):
    files = []
    for path in sorted(root.rglob("*"), key=lambda item: item.relative_to(root).as_posix()):
        require(not path.is_symlink(), "dependency symlinks forbidden")
        if path.is_file():
            files.append({"path": path.relative_to(root).as_posix(), **file_record(path)})
    encoded = json.dumps(files, sort_keys=True, separators=(",", ":")).encode()
    return {"files": files, "tree_sha256": hashlib.sha256(encoded).hexdigest()}


def verify_prepared_bytes(preparation, work):
    for dependency in preparation["dependencies"]:
        path = work / "dependencies" / (dependency["name"] + "-" + dependency["version"])
        require(directory_manifest(path) == dependency["patched"], "generated dependency bytes changed")
    require(directory_manifest(work / "dependencies" / HELPER[0]) == preparation["helper"], "helper bytes changed")


def copy_baseline(repo, source, output):
    require(not output.exists(), "private source copy already exists")
    output.mkdir()
    for relative in source["baseline_paths"]:
        path = regular_file(repo, relative)
        target = output / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, target)


def verify_private_source(private, source, lock_hash):
    expected = {row["path"]: row for row in source["files"] if row["path"] in source["baseline_paths"]}
    actual = {path.relative_to(private).as_posix() for path in private.rglob("*") if path.is_file()}
    require(actual == set(expected), "private source membership changed")
    for relative, record in expected.items():
        path = regular_file(private, relative)
        require(digest(path) == (lock_hash if relative == "Cargo.lock" else record["sha256"]),
                "private ordinary source changed: " + relative)


def load_module(path, name):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def validate_lock_delta(before, after):
    old_document, new_document = tomllib.loads(before), tomllib.loads(after)
    require({key: value for key, value in old_document.items() if key != "package"}
            == {key: value for key, value in new_document.items() if key != "package"}, "lock format/metadata changed")
    def index(data):
        result = {}
        for package in tomllib.loads(data)["package"]:
            key = (package["name"], package["version"])
            require(key not in result, "ambiguous lock package name/version")
            result[key] = package
        return result
    old, new = index(before), index(after)
    require(set(new) == set(old) | {HELPER} and HELPER not in old, "diagnostic lock changed package versions/set")
    require(new[HELPER] == {"name": HELPER[0], "version": HELPER[1]}, "helper must have no dependencies/source")
    for key, package in old.items():
        expected = dict(package)
        if key in PATCHED:
            require(expected.pop("source", "") == "registry+https://github.com/rust-lang/crates.io-index",
                    "patched package was not the pinned registry input")
            require(hex_value(expected.pop("checksum", None), 64), "missing pinned archive checksum")
            expected["dependencies"] = sorted([*expected.get("dependencies", []), HELPER[0]])
        actual = dict(new[key])
        if "dependencies" in actual:
            actual["dependencies"] = sorted(actual["dependencies"])
        if "dependencies" in expected:
            expected["dependencies"] = sorted(expected["dependencies"])
        require(actual == expected, "unapproved diagnostic lock delta: " + key[0])


def metadata_identity(metadata):
    packages = {p["id"]: p for p in metadata["packages"]}
    result = {}
    for node in metadata["resolve"]["nodes"]:
        package = packages[node["id"]]
        key = (package["name"], package["version"])
        require(key not in result, "ambiguous metadata package")
        result[key] = {
            "features": sorted(node["features"]),
            "dependencies": sorted((packages[dep]["name"], packages[dep]["version"])
                                   for dep in node["dependencies"]),
        }
    return result


def validate_metadata_delta(before, after):
    old, new = metadata_identity(before), metadata_identity(after)
    require(set(new) == set(old) | {HELPER}, "resolved package set changed")
    require(new[HELPER] == {"features": [], "dependencies": []}, "unexpected helper graph")
    for key, record in old.items():
        expected = {**record, "dependencies": list(record["dependencies"])}
        if key in PATCHED:
            expected["dependencies"] = sorted([*expected["dependencies"], HELPER])
        require(new[key] == expected, "resolved features/dependencies changed: " + key[0])
    apps = [value for key, value in new.items() if key[0] == "flightsim-app"]
    require(len(apps) == 1 and apps[0]["features"] == sorted(["default", *FEATURES]), "wrong app features/defaults")
    pipelines = [value for key, value in new.items() if key[0] == "bevy_core_pipeline"]
    require(len(pipelines) == 1 and "tonemapping_luts" in pipelines[0]["features"], "full LUT bundle must remain enabled")
    return [{"name": key[0], "version": key[1], **record} for key, record in sorted(new.items())]


def clean_log(data, repo, work):
    text = data.decode("utf-8", errors="replace") if isinstance(data, bytes) else str(data)
    text = ANSI.sub("", text).replace("\\\\?\\", "")
    # Keep only printable text and line/tab controls in the exported log.
    text = "".join(char if char.isprintable() or char in "\n\r\t" else "\ufffd" for char in text)
    for path, label in ((work, "<private-work>"), (repo, "<source>")):
        value = str(path)
        for variant in (repr(value)[1:-1], value, value.replace("\\", "/")):
            text = text.replace(variant, label)
    return text.replace("\\", "/")


def sanitize_value(value, repo, work):
    if isinstance(value, dict):
        return {key: sanitize_value(item, repo, work) for key, item in value.items()}
    if isinstance(value, list):
        return [sanitize_value(item, repo, work) for item in value]
    return clean_log(value, repo, work) if isinstance(value, str) else value


def bounded_log(path, text):
    data = text.encode("utf-8")
    # Preserve the beginning containing sparse trace events; truncation is explicit.
    if len(data) > MAX_FILE_BYTES:
        marker = ("\n" + TRUNCATION_MARKER + "\n").encode()
        prefix = data[:MAX_FILE_BYTES - len(marker) - 4]
        boundary = prefix.rfind(b"\n")
        text = prefix[:boundary + 1].decode("utf-8", errors="strict") + marker.decode()
    path.write_text(text, encoding="utf-8")


class Commands:
    def __init__(self, repo, work, evidence, env):
        self.repo, self.work, self.evidence, self.env = repo, work, evidence, env
        self.records = []
        (evidence / "commands.log").write_text("", encoding="utf-8")

    def __call__(self, command, *, cwd=None, timeout=3600, runtime=False, output=None, accepted=(0,)):
        env = self.env.copy()
        if runtime:
            env.update(WGPU_BACKEND="dx12", WGPU_FORCE_FALLBACK_ADAPTER="1",
                       BEVY_ASSET_ROOT=str(self.repo), CARGO_MANIFEST_DIR=str(self.repo))
        if "--screenshot" in [str(arg) for arg in command]:
            env["RUST_LOG"] = self.env["TRACE_RUST_LOG"]
        env.pop("TRACE_RUST_LOG", None)
        error = None
        try:
            result = subprocess.run([str(arg) for arg in command], cwd=cwd or self.repo,
                                    env=env, capture_output=True, timeout=timeout)
        except subprocess.TimeoutExpired as caught:
            error = caught
            result = subprocess.CompletedProcess(command, -1, caught.stdout or b"", caught.stderr or b"")
        except OSError as caught:
            error = caught
            result = subprocess.CompletedProcess(command, -1, b"", str(caught).encode("utf-8"))
        log = clean_log(result.stdout + b"\n" + result.stderr, self.repo, self.work)
        if output:
            bounded_log(output, log)
        transcript = self.evidence / "commands.log"
        prior = transcript.read_text(encoding="utf-8")
        record = clean_log(json.dumps([str(arg) for arg in command]), self.repo, self.work)
        suffix = record + f"\nexit_code={result.returncode}\n"
        if output:
            suffix += f"log={output.name} sha256={digest(output)}\n"
        elif "metadata" not in command:
            suffix += log + "\n"
        bounded_log(transcript, prior + suffix)
        self.records.append({"command": [clean_log(str(arg), self.repo, self.work) for arg in command],
                             "exit_code": result.returncode, "timeout_seconds": timeout,
                             "timed_out": isinstance(error, subprocess.TimeoutExpired), "runtime": runtime})
        if error:
            raise error
        require(accepted is None or result.returncode in accepted, "command failed: " + str(command[0]))
        return result, log


def stage_package(run, ordinary, repo, executable, work, notices, report):
    """Preserve ordinary inert package inputs using the unchanged pinned stager.

    Its baseline inventory deliberately does not qualify the instrumented binary.
    The only executable call inside the stager is the non-rendering handshake.
    """
    staged = work / "swift-candidate"
    report["informational_attempts"] += 1
    report["informational_invocations"] = None  # Unknown until the stager's handshake is proven.
    result, _ = run([sys.executable, "-B", repo / "scripts/stage-commercial-candidate.py", "--source-root", repo,
                     "--executable", executable, "--dependency-notices", notices, "--output", staged], accepted=(0, 1))
    readiness = read_json(staged / "commercial-readiness.json")
    ordinary.validate_readiness(readiness, 2 if result.returncode == 1 else 0)
    require(readiness["status"] == "blocked", "baseline readiness must remain blocked")
    manifest = read_json(staged / "bundle-manifest.json")
    ordinary.validate_distribution(manifest["distribution"], read_json(staged / "distribution-info.json"))
    report["informational_invocations"] = 1
    report["distribution"] = manifest["distribution"]
    manifest_hash = digest(staged / "bundle-manifest.json")
    ordinary.verify_bundle(staged, executable, manifest_hash)
    files = [*manifest["files"], {"path": "bundle-manifest.json", **file_record(staged / "bundle-manifest.json")}]
    files.sort(key=lambda record: record["path"])
    names = [record["path"] for record in files]
    require({name for name in names if name.startswith("assets/")} == {
        "assets/aircraft/swift_sport.glb", "assets/aircraft/swift_sport.json"}, "runtime assets are not ordinary Swift-only")
    archive = work / "swift-candidate.zip"
    with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as output:
        for name in sorted(names):
            output.write(staged / name, "swift-candidate/" + name)
    extracted = work / "extracted"
    with zipfile.ZipFile(archive) as source:
        require(set(source.namelist()) == {"swift-candidate/" + name for name in names}, "archive membership changed")
        source.extractall(extracted)
    bundle = extracted / "swift-candidate"
    verify_package(bundle, files)
    ordinary.verify_bundle(bundle, executable, manifest_hash)
    return bundle, {"schema_version": 1, "identity": IDENTITY, **QUALIFICATION,
                    "files": files, "bundle_manifest_sha256": manifest_hash,
                    "private_archive_sha256": digest(archive),
                    "baseline_inventory_sha256": digest(notices / "dependency-inventory.json"),
                    "baseline_readiness": readiness["status"],
                    "derived_metadata_changes": ["bundle-manifest.json records the instrumented executable's actual hash"],
                    "notice": "Ordinary package path set and inert data/notices preserved; baseline inventory and readiness do not qualify the instrumented executable or diagnostic helper"}


def verify_package(bundle, files):
    expected = {record["path"]: record for record in files}
    actual = {path.relative_to(bundle).as_posix() for path in bundle.rglob("*") if path.is_file()}
    require(actual == set(expected), "extracted package membership changed")
    for relative, record in expected.items():
        require(file_record(regular_file(bundle, relative)) == {key: record[key] for key in ("bytes", "sha256")},
                "extracted package bytes changed: " + relative)


def parse_trace(log):
    """Strict wire projection. Missing observations stay missing, including timeout."""
    events, statuses = [], []
    for line in log.splitlines():
        if "FS_READBACK_TRACE" not in line:
            continue
        if line.startswith("FS_READBACK_TRACE_STATUS"):
            match = STATUS.fullmatch(line)
            require(match is not None, "malformed trace status")
            values = [int(value) for value in match.groups()]
            require(all(value <= 2**64 - 1 for value in values), "trace integer overflow")
            status = dict(zip(("reserved", "drained", "dropped", "io_errors"), values))
            require(status["drained"] <= min(MAX_EVENTS, status["reserved"])
                    and status["dropped"] <= status["reserved"], "invalid trace status counts")
            require(not statuses or all(status[key] >= statuses[-1][key] for key in status), "trace status counter regressed")
            statuses.append(status)
            require(len(statuses) <= 16384, "too many trace status records")
        else:
            match = EVENT.fullmatch(line)
            require(match is not None, "malformed trace event")
            seq, us, event, a, b, c = match.groups()
            values = [int(value) for value in (seq, us, a, b, c)]
            require(all(value <= 2**64 - 1 for value in values), "trace integer overflow")
            require(event in EVENT_NAMES, "unknown trace event")
            require(values[0] < MAX_EVENTS and (not events or values[0] > events[-1]["seq"]),
                    "trace sequence duplicate, reversed or out of bounds")
            events.append(dict(zip(("seq", "us", "a", "b", "c"), values), event=event))
    return {"schema_version": 1, "identity": IDENTITY, **QUALIFICATION,
            "events": events, "statuses": statuses, "event_count": len(events),
            "missing_sequence_count": events[-1]["seq"] + 1 - len(events) if events else 0,
            "runtime_log_truncated": TRUNCATION_MARKER in log,
            "last_observed_event": events[-1]["event"] if events else None,
            "complete_trace_claimed": False}


def adapter_observation(log):
    lines = [line for line in log.splitlines() if "AdapterInfo" in line]
    observations = []
    for line in lines:
        match = ADAPTER.search(line)
        if match is None:
            continue
        name, vendor, device, device_type, driver, driver_info, backend = match.groups()
        observations.append({"name": json.loads(name), "vendor": int(vendor), "device": int(device),
                             "device_type": device_type, "driver": json.loads(driver),
                             "driver_info": json.loads(driver_info), "backend": backend})
    actual = observations[0] if len(observations) == 1 else None
    outcome = ("matches_baseline" if actual == BASELINE_ADAPTER else "mismatch") if actual else (
        "unobserved" if not lines else "unparsed_or_ambiguous")
    return {"expected": BASELINE_ADAPTER, "actual": actual, "status": outcome, "observed_lines": lines}


def capture_once(run, ordinary, app, cwd, work, evidence, report):
    require(report["scene_attempts"] == 0, "scene launch already attempted; reruns forbidden")
    require(digest(app) == report["executable_sha256"], "executable changed before scene")
    command = ordinary.capture_command(app, work / "default-swift.png")
    report["scene_attempts"] = 1  # Set before invoking: a failed spawn cannot earn a retry.
    report["capture"] = {"command": [clean_log(str(arg), run.repo, work) for arg in command],
                         "timeout_seconds": 180, "launch": ordinary.BASELINE_LAUNCH,
                         "rust_log": ordinary.CAPTURE_TRACE, "status": "failed"}
    try:
        result, log = run(command, cwd=cwd, timeout=180, runtime=True,
                          output=evidence / "runtime.log", accepted=None)
        report["capture"]["exit_code"] = result.returncode
        ordinary.validate_smoke(log, result.returncode, model=True)
        report["capture"]["private_png"] = ordinary.validate_png(work / "default-swift.png")
        report["capture"]["status"] = "captured_diagnostic_only"
    except subprocess.TimeoutExpired:
        report["capture"]["status"] = "timeout"
        raise
    finally:
        require(digest(app) == report["executable_sha256"], "executable changed during scene")
        path = evidence / "runtime.log"
        if path.exists():
            report["capture"]["log_sha256"] = digest(path)
            report["capture"]["runtime_log_truncated"] = TRUNCATION_MARKER in path.read_text(encoding="utf-8")
            report["capture"]["adapter"] = adapter_observation(path.read_text(encoding="utf-8"))
            try:
                trace = parse_trace(path.read_text(encoding="utf-8"))
                write_json(evidence / "trace-events.json", trace)
                report["capture"]["trace_sha256"] = digest(evidence / "trace-events.json")
            except ValueError as error:
                report["capture"]["trace_error"] = str(error)


def seal_report(evidence, report):
    report["evidence_files"] = {path.name: file_record(path) for path in sorted(evidence.iterdir())
                                if path.is_file() and path.name != "diagnostic-report.json"}
    write_json(evidence / "diagnostic-report.json", report)


def validate_hashed_manifest(manifest):
    require(isinstance(manifest, dict) and set(manifest) == {"files", "tree_sha256"}, "invalid dependency manifest")
    records = manifest["files"]
    require(isinstance(records, list) and records and len(records) <= 10000, "invalid dependency file list")
    seen = set()
    for record in records:
        require(set(record) == {"path", "bytes", "sha256"} and type(record["bytes"]) is int
                and record["bytes"] >= 0 and hex_value(record["sha256"], 64), "invalid dependency file record")
        path = safe_relative(record["path"])
        require(path not in seen, "duplicate dependency file")
        seen.add(path)
    encoded = json.dumps(records, sort_keys=True, separators=(",", ":")).encode()
    require(hashlib.sha256(encoded).hexdigest() == manifest["tree_sha256"], "dependency manifest hash changed")


def validate_dependency_manifest(dependencies, source, report):
    require(dependencies.get("identity") == IDENTITY and dependencies.get("schema_version") == 1
            and all(dependencies.get(key) is False for key in QUALIFICATION), "invalid dependency evidence identity")
    preparation = dependencies["preparation"]
    require(preparation.get("diagnostic_only") is True and preparation.get("ordinary_qualification") is False,
            "preparation cannot grant ordinary qualification")
    source_by_path = {row["path"]: row for row in source["files"]}
    require(dependencies.get("original_lock_sha256") == source_by_path["Cargo.lock"]["sha256"]
            == preparation.get("baseline_lock_sha256"), "original lock identity changed")
    require(dependencies.get("final_lock_sha256") == report.get("final_lock_sha256")
            == preparation.get("generated_lockfile_sha256") and hex_value(report.get("final_lock_sha256"), 64),
            "final diagnostic lock identity changed")
    require(dependencies.get("cargo_config_sha256") == preparation.get("cargo_config_sha256")
            and hex_value(dependencies.get("cargo_config_sha256"), 64), "patch config identity changed")
    validate_hashed_manifest(preparation["recipe_files"])
    prefix = "diagnostics/windows-readback/"
    expected_recipe = [{"path": row["path"][len(prefix):], "bytes": row["bytes"], "sha256": row["sha256"]}
                       for row in source["files"] if row["path"].startswith(prefix)
                       and row["path"][len(prefix):] in RECIPE_FILES]
    require(preparation["recipe_files"]["files"] == expected_recipe, "preparation recipe differs from committed diagnostic files")
    validate_hashed_manifest(preparation["helper"])
    expected_helper = [{**row, "path": row["path"][len("helper/"):]} for row in expected_recipe
                       if row["path"] in ("helper/Cargo.toml", "helper/src/lib.rs")]
    require(preparation["helper"]["files"] == expected_helper, "generated helper differs from committed source")
    require({(row["name"], row["version"]) for row in preparation["dependencies"]} == PATCHED
            and len(preparation["dependencies"]) == len(PATCHED), "patched package identity changed")
    for dependency in preparation["dependencies"]:
        require(hex_value(dependency.get("archive_sha256"), 64), "missing pinned archive hash")
        require(dependency.get("patch_sha256") == source_by_path[prefix + dependency["name"] + ".patch"]["sha256"],
                "patch source hash changed")
        validate_hashed_manifest(dependency["original"])
        validate_hashed_manifest(dependency["patched"])


def validate_package_manifest(package, source, report):
    require(package.get("identity") == IDENTITY and package.get("schema_version") == 1
            and all(package.get(key) is False for key in QUALIFICATION), "invalid package evidence identity")
    require(package.get("baseline_readiness") == "blocked", "baseline readiness unexpectedly qualified")
    source_by_path = {row["path"]: row for row in source["files"]}
    seen = set()
    for record in package["files"]:
        name = safe_relative(record["path"])
        require(name not in seen and set(record) == {"path", "bytes", "sha256"}
                and type(record["bytes"]) is int and record["bytes"] >= 0 and hex_value(record["sha256"], 64),
                "invalid package file record")
        seen.add(name)
        if name in source_by_path:
            require(record["sha256"] == source_by_path[name]["sha256"] and record["bytes"] == source_by_path[name]["bytes"],
                    "ordinary staged input bytes changed")
        if name == "flightsim-app.exe":
            require(record["sha256"] == report["executable_sha256"], "package executable differs from diagnostic build")
        if name == "bundle-manifest.json":
            require(record["sha256"] == package["bundle_manifest_sha256"], "package manifest hash changed")
    require({"flightsim-app.exe", "bundle-manifest.json", "third-party/dependency-inventory.json",
             "commercial-readiness.json", "distribution-info.json"} <= seen, "incomplete ordinary package layout")
    require({name for name in seen if name.startswith("assets/")} == {
        "assets/aircraft/swift_sport.glb", "assets/aircraft/swift_sport.json"}, "package assets changed")


def validate_evidence(evidence):
    require(evidence.is_dir() and not evidence.is_symlink(), "missing regular evidence directory")
    total = 0
    for path in evidence.iterdir():
        require(path.name in TEXT_EVIDENCE, "unapproved evidence path: " + path.name)
        require(path.is_file() and not path.is_symlink(), "evidence must contain only regular files")
        size = path.stat().st_size
        require(size <= MAX_FILE_BYTES, "evidence file exceeds size bound")
        total += size
        text = path.read_bytes().decode("utf-8")
        require(all(char.isprintable() or char in "\n\r\t" for char in text), "binary/control bytes in text evidence")
        if path.suffix == ".json":
            require(isinstance(read_json(path), dict), "JSON evidence must be an object")
    require(total <= MAX_TOTAL_BYTES, "total evidence exceeds size bound")
    report = read_json(evidence / "diagnostic-report.json")
    require(report.get("schema_version") == 1 and type(report["schema_version"]) is int
            and report.get("identity") == IDENTITY, "wrong diagnostic evidence identity")
    require(all(report.get(key) is False for key in QUALIFICATION), "diagnostic cannot qualify ordinary/release gates")
    require(report.get("baseline_commit") == BASELINE and report.get("baseline_tree") == BASELINE_TREE,
            "ordinary baseline identity changed")
    require(report.get("baseline_observation") == BASELINE_OBSERVATION, "baseline observation identity changed")
    require(hex_value(report.get("diagnostic_commit"), 40), "missing full diagnostic commit")
    require(report.get("target") == TARGET and report.get("toolchain") == TOOLCHAIN
            and report.get("features") == FEATURES and report.get("default_features") is True
            and report.get("profile") == "release", "diagnostic build recipe changed")
    require(type(report.get("scene_attempts")) is int and report["scene_attempts"] in (0, 1), "invalid scene launch count")
    require(type(report.get("informational_attempts")) is int and report["informational_attempts"] in (0, 1),
            "invalid informational attempt count")
    require((type(report.get("informational_invocations")) is int and report["informational_invocations"] in (0, 1))
            or (report.get("informational_invocations") is None and report["informational_attempts"] == 1
                and report["scene_attempts"] == 0 and report["status"] == "failed"),
            "invalid informational invocation count")
    require(report.get("status") in ("failed", "diagnostic_capture_completed"), "unrecognized diagnostic outcome")
    if report["status"] == "failed":
        require(isinstance(report.get("failure"), str) and report["failure"], "failed diagnostic lacks reason")
    hashes = report.get("evidence_files")
    actual = {path.name for path in evidence.iterdir()} - {"diagnostic-report.json"}
    require(isinstance(hashes, dict) and set(hashes) == actual, "evidence file set changed")
    for name, expected in hashes.items():
        require(file_record(evidence / name) == expected, "evidence bytes changed: " + name)
    source_path = evidence / "source-inputs.json"
    if source_path.exists():
        source = read_json(source_path)
        require(report.get("source_inputs_sha256") == digest(source_path), "source binding changed")
        validate_source_manifest(source, report)
    for name, validator in (("dependency-inputs.json", validate_dependency_manifest), ("package-inputs.json", validate_package_manifest)):
        if (evidence / name).exists():
            require(source_path.exists(), "derived inputs lack source identity")
            require(report.get(name.replace("-", "_").replace(".json", "_sha256")) == digest(evidence / name), "derived inputs binding changed")
            validator(read_json(evidence / name), source, report)
    capture = report.get("capture")
    if report["scene_attempts"]:
        require({"source-inputs.json", "dependency-inputs.json", "package-inputs.json"} <= actual,
                "scene observation lacks bound source/dependency/package inputs")
        require(isinstance(capture, dict) and capture.get("timeout_seconds") == 180
                and type(capture["timeout_seconds"]) is int
                and capture.get("launch") == {"creationflags": 0, "startupinfo": None}
                and type(capture["launch"]["creationflags"]) is int, "capture launcher/watchdog changed")
        require(capture.get("command") == ["<private-work>/extracted/swift-candidate/flightsim-app.exe",
                "--screenshot", "<private-work>/default-swift.png", "--screenshot-delay", "5",
                "--exit-after-screenshot", "--view", "chase"], "ordinary scene command changed")
        require(hex_value(report.get("executable_sha256"), 64), "missing executable identity")
        require(report["informational_attempts"] == report["informational_invocations"] == 1, "scene lacks its single distribution handshake")
        require(report.get("runtime_environment") == {"WGPU_BACKEND": "dx12", "WGPU_FORCE_FALLBACK_ADAPTER": "1",
                "BEVY_ASSET_ROOT": "<source>", "CARGO_MANIFEST_DIR": "<source>", "RUST_LOG": CAPTURE_TRACE}
                and capture.get("rust_log") == CAPTURE_TRACE, "runtime environment/logging differs from ordinary recipe")
        require(report.get("compiler_flags") == {"RUSTFLAGS": "-D warnings", "CARGO_TARGET_DIR": "<private-work>/target"},
                "compiler flags changed")
        expected_build = ["cargo", "+1.93.0", "build", "--locked", "--release", "-j", "2", "--target", TARGET,
                          "-p", "flightsim-app", "--features", "commercial-staging", "--config",
                          "<private-work>/dependencies/patch-config.toml"]
        require(report.get("build_command") == expected_build, "diagnostic build command differs from ordinary recipe plus private patch config")
        require(capture.get("status") in ("failed", "timeout", "captured_diagnostic_only"), "invalid capture outcome")
        commands = report.get("commands", [])
        require(sum(row.get("command") == expected_build and row.get("exit_code") == 0 for row in commands) == 1,
                "missing unique successful diagnostic build")
        stager_calls = [row for row in commands if "<source>/scripts/stage-commercial-candidate.py" in row.get("command", [])]
        require(len(stager_calls) == 1 and stager_calls[0].get("exit_code") in (0, 1), "missing unique informational staging invocation")
        executable_calls = [row for row in commands if row.get("command", [""])[0].endswith("flightsim-app.exe")]
        scene_calls = [row for row in commands if "--screenshot" in row.get("command", [])]
        require(executable_calls == scene_calls and len(scene_calls) == 1 and scene_calls[0]["command"] == capture["command"]
                and scene_calls[0].get("timeout_seconds") == 180 and scene_calls[0].get("runtime") is True,
                "commands do not prove exactly one ordinary scene invocation")
        require(scene_calls[0].get("timed_out") is (capture["status"] == "timeout"), "timeout outcome changed")
        if capture["status"] != "timeout":
            require(capture.get("exit_code", -1) == scene_calls[0].get("exit_code"), "capture exit outcome changed")
        require(sum("--screenshot" in line and line.startswith("[") for line in (evidence / "commands.log").read_text(encoding="utf-8").splitlines()) == 1,
                "command transcript has additional/missing screenshot invocation")
        require(capture.get("log_sha256") == digest(evidence / "runtime.log"), "runtime log binding changed")
        require(capture.get("runtime_log_truncated") is (TRUNCATION_MARKER in (evidence / "runtime.log").read_text(encoding="utf-8")),
                "runtime log truncation claim changed")
        require(capture.get("adapter") == adapter_observation((evidence / "runtime.log").read_text(encoding="utf-8")),
                "adapter observation differs from runtime log")
        if (evidence / "trace-events.json").exists():
            require(capture.get("trace_sha256") == digest(evidence / "trace-events.json"), "trace binding changed")
            require(json.dumps(read_json(evidence / "trace-events.json"), sort_keys=True) == json.dumps(parse_trace((evidence / "runtime.log").read_text(encoding="utf-8")), sort_keys=True),
                    "trace projection changed")
        else:
            require(isinstance(capture.get("trace_error"), str) and capture["trace_error"], "missing trace projection")
    else:
        require(capture is None and not {"runtime.log", "trace-events.json"} & actual, "runtime evidence without launch")
    if report["status"] == "diagnostic_capture_completed":
        require(report.get("post_run_integrity") == "verified_for_available_inputs" and "integrity_failure" not in report,
                "completed diagnostic lacks successful final input verification")
        require(set(TEXT_EVIDENCE) - {"diagnostic-report.json"} <= actual and report["scene_attempts"] == 1
                and capture.get("status") == "captured_diagnostic_only" and capture.get("exit_code") == 0
                and "trace_error" not in capture, "completed diagnostic lacks successful bound evidence")
        ordinary = load_module(Path(__file__).resolve().with_name("check-swift-windows-candidate.py"), "ordinary_evidence_validation")
        ordinary.validate_smoke((evidence / "runtime.log").read_text(encoding="utf-8"), capture["exit_code"], model=True)
        require(parse_trace((evidence / "runtime.log").read_text(encoding="utf-8"))["event_count"] > 0,
                "completed diagnostic has no instrumentation events")
        require(capture["runtime_log_truncated"] is False, "truncated runtime cannot be a completed diagnostic")
        require(capture["adapter"]["status"] == "matches_baseline", "completed diagnostic adapter differs from baseline")
        png = capture.get("private_png")
        require(isinstance(png, dict) and set(png) == {"width", "height", "sha256"}
                and hex_value(png.get("sha256"), 64) and type(png["width"]) is int and 640 <= png["width"] <= 4096
                and type(png["height"]) is int and 360 <= png["height"] <= 4096, "missing private PNG identity")
    return report


def run_trace(repo, expected, work, evidence):
    require(sys.platform == "win32", "actual trace execution requires Windows")
    require(not work.exists() and not evidence.exists(), "use new private work and evidence directories")
    require(work != evidence and not work.is_relative_to(evidence) and not evidence.is_relative_to(work),
            "private work and evidence must be separate")
    require(not work.is_relative_to(repo) and not evidence.is_relative_to(repo), "outputs must be outside checkout")
    require(os.environ.get("GITHUB_RUN_ATTEMPT", "1") == "1", "workflow reruns forbidden")
    work.mkdir(parents=True)
    evidence.mkdir(parents=True)
    report = {"schema_version": 1, "identity": IDENTITY, **QUALIFICATION, "baseline_commit": BASELINE,
              "baseline_tree": BASELINE_TREE, "diagnostic_commit": expected, "target": TARGET,
              "toolchain": TOOLCHAIN, "features": FEATURES, "default_features": True,
              "profile": "release", "status": "failed", "scene_attempts": 0,
              "informational_attempts": 0, "informational_invocations": 0,
              "baseline_observation": BASELINE_OBSERVATION,
              "host_snapshot": {"environment": {key: os.environ.get(key) for key in
                  ("ImageOS", "ImageVersion", "RUNNER_OS", "RUNNER_ARCH")},
                  "platform": platform.platform(), "windows_version": platform.version(),
                  "same_host_claimed": False},
              "limits": ["Dependency instrumentation changes execution and cannot qualify the ordinary binary",
                         "D3D12 WARP is not physical GPU, controller, audio or visual qualification",
                         "Absent trace events are unknown; timeout preserves only events already drained",
                         "No release approval or ordinary candidate acceptance is performed",
                         "Preserved baseline inventory/readiness do not qualify the instrumented executable"]}
    (evidence / "commands.log").write_text("", encoding="utf-8")
    source = private = preparation = package = bundle = None
    lock_hash = None
    try:
        source = source_inputs(repo, expected)
        report["diagnostic_tree"] = source["diagnostic_tree"]
        write_json(evidence / "source-inputs.json", source)
        report["source_inputs_sha256"] = digest(evidence / "source-inputs.json")
        ordinary = load_module(repo / "scripts/check-swift-windows-candidate.py", "ordinary_candidate")
        require((ordinary.TARGET, ordinary.TOOLCHAIN, ordinary.FEATURES, ordinary.CAPTURE_TIMEOUT_SECONDS)
                == (TARGET, TOOLCHAIN, FEATURES, 180), "ordinary recipe drift")
        require(ordinary.CAPTURE_TRACE == CAPTURE_TRACE, "ordinary logging recipe drift")
        private = work / "source"
        copy_baseline(repo, source, private)
        env = os.environ.copy()
        for name in ("CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS"):
            env.pop(name, None)
        env.update(CARGO_TARGET_DIR=str(work / "target"), RUSTFLAGS="-D warnings", CARGO_TERM_COLOR="never",
                   TRACE_RUST_LOG=ordinary.CAPTURE_TRACE)
        run = Commands(repo, work, evidence, env)
        report["commands"] = run.records
        report["runtime_environment"] = {"WGPU_BACKEND": "dx12", "WGPU_FORCE_FALLBACK_ADAPTER": "1",
                                          "BEVY_ASSET_ROOT": "<source>", "CARGO_MANIFEST_DIR": "<source>",
                                          "RUST_LOG": ordinary.CAPTURE_TRACE}
        report["compiler_flags"] = {"RUSTFLAGS": "-D warnings", "CARGO_TARGET_DIR": "<private-work>/target"}
        rustc, _ = run(["rustc", "+" + TOOLCHAIN, "-Vv"])
        report["rustc"] = rustc.stdout.decode("utf-8").strip()
        require(report["rustc"].startswith("rustc 1.93.0 "), "wrong Rust compiler")
        run(["cargo", "+" + TOOLCHAIN, "fetch", "--locked", "--target", TARGET], cwd=private)
        metadata_command = ordinary.candidate_commands()["metadata"]
        before, _ = run(metadata_command, cwd=repo)
        metadata_path = work / "ordinary-metadata.json"
        metadata_path.write_bytes(before.stdout)
        notices = work / "baseline-dependency-notices"
        run([sys.executable, "-B", repo / "scripts/collect-dependency-notices.py", "--metadata", metadata_path,
             "--repo", repo, "--target", TARGET, "--root-package", "flightsim-app", "--output", notices])
        ordinary.validate_inventory(read_json(notices / "dependency-inventory.json"), json.loads(before.stdout),
                                    repo, metadata_path)
        original_lock = (private / "Cargo.lock").read_text(encoding="utf-8")
        prepared = work / "prepared.json"
        run([sys.executable, "-B", repo / "diagnostics/windows-readback/prepare.py", "--repo", private,
             "--diagnostic-root", repo / "diagnostics/windows-readback", "--work", work / "dependencies",
             "--output", prepared])
        preparation = read_json(prepared)
        config = Path(preparation["cargo_config"])
        require(config.is_file() and config.resolve().is_relative_to(work), "patch config must be private")
        require(Path(preparation["generated_lockfile"]).resolve() == (private / "Cargo.lock").resolve(), "wrong generated lock location")
        patch_config = tomllib.loads(config.read_text(encoding="utf-8"))
        require(patch_config == {"patch": {"crates-io": {name: {"path": str(work / "dependencies" / (name + "-" + version)).replace("\\", "/")}
                                                       for name, version in PATCHED}}}, "unexpected Cargo patch/config overrides")
        validate_lock_delta(original_lock, (private / "Cargo.lock").read_text(encoding="utf-8"))
        lock_hash = digest(private / "Cargo.lock")
        after, _ = run([*metadata_command, "--config", config], cwd=private)
        packages = validate_metadata_delta(json.loads(before.stdout), json.loads(after.stdout))
        dependencies = {"schema_version": 1, "identity": IDENTITY, **QUALIFICATION,
                        "preparation": sanitize_value(preparation, repo, work),
                        "original_lock_sha256": hashlib.sha256(original_lock.encode("utf-8")).hexdigest(),
                        "final_lock_sha256": lock_hash, "cargo_config_sha256": digest(config),
                        "ordinary_metadata_sha256": hashlib.sha256(before.stdout).hexdigest(),
                        "diagnostic_metadata_sha256": hashlib.sha256(after.stdout).hexdigest(), "packages": packages}
        write_json(evidence / "dependency-inputs.json", dependencies)
        report["dependency_inputs_sha256"] = digest(evidence / "dependency-inputs.json")
        report["final_lock_sha256"] = lock_hash
        validate_dependency_manifest(dependencies, source, report)
        verify_private_source(private, source, lock_hash)
        verify_prepared_bytes(preparation, work)
        command = [*ordinary.candidate_commands()["build"], "--config", config]
        report["build_command"] = [clean_log(str(arg), repo, work) for arg in command]
        run(command, cwd=private)
        verify_private_source(private, source, lock_hash)
        verify_prepared_bytes(preparation, work)
        executable = work / "target" / TARGET / "release/flightsim-app.exe"
        report["executable_sha256"] = digest(executable)
        bundle, package = stage_package(run, ordinary, repo, executable, work, notices, report)
        write_json(evidence / "package-inputs.json", package)
        report["package_inputs_sha256"] = digest(evidence / "package-inputs.json")
        unrelated = work / "unrelated-cwd"
        unrelated.mkdir()
        capture_once(run, ordinary, bundle / "flightsim-app.exe", unrelated, work, evidence, report)
        require("trace_error" not in report["capture"], "malformed diagnostic trace")
        require(report["capture"]["runtime_log_truncated"] is False, "diagnostic runtime log exceeded size bound")
        require(report["capture"]["adapter"]["status"] == "matches_baseline", "diagnostic adapter differs from the ordinary baseline observation")
        require(parse_trace((evidence / "runtime.log").read_text(encoding="utf-8"))["event_count"] > 0,
                "capture produced no instrumentation events")
        require(source_inputs(repo, expected) == source, "source changed during diagnostic")
        verify_private_source(private, source, lock_hash)
        verify_package(bundle, package["files"])
        report["status"] = "diagnostic_capture_completed"
    except Exception as error:
        report["failure"] = clean_log(type(error).__name__ + ": " + str(error), repo, work)[:8192]
        raise
    finally:
        integrity_error = None
        try:
            if source is not None:
                require(source_inputs(repo, expected) == source, "source changed during diagnostic")
            if private is not None and lock_hash is not None:
                verify_private_source(private, source, lock_hash)
            if preparation is not None:
                verify_prepared_bytes(preparation, work)
            if package is not None and bundle is not None:
                verify_package(bundle, package["files"])
            report["post_run_integrity"] = "verified_for_available_inputs"
        except Exception as error:
            integrity_error = error
            report["status"] = "failed"
            report["post_run_integrity"] = "failed"
            report["integrity_failure"] = clean_log(str(error), repo, work)[:8192]
            report.setdefault("failure", report["integrity_failure"])
        seal_report(evidence, report)
        if integrity_error is not None and sys.exc_info()[0] is None:
            raise integrity_error


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--expected-source")
    parser.add_argument("--work", type=Path)
    parser.add_argument("--evidence", type=Path)
    parser.add_argument("--validate-evidence", type=Path)
    args = parser.parse_args(argv)
    try:
        if args.validate_evidence:
            validate_evidence(args.validate_evidence)
        else:
            require(args.expected_source and args.work and args.evidence, "expected-source, work and evidence required")
            run_trace(args.repo.resolve(), args.expected_source, args.work.resolve(), args.evidence.resolve())
    except Exception as error:
        print("Windows readback trace failed: " + str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
