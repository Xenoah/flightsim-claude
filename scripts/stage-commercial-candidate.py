#!/usr/bin/env python3
"""Assemble an allowlisted LOCAL candidate; never upload, approve or release it.

Run on the executable's target platform: the program must answer the structured
--distribution-info handshake. An unresolved rights/dependency gate still writes
a clearly blocked review candidate and returns a nonzero exit status.
"""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import shutil
import subprocess
import sys
import tempfile


SOURCE_FILES = (
    "assets/aircraft/swift_sport.glb",
    "assets/aircraft/swift_sport.json",
    "README.md",
    "CHANGELOG.md",
    "ATTRIBUTION.md",
    "LICENSE-MIT",
    "LICENSE-APACHE",
    "docs/data/NOTICE-GLOBAL-TERRAIN.txt",
    "docs/data/copernicus-glo90-license.pdf",
    "docs/data/global-sources.md",
    "docs/data/global-climate.md",
    "docs/data/global-surface-corrections.json",
    "docs/global-map.md",
    "docs/global-terrain.md",
    "crates/flightsim-world/data/global-terrain.provenance.json",
    "crates/flightsim-world/data/ncep-ncar-1991-2020.json",
    "docs/release/asset-rights-manifest.json",
    "docs/release/commercial-distribution-audit.md",
    "docs/release/commercial-candidate-staging.md",
    "docs/release/licenses/FiraMono-LICENSE",
    "docs/release/licenses/TonyMcMapface-LICENSE-MIT",
)
INVENTORY = "dependency-inventory.json"
MAX_METADATA_BYTES = 64 * 1024
MAX_NOTICE_BYTES = 2 * 1024 * 1024


def digest(path):
    result = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            result.update(block)
    return result.hexdigest()


def safe_file(root, relative):
    """Reject traversal and symlinks, including any intermediate component."""
    relative = PurePosixPath(relative)
    if relative.is_absolute() or not relative.parts or any(
        part in (".", "..") or ":" in part or "\\" in part for part in relative.parts
    ):
        raise ValueError(f"unsafe relative path: {relative}")
    current = root
    for part in relative.parts:
        current = current / part
        if current.is_symlink():
            raise ValueError(f"symlink is not allowed: {current}")
    if not current.is_file():
        raise ValueError(f"required file is missing: {current}")
    if not current.stat().st_size:
        raise ValueError(f"required file is empty: {current}")
    return current


def read_distribution_info(executable):
    """Read the binary's handshake; callers must validate their own build recipe."""
    if executable.is_symlink() or not executable.is_file():
        raise ValueError("executable must be an existing regular file, not a symlink")
    try:
        result = subprocess.run(
            [str(executable.resolve()), "--distribution-info"],
            capture_output=True, check=True, timeout=30,
        )
        if len(result.stdout) > MAX_METADATA_BYTES:
            raise ValueError("executable distribution metadata is too large")
        info = json.loads(result.stdout)
    except (OSError, subprocess.SubprocessError, UnicodeError, json.JSONDecodeError) as error:
        raise ValueError(f"cannot verify executable distribution identity: {error}") from error
    return info


def distribution_info(executable):
    info = read_distribution_info(executable)
    expected = {
        "schema_version": 1,
        "package": "flightsim-app",
        "profile": "commercial-staging",
        "region_downloads": False,
        "default_aircraft": "swift-sport",
        "default_model": "aircraft/swift_sport.glb",
        "bundled_aircraft": ["swift-sport"],
        "release_authorized": False,
    }
    if (not isinstance(info, dict) or any(info.get(k) != v for k, v in expected.items())
            or type(info.get("schema_version")) is not int
            or info.get("region_downloads") is not False
            or info.get("release_authorized") is not False):
        raise ValueError("executable is not the expected offline commercial-staging build")
    if (info.get("target_os"), info.get("target_arch"), info.get("target_env")) not in (
        ("windows", "x86_64", "msvc"), ("windows", "x86_64", "gnu"),
        ("linux", "x86_64", "gnu"),
    ):
        raise ValueError("candidate staging currently supports x86_64 Windows or Linux only")
    if not isinstance(info.get("package_version"), str) or not info["package_version"]:
        raise ValueError("executable package version is missing")
    return info


