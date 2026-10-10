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
import signal
import struct
import subprocess
import sys
import tempfile
import tomllib
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
# Historical provenance, never refreshed to current source. The reviewed manifest
# below supersedes the monolithic replay pin without relabeling old evidence.
LEGACY_SOURCE_HASHES = {
    "assets/aircraft/light_single.json": "8cf101b6785a7ceaa32772f10e9bf7bfdea68898c9f9ac9fa744ccadde7a1e25",
    "crates/flightsim-fdm/src/aircraft.rs": "72091944bfff40abace7f10f05566d9a4394b96aa2cf767e149b54c996219671",
    "crates/flightsim-fdm/src/lib.rs": "a956e3046e906304e23e675d440ed20875ea5e47d1a3552158b8356f16b8ccc9",
    "crates/flightsim-sim/src/replay.rs": "0b783ceed247b984729021ae57c74b061936d627c04275a850e59079266a18c1",
    "crates/flightsim-app/src/aircraft_profile.rs": "59c6deb0db1822178b30a0ba4e2fcbcac9e2177e1f0f0851f54c74510f3c6da0",
}
REPLAY_CONTRACT_PATH = "scripts/replay-candidate-contract.json"
HISTORICAL_REPLAY_CONTRACT_ID = "swift-candidate-replay-v3-v1"
ALPHA22_REPLAY_CONTRACT_ID = "full-two-aircraft-alpha22-version-source-v1"
REPLAY_CONTRACT_ID = "full-two-aircraft-cockpit-source-v1"
REVIEWED_RUNTIME_SOURCE = "960c3126e6a4bc22b8d4acc6e6737f8f2b473bef"
REVIEWED_RUNTIME_TREE = "b3bbd57bc3819b1b1eda35ce4f8b476c52d1b24e"
SOURCE_ADMISSION_SCOPE = "source-only; native, whole-target and publication gates remain unqualified"
HISTORICAL_CONTRACT_HASHES = {'scripts/history/d918943-replay-candidate-contract.json': '2290d90e367b6a48432741b9570e80102a213d387e55d39140859075b6cf3cf4', 'scripts/history/d918943-analytical-swift-source-contract.json': '00e8d5a493fcd3402077b6ab8d5f57d63a729395c2ee26d2b49834ab65260a4a', 'scripts/history/d918943-analytical-swift-capture-contract.json': '2cbb6c46002f8a414ac8cee1ba75f3362024f7b16b80b89c2bb1cacb84264fc5'}
PRESERVED_RUNTIME_PATH = "scripts/full-two-aircraft-runtime-pins.json"
PRESERVED_RUNTIME_SHA256 = "c9b71ddad46396b3b6ba8961a17f8323a28788f3c641e51f5c47098846653667"
# The old reviewed tree is base provenance only. This exact migration owns the
# changed startup gate; historical runtime hashes never become current evidence.
COMPONENT_TERMS_MIGRATION_PATH = "scripts/component-terms-source-migration.json"
COMPONENT_TERMS_MIGRATION_SHA256 = "b41c6e7d163f98d0ddc4bddb3063dbdff82ea3462064b2b887954367b457bcc9"
COMPONENT_TERMS_MAIN_PATH = "crates/flightsim-app/src/main.rs"
COMPONENT_TERMS_HISTORY_PATH = "scripts/history/4d40f9a-flightsim-app-main.rs"
COMPONENT_TERMS_RUNTIME_PATHS = frozenset({
    "crates/flightsim-app/src/component_terms.rs",
    "crates/flightsim-app/src/component_terms_dialog.ps1",
})
COMPONENT_TERMS_DOCUMENT_PATHS = frozenset({
    "docs/release/components/MICROSOFT-COMPONENT-TERMS.txt",
    "docs/release/components/MICROSOFT-COMPONENT-TERMS.ja.txt",
    "docs/release/components/MICROSOFT-COMPONENT-NOTICE.txt",
})
COMPONENT_TERMS_LICENSE_PATHS = frozenset({"LICENSE-MIT", "LICENSE-APACHE"})
COMPONENT_TERMS_SOURCE_PATHS = (COMPONENT_TERMS_RUNTIME_PATHS | COMPONENT_TERMS_DOCUMENT_PATHS
                               | COMPONENT_TERMS_LICENSE_PATHS)
