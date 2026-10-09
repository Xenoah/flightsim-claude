#!/usr/bin/env python3
"""Gate GitHub binary publication independently of ordinary source CI.

No authorization record is supplied by default. A real reviewer may later commit
``docs/release/release-authorization.json`` containing schema_version=1,
status="authorized", scope="github-windows-prerelease", authorized_by,
authorized_at (ISO-8601), evidence (nonempty references), and the exact version,
target, source_inventory_sha256 and release_inventory_sha256 reported here.
Never generate positive production approval merely to make this program pass.

The source digest covers every tracked blob/mode/path except that ONE receipt,
so committing the receipt itself needs no circular commit hash. The release
digest also covers exact shipped file bytes/destinations, build target/profile,
features and toolchain. Workflow/scripts are tracked source and thus bound too.
The receipt is itself separately hashed and must be committed and unmodified.

The existing commercial checker is reused for SOURCE/dependency preflight. Its
BUNDLE_NOT_CHECKED marker is expected here and is never called a bundle pass.
All its other blockers remain fatal. Bundle byte/allowlist verification happens
again before archiving and after extraction; no commercial checker is relaxed.
The release asset allowlist refers to exact hashes and reviewed states in the
shared asset records; it does not expand the Swift-only commercial candidate.
The legacy two-aircraft recipe below intentionally remains blocked by existing
rights evidence. To release a reviewed Swift-only payload, change the recipe,
feature/smoke expectations together, supply exact target dependency evidence,
resolve every real review blocker, then obtain a new inventory-bound receipt.
A version change or any other tracked source/recipe/evidence change invalidates
old authorization. This is an accidental-publication guard, not protection from
a maintainer who can rewrite the trusted main branch or its workflow.
"""
from __future__ import annotations

import argparse
from datetime import datetime
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import subprocess
import sys
import tomllib


AUTHORIZATION = "docs/release/release-authorization.json"
ASSET_MANIFEST = "docs/release/asset-rights-manifest.json"
# The existing docs/release/.gitattributes preserves exact upstream bytes under
# licenses/**. Do not move notices under its LF-normalized evidence/ siblings.
DEPENDENCY_INVENTORY = "docs/release/licenses/dependency-evidence/dependency-inventory.json"
DEPENDENCY_REVIEW = "docs/release/dependency-review.json"
TARGET = "x86_64-pc-windows-msvc"
BUILD = {"target": TARGET, "profile": "release", "features": [], "toolchain": "1.93.0"}
# Preserve the legacy release's explicit payload for now. Unlike a recursive
# assets/ copy, this cannot silently pick up recordings, local packs or secrets.
SOURCE_FILES = (
    "assets/aircraft/light_single.glb", "assets/aircraft/light_single.json",
    "assets/aircraft/swift_sport.glb", "assets/aircraft/swift_sport.json",
    "assets/aircraft/swift_sport.blend", "README.md", "CHANGELOG.md",
    "ATTRIBUTION.md", "LICENSE-MIT", "LICENSE-APACHE",
    "docs/data/NOTICE-GLOBAL-TERRAIN.txt", "docs/data/copernicus-glo90-license.pdf",
    "docs/data/global-sources.md", "docs/data/global-climate.md",
    "docs/data/global-surface-corrections.json", "docs/global-map.md",
    "docs/global-terrain.md", "crates/flightsim-world/data/global-terrain.provenance.json",
    "crates/flightsim-world/data/ncep-ncar-1991-2020.json",
)
REVIEWED_STATES = {"original_source_recorded", "licensed_with_notices"}
RAW_SUFFIXES = (".fsreplay", ".fsdem", ".fsairports", ".fsscenery", ".osm", ".pbf", ".csv", ".log")


