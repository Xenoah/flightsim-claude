#!/usr/bin/env python3
"""Bounded source checks, optionally with extracted Rust selection execution.

This is not a release checker or a Bevy/GPU compilation. It never changes the
existing acceptance checkers. The optional Rust harness executes the exact HDR
selection match and the full shared binding function extracted from the candidate
source, using minimal stand-ins for GPU resource identities. It cannot validate
Bevy type integration, shader compilation, image upload or native rendering.
"""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
VENDOR = ROOT / "vendor/bevy_core_pipeline"
AGX = "src/tonemapping/luts/AgX-default_contrast.ktx2"
LUTS = {
    "Blender_-11_12.ktx2": "a81a2462182bc8499d1a222345a72e7c4f1fabc2cb23d9c543caa567ccba7ad7",
    "tony_mc_mapface.ktx2": "053e5adc519b1d733c819a625199e61cf138549db1358e533811d45df3227f84",
}


def require(value, message):
    if not value:
        raise ValueError(message)


def sha(data):
    return hashlib.sha256(data).hexdigest()


def block(source, anchor):
    """Extract a braced item; these selected items contain no string braces."""
    require(source.count(anchor) == 1, f"ambiguous source anchor: {anchor}")
    start = source.index(anchor)
    opening = source.index("{", start)
    depth = 1
    end = opening + 1
    while depth:
        require(end < len(source), f"unclosed source block: {anchor}")
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[start:end]


def inspect_source(archive):
    provenance = json.loads((VENDOR / "FLIGHTSIM-UPSTREAM-SOURCE.json").read_text())
    require(sha(archive.read_bytes()) == provenance["upstream_archive_sha256"],
            "unexpected original crate archive")
    originals = {}
    with tarfile.open(archive, "r:gz") as package:
        for member in package.getmembers():
            if member.isfile():
                path = str(Path(member.name).relative_to("bevy_core_pipeline-0.18.1"))
                require(path not in originals, "duplicate archive member")
                originals[path] = package.extractfile(member).read()
    inventory = [{"path": path, "bytes": len(data), "sha256": sha(data)}
                 for path, data in sorted(originals.items())]
    require(inventory == provenance["original_files"], "original inventory mismatch")
    omitted = {item["path"] for item in provenance["omitted_original_files"]}
    require(omitted == {AGX, ".cargo_vcs_info.json"}, "unexpected omission scope")
    changed = set()
    for path, data in originals.items():
        local = VENDOR / path
        if path in omitted:
            require(not local.exists(), f"omitted source remains: {path}")
        else:
            require(local.is_file(), f"lost retained member: {path}")
            if local.read_bytes() != data:
                changed.add(path)
    require(changed == {"src/tonemapping/mod.rs"}, "unreviewed original-file modification")
    require(sorted(changed) == provenance["modified_original_files"], "modification inventory mismatch")
    files = [path for path in VENDOR.rglob("*") if path.is_file()]
    for path in files:
        require(originals[AGX] not in path.read_bytes(), f"AgX payload retained: {path}")
    source = (VENDOR / "src/tonemapping/mod.rs").read_text()
    includes = re.findall(r'include_bytes!\("([^\"]+)"\)', source)
    require(sorted(includes) == sorted("luts/" + name for name in LUTS),
            "unexpected embedded LUT inputs")
    for name, expected in LUTS.items():
        require(sha((VENDOR / "src/tonemapping/luts" / name).read_bytes()) == expected,
                f"changed licensed payload: {name}")
    manifest = tomllib.loads((VENDOR / "Cargo.toml").read_text())
    require(manifest["features"]["tonemapping_luts"] == ["bevy_image/ktx2", "bevy_image/zstd"],
            "LUT decoder feature closure changed")
    require(tomllib.loads((ROOT / "Cargo.toml").read_text())["patch"]["crates-io"]["bevy_core_pipeline"]
            == {"path": "vendor/bevy_core_pipeline"}, "wrong dependency patch route")
    for name in ("TonyMcMapface-LICENSE-MIT", "Blender-Filmic-OpenColorIO-LICENSE",
                 "Blender-Filmic-NOTICE.txt"):
        require((VENDOR / "third-party-notices" / name).read_bytes()
                == (ROOT / "docs/release/licenses" / name).read_bytes(), "changed asset notice")
    before = originals["src/tonemapping/mod.rs"].decode()
    for anchor in ("fn setup_tonemapping_lut_image(", "pub fn lut_placeholder(",
                   "pub fn get_lut_bind_group_layout_entries("):
        require(block(source, anchor) == block(before, anchor), f"changed image/sampler/layout: {anchor}")
    return source, {"original_members": len(originals), "modified_members": sorted(changed),
                    "omitted_members": sorted(omitted), "agx_full_payload_absent": True,
                    "licensed_payloads_sha256": LUTS,
                    "module_sha256": sha(source.encode()),
                    "upstream_archive_sha256": sha(archive.read_bytes())}


