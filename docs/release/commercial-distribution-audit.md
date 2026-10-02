# Commercial distribution readiness audit

Reviewed 2026-10-02 UTC. Source inventory began at
`08717b09075e75bda48b61eea87cc7e2e046eea0`; asset hashes, rather than a moving branch
name, identify the reviewed content. This is an engineering evidence record,
not a legal opinion, certification of non-infringement, or Steam approval.
No Steam account, agreement, payment, upload, or publication was performed.

## Decision

**Do not treat the existing development ZIP as commercially cleared.** A
Swift-only, offline staging variant is the lower-risk starting point, but its
dependency/embedded-asset review must still be completed. Ordinary development
builds remain available. The commercial checks are opt-in and fail closed.

The existing release workflow already preserves the project licenses,
ATTRIBUTION, global-data notices, the GLO-90 license and provenance documents.
It recursively copies `assets/`, including the legacy Meshy model. It does not
collect the complete target-specific dependency/font notice set. These new tools
do not silently change that workflow or authorize another release.

## Bundled-content inventory

Exact SHA-256 values, byte counts, source paths and review states are in
[asset-rights-manifest.json](asset-rights-manifest.json). A state such as
`licensed_with_notices` records the evidence found; it is not an all-rights
clearance or proof of the publisher's chain of title.

| Content | Actual delivery | Evidence and remaining condition |
|---|---|---|
| `light_single.glb`, 4,752,424 bytes | External model in current development ZIP | GLB says `meshy-scene`, with one embedded JPEG. Legacy attribution says paid-plan generation on 2026-08-21. No task ID, applicable plan/terms, or input-rights evidence was found. Commercial staging excludes it |
| `swift_sport.glb`, 229,192 bytes; profile JSON | External original procedural aircraft | GLB says Blender glTF exporter; no embedded image textures. Reproducible local primitive/material source is `tools/blender/build_swift_sport.py`. Project declares MIT OR Apache-2.0. Retain provenance; this is a generic model, not a licensed real-aircraft brand |
| `swift_sport.blend`, 160,584 bytes | Editable source in development ZIP | Original scene record exists. Not needed in the runtime commercial candidate. Blender output and Blender software have distinct licensing; see tool/source distribution note below |
| Global terrain, 8,912,960 bytes | Embedded in executable via `include_bytes!` | Mixed NOAA ETOPO, NGA-derived geoid, Natural Earth and modified Copernicus GLO-90. Preserve the GLO-90 terms/notices, not just a public-domain label |
| Monthly climate, 1,180,456 bytes | Embedded in executable via `include_bytes!` | NOAA PSL NCEP/NCAR 1991–2020 monthly means, with source hashes and transform record. Attribute PSL and identify derived climatology accurately |
| Fira Mono subset, 18,848 bytes | Embedded by Bevy `default_font` | Font metadata identifies Mozilla/Telefonica, OFL 1.1 or later. Exact Bevy v0.18.1 OFL text is now archived at `licenses/FiraMono-LICENSE` |
| Tony McMapface, AgX and Blender Filmic LUTs | All three embedded by enabled Bevy `tonemapping_luts` | Tony's pinned upstream MIT notice is archived. AgX's specific upstream grant and Filmic's exact source/version/license record remain open |
| Controller mapping database | Included by `gilrs` build machinery | Its nested SDL GameControllerDB notice is collected separately from GilRs code licensing. A nested notice must not substitute for the crate's own license |
| Engine/wind/stall audio | Synthesized by application code | No third-party recording files in tracked runtime assets. This is not a comprehensive patent, source-authorship or acoustic-content clearance |
| 400-byte `airport-valid.fsairports` | Test-only synthetic fixture | Not a real OSM regional database, and not a commercial runtime pack |

No real OSM PBF/FSAP, regional Copernicus GLO-30 tile set, ESA WorldCover,
Sentinel-2 imagery, or satellite-photography texture pack is bundled by the
reviewed source. ATTRIBUTION's planned-source section does not mean those assets
have been procured or cleared. Bevy's NVIDIA blue-noise texture feature is absent
from the reviewed Linux/Windows feature graphs; its crate-source presence alone
does not show runtime inclusion.

## Release blockers and how to close them

### 1. Legacy Meshy model: exclude or establish its actual grant