# A second bounded migration changes terrain forward interpolation only. The
# component-terms migration and all earlier runtime identities stay immutable.
TERRAIN_CENTROID_MIGRATION_PATH = "scripts/terrain-centroid-source-migration.json"
TERRAIN_CENTROID_MIGRATION_SHA256 = "203d43b06ef860813fd3f21f482f4751d3ad0b625872086a7eb76d3b91d04db5"
TERRAIN_CENTROID_DETAIL_PATH = "crates/flightsim-render/src/terrain_detail.rs"
TERRAIN_CENTROID_HISTORY_PATH = "scripts/history/42a7dde-terrain_detail.rs"
TERRAIN_CENTROID_RUNTIME_PATHS = frozenset({
    "crates/flightsim-render/src/terrain_interpolation_tests.rs",
})
TERRAIN_CENTROID_PREVIOUS_SHA256 = {
    'crates/flightsim-render/src/terrain_detail.rs': '6670c630aa58391ae05d5b70b847bf2df1f2b01c3805dc594628053b3a317aa5',
    'vendor/bevy_pbr/src/render/forward_io.wgsl': 'e140ef1063bf097432fc6866749e3fc2b3893dddcdb24ac1240de0f8fdd0ca8a',
    'vendor/bevy_pbr/FLIGHTSIM-MODIFICATION-NOTICE.txt': 'd038bd4936f7c7f402d0a1dbc1993c4945763d50bd500e1a37b5716f5d1e8287',
    'vendor/bevy_pbr/FLIGHTSIM-PATCHES.md': '79549aa643f47a8a4098815b0107ac90ae3915afcf31821234f272c9ffa2a164',
    'docs/release/analytical-modified-source-provenance.json': '010fb99b2073bc63b1a21eaeb36116e2d103f07340443090c940aff364f360e1',
    'docs/release/modified-source-patches/bevy_pbr-added-vendor.patch': 'f5f59e6dbfb208c5dbb7e6400bf277b870c23e453107e9c9e7ad7ab5652ceae6',
}
# This third migration changes only closed, nondegenerate three-source caps.
# Its base is an unpublished local checkpoint; checked-in witnesses suffice.
TERRAIN_STITCH_MIGRATION_PATH = "scripts/terrain-stitch-source-migration.json"
TERRAIN_STITCH_MIGRATION_SHA256 = "7e3bb3a090bb6ae345bea2795152d8368d72e88ea8352ea175e5ed38bb63ea02"
TERRAIN_STITCH_BASE_CONTRACT_PATH = "scripts/history/5eaaff3-replay-candidate-contract.json"
TERRAIN_STITCH_BASE_CONTRACT_SHA256 = "1b72efcc010124b0a02708f0c4b75c07c79a272e352ac697b71934fc9aa20869"
TERRAIN_STITCH_HISTORICAL_RELOCATIONS = {
    'crates/flightsim-world/src/seams.rs': 'scripts/history/5eaaff3-seams.rs',
    'crates/flightsim-world/tests/terrain_seam_geometry.rs': 'scripts/history/5eaaff3-terrain_seam_geometry.rs',
}
TERRAIN_STITCH_PREVIOUS_SHA256 = {
    'crates/flightsim-world/src/seams.rs': '4c69ca50946663ead1c82907793b37af75e54f886340c8986bfd4a573a8397ef',
    'crates/flightsim-world/tests/terrain_seam_geometry.rs': 'a88108b3c2e7dedb5d483d4578101d7e37f90b3d6cab0ec71e18aa51f2be929f',
}
TERRAIN_STITCH_SOURCE_PATHS = frozenset({
    TERRAIN_STITCH_MIGRATION_PATH, TERRAIN_STITCH_BASE_CONTRACT_PATH,
    *TERRAIN_STITCH_HISTORICAL_RELOCATIONS.values(),
})
# The fourth migration changes release versions only. The previous Cargo and
# all three source-contract witnesses retain their exact alpha.21 bytes.
ALPHA22_MIGRATION_PATH = "scripts/alpha22-version-source-migration.json"
ALPHA22_MIGRATION_SHA256 = "c35695a2319ca48b154617021ab9b48c43d7ae532d3f421e9a1058c4f7e4679a"
ALPHA22_BASE_CONTRACT_PATH = "scripts/history/0bc4a49-replay-candidate-contract.json"
ALPHA22_BASE_CONTRACT_HASHES = {
    "scripts/history/0bc4a49-replay-candidate-contract.json": "88a7a983376e227171243a69e3560f43af1a4326092f638f98bde64fef4accd7",
    "scripts/history/0bc4a49-analytical-swift-source-contract.json": "a0bb5a2e4b85b0481e882ac287a49c69dd737583155c930e127f18762d5aaaf6",
    "scripts/history/0bc4a49-analytical-swift-capture-contract.json": "7ba04750322d10c754c14ada52aa052d99bc80a97f37759e25de87e7ec3c2619",
}
ALPHA22_HISTORICAL_RELOCATIONS = {
    "Cargo.toml": "scripts/history/0bc4a49-Cargo.toml",
    "Cargo.lock": "scripts/history/0bc4a49-Cargo.lock",
}
ALPHA22_PREVIOUS_SHA256 = {
    "Cargo.toml": "d02e563c40d92d180cfa8ca321cf69b029285cd6c0da553e5209cf66ebb03300",
    "Cargo.lock": "198064f4ec49650db29b76de5834880de1e3fd9bc8e9860890eb3987703599b2",
}
ALPHA22_SOURCE_PATHS = frozenset({
    ALPHA22_MIGRATION_PATH, *ALPHA22_BASE_CONTRACT_HASHES,
    *ALPHA22_HISTORICAL_RELOCATIONS.values(),
})
ALPHA22_WORKSPACE_PACKAGES = frozenset({
    "flightsim-core", "flightsim-fdm", "flightsim-world", "flightsim-content",
    "flightsim-tilegen", "flightsim-assetgen", "flightsim-sim", "flightsim-render",
    "flightsim-app", "flightsim-audio", "flightsim-ui", "flightsim-input", "flightsim-net",
})
HISTORICAL_RUNTIME_RELOCATIONS = {
    COMPONENT_TERMS_MAIN_PATH: COMPONENT_TERMS_HISTORY_PATH,
    TERRAIN_CENTROID_DETAIL_PATH: TERRAIN_CENTROID_HISTORY_PATH,
    **TERRAIN_STITCH_HISTORICAL_RELOCATIONS,
}
# Fifth, source-only migration: four state-coupled cockpit replacements and
# two exact runtime helpers. The published base and all seven witnesses remain
# immutable; no source migration is native, rights or publication evidence.
COCKPIT_MIGRATION_PATH = "scripts/cockpit-source-migration.json"
COCKPIT_MIGRATION_SHA256 = "9086bfdc723889ca94a388e6b61111a2174703badf48e0afadc9375c78127814"
COCKPIT_BASE_CONTRACT_PATH = "scripts/history/be587384-replay-candidate-contract.json"
COCKPIT_BASE_CONTRACT_HASHES = {'scripts/history/be587384-replay-candidate-contract.json': 'd5575c552cdf2522da4a2e8a50c9b41ff800632dae55138700bbed3b165e7412', 'scripts/history/be587384-analytical-swift-source-contract.json': 'f018b328d06f464f7d30d3a272dba49ac00a98bd8ad96c7b846a6b09fc46a173', 'scripts/history/be587384-analytical-swift-capture-contract.json': 'ae3b0831127698b2f2fd38f1675176cee1a7a730b7dbd953d9e8c3d76df95eff'}
COCKPIT_HISTORICAL_RELOCATIONS = {'crates/flightsim-app/src/main.rs': 'scripts/history/be587384-flightsim-app-main.rs', 'crates/flightsim-app/src/aircraft_scene.rs': 'scripts/history/be587384-aircraft_scene.rs', 'crates/flightsim-render/src/cockpit.rs': 'scripts/history/be587384-cockpit.rs', 'crates/flightsim-ui/src/instruments.rs': 'scripts/history/be587384-instruments.rs'}
COCKPIT_PREVIOUS_SHA256 = {'crates/flightsim-app/src/main.rs': 'e7a8480d1a4f5af9a0e14c982fe1645ac94cc5748a9f224edc00e3f63d48aada', 'crates/flightsim-app/src/aircraft_scene.rs': '23cdbd0bc5fde9182c3704827fb997f68225c524871f0a00819642a93eb53f60', 'crates/flightsim-render/src/cockpit.rs': '769673b6a930280d657b90fde65821ba3443d8505da46d14d742a7e7a96a702d', 'crates/flightsim-ui/src/instruments.rs': '9295167b8c1c4089b441f92c36510884fc863a993d5f8882267ef32e9b1e2ab4'}
COCKPIT_RUNTIME_PATHS = frozenset(['crates/flightsim-app/src/cockpit_runtime.rs', 'crates/flightsim-render/src/cockpit/texture.rs'])
COCKPIT_SOURCE_PATHS = frozenset({
    COCKPIT_MIGRATION_PATH, *COCKPIT_BASE_CONTRACT_HASHES,
    *COCKPIT_HISTORICAL_RELOCATIONS.values(), *COCKPIT_RUNTIME_PATHS,
})
# The oldest main.rs witness remains the component-terms witness. Other newly
# replaced preserved files now have their own exact published-base witnesses.
CURRENT_HISTORICAL_RUNTIME_RELOCATIONS = {
    **COCKPIT_HISTORICAL_RELOCATIONS, **HISTORICAL_RUNTIME_RELOCATIONS,
}
# The reviewed additive jet/turboprop exports change this file, but no legacy FDM
# body. Keep a literal full-file guard; the historical lib hash above never moves.
REVIEWED_ADDITIVE_FDM_LIB_SHA256 = "2328631f895921e4152de6f8107c4d6f07ba764b6f6225189a4b826c6f9f013c"
# Whole files, not selected functions or text-normalized projections. Adding a
# helper or changing any reviewed implementation requires a boundary review.
REPLAY_CONTRACT_PATHS = {
    'assets/aircraft/light_single.json',
    'assets/aircraft/swift_sport.json',
    'crates/flightsim-fdm/src/aircraft.rs',
    'crates/flightsim-fdm/src/landing_gear.rs',
    'crates/flightsim-fdm/src/lib.rs',
    'crates/flightsim-sim/Cargo.toml',
    'crates/flightsim-sim/src/replay.rs',
    'crates/flightsim-sim/src/replay/identity.rs',
    'crates/flightsim-sim/src/replay/current.rs',
    'crates/flightsim-sim/src/replay/player.rs',
    'crates/flightsim-sim/src/lib.rs',
    'crates/flightsim-sim/src/weather.rs',
    'crates/flightsim-sim/examples/record_takeoff.rs',
    'crates/flightsim-app/src/aircraft_profile.rs',
    'crates/flightsim-app/src/distribution.rs',
    'crates/flightsim-app/src/main.rs',
    # Rebase following delegates state transport to the input camera rig.
    # Keep the whole helper and the actual app reset/basis witnesses together.
    'crates/flightsim-input/src/camera.rs',
    'crates/flightsim-app/src/runtime_tests.rs',
    'crates/flightsim-app/src/replay_policy.rs',
    'crates/flightsim-app/src/replay_runtime.rs',
    'crates/flightsim-app/src/replay_migration_tests.rs',
    'crates/flightsim-ui/src/replay.rs',
    'crates/flightsim-sim/tests/replay_complete_identity.rs',
    'crates/flightsim-sim/tests/replay_v3.rs',
    'crates/flightsim-sim/tests/replay_player_versions.rs',
    "crates/flightsim-app/src/weather_runtime.rs",
    "crates/flightsim-app/src/world_runtime.rs",
    "crates/flightsim-app/src/cloud_runtime.rs",
    "crates/flightsim-app/src/region_runtime.rs",
    "crates/flightsim-ui/src/world_map.rs",
    # Typed app dispatch and its UI/traffic adapters now sit on the legacy path.
    # ModelReplayFile must delegate original bytes to the old codecs and never
    # admit a jet replay as a Swift flight. These pins do not qualify jet flights.
    "crates/flightsim-app/src/flight_session.rs",
    "crates/flightsim-app/src/traffic_runtime.rs",
    "crates/flightsim-app/src/controls_runtime_tests.rs",
    "crates/flightsim-app/src/jet_runtime_tests.rs",
    "crates/flightsim-sim/src/replay_v4.rs",
    "crates/flightsim-ui/src/lib.rs",
    "crates/flightsim-ui/src/tutorial.rs",
    "crates/flightsim-ui/src/top_layout_tests.rs",
    # Cockpit replay help shares the actual six-pack width. Bind the complete
    # geometry owner, not a projected constant or a synthetic layout witness.
    "crates/flightsim-ui/src/instruments.rs",
    # The staged picker now owns Swift scene/session replacement. Its scene
    # builder is shared by initial startup, and audio replacement owns the
    # actual synthesized source rather than only its display/settings value.
    "crates/flightsim-app/src/aircraft_picker_runtime.rs",
    "crates/flightsim-app/src/aircraft_picker_tests.rs",
    "crates/flightsim-app/src/aircraft_scene.rs",
    "crates/flightsim-audio/src/lib.rs",
    "crates/flightsim-audio/src/lifecycle_tests.rs",
    # Explicit pending wind/turbulence now own the environment copied into a
    # prepared Swift session and recorder. Bind their exact-value/seed/default
    # boundary and the reviewed app, input and physical/replay witnesses.
    "crates/flightsim-app/src/conditions_runtime.rs",
    "crates/flightsim-app/src/conditions_runtime_tests.rs",
    "crates/flightsim-ui/src/wind_settings.rs",
    "crates/flightsim-ui/src/wind_settings_tests.rs",
    "crates/flightsim-sim/tests/wind_force_contract.rs",
    "crates/flightsim-sim/tests/wind_replay_rewind.rs",
    # The development-only turboprop entry points own admission before any
    # source/session read. Explicit lateral trim also runs on the Swift path;
    # its zero-bias compatibility and input schema witnesses remain required.
    "crates/flightsim-app/src/turboprop_session.rs",
    "crates/flightsim-app/src/turboprop_runtime_tests.rs",
    "crates/flightsim-app/src/turboprop_lifecycle_tests.rs",
    "crates/flightsim-input/src/lib.rs",
    "crates/flightsim-input/src/lateral_trim_regression.rs",
    "crates/flightsim-input/tests/fixtures/legacy-input-v1.json",
    # The full reference now delegates to a scrollable pause panel. Bind that
    # implementation and the actual wrapped-footer layout witness together.
    "crates/flightsim-ui/src/pause.rs",
    "crates/flightsim-ui/src/attribution_layout_tests.rs",
    # The same staged map now delegates presentation to measured scroll/reflow
    # helpers. Bind its child region controls and actual glyph/pointer witnesses;
    # these pins do not change the app's transaction, replay or source admission.
    "crates/flightsim-ui/src/regions.rs",
    "crates/flightsim-ui/src/world_map_layout.rs",
    "crates/flightsim-ui/src/world_map_layout_tests.rs",
    # Ordinary candidate captures now wait for a committed CPU world. Bind both
    # the request gate and its opt-in selector observation, including witnesses.
    # These pins do not attest GPU completion or a successful Windows capture.
    "crates/flightsim-app/src/screen_capture.rs",
    "crates/flightsim-render/src/terrain_selection.rs",
}
# Separate pure law/profile/replay review; these pins grant no new app or
# commercial aircraft admission. Retained forward and original-token inputs are
# whole-file dependencies of the new explicit boundary, not converted old laws.
NEAR_STATIC_FOUNDATION_PATHS = {
    'crates/flightsim-fdm/Cargo.toml',
    'crates/flightsim-fdm/src/aero.rs',
    'crates/flightsim-fdm/src/atmosphere.rs',
    'crates/flightsim-fdm/src/controls.rs',
    'crates/flightsim-fdm/src/definition.rs',
    'crates/flightsim-fdm/src/gravity.rs',
    'crates/flightsim-fdm/src/state.rs',
    'crates/flightsim-fdm/src/subsonic/config.rs',
    'crates/flightsim-fdm/src/subsonic/jet.rs',
    'crates/flightsim-fdm/src/subsonic/mod.rs',
    'crates/flightsim-fdm/src/subsonic/runtime.rs',
    'crates/flightsim-fdm/src/subsonic/schedule.rs',
    'crates/flightsim-fdm/src/turboprop/config.rs',
    'crates/flightsim-fdm/src/turboprop/governor.rs',
    'crates/flightsim-fdm/src/turboprop/mod.rs',
    'crates/flightsim-fdm/src/turboprop/near_static/config.rs',
    'crates/flightsim-fdm/src/turboprop/near_static/mod.rs',
    'crates/flightsim-fdm/src/turboprop/near_static/propeller.rs',
    'crates/flightsim-fdm/src/turboprop/near_static/runtime.rs',
    'crates/flightsim-fdm/src/turboprop/near_static/tests.rs',
    'crates/flightsim-fdm/src/turboprop/power.rs',
    'crates/flightsim-fdm/src/turboprop/propeller.rs',
    'crates/flightsim-fdm/src/turboprop/runtime.rs',
    'crates/flightsim-fdm/src/turboprop/tests.rs',
    'crates/flightsim-fdm/src/turbulence.rs',
    'crates/flightsim-fdm/tests/support/cedar_boundary_witness.rs',
    'crates/flightsim-sim/examples/cedar_near_static_qualification.rs',
    'crates/flightsim-sim/examples/support/cedar_law1_baseline.rs',
    'crates/flightsim-sim/examples/support/cedar_near_static.rs',
    'crates/flightsim-sim/src/aircraft_profile.rs',
    'crates/flightsim-sim/src/aircraft_profile/exact.rs',
    'crates/flightsim-sim/src/aircraft_profile/metadata.rs',
    'crates/flightsim-sim/src/aircraft_profile/tests.rs',
    'crates/flightsim-sim/src/aircraft_profile/wire.rs',
    'crates/flightsim-sim/src/aircraft_profile_v3/mod.rs',
    'crates/flightsim-sim/src/aircraft_profile_v3/wire.rs',
    'crates/flightsim-sim/src/aircraft_profile_v4/mod.rs',
    'crates/flightsim-sim/src/aircraft_profile_v4/wire.rs',
    'crates/flightsim-sim/src/model_identity.rs',
    'crates/flightsim-sim/src/near_static_turboprop_identity.rs',
    'crates/flightsim-sim/src/near_static_turboprop_simulation/mod.rs',
    'crates/flightsim-sim/src/near_static_turboprop_simulation/parked.rs',
    'crates/flightsim-sim/src/replay_v5.rs',
    'crates/flightsim-sim/src/replay_v6/mod.rs',
    'crates/flightsim-sim/src/replay_v6/terminal.rs',
    'crates/flightsim-sim/src/turboprop_identity.rs',
    'crates/flightsim-sim/src/turboprop_simulation/mod.rs',
    'crates/flightsim-sim/src/turboprop_simulation/parked.rs',
    'crates/flightsim-sim/src/turboprop_simulation/presentation.rs',
    'crates/flightsim-sim/tests/aircraft_profile_v4.rs',
    'crates/flightsim-sim/tests/cedar_near_static_qualification.rs',
    'crates/flightsim-sim/tests/near_static_turboprop_codec_v6.rs',
    'crates/flightsim-sim/tests/near_static_turboprop_common/mod.rs',
    'crates/flightsim-sim/tests/near_static_turboprop_model_identity.rs',
    'crates/flightsim-sim/tests/near_static_turboprop_replay_v6.rs',
    'schemas/aircraft-profile-v4.schema.json',
    'schemas/tests/aircraft-profile-v4-cases.json',
    'schemas/tests/test_aircraft_profile_v4_schema.py',
    'tools/qualification/analyze_cedar_near_static.py',
    'tools/qualification/build_cedar_near_static_rows.py',
}
REPLAY_CONTRACT_PATHS |= NEAR_STATIC_FOUNDATION_PATHS
# Exact independently reviewed profile-4 app ownership and read-only presentation.
# This source binding admits no Cedar preset, commercial aircraft or native result.
NEAR_STATIC_APP_PATHS = {
    "crates/flightsim-app/src/aircraft_picker_runtime.rs",
    "crates/flightsim-app/src/aircraft_picker_tests.rs",
    "crates/flightsim-app/src/aircraft_profile.rs",
    "crates/flightsim-app/src/distribution.rs",
    "crates/flightsim-app/src/flight_session.rs",
    "crates/flightsim-app/src/main.rs",
    "crates/flightsim-app/src/near_static_turboprop_session.rs",
    "crates/flightsim-app/src/nearstatic_lifecycle_tests.rs",
    "crates/flightsim-app/src/nearstatic_runtime_tests.rs",
    "crates/flightsim-app/src/replay_policy.rs",
    "crates/flightsim-app/src/turboprop_session.rs",
    "crates/flightsim-sim/src/near_static_turboprop_simulation/mod.rs",
    "crates/flightsim-sim/src/near_static_turboprop_simulation/presentation.rs",
}
REPLAY_CONTRACT_PATHS |= NEAR_STATIC_APP_PATHS
# Reviewed authored visibility/cloud-base drafts join the same transactional
# Swift launch boundary. Bind complete UI/app implementations and the actual
# all-family recording witnesses; this does not widen aircraft/release gates.
AUTHORED_WEATHER_PATHS = {
    "crates/flightsim-app/src/aircraft_picker_runtime.rs",
    "crates/flightsim-app/src/aircraft_picker_tests.rs",
    "crates/flightsim-app/src/main.rs",
    "crates/flightsim-app/src/weather_draft.rs",
    "crates/flightsim-app/src/weather_runtime.rs",
    "crates/flightsim-app/src/weather_editor_tests.rs",
    "crates/flightsim-app/src/custom_weather_replay_tests.rs",
    "crates/flightsim-app/src/world_runtime.rs",
    "crates/flightsim-ui/src/lib.rs",
    "crates/flightsim-ui/src/weather_settings.rs",
    "crates/flightsim-ui/src/weather_settings_tests.rs",
    "crates/flightsim-ui/src/world_map.rs",
    "crates/flightsim-ui/src/world_map_layout.rs",
    "crates/flightsim-ui/src/world_map_layout_tests.rs",
}
REPLAY_CONTRACT_PATHS |= AUTHORED_WEATHER_PATHS
# Explicit ordinary-build aircraft package commands exit before startup and are
# rejected by commercial-staging before I/O. Bind their complete offline closure,
# shared terrain mechanics, existing axes/fit owner and actual fixture witnesses.
# This protects source isolation; it grants no package/runtime/release admission.
AIRCRAFT_PACKAGE_PATHS = {
    "assets/aircraft/swift_sport.glb",
    "crates/flightsim-app/src/aircraft_package_cli.rs",
    "crates/flightsim-content/Cargo.toml",
    "crates/flightsim-content/src/aircraft/glb.rs",
    "crates/flightsim-content/src/aircraft/manifest.rs",
    "crates/flightsim-content/src/aircraft/mod.rs",
    "crates/flightsim-content/src/archive.rs",
    "crates/flightsim-content/src/install.rs",
    "crates/flightsim-content/src/lib.rs",
    "crates/flightsim-content/src/manifest.rs",
    "crates/flightsim-content/tests/aircraft_packages.rs",
    "crates/flightsim-content/tests/packages.rs",
    "crates/flightsim-content/tests/support/aircraft_zip.rs",
    "crates/flightsim-render/src/model.rs",
    "docs/examples/aircraft-packages/swift-sport-1.0.0.zip",
    "docs/examples/aircraft-packages/swift/LICENSE-APACHE.txt",
    "docs/examples/aircraft-packages/swift/LICENSE-MIT.txt",
    "docs/examples/aircraft-packages/swift/PROVENANCE.md",
    "docs/examples/aircraft-packages/swift/manifest.json",
}
REPLAY_CONTRACT_PATHS |= AIRCRAFT_PACKAGE_PATHS
# Reviewed render-only regional-root discovery. The five changed app/content/
# renderer owners are already pinned above. Bind the complete new helper/input
# closure, its exact topology/radius primitive, and regression/benchmark witnesses.
# These source pins admit neither package replay nor a binary distribution.
REGIONAL_COVERAGE_PATHS = {
    "crates/flightsim-core/src/geodetic.rs",
    "crates/flightsim-world/src/coverage.rs",
    "crates/flightsim-world/src/draw_distance.rs",
    "crates/flightsim-world/src/draw_distance_tests.rs",
    "crates/flightsim-world/src/global.rs",
    "crates/flightsim-world/src/lib.rs",
    "crates/flightsim-world/src/lod.rs",
    "crates/flightsim-world/src/terrain.rs",
    "crates/flightsim-world/src/tile.rs",
    "crates/flightsim-world/benches/terrain.rs",
    "crates/flightsim-content/tests/balzers_package.rs",
}
REPLAY_CONTRACT_PATHS |= REGIONAL_COVERAGE_PATHS
# Capture-only render admission depends on the reviewed complete scheduling
# helper and the exact Bevy/wgpu build inputs, not just the app call site.
# These pins cannot establish GPU progress, a native capture or release rights.
CAPTURE_BACKPRESSURE_PATHS = {
    "Cargo.toml",
    "Cargo.lock",
    "crates/flightsim-app/Cargo.toml",
    "crates/flightsim-app/src/capture_backpressure.rs",
}
REPLAY_CONTRACT_PATHS |= CAPTURE_BACKPRESSURE_PATHS
# Batch preparation admission is a separate CPU Render-opportunity handshake.
# Bind its complete engine hooks and pure generation state, including tests.
CAPTURE_ADMISSION_PATHS = {
    "crates/flightsim-app/src/capture_admission.rs",
    "crates/flightsim-app/src/capture_admission_state.rs",
}
REPLAY_CONTRACT_PATHS |= CAPTURE_ADMISSION_PATHS
# Reviewed local replacements lose registry-checksum authentication. Bind the
# complete package, witness, provenance and export-policy files explicitly.
# These paths grant neither licence clearance nor native/release qualification.
VENDORED_PACKAGE_PATHS = {
    'vendor/bevy_pbr/Cargo.lock',
    'vendor/bevy_pbr/Cargo.toml',
    'vendor/bevy_pbr/Cargo.toml.orig',
    'vendor/bevy_pbr/FLIGHTSIM-MODIFICATION-NOTICE.txt',
    'vendor/bevy_pbr/FLIGHTSIM-PARALLAX-README.md',
    'vendor/bevy_pbr/FLIGHTSIM-PATCHES.md',
    'vendor/bevy_pbr/FLIGHTSIM-UPSTREAM-SOURCE.json',
    'vendor/bevy_pbr/LICENSE-APACHE',
    'vendor/bevy_pbr/LICENSE-FLIGHTSIM-APACHE',
    'vendor/bevy_pbr/LICENSE-FLIGHTSIM-MIT',
    'vendor/bevy_pbr/LICENSE-MIT',
    'vendor/bevy_pbr/README.md',
    'vendor/bevy_pbr/src/atmosphere/aerial_view_lut.wgsl',
    'vendor/bevy_pbr/src/atmosphere/bindings.wgsl',
    'vendor/bevy_pbr/src/atmosphere/bruneton_functions.wgsl',
    'vendor/bevy_pbr/src/atmosphere/environment.rs',
    'vendor/bevy_pbr/src/atmosphere/environment.wgsl',
    'vendor/bevy_pbr/src/atmosphere/functions.wgsl',
    'vendor/bevy_pbr/src/atmosphere/mod.rs',
    'vendor/bevy_pbr/src/atmosphere/multiscattering_lut.wgsl',
    'vendor/bevy_pbr/src/atmosphere/node.rs',
    'vendor/bevy_pbr/src/atmosphere/render_sky.wgsl',
    'vendor/bevy_pbr/src/atmosphere/resources.rs',
    'vendor/bevy_pbr/src/atmosphere/sky_view_lut.wgsl',
    'vendor/bevy_pbr/src/atmosphere/transmittance_lut.wgsl',
    'vendor/bevy_pbr/src/atmosphere/types.wgsl',
    'vendor/bevy_pbr/src/bluenoise/stbn.ktx2',
    'vendor/bevy_pbr/src/cluster.rs',
    'vendor/bevy_pbr/src/components.rs',
    'vendor/bevy_pbr/src/decal/clustered.rs',
    'vendor/bevy_pbr/src/decal/clustered.wgsl',
    'vendor/bevy_pbr/src/decal/forward.rs',
    'vendor/bevy_pbr/src/decal/forward_decal.wgsl',
    'vendor/bevy_pbr/src/decal/mod.rs',
    'vendor/bevy_pbr/src/deferred/deferred_lighting.wgsl',
    'vendor/bevy_pbr/src/deferred/mod.rs',
    'vendor/bevy_pbr/src/deferred/pbr_deferred_functions.wgsl',
    'vendor/bevy_pbr/src/deferred/pbr_deferred_types.wgsl',
    'vendor/bevy_pbr/src/diagnostic.rs',
    'vendor/bevy_pbr/src/extended_material.rs',
    'vendor/bevy_pbr/src/fog.rs',
    'vendor/bevy_pbr/src/lib.rs',
    'vendor/bevy_pbr/src/light_probe/copy.wgsl',
    'vendor/bevy_pbr/src/light_probe/downsample.wgsl',
    'vendor/bevy_pbr/src/light_probe/environment_filter.wgsl',
    'vendor/bevy_pbr/src/light_probe/environment_map.rs',
    'vendor/bevy_pbr/src/light_probe/environment_map.wgsl',
    'vendor/bevy_pbr/src/light_probe/generate.rs',
    'vendor/bevy_pbr/src/light_probe/irradiance_volume.rs',
    'vendor/bevy_pbr/src/light_probe/irradiance_volume.wgsl',
    'vendor/bevy_pbr/src/light_probe/light_probe.wgsl',
    'vendor/bevy_pbr/src/light_probe/mod.rs',
    'vendor/bevy_pbr/src/lightmap/lightmap.wgsl',
    'vendor/bevy_pbr/src/lightmap/mod.rs',
    'vendor/bevy_pbr/src/material.rs',
    'vendor/bevy_pbr/src/material_bind_groups.rs',
    'vendor/bevy_pbr/src/medium.rs',
    'vendor/bevy_pbr/src/mesh_material.rs',
    'vendor/bevy_pbr/src/meshlet/asset.rs',
    'vendor/bevy_pbr/src/meshlet/clear_visibility_buffer.wgsl',
    'vendor/bevy_pbr/src/meshlet/cull_bvh.wgsl',
    'vendor/bevy_pbr/src/meshlet/cull_clusters.wgsl',
    'vendor/bevy_pbr/src/meshlet/cull_instances.wgsl',
    'vendor/bevy_pbr/src/meshlet/dummy_visibility_buffer_resolve.wgsl',
    'vendor/bevy_pbr/src/meshlet/fill_counts.wgsl',
    'vendor/bevy_pbr/src/meshlet/from_mesh.rs',
    'vendor/bevy_pbr/src/meshlet/instance_manager.rs',
    'vendor/bevy_pbr/src/meshlet/material_pipeline_prepare.rs',
    'vendor/bevy_pbr/src/meshlet/material_shade_nodes.rs',
    'vendor/bevy_pbr/src/meshlet/meshlet_bindings.wgsl',
    'vendor/bevy_pbr/src/meshlet/meshlet_cull_shared.wgsl',
    'vendor/bevy_pbr/src/meshlet/meshlet_mesh_manager.rs',
    'vendor/bevy_pbr/src/meshlet/meshlet_mesh_material.wgsl',
    'vendor/bevy_pbr/src/meshlet/mod.rs',
    'vendor/bevy_pbr/src/meshlet/persistent_buffer.rs',
    'vendor/bevy_pbr/src/meshlet/persistent_buffer_impls.rs',
    'vendor/bevy_pbr/src/meshlet/pipelines.rs',
    'vendor/bevy_pbr/src/meshlet/remap_1d_to_2d_dispatch.wgsl',
    'vendor/bevy_pbr/src/meshlet/resolve_render_targets.wgsl',
    'vendor/bevy_pbr/src/meshlet/resource_manager.rs',
    'vendor/bevy_pbr/src/meshlet/visibility_buffer_hardware_raster.wgsl',
    'vendor/bevy_pbr/src/meshlet/visibility_buffer_raster_node.rs',
    'vendor/bevy_pbr/src/meshlet/visibility_buffer_resolve.wgsl',
    'vendor/bevy_pbr/src/meshlet/visibility_buffer_software_raster.wgsl',
    'vendor/bevy_pbr/src/parallax.rs',
    'vendor/bevy_pbr/src/pbr_material.rs',
    'vendor/bevy_pbr/src/prepass/mod.rs',
    'vendor/bevy_pbr/src/prepass/prepass.wgsl',
    'vendor/bevy_pbr/src/prepass/prepass_bindings.rs',
    'vendor/bevy_pbr/src/prepass/prepass_bindings.wgsl',
    'vendor/bevy_pbr/src/prepass/prepass_io.wgsl',
    'vendor/bevy_pbr/src/prepass/prepass_utils.wgsl',
    'vendor/bevy_pbr/src/render/build_indirect_params.wgsl',
    'vendor/bevy_pbr/src/render/clustered_forward.wgsl',
    'vendor/bevy_pbr/src/render/fog.rs',
    'vendor/bevy_pbr/src/render/fog.wgsl',
    'vendor/bevy_pbr/src/render/forward_io.wgsl',
    'vendor/bevy_pbr/src/render/gpu_preprocess.rs',
    'vendor/bevy_pbr/src/render/light.rs',
    'vendor/bevy_pbr/src/render/mesh.rs',
    'vendor/bevy_pbr/src/render/mesh.wgsl',
    'vendor/bevy_pbr/src/render/mesh_bindings.rs',
    'vendor/bevy_pbr/src/render/mesh_bindings.wgsl',
    'vendor/bevy_pbr/src/render/mesh_functions.wgsl',
    'vendor/bevy_pbr/src/render/mesh_preprocess.wgsl',
    'vendor/bevy_pbr/src/render/mesh_types.wgsl',
    'vendor/bevy_pbr/src/render/mesh_view_bindings.rs',
    'vendor/bevy_pbr/src/render/mesh_view_bindings.wgsl',
    'vendor/bevy_pbr/src/render/mesh_view_types.wgsl',
    'vendor/bevy_pbr/src/render/mod.rs',
    'vendor/bevy_pbr/src/render/morph.rs',
    'vendor/bevy_pbr/src/render/morph.wgsl',
    'vendor/bevy_pbr/src/render/occlusion_culling.wgsl',
    'vendor/bevy_pbr/src/render/parallax_mapping.wgsl',
    'vendor/bevy_pbr/src/render/pbr.wgsl',
    'vendor/bevy_pbr/src/render/pbr_ambient.wgsl',
    'vendor/bevy_pbr/src/render/pbr_bindings.wgsl',
    'vendor/bevy_pbr/src/render/pbr_fragment.wgsl',
    'vendor/bevy_pbr/src/render/pbr_functions.wgsl',
    'vendor/bevy_pbr/src/render/pbr_lighting.wgsl',
    'vendor/bevy_pbr/src/render/pbr_prepass.wgsl',
    'vendor/bevy_pbr/src/render/pbr_prepass_functions.wgsl',
    'vendor/bevy_pbr/src/render/pbr_transmission.wgsl',
    'vendor/bevy_pbr/src/render/pbr_types.wgsl',
    'vendor/bevy_pbr/src/render/reset_indirect_batch_sets.wgsl',
    'vendor/bevy_pbr/src/render/rgb9e5.wgsl',
    'vendor/bevy_pbr/src/render/shadow_sampling.wgsl',
    'vendor/bevy_pbr/src/render/shadows.wgsl',
    'vendor/bevy_pbr/src/render/skin.rs',
    'vendor/bevy_pbr/src/render/skinning.wgsl',
    'vendor/bevy_pbr/src/render/utils.wgsl',
    'vendor/bevy_pbr/src/render/view_transformations.wgsl',
    'vendor/bevy_pbr/src/render/wireframe.wgsl',
    'vendor/bevy_pbr/src/ssao/mod.rs',
    'vendor/bevy_pbr/src/ssao/preprocess_depth.wgsl',
    'vendor/bevy_pbr/src/ssao/spatial_denoise.wgsl',
    'vendor/bevy_pbr/src/ssao/ssao.wgsl',
    'vendor/bevy_pbr/src/ssao/ssao_utils.wgsl',
    'vendor/bevy_pbr/src/ssr/mod.rs',
    'vendor/bevy_pbr/src/ssr/raymarch.wgsl',
    'vendor/bevy_pbr/src/ssr/ssr.wgsl',
    'vendor/bevy_pbr/src/volumetric_fog/mod.rs',
    'vendor/bevy_pbr/src/volumetric_fog/render.rs',
    'vendor/bevy_pbr/src/volumetric_fog/volumetric_fog.wgsl',
    'vendor/bevy_pbr/src/wireframe.rs',
    'vendor/bevy_pbr/third-party-notices/EA-fastnoise-LICENSE.txt',
    'vendor/bevy_pbr/third-party-notices/EA-fastnoise-NOTICE.txt',
    'vendor/bevy_pbr/third-party-notices/src__atmosphere__bruneton_functions.wgsl.notice-0.txt',
    'vendor/bevy_pbr/third-party-notices/src__ssr__raymarch.wgsl.notice-0.txt',
    'vendor/zune-jpeg/.gitignore',
    'vendor/zune-jpeg/Benches.md',
    'vendor/zune-jpeg/Cargo.lock',
    'vendor/zune-jpeg/Cargo.toml',
    'vendor/zune-jpeg/Cargo.toml.orig',
    'vendor/zune-jpeg/Changelog.md',
    'vendor/zune-jpeg/FLIGHTSIM-MODIFICATION-NOTICE.txt',
    'vendor/zune-jpeg/FLIGHTSIM-PATCHES.md',
    'vendor/zune-jpeg/FLIGHTSIM-TRANSPOSE-PROVENANCE.md',
    'vendor/zune-jpeg/FLIGHTSIM-TRANSPOSE-README.md',
    'vendor/zune-jpeg/FLIGHTSIM-UPSTREAM-SOURCE.json',
    'vendor/zune-jpeg/LICENSE-APACHE',
    'vendor/zune-jpeg/LICENSE-FLIGHTSIM-APACHE',
    'vendor/zune-jpeg/LICENSE-FLIGHTSIM-MIT',
    'vendor/zune-jpeg/LICENSE-MIT',
    'vendor/zune-jpeg/LICENSE-ZLIB',
    'vendor/zune-jpeg/README.md',
    'vendor/zune-jpeg/src/bitstream.rs',
    'vendor/zune-jpeg/src/color_convert.rs',
    'vendor/zune-jpeg/src/color_convert/avx.rs',
    'vendor/zune-jpeg/src/color_convert/neon64.rs',
    'vendor/zune-jpeg/src/color_convert/scalar.rs',
    'vendor/zune-jpeg/src/components.rs',
    'vendor/zune-jpeg/src/decoder.rs',
    'vendor/zune-jpeg/src/errors.rs',
    'vendor/zune-jpeg/src/flightsim_array_transpose.rs',
    'vendor/zune-jpeg/src/flightsim_transpose_tests.rs',
    'vendor/zune-jpeg/src/flightsim_transpose_tests_body.rs',
    'vendor/zune-jpeg/src/headers.rs',
    'vendor/zune-jpeg/src/huffman.rs',
    'vendor/zune-jpeg/src/idct.rs',
    'vendor/zune-jpeg/src/idct/avx2.rs',
    'vendor/zune-jpeg/src/idct/neon.rs',
    'vendor/zune-jpeg/src/idct/scalar.rs',
    'vendor/zune-jpeg/src/lib.rs',
    'vendor/zune-jpeg/src/marker.rs',
    'vendor/zune-jpeg/src/mcu.rs',
    'vendor/zune-jpeg/src/mcu_prog.rs',
    'vendor/zune-jpeg/src/misc.rs',
    'vendor/zune-jpeg/src/unsafe_utils.rs',
    'vendor/zune-jpeg/src/unsafe_utils_avx2.rs',
    'vendor/zune-jpeg/src/unsafe_utils_neon.rs',
    'vendor/zune-jpeg/src/upsampler.rs',
    'vendor/zune-jpeg/src/upsampler/avx2.rs',
    'vendor/zune-jpeg/src/upsampler/neon.rs',
    'vendor/zune-jpeg/src/upsampler/portable_simd.rs',
    'vendor/zune-jpeg/src/upsampler/scalar.rs',
    'vendor/zune-jpeg/src/worker.rs',
    'vendor/zune-jpeg/third-party-notices/STANFORD-NOTICE.txt',
    'vendor/zune-jpeg/third-party-notices/libjpeg-turbo-LICENSE.ijg',
    'vendor/zune-jpeg/third-party-notices/libjpeg-turbo-acknowledgement-NOTICE.txt',
    'vendor/zune-jpeg/third-party-notices/libjpeg-turbo-adaptation-NOTICE.txt',
    'vendor/zune-jpeg/third-party-notices/libjpeg-turbo-jdhuff-copyright-header.txt',
    'vendor/zune-jpeg/third-party-notices/libultrahdr-Apache-2.0-LICENSE.txt',
    'vendor/zune-jpeg/third-party-notices/libultrahdr-copyright-headers.txt',
    'vendor/zune-jpeg/third-party-notices/libultrahdr-upstream-Adobe-NOTICE.txt',
    'vendor/zune-jpeg/third-party-notices/stb-MIT-NOTICE.txt',
    'vendor/zune-jpeg/third-party-notices/stb-upstream-dual-license.txt',
}

