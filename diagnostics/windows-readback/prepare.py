#!/usr/bin/env python3
"""Prepare pinned, private diagnostic dependencies; never edit registry caches.

The caller supplies a private ordinary-source copy and fetches its locked crates
first. Only that copy's Cargo.lock changes. Every original crate archive is
checked against both the ordinary lock and the reviewed patch manifest.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import tarfile
import tomllib


BASELINE_LOCK_SHA256 = "9ef5a7ccfa27755027201438dd15b973ff2a834565e62c25b41086f85b612c06"
HELPER = "flightsim-readback-trace"
RECIPE_PATHS = (
    "prepare.py", "edits.json", "bevy_render.patch", "wgpu-core.patch", "events.json",
    "helper/Cargo.toml", "helper/src/lib.rs",
)


def require(ok, message):
    if not ok:
        raise ValueError(message)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def manifest(root, paths=None):
    files = []
    candidates = root.rglob("*") if paths is None else (root / p for p in paths)
    for path in sorted(candidates, key=lambda value: value.relative_to(root).as_posix()):
        require(not path.is_symlink(), "symlinks are forbidden")
        if path.is_file():
            files.append({"path": path.relative_to(root).as_posix(),
                          "bytes": path.stat().st_size, "sha256": sha(path)})
    encoded = json.dumps(files, sort_keys=True, separators=(",", ":")).encode()
    return {"files": files, "tree_sha256": hashlib.sha256(encoded).hexdigest()}


def extract(archive, destination, prefix):
    destination.mkdir()
    with tarfile.open(archive, "r:gz") as stream:
        members = stream.getmembers()
        require(len(members) < 10000, "oversized crate member list")
        total = 0
        seen = set()
        for member in members:
            path = PurePosixPath(member.name)
            require(not path.is_absolute() and ".." not in path.parts and "\\" not in member.name,
                    "unsafe crate member")
            require(path.parts and path.parts[0] == prefix, "crate root mismatch")
            require(member.isdir() or member.isfile(), "non-regular crate member")
            relative = Path(*path.parts[1:])
            require(relative.as_posix() not in seen, "duplicate crate member")
            seen.add(relative.as_posix())
            target = destination / relative
            require(target.resolve().is_relative_to(destination.resolve()), "crate path escape")
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
                continue
            total += member.size
            require(0 <= member.size <= 16 * 1024 * 1024 and total <= 64 * 1024 * 1024,
                    "crate size limit exceeded")
            target.parent.mkdir(parents=True, exist_ok=True)
            with stream.extractfile(member) as source, target.open("xb") as output:
                shutil.copyfileobj(source, output)


def patch_lock(text, dependencies):
    parts = text.split("[[package]]")
    found = set()
    for i in range(1, len(parts)):
        entry = tomllib.loads("[[package]]" + parts[i])["package"][0]
        if entry["name"] not in dependencies:
            continue
        name = entry["name"]
        require(name not in found, "duplicate patched crate in lock")
        found.add(name)
        parts[i] = re.sub(r'^source = .*\n|^checksum = .*\n', "", parts[i], flags=re.MULTILINE)
        require(parts[i].count("dependencies = [\n") == 1, "missing dependency list")
        parts[i] = parts[i].replace("dependencies = [\n", 'dependencies = [\n "flightsim-readback-trace",\n')
    require(found == set(dependencies), "missing patched lock entry")
    require(not any(p.get("name") == HELPER for p in tomllib.loads(text)["package"]),
            "helper already present in ordinary lock")
    return "[[package]]".join(parts) + f'[[package]]\nname = "{HELPER}"\nversion = "0.0.0"\n'


def prepare(repo, diagnostic_root, work, output, cargo_home):
    repo, diagnostic_root, work = (p.resolve() for p in (repo, diagnostic_root, work))
    require(repo != diagnostic_root and not diagnostic_root.is_relative_to(repo),
            "diagnostic input must be outside private ordinary source copy")
    require(not work.exists(), "dependency work directory must be fresh")
    lock_path = repo / "Cargo.lock"
    require(sha(lock_path) == BASELINE_LOCK_SHA256, "ordinary baseline Cargo.lock changed")
    lock = tomllib.loads(lock_path.read_text(encoding="utf-8"))
    edits = json.loads((diagnostic_root / "edits.json").read_text(encoding="utf-8"))
    require(edits["schema_version"] == 1, "unsupported edit manifest")
    require([(p["name"], p["version"]) for p in edits["dependencies"]]
            == [("bevy_render", "0.18.1"), ("wgpu-core", "27.0.3")], "unexpected patch set")
    work.mkdir(parents=True)
    result = {"schema_version": 1, "diagnostic_only": True, "ordinary_qualification": False,
              "baseline_lock_sha256": sha(lock_path), "dependencies": [],
              "recipe_files": manifest(diagnostic_root, RECIPE_PATHS)}
    patches = {}
    for dependency in edits["dependencies"]:
        name, version = dependency["name"], dependency["version"]
        package = [p for p in lock["package"] if p["name"] == name and p["version"] == version]
        require(len(package) == 1 and package[0]["checksum"] == dependency["archive_sha256"],
                "patch archive differs from baseline lock")
        matches = sorted((cargo_home / "registry/cache").glob(f"*/{name}-{version}.crate"))
        require(matches, f"fetch the pinned crate first: {name}-{version}")
        for archive in matches:
            require(sha(archive) == dependency["archive_sha256"], "crate archive checksum mismatch")
        destination = work / f"{name}-{version}"
        extract(matches[0], destination, f"{name}-{version}")
        before = manifest(destination)
        for edit in dependency["files"]:
            path = destination / edit["path"]
            require(path.resolve().is_relative_to(destination) and path.is_file(), "invalid patch path")
            require(sha(path) == edit["source_sha256"], "unreviewed original source bytes")
            text = path.read_text(encoding="utf-8")
            for replacement in edit["replacements"]:
                require(text.count(replacement["old"]) == 1, "patch context must match exactly once")
                text = text.replace(replacement["old"], replacement["new"])
            path.write_bytes(text.encode("utf-8"))
            require(sha(path) == edit["patched_sha256"], "patched source checksum mismatch")
        patches[name] = str(destination)
        result["dependencies"].append({"name": name, "version": version,
                                       "archive_sha256": sha(matches[0]),
                                       "original": before, "patched": manifest(destination),
                                       "patch_sha256": sha(diagnostic_root / f"{name}.patch")})
    helper = work / HELPER
    helper.mkdir()
    (helper / "src").mkdir()
    for relative in ("Cargo.toml", "src/lib.rs"):
        shutil.copyfile(diagnostic_root / "helper" / relative, helper / relative)
    result["helper"] = manifest(helper)
    config = work / "patch-config.toml"
    config.write_text("[patch.crates-io]\n" + "".join(
        f"{json.dumps(name)} = {{ path = {json.dumps(path.replace(chr(92), '/'))} }}\n"
        for name, path in patches.items()), encoding="utf-8")
    new_lock = patch_lock(lock_path.read_text(encoding="utf-8"), patches)
    lock_path.write_bytes(new_lock.encode("utf-8"))
    result.update(cargo_config=str(config), cargo_config_sha256=sha(config),
                  generated_lockfile=str(lock_path), generated_lockfile_sha256=sha(lock_path))
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for option in ("repo", "diagnostic-root", "work", "output"):
        parser.add_argument("--" + option, required=True, type=Path)
    parser.add_argument("--cargo-home", type=Path,
                        default=Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo")))
    args = parser.parse_args()
    prepare(args.repo, args.diagnostic_root, args.work, args.output, args.cargo_home)


if __name__ == "__main__":
    main()
