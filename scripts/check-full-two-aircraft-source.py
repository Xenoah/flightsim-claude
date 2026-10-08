#!/usr/bin/env python3
"""Verify a local source proposal. This is not release admission or a build audit.

The explicit input map must itself be independently reviewed at the final commit.
No legacy three-LUT positive control, replay pin or publication gate is rewritten.
"""
from __future__ import annotations
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[1]
MAP = 'docs/release/full-two-aircraft-source-inputs.json'
RECIPE = 'full-two-aircraft-tony-filmic-source-preparation-v1'
BASE = 'd918943a70d01644b5a6bebb8a903a5bbc89139f'
LIGHT = 'b41f29ade89701d31759e6bc8164d5cdb3aa8734f512628af63823ad7eaaa3cc'
PROFILE = '8cf101b6785a7ceaa32772f10e9bf7bfdea68898c9f9ac9fa744ccadde7a1e25'
LUTS = {
    'Blender_-11_12.ktx2': 'a81a2462182bc8499d1a222345a72e7c4f1fabc2cb23d9c543caa567ccba7ad7',
    'tony_mc_mapface.ktx2': '053e5adc519b1d733c819a625199e61cf138549db1358e533811d45df3227f84',
}
PENDING = (
    'independent_final_source_and_recipe_admission',
    'fresh_two_lut_target_graph_cargo_fingerprints_depinfo_and_binary_payload_audit',
    'complete_whole_target_and_native_rights_review',
    'native_terms_adoption_where_required',
    'final_target_inventory_and_notice_binding',
    'two_aircraft_extracted_windows_runtime_and_screenshots',
    'final_local_and_hosted_source_archive_verification',
    'inventory_bound_publication_authorization',
)


def require(condition, message):
    if not condition:
        raise ValueError(message)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def regular(root, relative):
    parts = relative.split('/')
    require(relative and '\\' not in relative and ':' not in relative
            and not any(p in ('', '.', '..') for p in parts), 'unsafe input path')
    path = root.joinpath(*parts)
    require(not any(p.is_symlink() for p in [path, *path.parents]), 'symlink input: ' + relative)
    require(path.is_file(), 'missing input: ' + relative)
    return path


