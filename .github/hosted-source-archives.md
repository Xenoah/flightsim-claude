# Hosted source archive evidence

This independent workflow checks downloaded GitHub source ZIP and tar.gz bytes.
It does not create a tag/release, launch the app, change rights decisions, or
satisfy binary, runtime, visual, GPU or publication acceptance. The existing
`scripts/check-source-archive.py` and all release gates remain unchanged.
The transport helper requires that exact reviewed checker SHA-256
`25c88911e4db103225e813d0d8aaa33d1d6d6d4aad97255b515b6be1ab4b1ccc`
in both the target commit and target working file before importing it.

Preparation verified repository ID `1318879072`, `Xenoah/flightsim-claude`, is
public. Each run verifies that again using anonymous GitHub metadata before
downloading. It fails if visibility or identity changes; it does not request,
read or generate credentials. Checkout uses the normal read-only Actions token
with credential persistence disabled. Archive requests do not use that token.

## Run only after source publication is separately authorized

1. Set `.github/hosted-source-archive-target.json` to the exact reviewed target
   source commit and tree. The prepared baseline is `7d8c8770a5f3aaa69f36303ac11390ff0755a037`,
   tree `4abfd4528b80e005c8c0a5dd9aa17848614cfb45`; it is not a receipt for a later
   source-migration commit. Commit this separate CI-only change on its evidence
   branch. After the target source is available on GitHub, separately authorized
   publication to `agent/hosted-source-archive-check` triggers this evidence
   workflow while the target's normal CI/native work can proceed independently.
2. The job verifies the workflow/helper/target-manifest bytes against its own
   evidence-branch commit, then checks out the explicit target into `source`.
   Its commit and tree must match both the reviewed manifest and GitHub metadata.
   Nothing is merged into main or written to the target source branch. Once the
   workflow is on the default branch, manual dispatch can rerun a selected
   evidence-branch revision using its already committed target manifest.
3. After a release tag has independently been authorized and created, set the
   manifest's `tag` to that existing tag, retaining the reviewed source SHA/tree,
   and run a separately reviewed evidence-branch revision. Both
   commit archives and both `refs/tags/TAG` archives must pass. The exact tag ref
   (including bounded annotated-tag peeling) is checked before and after the
   downloads. A tag must match `v[0-9][0-9A-Za-z.+-]{0,99}`.
4. Review the successful exact-evidence-SHA Actions run and its single JSON artifact.
   It records repository/commit/tree, tag object chain if applicable, requested
   and final URLs, archive byte lengths/SHA-256, normalized member manifest hash,
   content-checker hash, separate evidence commit/file hashes and observation time.
   An unexecuted workflow, offline fixture
   test or local `git archive` pass is never hosted evidence.

The job fetches the documented `https://github.com/OWNER/REPO/archive/SHA.zip`
and `.tar.gz` routes and, when requested, their `refs/tags/TAG` counterparts.
Only their exact HTTPS codeload redirect is allowed; unexpected redirects,
queries, credentials, proxy settings and HTTP content encodings are not used.
Downloads have a 64 MiB compressed-byte limit, 120-second transfer budget and
15-second socket timeout; metadata is capped at 1 MiB. The job has a ten-minute
limit. The unchanged content checker rejects oversized unpacked contents,
unsafe/duplicate paths, links, wrong/missing members, and the unresolved Light
Single bytes, including identical copies inside nested archives. No extraction
or binary upload occurs. The receipt is at most 16 KiB of text and is written
only after every requested archive and the final identity checks pass.

Run the same check locally from a clean, committed evidence checkout, pointing
at a complete checkout containing the manifest's exact source objects:

```sh
python3 .github/scripts/check-hosted-source-archives.py \
  --repo /path/to/target/source --evidence-commit FULL_EVIDENCE_COMMIT \
  --report /tmp/new-source-archive-receipt.json
```

The output path must be new. GitHub may regenerate different compression bytes;
the normalized contents must still match the exact committed export. Tag
observations prove the checked downloads at the recorded time, not future tag
immutability. Failed runs export no receipt. Raw response bodies, archives,
source/binary files and exception details are not uploaded.

Official format/URL and dispatch references:
- https://docs.github.com/en/repositories/working-with-files/using-files/downloading-source-code-archives
- https://docs.github.com/en/rest/repos/contents#download-a-repository-archive-zip
- https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows#workflow_dispatch