REPLACEMENT_WITNESS_PATHS = {
    'tools/validate-jpeg-replacement/Cargo.lock',
    'tools/validate-jpeg-replacement/Cargo.toml',
    'tools/validate-jpeg-replacement/README.md',
    'tools/validate-jpeg-replacement/src/main.rs',
    'tools/validate-parallax-math/Cargo.lock',
    'tools/validate-parallax-math/Cargo.toml',
    'tools/validate-parallax-math/README.md',
    'tools/validate-parallax-math/shader/parallaxed_uv.wgsl',
    'tools/validate-parallax-math/src/lib.rs',
    'tools/validate-parallax-math/tests/wgsl_validation.rs',
    'tools/validate-parallax-math/validate.sh',
    'tools/validate-parallax-replacement/.gitignore',
    'tools/validate-parallax-replacement/Cargo.lock',
    'tools/validate-parallax-replacement/Cargo.toml',
    'tools/validate-parallax-replacement/README.md',
    'tools/validate-parallax-replacement/inputs.json',
    'tools/validate-parallax-replacement/reference-inputs.json',
    'tools/validate-parallax-replacement/shader/integration.wgsl',
    'tools/validate-parallax-replacement/src/main.rs',
    'tools/validate-parallax-replacement/validate.sh',
}

MODIFIED_SOURCE_POLICY_PATHS = {
    '.gitattributes',
    'docs/release/analytical-modified-source-provenance.json',
    'docs/release/analytical-source-header-evidence.json',
    'docs/release/modified-source-patches/bevy_pbr-added-vendor.patch',
    'docs/release/modified-source-patches/zune-jpeg-added-vendor.patch',
    'docs/release/original-routine-remediation.md',
    'docs/release/source-archive-policy.md',
    'scripts/check-source-archive.py',
    'scripts/test_source_archive.py',
}

REPLAY_CONTRACT_PATHS |= VENDORED_PACKAGE_PATHS | REPLACEMENT_WITNESS_PATHS | MODIFIED_SOURCE_POLICY_PATHS

# This is a new source admission, not a new runtime/aircraft acceptance.
# Keep the previous 404-file boundary inspectable and version it explicitly.
HISTORICAL_REPLAY_CONTRACT_PATHS = frozenset(REPLAY_CONTRACT_PATHS)
CORE_PIPELINE_SOURCE_PATHS = {
    'vendor/bevy_core_pipeline/Cargo.lock',
    'vendor/bevy_core_pipeline/Cargo.toml',
    'vendor/bevy_core_pipeline/Cargo.toml.orig',
    'vendor/bevy_core_pipeline/FLIGHTSIM-MODIFICATION-NOTICE.txt',
    'vendor/bevy_core_pipeline/FLIGHTSIM-PATCHES.md',
    'vendor/bevy_core_pipeline/FLIGHTSIM-UPSTREAM-SOURCE.json',
    'vendor/bevy_core_pipeline/LICENSE-APACHE',
    'vendor/bevy_core_pipeline/LICENSE-FLIGHTSIM-APACHE',
    'vendor/bevy_core_pipeline/LICENSE-FLIGHTSIM-MIT',
    'vendor/bevy_core_pipeline/LICENSE-MIT',
    'vendor/bevy_core_pipeline/README.md',
    'vendor/bevy_core_pipeline/src/blit/blit.wgsl',
    'vendor/bevy_core_pipeline/src/blit/mod.rs',
    'vendor/bevy_core_pipeline/src/core_2d/main_opaque_pass_2d_node.rs',
    'vendor/bevy_core_pipeline/src/core_2d/main_transparent_pass_2d_node.rs',
    'vendor/bevy_core_pipeline/src/core_2d/mod.rs',
    'vendor/bevy_core_pipeline/src/core_3d/main_opaque_pass_3d_node.rs',
    'vendor/bevy_core_pipeline/src/core_3d/main_transmissive_pass_3d_node.rs',
    'vendor/bevy_core_pipeline/src/core_3d/main_transparent_pass_3d_node.rs',
    'vendor/bevy_core_pipeline/src/core_3d/mod.rs',
    'vendor/bevy_core_pipeline/src/deferred/copy_deferred_lighting_id.wgsl',
    'vendor/bevy_core_pipeline/src/deferred/copy_lighting_id.rs',
    'vendor/bevy_core_pipeline/src/deferred/mod.rs',
    'vendor/bevy_core_pipeline/src/deferred/node.rs',
    'vendor/bevy_core_pipeline/src/experimental/mip_generation/downsample_depth.wgsl',
    'vendor/bevy_core_pipeline/src/experimental/mip_generation/mod.rs',
    'vendor/bevy_core_pipeline/src/experimental/mod.rs',
    'vendor/bevy_core_pipeline/src/fullscreen_material.rs',
    'vendor/bevy_core_pipeline/src/fullscreen_vertex_shader/fullscreen.wgsl',
    'vendor/bevy_core_pipeline/src/fullscreen_vertex_shader/mod.rs',
    'vendor/bevy_core_pipeline/src/lib.rs',
    'vendor/bevy_core_pipeline/src/oit/mod.rs',
    'vendor/bevy_core_pipeline/src/oit/oit_draw.wgsl',
    'vendor/bevy_core_pipeline/src/oit/resolve/mod.rs',
    'vendor/bevy_core_pipeline/src/oit/resolve/node.rs',
    'vendor/bevy_core_pipeline/src/oit/resolve/oit_resolve.wgsl',
    'vendor/bevy_core_pipeline/src/prepass/mod.rs',
    'vendor/bevy_core_pipeline/src/prepass/node.rs',
    'vendor/bevy_core_pipeline/src/skybox/mod.rs',
    'vendor/bevy_core_pipeline/src/skybox/prepass.rs',
    'vendor/bevy_core_pipeline/src/skybox/skybox.wgsl',
    'vendor/bevy_core_pipeline/src/skybox/skybox_prepass.wgsl',
    'vendor/bevy_core_pipeline/src/tonemapping/lut_bindings.wgsl',
    'vendor/bevy_core_pipeline/src/tonemapping/luts/Blender_-11_12.ktx2',
    'vendor/bevy_core_pipeline/src/tonemapping/luts/info.txt',
    'vendor/bevy_core_pipeline/src/tonemapping/luts/tony_mc_mapface.ktx2',
    'vendor/bevy_core_pipeline/src/tonemapping/mod.rs',
    'vendor/bevy_core_pipeline/src/tonemapping/node.rs',
    'vendor/bevy_core_pipeline/src/tonemapping/tonemapping.wgsl',
    'vendor/bevy_core_pipeline/src/tonemapping/tonemapping_shared.wgsl',
    'vendor/bevy_core_pipeline/src/upscaling/mod.rs',
    'vendor/bevy_core_pipeline/src/upscaling/node.rs',
    'vendor/bevy_core_pipeline/third-party-notices/Blender-Filmic-NOTICE.txt',
    'vendor/bevy_core_pipeline/third-party-notices/Blender-Filmic-OpenColorIO-LICENSE',
    'vendor/bevy_core_pipeline/third-party-notices/TonyMcMapface-LICENSE-MIT',
}
ORIGINAL_HIGHWING_SOURCE_PATHS = {
    'tools/original-highwing/CHECKSUMS.sha256',
    'tools/original-highwing/README.md',
    'tools/original-highwing/adapt_original_highwing.py',
    'tools/original-highwing/assets/aircraft/light_single.glb',
    'tools/original-highwing/assets/aircraft/light_single.json',
    'tools/original-highwing/check_rejection_cases.py',
    'tools/original-highwing/evidence/adapter-output.json',
    'tools/original-highwing/evidence/author-visual-review.json',
    'tools/original-highwing/evidence/blender-import-and-render.json',
    'tools/original-highwing/evidence/blender-import.log',
    'tools/original-highwing/evidence/blender-render.log',
    'tools/original-highwing/evidence/geometry-validation.json',
    'tools/original-highwing/evidence/independent-review.json',
    'tools/original-highwing/evidence/independent-review.md',
    'tools/original-highwing/evidence/original-validation-rerun.json',
    'tools/original-highwing/evidence/preservation-check.json',
    'tools/original-highwing/evidence/rebuilt-output.json',
    'tools/original-highwing/evidence/rebuilt.glb',
    'tools/original-highwing/evidence/rejection-cases.json',
    'tools/original-highwing/evidence/source-manifest.json',
    'tools/original-highwing/previews/candidate-front.jpg',
    'tools/original-highwing/previews/candidate-starboard.jpg',
    'tools/original-highwing/previews/candidate-three-quarter.jpg',
    'tools/original-highwing/render_candidate.py',
    'tools/original-highwing/source/LICENSE-APACHE',
    'tools/original-highwing/source/LICENSE-MIT',
    'tools/original-highwing/source/build_meadow_trainer.py',
    'tools/original-highwing/source/light_single.json',
    'tools/original-highwing/source/meadow_trainer.blend',
    'tools/original-highwing/source/meadow_trainer.glb',
    'tools/original-highwing/source/meadow_trainer.json',
    'tools/original-highwing/source/validate_meadow_trainer.py',
    'tools/original-highwing/validate_candidate.py',
}
FULL_SOURCE_PROVENANCE_PATHS = {
    'docs/release/filmic-asset-source-decision.json',
    'docs/release/filmic-asset-source-decision.md',
    'docs/release/licenses/Blender-Filmic-NOTICE.txt',
    'docs/release/licenses/Blender-Filmic-OpenColorIO-LICENSE',
    'docs/release/licenses/TonyMcMapface-LICENSE-MIT',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/ASSESSMENT.md',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/MANIFEST.json',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/REPRODUCE.md',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/bevy-recipe-exact.txt',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/blender-3.4.1-provenance.json',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/blender-3.4.1.sha256',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/comparison-test-results.json',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/final-comparison.json',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/independent-review/review-result.json',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/independent-review/review.md',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/independent-review/source-evidence.json',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/independent-review/verification-final.json',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/independent-review/verify.py',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/notices/Bevy-LICENSE-MIT.txt',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/notices/Blender-3.4.1-config.ocio',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/notices/Blender-OpenColorIO-BSD-3-Clause.txt',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/prior-native-inventory.json',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/recipe-source-bindings.json',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/reference-metadata.json',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/reproduce.py',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/run-5/invocation.json',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/run-5/observation.json',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/run-6/invocation.json',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/run-6/observation.json',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/run_pinned.py',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/source-bindings.json',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/stimulus.rs',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/test_comparison.py',
    'docs/release/licenses/review-evidence/bevy-blender-filmic-lut/verify_reproduction.py',
    '.gitattributes',
    'ATTRIBUTION.md',
    'Cargo.lock',
    'Cargo.toml',
    'assets/aircraft/light_single.glb',
    'docs/qa/licensed-tonemap-source-candidate-2026-10-08.md',
    'docs/release/asset-rights-manifest.json',
    'docs/release/full-two-aircraft-source-candidate.md',
    'docs/release/full-two-aircraft-source-inputs.json',
    'docs/release/history/mesh-and-lut-boundary-d918943.json',
    'docs/release/history/source-archive-policy-d918943.md',
    'docs/release/licenses/review-evidence/constgebra-0.1.4/package/.cargo_vcs_info.json',
    'docs/release/licenses/review-evidence/constgebra-0.1.4/package/.github/workflows/rust.yml',
    'docs/release/licenses/review-evidence/constgebra-0.1.4/package/.gitignore',
    'docs/release/licenses/review-evidence/constgebra-0.1.4/package/Cargo.toml',
    'docs/release/licenses/review-evidence/constgebra-0.1.4/package/Cargo.toml.orig',
    'docs/release/licenses/review-evidence/constgebra-0.1.4/package/README.md',
    'docs/release/licenses/review-evidence/constgebra-0.1.4/package/src/comptime.rs',
    'docs/release/licenses/review-evidence/constgebra-0.1.4/package/src/lib.rs',
    'docs/release/licenses/review-evidence/constgebra-0.1.4/package/src/runtime.rs',
    'docs/release/licenses/review-evidence/constgebra-0.1.4/source-record.json',
    'docs/release/licenses/review-evidence/standard-license-texts/Apache-2.0.txt',
    'docs/release/licenses/review-evidence/standard-license-texts/CC0-1.0.txt',
    'docs/release/licenses/review-evidence/standard-license-texts/README.txt',
    'docs/release/licenses/review-evidence/two-package-ordinary/independent-review/REVIEW.md',
    'docs/release/licenses/review-evidence/two-package-ordinary/independent-review/VERIFICATION.md',
    'docs/release/licenses/review-evidence/two-package-ordinary/independent-review/acceptance.json',
    'docs/release/licenses/review-evidence/two-package-ordinary/independent-review/assessed-input.json',
    'docs/release/licenses/review-evidence/two-package-ordinary/independent-review/candidate-verification.json',
    'docs/release/licenses/review-evidence/two-package-ordinary/independent-review/fresh-per-file-histories.json',
    'docs/release/licenses/review-evidence/two-package-ordinary/independent-review/fresh-primary-records.json',
    'docs/release/licenses/review-evidence/two-package-ordinary/independent-review/original-verification.json',
    'docs/release/licenses/review-evidence/two-package-ordinary/independent-review/reviewed-assessment-snapshot.json',
    'docs/release/licenses/review-evidence/two-package-ordinary/independent-review/verify_candidate.py',
    'docs/release/licenses/review-evidence/two-package-ordinary/independent-review/verify_original.py',
    'docs/release/licenses/review-evidence/two-package-ordinary/native-inventory.json',
    'docs/release/licenses/review-evidence/two-package-ordinary/prior-package-assessment.json',
    'docs/release/licenses/review-evidence/two-package-ordinary/reconciliation.json',
    'docs/release/licenses/review-evidence/two-package-ordinary/windows-candidate-archive-receipt.json',
    'docs/release/licenses/review-evidence/two-package-ordinary/windows-candidate-validation.json',
    'docs/release/native-link-output-association.md',
    'docs/release/ordinary-two-package-assessment.json',
    'docs/release/ordinary-two-package-assessment.md',
    'docs/release/source-archive-policy.md',
    'scripts/check-commercial-readiness.py',
    'scripts/check-full-two-aircraft-source.py',
    'scripts/check-source-archive.py',
    'scripts/tests/test_commercial_readiness.py',
    'scripts/tests/test_full_two_aircraft_source.py',
    'tools/blender/validate_cedar_turboprop.py',
    'tools/blender/validate_kestrel_jet_trainer.py',
    'tools/blender/validate_meadow_trainer.py',
    'tools/validate-tonemap-subset.py',
}
PRESERVED_RUNTIME_EXTRA_PATHS = {
    'assets/aircraft/swift_sport.blend',
    'crates/flightsim-app/src/aircraft_profile/schema_contract.rs',
    'crates/flightsim-app/src/airport_drape_runtime.rs',
    'crates/flightsim-app/src/distance_runtime.rs',
    'crates/flightsim-app/src/graphics_runtime.rs',
    'crates/flightsim-app/src/region_downloads.rs',
    'crates/flightsim-app/src/region_downloads_tests.rs',
    'crates/flightsim-app/src/render_metrics.rs',
    'crates/flightsim-app/src/scenery_map_tests.rs',
    'crates/flightsim-app/src/scenery_runtime.rs',
    'crates/flightsim-app/src/scenery_runtime_tests.rs',
    'crates/flightsim-app/src/scenery_terrain_policy_tests.rs',
    'crates/flightsim-app/src/water_runtime.rs',
    'crates/flightsim-app/src/windows_readback_diagnostic.rs',
    'crates/flightsim-app/tests/aircraft_scene_hierarchy.rs',
    'crates/flightsim-app/tests/fixtures/README.md',
    'crates/flightsim-app/tests/fixtures/region-synthetic.zip',
    'crates/flightsim-assetgen/Cargo.toml',
    'crates/flightsim-assetgen/src/env_file.rs',
    'crates/flightsim-assetgen/src/main.rs',
    'crates/flightsim-assetgen/src/meshy.rs',
    'crates/flightsim-audio/Cargo.toml',
    'crates/flightsim-audio/examples/render_engine.rs',
    'crates/flightsim-audio/src/airframe.rs',
    'crates/flightsim-audio/src/dsp.rs',
    'crates/flightsim-audio/src/engine.rs',
    'crates/flightsim-audio/src/mixer.rs',
    'crates/flightsim-audio/src/source.rs',
    'crates/flightsim-audio/src/turbine.rs',
    'crates/flightsim-content/examples/download_region.rs',
    'crates/flightsim-content/examples/validate_aircraft_package.rs',
    'crates/flightsim-content/examples/validate_region_package.rs',
    'crates/flightsim-content/src/download/http.rs',
    'crates/flightsim-content/src/download/mod.rs',
    'crates/flightsim-content/src/download/source.rs',
    'crates/flightsim-content/src/download/tests.rs',
    'crates/flightsim-content/tests/data/download-fixture.zip',
    'crates/flightsim-content/tests/data/make_download_fixture.py',
    'crates/flightsim-core/Cargo.toml',
    'crates/flightsim-core/benches/origin.rs',
    'crates/flightsim-core/src/fixed_step.rs',
    'crates/flightsim-core/src/frames.rs',
    'crates/flightsim-core/src/lib.rs',
    'crates/flightsim-core/src/origin.rs',
    'crates/flightsim-core/src/render_frame.rs',
    'crates/flightsim-core/src/units.rs',
    'crates/flightsim-fdm/benches/step.rs',
    'crates/flightsim-fdm/benches/turboprop_step.rs',
    'crates/flightsim-fdm/examples/aero_trace.rs',
    'crates/flightsim-fdm/examples/longitudinal_trace.rs',
    'crates/flightsim-fdm/tests/bundled_definitions.rs',
    'crates/flightsim-fdm/tests/fixtures/cedar-law1-calm-braking-boundary.json',
    'crates/flightsim-fdm/tests/fixtures/cedar-law1-component-bits.json',
    'crates/flightsim-fdm/tests/fixtures/cedar-law1-components.json',
    'crates/flightsim-fdm/tests/fixtures/jet-contact-dynamics.json',
    'crates/flightsim-fdm/tests/fixtures/subsonic-identity-v1.json',
    'crates/flightsim-fdm/tests/fixtures/turboprop-numerical.json',
    'crates/flightsim-fdm/tests/invariants.rs',
    'crates/flightsim-fdm/tests/jet_contact_regression.rs',
    'crates/flightsim-fdm/tests/jet_runtime.rs',
    'crates/flightsim-fdm/tests/longitudinal.rs',
    'crates/flightsim-fdm/tests/longitudinal_support/mod.rs',
    'crates/flightsim-fdm/tests/slope_contact.rs',
    'crates/flightsim-fdm/tests/stall_peak.rs',
    'crates/flightsim-fdm/tests/subsonic_tables.rs',
    'crates/flightsim-fdm/tests/turbulence.rs',
    'crates/flightsim-fdm/tests/turbulence_global.rs',
    'crates/flightsim-input/Cargo.toml',
    'crates/flightsim-input/src/configuration.rs',
    'crates/flightsim-input/src/controllers.rs',
    'crates/flightsim-input/src/gamepad.rs',
    'crates/flightsim-input/src/keyboard_regression.rs',
    'crates/flightsim-input/src/native.rs',
    'crates/flightsim-net/Cargo.toml',
    'crates/flightsim-net/src/lib.rs',
    'crates/flightsim-net/src/protocol.rs',
    'crates/flightsim-net/src/session.rs',
    'crates/flightsim-net/src/traffic.rs',
    'crates/flightsim-net/tests/local_sessions.rs',
    'crates/flightsim-net/tests/protocol_traffic.rs',
    'crates/flightsim-render/Cargo.toml',
    'crates/flightsim-render/benches/scenery_mesh.rs',
    'crates/flightsim-render/benches/terrain_polar_normals.rs',
    'crates/flightsim-render/examples/sun_clock.rs',
    'crates/flightsim-render/src/aircraft.rs',
    'crates/flightsim-render/src/apron.rs',
    'crates/flightsim-render/src/biome.rs',
    'crates/flightsim-render/src/cloud_field.rs',
    'crates/flightsim-render/src/cloud_field.wgsl',
    'crates/flightsim-render/src/cloud_volume.rs',
    'crates/flightsim-render/src/cloud_volume.wgsl',
    'crates/flightsim-render/src/cockpit.rs',
    'crates/flightsim-render/src/daylight.rs',
    'crates/flightsim-render/src/graphics_quality.rs',
    'crates/flightsim-render/src/graphics_quality_metrics.rs',
    'crates/flightsim-render/src/graphics_quality_pipelines.rs',
    'crates/flightsim-render/src/holding_position.rs',
    'crates/flightsim-render/src/lib.rs',
    'crates/flightsim-render/src/modeled_weather.rs',
    'crates/flightsim-render/src/precipitation.rs',
    'crates/flightsim-render/src/runway.rs',
    'crates/flightsim-render/src/runway_lights.rs',
    'crates/flightsim-render/src/scenery.rs',
    'crates/flightsim-render/src/scenery_canopy_tests.rs',
    'crates/flightsim-render/src/scenery_exclusions.rs',
    'crates/flightsim-render/src/sun.rs',
    'crates/flightsim-render/src/taxiway.rs',
    'crates/flightsim-render/src/taxiway_lights.rs',
    'crates/flightsim-render/src/taxiway_sign.rs',
    'crates/flightsim-render/src/terrain.rs',
    'crates/flightsim-render/src/terrain_detail.rs',
    'crates/flightsim-render/src/terrain_detail.wgsl',
    'crates/flightsim-render/src/terrain_drape.rs',
    'crates/flightsim-render/src/terrain_overlays.rs',
    'crates/flightsim-render/src/terrain_polar_normals.rs',
    'crates/flightsim-render/src/terrain_stitching.rs',
    'crates/flightsim-render/src/tonemapping.rs',
    'crates/flightsim-render/src/water.rs',
    'crates/flightsim-render/src/water.wgsl',
    'crates/flightsim-render/src/water_mask.rs',
    'crates/flightsim-render/src/water_pipelines.rs',
    'crates/flightsim-render/src/weather.rs',
    'crates/flightsim-render/tests/daylight_systems.rs',
    'crates/flightsim-render/tests/modeled_weather_systems.rs',
    'crates/flightsim-render/tests/scenery_bounds.rs',
    'crates/flightsim-render/tests/scenery_forest_lattice.rs',
    'crates/flightsim-render/tests/solar_cross_check.rs',
    'crates/flightsim-render/tests/sun_shadow_cascades.rs',
    'crates/flightsim-render/tests/support/scenery_fixtures.rs',
    'crates/flightsim-render/tests/terrain_overlay_lifecycle.rs',
    'crates/flightsim-render/tests/terrain_overlay_transactions.rs',
    'crates/flightsim-render/tests/terrain_parent_fallback.rs',
    'crates/flightsim-render/tests/terrain_stitching_systems.rs',
    'crates/flightsim-render/tests/terrain_streaming_systems.rs',
    'crates/flightsim-render/tests/tonemapping_modes.rs',
    'crates/flightsim-render/tests/twilight.rs',
    'crates/flightsim-render/tests/weather_systems.rs',
    'crates/flightsim-sim/examples/peregrine_qualification.rs',
    'crates/flightsim-sim/examples/record_visual_flight.rs',
    'crates/flightsim-sim/examples/support/turbulence_scenarios.rs',
    'crates/flightsim-sim/examples/turbulence_diagnostics.rs',
    'crates/flightsim-sim/examples/turbulence_validation.rs',
    'crates/flightsim-sim/src/bin/jet-headless.rs',
    'crates/flightsim-sim/src/crash.rs',
    'crates/flightsim-sim/src/director.rs',
    'crates/flightsim-sim/src/flight.rs',
    'crates/flightsim-sim/src/ground.rs',
    'crates/flightsim-sim/src/jet_scenarios.rs',
    'crates/flightsim-sim/src/main.rs',
    'crates/flightsim-sim/src/model_simulation.rs',
    'crates/flightsim-sim/src/model_simulation/parked.rs',
    'crates/flightsim-sim/src/model_simulation/presentation.rs',
    'crates/flightsim-sim/src/simulation.rs',
    'crates/flightsim-sim/tests/acceptance.rs',
    'crates/flightsim-sim/tests/aircraft_profile_v2.rs',
    'crates/flightsim-sim/tests/aircraft_profile_v3.rs',
    'crates/flightsim-sim/tests/airport_circuit.rs',
    'crates/flightsim-sim/tests/approach.rs',
    'crates/flightsim-sim/tests/climate_integration.rs',
    'crates/flightsim-sim/tests/crash_flight.rs',
    'crates/flightsim-sim/tests/fixed_step_controls.rs',
    'crates/flightsim-sim/tests/fixtures/cedar-law1-profile.json',
    'crates/flightsim-sim/tests/fixtures/cedar-law2-negative-rows.json',
    'crates/flightsim-sim/tests/fixtures/jet-identity-v2.json',
    'crates/flightsim-sim/tests/fixtures/legacy_v1.fsreplay',
    'crates/flightsim-sim/tests/fixtures/legacy_v2_disabled.fsreplay',
    'crates/flightsim-sim/tests/fixtures/legacy_v2_world.fsreplay',
    'crates/flightsim-sim/tests/fixtures/near-static-cedar-negative-rows.json',
    'crates/flightsim-sim/tests/fixtures/near-static-turboprop-identity-v4.json',
    'crates/flightsim-sim/tests/fixtures/turboprop-identity-v3.json',
    'crates/flightsim-sim/tests/fixtures/v3_clear.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v3_cloud.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v3_custom_both.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v3_fog.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v3_legacy_weather.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v3_rain.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v3_snow.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v3_storm.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v4_clear.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v4_cloud.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v4_custom_both.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v4_fog.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v4_rain.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v4_snow.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v4_storm.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v4_terminal_no_query.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v4_terminal_zero.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v4_zero.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v5_clear.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v5_cloud.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v5_custom_both.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v5_fog.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v5_rain.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v5_snow.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v5_storm.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v5_successful_121.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v5_terminal_aero.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v5_terminal_altitude.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v5_terminal_budget.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v5_terminal_disk_below.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v5_terminal_disk_derived.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v5_terminal_disk_nonpositive.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v5_terminal_envelope.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v5_terminal_power_map.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v5_terminal_propeller_map.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v5_terminal_raw.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v5_terminal_tip.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v5_terminal_wind.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v5_zero.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_clear.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_cloud.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_custom_both.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_fog.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_rain.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_snow.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_storm.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_successful_121.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_adverse.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_aero.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_altitude.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_both_inflows.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_budget.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_disk_below.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_disk_derived.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_disk_nonpositive.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_envelope.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_negative_aero.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_negative_budget.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_negative_component.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_negative_derivative.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_negative_ground.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_power_map.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_propeller_map.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_raw.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_scale_load.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_scale_underflow.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_static_derived.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_static_floor.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_static_power.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_static_thrust.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_tip.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_transverse.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_wind.fsreplay',
    'crates/flightsim-sim/tests/fixtures/v6_zero.fsreplay',
    'crates/flightsim-sim/tests/flight_log.rs',
    'crates/flightsim-sim/tests/global_climate_duration.rs',
    'crates/flightsim-sim/tests/global_climate_flight.rs',
    'crates/flightsim-sim/tests/hands_off.rs',
    'crates/flightsim-sim/tests/jet_headless_cli.rs',
    'crates/flightsim-sim/tests/jet_model_identity.rs',
    'crates/flightsim-sim/tests/jet_numerical_flight.rs',
    'crates/flightsim-sim/tests/jet_presentation.rs',
    'crates/flightsim-sim/tests/jet_replay_v4.rs',
    'crates/flightsim-sim/tests/modeled_weather.rs',
    'crates/flightsim-sim/tests/parked_slope.rs',
    'crates/flightsim-sim/tests/render_rehearsal.rs',
    'crates/flightsim-sim/tests/replay_fidelity.rs',
    'crates/flightsim-sim/tests/replay_file_boundaries.rs',
    'crates/flightsim-sim/tests/replay_hostile.rs',
    'crates/flightsim-sim/tests/replay_model_identity.rs',
    'crates/flightsim-sim/tests/replay_world_climate.rs',
    'crates/flightsim-sim/tests/restart.rs',
    'crates/flightsim-sim/tests/slope_spawn.rs',
    'crates/flightsim-sim/tests/stall_margin.rs',
    'crates/flightsim-sim/tests/stress.rs',
    'crates/flightsim-sim/tests/touchdown.rs',
    'crates/flightsim-sim/tests/turboprop_codec_v5.rs',
    'crates/flightsim-sim/tests/turboprop_common/mod.rs',
    'crates/flightsim-sim/tests/turboprop_model_identity.rs',
    'crates/flightsim-sim/tests/turboprop_replay_v5.rs',
    'crates/flightsim-sim/tests/turbulence_envelopes.rs',
    'crates/flightsim-sim/tests/turbulence_flight.rs',
    'crates/flightsim-sim/tests/turbulence_integration.rs',
    'crates/flightsim-sim/tests/wind.rs',
    'crates/flightsim-tilegen/Cargo.toml',
    'crates/flightsim-tilegen/examples/check_geoid.rs',
    'crates/flightsim-tilegen/examples/synthetic_dem.rs',
    'crates/flightsim-tilegen/src/airport.rs',
    'crates/flightsim-tilegen/src/bin/flightsim-airportgen.rs',
    'crates/flightsim-tilegen/src/bin/flightsim-globalgen.rs',
    'crates/flightsim-tilegen/src/bin/flightsim-scenerygen.rs',
    'crates/flightsim-tilegen/src/generate.rs',
    'crates/flightsim-tilegen/src/geoid.rs',
    'crates/flightsim-tilegen/src/geotiff.rs',
    'crates/flightsim-tilegen/src/global.rs',
    'crates/flightsim-tilegen/src/lib.rs',
    'crates/flightsim-tilegen/src/main.rs',
    'crates/flightsim-tilegen/src/region.rs',
    'crates/flightsim-tilegen/src/scenery.rs',
    'crates/flightsim-tilegen/src/testing.rs',
    'crates/flightsim-tilegen/src/vertical_datum.rs',
    'crates/flightsim-tilegen/tests/dateline_coverage.rs',
    'crates/flightsim-tilegen/tests/fixtures/airport-valid.fsairports',
    'crates/flightsim-tilegen/tests/generation_limits.rs',
    'crates/flightsim-tilegen/tests/geoid_cli.rs',
    'crates/flightsim-tilegen/tests/geoid_normalization.rs',
    'crates/flightsim-tilegen/tests/geotiff_metadata.rs',
    'crates/flightsim-tilegen/tests/output_safety.rs',
    'crates/flightsim-tilegen/tests/pbf_hostile_probe.rs',
    'crates/flightsim-tilegen/tests/pipeline.rs',
    'crates/flightsim-tilegen/tests/scenery_pipeline.rs',
    'crates/flightsim-tilegen/tests/support/pbf.rs',
    'crates/flightsim-tilegen/tests/vertical_datum_gate.rs',
    'crates/flightsim-tilegen/tests/vertical_datum_read.rs',
    'crates/flightsim-ui/Cargo.toml',
    'crates/flightsim-ui/examples/attitude_clip_repro.rs',
    'crates/flightsim-ui/examples/attitude_matrix.rs',
    'crates/flightsim-ui/examples/check_attitude_pixels.py',
    'crates/flightsim-ui/src/attitude.rs',
    'crates/flightsim-ui/src/attitude.wgsl',
    'crates/flightsim-ui/src/crash.rs',
    'crates/flightsim-ui/src/input_diagnostics.rs',
    'crates/flightsim-ui/src/landing.rs',
    'crates/flightsim-ui/src/traffic.rs',
    'crates/flightsim-world/Cargo.toml',
    'crates/flightsim-world/benches/scenery.rs',
    'crates/flightsim-world/benches/terrain_seam_planning.rs',
    'crates/flightsim-world/data/global-terrain.fsgt',
    'crates/flightsim-world/data/global-terrain.provenance.json',
    'crates/flightsim-world/data/ncep-ncar-1991-2020.fsclim',
    'crates/flightsim-world/data/ncep-ncar-1991-2020.json',
    'crates/flightsim-world/src/airport.rs',
    'crates/flightsim-world/src/airport/io.rs',
    'crates/flightsim-world/src/airport/io/v3.rs',
    'crates/flightsim-world/src/climate.rs',
    'crates/flightsim-world/src/climate_metadata.rs',
    'crates/flightsim-world/src/dem.rs',
    'crates/flightsim-world/src/dem/io.rs',
    'crates/flightsim-world/src/global_metadata.rs',
    'crates/flightsim-world/src/mesh.rs',
    'crates/flightsim-world/src/scenery.rs',
    'crates/flightsim-world/src/scenery/io.rs',
    'crates/flightsim-world/src/seams.rs',
    'crates/flightsim-world/src/seams/planner.rs',
    'crates/flightsim-world/src/streaming.rs',
    'crates/flightsim-world/tests/airport_mesh_clearance.rs',
    'crates/flightsim-world/tests/dateline_dem_mesh.rs',
    'crates/flightsim-world/tests/scenery_database.rs',
    'crates/flightsim-world/tests/terrain_seam_geometry.rs',
    'crates/flightsim-world/tests/v3_fuzz_review.rs',
}
HISTORICAL_SOURCE_PATHS = {
    'scripts/full-two-aircraft-runtime-pins.json',
    'scripts/history/d918943-analytical-swift-capture-contract.json',
    'scripts/history/d918943-analytical-swift-source-contract.json',
    'scripts/history/d918943-replay-candidate-contract.json',
}