The current [Meshy terms](https://www.meshy.ai/terms-of-use), last updated
2026-09-19, distinguish paid-plan output, free-plan CC BY 4.0 output, and public
Community output. [Meshy's commercial-use guide](https://help.meshy.ai/en/articles/16102098-can-i-use-meshy-assets-commercially)
also ties rights to the plan at generation and the rights in supplied references.
These current pages postdate the recorded 2026-08-21 generation. They are useful
guidance, not proof of this file's historical grant.

To include this exact model, retain a private, redacted record of generation
task/date, generating account/publisher authority, plan at that date, applicable
terms or a specific grant, Community publication status, prompts/reference
sources and rights, and the downloaded file's SHA-256. Do not put credentials or
payment details in the repository. Alternatively keep it excluded and use the
original Swift asset. **Adding a Meshy credit does not by itself establish a
CC BY license for an unverified artifact.** Do not relabel it automatically.

### 2. Complete dependency evidence and review the final target

The first exact-feature collection produced:

| Target | Conservative app normal/build closure | Copied dependency notice files | Embedded supplemental notices | Open records |
|---|---:|---:|---:|---:|
| `x86_64-unknown-linux-gnu` | 340 packages | 665 | 2 | 4 |
| `x86_64-pc-windows-msvc` | 347 packages | 682 | 2 | 4 |
| `x86_64-pc-windows-gnu` (separate later capture) | 347 packages | 682 | 2 | 4 |

Twenty-five version-pinned upstream notice files repair missing notices in the
published crate archives. [dependency-notice-supplements.json](dependency-notice-supplements.json)
records source revisions, official source URLs and file hashes. GitHub-sourced
bytes were checked against the upstream Git blob hashes; the GilRs MIT text was
retrieved from its exact GitLab source revision. No copyright names or license
texts were invented. Original line endings are retained.

Four records remain unresolved in both inventories:

- `constgebra 0.1.4`: Cargo declares MIT OR Apache-2.0, but the exact published
  source and pinned repository lack a primary license text. Have a reviewer
  resolve the declared grant and redistribution notice requirements
- `hexf-parse 0.2.1`: Cargo declares CC0-1.0, but no primary license text was found
  in the exact source/pinned repository. Record the declaration and applicable
  CC0 evidence; do not mistake missing text for a finding of infringement
- Bevy AgX LUT: the pinned MrLixm/AgXc source/README does not supply a specific
  grant in the inspected record. Obtain applicable upstream evidence or replace
  this embedding through a separately tested engineering change
- Bevy Blender Filmic LUT: Bevy's recipe describes rendering through Blender
  Filmic but does not pin the source version or its asset terms. Establish that
  record or replace the embedding. A render recipe alone is not a license review

The collector preserves SPDX expressions exactly. `AND` is not an alternative;
`OR` is not a ban merely because one offered option is copyleft. For example,
`self_cell` offers Apache-2.0 OR GPL-2.0-only, and `unicode-ident` adds Unicode-3.0
obligations. The reviewer must select and document compatible alternatives,
retain applicable notices, and inspect nested/source-header exceptions.

Cargo metadata is a conservative dependency graph, **not a linked-binary SBOM**.
Build scripts/proc macros can overcount; native libraries, Rust standard/runtime
code, redistributable DLLs, fonts, shaders and generated databases need separate
coverage. Review imported DLLs/ELF dependencies from the actual shipped binary.
Do not package this machine's Linux libraries, a GPU driver, or the Rust toolchain
as an assumed redistributable. Windows GNU and Windows MSVC need separate
inventories and runtime reviews. The separate GNU source inventory does not
establish MinGW/Rust runtime or DLL redistribution clearance. Matching package
counts do not make GNU and MSVC inventories interchangeable. Rerun collection
after feature, lockfile or target changes.

### 3. Preserve data obligations in every depot/archive

- NOAA ETOPO's [official metadata](https://www.ncei.noaa.gov/access/metadata/landing-page/bin/iso?id=gov.noaa.ngdc.mgg.dem:etopo_2022)
  identifies CC0-1.0. Keep its citation and non-navigation/derived-product warning
- [Natural Earth](https://www.naturalearthdata.com/about/terms-of-use/) permits
  commercial reuse of its public-domain data; its credit is voluntary
- [NOAA PSL](https://www.psl.noaa.gov/data/help/) identifies federal data as public
  domain unless annotated otherwise, requests acknowledgment and prohibits
  misleading official-product/endorsement presentations
- The applicable [Copernicus GLO-90 terms](https://dataspace.copernicus.eu/sites/default/files/media/files/2025-06/copernicus_contributing_mission_data_access_v2_cop_dem_licenses.pdf)
  are pages 19–21, not the earlier restricted-product sections. Articles 4–6
  permit reproduction/distribution/adaptation with source/modification notices,
  liability disclaimer, non-endorsement and downstream obligations. Keep the
  archived three-page license and exact `NOTICE-GLOBAL-TERRAIN.txt` in the depot
- Coarse terrain and modeled monthly weather cues must not be sold as surveyed
  airport geography, certified navigation, live weather, or official NOAA output

### 4. Keep OSM-derived databases separately managed

[ODbL 1.0](https://opendatacommons.org/licenses/odbl/1-0/) permits commercial use;
computer programs are excluded from its database-license scope. This does not
remove obligations on a derived database. Sections 4.2, 4.4, 4.6 and 4.7 cover
notices, share-alike, machine-readable database/alterations availability, and
restricting measures. Simply acknowledging OSM is not always sufficient.

The current app credits actual OSM-loaded data, and its map credits include the
copyright URL. [OSMF's game guidance](https://osmfoundation.org/wiki/Licence/Attribution_Guidelines#Computer_games_and_simulations)
allows practical in-game/menu/credit placement with more information available.
Before publishing any real airport/world pack, record provider, snapshot,
source/output hashes, conversion version, license and machine-readable offer.
Keep the database separable from code and from unrelated terrain sources; verify
any required unrestricted copy if the depot/DRM restricts database rights.
See the companion [OSM airport-pack guide](../data/osm-airport-packs.md).

The staging checker currently rejects `.osm`, `.osm.pbf`, `.fsairports` and
regional `.fsdem` anywhere in the candidate. This is a conservative project
release policy, not a claim that these formats cannot be commercially sold.
Changing it requires a reviewed data-pack manifest and distribution decision.

### 5. Publisher and platform review remains separate

Steam's [Content Survey](https://partner.steamgames.com/doc/gettingstarted/contentsurvey)
requires appropriate disclosure of AI-assisted content shipped to and consumed
by players. The publisher should assess the whole final asset set, including
AI-assisted procedural artwork, rather than only the legacy Meshy model.
Ordinary coding-efficiency assistance is distinguished from player-consumed
content by the current guidance. This simulator's offline assets are not a live
generative-AI service. Do not submit a survey automatically or promise Steam
acceptance based on a technical smoke test.

Also review the commercial product name, aircraft/livery markings, store artwork,
trailer/audio rights, contributor authority and any implied third-party
endorsement. The development name `flightsim-claude` is not evidence of rights
to use another company's marks as a commercial product brand. This audit did
not perform trademark clearance or inspect a final store-page package.

Blender's [current FAQ](https://www.blender.org/support/faq/) distinguishes
artistic `.blend`/exported output from the Blender program and Python API
scripts. Swift output does not become GPL merely because Blender wrote it.
The repository's MIT/Apache statement for the generator script is not a review
of a distributed Blender/script combination. Before shipping the generator or
Blender itself, resolve the API-script/GPL conditions separately. The runtime
commercial candidate ships neither.

## Reproducible evidence workflow

Use the exact features and target used by the candidate build. These commands
only capture metadata and copy notices; they do not build, upload, register or
publish a product.

```sh
cargo metadata --locked --format-version 1 \
  --filter-platform x86_64-pc-windows-msvc \
  --features flightsim-app/commercial-staging > metadata-windows.json
python scripts/collect-dependency-notices.py \
  --metadata metadata-windows.json --target x86_64-pc-windows-msvc \
  --output /path/to/new-empty-notice-directory
python scripts/check-commercial-readiness.py \
  --bundle /path/to/staged-candidate --profile commercial \
  --dependency-inventory /path/to/staged-candidate/third-party/dependency-inventory.json \
  --json --report /path/to/staged-candidate/commercial-readiness.json
python -m unittest discover -s scripts/tests -p test_commercial_readiness.py
```

The collector always writes `review_status: not_reviewed`, preserves exact text
bytes and hashes, and records unresolved items. It uses no SPDX allowlist and
does not synthesize a legal approval. Never reuse a nonempty output directory.

The checker accepts a separate `--dependency-review FILE` only after actual
review. That JSON must carry `schema_version: 1`, `status: reviewed`,
`inventory_sha256`, `reviewed_by`, `reviewed_at`, and an explicit `scope` covering
the intended target/runtime. Every resolved open item requires its `id`, a
reason, and `evidence` records with packaged relative `path`, `sha256` and
authoritative `source`. A boolean or SPDX label alone cannot close a missing
grant. Keep such a reviewer record with the release evidence; do not fabricate
an approver. The checker validates binding/integrity, not the reviewer's legal
judgment or authority.

Checker exits: `0` mechanical checks passed, `2` known blockers, `1` malformed
input or an execution error. Reports separate integrity failures from outstanding
review. A review-blocked local candidate is useful for inspection, but it must
remain labeled blocked. The checker verifies manifest/source and external asset
hashes, notice identity, feature/OS/architecture/environment alignment, and
packaged evidence; it does not prove a binary was built from that source.
The stager must capture actual executable identity and binary SHA-256.

## Final release checklist

- [ ] Select the exact product build/target and freeze its source, binary and asset hashes
- [ ] Keep unverified Meshy bytes out, or establish the generation-specific grant
- [ ] Resolve the four open source/embedded-asset records and review every final dependency obligation
- [ ] Review native/Rust runtimes and redistributables against actual binary imports
- [ ] Preserve project, font, LUT, dependency and global-data notices in each shipped depot/archive
- [ ] Add any real OSM/regional data only with the separate approved data-pack evidence and offer
- [ ] Check final source/asset changes against provenance and obtain publisher rights/trademark review
- [ ] Complete truthful Steam content/store disclosures with authorized human review
- [ ] Run the actual extracted candidate on its target platform, independently of these license checks
- [ ] Obtain explicit authorization for any agreement, payment, publication or upload