def load_script(name):
    spec = importlib.util.spec_from_file_location(name.replace("-", "_"), Path(__file__).with_name(name + ".py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


readiness = load_script("check-commercial-readiness")
staging = load_script("stage-commercial-candidate")


def encoded(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode()


def hash_value(value):
    return hashlib.sha256(encoded(value)).hexdigest()


def read_object(path):
    value = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(value, dict):
        raise ValueError(f"expected a JSON object: {path.name}")
    return value


def git(root, *args):
    return subprocess.run(["git", "-C", str(root), *args], check=True, capture_output=True).stdout


def source_inventory(root):
    # Check both index and working tree. Untracked files never authorize a run.
    git(root, "diff", "--quiet", "HEAD", "--")
    entries = []
    tracked = set()
    for entry in git(root, "ls-tree", "-rz", "HEAD").split(b"\0"):
        if not entry:
            continue
        identity, path = entry.split(b"\t", 1)
        relative = path.decode("utf-8")
        mode, kind, oid = identity.decode("ascii").split()
        if kind != "blob" or mode not in ("100644", "100755"):
            raise ValueError(f"source has an unsupported symlink/submodule: {relative}")
        tracked.add(relative)
        if relative != AUTHORIZATION:
            entries.append({"path": relative, "mode": mode, "git_blob": oid})
    if not entries:
        raise ValueError("source inventory is empty")
    return hash_value(entries), tracked


def inspect(root):
    root = root.resolve()
    source_hash, tracked = source_inventory(root)
    version = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"]
    if not isinstance(version, str) or not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.+-]+)?", version):
        raise ValueError("invalid workspace version")
    blockers = []

    def block(code, message):
        blockers.append({"code": code, "message": message})

    def regular(relative):
        path = staging.safe_file(root, relative)
        if relative not in tracked:
            raise ValueError(f"release input is not committed: {relative}")
        return path

    manifest = read_object(regular(ASSET_MANIFEST))
    if manifest.get("schema_version") != 1:
        raise ValueError("unsupported asset manifest schema")
    assets = {a["path"]: a for a in manifest["assets"]}
    if len(assets) != len(manifest["assets"]):
        raise ValueError("duplicate asset record")
    release_assets = manifest["release_external_assets"]
    if (not isinstance(release_assets, list)
            or any(not isinstance(path, str) for path in release_assets)
            or len(set(release_assets)) != len(release_assets)):
        raise ValueError("release asset allowlist must contain unique paths")
    allowed_assets = set(release_assets)
    for relative in allowed_assets:
        asset = assets.get(relative)
        if not relative.startswith("assets/") or not asset or asset["review_state"] not in REVIEWED_STATES:
            block("INVALID_RELEASE_ASSET_ALLOWLIST", f"Release allowlist entry lacks reviewed source record: {relative}")
    unresolved_hashes = {a["sha256"] for a in [*assets.values(),
                         *manifest.get("historical_excluded_assets", []),
                         *manifest.get("historical_excluded_dependency_assets", [])]
                         if a["review_state"] not in REVIEWED_STATES}
    for relative, asset in assets.items():
        if asset.get("delivery") == "embedded" and asset["review_state"] not in REVIEWED_STATES:
            block("UNRESOLVED_EMBEDDED_ASSET", f"Embedded source asset lacks reviewed rights: {relative}")
    files = []

    def add_file(source, destination):
        # Validate destination with the strict path routine without touching it.
        if (not isinstance(destination, str) or "\\" in destination or ":" in destination
                or any(part in ("", ".", "..") for part in destination.split("/"))):
            raise ValueError("unsafe bundle destination")
        path = regular(source)
        digest = readiness.digest(path)
        with path.open("rb") as stream:
            scenery_magic = stream.read(4) == b"FSSC"
        if path.name.lower().endswith(RAW_SUFFIXES) or scenery_magic:
            block("RAW_DATA_EXCLUDED", f"Raw telemetry/regional data is excluded: {source}")
        if digest in unresolved_hashes:
            block("UNRESOLVED_ASSET_RIGHTS", f"Unresolved asset bytes would be published: {source}")
        if source.startswith("assets/"):
            asset = assets.get(source)
            if source not in allowed_assets or not asset or asset["review_state"] not in REVIEWED_STATES:
                block("ASSET_NOT_ALLOWLISTED", f"Release asset lacks reviewed allowlist coverage: {source}")
            elif digest != asset["sha256"]:
                block("ASSET_CHANGED", f"Release asset differs from its reviewed bytes: {source}")
        files.append({"source": source, "path": destination, "bytes": path.stat().st_size, "sha256": digest})

    for relative in sorted(set(SOURCE_FILES) | set(manifest["required_bundle_files"])):
        add_file(relative, relative)
    # Do not accept data supplied merely by an ignored/untracked local file.
    inventory_path = review_path = None
    if DEPENDENCY_INVENTORY not in tracked or DEPENDENCY_REVIEW not in tracked:
        block("DEPENDENCY_REVIEW_MISSING", "Exact target dependency inventory, notices and completed review must be committed before release")
    if DEPENDENCY_INVENTORY in tracked:
        # An absent review must not hide the inventory's actual unresolved
        # evidence. Only committed inputs may influence the release preflight.
        inventory_path = regular(DEPENDENCY_INVENTORY)
        inventory = read_object(inventory_path)
        review = None
        if DEPENDENCY_REVIEW in tracked:
            review_path = regular(DEPENDENCY_REVIEW)
            review = read_object(review_path)
        if inventory.get("target") != TARGET:
            block("DEPENDENCY_TARGET_MISMATCH", "Reviewed dependency target must be Windows x86_64 MSVC")
        app = [p for p in inventory.get("packages", []) if p.get("name") == "flightsim-app"]
        expected_features = set(BUILD["features"]) | {"default"}
        if (len(app) != 1 or app[0].get("version") != version
                or set(app[0].get("features", [])) != expected_features):
            block("DEPENDENCY_BUILD_MISMATCH", "Dependency app version/features do not match this exact build recipe")
        for relative in staging.notice_files(inventory_path.parent, inventory, review):
            add_file((inventory_path.parent / relative).relative_to(root).as_posix(), "third-party/" + relative)
        if review_path is not None:
            add_file(DEPENDENCY_REVIEW, DEPENDENCY_REVIEW)
    # Reuse the established dependency integrity + substantive review rules.
    # This source-only preflight does not claim an absent bundle was checked.
    result = readiness.check(root, None, inventory_path, review_path)
    for item in result["blockers"]:
        if item["code"] != "BUNDLE_NOT_CHECKED":
            block(item["code"], item["message"])
    files.sort(key=lambda record: record["path"])
    if len({f["path"].casefold() for f in files}) != len(files):
        raise ValueError("duplicate or case-colliding Windows bundle destination")
    plan = {"schema_version": 1, "version": version, "build": BUILD,
            "source_inventory_sha256": source_hash, "files": files}
    release_hash = hash_value(plan)
    receipt_hash = ""
    if AUTHORIZATION not in tracked:
        block("AUTHORIZATION_MISSING", "No committed inventory-bound binary publication authorization; source CI may proceed")
    else:
        receipt_path = regular(AUTHORIZATION)
        receipt_hash = readiness.digest(receipt_path)
        receipt = read_object(receipt_path)
        expected = {"schema_version": 1, "status": "authorized", "scope": "github-windows-prerelease",
                    "version": version, "target": TARGET, "source_inventory_sha256": source_hash,
                    "release_inventory_sha256": release_hash}
        if any(receipt.get(k) != v for k, v in expected.items()) or type(receipt.get("schema_version")) is not int:
            block("AUTHORIZATION_STALE_OR_INVALID", "Authorization does not match this exact source, version, build recipe and release inventory")
        if (not isinstance(receipt.get("authorized_by"), str) or not receipt["authorized_by"].strip()
                or not isinstance(receipt.get("evidence"), list) or not receipt["evidence"]
                or any(not isinstance(e, str) or not e.strip() for e in receipt["evidence"])):
            block("AUTHORIZATION_EVIDENCE_MISSING", "Authorization requires a named authorizer and durable references to the actual publication decision")
        try:
            datetime.fromisoformat(receipt["authorized_at"].replace("Z", "+00:00"))
        except (KeyError, TypeError, ValueError, AttributeError):
            block("AUTHORIZATION_DATE_INVALID", "Authorization requires an ISO-8601 decision date")
    return {"schema_version": 1, "status": "blocked" if blockers else "authorized",
            "authorized": not blockers, "version": version,
            "source_inventory_sha256": source_hash, "release_inventory_sha256": release_hash,
            "authorization_sha256": receipt_hash, "blockers": blockers}, plan


def verify_distribution_info(executable, version):
    """The existing two-aircraft recipe is offline, regardless of notice inventory."""
    info = staging.read_distribution_info(executable)
    expected = {
        "schema_version": 1, "package": "flightsim-app", "package_version": version,
        "profile": "development", "region_downloads": False,
        "default_aircraft": "light-single", "default_model": "aircraft/light_single.glb",
        "bundled_aircraft": ["light-single", "swift-sport"],
        "target_os": "windows", "target_arch": "x86_64", "target_env": "msvc",
        "release_authorized": False,
    }
    if (not isinstance(info, dict) or any(info.get(key) != value for key, value in expected.items())
            or type(info.get("schema_version")) is not int
            or info.get("region_downloads") is not False
            or info.get("release_authorized") is not False):
        raise ValueError("executable is not the expected offline default Windows release build")
    return info


def verify_bundle(bundle, plan, executable):
    if bundle.is_symlink() or not bundle.is_dir():
        raise ValueError("bundle must be a regular directory")
    if executable.is_symlink() or not executable.is_file() or not executable.stat().st_size:
        raise ValueError("built executable must be a nonempty regular file")
    expected = {item["path"]: item for item in plan["files"]}
    expected["flightsim-app.exe"] = {"bytes": executable.stat().st_size, "sha256": readiness.digest(executable)}
    actual = set()
    for path in bundle.rglob("*"):
        if path.is_symlink():
            raise ValueError("bundle contains a symlink")
        if path.is_dir():
            continue
        relative = path.relative_to(bundle).as_posix()
        actual.add(relative)
        record = expected.get(relative)
        if (not record or not path.is_file() or path.stat().st_size != record["bytes"]
                or readiness.digest(path) != record["sha256"]):
            raise ValueError(f"bundle has an extra or changed file: {relative}")
    if actual != set(expected):
        raise ValueError("bundle does not contain exactly the authorized payload")


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--allow-blocked", action="store_true", help="report policy blocks successfully, while outputs still forbid downstream jobs")
    parser.add_argument("--github-output", type=Path)
    parser.add_argument("--github-summary", type=Path)
    parser.add_argument("--inventory-file", type=Path, help="write the exact copy plan ONLY if authorized")
    parser.add_argument("--bundle", type=Path, help="verify a staged or freshly extracted bundle")
    parser.add_argument("--executable", type=Path, help="exact just-built executable for bundle verification")
    parser.add_argument("--verify-built-executable", type=Path,
                        help="execute --distribution-info on the trusted freshly built binary after authorization; never use for an untrusted archive")
    args = parser.parse_args(argv)
    try:
        result, plan = inspect(args.repo)
        if args.verify_built_executable is not None:
            if not result["authorized"]:
                raise ValueError("built executable identity verification requires authorization")
            info = verify_distribution_info(args.verify_built_executable, plan["version"])
            result["built_executable"] = {"sha256": readiness.digest(args.verify_built_executable), "distribution": info}
        if args.bundle is not None:
            if not result["authorized"] or args.executable is None:
                raise ValueError("bundle verification requires authorization and the built executable")
            verify_bundle(args.bundle, plan, args.executable)
        if args.inventory_file and result["authorized"]:
            args.inventory_file.write_bytes(encoded(plan) + b"\n")
        if args.github_output:
            with args.github_output.open("a", encoding="utf-8") as stream:
                for key in ("authorized", "source_inventory_sha256", "release_inventory_sha256", "authorization_sha256"):
                    value = str(result[key]).lower() if key == "authorized" else result[key]
                    stream.write(f"{key}={value}\n")
        if args.github_summary:
            with args.github_summary.open("a", encoding="utf-8") as stream:
                stream.write("## Binary publication: " + result["status"] + "\n\n")
                stream.write("Ordinary source CI is separate. No package, release artifact or tag is created when blocked.\n\n")
                for item in result["blockers"]:
                    stream.write(f"- {item['code']}: {item['message']}\n")
        print(json.dumps(result, indent=2))
        return 0 if result["authorized"] or args.allow_blocked else 2
    except (OSError, ValueError, KeyError, TypeError, AttributeError, subprocess.SubprocessError) as error:
        print(f"release authorization check failed closed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