def notice_text(path, relative):
    if path.stat().st_size > MAX_NOTICE_BYTES:
        raise ValueError(f"dependency notice is too large: {relative}")
    contents = path.read_bytes()
    try:
        contents.decode("utf-8")
    except UnicodeError as error:
        raise ValueError(f"dependency notice is not UTF-8 text: {relative}") from error
    if b"\0" in contents:
        raise ValueError(f"dependency notice contains binary/NUL bytes: {relative}")
    return contents


def notice_files(root, inventory, review=None):
    """Copy only referenced, bounded UTF-8 notice text; never an arbitrary tree."""
    if root.is_symlink() or not root.is_dir():
        raise ValueError("dependency-notices must be a regular directory")
    safe_file(root, INVENTORY)
    references = []
    for key in ("packages", "embedded_assets"):
        records = inventory.get(key, [])
        if not isinstance(records, list):
            raise ValueError("dependency inventory records must be arrays")
        for record in records:
            if not isinstance(record, dict) or not isinstance(record.get("notices", []), list):
                raise ValueError("dependency notice records are malformed")
            references.extend(record.get("notices", []))
    if review is not None:
        if not isinstance(review, dict) or not isinstance(review.get("resolutions", []), list):
            raise ValueError("dependency review must contain valid resolutions")
        for resolution in review.get("resolutions", []):
            if not isinstance(resolution, dict) or not isinstance(resolution.get("evidence", []), list):
                raise ValueError("dependency review evidence is malformed")
            references.extend(resolution.get("evidence", []))
    referenced = set()
    for record in references:
        if not isinstance(record, dict) or not isinstance(record.get("path"), str):
            raise ValueError("dependency notice reference is malformed")
        relative = record["path"]
        if not relative.startswith("licenses/"):
            raise ValueError(f"dependency notice must be under licenses/: {relative}")
        path = safe_file(root, relative)
        contents = notice_text(path, relative)
        if hashlib.sha256(contents).hexdigest() != record.get("sha256"):
            raise ValueError(f"dependency notice hash mismatch: {relative}")
        referenced.add(relative)
    if not referenced:
        raise ValueError("dependency notices contain no referenced license texts")
    files = []
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            raise ValueError(f"dependency notices contain a symlink: {path}")
        if path.is_dir():
            continue
        relative = path.relative_to(root).as_posix()
        if relative not in (INVENTORY, "README.txt") and relative not in referenced:
            raise ValueError(f"unexpected dependency-notice file: {relative}")
        safe_file(root, relative)
        if relative == "README.txt":
            notice_text(path, relative)
        files.append(relative)
    return files


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def copy_file(source, destination):
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, destination)


