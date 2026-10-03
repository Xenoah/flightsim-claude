#!/usr/bin/env python3
"""Build/inspect an ephemeral MSVC Swift candidate; export only QA text/PNG.

No publication, authorization receipt, dependency approval, or binary upload.
The existing stager alone owns the copy set. A review-blocked candidate can pass
engineering acceptance while remaining explicitly unauthorized for distribution.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import zipfile
import zlib


TARGET = "x86_64-pc-windows-msvc"
TOOLCHAIN = "1.93.0"
FEATURES = ["commercial-staging"]  # Defaults retained, including Bevy's LUT bundle.
IDENTITY = "swift-only-windows-engineering-candidate-v1"
LEGACY_BASELINE = "5c5b2a3057549c7429236b93aa0cdc99e2de38d1"
# Independently reproduced from the 68 f64 values, signed-zero inertia entries,
# FNV byte order and FDM revision 2 suffix; also matches prior native QA.
LEGACY_FINGERPRINT = "0505e6644bb29a53"
# A same-head fixture and consumer must not silently drift together. Updating
# these requires explicit review of legacy identity, not a generated refresh.
LEGACY_SOURCE_HASHES = {
    "assets/aircraft/light_single.json": "8cf101b6785a7ceaa32772f10e9bf7bfdea68898c9f9ac9fa744ccadde7a1e25",
    "crates/flightsim-fdm/src/aircraft.rs": "72091944bfff40abace7f10f05566d9a4394b96aa2cf767e149b54c996219671",
    "crates/flightsim-fdm/src/lib.rs": "a956e3046e906304e23e675d440ed20875ea5e47d1a3552158b8356f16b8ccc9",
    "crates/flightsim-sim/src/replay.rs": "0b783ceed247b984729021ae57c74b061936d627c04275a850e59079266a18c1",
    "crates/flightsim-app/src/aircraft_profile.rs": "59c6deb0db1822178b30a0ba4e2fcbcac9e2177e1f0f0851f54c74510f3c6da0",
}
TEXT_EVIDENCE = {
    "acceptance.json", "source-inputs.json", "dependency-inventory.json",
    "commercial-readiness.json", "commands.log", "default-swift.log",
    "absent-light-single.log", "default-rejects-legacy.log", "legacy-no-model.log",
}
PNG_NAME = "default-swift.png"
SWIFT_MODEL_LOG = "aircraft model: <private-work>/extracted/swift-candidate/assets/aircraft/swift_sport.glb"
MAX_EVIDENCE_BYTES = 32 * 1024 * 1024
ANSI = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def sanitize(text, repo, work):
    text = text.replace("\\\\?\\", "")
    for path, replacement in ((repo, "<source>"), (work, "<private-work>")):
        value = str(path)
        for variant in (repr(value)[1:-1], value, value.replace("\\", "/")):
            text = text.replace(variant, replacement)
    return text.replace("\\", "/")


def git(repo, *args):
    return subprocess.check_output(["git", *args], cwd=repo).decode("utf-8").strip()


def source_inputs(repo, expected):
    require(re.fullmatch(r"[0-9a-f]{40}", expected), "expected source must be a full lowercase SHA")
    require(git(repo, "rev-parse", "HEAD") == expected, "checkout is not the expected source")
    require(not git(repo, "status", "--porcelain", "--untracked-files=all"), "source checkout must be clean")
    paths = subprocess.check_output(["git", "ls-files", "-z"], cwd=repo).decode("utf-8").split("\0")
    records = []
    for relative in sorted(p for p in paths if p):
        path = repo / relative
        require(path.is_file() and not path.is_symlink(), f"non-regular source input: {relative}")
        records.append({"path": relative, "bytes": path.stat().st_size, "sha256": digest(path)})
    for relative, expected_hash in LEGACY_SOURCE_HASHES.items():
        require(digest(repo / relative) == expected_hash, f"legacy baseline changed: {relative}")
    return {"source_sha": expected, "source_tree": git(repo, "rev-parse", "HEAD^{tree}"),
            "legacy_baseline": LEGACY_BASELINE, "legacy_source_sha256": LEGACY_SOURCE_HASHES,
            "files": records}


def candidate_commands():
    common = ["--locked", "--release", "-j", "2", "--target", TARGET, "-p", "flightsim-app",
              "--features", ",".join(FEATURES)]
    return {
        "build": ["cargo", "+" + TOOLCHAIN, "build", *common],
        "identity_test": ["cargo", "+" + TOOLCHAIN, "test", *common, "--bin", "flightsim-app",
                          "distribution::tests::explicit_legacy_aircraft_retains_fingerprint_and_is_not_remapped",
                          "--", "--exact"],
        "metadata": ["cargo", "+" + TOOLCHAIN, "metadata", "--locked", "--format-version", "1",
                     "--filter-platform", TARGET, "--features",
                     ",".join("flightsim-app/" + feature for feature in FEATURES)],
    }


def validate_inventory(inventory, metadata, repo, metadata_path):
    require(inventory.get("target") == TARGET, "inventory is not MSVC")
    require(inventory.get("metadata_sha256") == digest(metadata_path), "inventory metadata mismatch")
    require(inventory.get("cargo_lock_sha256") == digest(repo / "Cargo.lock"), "inventory lock mismatch")
    require(inventory.get("asset_manifest_sha256") == digest(repo / "docs/release/asset-rights-manifest.json"),
            "inventory asset manifest mismatch")
    require(inventory.get("review_status") == "not_reviewed", "collection must not create review approval")
    app = [p for p in inventory["packages"] if p["name"] == "flightsim-app"]
    require(len(app) == 1 and set(app[0]["features"]) == {"default", *FEATURES}, "unexpected candidate app features")
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    engine = [p for p in metadata["packages"] if p["name"] == "bevy_core_pipeline"]
    require(len(engine) == 1 and "tonemapping_luts" in nodes[engine[0]["id"]]["features"],
            "this recipe requires the supported full LUT bundle; do not hide its open reviews")


def validate_readiness(report, returncode):
    require(returncode in (0, 2), "readiness checker failed unexpectedly")
    require(report.get("schema_version") == 1, "invalid readiness schema")
    blockers = report.get("blockers")
    require(isinstance(blockers, list), "missing readiness blockers")
    require(report.get("status") == ("blocked" if returncode == 2 else "checks_passed")
            and bool(blockers) == (returncode == 2), "readiness status/exit disagree")
    require(all(b.get("category") == "review" and b.get("code") not in
                ("UNRESOLVED_ASSET_RIGHTS", "UNAPPROVED_GEODATA") for b in blockers),
            "candidate has an integrity blocker")


def verify_bundle(bundle, built_executable, manifest_hash):
    manifest_file = bundle / "bundle-manifest.json"
    require(digest(manifest_file) == manifest_hash, "bundle manifest changed through archive/extraction")
    manifest = json.loads(manifest_file.read_text(encoding="utf-8"))
    require(manifest.get("release_authorized") is False, "candidate cannot be authorized by this recipe")
    entries = manifest["files"]
    paths = [entry["path"] for entry in entries]
    require(len(paths) == len(set(paths)), "duplicate bundle path")
    actual = set()
    for path in bundle.rglob("*"):
        require(not path.is_symlink(), "bundle symlinks are forbidden")
        if path.is_file():
            actual.add(path.relative_to(bundle).as_posix())
    require(actual == set(paths) | {"bundle-manifest.json"}, "bundle membership differs from manifest")
    require({p for p in actual if p.startswith("assets/")} == {
        "assets/aircraft/swift_sport.glb", "assets/aircraft/swift_sport.json"}, "candidate external assets are not Swift-only")
    for entry in entries:
        path = bundle / entry["path"]
        require(path.resolve().is_relative_to(bundle.resolve()), "bundle path escapes directory")
        require(path.stat().st_size == entry["bytes"] and digest(path) == entry["sha256"], "bundle file integrity failed")
    require(digest(bundle / "flightsim-app.exe") == digest(built_executable), "extracted executable differs from built executable")


def validate_png(path):
    """Validate complete 8-bit RGB/RGBA capture, including CRCs and zlib rows."""
    data = path.read_bytes()
    require(len(data) <= MAX_EVIDENCE_BYTES and data[:8] == b"\x89PNG\r\n\x1a\n", "invalid PNG signature/size")
    offset, image_data, header, ended = 8, bytearray(), None, False
    while offset + 12 <= len(data):
        size = struct.unpack_from(">I", data, offset)[0]
        kind = data[offset + 4:offset + 8]
        end = offset + 12 + size
        require(end <= len(data), "truncated PNG chunk")
        payload = data[offset + 8:offset + 8 + size]
        require(zlib.crc32(kind + payload) == struct.unpack_from(">I", data, offset + 8 + size)[0], "PNG CRC mismatch")
        if header is None:
            require(kind == b"IHDR" and size == 13, "PNG needs first IHDR")
            header = struct.unpack(">IIBBBBB", payload)
            width, height, depth, color, compression, filtering, interlace = header
            require(640 <= width <= 4096 and 360 <= height <= 4096 and depth == 8
                    and color in (2, 6) and (compression, filtering, interlace) == (0, 0, 0), "unsupported PNG capture dimensions/format")
        elif kind == b"IDAT":
            image_data.extend(payload)
        elif kind == b"IEND":
            require(size == 0 and end == len(data), "PNG trailing bytes or invalid IEND")
            ended = True
            break
        else:
            # The application's image writer emits only these three chunk
            # types. Do not allow arbitrary attachments in ancillary chunks.
            raise ValueError("unexpected PNG chunk")
        offset = end
    require(ended and image_data, "incomplete PNG")
    width, height, _, color, *_ = header
    expected = height * (1 + width * (3 if color == 2 else 4))
    decoder = zlib.decompressobj()
    raw = decoder.decompress(image_data, expected + 1)
    require(len(raw) == expected and decoder.eof and not decoder.unused_data and not decoder.unconsumed_tail,
            "invalid PNG pixel stream")
    stride = expected // height
    require(all(raw[i] <= 4 for i in range(0, expected, stride)), "invalid PNG row filter")
    return {"width": width, "height": height, "sha256": digest(path)}


def validate_smoke(log, exit_code, *, model):
    require(exit_code == 0, "screenshot process did not exit 0")
    plain = ANSI.sub("", log)
    for token in ("(swift-sport)" if model else "(light-single)",
                  "Screenshot saved to", "Batch capture complete: status 0"):
        require(token in plain, f"missing runtime proof: {token}")
    require(not re.search(r"(?m)(^|\s)ERROR(\s|:|$)|(?im:thread .+ panicked at|panic(?:ked)? at|Failed to load asset|unregistered type)", plain),
            "runtime logged ERROR, panic, or asset failure")
    if model:
        require(re.search(re.escape(SWIFT_MODEL_LOG) + r"[ \t\r]*(?:\n|$)", plain) and re.search(
            r"aircraft model fitted: 7\.12 m along its length → scale 1\.0000(?:\s|$)", plain),
            "Swift model/fit is not the baseline model")
        require("placeholder" not in plain.lower(), "default Swift fell back to placeholder")
    else:
        require("aircraft model fitted:" not in plain and "aircraft model:" not in plain,
                "explicit no-model unexpectedly loaded a model")


def legacy_identity(path):
    with path.open("rb") as source:
        prefix = source.read(14)
        require(len(prefix) == 14 and prefix[:8] == b"FSREPLAY", "missing legacy replay header")
        version, length = struct.unpack_from("<HI", prefix, 8)
        require(version == 1 and 0 < length <= 256, "fixture is not bounded legacy replay v1")
        name = source.read(length).decode("utf-8")
        fingerprint = source.read(8)
    require(name == "Light Single (generic)" and len(fingerprint) == 8, "fixture is not original Light Single")
    observed = struct.unpack("<Q", fingerprint)[0]
    require(f"{observed:016x}" == LEGACY_FINGERPRINT, "legacy replay fingerprint differs from frozen baseline")
    return {"format_version": version, "name": name, "fingerprint": f"{observed:016x}", "sha256": digest(path)}


def validate_evidence(directory):
    require(directory.is_dir() and not directory.is_symlink(), "missing evidence directory")
    for path in directory.iterdir():
        require(path.is_file() and not path.is_symlink(), "evidence must contain only regular files")
        require(path.name in TEXT_EVIDENCE | {PNG_NAME}, f"unapproved evidence path: {path.name}")
        require(path.stat().st_size <= MAX_EVIDENCE_BYTES, "evidence exceeds size bound")
        if path.name in TEXT_EVIDENCE:
            data = path.read_bytes()
            require(b"\0" not in data, "binary bytes in text evidence")
            text = data.decode("utf-8")
            if path.suffix == ".json":
                require(isinstance(json.loads(text), dict), "JSON evidence must be an object")
    report = json.loads((directory / "acceptance.json").read_text(encoding="utf-8"))
    require(report.get("candidate") == IDENTITY and report.get("release_authorized") is False,
            "invalid evidence identity/authorization")
    require(report.get("schema_version") == 1 and report.get("target") == TARGET
            and report.get("features") == FEATURES and report.get("default_features") is True,
            "invalid evidence schema/feature identity")
    expected_checks = {"default_swift": 0, "absent_light_single": 2,
                       "default_rejects_legacy": 2, "legacy_no_model": 0}
    checks = report.get("checks")
    require(isinstance(checks, dict) and set(checks) <= set(expected_checks), "unexpected evidence checks")
    for name, check in checks.items():
        require(check.get("status") == "passed" and check.get("exit_code") == expected_checks[name],
                "invalid acceptance check state")
    if report.get("status") == "engineering_checks_passed":
        require(set(checks) == set(expected_checks), "successful report lacks required checks")
        require((directory / PNG_NAME).exists() and all((directory / name).is_file() for name in TEXT_EVIDENCE),
                "successful report lacks required evidence")
        require(report.get("legacy_replay", {}).get("fingerprint") == LEGACY_FINGERPRINT,
                "successful report lacks frozen legacy fingerprint")
    else:
        require(report.get("status") == "failed" and isinstance(report.get("failure"), str)
                and report["failure"], "failed report needs an explicit reason")
    hashes = report.get("evidence_files")
    require(isinstance(hashes, dict), "missing evidence hashes")
    actual = {p.name for p in directory.iterdir()} - {"acceptance.json"}
    require(actual == set(hashes), "evidence file set changed")
    for name, record in hashes.items():
        path = directory / name
        require(record == {"bytes": path.stat().st_size, "sha256": digest(path)}, "evidence file hash/size mismatch")
    if (directory / PNG_NAME).exists():
        proof = report.get("checks", {}).get("default_swift", {})
        require(proof.get("status") == "passed" and proof.get("exit_code") == 0, "PNG has no successful Swift runtime proof")
        require(validate_png(directory / PNG_NAME) == proof.get("png"), "PNG does not match runtime proof")
        log = (directory / "default-swift.log").read_text(encoding="utf-8")
        require(digest(directory / "default-swift.log") == proof.get("log_sha256"), "Swift log changed")
        validate_smoke(log, proof["exit_code"], model=True)


def run_candidate(repo, expected, work, evidence):
    require(sys.platform == "win32", "actual candidate execution requires Windows")
    require(not work.exists() and not evidence.exists(), "use new work and evidence directories")
    require(work != evidence and not work.is_relative_to(evidence) and not evidence.is_relative_to(work), "work/evidence must be separate siblings")
    require(not work.is_relative_to(repo) and not evidence.is_relative_to(repo), "outputs must be outside the source checkout")
    work.mkdir(parents=True)
    evidence.mkdir(parents=True)
    report = {"schema_version": 1, "candidate": IDENTITY, "source_sha": expected,
              "target": TARGET, "toolchain": TOOLCHAIN, "profile": "release",
              "features": FEATURES, "default_features": True, "release_authorized": False,
              "status": "failed", "checks": {}, "limits": [
                  "Engineering acceptance only; rights, dependency review and release authorization remain independent",
                  "Bundled AgX/Filmic LUTs remain enabled and retain unresolved review records",
                  "Software D3D12 fallback is not physical GPU/controller/audio or Steam qualification",
                  "Legacy check proves original identity and replay startup, not whole-flight/cross-version reproduction"],
              "commands": candidate_commands()}
    env = os.environ.copy()
    for name in ("CARGO_ENCODED_RUSTFLAGS", "CARGO_BUILD_RUSTFLAGS"):
        env.pop(name, None)
    env.update(CARGO_TARGET_DIR=str(work / "target"), RUSTFLAGS="-D warnings", CARGO_TERM_COLOR="never")
    report["compiler_flags"] = {"RUSTFLAGS": env["RUSTFLAGS"], "CARGO_TARGET_DIR": "<private-work>/target"}

    def run(command, *, cwd=repo, timeout=3600, accepted=(0,), output=None, runtime=False):
        run_env = env.copy()
        if runtime:
            run_env.update(WGPU_BACKEND="dx12", WGPU_FORCE_FALLBACK_ADAPTER="1",
                           BEVY_ASSET_ROOT=str(repo), CARGO_MANIFEST_DIR=str(repo))
        timed_out = None
        try:
            result = subprocess.run([str(x) for x in command], cwd=cwd, env=run_env,
                                    capture_output=True, timeout=timeout)
        except subprocess.TimeoutExpired as error:
            # subprocess.run kills and waits for its process. Keep the captured
            # diagnostic bytes, but no partial screenshot enters evidence.
            timed_out = error
            result = subprocess.CompletedProcess(command, -1, error.stdout or b"", error.stderr or b"")
        # Cargo output is text; raw metadata goes only into the private workspace.
        log = (result.stdout + b"\n" + result.stderr).decode("utf-8", errors="replace")
        log = sanitize(log, repo, work)
        if output:
            output.write_text(log, encoding="utf-8")
        with (evidence / "commands.log").open("a", encoding="utf-8") as stream:
            stream.write(json.dumps([str(x).replace(str(work), "<private-work>").replace(str(repo), "<source>") for x in command]) + "\n")
            stream.write(f"exit_code={result.returncode}\n")
            if "metadata" not in command:
                stream.write(log.replace(str(work), "<private-work>").replace(str(repo), "<source>") + "\n")
        if timed_out is not None:
            raise timed_out
        require(result.returncode in accepted, f"command failed with {result.returncode}: {command[0]}")
        return result, log

    try:
        source = source_inputs(repo, expected)
        write_json(evidence / "source-inputs.json", source)
        report["source_inputs_sha256"] = digest(evidence / "source-inputs.json")
        rustc, _ = run(["rustc", "+" + TOOLCHAIN, "-Vv"])
        report["rustc"] = rustc.stdout.decode("utf-8").strip()
        require(report["rustc"].startswith("rustc 1.93.0 "), "wrong Rust compiler")
        run(report["commands"]["build"])
        executable = work / "target" / TARGET / "release/flightsim-app.exe"
        report["executable_sha256"] = digest(executable)
        result, _ = run(report["commands"]["metadata"])
        metadata_path = work / "metadata.json"
        metadata_path.write_bytes(result.stdout)
        notices = work / "dependency-notices"
        run([sys.executable, repo / "scripts/collect-dependency-notices.py", "--metadata", metadata_path,
             "--repo", repo, "--target", TARGET, "--root-package", "flightsim-app", "--output", notices])
        inventory_path = notices / "dependency-inventory.json"
        inventory = json.loads(inventory_path.read_text(encoding="utf-8"))
        validate_inventory(inventory, json.loads(result.stdout), repo, metadata_path)
        shutil.copyfile(inventory_path, evidence / "dependency-inventory.json")
        report["dependency_inventory_sha256"] = digest(inventory_path)
        report["metadata_sha256"] = digest(metadata_path)
        staged = work / "swift-candidate"
        result, _ = run([sys.executable, repo / "scripts/stage-commercial-candidate.py", "--source-root", repo,
                        "--executable", executable, "--dependency-notices", notices, "--output", staged], accepted=(0, 1))
        readiness = json.loads((staged / "commercial-readiness.json").read_text(encoding="utf-8"))
        validate_readiness(readiness, 2 if result.returncode == 1 else 0)
        manifest_hash = digest(staged / "bundle-manifest.json")
        verify_bundle(staged, executable, manifest_hash)
        # Both archive and extraction remain local to this runner, never evidence.
        archive = work / "swift-candidate.zip"
        with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as output:
            for path in sorted(staged.rglob("*")):
                if path.is_file():
                    output.write(path, "swift-candidate/" + path.relative_to(staged).as_posix())
        extracted = work / "extracted"
        with zipfile.ZipFile(archive) as source_zip:
            source_zip.extractall(extracted)
        bundle = extracted / "swift-candidate"
        verify_bundle(bundle, executable, manifest_hash)
        report.update(bundle_manifest_sha256=manifest_hash, private_archive_sha256=digest(archive))
        result, _ = run([sys.executable, repo / "scripts/check-commercial-readiness.py", "--repo", repo,
                        "--bundle", bundle, "--dependency-inventory", bundle / "third-party/dependency-inventory.json",
                        "--json"], accepted=(0, 2))
        readiness = json.loads(result.stdout)
        validate_readiness(readiness, result.returncode)
        write_json(evidence / "commercial-readiness.json", readiness)
        report["readiness"] = readiness["status"]
        report["readiness_blockers"] = readiness["blockers"]
        # Check the binary's deterministic handshake again after extraction.
        app = bundle / "flightsim-app.exe"
        unrelated = work / "unrelated-cwd"
        unrelated.mkdir()
        first, _ = run([app, "--distribution-info"], cwd=unrelated, timeout=30, runtime=True)
        second, _ = run([app, "--distribution-info"], cwd=unrelated, timeout=30, runtime=True)
        require(first.stdout == second.stdout, "distribution metadata is not deterministic")
        info = json.loads(first.stdout)
        require(info == json.loads((bundle / "distribution-info.json").read_text(encoding="utf-8")), "extracted distribution identity changed")
        report["distribution"] = info
        screenshot = work / PNG_NAME
        result, log = run([app, "--screenshot", screenshot, "--screenshot-delay", "5", "--exit-after-screenshot",
                           "--view", "chase"], cwd=unrelated, timeout=180, runtime=True,
                          output=evidence / "default-swift.log")
        validate_smoke(log, result.returncode, model=True)
        png = validate_png(screenshot)
        report["checks"]["default_swift"] = {"status": "passed", "exit_code": result.returncode,
                                               "png": png, "log_sha256": digest(evidence / "default-swift.log")}
        # Publishable evidence appears only after all Swift image prerequisites pass.
        shutil.copyfile(screenshot, evidence / PNG_NAME)
        result, log = run([app, "--aircraft", "light-single"], cwd=unrelated, timeout=30, accepted=(2,),
                          runtime=True, output=evidence / "absent-light-single.log")
        require("selected aircraft model is missing: aircraft/light_single.glb" in log, "wrong absent-model failure")
        require("aircraft model fitted:" not in log, "absent model reached graphics")
        report["checks"]["absent_light_single"] = {"status": "passed", "exit_code": result.returncode}
        _, log = run(report["commands"]["identity_test"])
        require("1 passed; 0 failed" in log, "legacy identity test did not actually execute")
        fixture = work / "legacy.fsreplay"
        run(["cargo", "+" + TOOLCHAIN, "run", "--locked", "--release", "-j", "2", "--target", TARGET,
             "-p", "flightsim-sim", "--example", "record_takeoff", "--", fixture])
        report["legacy_replay"] = legacy_identity(fixture)
        result, log = run([app, "--replay", fixture], cwd=unrelated, timeout=30, accepted=(2,), runtime=True,
                          output=evidence / "default-rejects-legacy.log")
        require("aircraft/FDM model mismatch" in log and report["legacy_replay"]["fingerprint"] in log,
                "default Swift did not reject the actual legacy identity")
        report["checks"]["default_rejects_legacy"] = {"status": "passed", "exit_code": result.returncode}
        result, log = run([app, "--aircraft", "light-single", "--no-model", "--replay", fixture,
                           "--screenshot", work / "legacy-no-model.png", "--screenshot-delay", "5",
                           "--exit-after-screenshot", "--view", "chase"], cwd=unrelated, timeout=180, runtime=True,
                          output=evidence / "legacy-no-model.log")
        validate_smoke(log, result.returncode, model=False)
        validate_png(work / "legacy-no-model.png")
        report["checks"]["legacy_no_model"] = {"status": "passed", "exit_code": result.returncode,
                                                "fingerprint": report["legacy_replay"]["fingerprint"]}
        require(source_inputs(repo, expected) == source, "source changed during candidate check")
        verify_bundle(bundle, executable, manifest_hash)
        report["status"] = "engineering_checks_passed"
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError, zlib.error) as error:
        report["failure"] = sanitize(str(error), repo, work)
        raise
    finally:
        report["evidence_files"] = {
            path.name: {"bytes": path.stat().st_size, "sha256": digest(path)}
            for path in sorted(evidence.iterdir()) if path.is_file() and path.name != "acceptance.json"
        }
        report["evidence_inventory_excludes_itself"] = True
        write_json(evidence / "acceptance.json", report)


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
            require(args.expected_source and args.work and args.evidence, "expected-source, work and evidence are required")
            run_candidate(args.repo.resolve(), args.expected_source, args.work.resolve(), args.evidence.resolve())
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError, zlib.error) as error:
        print(f"Swift Windows candidate check failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
