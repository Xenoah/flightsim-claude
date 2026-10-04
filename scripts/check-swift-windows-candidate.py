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
REPLAY_CONTRACT_ID = "swift-candidate-replay-v3-v1"
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


def validate_replay_contract(contract):
    require(isinstance(contract, dict) and contract.get("schema_version") == 1
            and contract.get("contract") == REPLAY_CONTRACT_ID, "invalid reviewed replay contract")
    require(isinstance(contract.get("reviewed_source"), str)
            and re.fullmatch(r"[0-9a-f]{40}", contract["reviewed_source"]), "missing reviewed replay source")
    pins = contract.get("source_sha256")
    require(isinstance(pins, dict) and set(pins) == REPLAY_CONTRACT_PATHS,
            "reviewed replay source boundary changed")
    require(all(isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value) for value in pins.values()),
            "invalid reviewed replay source digest")
    # These physical inputs never migrated. The FDM module root only gained
    # additive exports and retains its own strict reviewed whole-file guard.
    for path in ("assets/aircraft/light_single.json", "crates/flightsim-fdm/src/aircraft.rs"):
        require(pins[path] == LEGACY_SOURCE_HASHES[path], "frozen legacy input changed: " + path)
    require(pins["crates/flightsim-fdm/src/lib.rs"] == REVIEWED_ADDITIVE_FDM_LIB_SHA256,
            "frozen reviewed FDM module input changed")
    return contract


def load_replay_contract(repo):
    return validate_replay_contract(json.loads((repo / REPLAY_CONTRACT_PATH).read_text(encoding="utf-8")))


def source_inputs(repo, expected):
    require(re.fullmatch(r"[0-9a-f]{40}", expected), "expected source must be a full lowercase SHA")
    require(git(repo, "rev-parse", "HEAD") == expected, "checkout is not the expected source")
    require(not git(repo, "status", "--porcelain", "--untracked-files=all"), "source checkout must be clean")
    tree = subprocess.check_output(["git", "ls-tree", "-r", "-z", "HEAD"], cwd=repo).decode("utf-8")
    records = []
    for entry in sorted(p for p in tree.split("\0") if p):
        metadata, relative = entry.split("\t", 1)
        mode, kind, object_id = metadata.split()
        require(kind == "blob" and mode in ("100644", "100755"), f"non-regular canonical input: {relative}")
        path = repo / relative
        require(path.is_file() and not path.is_symlink(), f"non-regular source input: {relative}")
        records.append({"path": relative, "canonical_git_blob": object_id, "git_mode": mode,
                        "checkout_bytes": path.stat().st_size, "checkout_sha256": digest(path)})
    records.sort(key=lambda record: record["path"])
    by_path = {record["path"]: record for record in records}
    require(REPLAY_CONTRACT_PATH in by_path, "reviewed replay contract must be tracked")
    contract_record = by_path[REPLAY_CONTRACT_PATH]
    contract_blob = subprocess.check_output(
        ["git", "cat-file", "blob", contract_record["canonical_git_blob"]], cwd=repo)
    require(hashlib.sha256(contract_blob).hexdigest() == contract_record["checkout_sha256"],
            "reviewed replay contract checkout differs from canonical Git blob")
    contract = load_replay_contract(repo)
    reviewed_sources = {}
    for relative, expected_hash in {**contract["source_sha256"], **INDEPENDENT_REPLAY_HASHES}.items():
        require(relative in by_path, "missing reviewed replay source: " + relative)
        record = by_path[relative]
        blob = subprocess.check_output(["git", "cat-file", "blob", record["canonical_git_blob"]], cwd=repo)
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
                  "Bundled AgX/Filmic LUTs remain enabled and retain unresolved review records",
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
            if output:
                stream.write(f"log={output.name} sha256={digest(output)}\n")
            elif "metadata" not in command:
                stream.write(log.replace(str(work), "<private-work>").replace(str(repo), "<source>") + "\n")
        if timed_out is not None:
            raise timed_out
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