def stage(root, executable, notices, output, dependency_review=None):
    if output.exists() or output.is_symlink():
        raise ValueError("output already exists; choose a new local candidate directory")
    root = root.resolve()
    executable = executable.absolute()
    notices = notices.absolute()
    # Resolve and check everything before creating an output candidate.
    sources = [(name, safe_file(root, name)) for name in SOURCE_FILES]
    gate = safe_file(root, "scripts/check-commercial-readiness.py")
    info = distribution_info(executable)
    inventory = json.loads(safe_file(notices, INVENTORY).read_text(encoding="utf-8"))
    if not isinstance(inventory, dict):
        raise ValueError("dependency inventory must be a JSON object")
    target = inventory.get("target", "")
    expected_target = (
        f"x86_64-pc-windows-{info['target_env']}" if info["target_os"] == "windows"
        else "x86_64-unknown-linux-gnu"
    )
    if target != expected_target:
        raise ValueError(f"dependency inventory target must be {expected_target}")
    review = None
    if dependency_review is not None:
        dependency_review = safe_file(dependency_review.parent, dependency_review.name)
        review = json.loads(dependency_review.read_text(encoding="utf-8"))
    dependency_files = notice_files(notices, inventory, review)
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".commercial-candidate-", dir=output.parent) as temporary:
        bundle = Path(temporary) / "bundle"
        bundle.mkdir()
        for name, source in sources:
            copy_file(source, bundle / name)
        binary_name = "flightsim-app.exe" if info["target_os"] == "windows" else "flightsim-app"
        copy_file(executable, bundle / binary_name)
        for name in dependency_files:
            copy_file(notices / name, bundle / "third-party" / name)
        write_json(bundle / "distribution-info.json", info)
        command = [
            sys.executable, str(gate), "--bundle", str(bundle), "--profile", "commercial",
            "--dependency-inventory", str(bundle / "third-party" / INVENTORY),
            "--json", "--repo", str(root),
        ]
        if dependency_review is not None:
            copy_file(dependency_review, bundle / "docs/release/dependency-review.json")
            command += ["--dependency-review", str(bundle / "docs/release/dependency-review.json")]
        result = subprocess.run(command, capture_output=True, text=True, timeout=120, check=False)
        try:
            report = json.loads(result.stdout)
        except json.JSONDecodeError as error:
            raise ValueError(f"readiness gate did not return JSON: {result.stderr[-2000:]}") from error
        if result.returncode not in (0, 2) or not isinstance(report, dict):
            raise ValueError(f"readiness gate failed unexpectedly: {result.stderr[-2000:]}")
        write_json(bundle / "commercial-readiness.json", report)
        blocked = result.returncode == 2
        if report.get("schema_version") != 1 or report.get("status") != ("blocked" if blocked else "checks_passed"):
            raise ValueError("readiness gate status disagrees with its exit code")
        blockers = report.get("blockers")
        if (not isinstance(blockers, list) or any(not isinstance(item, dict) for item in blockers)
                or bool(blockers) != blocked):
            raise ValueError("readiness gate blockers are malformed")
        if any(item.get("category") != "review"
               or item.get("code") in ("UNRESOLVED_ASSET_RIGHTS", "UNAPPROVED_GEODATA")
               for item in blockers):
            raise ValueError("readiness gate found an integrity error: " + json.dumps(blockers))
        (bundle / "LOCAL-CANDIDATE.txt").write_text(
            "LOCAL COMMERCIAL REVIEW CANDIDATE. NOT A STEAM RELEASE.\n"
            + ("READINESS GATE BLOCKED. See commercial-readiness.json.\n" if blocked else
               "Automated inventory checks passed; release approval is still required.\n")
            + "No account, agreement, payment, SDK setup, upload or publication was performed.\n"
            "Swift Sport is the default. The Meshy Light Single model is excluded.\n"
            "Start flightsim-app[.exe] directly; adjacent assets must stay together.\n"
            "World terrain and monthly climate are embedded; no network is required for them.\n"
            "Real GPU/controller/audio, target-platform and Steam store review gates remain.\n",
            encoding="utf-8",
        )
        files = [
            {"path": path.relative_to(bundle).as_posix(), "bytes": path.stat().st_size,
             "sha256": digest(path)}
            for path in sorted(bundle.rglob("*")) if path.is_file()
        ]
        write_json(bundle / "bundle-manifest.json", {
            "schema_version": 1,
            "kind": "local-commercial-review-candidate",
            "release_authorized": False,
            "readiness_gate": "blocked" if blocked else "passed",
            "distribution": info,
            "dependency_target": target,
            "excluded_assets": ["assets/aircraft/light_single.glb"],
            "files": files,
            "inventory_excludes_itself": True,
            "source_binary_correspondence": "requires separately recorded exact-build evidence",
        })
        # Never merge into or overwrite a previous candidate.
        if output.exists():
            raise ValueError("output appeared while staging; refusing to overwrite it")
        bundle.rename(output)
    return blocked


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source-root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--executable", type=Path, required=True)
    parser.add_argument("--dependency-notices", type=Path, required=True)
    parser.add_argument("--dependency-review", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        blocked = stage(args.source_root, args.executable, args.dependency_notices,
                        args.output.absolute(), args.dependency_review)
    except (ValueError, OSError, subprocess.SubprocessError) as error:
        print(f"candidate not staged: {error}", file=sys.stderr)
        return 2
    print(f"Local candidate: {args.output} (readiness {'BLOCKED' if blocked else 'checks passed'}; not released)")
    return 1 if blocked else 0


if __name__ == "__main__":
    raise SystemExit(main())
