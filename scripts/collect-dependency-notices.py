#!/usr/bin/env python3
"""Collect exact Cargo dependency notices; never infer legal clearance.

Run cargo metadata --locked --format-version 1 --filter-platform TARGET with
the release features first. This script performs no network requests/builds.
The normal/build closure is conservative: build tools and feature-unified
dependencies may not be linked. Inspect the final binary separately.
"""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import sys
import tomllib

NOTICE_NAME = re.compile(r"(?:licen[cs]e|copying|copyright|notice|unlicense)", re.I)
MAX_TEXT_BYTES = 2 * 1024 * 1024


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def safe_relative(value: str) -> Path:
    path = Path(value)
    if not value or path.is_absolute() or any(p in (".", "..") for p in value.split("/")) or "\\" in value:
        raise ValueError(f"unsafe relative path: {value!r}")
    return path


def read_regular(path: Path, root: Path) -> bytes:
    if not path.resolve().is_relative_to(root.resolve()):
        raise ValueError(f"notice escapes source root: {path.name}")
    if path.is_symlink() or not path.is_file():
        raise ValueError(f"not a regular file: {path.name}")
    if path.stat().st_size > MAX_TEXT_BYTES:
        raise ValueError(f"notice exceeds {MAX_TEXT_BYTES} bytes: {path.name}")
    data = path.read_bytes()
    data.decode("utf-8")  # Do not silently re-encode or discard notice bytes.
    if b"\0" in data:
        raise ValueError(f"notice contains NUL bytes: {path.name}")
    return data


def closure(metadata: dict, root_package: str) -> tuple[dict, dict, list[str]]:
    packages = {p["id"]: p for p in metadata["packages"]}
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    roots = [p["id"] for p in metadata["packages"] if p["name"] == root_package and p["id"] in metadata["workspace_members"]]
    if len(roots) != 1:
        raise ValueError("root package must identify one workspace member")
    seen: set[str] = set()
    todo = roots[:]
    while todo:
        package_id = todo.pop()
        if package_id in seen:
            continue
        if package_id not in packages or package_id not in nodes:
            raise ValueError("incomplete Cargo resolve graph")
        seen.add(package_id)
        for dep in nodes[package_id]["deps"]:
            kinds = dep.get("dep_kinds", [])
            if not kinds:
                raise ValueError("missing dependency-kind information")
            if any(k["kind"] in (None, "normal", "build") for k in kinds):
                todo.append(dep["pkg"])
    return packages, nodes, sorted(seen, key=lambda i: (packages[i]["name"], packages[i]["version"], i))


