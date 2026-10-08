#!/usr/bin/env python3
"""Fail-closed mechanical checks for an opt-in commercial staging directory.

Developer builds are unaffected. Success means recorded engineering checks pass;
it is not legal clearance, a license grant, or permission to publish on Steam.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import sys


def digest(path: Path) -> str:
    value = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def safe_path(root: Path, relative: str) -> Path:
    parts = relative.split("/")
    if not relative or Path(relative).is_absolute() or "\\" in relative or any(p in ("", ".", "..") for p in parts):
        raise ValueError(f"unsafe relative path: {relative!r}")
    path = root.joinpath(*parts)
    if not path.resolve().is_relative_to(root.resolve()):
        raise ValueError(f"path escapes root: {relative}")
    for parent in [path, *path.parents]:
        if parent == root.parent:
            break
        if parent.is_symlink():
            raise ValueError(f"symlink not allowed: {relative}")
    return path


def check(repo: Path, bundle: Path | None, dependency_inventory: Path | None = None,
          dependency_review: Path | None = None) -> dict:
    blockers = []
    info = None
    warnings = ["Engineering evidence only; no blanket legality, trademark clearance, Steam approval or publishing authorization is implied"]

    def block(code: str, message: str, category: str = "integrity") -> None:
        blockers.append({"code": code, "message": message, "category": category})

    manifest_path = repo / "docs/release/asset-rights-manifest.json"
    manifest = json.loads(manifest_path.read_text())
    if manifest.get("schema_version") != 1:
        raise ValueError("unsupported asset manifest schema")
    assets = {a["path"]: a for a in manifest["assets"]}
    if len(assets) != len(manifest["assets"]):
        raise ValueError("duplicate manifest asset path")
    for relative, asset in assets.items():
        path = safe_path(repo, relative)
        if not path.is_file() or digest(path) != asset["sha256"]:
            block("SOURCE_ASSET_CHANGED", f"Recorded source asset missing or changed: {relative}")

    if bundle is None:
        block("BUNDLE_NOT_CHECKED", "No staged bundle supplied; archive/executable/notice integrity remains unchecked", "review")
    else:
        if not bundle.is_dir() or bundle.is_symlink():
            raise ValueError("bundle must be a regular directory")
        allowed_assets = set(manifest["commercial_external_assets"])
        blocked_hashes = {a["sha256"] for a in [*assets.values(), *manifest.get("historical_excluded_assets", [])]
                          if a["review_state"] == "unresolved"}
        for path in sorted(bundle.rglob("*")):
            relative = path.relative_to(bundle).as_posix()
            safe_path(bundle, relative)
            if path.is_dir():
                continue
            if not path.is_file():
                block("NON_REGULAR_FILE", f"Unsupported bundle entry: {relative}")
                continue
            if relative.startswith(("assets/", "data/")) and relative not in allowed_assets:
                block("UNAPPROVED_ASSET", f"External asset is not in the commercial allowlist: {relative}")
            with path.open("rb") as stream:
                scenery_magic = stream.read(4) == b"FSSC"
            if path.name.lower().endswith((".osm", ".pbf", ".fsairports", ".fsdem", ".fsscenery")) or scenery_magic:
                block("UNAPPROVED_GEODATA", f"No reviewed regional/OSM distribution grant exists for: {relative}")
            if path.suffix.lower() in (".glb", ".gltf", ".blend", ".ttf", ".otf", ".wav", ".mp3", ".ogg", ".mp4", ".png", ".jpg", ".jpeg", ".webp", ".dds", ".ktx2", ".fsclim", ".fsgt") and relative not in allowed_assets:
                block("UNINVENTORIED_BINARY_ASSET", f"Unapproved binary asset path: {relative}")
            if digest(path) in blocked_hashes:
                block("UNRESOLVED_ASSET_RIGHTS", f"Excluded unresolved-rights asset bytes present (including renamed copies): {relative}")
        for relative in allowed_assets:
            if relative not in assets or assets[relative]["review_state"] == "unresolved":
                block("INVALID_ASSET_ALLOWLIST", f"Allowlist entry lacks reviewed source record: {relative}")
                continue
            path = safe_path(bundle, relative)
            if not path.is_file() or digest(path) != assets[relative]["sha256"]:
                block("BUNDLE_ASSET_CHANGED", f"Approved asset missing or changed: {relative}")
        for relative in manifest["required_bundle_files"]:
            original, staged = safe_path(repo, relative), safe_path(bundle, relative)
            if not original.is_file() or not staged.is_file() or digest(original) != digest(staged):
                block("REQUIRED_NOTICE_MISSING_OR_CHANGED", f"Exact source notice/provenance file missing or changed: {relative}")
        info_path = bundle / "distribution-info.json"
        if not info_path.is_file():
            block("DISTRIBUTION_IDENTITY_MISSING", "Stage must record actual executable --distribution-info output")
        else:
            info = json.loads(info_path.read_text())
            if not isinstance(info, dict):
                raise ValueError("distribution identity must be a JSON object")
            if info.get("schema_version") != 1 or info.get("profile") != "commercial-staging" or info.get("default_aircraft") != "swift-sport" or info.get("bundled_aircraft") != ["swift-sport"]:
                block("DISTRIBUTION_PROFILE_UNVERIFIED", "Executable identity does not report the Swift-only commercial-staging profile")
            if info.get("region_downloads") is not False:
                block("DISTRIBUTION_FEATURE_UNVERIFIED", "The current offline candidate requires explicit region_downloads=false in executable identity")
            warnings.append("distribution-info.json is an attestation captured by the stager; this checker does not execute or independently authenticate the binary")

    if dependency_inventory is None:
        block("DEPENDENCY_INVENTORY_MISSING", "Collect notices for the exact release target and features", "review")
    else:
        if bundle is not None and not dependency_inventory.resolve().is_relative_to(bundle.resolve()):
            block("INVENTORY_NOT_BUNDLED", "Dependency inventory must be inside the staged bundle")
        inventory = json.loads(dependency_inventory.read_text())
        if inventory.get("schema_version") != 1 or inventory.get("kind") != "cargo-dependency-notices":
            raise ValueError("unsupported dependency inventory")
        if inventory.get("cargo_lock_sha256") != digest(repo / "Cargo.lock"):
            block("DEPENDENCY_LOCK_CHANGED", "Dependency inventory does not match current Cargo.lock")
        if inventory.get("asset_manifest_sha256") != digest(manifest_path):
            block("DEPENDENCY_ASSET_MANIFEST_CHANGED", "Regenerate dependency notices against the current asset manifest")
        supplements_path = repo / "docs/release/dependency-notice-supplements.json"
        expected_supplements = digest(supplements_path) if supplements_path.is_file() else None
        if inventory.get("supplement_manifest_sha256") != expected_supplements:
            block("DEPENDENCY_SUPPLEMENTS_CHANGED", "Regenerate dependency notices against the current pinned supplement record")
        if inventory.get("root_package") != "flightsim-app" or not inventory.get("target"):
            block("DEPENDENCY_SCOPE_INVALID", "Inventory must identify flightsim-app and its target")
        packages = inventory.get("packages", [])
        if not packages or len({p["id"] for p in packages}) != len(packages):
            block("DEPENDENCY_PACKAGES_INVALID", "Dependency inventory has an empty/duplicate package list")
        app_packages = [p for p in packages if p.get("name") == "flightsim-app"]
        if bundle is not None:
            features = app_packages[0].get("features") if len(app_packages) == 1 else None
            if (not isinstance(features, list) or "commercial-staging" not in features
                    or "region-downloads" in features):
                block("DEPENDENCY_FEATURE_MISMATCH", "Capture metadata for the offline commercial-staging build, without region-downloads")
        embedded_by_id = {p["id"]: p for p in inventory.get("embedded_assets", [])}
        manifest_unresolved = []
        for expected in manifest.get("dependency_assets", []):
            matches = [p for p in packages if p.get("name") == expected["package"] and p.get("version") == expected["version"]]
            if len(matches) != 1:
                block("DEPENDENCY_EMBEDDING_UNVERIFIED", f"Expected engine package/version not recorded: {expected['package']}@{expected['version']}")
                continue
            if expected["feature"] in matches[0].get("features", []):
                observed = embedded_by_id.get(expected["id"])
                if not observed or observed.get("observed_sha256") != expected["sha256"]:
                    block("DEPENDENCY_EMBEDDED_ASSET_CHANGED", f"Embedded engine asset missing or changed: {expected['id']}")
                if expected["review_state"] == "unresolved":
                    manifest_unresolved.append({"id": expected["id"], "reason": expected["reason"]})
        for record in [*packages, *inventory.get("embedded_assets", [])]:
            for notice in record.get("notices", []):
                path = safe_path(dependency_inventory.parent, notice["path"])
                if not path.is_file() or digest(path) != notice["sha256"]:
                    block("DEPENDENCY_NOTICE_CHANGED", f"Dependency notice missing or changed: {notice['path']}")
                elif path.stat().st_size > 2 * 1024 * 1024:
                    block("DEPENDENCY_NOTICE_NOT_TEXT", f"Dependency notice exceeds text size bound: {notice['path']}")
                else:
                    try:
                        data = path.read_bytes()
                        data.decode("utf-8")
                        if b"\0" in data:
                            raise ValueError("NUL in text")
                    except (UnicodeError, ValueError):
                        block("DEPENDENCY_NOTICE_NOT_TEXT", f"Dependency notice is not UTF-8 non-NUL text: {notice['path']}")
        summary = inventory.get("unresolved", [])
        package_unresolved = [{"id": package["id"], "reason": reason}
                              for package in packages for reason in package.get("unresolved", [])]
        derived = [*package_unresolved, *manifest_unresolved]
        if not isinstance(summary, list) or any(not isinstance(item, dict) or not isinstance(item.get("id"), str) or not isinstance(item.get("reason"), str) for item in [*summary, *derived]):
            raise ValueError("malformed unresolved dependency evidence")
        summarized = {(item["id"], item["reason"]) for item in summary}
        missing = {(item["id"], item["reason"]) for item in derived} - summarized
        if missing:
            block("DEPENDENCY_UNRESOLVED_SUMMARY_INCONSISTENT", "Dependency summary omits unresolved package or enabled manifest-asset evidence; regenerate the inventory")
        # Never let a truncated summary erase evidence already present in an
        # individual package or the trusted repository asset manifest.
        unresolved = [{"id": identity, "reason": reason} for identity, reason in sorted(summarized | {(item["id"], item["reason"]) for item in derived})]
        review = json.loads(dependency_review.read_text()) if dependency_review else None
        resolved_ids = set()
        if review is None:
            block("DEPENDENCY_REVIEW_REQUIRED", "Notice collection does not select license alternatives or verify nested/platform obligations; reviewer record is required", "review")
        elif review.get("schema_version") != 1 or review.get("inventory_sha256") != digest(dependency_inventory) or review.get("status") != "reviewed" or not review.get("reviewed_by") or not review.get("reviewed_at") or not review.get("scope"):
            block("DEPENDENCY_REVIEW_INVALID", "Reviewer record must identify reviewer/date/scope and bind to this exact inventory hash", "review")
        else:
            for resolution in review.get("resolutions", []):
                # Evidence must itself be included for review; a bare 'approved'
                # boolean or SPDX string cannot resolve a missing source grant.
                evidence = resolution.get("evidence", [])
                if not resolution.get("id") or not resolution.get("reason") or not evidence:
                    block("DEPENDENCY_RESOLUTION_INVALID", "Each resolution needs an id, reason and packaged evidence", "review")
                    continue
                good = True
                for item in evidence:
                    path = safe_path(dependency_inventory.parent, item["path"])
                    if not item.get("source") or not path.is_file() or digest(path) != item["sha256"]:
                        block("DEPENDENCY_EVIDENCE_INVALID", f"Resolution evidence missing/changed: {item.get('path')}")
                        good = False
                if good:
                    resolved_ids.add(resolution["id"])
        for item in unresolved:
            if item["id"] not in resolved_ids:
                block("DEPENDENCY_UNRESOLVED", f"{item['id']}: {item['reason']}", "review")
        if info is not None:
            target = inventory.get("target", "")
            os_name = info.get("target_os", info.get("os"))
            arch = info.get("target_arch", info.get("arch"))
            env = info.get("target_env")
            if not os_name or not arch or not env or os_name not in target or not target.startswith(arch + "-") or not target.endswith("-" + env):
                block("DEPENDENCY_TARGET_MISMATCH", "Executable OS/architecture/environment and dependency inventory target do not match (GNU and MSVC are different)")

    return {"schema_version": 1, "status": "blocked" if blockers else "checks_passed", "blockers": blockers, "warnings": warnings}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--bundle", type=Path)
    parser.add_argument("--profile", choices=["commercial"], default="commercial")
    parser.add_argument("--dependency-inventory", type=Path)
    parser.add_argument("--dependency-review", type=Path)
    parser.add_argument("--json", action="store_true", help="write structured report to stdout")
    parser.add_argument("--report", type=Path, help="write structured report to this file")
    args = parser.parse_args()
    try:
        result = check(args.repo, args.bundle, args.dependency_inventory, args.dependency_review)
        encoded = json.dumps(result, indent=2, ensure_ascii=False) + "\n"
        if args.report:
            args.report.write_text(encoded, encoding="utf-8")
        if args.json:
            print(encoded, end="")
        else:
            print(f"Commercial staging: {result['status']}")
            for item in result["blockers"]:
                print(f"- [{item['category']}] {item['code']}: {item['message']}")
            for item in result["warnings"]:
                print(f"- Note: {item}")
        return 2 if result["blockers"] else 0
    except (KeyError, ValueError, OSError, UnicodeError, TypeError) as error:
        print(f"commercial readiness check failed: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
