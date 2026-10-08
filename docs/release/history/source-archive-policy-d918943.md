# Source archive exclusion policy

Status: prepared, subject to independent review and exact hosted-archive verification.
This policy does not authorize a release, select the Swift-only deliverable, clear
asset rights, remove git history, or alter binary publication gates.

GitHub documents that its branch, tag and commit source snapshots, including the
release page's automatic ZIP and tar.gz links, use `git archive`:
https://docs.github.com/en/repositories/working-with-files/using-files/downloading-source-code-archives
Git documents that committed `export-ignore` attributes omit files from archives:
https://git-scm.com/docs/git-archive#ATTRIBUTES

The exact root rule excludes `assets/aircraft/light_single.glb`, whose generation
rights remain unresolved. The model is retained in the development tree/history;
its project-authored JSON and rights/provenance records remain included. This does
not change ordinary application defaults. Extracted source archives lack that
legacy external model; runnable reduced candidates must use the separately
reviewed Swift recipe. Do not describe these archives as including every asset.

The rule is committed before selecting a candidate commit. A worktree-only or
`.git/info/attributes` rule would not establish GitHub behavior. It applies to new
snapshots of commits containing it, not earlier tags/commits or cloned history.
No forced tag movement or history rewrite is part of this preparation.

## Required verification before a release

1. Independently review the exact final source and export rule. Run
   `python scripts/check-source-archive.py --commit FINAL_COMMIT` for both local
   formats. It must retain the original Swift assets and required notices while
   excluding the named model and any identical nested archive copy.
2. After the parent has separately authorized and published the source commit,
   download BOTH hosted commit ZIP and tar.gz snapshots using GitHub's documented
   commit archive URLs or REST archive API. Record final URL, exact commit, archive
   SHA-256 and byte length. Run the same checker with `--archive LOCAL_FILE` on
   each. Compare every normalized member path and file byte hash against the
   reviewed local commit archive; local success alone is not hosted proof.
3. Resolve the intended tag to that same reviewed commit. Creating the tag/release
   awaits the appropriate publication decision and all existing gates. Inspect
   both tag archives again and bind the receipt to the tag, commit and file
   manifests before declaring the published release correct. No positive receipt
   may be fabricated from a plan or an unexecuted hosted check.
4. GitHub may vary compression bytes while retaining extracted contents. Use the
   normalized member hashes for content equality and retain the observed archive
   hash for the actual captured download. Reject duplicates, unsafe member paths,
   symlinks, unexpected files and partial/mismatched archives.

This is a bounded exclusion for the known unresolved Light Single GLB. It is not
an exhaustive title review of all retained source, data, screenshots or vendored
components. Modified dependency source must have its own complete provenance and
notice review. Do not attach original crate archives, deleted third-party routine
text, or patches that reconstruct removed routines as replacement evidence.

## Source-build usability and checked limits

The omitted GLB is an external runtime asset, not a Rust `include_bytes!` input.
The ordinary application still names it as its default model. Consequently an
ordinary launch or the existing two-aircraft package acceptance cannot be claimed
from an extracted archive; the ordinary binary gate remains blocked. The original
project-authored Light Single configuration and replay identities are retained.

The existing Swift path selects `aircraft/swift_sport.glb` under
`commercial-staging`. The archive retains that GLB, its JSON, project licences,
terrain/climate inputs and notices. The source command for the separately reviewed
analytical candidate is:

```sh
cargo run --locked --release -p flightsim-app --no-default-features \
  --features commercial-staging,analytic-tonemapping
```

Preparation checked an actual extracted archive with Light Single absent using
`cargo check --locked --offline -j 2 -p flightsim-app --no-default-features
--features commercial-staging,analytic-tonemapping`; it passed on Rust 1.93/Linux
with the real ALSA/udev development sysroot. This establishes compilation without
the model, not a successful native launch, GPU appearance, Windows qualification
or approval of the reduced release. The exact final modified-dependency source
must be rechecked after integration. General source tests need their ordinary
platform development libraries; rights/package checks may intentionally reject
an extracted archive missing the excluded legacy model. Do not bypass those
checks or call ordinary two-aircraft acceptance satisfied.

The checker builds its expected archive in a fresh temporary bare repository,
using only immutable source objects and committed attributes. Source-local
info/attributes, local attribute-file configuration, global/system attributes
and Git environment overrides cannot remove expected notices. ZIP and tar
adversarial tests verify this boundary and reject the resulting truncated input.