REPLAY_CONTRACT_PATHS |= (CORE_PIPELINE_SOURCE_PATHS | ORIGINAL_HIGHWING_SOURCE_PATHS
                          | FULL_SOURCE_PROVENANCE_PATHS | PRESERVED_RUNTIME_EXTRA_PATHS
                          | HISTORICAL_SOURCE_PATHS)
# Add exact members after freezing the original 404-path history above.
REPLAY_CONTRACT_PATHS |= (COMPONENT_TERMS_SOURCE_PATHS
                          | {COMPONENT_TERMS_MIGRATION_PATH, COMPONENT_TERMS_HISTORY_PATH})
REPLAY_CONTRACT_PATHS |= (TERRAIN_CENTROID_RUNTIME_PATHS
                          | {TERRAIN_CENTROID_MIGRATION_PATH, TERRAIN_CENTROID_HISTORY_PATH})
REPLAY_CONTRACT_PATHS |= TERRAIN_STITCH_SOURCE_PATHS
REPLAY_CONTRACT_PATHS |= ALPHA22_SOURCE_PATHS
ALPHA22_REPLAY_CONTRACT_PATHS = frozenset(REPLAY_CONTRACT_PATHS)
REPLAY_CONTRACT_PATHS |= COCKPIT_SOURCE_PATHS