def load_script(root, relative, name):
    spec = importlib.util.spec_from_file_location(name, regular(root, relative))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def verify(root):
    raw_map = regular(root, MAP)
    proposal = json.loads(raw_map.read_text())
    require(proposal['schema_version'] == 1 and proposal['recipe'] == RECIPE
            and proposal['base_commit'] == BASE, 'wrong source proposal identity')
    require(proposal['release_admitted'] is False and proposal['publication_authorized'] is False
            and proposal['remaining_gates'] == list(PENDING), 'source proposal overstates admission')
    require(proposal['build_recipe'] == {
        'package': 'flightsim-app', 'target': 'x86_64-pc-windows-msvc',
        'toolchain': '1.93.0', 'profile': 'release', 'default_features': True,
        'additional_features': [], 'default_aircraft': 'light-single',
        'bundled_aircraft': ['light-single', 'swift-sport'], 'tonemapping': 'TonyMcMapface',
    }, 'wrong full release proposal recipe')
    inputs = proposal['source_sha256']
    require(MAP not in inputs and len(inputs) > 2000, 'incomplete or circular source input map')
    for relative, expected in inputs.items():
        require(digest(regular(root, relative)) == expected, 'source input changed: ' + relative)
    # Enumeration prevents arbitrary source siblings from entering the proposed
    # dependency boundary. Every accepted path is explicit in the reviewed map.
    for prefix in ('vendor/', 'crates/', 'tools/original-highwing/'):
        actual = {p.relative_to(root).as_posix() for p in (root/prefix).rglob('*')
                  if p.is_file() and '__pycache__' not in p.parts}
        declared = {p for p in inputs if p.startswith(prefix)}
        require(actual == declared, 'source boundary member set changed: ' + prefix)
    if (root/'.git').exists():
        tracked = set(subprocess.check_output(['git', '-C', str(root), 'ls-files', '-z']).decode().split('\0')) - {''}
        require(tracked == set(inputs) | {MAP}, 'tracked input map differs from candidate source')
    for relative, expected in proposal['unchanged_runtime_and_profile_sha256'].items():
        require(inputs.get(relative) == expected, 'protected runtime/profile source changed: ' + relative)
    legacy = load_script(root, 'scripts/check-swift-windows-candidate.py', 'full_source_legacy')
    require(len(legacy.INDEPENDENT_REPLAY_HASHES) == 102, 'independent replay anchor set changed')
    for relative, expected in legacy.INDEPENDENT_REPLAY_HASHES.items():
        require(inputs.get(relative) == expected, 'independent replay anchor changed: ' + relative)
    require(digest(root/'assets/aircraft/light_single.glb') == LIGHT, 'wrong original Light Single')
    require(digest(root/'assets/aircraft/light_single.json') == PROFILE, 'Light Single profile changed')
    adapter = load_script(root, 'tools/original-highwing/adapt_original_highwing.py', 'full_source_adapter')
    require(adapter.adapt((root/'assets/aircraft/meadow_trainer.glb').read_bytes())
            == (root/'assets/aircraft/light_single.glb').read_bytes(), 'installed mesh differs from deterministic adapter')
    geometry = load_script(root, 'tools/original-highwing/validate_candidate.py', 'full_source_geometry')
    asset_report = geometry.validate(root/'assets/aircraft/light_single.glb')
    manifest = json.loads((root/'docs/release/asset-rights-manifest.json').read_text())
    require(manifest['current_source_boundary'] == RECIPE, 'asset manifest belongs to another recipe')
    active_luts = {x['id'] for x in manifest['dependency_assets'] if x['package'] == 'bevy_core_pipeline'}
    require(active_luts == {'bevy-tony-mc-mapface', 'bevy-blender-filmic-lut'}, 'wrong current LUT rights records')
    core = root/'vendor/bevy_core_pipeline'
    luts = core/'src/tonemapping/luts'
    require({p.name for p in luts.glob('*.ktx2')} == set(LUTS), 'wrong source LUT payload set')
    for name, expected in LUTS.items():
        require(digest(luts/name) == expected, 'licensed LUT changed: ' + name)
    patch = tomllib.loads((root/'Cargo.toml').read_text())['patch']['crates-io']['bevy_core_pipeline']
    require(patch == {'path': 'vendor/bevy_core_pipeline'}, 'unreviewed core pipeline source route')
    require(not (root/'docs/release/release-authorization.json').exists(), 'separate publication authorization requires a new reviewed source state')
    return {'schema_version': 1, 'status': 'source_preparation_verified', 'recipe': RECIPE,
            'source_input_map_sha256': digest(raw_map), 'source_inputs': len(inputs),
            'unchanged_runtime_and_profile_inputs': len(proposal['unchanged_runtime_and_profile_sha256']),
            'independent_replay_anchors': 102, 'original_highwing_geometry': asset_report,
            'licensed_lut_sha256': LUTS, 'release_admitted': False,
            'publication_authorized': False, 'remaining_gates': list(PENDING),
            'limits': 'Static source preparation only; no Cargo build, native runtime, source admission, inventory approval, hosted archive or publication proof.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', type=Path, default=ROOT)
    parser.add_argument('--require-release-admission', action='store_true',
                        help='Always fails for this preparation-only schema; use genuine final release gates after review')
    args = parser.parse_args()
    try:
        result = verify(args.repo.resolve())
    except (ValueError, OSError, KeyError) as error:
        print(json.dumps({'status': 'source_preparation_rejected', 'error': str(error), 'release_admitted': False}))
        raise SystemExit(1)
    print(json.dumps(result, indent=2))
    if args.require_release_admission:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