def rust_harness(source):
    # The tested selection logic is extracted, never duplicated or translated.
    enum = block(source, "pub enum Tonemapping {")
    hdr_match = block(source, "match key.tonemapping {")
    binding = block(source, "pub fn get_lut_bindings<'a>(")
    return r'''
use std::sync::atomic::{AtomicUsize, Ordering};
static ERRORS: AtomicUsize = AtomicUsize::new(0);
#[cfg(not(feature = "tonemapping_luts"))]
macro_rules! error { ($($x:tt)*) => {{ let _ = format!($($x)*); ERRORS.fetch_add(1, Ordering::SeqCst); }}; }
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
''' + enum + r'''
struct Key { tonemapping: Tonemapping }
fn hdr_selection(key: Key) -> Vec<String> {
    let mut shader_defs = Vec::<String>::new();
''' + hdr_match + r'''
    shader_defs
}
pub type TextureView = u32;
pub type Sampler = u32;
pub struct GpuImage { texture_view: TextureView, sampler: Sampler }
pub struct FallbackImage { d3: GpuImage }
pub struct RenderAssets<T> { data: Vec<(u32, T)> }
impl<T> RenderAssets<T> { fn get(&self, key: &u32) -> Option<&T> {
    self.data.iter().find(|(id,_)| id == key).map(|(_, image)| image)
} }
pub struct TonemappingLuts { blender_filmic: u32, agx: u32, tony_mc_mapface: u32 }
''' + binding + r'''
fn main() {
    assert_eq!(Tonemapping::default(), Tonemapping::TonyMcMapface);
    let supported = [
        (Tonemapping::None, "TONEMAP_METHOD_NONE", 1),
        (Tonemapping::Reinhard, "TONEMAP_METHOD_REINHARD", 1),
        (Tonemapping::ReinhardLuminance, "TONEMAP_METHOD_REINHARD_LUMINANCE", 1),
        (Tonemapping::AcesFitted, "TONEMAP_METHOD_ACES_FITTED", 1),
        (Tonemapping::SomewhatBoringDisplayTransform, "TONEMAP_METHOD_SOMEWHAT_BORING_DISPLAY_TRANSFORM", 1),
        (Tonemapping::TonyMcMapface, "TONEMAP_METHOD_TONY_MC_MAPFACE", 1),
        (Tonemapping::BlenderFilmic, "TONEMAP_METHOD_BLENDER_FILMIC", 2),
    ];
    let images = RenderAssets { data: (1..=3).map(|id| (id, GpuImage { texture_view: id, sampler: id })).collect() };
    let fallback = FallbackImage { d3: GpuImage { texture_view: 9, sampler: 9 } };
    // Even a supplied/resident custom AgX resource must not bypass rejection.
    let luts = TonemappingLuts { tony_mc_mapface: 1, blender_filmic: 2, agx: 3 };
    assert_eq!(luts.agx, 3);
    for (method, shader, image) in supported {
        assert_eq!(hdr_selection(Key { tonemapping: method }), vec![shader]);
        assert_eq!(get_lut_bindings(&images, &luts, &method, &fallback), (&image, &image));
    }
    assert_eq!(ERRORS.load(Ordering::SeqCst), if cfg!(feature = "tonemapping_luts") { 0 } else { 2 });
    for error in [
        std::panic::catch_unwind(|| { hdr_selection(Key { tonemapping: Tonemapping::AgX }); }),
        std::panic::catch_unwind(|| { get_lut_bindings(&images, &luts, &Tonemapping::AgX, &fallback); }),
    ] {
        let panic = error.expect_err("AgX silently selected a fallback");
        let message = panic.downcast_ref::<String>().map(String::as_str)
            .or_else(|| panic.downcast_ref::<&str>().copied()).expect("non-string panic");
        assert!(message.contains("AgX tonemapping is unsupported in the FlightSim source subset"));
    }
    let empty = RenderAssets::<GpuImage> { data: vec![] };
    for (method, _, _) in supported {
        assert_eq!(get_lut_bindings(&empty, &luts, &method, &fallback), (&9, &9));
    }
    println!("PASS: seven supported HDR/binding selections, both explicit AgX panics, default Tony, feature-off diagnostics, unchanged missing-image fallback");
}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--upstream-archive", type=Path, required=True)
    parser.add_argument("--rustc", type=Path, help="Optional: run the small extracted selection harness in both feature modes")
    args = parser.parse_args()
    source, report = inspect_source(args.upstream_archive)
    report["extracted_rust_selection_tests"] = "not run"
    if args.rustc:
        runs = []
        with tempfile.TemporaryDirectory(prefix="tonemap-subset-") as directory:
            directory = Path(directory)
            harness = directory / "selection.rs"
            harness.write_text(rust_harness(source))
            for mode, extra in [("feature-off", []), ("feature-on", ["--cfg", 'feature="tonemapping_luts"'])]:
                executable = directory / mode
                subprocess.run([str(args.rustc), "--edition=2024", "-Dwarnings", str(harness),
                                "-o", str(executable), *extra], check=True)
                result = subprocess.run([str(executable)], text=True, capture_output=True, check=True)
                runs.append({"mode": mode, "result": result.stdout.strip(),
                             "expected_panic_diagnostics": result.stderr.strip()})
        report["extracted_rust_selection_tests"] = runs
    report["limits"] = "Source/extracted CPU-selection evidence only. No Bevy type integration, shader compilation, image upload, native runtime, final executable payload, whole-target rights or release qualification."
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