# Independent Python encoders and their existing bytes stay frozen separately
# from the moving reviewed implementation. Never regenerate to satisfy a pin.
INDEPENDENT_REPLAY_HASHES = {
    'docs/qa/replay_identity_reference.py': 'a1b65acbc795880f8fa9ba5891260ab0437dc0281081658454169c22277b1034',
    'docs/qa/replay_v3_reference.py': 'f5b5128b147d512f55441c8610ff337b787fd8be28c7564149861aa0d442b1c6',
    'crates/flightsim-sim/tests/fixtures/legacy_v1.fsreplay': 'c9d0404374f540797ad4dd3b648abb3fbf928b6de0cde88195172cb2beffad57',
    'crates/flightsim-sim/tests/fixtures/legacy_v2_disabled.fsreplay': '02f14d7c188fd9426faa78dd1670168997abf18f9ec4aeae1f02fbce024732b7',
    'crates/flightsim-sim/tests/fixtures/legacy_v2_world.fsreplay': 'fe156d5e407b847b37544dc6fb7e153b6b0ba49d0e2d0a38ffda57ac1c8c57f0',
    'crates/flightsim-sim/tests/fixtures/v3_clear.fsreplay': 'c26a89dfc0db1cb75bb9a09a02be0c650d13d3fae6f584c277c9da0ba5541f94',
    'crates/flightsim-sim/tests/fixtures/v3_cloud.fsreplay': '77ee335e29b6c352ba121d6134f2aa0add52cc57dc7e26f0ebd10375e8906476',
    'crates/flightsim-sim/tests/fixtures/v3_custom_both.fsreplay': 'dc558b9a5dcc46ba91513b9fc1894121f3314252eb4292cb7e47ad9ac010b30f',
    'crates/flightsim-sim/tests/fixtures/v3_fog.fsreplay': '9b7fcb8ceb52e84d54f015fa01c8d5aed24721ef01952805b23164b2d12a0760',
    'crates/flightsim-sim/tests/fixtures/v3_legacy_weather.fsreplay': 'ba5fa5cc8fac02853f1f00c1364316fff46fbe4e95734b915a966dbafb10ba91',
    'crates/flightsim-sim/tests/fixtures/v3_rain.fsreplay': 'de73fc4ab21401bd38f99a9be49b024683b593c7f4ae834c1f21b2ba1bb79b85',
    'crates/flightsim-sim/tests/fixtures/v3_snow.fsreplay': '29a0d186e4efdbac32e901ceabda6b831e8a13bf0517d57a5d37333688c382fb',
    'crates/flightsim-sim/tests/fixtures/v3_storm.fsreplay': 'b9f6208a9880a87998fb9a4e04b2d0e3400ad2df3b4e312f58fd98bb2630945d',
}
# Additive independent witnesses. The 13 earlier anchors above never move.
NEAR_STATIC_INDEPENDENT_HASHES = {
    'docs/examples/aircraft-profiles-v2/numerical-jet.json': '8339b68183ea31c228082e56984a39775f7f23df5633dd8a333fe06a65be7c74',
    'docs/examples/aircraft-profiles-v3/numerical-turboprop.json': '7c5a091eb33821b8cbd48d7be8881d21fac02e85773b5457e96431eb7ebbac9f',
    'crates/flightsim-fdm/tests/fixtures/cedar-law1-calm-braking-boundary.json': 'a9819ee75f94dae91adeed8e63d661c9ae08b64e7e443431c23c3f8e69ac181f',
    'crates/flightsim-fdm/tests/fixtures/cedar-law1-component-bits.json': '07692d8d292c8c101c252c64adefe2c22ff41163824bc23bb6ce25ed31a7e61c',
    'crates/flightsim-fdm/tests/fixtures/cedar-law1-components.json': '851847ec222304d548144184ab4b6994471f6d861e45475ed71d4dbfb81bcd75',
    'crates/flightsim-fdm/tests/fixtures/jet-contact-dynamics.json': '8c3e19849c6e7c22761cdaa1638efa69490f16e2dcf0d6e913c5a18034d109ec',
    'crates/flightsim-fdm/tests/fixtures/subsonic-identity-v1.json': '3239b77c88226892c7a5f0d0ef4f58c66e14b68372719bf0745998c298587179',
    'crates/flightsim-fdm/tests/fixtures/turboprop-numerical.json': 'bd609786819baf89790936768105f2338a828397eaad774a5720a83e42faa3ce',
    'crates/flightsim-sim/tests/fixtures/cedar-law1-profile.json': 'a4b13ac7c59830967666bb874e25416587a5fbf9945cbad248afa5bc7c9344e1',
    'crates/flightsim-sim/tests/fixtures/cedar-law2-negative-rows.json': '2467ad7537216ae1470caa73e94eeba44f737686ef251fb5ac046a1cf6ca1fd4',
    'crates/flightsim-sim/tests/fixtures/jet-identity-v2.json': '4c37e2959c782782e1e93df8cf42c967b3a860e3e37ed374f0c32ddcfb7815e5',
    'crates/flightsim-sim/tests/fixtures/near-static-cedar-negative-rows.json': '2467ad7537216ae1470caa73e94eeba44f737686ef251fb5ac046a1cf6ca1fd4',
    'crates/flightsim-sim/tests/fixtures/near-static-turboprop-identity-v4.json': '9cae1fea4fb03fdde8cc4262a9fdfa060bd38013cd6b7c87eb8f96a67a7d858c',
    'crates/flightsim-sim/tests/fixtures/turboprop-identity-v3.json': '96bc7abcc5509a54f4561bc32a1ed4780bf077ee1564c3c2e522ecc6aceb9fa5',
    'crates/flightsim-sim/tests/fixtures/v4_clear.fsreplay': '7314d870ed47e518e081f548c101a358372c1a7eb5ad8f4c648692f4b86466fc',
    'crates/flightsim-sim/tests/fixtures/v4_cloud.fsreplay': 'e9feb26e45e9e6d7c0191eecd41d50292dc8550fd8558eac664543e8fa58512a',
    'crates/flightsim-sim/tests/fixtures/v4_custom_both.fsreplay': '78975a71788b9eca6a7df4e5b069bccc0dfc591598db048049f50f6d88e60e2f',
    'crates/flightsim-sim/tests/fixtures/v4_fog.fsreplay': 'edb7eafa37b10b33ccca2718617ece8620fc90a8ad0649dde18dab83a1c647e3',
    'crates/flightsim-sim/tests/fixtures/v4_rain.fsreplay': '0ac894a84fcb6870bebe992011005e555208b4aac3b6c8016af1a26b4c345529',
    'crates/flightsim-sim/tests/fixtures/v4_snow.fsreplay': '30cc33b66be18a1d17640e06f13a677319e333baf46bfef780524b3ec9dcacac',
    'crates/flightsim-sim/tests/fixtures/v4_storm.fsreplay': '494cb25bf829dafdc6b65f13ee4dd5dcbff2e33c8c7a312774e722e555bede34',
    'crates/flightsim-sim/tests/fixtures/v4_terminal_no_query.fsreplay': '87618ba7b008e63d591a411f1f37e57bdb88e46f24e2520fdb5c80288a2d4dba',
    'crates/flightsim-sim/tests/fixtures/v4_terminal_zero.fsreplay': '69d58add993fa398ac741bb75db697115ee11346d55a5f2ced5b42376c276a36',
    'crates/flightsim-sim/tests/fixtures/v4_zero.fsreplay': 'cfc25b9f32bb79679c9911536acb539279f4fd6a33349e1604f6e70ce1aac458',
    'crates/flightsim-sim/tests/fixtures/v5_clear.fsreplay': 'b801b3d61aa88d681bb9275570938711ede13bbac3a75b787f9b57672b912b6a',
    'crates/flightsim-sim/tests/fixtures/v5_cloud.fsreplay': '41a20646d1de5b07a1c08d80cdcd34bb3f1e62e64115b25d6cb5f21ee7bd9972',
    'crates/flightsim-sim/tests/fixtures/v5_custom_both.fsreplay': 'bb54eef5d583968d2a05228d05327564497bacaf4333b98eb571ceee752d48ef',
    'crates/flightsim-sim/tests/fixtures/v5_fog.fsreplay': '8afed51fac6ef95641399253ff3758f44edbdc57b08da78d1776334b92e01fd1',
    'crates/flightsim-sim/tests/fixtures/v5_rain.fsreplay': '64015c8c757bfe89e51cd57f1d71fec580e4641b6dbcd27490f7ea7de525d7d9',
    'crates/flightsim-sim/tests/fixtures/v5_snow.fsreplay': '999bd016d09c264e1327d75d9dfdc979ed2cbe405f8b72fa9c38dd0b23177a7a',
    'crates/flightsim-sim/tests/fixtures/v5_storm.fsreplay': 'c32d4bb0addb09beb0cd7c4442fc87c06ae636c099388ccfc2374ee8daff19bd',
    'crates/flightsim-sim/tests/fixtures/v5_successful_121.fsreplay': '562668d47cef4dbb53d13e794be28cca6acd7086ed02005f1a23fca58be9b556',
    'crates/flightsim-sim/tests/fixtures/v5_terminal_aero.fsreplay': '046fb34b3c60abb68d931997b79b81b582ac4e5a88862c967067dfb3a59e5870',
    'crates/flightsim-sim/tests/fixtures/v5_terminal_altitude.fsreplay': '625fea452fa569828a423f8aa3e5c5dca884c0f4c00358454bf61a430c23e2fb',
    'crates/flightsim-sim/tests/fixtures/v5_terminal_budget.fsreplay': '59614beb81170399ed4f9706335d7dacd5f53d1985f1ebfb9e0a8979db8ccc40',
    'crates/flightsim-sim/tests/fixtures/v5_terminal_disk_below.fsreplay': '77426b90ee52e3dc424b062ac55f5254bb88d217543fe22130accf5c31041562',
    'crates/flightsim-sim/tests/fixtures/v5_terminal_disk_derived.fsreplay': 'b077031f38f23064e63cfa698062b75a8952dd2e3cf6d049c05b46ded929d8c6',
    'crates/flightsim-sim/tests/fixtures/v5_terminal_disk_nonpositive.fsreplay': 'd592bae63d961cb5f79485fc9d037ad6998ce350821c41b625ba0ac66abfecf2',
    'crates/flightsim-sim/tests/fixtures/v5_terminal_envelope.fsreplay': '259384b2bad80634f7ffd6df04a9971d51660e25413c832ac2fa391d3f93fa0f',
    'crates/flightsim-sim/tests/fixtures/v5_terminal_power_map.fsreplay': 'a0680b0d1ab116d545ef9789f947c719181aff95626eae3152a2b5df533f39fa',
    'crates/flightsim-sim/tests/fixtures/v5_terminal_propeller_map.fsreplay': '747ea3106048b39f15c143b0c66d2be0f69370b6c452d3a7a6c3b597a2cf5720',
    'crates/flightsim-sim/tests/fixtures/v5_terminal_raw.fsreplay': 'ba78f21d607625bdbfd92cf3773456aad819c37682d2183fe679f1e18d137db8',
    'crates/flightsim-sim/tests/fixtures/v5_terminal_tip.fsreplay': '7306b5f06080052229d5e4a6caada37264c5697b895e240bafc5d69f7e1578d6',
    'crates/flightsim-sim/tests/fixtures/v5_terminal_wind.fsreplay': 'be28d88516e5a3efa06e47e58b7d06e0a966d9f1675c6a3a48c18498ce6bc983',
    'crates/flightsim-sim/tests/fixtures/v5_zero.fsreplay': '7f57928fa7e0757d6f93c0c2086ed8dc4c87035ec6875af3ecf5190e2f2b8a05',
    'crates/flightsim-sim/tests/fixtures/v6_clear.fsreplay': '7bf2e16599fd931f5b353bbea4ec64674645f9dc3164c03b951148e4adf7d249',
    'crates/flightsim-sim/tests/fixtures/v6_cloud.fsreplay': '7e95f6dcccab3e471e8f13d97246836b46da5cfa1f59f94c7ac4a253c3cfe759',
    'crates/flightsim-sim/tests/fixtures/v6_custom_both.fsreplay': '99134652513041be2b39f9c3071211c5c9a43b0c07b1be33dd6dc4cee69a8353',
    'crates/flightsim-sim/tests/fixtures/v6_fog.fsreplay': 'c76efc7bb622762efbc7068c1b90eff6f4e9e3a2e5f8f4e6121ba42a32950458',
    'crates/flightsim-sim/tests/fixtures/v6_rain.fsreplay': 'eb06fb4b7f58a1e676e69f195cbd5283038b7126d5847879f27f2ec494f8e55a',
    'crates/flightsim-sim/tests/fixtures/v6_snow.fsreplay': 'a529bf64cc1a380b3c78490c9dcff280f6b075e1b262d0ffa9033d55be4604a3',
    'crates/flightsim-sim/tests/fixtures/v6_storm.fsreplay': '036c0e9dd3cf58a4fdbd1feb46deebcbd97381751bb1572bdb6d2ceb6ecc4c69',
    'crates/flightsim-sim/tests/fixtures/v6_successful_121.fsreplay': '2cd56a94ff82e67c8c8a7788f07a2a2ce77e6c6d95b879723b1d08b7aa7093e0',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_adverse.fsreplay': 'f10ecf51cd49296279a6a8bba34f25d941088d323820def19662887097538fd7',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_aero.fsreplay': '2e6a977483646d26c482f7a8bd4f2f4938aee45a90651673164485693748fe27',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_altitude.fsreplay': '0616de12d9825039770e8a68ea6a670c4c77ddba8285bdef836f16c522e4f166',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_both_inflows.fsreplay': '1706f5243a71509703af392a192dde44e7eb3961dcbaa9f820e1eee1793cd9d0',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_budget.fsreplay': 'ff2dfe253232ffa5eb6a6d5776a25e74b598290673d0a689ab6b46599ea3c0d9',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_disk_below.fsreplay': '94cd3b4ae4c46dcf9dc7929fc8d07e7f2c54cdb25ab95e419f1d2493f9d330dc',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_disk_derived.fsreplay': 'bcd210866214c5a8b6a518daf1d35c694abe751ac4cdfee981679cddb86d0bdc',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_disk_nonpositive.fsreplay': 'bdc06ee58c01bbfcdc4864f75cb7dbe8e71bfea6110e7ce9e88c982ccc5d0a53',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_envelope.fsreplay': 'd845fbdc57e6b15be18a3b5d3a412ca5b399e55f96facb777b9dc24acce93fc6',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_negative_aero.fsreplay': 'f7994d8f8797e68eb5c74c8deb4f1d30157805c85def035bb9d044e525db210e',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_negative_budget.fsreplay': 'dc84f0bf1f5339698c22f49201d951ce16105cbf1b330d4d28359ce3e1393f9c',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_negative_component.fsreplay': 'e4d1a9f33c221ce9e0445b9933a1ad6b945af51609a661c4aac988efbb5701a5',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_negative_derivative.fsreplay': '30de50d8f8d26c177116e238896c6855f99f0ec42140dd838837a51544e0af4b',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_negative_ground.fsreplay': 'ff983ef917703c354d1d84b7f4dfd9597335c6c82e108864904aee5ef762b66b',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_power_map.fsreplay': 'eb367e260a3438887519045d27ef71ddf2b77b5c71c229d991d3efe2bf2512c6',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_propeller_map.fsreplay': '2a5e0a88c2afd788d1801e11fd423ade3ec2897215239b9db8d17114ea82bd5f',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_raw.fsreplay': 'd79cbad1d258a9320c3412fc52646899e93062b26004ec28f4b5748e00bf1cce',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_scale_load.fsreplay': '34b3160827b673fab3f10004d64b4210de01001ff2606012153814e8e80aa8fe',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_scale_underflow.fsreplay': '69ada39a6a66b604786c57fc4ca52afb2bff70dc389d2bb17c3bab11e06af2db',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_static_derived.fsreplay': '8dc1069497bf30fedf4269d02e67854113f02ca21963cea1ccf42c036224bd0e',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_static_floor.fsreplay': '69a1644b1a6fcceeb405c8bd2927ed7bc80874c7161e18f78f2193f9db83ba14',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_static_power.fsreplay': 'cccddc3205f560d3c44f7691914928368547125808db46d83c95c9dee2b9dcc3',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_static_thrust.fsreplay': '43eef35f4d27940a3b60f277c79cbd2a394cde08213379991dee9cc5db854ee7',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_tip.fsreplay': 'fcc1fb917689100c7c2c1118e69a1db71613dc8705573dc4cc28b8b5210f18bc',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_transverse.fsreplay': 'a14e03aaf11014bba3a62f61dade364230c198e8905267be366250102020ac39',
    'crates/flightsim-sim/tests/fixtures/v6_terminal_wind.fsreplay': '8969c32ae25c2a8edd979ca3f96b111b067d827652a7db43dad2d24648775e93',
    'crates/flightsim-sim/tests/fixtures/v6_zero.fsreplay': 'ce9639d0c3247d8300185284f075b3b0733cdbead9245053ea05db4ee7867505',
    'docs/examples/aircraft-profiles-v4/numerical-near-static-turboprop.json': 'd53f6e59cf823a6ef3bbc87cf10ccfe077c433a23b1e404e06bfd26d6cc8b950',
    'docs/qa/jet_identity_reference.py': '6b0c906c6848b1969e63021988c703a22aa6bd04c130492a738e00bc4db2a410',
    'docs/qa/near_static_turboprop_identity_reference.py': '4096981308a54515fdf0b2f37d5ce38ac53e2449736606d7f11bc0547da64ea5',
    'docs/qa/replay_v4_reference.py': '72bcdb75e463a84b43d876149dbea52eb6ac27751a3c53a6b10410466a80f21a',
    'docs/qa/replay_v5_reference.py': '6b9bc209b883887d815765f56d54e8d7be3ab1891bff7e606d6ae68d33f46bc0',
    'docs/qa/replay_v6_reference.py': '0f222dc7c9ea4bb048bbbf569cc706b58a749e7ddbb6b7c071a6d6679d50ab9f',
    'docs/qa/subsonic_reference.py': '35cb9fa93e96d3f32a60cb737e25bd81549726221afc4d64341bce3c7d016e65',
    'docs/qa/turboprop_identity_reference.py': '624dfbc1305988bbfb324015c8b82d32dc0e1a094381cd99db8cd3c6a60289aa',
    'docs/qa/turboprop_profile_reference.py': 'fb695f2c9f968403b017af3b385e0bd05bcc8bba5c5cce124539882ea9af4806',
}
INDEPENDENT_REPLAY_HASHES.update(NEAR_STATIC_INDEPENDENT_HASHES)
LEGACY_NOTICE = "LEGACY PARTIAL IDENTITY: historical yaw_rate_p was not recorded or verified"
LEGACY_LIMIT = ("Legacy smoke checks the original partial fingerprint and replay startup under explicit "
                "baseline assumption; historical yaw_rate_p is not verified, nor whole-flight/cross-version reproduction")
REPLAY_ACCEPTANCE_TESTS = {
    "legacy_policy_test": "replay_policy::tests::legacy_requires_explicit_choice_and_selected_complete_baseline",
    "legacy_notice_test": "replay_migration_tests::light_and_swift_all_versions_keep_exact_controls_clock_rewind_and_export",
    "legacy_layout_test": "replay_migration_tests::real_replay_notices_fit_narrow_resizes_and_keep_live_tutorial_clear",
}
REQUIRED_TEXT_EVIDENCE = {
    "acceptance.json", "source-inputs.json", "dependency-inventory.json",
    "commercial-readiness.json", "commands.log", "default-swift.log",
    "absent-light-single.log", "default-rejects-legacy.log", "legacy-no-model.log",
}
PNG_NAME = "default-swift.png"
PROBE_LOG_NAME = "diagnostic-readback.log"
PROBE_PNG_NAME = "diagnostic-readback.png"
PROBE_JSON_NAME = "diagnostic-readback.json"
TEXT_EVIDENCE = REQUIRED_TEXT_EVIDENCE | {PROBE_LOG_NAME, PROBE_JSON_NAME}
PNG_EVIDENCE = {PNG_NAME, PROBE_PNG_NAME}
CAPTURE_TRACE = ("info,wgpu_core::device::global=trace,wgpu_core::device::queue=trace,"
                 "wgpu_core::command::transfer=trace,wgpu_hal::dx12=debug,"
                 "bevy_app::task_pool_plugin=trace")
# The diagnostic keeps the primary's ordinary Python launcher unchanged.
BASELINE_LAUNCH = {"creationflags": 0, "startupinfo": None}
CAPTURE_TIMEOUT_SECONDS = 180
PROCESS_CLEANUP_TIMEOUT_SECONDS = 10
PROBE_PREFIX = "FS_READBACK_PROBE"
PROBE_MAX_JSON_BYTES = 64 * 1024
PROBE_MAX_EVENTS = 32
PROBE_POLL_STATUSES = {"wait_succeeded", "queue_empty", "timeout", "wrong_submission", "unexpected_poll"}
CAPTURE_ERRORS = (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError, zlib.error)
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


# Cargo auto-discovers build scripts, binary/example/benchmark targets. These
# retain 492 preserved members, two terms inputs, one terrain test and two
# explicitly reviewed cockpit helpers.
# No on-disk glob can admit additional build, binary, example or benchmark files.
PRESERVED_CRATE_SOURCE_PATHS = frozenset(path for path in REPLAY_CONTRACT_PATHS
                                       if path.startswith("crates/")
                                       and path not in (COMPONENT_TERMS_RUNTIME_PATHS
                                                        | TERRAIN_CENTROID_RUNTIME_PATHS
                                                        | COCKPIT_RUNTIME_PATHS))
CURRENT_CRATE_SOURCE_PATHS = (PRESERVED_CRATE_SOURCE_PATHS | COMPONENT_TERMS_RUNTIME_PATHS
                              | TERRAIN_CENTROID_RUNTIME_PATHS | COCKPIT_RUNTIME_PATHS)
MODIFIED_SOURCE_BOUNDARIES = (
    ("crates/", CURRENT_CRATE_SOURCE_PATHS),
    ("docs/release/components/", COMPONENT_TERMS_DOCUMENT_PATHS),
    ("vendor/bevy_core_pipeline/", CORE_PIPELINE_SOURCE_PATHS),
    ("tools/original-highwing/", ORIGINAL_HIGHWING_SOURCE_PATHS),
    ("vendor/bevy_pbr/", VENDORED_PACKAGE_PATHS),
    ("vendor/zune-jpeg/", VENDORED_PACKAGE_PATHS),
    ("tools/validate-jpeg-replacement/", REPLACEMENT_WITNESS_PATHS),
    ("tools/validate-parallax-math/", REPLACEMENT_WITNESS_PATHS),
    ("tools/validate-parallax-replacement/", REPLACEMENT_WITNESS_PATHS),
)


def validate_modified_source_boundaries(paths):
    # Prefixes reject unexpected members; they do not admit new files.
    for prefix, declared in MODIFIED_SOURCE_BOUNDARIES:
        expected = {path for path in declared if path.startswith(prefix)}
        observed = {path for path in paths
                    if path.casefold() == prefix[:-1].casefold()
                    or path.casefold().startswith(prefix.casefold())}
        require(observed == expected,
                "modified source boundary changed: " + prefix)



def validate_modified_source_checkout(repo):
    # Git's clean status excludes ignored files. Cargo can discover a witness
    # build.rs or local config among those files, so reject them here as well.
    # This walk only rejects; acceptance still comes from the literal path sets.
    for prefix, declared in MODIFIED_SOURCE_BOUNDARIES:
        root = repo
        for part in Path(prefix).parts:
            matches = [entry for entry in root.iterdir() if entry.name.casefold() == part.casefold()]
            require(len(matches) == 1 and matches[0].name == part,
                    "modified source root case alias: " + prefix)
            root /= part
            details = root.lstat()
            require(root.is_dir() and not root.is_symlink()
                    and not getattr(details, "st_file_attributes", 0) & 0x400,
                    "non-regular/reparse modified source root: " + prefix)
        expected = {path for path in declared if path.startswith(prefix)}
        observed = set()
        for member in root.rglob("*"):
            relative = member.relative_to(repo).as_posix()
            details = member.lstat()
            require(not member.is_symlink()
                    and not getattr(details, "st_file_attributes", 0) & 0x400,
                    "modified source symlink/reparse input: " + relative)
            if member.is_dir():
                continue
            require(member.is_file() and relative in expected,
                    "unreviewed modified source checkout input: " + relative)
            observed.add(relative)
        require(observed == expected, "modified source checkout boundary changed: " + prefix)


def validate_alpha22_version_delta(previous, current):
    """Admit only one workspace and thirteen local-package version changes.

    TOML semantics fix the package/field boundary; exact replacement also rejects
    comments, whitespace, dependency order and any other incidental byte drift.
    """
    paths = set(ALPHA22_HISTORICAL_RELOCATIONS)
    require(isinstance(previous, dict) and isinstance(current, dict)
            and set(previous) == paths and set(current) == paths,
            "alpha22 version file boundary changed")
    old, new = "0.6.0-alpha.21", "0.6.0-alpha.22"
    for path in paths:
        require(isinstance(previous[path], bytes) and isinstance(current[path], bytes)
                and 0 < len(previous[path]) <= 1024 * 1024
                and 0 < len(current[path]) <= 1024 * 1024,
                "alpha22 version input exceeds byte boundary: " + path)
    before = tomllib.loads(previous["Cargo.toml"].decode("utf-8"))
    after = tomllib.loads(current["Cargo.toml"].decode("utf-8"))
    require(before.get("workspace", {}).get("package", {}).get("version") == old
            and after.get("workspace", {}).get("package", {}).get("version") == new,
            "alpha22 workspace version changed outside approved transition")
    require(len(before["workspace"]["members"]) == 13
            and set(before["workspace"]["members"])
                == {"crates/" + name for name in ALPHA22_WORKSPACE_PACKAGES},
            "alpha22 workspace membership changed")
    after["workspace"]["package"]["version"] = old
    require(after == before, "alpha22 manifest contains non-version changes")
    before = tomllib.loads(previous["Cargo.lock"].decode("utf-8"))
    after = tomllib.loads(current["Cargo.lock"].decode("utf-8"))
    require(isinstance(before.get("package"), list) and isinstance(after.get("package"), list)
            and len(before["package"]) == len(after["package"]),
            "alpha22 lock package boundary changed")
    seen = set()
    for prior, changed in zip(before["package"], after["package"]):
        name = prior.get("name")
        if name in ALPHA22_WORKSPACE_PACKAGES:
            require(name not in seen and prior.get("version") == old
                    and changed.get("name") == name and changed.get("version") == new
                    and "source" not in prior and "checksum" not in prior,
                    "alpha22 local package identity changed: " + name)
            seen.add(name)
            changed["version"] = old
        require(changed == prior, "alpha22 lock contains non-version changes")
    require(seen == ALPHA22_WORKSPACE_PACKAGES and after == before,
            "alpha22 lock workspace or metadata changed")
    for path, count in (("Cargo.toml", 1), ("Cargo.lock", 13)):
        require(previous[path].count(old.encode("ascii")) == count
                and current[path] == previous[path].replace(old.encode("ascii"), new.encode("ascii")),
                "alpha22 version bytes differ outside approved substitutions: " + path)


