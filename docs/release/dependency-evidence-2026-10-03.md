# Windows dependency notice collection

Collected 2026-10-03 with Rust 1.93.0 for `flightsim-app` 0.6.0-alpha.21,
default application features, target `x86_64-pc-windows-msvc`. This is mechanical
source/notice evidence, **not completed dependency review or permission to
publish**. The inventory remains `not_reviewed`, with four unresolved records.
No application build, Windows runtime check or release was performed for this
collection. The release recipe and commercial-staging policy are unchanged.

## Exact scope and reproduction

The [inventory](licenses/dependency-evidence/dependency-inventory.json) contains
347 packages in the conservative normal/build closure, 682 package notice files
and two supplemental font/LUT notices. Dev-only edges are excluded. `earcutr`
belongs to the offline generator and is absent from this application closure.

The unchanged collector used fresh target-filtered metadata from the current
checkout and existing cached sources, without network access. Keep the metadata
outside source control: it contains host-specific source paths. Its exact byte
hash is recorded in the inventory, together with the Cargo.lock, asset manifest
and supplement manifest hashes. Recollect after any relevant input change;
do not copy an earlier commercial-staging or GNU inventory into this MSVC slot.

```sh
# Use Rust 1.93.0 and a new private metadata destination.
cargo metadata --offline --locked --format-version 1 \
  --filter-platform x86_64-pc-windows-msvc > "$PRIVATE_METADATA"
python3 scripts/collect-dependency-notices.py --metadata "$PRIVATE_METADATA" \
  --root-package flightsim-app --target x86_64-pc-windows-msvc \
  --output "$NEW_NOTICE_DIRECTORY"
```

The application feature record must be exactly `["default"]`. Preserve every
original notice byte, including line endings; `docs/release/.gitattributes`
disables text normalization under `licenses/`. The metadata was resolved offline
on a Linux host using the exact MSVC platform filter. This supplies a conservative
source graph, not an attestation of a Windows build or a linked-binary SBOM.

Verification compared each collected notice with its original cached crate,
workspace license or version-pinned supplement. All 337 registry archive checksums,
supplement hashes, four enabled embedded-asset hashes, inventory bindings and
the stager's referenced-file/text/path checks passed. Collection output includes
no raw metadata, absolute host paths, reviewer record or authorization receipt.

## Review still required

Retain declared license expressions exactly. `OR` permits selecting an offered
alternative; `AND` requires both obligations. Relevant cases include
`self_cell` (Apache-2.0 OR GPL-2.0-only), `unicode-ident` (an MIT/Apache option AND
Unicode-3.0), `dpi` (Apache-2.0 AND MIT) and `bevy_mikktspace` (Zlib AND an
MIT/Apache option). Nested notices, source headers and the GilRs controller
mapping database need review independently of their containing crate's license.
See [Cargo's license-field documentation](https://doc.rust-lang.org/cargo/reference/manifest.html#the-license-and-license-file-fields).

The four unresolved records are preserved:

- **constgebra 0.1.4:** its [exact manifest](https://github.com/knickish/constgebra/blob/eae8e094e5779f42f1db2a7adf9036aa33744bc8/Cargo.toml)
  declares MIT OR Apache-2.0, but no primary license text is packaged or present
  in the inspected pinned source. A reviewer can assess the declared Apache
  option and [official redistribution conditions](https://www.apache.org/licenses/LICENSE-2.0).
  Missing text is not absence of a declared license. Do not invent a copyright
  holder/year or label generic legal text as a notice supplied with this crate.
- **hexf-parse 0.2.1:** the exact published archive declares CC0-1.0 but lacks
  primary full text. Review the declared route with the [official CC0 text](https://creativecommons.org/publicdomain/zero/1.0/legalcode.en),
  or the contribution consents in [upstream issue 26](https://github.com/lifthrasiir/hexf/issues/26).
  Earlier research bound all four historical authors of the packaged Rust file
  to those consents. Applying that additional 0BSD evidence to these bytes still
  needs an explicit rationale; later upstream code/license files are not the
  published 0.2.1 archive. Its recorded SPDX declaration remains CC0-1.0.
- **Bevy AgX LUT:** exact upstream input and transformed output identity have
  been established; an applicable redistribution grant remains unestablished.
  The [pinned input](https://github.com/MrLixm/AgXc/tree/898198e0490b0551ed81412a0c22e0b72fffb7cd)
  and Bevy's crate SPDX do not resolve this asset-specific question.
- **Bevy Blender Filmic LUT:** the [exact recipe](https://github.com/bevyengine/bevy/blob/f667c282dad2c1419afb5836ded22a3ec263970e/crates/bevy_core_pipeline/src/tonemapping/luts/info.txt)
  does not pin the producing Blender/OCIO/source LUT versions or establish an
  applicable rights record for the resulting bytes. A candidate upstream notice
  cannot be assigned to this output without that binding.

The feature `tonemapping_luts` embeds all three LUTs. Choosing a different active
tonemapper does not remove the unresolved assets. Font/LUT notices are separate
from crate-code licenses. A genuine completed review must bind this inventory's
hash, identify its reviewer/date/scope and package evidence for each resolution;
collection does not supply that review.

Before distribution, inspect the actual MSVC executable's imported DLLs and
native/static runtime contributions, including Rust/runtime code and any
redistributables. Metadata cannot establish their complete inclusion, notices
or redistribution conditions. No Linux system library, GPU driver or toolchain
is included by this collection. MSVC and GNU runtime reviews are distinct.

## If a Swift-only Windows candidate is explicitly selected

The existing `commercial-staging` feature can produce a local Swift-only
candidate, but it does not resolve the four dependency records. It also changes
the argument-free aircraft and asset-discovery policy. Adoption for publication
requires a coordinated, separately reviewed release recipe, feature and smoke
contract change; merely swapping an allowlist or choosing Swift at runtime is
insufficient. Recollect with that exact feature set after the decision.

Acceptance must establish an exact-source MSVC build, adjacent-asset isolation,
Swift default/model identity and chase-view PNG/process exit 0 from an extracted
bundle and unrelated working directory. Verify clean failure for the excluded
Light Single model and preserve explicit legacy no-model replay identity.
Keep terrain/climate obligations and exact notice bytes; verify the final
archive and real platform/runtime prerequisites. Applicable rights evidence,
completed dependency review and inventory-bound publication authorization remain
separate requirements. See [candidate staging](commercial-candidate-staging.md)
and [the distribution audit](commercial-distribution-audit.md).
