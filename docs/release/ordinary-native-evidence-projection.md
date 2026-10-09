# Ordinary native evidence projection

`scripts/project-ordinary-native-evidence.py` is an additive, read-only factual
projector for the ordinary two-aircraft payload. It derives the same factual
categories as the full native policy projector without changing that projector's
diagnostic-bundle contract. It is not a review, runtime acceptance, publication
grant, archive check, or extracted-archive smoke. No UI capability gate is added.
The inherited capture, analytical/native parsers, source recipes, collector and
ordinary authorization/copy-plan contracts remain unchanged.

## API and authority

```python
project(repo, expected, build_private, build_text, bundle, *, runtime_facts_private)
```

Every root is mandatory, canonical, absolute and pairwise disjoint. The return
value is a schema-1 `ordinary_native_review_projection_not_approval` object.
The CLI uses `--repo`, `--expected-sha`, `--build-private`, `--build-text`,
`--bundle`, and `--runtime-facts-private`, printing JSON on stdout. Validation
failure returns exit 2 with a bounded diagnostic; it does not write evidence.

1. The reviewed `project-ordinary-release-payload.project_payload` validates the
   real ordinary gate, original copy plan, full native capture and actual bundle.
   It preserves original/fresh inventory bytes, permits only the separately
   reported metadata-digest semantic difference, and proves each fresh notice
   equals its shipped original. Its complete result is `payload_projection`.
2. The native capture remains the canonical source/build authority. The exact
   completed ordinary two-LUT capture supplies the executable; this projector
   never builds a replacement. Source, package closure, features, normal/build
   edges, source headers, modified-vendor provenance, embedded Fira/Tony/Filmic
   assets, build-script requests and PE import facts use the existing parsers.
   Duplicate metadata identities, closure mismatches and unexpected native
   payload files fail. Missing exact graph membership remains null, never an
   inferred feature set. Missing header coverage remains `not_established`.
3. `collect-analytical-runtime-facts.project` runs its existing complete private
   replay validator, including original installed files and fresh final-output
   association. There is no offline bypass. Its unchanged public projection is
   retained as `runtime_facts`; no raw paths, commands, notice text or binary
   bytes enter the export.
4. The runtime packet's compiler must be observed and exactly equal to the
   captured ordinary compiler, including release, revision, host and LLVM. Its
   private `recipe_cfg_args` must be `['-D', 'warnings']`. Its trace and executable
   paths must name this ordinary capture's `capture/ordinary/build.stderr` and
   `target-ordinary/x86_64-pc-windows-msvc/release/flightsim-app.exe`, and their
   observed byte records must match those inputs. An unknown compiler cannot
   establish that association. An unknown constructed final command is allowed;
   its reason and every remaining unknown are retained. File identity does not
   establish actual linker execution, static membership, SDK selection or runtime
   closure.
5. Payload/capture, canonical source and full runtime validations repeat before
   return. Parsed graph/messages/compiler bytes are bound before consumption and
   rechecked afterward. A mutation fails rather than becoming a new baseline.
   The formatted output is limited to 8 MiB; inputs and arrays have explicit
   limits, and linked/aliased/nonregular inputs fail existing validators.

All top-level `release_authorized`, `dependency_review_approved`,
`review_applicability_approved`, `runtime_accepted`, and
`native_runtime_coverage_complete` fields remain false. Inner collector and
modified-source approval fields also remain false. Package `review_status`
remains `not_reviewed`. The existing authorization receipt is only a bound input.

## Full result and comparison sections

The full result retains the full native reference's source/recipe/target/compiler,
profile, application features, package records, edges, headers, embedded assets,
build-script requests, PE facts, shipped files, OS prerequisite limitations,
static-contribution unknowns, runtime-coverage limitations and unresolved reviews.
It adds `payload_projection`, `inventory_comparison`, unchanged `runtime_facts`,
`source_content_files`, and `content_view`. `bindings` additionally records the
original compiler text, build trace, runtime public projection and private
manifest. Original and captured inventory/metadata identities are never replaced
with a normalized inventory or a content-count proxy.

`content_view(projection)` validates the top-level projector schema and returns:

```json
{
  "schema_version": 2,
  "kind": "ordinary_native_content_view_not_approval",
  "sections": {
    "source": {},
    "packages": [],
    "graph": {},
    "headers": {},
    "embedded_assets": [],
    "build_script_link_requests": {},
    "platform": {},
    "runtime_rust": {},
    "runtime_microsoft": {},
    "runtime_final_link": {},
    "runtime_query_outcomes": []
  }
}
```

The exact section names are exported as `CONTENT_SECTIONS`:

- `source`: source recipe and the complete `source_content_files` selection below
- `packages`: every full package/provenance/notice/feature record
- `graph`: target/profile/application feature selection and all normal/build edges
- `headers`: the complete source-header observation, including its manifest hash
- `embedded_assets`: all exact embedded asset/notice identities
- `build_script_link_requests`: the complete request list and the false
  static-inclusion inference flag