def validate_alpha22_replay_contract(contract):
    require(isinstance(contract, dict) and set(contract) == {"schema_version", "contract",
            "base_reviewed_source", "base_reviewed_runtime_tree", "source_migration_sha256",
            "scope", "release_authorized", "source_sha256"}
            and type(contract.get("schema_version")) is int and contract.get("schema_version") == 1
            and contract.get("contract") == ALPHA22_REPLAY_CONTRACT_ID, "invalid reviewed replay contract")
    require(contract.get("base_reviewed_source") == REVIEWED_RUNTIME_SOURCE
            and contract.get("base_reviewed_runtime_tree") == REVIEWED_RUNTIME_TREE
            and contract.get("source_migration_sha256") == ALPHA22_MIGRATION_SHA256
            and contract.get("scope") == SOURCE_ADMISSION_SCOPE
            and contract.get("release_authorized") is False, "source admission identity or scope changed")
    require(isinstance(contract.get("base_reviewed_source"), str)
            and re.fullmatch(r"[0-9a-f]{40}", contract["base_reviewed_source"]), "missing reviewed replay source")
    pins = contract.get("source_sha256")
    require(isinstance(pins, dict) and set(pins) == ALPHA22_REPLAY_CONTRACT_PATHS,
            "reviewed replay source boundary changed")
    require(all(isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value) for value in pins.values()),
            "invalid reviewed replay source digest")
    # These physical inputs never migrated. The FDM module root only gained
    # additive exports and retains its own strict reviewed whole-file guard.
    for path in ("assets/aircraft/light_single.json", "crates/flightsim-fdm/src/aircraft.rs"):
        require(pins[path] == LEGACY_SOURCE_HASHES[path], "frozen legacy input changed: " + path)
    require(pins["crates/flightsim-fdm/src/lib.rs"] == REVIEWED_ADDITIVE_FDM_LIB_SHA256,
            "frozen reviewed FDM module input changed")
    for path, expected in HISTORICAL_CONTRACT_HASHES.items():
        require(pins[path] == expected, "historical contract changed: " + path)
    require(pins[PRESERVED_RUNTIME_PATH] == PRESERVED_RUNTIME_SHA256,
            "preserved runtime contract changed")
    frozen_file = Path(__file__).with_name("full-two-aircraft-runtime-pins.json")
    require(digest(frozen_file) == PRESERVED_RUNTIME_SHA256, "preserved runtime contract changed")
    preserved = json.loads(frozen_file.read_text(encoding="utf-8"))["source_sha256"]
    migration_file = Path(__file__).with_name("component-terms-source-migration.json")
    require(digest(migration_file) == COMPONENT_TERMS_MIGRATION_SHA256
            and pins[COMPONENT_TERMS_MIGRATION_PATH] == COMPONENT_TERMS_MIGRATION_SHA256,
            "component terms migration changed")
    migration = json.loads(migration_file.read_text(encoding="utf-8"))
    require(set(migration) == {"schema_version", "identity", "base_source", "base_runtime_source",
            "base_runtime_tree", "base_replay_contract_sha256", "preserved_runtime_sha256",
            "historical_relocations", "replaced_source_sha256", "added_source_sha256"}
            and type(migration["schema_version"]) is int and migration["schema_version"] == 1
            and migration["identity"] == "full-two-aircraft-component-terms-source-migration-v1"
            and migration["base_source"] == "4d40f9abbfa90be38c91f2a1379d8355530dc2f5"
            and migration["base_runtime_source"] == REVIEWED_RUNTIME_SOURCE
            and migration["base_runtime_tree"] == REVIEWED_RUNTIME_TREE
            and migration["base_replay_contract_sha256"] == "81878db06cc1620627447317bce4d412c7e1195939ef3ef02abed13cb8db3a19"
            and migration["preserved_runtime_sha256"] == PRESERVED_RUNTIME_SHA256,
            "invalid component terms migration identity")
    relocations = migration["historical_relocations"]
    require(relocations == {COMPONENT_TERMS_MAIN_PATH: COMPONENT_TERMS_HISTORY_PATH},
            "component terms historical relocation changed")
    replaced = migration["replaced_source_sha256"]
    require(set(replaced) == {COMPONENT_TERMS_MAIN_PATH}
            and set(replaced[COMPONENT_TERMS_MAIN_PATH]) == {"previous_sha256", "sha256"}
            and replaced[COMPONENT_TERMS_MAIN_PATH]["previous_sha256"] == preserved[COMPONENT_TERMS_MAIN_PATH]
            and replaced[COMPONENT_TERMS_MAIN_PATH]["sha256"] != preserved[COMPONENT_TERMS_MAIN_PATH]
            and pins[COMPONENT_TERMS_MAIN_PATH] == replaced[COMPONENT_TERMS_MAIN_PATH]["sha256"],
            "component terms replacement changed")
    added = migration["added_source_sha256"]
    require(set(added) == COMPONENT_TERMS_SOURCE_PATHS
            and all(pins.get(path) == value for path, value in added.items()),
            "component terms source pins changed")
    terrain_file = Path(__file__).with_name("terrain-centroid-source-migration.json")
    require(digest(terrain_file) == TERRAIN_CENTROID_MIGRATION_SHA256
            and pins[TERRAIN_CENTROID_MIGRATION_PATH] == TERRAIN_CENTROID_MIGRATION_SHA256,
            "terrain centroid migration changed")
    terrain = json.loads(terrain_file.read_text(encoding="utf-8"))
    require(set(terrain) == {"schema_version", "identity", "base_source", "base_tree",
            "base_replay_contract_sha256", "previous_source_migration_sha256",
            "preserved_runtime_sha256", "historical_relocations", "replaced_source_sha256",
            "added_source_sha256"}
            and type(terrain["schema_version"]) is int and terrain["schema_version"] == 1
            and terrain["identity"] == "full-two-aircraft-terrain-centroid-source-migration-v1"
            and terrain["base_source"] == "42a7ddeae36f3022c2c77e54d5dce33f71b11bca"
            and terrain["base_tree"] == "625c39f9b182b16a0dada2dc4131434d178a0831"
            and terrain["base_replay_contract_sha256"] == "00b8aaada63fed4e10375e2eefa543eefdff7baa014b424cfb89c148e5686dad"
            and terrain["previous_source_migration_sha256"] == COMPONENT_TERMS_MIGRATION_SHA256
            and terrain["preserved_runtime_sha256"] == PRESERVED_RUNTIME_SHA256,
            "invalid terrain centroid migration identity")
    require(terrain["historical_relocations"] == {
            TERRAIN_CENTROID_DETAIL_PATH: TERRAIN_CENTROID_HISTORY_PATH},
            "terrain centroid historical relocation changed")
    terrain_replaced = terrain["replaced_source_sha256"]
    require(isinstance(terrain_replaced, dict)
            and set(terrain_replaced) == set(TERRAIN_CENTROID_PREVIOUS_SHA256),
            "terrain centroid replacement boundary changed")
    for path, previous in TERRAIN_CENTROID_PREVIOUS_SHA256.items():
        row = terrain_replaced[path]
        require(isinstance(row, dict) and set(row) == {"previous_sha256", "sha256"}
                and row["previous_sha256"] == previous
                and row["sha256"] != previous and pins[path] == row["sha256"],
                "terrain centroid replacement changed: " + path)
    terrain_added = terrain["added_source_sha256"]
    require(isinstance(terrain_added, dict) and set(terrain_added) == TERRAIN_CENTROID_RUNTIME_PATHS
            and all(pins.get(path) == value for path, value in terrain_added.items()),
            "terrain centroid source additions changed")
    require(pins[TERRAIN_CENTROID_HISTORY_PATH] == preserved[TERRAIN_CENTROID_DETAIL_PATH]
            == TERRAIN_CENTROID_PREVIOUS_SHA256[TERRAIN_CENTROID_DETAIL_PATH],
            "terrain centroid historical bytes changed")
    stitch_file = Path(__file__).with_name("terrain-stitch-source-migration.json")
    require(digest(stitch_file) == TERRAIN_STITCH_MIGRATION_SHA256
            and pins[TERRAIN_STITCH_MIGRATION_PATH] == TERRAIN_STITCH_MIGRATION_SHA256,
            "terrain stitch migration changed")
    stitch = json.loads(stitch_file.read_text(encoding="utf-8"))
    require(set(stitch) == {"schema_version", "identity", "base_source", "base_tree",
            "base_source_kind", "public_base_source", "public_base_tree",
            "base_replay_contract_path", "base_replay_contract_sha256",
            "previous_source_migration_sha256", "preserved_runtime_sha256",
            "historical_relocations", "replaced_source_sha256", "added_source_sha256"}
            and type(stitch["schema_version"]) is int and stitch["schema_version"] == 1
            and stitch["identity"] == "full-two-aircraft-terrain-stitch-source-migration-v1"
            and stitch["base_source"] == "5eaaff379f19cc986fa5600619f1491213e9da0d"
            and stitch["base_tree"] == "a469ee76cd0d7aabf3dd2c07114d44c0bbdf5d37"
            and stitch["base_source_kind"] == "unpublished-local-checkpoint"
            and stitch["public_base_source"] == "42a7ddeae36f3022c2c77e54d5dce33f71b11bca"
            and stitch["public_base_tree"] == "625c39f9b182b16a0dada2dc4131434d178a0831"
            and stitch["base_replay_contract_path"] == TERRAIN_STITCH_BASE_CONTRACT_PATH
            and stitch["base_replay_contract_sha256"] == TERRAIN_STITCH_BASE_CONTRACT_SHA256
            and stitch["previous_source_migration_sha256"] == TERRAIN_CENTROID_MIGRATION_SHA256
            and stitch["preserved_runtime_sha256"] == PRESERVED_RUNTIME_SHA256,
            "invalid terrain stitch migration identity")
    require(stitch["historical_relocations"] == TERRAIN_STITCH_HISTORICAL_RELOCATIONS
            and stitch["added_source_sha256"] == {},
            "terrain stitch relocation or additions changed")
    stitch_replaced = stitch["replaced_source_sha256"]
    require(isinstance(stitch_replaced, dict)
            and set(stitch_replaced) == set(TERRAIN_STITCH_PREVIOUS_SHA256),
            "terrain stitch replacement boundary changed")
    for path, previous in TERRAIN_STITCH_PREVIOUS_SHA256.items():
        row = stitch_replaced[path]
        require(isinstance(row, dict) and set(row) == {"previous_sha256", "sha256"}
                and row["previous_sha256"] == previous == preserved[path]
                and row["sha256"] != previous and pins[path] == row["sha256"]
                and pins[TERRAIN_STITCH_HISTORICAL_RELOCATIONS[path]] == previous,
                "terrain stitch replacement changed: " + path)
    require(len(preserved) == 497
            and all(pins.get(HISTORICAL_RUNTIME_RELOCATIONS.get(path, path)) == value
                    for path, value in preserved.items()),
            "preserved runtime source pins changed")
    base_contract_file = Path(__file__).parent.parent / TERRAIN_STITCH_BASE_CONTRACT_PATH
    require(digest(base_contract_file) == TERRAIN_STITCH_BASE_CONTRACT_SHA256
            and pins[TERRAIN_STITCH_BASE_CONTRACT_PATH] == TERRAIN_STITCH_BASE_CONTRACT_SHA256,
            "terrain stitch base contract changed")
    base_contract = json.loads(base_contract_file.read_text(encoding="utf-8"))
    base_pins = base_contract["source_sha256"]
    require(base_contract["contract"] == "full-two-aircraft-terrain-centroid-source-v1"
            and base_contract["source_migration_sha256"] == TERRAIN_CENTROID_MIGRATION_SHA256
            and set(base_pins) == ALPHA22_REPLAY_CONTRACT_PATHS - TERRAIN_STITCH_SOURCE_PATHS - ALPHA22_SOURCE_PATHS
            and all(pins.get(ALPHA22_HISTORICAL_RELOCATIONS.get(
                        path, TERRAIN_STITCH_HISTORICAL_RELOCATIONS.get(path, path))) == value
                    for path, value in base_pins.items()),
            "terrain stitch prior source pins changed")
    root = Path(__file__).parent.parent
    version_file = root / ALPHA22_MIGRATION_PATH
    require(digest(version_file) == ALPHA22_MIGRATION_SHA256
            and pins[ALPHA22_MIGRATION_PATH] == ALPHA22_MIGRATION_SHA256,
            "alpha22 version migration changed")
    version = json.loads(version_file.read_text(encoding="utf-8"))
    require(set(version) == {"schema_version", "identity", "base_source", "base_tree",
            "base_source_kind", "public_base_source", "public_base_tree",
            "base_contract_sha256", "previous_source_migration_sha256", "preserved_runtime_sha256",
            "previous_version", "version", "historical_relocations", "replaced_source_sha256",
            "added_source_sha256"}
            and type(version["schema_version"]) is int and version["schema_version"] == 1
            and version["identity"] == "full-two-aircraft-alpha22-version-source-migration-v1"
            and version["base_source"] == "0bc4a4961c677a5173350630bf2641a2c1b40b94"
            and version["base_tree"] == "7ac0cedd0963ac4133fa8c5251f57f26037324d7"
            and version["base_source_kind"] == "unpublished-local-checkpoint"
            and version["public_base_source"] == "3574bf175c66851df51ae89903e04a13f4a172e6"
            and version["public_base_tree"] == version["base_tree"]
            and version["base_contract_sha256"] == ALPHA22_BASE_CONTRACT_HASHES
            and version["previous_source_migration_sha256"] == TERRAIN_STITCH_MIGRATION_SHA256
            and version["preserved_runtime_sha256"] == PRESERVED_RUNTIME_SHA256
            and version["previous_version"] == "0.6.0-alpha.21"
            and version["version"] == "0.6.0-alpha.22"
            and version["historical_relocations"] == ALPHA22_HISTORICAL_RELOCATIONS
            and version["added_source_sha256"] == {}, "invalid alpha22 version migration identity")
    for path, expected in ALPHA22_BASE_CONTRACT_HASHES.items():
        require(digest(root / path) == expected and pins[path] == expected,
                "alpha22 historical contract changed: " + path)
    previous_contract = json.loads((root / ALPHA22_BASE_CONTRACT_PATH).read_text(encoding="utf-8"))
    previous_pins = previous_contract["source_sha256"]
    require(previous_contract["contract"] == "full-two-aircraft-terrain-stitch-source-v1"
            and previous_contract["source_migration_sha256"] == TERRAIN_STITCH_MIGRATION_SHA256
            and len(previous_pins) == 934
            and set(previous_pins) == ALPHA22_REPLAY_CONTRACT_PATHS - ALPHA22_SOURCE_PATHS
            and all(pins.get(ALPHA22_HISTORICAL_RELOCATIONS.get(path, path)) == value
                    for path, value in previous_pins.items()),
            "alpha22 prior source pins changed")
    replacements = version["replaced_source_sha256"]
    require(isinstance(replacements, dict) and set(replacements) == set(ALPHA22_PREVIOUS_SHA256),
            "alpha22 version replacement boundary changed")
    previous_bytes, current_bytes = {}, {}
    for path, previous in ALPHA22_PREVIOUS_SHA256.items():
        historical = ALPHA22_HISTORICAL_RELOCATIONS[path]
        previous_bytes[path] = (root / historical).read_bytes()
        require(hashlib.sha256(previous_bytes[path]).hexdigest() == previous
                == pins[historical] == previous_pins[path],
                "alpha22 historical Cargo bytes changed: " + path)
        # Derive the sole admitted current bytes from the immutable witness.
        # Actual checkout/canonical bytes are compared in source_inputs below.
        current_bytes[path] = previous_bytes[path].replace(b"0.6.0-alpha.21", b"0.6.0-alpha.22")
        expected = hashlib.sha256(current_bytes[path]).hexdigest()
        require(replacements[path] == {"previous_sha256": previous, "sha256": expected}
                and pins[path] == expected, "alpha22 version replacement changed: " + path)
    validate_alpha22_version_delta(previous_bytes, current_bytes)
    return contract

def validate_replay_contract(contract):
    """Admit exactly the frozen cockpit delta over the complete alpha.22 proof."""
    require(isinstance(contract, dict) and set(contract) == {"schema_version", "contract",
            "base_reviewed_source", "base_reviewed_runtime_tree", "source_migration_sha256",
            "scope", "release_authorized", "source_sha256"}
            and type(contract.get("schema_version")) is int and contract["schema_version"] == 1
            and contract.get("contract") == REPLAY_CONTRACT_ID,
            "invalid reviewed replay contract")
    require(contract.get("base_reviewed_source") == REVIEWED_RUNTIME_SOURCE
            and contract.get("base_reviewed_runtime_tree") == REVIEWED_RUNTIME_TREE
            and contract.get("source_migration_sha256") == COCKPIT_MIGRATION_SHA256
            and contract.get("scope") == SOURCE_ADMISSION_SCOPE
            and contract.get("release_authorized") is False,
            "source admission identity or scope changed")
    pins = contract.get("source_sha256")
    require(isinstance(pins, dict) and set(pins) == REPLAY_CONTRACT_PATHS,
            "reviewed replay source boundary changed")
    require(all(isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value)
                for value in pins.values()), "invalid reviewed replay source digest")
    # Keep the existing current-contract physics guards and diagnostics explicit;
    # the historical proof below independently retains the same frozen anchors.
    for path in ("assets/aircraft/light_single.json", "crates/flightsim-fdm/src/aircraft.rs"):
        require(pins[path] == LEGACY_SOURCE_HASHES[path], "frozen legacy input changed: " + path)
    require(pins["crates/flightsim-fdm/src/lib.rs"] == REVIEWED_ADDITIVE_FDM_LIB_SHA256,
            "frozen reviewed FDM module input changed")
    root = Path(__file__).parent.parent
    for path, expected in COCKPIT_BASE_CONTRACT_HASHES.items():
        require(digest(root / path) == expected and pins[path] == expected,
                "cockpit historical contract changed: " + path)
    previous = json.loads((root / COCKPIT_BASE_CONTRACT_PATH).read_text(encoding="utf-8"))
    # This invokes every existing migration, preserved runtime and physics guard
    # against the byte-exact historical contract, without rewriting any history.
    validate_alpha22_replay_contract(previous)
    previous_pins = previous["source_sha256"]
    require(len(previous_pins) == 940
            and set(previous_pins) == REPLAY_CONTRACT_PATHS - COCKPIT_SOURCE_PATHS,
            "cockpit prior source boundary changed")
    for path, expected in previous_pins.items():
        historical = COCKPIT_HISTORICAL_RELOCATIONS.get(path, path)
        require(pins.get(historical) == expected,
                "cockpit prior source pins changed (preserved runtime/historical contract): " + path)
    require(digest(root / COCKPIT_MIGRATION_PATH) == COCKPIT_MIGRATION_SHA256
            and pins[COCKPIT_MIGRATION_PATH] == COCKPIT_MIGRATION_SHA256,
            "cockpit source migration changed")
    migration = json.loads((root / COCKPIT_MIGRATION_PATH).read_text(encoding="utf-8"))
    require(set(migration) == {"schema_version", "identity", "base_source", "base_tree",
            "base_source_kind", "base_contract_sha256", "previous_source_migration_sha256",
            "preserved_runtime_sha256", "historical_relocations", "replaced_source_sha256",
            "added_source_sha256"}
            and type(migration["schema_version"]) is int and migration["schema_version"] == 1
            and migration["identity"] == "full-two-aircraft-cockpit-source-migration-v1"
            and migration["base_source"] == "be5873840dc86eb551971bad6e7290840eaf9cd2"
            and migration["base_tree"] == "49479664f838700cf52ed7621a727db7732752e2"
            and migration["base_source_kind"] == "published-commit"
            and migration["base_contract_sha256"] == COCKPIT_BASE_CONTRACT_HASHES
            and migration["previous_source_migration_sha256"] == ALPHA22_MIGRATION_SHA256
            and migration["preserved_runtime_sha256"] == PRESERVED_RUNTIME_SHA256
            and migration["historical_relocations"] == COCKPIT_HISTORICAL_RELOCATIONS,
            "invalid cockpit source migration identity")
    replacements = migration["replaced_source_sha256"]
    require(isinstance(replacements, dict) and set(replacements) == set(COCKPIT_PREVIOUS_SHA256),
            "cockpit replacement boundary changed")
    for path, expected in COCKPIT_PREVIOUS_SHA256.items():
        historical = COCKPIT_HISTORICAL_RELOCATIONS[path]
        row = replacements[path]
        require(isinstance(row, dict) and set(row) == {"previous_sha256", "sha256"}
                and row["previous_sha256"] == expected == previous_pins[path]
                and digest(root / historical) == expected == pins[historical]
                and row["sha256"] != expected and pins[path] == row["sha256"],
                "cockpit replacement changed: " + path)
    additions = migration["added_source_sha256"]
    require(isinstance(additions, dict) and set(additions) == COCKPIT_RUNTIME_PATHS
            and all(pins.get(path) == expected for path, expected in additions.items()),
            "cockpit source additions changed")
    return contract


def load_replay_contract(repo):
    return validate_replay_contract(json.loads((repo / REPLAY_CONTRACT_PATH).read_text(encoding="utf-8")))


MAX_CANONICAL_BLOB_BYTES = 32 * 1024 * 1024
MAX_CANONICAL_BATCH_BYTES = 128 * 1024 * 1024
MAX_CANONICAL_BATCH_OBJECTS = 4096


def canonical_blobs(repo, sizes):
    """Read the requested immutable objects once, without a cross-call cache.

    Sizes come from this invocation's canonical ls-tree, not checkout lengths.
    A fixed expected frame length bounds the accepted response. Every object ID,
    blob type, decimal size, binary byte, separator and end-of-response is checked.
    """
    require(isinstance(sizes, dict) and 0 < len(sizes) <= MAX_CANONICAL_BATCH_OBJECTS,
            "invalid canonical batch object count")
    require(all(isinstance(oid, str) and re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", oid)
                and type(size) is int and 0 <= size <= MAX_CANONICAL_BLOB_BYTES
                for oid, size in sizes.items()), "invalid canonical batch object identity or size")
    frames = [(oid, size, f"{oid} blob {size}\n".encode("ascii")) for oid, size in sizes.items()]
    expected_bytes = sum(len(header) + size + 1 for _, size, header in frames)
    require(expected_bytes <= MAX_CANONICAL_BATCH_BYTES, "canonical batch exceeds byte bound")
    # Spool subprocess output before loading it, so malformed/trailing output
    # cannot amplify the in-memory read beyond the declared 128 MiB boundary.
    with tempfile.TemporaryFile() as output, tempfile.TemporaryFile() as errors:
        subprocess.run(["git", "cat-file", "--batch"], cwd=repo,
                       input=("\n".join(sizes) + "\n").encode("ascii"),
                       stdout=output, stderr=errors, timeout=60, check=True)
        require(output.tell() == expected_bytes, "canonical batch truncated or has extra bytes")
        output.seek(0)
        raw = output.read(expected_bytes + 1)
    require(len(raw) == expected_bytes, "canonical batch truncated or has extra bytes")
    blobs, offset = {}, 0
    for oid, size, header in frames:
        require(raw[offset:offset + len(header)] == header,
                "canonical batch object ID, type, size or framing changed")
        offset += len(header)
        blobs[oid] = raw[offset:offset + size]
        offset += size
        require(raw[offset:offset + 1] == b"\n", "canonical batch separator changed")
        offset += 1
    require(offset == len(raw), "canonical batch trailing bytes")
    return blobs


def source_inputs(repo, expected):
    require(re.fullmatch(r"[0-9a-f]{40}", expected), "expected source must be a full lowercase SHA")
    require(git(repo, "rev-parse", "HEAD") == expected, "checkout is not the expected source")
    require(not git(repo, "status", "--porcelain", "--untracked-files=all"), "source checkout must be clean")
    tree = subprocess.check_output(["git", "ls-tree", "-r", "-l", "-z", "HEAD"], cwd=repo).decode("utf-8")
    records = []
    object_sizes = {}
    for entry in sorted(p for p in tree.split("\0") if p):
        metadata, relative = entry.split("\t", 1)
        mode, kind, object_id, size_text = metadata.split()
        require(size_text.isascii() and size_text.isdecimal(), "invalid canonical blob size")
        size = int(size_text)
        require(object_id not in object_sizes or object_sizes[object_id] == size,
                "inconsistent canonical object size")
        object_sizes[object_id] = size
        require(kind == "blob" and mode in ("100644", "100755"), f"non-regular canonical input: {relative}")
        path = repo / relative
        require(path.is_file() and not path.is_symlink(), f"non-regular source input: {relative}")
        records.append({"path": relative, "canonical_git_blob": object_id, "git_mode": mode,
                        "checkout_bytes": path.stat().st_size, "checkout_sha256": digest(path)})
    records.sort(key=lambda record: record["path"])
    by_path = {record["path"]: record for record in records}
    validate_modified_source_boundaries(by_path)
    validate_modified_source_checkout(repo)
    require(REPLAY_CONTRACT_PATH in by_path, "reviewed replay contract must be tracked")
    contract_record = by_path[REPLAY_CONTRACT_PATH]
    contract_blob = subprocess.check_output(
        ["git", "cat-file", "blob", contract_record["canonical_git_blob"]], cwd=repo)
    require(hashlib.sha256(contract_blob).hexdigest() == contract_record["checkout_sha256"],
            "reviewed replay contract checkout differs from canonical Git blob")
    contract = load_replay_contract(repo)
    pins = {**contract["source_sha256"], **INDEPENDENT_REPLAY_HASHES}
    for relative in pins:
        require(relative in by_path, "missing reviewed replay source: " + relative)
    objects = {by_path[path]["canonical_git_blob"]: object_sizes[by_path[path]["canonical_git_blob"]]
               for path in pins}
    blobs = canonical_blobs(repo, objects)
    reviewed_sources = {}
    for relative, expected_hash in pins.items():
        require(relative in by_path, "missing reviewed replay source: " + relative)
        record = by_path[relative]
        blob = blobs[record["canonical_git_blob"]]
        canonical_hash = hashlib.sha256(blob).hexdigest()
        diagnostic = (f"{relative}; expected_sha256={expected_hash}; canonical_sha256={canonical_hash}; "
                      f"checkout_sha256={record['checkout_sha256']}")
        require(canonical_hash == expected_hash, "reviewed replay canonical baseline changed: " + diagnostic)
        # Exact compiled bytes still matter. No hashing normalization or newline
        # equivalence is accepted; the workflow explicitly selects LF checkout.
        require(record["checkout_sha256"] == canonical_hash,
                "reviewed replay checkout differs from canonical Git blob (check core.eol=lf): " + diagnostic)
        reviewed_sources[relative] = {"canonical_sha256": canonical_hash, "canonical_bytes": len(blob),
                                    "canonical_git_blob": record["canonical_git_blob"],
                                    "checkout_sha256": record["checkout_sha256"],
                                    "checkout_bytes": record["checkout_bytes"]}
    return {"schema_version": 3, "source_sha": expected, "source_tree": git(repo, "rev-parse", "HEAD^{tree}"),
            "canonical_git_object_format": git(repo, "rev-parse", "--show-object-format"),
            "legacy_baseline": LEGACY_BASELINE, "legacy_source_sha256": LEGACY_SOURCE_HASHES,
            "replay_contract": contract, "replay_contract_text": contract_blob.decode("utf-8"),
            "replay_contract_sha256": digest(repo / REPLAY_CONTRACT_PATH),
            "independent_replay_sha256": INDEPENDENT_REPLAY_HASHES,
            "reviewed_replay_source_evidence": reviewed_sources, "files": records}