def collect(metadata_path: Path, repo: Path, output: Path, target: str, root_package: str) -> dict:
    if output.is_symlink() or (output.exists() and any(output.iterdir())):
        raise ValueError("output must be absent or empty; do not reuse a stale notice tree")
    if not re.fullmatch(r"[A-Za-z0-9_-]+", target):
        raise ValueError("invalid target triple")
    metadata_bytes = metadata_path.read_bytes()
    metadata = json.loads(metadata_bytes)
    workspace = Path(metadata["workspace_root"])
    # Local metadata may refer to a worktree distinct from --repo. License bytes
    # for its workspace members must come from that exact metadata workspace.
    packages, nodes, package_ids = closure(metadata, root_package)
    identities = [(packages[i]["name"], packages[i]["version"]) for i in package_ids]
    if len(set(identities)) != len(identities):
        raise ValueError("same-name/version packages from multiple sources need distinct reviewed notice destinations")
    lock_bytes = (workspace / "Cargo.lock").read_bytes()
    lock = tomllib.loads(lock_bytes.decode("utf-8"))
    checksums = {(p["name"], p["version"], p.get("source")): p.get("checksum") for p in lock["package"]}
    manifest = json.loads((repo / "docs/release/asset-rights-manifest.json").read_text())
    supplements_path = repo / "docs/release/dependency-notice-supplements.json"
    supplements = json.loads(supplements_path.read_text()) if supplements_path.is_file() else {"entries": []}
    output.mkdir(parents=True, exist_ok=True)
    unresolved = []
    records = []
    for package_id in package_ids:
        package = packages[package_id]
        name, version = package["name"], package["version"]
        slug = f"{name}-{version}"
        if not re.fullmatch(r"[A-Za-z0-9_.+-]+", slug):
            raise ValueError("unsafe package name/version")
        source = Path(package["manifest_path"]).parent
        vcs = source / ".cargo_vcs_info.json"
        vcs_info = json.loads(vcs.read_text()) if vcs.is_file() else None
        revision = vcs_info.get("git", {}).get("sha1") if vcs_info else None
        candidates = []
        if package_id in metadata["workspace_members"]:
            candidates = [(workspace / p, workspace) for p in ("LICENSE-MIT", "LICENSE-APACHE")]
        else:
            for path in sorted(source.rglob("*")):
                if path.is_file() and NOTICE_NAME.search(path.name) and ".git" not in path.parts:
                    candidates.append((path, source))
            explicit = package.get("license_file")
            if explicit:
                path = Path(explicit)
                if not path.is_absolute():
                    path = source / path
                if not any(path == candidate for candidate, _ in candidates):
                    candidates.append((path, source))
        notices = []
        package_unresolved = []
        primary_notice = False
        for path, source_root in candidates:
            try:
                data = read_regular(path, source_root)
                relative = safe_relative(path.relative_to(source_root).as_posix())
                destination = Path("licenses") / slug / relative
                target_path = output / destination
                target_path.parent.mkdir(parents=True, exist_ok=True)
                target_path.write_bytes(data)
                notices.append({"path": destination.as_posix(), "upstream_path": relative.as_posix(), "sha256": digest(data), "bytes": len(data)})
                explicit = package.get("license_file")
                explicit_path = Path(explicit) if explicit else None
                if explicit_path is not None and not explicit_path.is_absolute():
                    explicit_path = source / explicit_path
                primary_notice |= len(relative.parts) == 1 or path == explicit_path
            except (ValueError, OSError, UnicodeError) as error:
                package_unresolved.append(str(error))
        for supplement in supplements["entries"]:
            # Repository provenance is pinned to the source commit Cargo itself
            # records, not merely a crate name or a mutable default branch.
            if not revision or supplement["revision"] != revision:
                continue
            relative = safe_relative(supplement["path"])
            data = read_regular(repo / relative, repo)
            if digest(data) != supplement["sha256"]:
                raise ValueError(f"changed pinned license supplement: {relative}")
            destination = Path("licenses") / slug / "upstream" / safe_relative(supplement["upstream_path"])
            (output / destination).parent.mkdir(parents=True, exist_ok=True)
            (output / destination).write_bytes(data)
            notices.append({"path": destination.as_posix(), "upstream_path": supplement["upstream_path"], "source": supplement["source"], "source_revision": revision, "sha256": digest(data), "bytes": len(data)})
            primary_notice = True
        if not primary_notice:
            package_unresolved.append("No root/declared primary license text found in exact packaged source or pinned supplements; nested third-party notices do not substitute for the crate's own grant")
        if not package.get("license") and not package.get("license_file"):
            package_unresolved.append("Cargo manifest declares no license expression or license file")
        identity = f"{name}@{version}"
        for reason in package_unresolved:
            unresolved.append({"id": identity, "reason": reason})
        records.append({
            "id": identity, "name": name, "version": version,
            "source": package.get("source") or "workspace",
            "source_checksum": checksums.get((name, version, package.get("source"))),
            "source_revision": vcs_info.get("git", {}).get("sha1") if vcs_info else None,
            "source_path_in_repository": vcs_info.get("path_in_vcs") if vcs_info else None,
            "repository": package.get("repository"),
            "license_expression": package.get("license"),
            "features": nodes[package_id].get("features", []),
            "notices": notices, "unresolved": package_unresolved,
        })
    embedded = []
    for asset in manifest.get("dependency_assets", []):
        selected = [p for p in packages.values() if p["id"] in package_ids and p["name"] == asset["package"] and p["version"] == asset["version"]]
        if not selected:
            continue
        package = selected[0]
        if asset.get("feature") not in nodes[package["id"]].get("features", []):
            continue
        asset_path = Path(package["manifest_path"]).parent / safe_relative(asset["source_path"])
        entry = dict(asset)
        entry["observed_sha256"] = digest(asset_path.read_bytes())
        if entry["observed_sha256"] != asset["sha256"]:
            unresolved.append({"id": asset["id"], "reason": "Embedded dependency asset differs from the reviewed hash"})
        if asset["review_state"] == "unresolved":
            unresolved.append({"id": asset["id"], "reason": asset["reason"]})
        entry["notices"] = []
        for notice in asset.get("notice_files", []):
            relative = safe_relative(notice)
            data = read_regular(repo / relative, repo)
            destination = Path("licenses/supplemental") / relative.name
            (output / destination).parent.mkdir(parents=True, exist_ok=True)
            (output / destination).write_bytes(data)
            entry["notices"].append({"path": destination.as_posix(), "sha256": digest(data), "bytes": len(data)})
        embedded.append(entry)
    inventory = {
        "schema_version": 1, "kind": "cargo-dependency-notices",
        "target": target, "root_package": root_package,
        "cargo_lock_sha256": digest(lock_bytes), "metadata_sha256": digest(metadata_bytes),
        "asset_manifest_sha256": digest((repo / "docs/release/asset-rights-manifest.json").read_bytes()),
        "supplement_manifest_sha256": digest(supplements_path.read_bytes()) if supplements_path.is_file() else None,
        "scope": "Conservative normal/build dependency closure of target-filtered Cargo metadata; excludes dev-only edges, not proof of final linkage or exhaustive nested-source licensing",
        "review_status": "not_reviewed",
        "packages": records, "embedded_assets": embedded, "unresolved": unresolved,
    }
    (output / "dependency-inventory.json").write_text(json.dumps(inventory, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    (output / "README.txt").write_text(
        "Exact upstream notice bytes and declared license expressions, collected for review.\n"
        "This is not a license grant, legal approval, or a complete linked-binary SBOM.\n"
        "AND obligations, OR alternatives, embedded assets, source headers and platform runtimes\n"
        "need review. Missing notices remain unresolved in dependency-inventory.json.\n", encoding="utf-8")
    return inventory


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--metadata", required=True, type=Path)
    parser.add_argument("--repo", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--target", required=True)
    parser.add_argument("--root-package", default="flightsim-app")
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    try:
        inventory = collect(args.metadata, args.repo, args.output, args.target, args.root_package)
    except (KeyError, ValueError, OSError, UnicodeError, TypeError) as error:
        print(f"notice collection failed: {error}", file=sys.stderr)
        return 1
    print(json.dumps({"packages": len(inventory["packages"]), "unresolved": len(inventory["unresolved"]), "review_status": inventory["review_status"], "inventory": str(args.output / "dependency-inventory.json")}))
    return 0  # Collection succeeded, even though a release may remain blocked.


if __name__ == "__main__":
    raise SystemExit(main())
