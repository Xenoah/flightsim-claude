# Source archive policies

The full source candidate replaces the current Light Single path with one exact
project-original high-wing asset. Its source archive policy is explicitly
`original-highwing-v1`; the legacy `legacy-meshy-excluded-v1` checker behavior
remains the default and rejects this new path-inclusive archive. This is source
preparation only and does not authorize publication.

The committed root `.gitattributes` no longer applies `export-ignore` to the
current path. The new checker requires SHA-256
`b41f29ade89701d31759e6bc8164d5cdb3aa8734f512628af63823ad7eaaa3cc`, its current
original-source rights record, unchanged profile and retained authoring/provenance
files. It continues to reject the old Meshy payload hash anywhere, including
renamed files and nested archives. Removing an exclusion is not a grant over
historical bytes. Previous source-export decisions and limitations are retained
in [the historical policy](history/source-archive-policy-d918943.md).

For the reviewed exact final commit, check both local formats:

```sh
python scripts/check-source-archive.py --commit FINAL_COMMIT --policy original-highwing-v1 --format zip
python scripts/check-source-archive.py --commit FINAL_COMMIT --policy original-highwing-v1 --format tar
```

The checker still creates expected exports from immutable objects in a fresh bare
repository, ignoring ambient attributes, and compares every normalized member
and byte. Duplicate/unsafe paths, symlinks, unexpected files, changed notices and
partial archives remain rejected. New two-LUT source identity is checked
separately by `scripts/check-full-two-aircraft-source.py`; neither check is native
qualification or release authorization.

After separately authorized source publication, both hosted commit ZIP and tar
snapshots must be downloaded and checked with `--archive`, recording observed
URL, commit, length and SHA-256. Then confirm the authorized tag resolves to that
same commit and repeat both tag snapshot checks. A local export pass does not
prove hosted behavior. Whole/native review, terms adoption where needed, final
inventory binding, two-aircraft Windows runtime/screenshots and publication
authorization remain required. No existing release admission is broadened here.