def candidate_commands():
    common = ["--locked", "--release", "-j", "2", "--target", TARGET, "-p", "flightsim-app",
              "--features", ",".join(FEATURES)]
    commands = {
        "build": ["cargo", "+" + TOOLCHAIN, "build", *common],
        "identity_test": ["cargo", "+" + TOOLCHAIN, "test", *common, "--bin", "flightsim-app",
                          "distribution::tests::explicit_legacy_aircraft_retains_fingerprint_and_is_not_remapped",
                          "--", "--exact"],
        "metadata": ["cargo", "+" + TOOLCHAIN, "metadata", "--locked", "--format-version", "1",
                     "--filter-platform", TARGET, "--features",
                     ",".join("flightsim-app/" + feature for feature in FEATURES)],
    }
    for name, test in REPLAY_ACCEPTANCE_TESTS.items():
        commands[name] = ["cargo", "+" + TOOLCHAIN, "test", *common,
                          "--bin", "flightsim-app", test, "--", "--exact"]
    return commands


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


def validate_distribution(info, staged):
    require(isinstance(info, dict) and isinstance(staged, dict), "distribution identity must be an object")
    # Equality alone would accept JSON 0 as false, including in the attestation.
    require(info.get("region_downloads") is False and staged.get("region_downloads") is False,
            "this offline candidate requires explicit region_downloads=false")
    require(info == staged, "extracted distribution identity changed")
    # Bind exported evidence too: equality with itself is not proof of the
    # selected model, package membership or platform. Explicit external v2/v4
    # support in this executable never broadens this Swift-only qualification.
    expected = {
        "schema_version": 1, "package": "flightsim-app", "profile": "commercial-staging",
        "default_aircraft": "swift-sport", "default_model": "aircraft/swift_sport.glb",
        "bundled_aircraft": ["swift-sport"], "release_authorized": False,
        "target_os": "windows", "target_arch": "x86_64", "target_env": "msvc",
    }
    require(all(info.get(key) == value for key, value in expected.items())
            and type(info.get("schema_version")) is int
            and info.get("release_authorized") is False
            and isinstance(info.get("package_version"), str) and bool(info["package_version"]),
            "candidate distribution must be the Swift-only Windows MSVC build")


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
    require(path.stat().st_size <= MAX_EVIDENCE_BYTES, "invalid PNG signature/size")
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


def legacy_capture_command(executable, fixture, screenshot):
    return [str(executable), "--aircraft", "light-single", "--no-model",
            "--legacy-replay-compatibility", "--replay", str(fixture),
            "--screenshot", str(screenshot), "--screenshot-delay", "5",
            "--exit-after-screenshot", "--view", "chase"]


def validate_legacy_smoke(log, exit_code):
    validate_smoke(log, exit_code, model=False)
    require(LEGACY_NOTICE in ANSI.sub("", log), "legacy playback lacks full partial-identity limitation")


def validate_legacy_rejection(log, exit_code):
    require(exit_code == 2 and "aircraft/FDM model mismatch" in log
            and "legacy partial fingerprint " + LEGACY_FINGERPRINT in log,
            "default Swift did not reject the actual legacy identity in hexadecimal")


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


def capture_command(executable, screenshot):
    return [str(executable), "--screenshot", str(screenshot), "--screenshot-delay", "5",
            "--exit-after-screenshot", "--view", "chase"]


def diagnostic_capture_command(executable, screenshot):
    return capture_command(executable, screenshot) + ["--windows-readback-diagnostic"]


def parse_readback_log(log):
    """Accept historical v1 and bounded v2; neither protocol qualifies capture."""
    lines = [line for line in ANSI.sub("", log).splitlines() if PROBE_PREFIX in line]
    if lines and lines[0] == PROBE_PREFIX + " event=enabled version=2":
        return parse_readback_log_v2(lines)
    return parse_readback_log_v1(log)


def parse_readback_log_v2(lines):
    boolean = {"true", "false"}
    elapsed = {"elapsed_ms": 180_000}
    fields = {
        "prescene_begin": {},
        "prescene_submitted": elapsed,
        "prescene_map_register_enter": elapsed,
        "prescene_map_registered": elapsed,
        "prescene_poll_enter": {"gpu_timeout_ms": {"5000"}, **elapsed},
        "prescene_poll_return": {"status": PROBE_POLL_STATUSES, "wall_ms": 180_000, **elapsed},
        "prescene_map_callback": {"result": {"ok", "error"}, "cancelled": boolean,
                                  "phase": {"startup", "cleanup"}, **elapsed},
        "prescene_pixels": {"valid": boolean, "count": {"16"}, **elapsed},
        "prescene_summary": {"submitted": {"true"}, "map_callback": {"missing", "ok", "error"},
                             "pixels": {"missing", "valid", "invalid"}, "poll": PROBE_POLL_STATUSES,
                             "cleanup": {"true"}, **elapsed},
        "scene_register": {"frame": {"1", "2", "4", "8", "16", "30"}, **elapsed},
        "scene_callback": {"frame": {"1", "2", "4", "8", "16", "30"}, "cancelled": boolean,
                           "phase": {"active", "deadline", "late_probe", "owner_teardown", "owner_released"}, **elapsed},
        "scene_summary": {"registered": 6, "completed": 6, "cleanup": {"true"},
                          "reason": {"late_probe", "deadline"}, **elapsed},
    }
    require(len(lines) <= 64, "v2 readback event count exceeds bound")
    events, old_lines, seen = [], [], set()
    prescene, registered, completed = {}, {}, {}
    prescene_summary = scene_summary = late_summary = None
    prescene_cleanup = scene_cleanup = False
    scene_close_phase = None
    predecessors = {
        "prescene_submitted": "prescene_begin",
        "prescene_map_register_enter": "prescene_submitted",
        "prescene_map_registered": "prescene_map_register_enter",
        "prescene_poll_enter": "prescene_map_registered",
        "prescene_poll_return": "prescene_poll_enter",
        "prescene_map_callback": "prescene_map_register_enter",
        "prescene_pixels": "prescene_poll_return",
        "prescene_summary": "prescene_poll_return",
    }
    for line in lines:
        require(len(line) <= 1024 and line.startswith(PROBE_PREFIX + " "), "malformed readback event prefix/length")
        record = {}
        for token in line[len(PROBE_PREFIX) + 1:].split(" "):
            require(re.fullmatch(r"[a-z_]+=[a-z0-9_]+", token), "malformed readback event field")
            key, value = token.split("=", 1)
            require(key not in record, "duplicate readback event field")
            record[key] = value
        event = record.pop("event", None)
        if event not in fields:
            # Feed unchanged old events through the original strict state machine.
            # Only this exact version marker is normalized; unknown fields remain errors.
            if event == "enabled":
                require(record == {"version": "2"}, "invalid v2 enabled event")
                old_lines.append(PROBE_PREFIX + " event=enabled")
            else:
                require(prescene_summary is not None, "late probe precedes pre-scene cleanup")
                old_lines.append(line)
                if event == "summary":
                    late_summary = parse_readback_log_v1("\n".join(old_lines))["summary"]
            events.append(event)
            continue
        require(set(record) == set(fields[event]), "unexpected v2 readback fields")
        converted = {"event": event}
        for key, allowed in fields[event].items():
            value = record[key]
            if type(allowed) is int:
                require(re.fullmatch(r"0|[1-9][0-9]{0,6}", value) and int(value) <= allowed,
                        "v2 readback integer exceeds bound")
                converted[key] = int(value)
            else:
                require(value in allowed, "invalid v2 readback value")
                converted[key] = int(value) if key == "frame" else value == "true" if allowed <= boolean else value
        identity = (event, converted.get("frame"))
        require(identity not in seen, "duplicate v2 readback event")
        seen.add(identity)
        if event.startswith("prescene_"):
            if prescene_summary is not None or prescene_cleanup:
                require(event == "prescene_summary" or (event == "prescene_map_callback" and converted["cancelled"]),
                        "active pre-scene event after cleanup")
            if event in predecessors:
                require(predecessors[event] in prescene, "pre-scene causal predecessor missing")
                prior = prescene[predecessors[event]]
                require(converted["elapsed_ms"] >= prior.get("elapsed_ms", 0), "pre-scene time precedes predecessor")
            if event == "prescene_poll_return":
                interval = converted["elapsed_ms"] - prescene["prescene_poll_enter"]["elapsed_ms"]
                require(converted["wall_ms"] <= interval + 1, "pre-scene poll wall time exceeds elapsed interval")
            if event == "prescene_map_callback":
                require(converted["cancelled"] == (converted["phase"] == "cleanup"), "pre-scene callback phase differs")
                prescene_cleanup |= converted["cancelled"]
            if event == "prescene_pixels":
                callback = prescene.get("prescene_map_callback", {})
                require(callback.get("result") == "ok", "pre-scene pixels lack successful map")
                require(converted["elapsed_ms"] >= callback["elapsed_ms"], "pre-scene pixels precede map callback time")
            if event == "prescene_summary":
                require(converted["elapsed_ms"] >= max(item.get("elapsed_ms", 0) for item in prescene.values()),
                        "pre-scene summary time precedes observations")
                expected = prescene_observations(prescene)
                require(converted["poll"] == expected["poll"] and converted["map_callback"] == expected["map_callback"]
                        and converted["pixels"] == expected["pixels"], "pre-scene summary contradicts events")
                prescene_summary = converted
            if not converted.get("cancelled", False):
                prescene[event] = converted
        else:
            require(prescene_summary is not None, "scene observation precedes pre-scene cleanup")
            if scene_summary is not None or scene_cleanup:
                require(event == "scene_summary" or (event == "scene_callback" and converted["cancelled"]),
                        "active scene event after cleanup")
            if event == "scene_register":
                expected_frames = (1, 2, 4, 8, 16, 30)
                require(len(registered) < 6 and converted["frame"] == expected_frames[len(registered)],
                        "scene frame registration sequence differs")
                require(converted["elapsed_ms"] < 60000, "scene registration after deadline")
                if registered:
                    require(converted["elapsed_ms"] >= next(reversed(registered.values()))["elapsed_ms"], "scene registration time regressed")
                registered[converted["frame"]] = converted
            elif event == "scene_callback":
                frame = converted["frame"]
                require(frame in registered and converted["elapsed_ms"] >= registered[frame]["elapsed_ms"],
                        "scene callback precedes registration")
                require(converted["cancelled"] == (converted["phase"] != "active"), "scene callback phase differs")
                if scene_summary is not None:
                    require(converted["phase"] in {scene_summary["reason"], "owner_released"},
                            "scene callback contradicts cleanup reason")
                if converted["phase"] == "deadline":
                    require(converted["elapsed_ms"] >= 60000, "scene deadline callback before cutoff")
                if converted["phase"] == "late_probe":
                    require(late_summary is not None and converted["elapsed_ms"] >= late_summary["elapsed_ms"],
                            "scene late callback lacks late-probe completion")
                if converted["cancelled"]:
                    scene_close_phase = converted["phase"] if scene_close_phase is None else scene_close_phase
                scene_cleanup |= converted["cancelled"]
                if not converted["cancelled"]:
                    require(converted["elapsed_ms"] < 60000, "active scene callback after deadline")
                    completed[frame] = converted
            else:
                require(scene_close_phase != "owner_teardown", "scene summary after owner teardown")
                require(scene_close_phase != "deadline" or converted["reason"] == "deadline", "scene deadline reason differs")
                require(converted["registered"] == len(registered) and converted["completed"] == len(completed),
                        "scene summary contradicts observations")
                require(converted["elapsed_ms"] >= max((item["elapsed_ms"] for item in (*registered.values(), *completed.values())), default=0),
                        "scene summary time precedes observations")
                require((converted["reason"] == "deadline" and converted["elapsed_ms"] >= 60000)
                        or (converted["reason"] == "late_probe" and late_summary is not None
                            and late_summary["elapsed_ms"] <= converted["elapsed_ms"] < 60000),
                        "scene cleanup lacks deadline or late-probe completion")
                scene_summary = converted
        events.append(converted)
    late = parse_readback_log_v1("\n".join(old_lines))
    old_events = {record["event"]: record for record in late["events"]}
    events = [({**old_events[event], **({"version": 2} if event == "enabled" else {})}
               if isinstance(event, str) else event) for event in events]
    return {"schema_version": 2, "protocol": PROBE_PREFIX + "/v2", "events": events,
            "summary": late["summary"], "prescene_summary": prescene_summary, "scene_summary": scene_summary,
            "observations": {**late["observations"], "prescene": prescene_observations(prescene),
                             "scene": {"registered_frames": list(registered), "completed_frames": list(completed),
                                       "completion_elapsed_ms": {str(frame): item["elapsed_ms"] for frame, item in completed.items()}}}}


def prescene_observations(events):
    poll = events.get("prescene_poll_return", {}).get("status", "entered_without_return" if "prescene_poll_enter" in events else "not_entered")
    pixel = events.get("prescene_pixels")
    pixels = ("valid" if pixel["valid"] else "invalid") if pixel else "missing"
    return {"gpu_completion_observed": poll in {"wait_succeeded", "queue_empty"} or pixels == "valid",
            "map_callback": events.get("prescene_map_callback", {}).get("result", "missing"),
            "pixels": pixels, "poll": poll}


def parse_readback_log_v1(log):
    """Strict bounded projection of diagnostic events, never an acceptance gate."""
    boolean = {"true", "false"}
    phase = {"normal", "poll", "after_poll", "cleanup"}
    fields = {
        name: {} for name in ("enabled", "armed", "submitted", "map_register_enter", "map_registered",
                              "async_started", "async_waiting", "async_signal", "async_resumed")
    }
    fields.update({
        "map_callback": {"result": {"ok", "error"}, "cancelled": boolean, "phase": phase},
        "pixels": {"valid": boolean, "count": {"16"}},
        "queue_callback": {"cancelled": boolean, "phase": phase},
        "poll_enter": {"gpu_timeout_ms": {"250"}, "elapsed_ms": 180_000},
        "poll_return": {"status": PROBE_POLL_STATUSES, "wall_ms": 180_000},
        "summary": {"submitted": {"true"}, "queue_callback": boolean,
                    "map_callback": {"missing", "ok", "error"}, "pixels": {"missing", "valid", "invalid"},
                    "async_started": boolean, "async_waiting": boolean, "async_signal": boolean,
                    "async_resumed": boolean, "poll": {"not_entered"} | PROBE_POLL_STATUSES,
                    "cleanup": {"true"}, "render_frames": 1_000_000, "elapsed_ms": 180_000},
    })
    events, seen, active, summary, cleaning_up = [], set(), {}, None, False
    for line in ANSI.sub("", log).splitlines():
        if PROBE_PREFIX not in line:
            continue
        require(len(line) <= 1024 and line.startswith(PROBE_PREFIX + " "), "malformed readback event prefix/length")
        tokens = line[len(PROBE_PREFIX) + 1:].split(" ")
        record = {}
        for token in tokens:
            require(re.fullmatch(r"[a-z_]+=[a-z0-9_]+", token), "malformed readback event field")
            key, value = token.split("=", 1)
            require(key not in record, "duplicate readback event field")
            record[key] = value
        event = record.pop("event", None)
        require(event in fields and event not in seen, "unknown or duplicate readback event")
        require(len(events) < PROBE_MAX_EVENTS and set(record) == set(fields[event]), "unexpected readback event fields/count")
        converted = {"event": event}
        for key, allowed in fields[event].items():
            value = record[key]
            if type(allowed) is int:
                require(re.fullmatch(r"0|[1-9][0-9]{0,6}", value) and int(value) <= allowed,
                        "readback event integer exceeds bound")
                converted[key] = int(value)
            else:
                require(value in allowed, "invalid readback event value")
                converted[key] = value == "true" if allowed <= boolean else value
        if event in ("map_callback", "queue_callback"):
            require(converted["cancelled"] == (converted["phase"] == "cleanup"), "readback callback cancellation/phase differs")
            if converted["phase"] == "normal":
                require("poll_enter" not in active, "readback normal callback occurred after poll entry")
            elif converted["phase"] == "poll":
                require("poll_enter" in active and "poll_return" not in active, "readback poll callback outside poll")
            elif converted["phase"] == "after_poll":
                require("poll_return" in active, "readback callback precedes poll return")
        if cleaning_up:
            require(event == "summary" or converted.get("cancelled") is True,
                    "active readback event after cleanup")
        if summary is not None:
            require(event in ("map_callback", "queue_callback") and converted["cancelled"],
                    "readback event after terminal summary")
        if not events:
            require(event == "enabled", "readback events lack initial enabled record")
        predecessors = {
            "armed": "enabled", "submitted": "armed", "map_register_enter": "submitted",
            "map_registered": "map_register_enter", "map_callback": "map_register_enter",
            "queue_callback": "map_registered", "pixels": "map_callback", "poll_enter": "map_registered",
            "poll_return": "poll_enter", "async_started": "map_registered", "async_waiting": "async_started",
            "async_signal": "async_waiting", "async_resumed": "async_signal", "summary": "map_registered",
        }
        if event in predecessors:
            require(predecessors[event] in active, "readback event precedes causal predecessor")
        if event == "pixels":
            require("map_registered" in active, "readback pixels precede map registration return")
            require(active["map_callback"]["result"] == "ok", "readback pixels lack successful map")
        if event == "poll_enter":
            require(5000 <= converted["elapsed_ms"] < 15000, "readback poll entry outside diagnostic window")
        if event == "summary":
            require(converted["elapsed_ms"] >= 15000 and converted["render_frames"] >= 1,
                    "readback summary precedes deadline/render observation")
        if event == "summary":
            observed = readback_observations(active)
            expected = {
                "submitted": "submitted" in active,
                "queue_callback": observed["queue_callback_observed"],
                "map_callback": observed["map_callback"], "pixels": observed["pixels"],
                **{name: name in active for name in ("async_started", "async_waiting", "async_signal", "async_resumed")},
                "poll": observed["poll"],
            }
            require(all(converted[key] == value for key, value in expected.items()), "readback summary contradicts events")
            summary = converted
        elif not converted.get("cancelled", False):
            active[event] = converted
        cleaning_up = cleaning_up or converted.get("cancelled", False)
        events.append(converted)
        seen.add(event)
    return {"schema_version": 1, "protocol": PROBE_PREFIX + "/v1", "events": events,
            "summary": summary, "observations": readback_observations(active)}


def readback_observations(events):
    poll = events.get("poll_return", {}).get("status", "entered_without_return" if "poll_enter" in events else "not_entered")
    pixels = ("valid" if events["pixels"]["valid"] else "invalid") if "pixels" in events else "missing"
    return {"gpu_completion_observed": poll in {"wait_succeeded", "queue_empty"} or pixels == "valid",
            "queue_callback_observed": "queue_callback" in events,
            "map_callback": events.get("map_callback", {}).get("result", "missing"),
            "pixels": pixels, "async_resumed": "async_resumed" in events, "poll": poll}


def readback_document(log_path):
    require(log_path.stat().st_size <= MAX_EVIDENCE_BYTES, "diagnostic log exceeds size bound")
    with log_path.open("rb") as stream:
        raw = stream.read(MAX_EVIDENCE_BYTES + 1)
    require(len(raw) <= MAX_EVIDENCE_BYTES, "diagnostic log exceeds size bound")
    require(b"\0" not in raw, "binary bytes in diagnostic log")
    return {**parse_readback_log(raw.decode("utf-8")), "log_sha256": hashlib.sha256(raw).hexdigest()}


def readback_json(path):
    require(path.stat().st_size <= PROBE_MAX_JSON_BYTES, "diagnostic JSON exceeds size bound")

    def unique_object(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "duplicate diagnostic JSON key")
            result[key] = value
        return result

    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique_object)


def record_missing_output(path, message):
    """An invocation can fail before subprocess captures either output pipe."""
    if not path.exists():
        path.write_text("[harness] No process output was captured; invocation failed: "
                        + message[:4096] + "\n", encoding="utf-8")


def check_default_capture(run, repo, app, unrelated, work, evidence, report, *, diagnose_readback=False):
    """One required attempt; an explicit opt-in allows one nonqualifying probe."""
    require(digest(app) == report["executable_sha256"], "executable changed before primary capture")
    screenshot = work / PNG_NAME
    command = capture_command(app, screenshot)
    result = None
    try:
        result, log = run(command, cwd=unrelated, timeout=CAPTURE_TIMEOUT_SECONDS, runtime=True,
                          output=evidence / "default-swift.log")
        validate_smoke(log, result.returncode, model=True)
        png = validate_png(screenshot)
        require(digest(app) == report["executable_sha256"], "executable changed during primary capture")
        require(not (evidence / PNG_NAME).exists(), "primary image destination already exists")
        screenshot.rename(evidence / PNG_NAME)
    except CAPTURE_ERRORS as primary:
        report["primary_capture_failure"] = {
            "kind": "timeout" if isinstance(primary, subprocess.TimeoutExpired) else "capture_failed",
            "message": sanitize(str(primary), repo, work), "timeout_seconds": CAPTURE_TIMEOUT_SECONDS,
            "command": [sanitize(arg, repo, work) for arg in command],
            "launch": BASELINE_LAUNCH, "rust_log": CAPTURE_TRACE,
        }
        if result is not None:
            report["primary_capture_failure"]["exit_code"] = result.returncode
        try:
            record_missing_output(evidence / "default-swift.log", report["primary_capture_failure"]["message"])
            if diagnose_readback:
                record_readback_probe(run, repo, app, unrelated, work, evidence, report)
            else:
                report["primary_capture_failure"]["log_sha256"] = digest(evidence / "default-swift.log")
        finally:
            # Even diagnostic preparation/parsing/I/O errors must never hide
            # the original failure, add an acceptance check, or launch again.
            raise primary from None
    report["checks"]["default_swift"] = {"status": "passed", "exit_code": result.returncode,
                                        "png": png, "log_sha256": digest(evidence / "default-swift.log")}