- `platform`: compiler release/revision/host, PE, redistributed-native list,
  prerequisite/static/runtime coverage limitations, and unresolved reviews
- `runtime_rust`: all Rust identity, sysroot state, cfg state, library/notice
  identities and non-inference flags
- `runtime_microsoft`: all candidate product/toolset/SDK identities, versioned
  file/terms observations, and every entitlement/selection/coverage unknown
- `runtime_final_link`: all final-link classifications, reasons, linker identity,
  request lists, library-origin classifications, observation/non-inference flags,
  and optional trace/output-association diagnostics
- `runtime_query_outcomes`: every query ID, outcome and exit code, in original order

Schema v2 sorts complete build-script request rows across packages, retaining
every row, duplicate multiplicity, same-package event sequence and each
`linked_libs` order. Cargo
event scheduling is not library-link ordering. All other arrays retain their
order. Rlib basenames, library hashes, candidate versions, unknowns and optional
field presence remain exact. Microsoft installed-candidate identities still
require genuine reviewed observations; a new host is never accepted by assumption.
Only `trace_observation.total_lines` is excluded as compiler-log volume. Every
other trace counter and output-association field remains exact or fails the
original closed-schema validator. Schema v1 comparison views are rejected.

### Explicit runtime content exclusions

`runtime_content(value)` first runs the collector's closed schema validator.
It does not recursively remove arbitrary key names. Its only observation-level
exclusions are:

1. `evidence_id` from Rust `notices[]` and `target_rlibs[]`; Microsoft `vswhere`,
   each VS `default_toolset_hint`, every listed `terms_candidates[]`, every
   toolset `linkers[]` and `runtime_libraries[]`, every SDK `libraries[]`; and
   the five final-link file observations. These are packet-local receipt IDs.
   Their status, byte record, file version, product version and semantic labels
   are preserved, subject only to the four build-input exceptions below.
2. `record` from `final_link.trace`, `.executable`, `.fingerprint`, and
   `.link_output` only. These capture-specific hashes remain in unchanged
   `runtime_facts`, and exact trace/executable association is enforced before
   projection. The view retains each observation's presence, status and version
   fields under `build_input_observations`. The linker record is never removed.
3. `final_link.installed_candidate_evidence_id` becomes an explicit candidate
   identity containing product, installation version, toolset version and the
   exact host/target/file observation. Null remains null; an unresolved or
   ambiguous candidate ID fails.
4. Each `query_bindings[]` retains `id`, `outcome`, `exit_code`; only its stdout
   and stderr byte bindings are omitted from the content section. Parsed Rust,
   cfg and Microsoft content is preserved, and unchanged raw bindings remain
   in `runtime_facts`.
5. The runtime projection envelope's `private_manifest` binding and `source_sha`
   are capture-specific, so they remain only in the full packet. Its schema/kind,
   target/requested toolchain/native-Windows constants and false approval flags
   are enforced by the closed validator; target/compiler are also included in
   the native graph/platform content sections. These fixed envelope fields are
   not silently interpreted as approvals.

No new unknown field is dropped: the closed collector schema or explicit
observation field-set checks reject it. Optional `trace_observation` and
`output_association` retain their presence/absence. A present non-null trace
object loses only `total_lines` in the comparison view, after its original type,
bounds and consistency have been validated; raw `runtime_facts` remains unchanged.

### Source content selection

Rows are sorted `{path, git_mode, sha256, bytes}` records, rehashed against
canonical capture source evidence. The closed selection is:

- Every tracked path under `crates/`, `vendor/`, `assets/`
- `Cargo.toml`, `Cargo.lock`, the ordinary asset-rights manifest, and
  `scripts/tonemapping-two-lut-source.json`
- The source-header and modified-source-provenance manifests when present
- Every replacement patch referenced by a projected modified-source package
- Every exact shipped path also present in the canonical source record, except
  the dependency inventory, dependency review and ordinary authorization receipt

The three excluded packet paths have independent raw input bindings and cannot
recursively authorize themselves. The whole canonical source SHA/tree still
binds all tracked files, and the ordinary gate independently binds its complete
source/copy plan. A separate applicability policy may require a broader source
content boundary; this subset does not waive that review or excuse changed
engineering inputs. Package records retain exact replacement and vendor-file
identities as well.

## Verification limits

The dedicated tests are synthetic only. They exercise the unchanged ordinary
gate/copy plan, notices, PE parser, runtime collector replay and new projection;
the genuine native capture and canonical-source admission boundary is mocked.
They cover adversarial paths/links, closure identities, feature changes, runtime
compiler/build association, original installed-file mutation, schema extensions,
private-data exclusion, content identities/unknowns, and read-time mutations.
They do not establish a Windows build, substantive review, runtime acceptance,
archive extraction or publication readiness. Existing reference-projector and
pinned scripts are not modified by this preparation.
