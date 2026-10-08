"""Preparation integrity tests, including hostile archive members and lock drift."""
import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import tomllib
import unittest

spec = importlib.util.spec_from_file_location("prepare_readback", Path(__file__).with_name("prepare.py"))
prepare = importlib.util.module_from_spec(spec)
spec.loader.exec_module(prepare)


class PrepareTests(unittest.TestCase):
    def test_extract_rejects_paths_links_duplicates_and_oversize(self):
        for name, kind, size in (
            ("../escape", tarfile.REGTYPE, 1),
            ("crate/../../escape", tarfile.REGTYPE, 1),
            ("/crate/escape", tarfile.REGTYPE, 1),
            ("crate/symlink", tarfile.SYMTYPE, 0),
            ("crate/hardlink", tarfile.LNKTYPE, 0),
            ("other/file", tarfile.REGTYPE, 1),
            ("crate/file", tarfile.REGTYPE, 17 * 1024 * 1024),
        ):
            with self.subTest(name=name, kind=kind, size=size), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                archive = root / "bad.crate"
                with tarfile.open(archive, "w:gz") as output:
                    member = tarfile.TarInfo(name)
                    member.type, member.size = kind, size
                    # The size case is an actual complete oversized member.
                    output.addfile(member, io.BytesIO(bytes(size)) if kind == tarfile.REGTYPE else None)
                with self.assertRaises(ValueError):
                    prepare.extract(archive, root / "out", "crate")
                self.assertFalse((root / "escape").exists())
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            archive = root / "duplicate.crate"
            with tarfile.open(archive, "w:gz") as output:
                for _ in range(2):
                    member = tarfile.TarInfo("crate/file")
                    member.size = 1
                    output.addfile(member, io.BytesIO(b"x"))
            with self.assertRaisesRegex(ValueError, "duplicate"):
                prepare.extract(archive, root / "out", "crate")

    def test_lock_only_replaces_two_sources_and_adds_shared_helper(self):
        repo = Path(__file__).resolve().parents[2]
        original = (repo / "Cargo.lock").read_text()
        changed = prepare.patch_lock(original, {"bevy_render", "wgpu-core"})
        old = {p["name"] + "@" + p["version"]: p for p in tomllib.loads(original)["package"]}
        new = {p["name"] + "@" + p["version"]: p for p in tomllib.loads(changed)["package"]}
        helper = new.pop("flightsim-readback-trace@0.0.0")
        self.assertEqual(helper, {"name": "flightsim-readback-trace", "version": "0.0.0"})
        self.assertEqual(set(new), set(old))
        for identity, package in old.items():
            if package["name"] in {"bevy_render", "wgpu-core"}:
                package = package.copy()
                package.pop("source")
                package.pop("checksum")
                package["dependencies"] = ["flightsim-readback-trace", *package["dependencies"]]
            self.assertEqual(new[identity], package)
        with self.assertRaises(ValueError):
            prepare.patch_lock(changed, {"bevy_render", "wgpu-core"})

    def test_wrong_ordinary_lock_fails_before_any_dependency_write(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source, diag = root / "source", root / "diag"
            source.mkdir()
            diag.mkdir()
            (source / "Cargo.lock").write_text("not the baseline")
            with self.assertRaisesRegex(ValueError, "baseline Cargo.lock changed"):
                prepare.prepare(source, diag, root / "work", root / "manifest.json", root / "cargo")
            self.assertFalse((root / "work").exists())


if __name__ == "__main__":
    unittest.main()