def record_readback_probe(run, repo, app, unrelated, work, evidence, report):
    probe_command = diagnostic_capture_command(app, work / PROBE_PNG_NAME)
    probe = {
        "status": "failed", "attempts": 0, "qualifies_acceptance": False,
        "timeout_seconds": CAPTURE_TIMEOUT_SECONDS,
        "command": [sanitize(arg, repo, work) for arg in probe_command],
        "working_directory": sanitize(str(unrelated), repo, work),
        "launch": BASELINE_LAUNCH, "rust_log": CAPTURE_TRACE,
    }
    report["diagnostics"] = {"readback_probe": probe}
    try:
        report["primary_capture_failure"]["log_sha256"] = digest(evidence / "default-swift.log")
        probe["executable_sha256"] = digest(app)
        require(probe["executable_sha256"] == report["executable_sha256"], "executable changed before diagnostic probe")
        probe["attempts"] = 1
        result, log = run(probe_command, cwd=unrelated, timeout=CAPTURE_TIMEOUT_SECONDS, runtime=True,
                          accepted=None, output=evidence / PROBE_LOG_NAME)
        probe["exit_code"] = result.returncode
        # Save the structured evidence before deciding whether an independently
        # successful screenshot may be exposed. No JSON is trusted from the app.
        document = readback_document(evidence / PROBE_LOG_NAME)
        write_json(evidence / PROBE_JSON_NAME, document)
        validate_smoke(log, result.returncode, model=True)
        png = validate_png(work / PROBE_PNG_NAME)
        probe["executable_sha256_after"] = digest(app)
        require(probe["executable_sha256_after"] == probe["executable_sha256"], "executable changed during diagnostic probe")
        require(not (evidence / PROBE_PNG_NAME).exists(), "diagnostic image destination already exists")
        (work / PROBE_PNG_NAME).rename(evidence / PROBE_PNG_NAME)
        probe.update(status="captured", png=png)
    except Exception as error:
        probe["status"] = "timed_out" if isinstance(error, subprocess.TimeoutExpired) else "failed"
        probe["failure"] = sanitize(str(error), repo, work)
        if probe["attempts"] == 1:
            record_missing_output(evidence / PROBE_LOG_NAME, probe["failure"])
    finally:
        try:
            if (evidence / PROBE_LOG_NAME).is_file():
                probe["log_sha256"] = digest(evidence / PROBE_LOG_NAME)
                document = readback_document(evidence / PROBE_LOG_NAME)
                write_json(evidence / PROBE_JSON_NAME, document)
                probe["json_sha256"] = digest(evidence / PROBE_JSON_NAME)
        except Exception as error:
            probe["evidence_failure"] = sanitize(str(error), repo, work)


def validate_probe_evidence(directory, report):
    diagnostics = report.get("diagnostics")
    has_probe = any((directory / name).exists() for name in (PROBE_LOG_NAME, PROBE_PNG_NAME, PROBE_JSON_NAME))
    if diagnostics is None:
        require(not has_probe, "diagnostic files lack a primary failure/probe record")
        if "primary_capture_failure" not in report:
            return
    else:
        require(isinstance(diagnostics, dict) and set(diagnostics) == {"readback_probe"}, "unexpected diagnostic record")
    require(isinstance(report.get("executable_sha256"), str)
            and re.fullmatch(r"[0-9a-f]{64}", report["executable_sha256"]), "diagnostic binary digest is missing")
    primary = report.get("primary_capture_failure", {})
    primary_fields = {"kind", "message", "timeout_seconds", "command", "launch", "rust_log", "log_sha256"}
    require(isinstance(primary, dict) and report["status"] == "failed"
            and primary_fields <= set(primary) <= primary_fields | {"exit_code"}
            and primary.get("kind") in {"timeout", "capture_failed"}
            and type(primary.get("timeout_seconds")) is int and primary["timeout_seconds"] == CAPTURE_TIMEOUT_SECONDS
            and report.get("failure") == primary.get("message")
            and not report["checks"] and not (directory / PNG_NAME).exists(),
            "diagnostic probe cannot replace or pass primary failure")
    expected_exe = "<private-work>/extracted/swift-candidate/flightsim-app.exe"
    require(primary.get("command") == capture_command(expected_exe, "<private-work>/" + PNG_NAME)
            and primary.get("launch") == BASELINE_LAUNCH
            and type(primary["launch"]["creationflags"]) is int
            and primary.get("rust_log") == report.get("runtime_capture_rust_log") == CAPTURE_TRACE
            and ("exit_code" not in primary or type(primary["exit_code"]) is int),
            "primary launch changed")
    require(digest(directory / "default-swift.log") == primary.get("log_sha256"), "primary failure log changed")
    if diagnostics is None:
        return
    probe = diagnostics["readback_probe"]
    require(isinstance(probe, dict) and type(probe.get("attempts")) is int and probe["attempts"] == 1
            and probe.get("qualifies_acceptance") is False
            and type(probe.get("timeout_seconds")) is int and probe["timeout_seconds"] == CAPTURE_TIMEOUT_SECONDS
            and probe.get("launch") == BASELINE_LAUNCH
            and type(probe["launch"]["creationflags"]) is int
            and probe.get("working_directory") == "<private-work>/unrelated-cwd"
            and probe.get("command") == diagnostic_capture_command(expected_exe, "<private-work>/" + PROBE_PNG_NAME)
            and probe.get("rust_log") == CAPTURE_TRACE
            and probe.get("executable_sha256") == report.get("executable_sha256"), "diagnostic probe identity/launch differs")
    require("evidence_failure" not in probe, "malformed diagnostic evidence")
    probe_fields = {"status", "attempts", "qualifies_acceptance", "timeout_seconds", "command", "working_directory",
                    "launch", "rust_log", "executable_sha256", "log_sha256", "json_sha256"}
    extra = {"exit_code", "png", "executable_sha256_after"} if probe.get("status") == "captured" else {"exit_code", "failure"}
    require(probe_fields <= set(probe) <= probe_fields | extra, "unexpected diagnostic proof fields")
    require((directory / PROBE_LOG_NAME).is_file() and (directory / PROBE_JSON_NAME).is_file(),
            "diagnostic log/JSON pair is incomplete")
    require(digest(directory / PROBE_LOG_NAME) == probe.get("log_sha256"), "diagnostic log changed")
    require(digest(directory / PROBE_JSON_NAME) == probe.get("json_sha256"), "diagnostic JSON changed")
    require(json.dumps(readback_json(directory / PROBE_JSON_NAME), sort_keys=True)
            == json.dumps(readback_document(directory / PROBE_LOG_NAME), sort_keys=True),
            "diagnostic JSON differs from log projection")
    if probe.get("status") == "captured":
        require(type(probe.get("exit_code")) is int and probe["exit_code"] == 0
                and probe.get("executable_sha256_after") == probe["executable_sha256"],
                "diagnostic probe lacks exit/binary proof")
        require(validate_png(directory / PROBE_PNG_NAME) == probe.get("png"), "diagnostic PNG does not match proof")
        validate_smoke((directory / PROBE_LOG_NAME).read_text(encoding="utf-8"), 0, model=True)
    else:
        require(probe.get("status") in ("failed", "timed_out") and isinstance(probe.get("failure"), str)
                and probe["failure"] and not (directory / PROBE_PNG_NAME).exists(),
                "unproven diagnostic image cannot be uploaded")
        if probe["status"] == "timed_out":
            require("exit_code" not in probe, "timed-out diagnostic cannot claim an exit code")


def validate_source_evidence(source, report):
    def hex_value(value, length):
        return isinstance(value, str) and re.fullmatch(r"[0-9a-f]{" + str(length) + "}", value)

    require(isinstance(source, dict) and source.get("schema_version") == 3
            and hex_value(report.get("source_sha"), 40) and source.get("source_sha") == report["source_sha"],
            "invalid reviewed source identity")
    object_length = {"sha1": 40, "sha256": 64}.get(source.get("canonical_git_object_format"))
    require(object_length and hex_value(source.get("source_tree"), object_length), "invalid reviewed source tree")
    contract = validate_replay_contract(source.get("replay_contract"))
    contract_text = source.get("replay_contract_text")
    contract_hash = source.get("replay_contract_sha256")
    require(isinstance(contract_text, str) and hex_value(contract_hash, 64)
            and hashlib.sha256(contract_text.encode("utf-8")).hexdigest() == contract_hash
            and json.loads(contract_text) == contract
            and report.get("replay_contract") == REPLAY_CONTRACT_ID
            and report.get("replay_contract_sha256") == contract_hash,
            "invalid reviewed contract byte binding")
    require(source.get("legacy_baseline") == LEGACY_BASELINE
            and source.get("legacy_source_sha256") == LEGACY_SOURCE_HASHES
            and source.get("independent_replay_sha256") == INDEPENDENT_REPLAY_HASHES,
            "frozen replay evidence anchors changed")
    files = source.get("files")
    require(isinstance(files, list) and files, "missing tracked source inputs")
    by_path = {}
    for record in files:
        require(isinstance(record, dict) and isinstance(record.get("path"), str)
                and record["path"] not in by_path and record.get("git_mode") in ("100644", "100755")
                and hex_value(record.get("canonical_git_blob"), object_length)
                and type(record.get("checkout_bytes")) is int and record["checkout_bytes"] >= 0
                and hex_value(record.get("checkout_sha256"), 64), "invalid tracked source input")
        by_path[record["path"]] = record
    validate_modified_source_boundaries(by_path)
    require(by_path.get(REPLAY_CONTRACT_PATH, {}).get("checkout_sha256") == contract_hash
            and by_path[REPLAY_CONTRACT_PATH]["checkout_bytes"] == len(contract_text.encode("utf-8")),
            "contract missing from tracked source inputs")
    pins = {**contract["source_sha256"], **INDEPENDENT_REPLAY_HASHES}
    reviewed = source.get("reviewed_replay_source_evidence")
    require(isinstance(reviewed, dict) and set(reviewed) == set(pins), "incomplete reviewed source evidence")
    for path, expected_hash in pins.items():
        record = by_path.get(path)
        require(record is not None and record["checkout_sha256"] == expected_hash,
                "reviewed source missing or differs: " + path)
        require(reviewed[path] == {
            "canonical_sha256": expected_hash, "canonical_bytes": record["checkout_bytes"],
            "canonical_git_blob": record["canonical_git_blob"],
            "checkout_sha256": expected_hash, "checkout_bytes": record["checkout_bytes"],
        }, "reviewed source byte evidence differs: " + path)


def validate_evidence(directory):
    require(directory.is_dir() and not directory.is_symlink(), "missing evidence directory")
    for path in directory.iterdir():
        require(path.is_file() and not path.is_symlink(), "evidence must contain only regular files")
        require(path.name in TEXT_EVIDENCE | PNG_EVIDENCE, f"unapproved evidence path: {path.name}")
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
    if "default_rejects_legacy" in checks:
        validate_legacy_rejection((directory / "default-rejects-legacy.log").read_text(encoding="utf-8"),
                                  checks["default_rejects_legacy"]["exit_code"])
    if "legacy_no_model" in checks:
        proof = checks["legacy_no_model"]
        require(proof.get("identity_evidence") == "legacy_partial" and proof.get("legacy_opt_in") is True
                and proof.get("historical_yaw_verified") is False and proof.get("notice") == LEGACY_NOTICE
                and proof.get("fingerprint") == LEGACY_FINGERPRINT,
                "legacy smoke cannot claim complete or historical yaw identity")
        require(LEGACY_LIMIT in report.get("limits", []), "missing legacy partial-identity limitation")
        legacy_log = directory / "legacy-no-model.log"
        require(digest(legacy_log) == proof.get("log_sha256"), "legacy log changed")
        validate_legacy_smoke(legacy_log.read_text(encoding="utf-8"), proof["exit_code"])
        require(report.get("replay_tests") == {
            name: {"status": "passed", "test": test} for name, test in REPLAY_ACCEPTANCE_TESTS.items()
        }, "legacy smoke lacks persistent notice/policy test evidence")
    if report.get("status") == "engineering_checks_passed":
        require(set(checks) == set(expected_checks), "successful report lacks required checks")
        require((directory / PNG_NAME).exists() and all((directory / name).is_file() for name in REQUIRED_TEXT_EVIDENCE),
                "successful report lacks required evidence")
        require(report.get("legacy_replay", {}).get("fingerprint") == LEGACY_FINGERPRINT,
                "successful report lacks frozen legacy fingerprint")
        source = json.loads((directory / "source-inputs.json").read_text(encoding="utf-8"))
        require(report.get("source_inputs_sha256") == digest(directory / "source-inputs.json"),
                "successful report lacks reviewed replay source binding")
        validate_source_evidence(source, report)
        validate_distribution(report.get("distribution"), report.get("distribution"))
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
    validate_probe_evidence(directory, report)


def stop_candidate_process_tree(process):
    """Bound cleanup independently; never drain a descendant-owned output pipe."""
    failures = []
    try:
        if sys.platform == "win32":
            taskkill = Path(os.environ["SystemRoot"]) / "System32/taskkill.exe"
            killer = subprocess.Popen([str(taskkill), "/PID", str(process.pid), "/T", "/F"],
                                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            try:
                code = killer.wait(timeout=PROCESS_CLEANUP_TIMEOUT_SECONDS)
                if code != 0:
                    failures.append("process-tree termination returned a failure")
            except subprocess.TimeoutExpired:
                killer.kill()
                killer.wait(timeout=PROCESS_CLEANUP_TIMEOUT_SECONDS)
                failures.append("process-tree termination exceeded its cleanup deadline")
        else:
            # Real subprocess regression tests run on Linux; production rejects it.
            os.killpg(process.pid, signal.SIGKILL)
    except (OSError, KeyError, subprocess.SubprocessError) as error:
        failures.append("process-tree termination failed: " + str(error))
    try:
        if process.poll() is None:
            process.kill()
        process.wait(timeout=PROCESS_CLEANUP_TIMEOUT_SECONDS)
    except (OSError, subprocess.SubprocessError) as error:
        failures.append("root-process cleanup failed: " + str(error))
    return "; ".join(failures)


def run_bounded(command, *, cwd, env, timeout, private):
    """Use private files so inherited handles cannot defeat the process deadline.

    Do not use Popen's context manager: its exit waits without a deadline. Windows
    keeps the ordinary launch flags; only Linux regression tests use a new group.
    """
    command = [str(arg) for arg in command]
    # Retain raw streams only in the private workspace. Separate read handles
    # below must not seek a file offset shared with a surviving descendant.
    with tempfile.NamedTemporaryFile(mode="wb", dir=private, prefix="command-", suffix="-stdout.log",
                                     delete=False) as out, \
            tempfile.NamedTemporaryFile(mode="wb", dir=private, prefix="command-", suffix="-stderr.log",
                                        delete=False) as err:
        options = {} if sys.platform == "win32" else {"start_new_session": True}
        process = subprocess.Popen(command, cwd=cwd, env=env, stdout=out, stderr=err, **options)
        timed_out = None
        cleanup_failure = ""
        try:
            process.wait(timeout=timeout)
        except subprocess.TimeoutExpired as error:
            timed_out = error
            cleanup_failure = stop_candidate_process_tree(process)
        streams = []
        oversized = False
        for stream in (out, err):
            # Snapshot regular-file length rather than following a surviving
            # writer to EOF. Export already imposes this same per-file bound.
            size = os.fstat(stream.fileno()).st_size
            oversized |= size > MAX_EVIDENCE_BYTES
            with open(stream.name, "rb") as reader:
                streams.append(reader.read(min(size, MAX_EVIDENCE_BYTES)))
        if timed_out is not None:
            timed_out.stdout, timed_out.stderr = streams
            if oversized:
                timed_out.stderr += b"\n[harness] Timed-out output exceeded the evidence size limit; output truncated.\n"
            if cleanup_failure:
                timed_out.stderr += ("\n[harness] " + cleanup_failure + "\n").encode("utf-8", errors="replace")
            raise timed_out
        require(not oversized, "command output exceeds evidence size limit")
        return subprocess.CompletedProcess(command, process.returncode, *streams)



def command_log(stdout, stderr, repo, work):
    """Bound the final UTF-8 representation, retaining head and cleanup tail."""
    log = sanitize((stdout + b"\n" + stderr).decode("utf-8", errors="replace"), repo, work)
    encoded = log.encode("utf-8")
    if len(encoded) <= MAX_EVIDENCE_BYTES:
        return log, False
    marker = b"\n[harness] Output exceeded the evidence size limit; middle truncated.\n"
    tail = min(8192, (MAX_EVIDENCE_BYTES - len(marker)) // 2)
    head = MAX_EVIDENCE_BYTES - len(marker) - tail
    log = (encoded[:head].decode("utf-8", errors="ignore") + marker.decode("ascii")
           + encoded[-tail:].decode("utf-8", errors="ignore"))
    return log, True


def run_candidate(repo, expected, work, evidence, *, diagnose_readback=False):
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
                  "Bundled AgX/Filmic LUTs remain enabled; Filmic has a scoped source-notice decision, while AgX rights and whole-target dependency review remain unresolved",
                  "Software D3D12 fallback is not physical GPU/controller/audio or Steam qualification",
                  LEGACY_LIMIT],
              "commands": candidate_commands(), "runtime_capture_rust_log": CAPTURE_TRACE}
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
            if "--screenshot" in command:
                run_env["RUST_LOG"] = CAPTURE_TRACE
        timed_out = None
        try:
            result = run_bounded(command, cwd=cwd, env=run_env, timeout=timeout, private=work)
        except subprocess.TimeoutExpired as error:
            # Tree cleanup and output collection have separate finite bounds.
            # Keep the diagnostic bytes; partial screenshots stay private.
            timed_out = error
            result = subprocess.CompletedProcess(command, -1, error.stdout or b"", error.stderr or b"")
        # Cargo output is text; raw metadata goes only into the private workspace.
        log, truncated = command_log(result.stdout, result.stderr, repo, work)
        if output:
            output.write_bytes(log.encode("utf-8"))
        with (evidence / "commands.log").open("a", encoding="utf-8", newline="\n") as stream:
            stream.write(json.dumps([str(x).replace(str(work), "<private-work>").replace(str(repo), "<source>") for x in command]) + "\n")
            stream.write(f"exit_code={result.returncode}\n")
            if output:
                stream.write(f"log={output.name} sha256={digest(output)}\n")
            elif "metadata" not in command:
                stream.write(log.replace(str(work), "<private-work>").replace(str(repo), "<source>") + "\n")
        if timed_out is not None:
            raise timed_out
        require(not truncated, "command output exceeds evidence size limit")
        require(accepted is None or result.returncode in accepted, f"command failed with {result.returncode}: {command[0]}")
        return result, log

    try:
        source = source_inputs(repo, expected)
        write_json(evidence / "source-inputs.json", source)
        report["source_inputs_sha256"] = digest(evidence / "source-inputs.json")
        report["replay_contract"] = REPLAY_CONTRACT_ID
        report["replay_contract_sha256"] = source["replay_contract_sha256"]
        for reference in ("replay_identity_reference.py", "replay_v3_reference.py"):
            run([sys.executable, repo / "docs/qa" / reference])
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
        validate_distribution(info, json.loads((bundle / "distribution-info.json").read_text(encoding="utf-8")))
        report["distribution"] = info
        check_default_capture(run, repo, app, unrelated, work, evidence, report,
                              diagnose_readback=diagnose_readback)
        result, log = run([app, "--aircraft", "light-single"], cwd=unrelated, timeout=30, accepted=(2,),
                          runtime=True, output=evidence / "absent-light-single.log")
        require("selected aircraft model is missing: aircraft/light_single.glb" in log, "wrong absent-model failure")
        require("aircraft model fitted:" not in log, "absent model reached graphics")
        report["checks"]["absent_light_single"] = {"status": "passed", "exit_code": result.returncode}
        _, log = run(report["commands"]["identity_test"])
        require("1 passed; 0 failed" in log, "legacy identity test did not actually execute")
        report["replay_tests"] = {}
        for name in REPLAY_ACCEPTANCE_TESTS:
            _, log = run(report["commands"][name])
            require("1 passed; 0 failed" in log, name + " did not actually execute")
            report["replay_tests"][name] = {"status": "passed", "test": REPLAY_ACCEPTANCE_TESTS[name]}
        fixture = work / "legacy.fsreplay"
        run(["cargo", "+" + TOOLCHAIN, "run", "--locked", "--release", "-j", "2", "--target", TARGET,
             "-p", "flightsim-sim", "--example", "record_takeoff", "--", fixture])
        report["legacy_replay"] = legacy_identity(fixture)
        result, log = run([app, "--replay", fixture], cwd=unrelated, timeout=30, accepted=(2,), runtime=True,
                          output=evidence / "default-rejects-legacy.log")
        validate_legacy_rejection(log, result.returncode)
        report["checks"]["default_rejects_legacy"] = {"status": "passed", "exit_code": result.returncode}
        result, log = run(legacy_capture_command(app, fixture, work / "legacy-no-model.png"),
                          cwd=unrelated, timeout=180, runtime=True, output=evidence / "legacy-no-model.log")
        validate_legacy_smoke(log, result.returncode)
        validate_png(work / "legacy-no-model.png")
        report["checks"]["legacy_no_model"] = {"status": "passed", "exit_code": result.returncode,
                                                "fingerprint": report["legacy_replay"]["fingerprint"],
                                                "identity_evidence": "legacy_partial", "legacy_opt_in": True,
                                                "historical_yaw_verified": False, "notice": LEGACY_NOTICE,
                                                "log_sha256": digest(evidence / "legacy-no-model.log")}
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
    parser.add_argument("--diagnose-readback", action="store_true",
                        help="after primary capture failure, run one nonqualifying readback probe (default: disabled)")
    args = parser.parse_args(argv)
    try:
        if args.validate_evidence:
            validate_evidence(args.validate_evidence)
        else:
            require(args.expected_source and args.work and args.evidence, "expected-source, work and evidence are required")
            run_candidate(args.repo.resolve(), args.expected_source, args.work.resolve(), args.evidence.resolve(),
                          diagnose_readback=args.diagnose_readback)
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError, zlib.error) as error:
        print(f"Swift Windows candidate check failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
