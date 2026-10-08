# Bounded association of a linker output with the audited application

This change collects facts only. It does not change the build recipe, launch a
linker, activate a workflow, establish static-member inclusion or authorize a
release. A fresh native run is still needed to determine the cause of the
previous 55 parsed commands with no matching application output.

The caller must retain the existing exact source, original-build, trace,
application-executable and unique application-fingerprint checks. The target
tree must be the fresh isolated tree produced by that audited build. The
collector does not accept arbitrary old workspaces as a substitute for the
caller’s build audit.

For a new capture, one parsed command must have exactly one absolute `/OUT:`
argument naming a regular EXE directly in that release root or its `deps`
directory. Parent-directory aliases, other build roots, nested unrelated paths,
UNC/device/stream paths, symlinks and reparse escapes cannot identify an output.
The name is not used as a substitute for byte identity. The output's exact
size and SHA-256 must equal the already audited application, and exactly one
constructed command must qualify. Two matching commands remain ambiguous even
when their outputs are byte-identical.

Candidate examination is bounded to 64 admitted commands and one GiB of total
candidate hashing. Size mismatches are rejected before content reads. Existing
trace, file and matching-command budgets remain in force. The public projection
contains only a closed method identifier and candidate/missing/mismatch counts;
paths, output names, raw command text and environment values remain private.

The optional `output_association` field identifies this content-based method.
`trace_observation.matching_output_commands` then counts byte-matching commands.
The private validator repeats the association against the original trace and
build files. A positive association under this method cannot be validated using
the explicit receipt-only `recheck_installed=False` mode: that mode lacks the
original files needed for the broader output-name association. Public factual
exports can still be inspected, but they are not a replacement for that replay.

Previously sealed records without `output_association` replay under their
original direct-path/fingerprint-name algorithm, including the original counter
semantics. An old unknown result is not retroactively promoted. Existing older
records without trace counters also retain their previous compatibility path.

The observed linker must still be an absolute `link.exe` path, and association
with an installed candidate still requires the exact source path and record.
The result describes a logged constructed command, not proof of process
execution. Implicit SDK selection and actual static contributions stay unknown;
whole-target, native-runtime and publication flags remain false.

No LIB/LIBPATH/INCLUDE capture or additional coverage requirement is introduced.
The existing command/build records and a justified conservative source-family
assessment may suffice for rights review without perfect tracing or an exact
static-member map. This observer merely closes one bounded output-identity gap
when the fresh data permits it.
