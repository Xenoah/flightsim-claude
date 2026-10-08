# Independent bounded Filmic review

Status: final run-5/run-6 numerical reproduction and documented source-notice route supported within the bounds below. No publication authorization, whole-release clearance, or claim about Bevy's historical Blender version is made. Review completed 2026-10-08. The final evidence pair uses hardened execution.

## Final hardened pair

The final reproduce.py is SHA-256 `79a98984849c468495fd0bd4e99fd990757e9a79707c08d2c3ec5a14d953e602`. I reviewed it and run_pinned.py, then independently checked run-5 and run-6 with the review's own verify.py. Every KTX2, complete texel, grid-coordinate, half conversion, EXR header, EXR pixel, file hash, exact-alpha and finite-value check passes. Both final observations have SHA-256 `8a2e3e774e65fc95ee231b01e7589562eb8eed1bf54d62690691077c8371382a`, and both exactly match the decoded reference.

The driver clears ambient OCIO-prefixed variables and sets the bundled config before launching the hash-pinned Blender binary with factory startup and Python failure exit status. It requires Blender's exact config-selection acknowledgement before writing its invocation record. The evaluator script verifies the executable, system DATAFILES location, explicit OCIO path, startup flags, and six packaged input/notice hashes, including a second input check after the transforms. Each final invocation's script, stimulus and observation hashes independently match its actual files. This closes the initial ambient-configuration concern within the reviewed invocation contract. The invocation report is an execution record from that driver, not an OS-level trace of each table read.

I also reviewed verify_reproduction.py and test_comparison.py. Independently running the six synthetic comparison tests passes. The test cases are clearly separated from evidence of the actual transforms. The read-only final audit is verification-final.json; all reviewed scripts' exact hashes are recorded there. It does not invoke Blender or create any LUT.

## Numerical result

The actual Cargo reference `bevy_core_pipeline-0.18.1/src/tonemapping/luts/Blender_-11_12.ktx2` has SHA-256 `a81a2462182bc8499d1a222345a72e7c4f1fabc2cb23d9c543caa567ccba7ad7` and Git blob `db07c847a1879aabe14cbcc1a4d00ec402bc9f1e`. Independent KTX2 header parsing and direct `libzstd` decompression produce 2,097,152 bytes with SHA-256 `b8e2f0a26ee58edda1cf54e591175e8688ae21550e4d16af4c409cfdf5d919dd`.

Both run-5 and run-6 equal those decoded bytes, without tolerance. The container describes one 64×64×64 RGBA16F level, one face, no layers, and Zstandard compression. All alpha values are exactly one. This review compares decoded texels, not newly encoded KTX2 container bytes, and is not a general KTX2 conformance audit.

Independent Python stdlib conversion of every output32 value to binary16 matches every output16 byte. All output32 values are finite. Independent ZIP16 OpenEXR decoding confirms that all six EXRs are 64×4096 with FLOAT32 A/B/G/R channels and increasing-Y rows. The decoded stimulus EXRs exactly equal stimulus.rgba32f; the final EXRs exactly equal output.rgba32f. Intermediate EXR values are finite. Every file hash and byte count listed in the two observations is valid, and the complete observations are byte-identical.

## Recipe and scripts

The documented stimulus is sampled at i/64, not i/63: each coordinate becomes `(2^((i/64)*23-11))*0.18` using float32 operations. Therefore the last sampled exposure is 11.640625, not 12. The implementation correctly has red varying fastest, then green, then blue; strip coordinates are x=R and y=64B+G. Alpha one is consistent with the RGB-source to RGBA-loader path. Independently recomputing all 64 levels with float32 operation boundaries and checking every coordinate exactly matches the stimulus bytes.

The reproduction implements the documented image sequence: linear stimulus → 32-bit EXR using Filmic sRGB; reinterpret that EXR as sRGB → 32-bit EXR using Linear; then nearest-even binary16 conversion. The additional initial identity Linear EXR save/load preserves every stimulus byte. The two explicit vertical flips correctly bridge Blender's image-pixel accessor and the top-row-first strip. No renderer is invoked by the script, and no reference LUT is read by its transformation code.

Reviewed source hashes:

- stimulus.rs: `d12c2d58e14c92d54091eb4763eb522ea3a3c387a9a31b2c7ed7b5302b84d625`
- reproduce.py: `79a98984849c468495fd0bd4e99fd990757e9a79707c08d2c3ec5a14d953e602`
- stimulus gist snapshot: `6b289874908fd89ceab101c33d8e8f0f62910843e62cc790e1335778e2c406ca`
- conversion gist snapshot: `31dc8e31e282e7c9b3243a24664207936918403312de598107e699615023d00d`

The two gist snapshots' Git blobs equal the pinned raw URLs recorded in their metadata. The scratch bevy-recipe.txt differs from upstream only by one additional terminal LF; actual Cargo luts/info.txt is the exact 1171-byte upstream blob `e3b6b8a17f4296ab6ceb7d2338a1e93965f561ea`, SHA-256 `e49669e0d8444697e9a564169c11620dc8b9a80c65bacdc4226613b636ea626b`. Use the exact file or label the normalization.

## Evaluator and source identity

I independently hashed the archive and executable. They match the recorded officially checked archive SHA-256 `1497f83f93e9bbbde745422c795ed10fe15f92f5622b4421768f149fbe776981` and executable SHA-256 `40999207b8b7f8a43a8902535cd6e3a35724b9590cb08715b161c39c5a0119f6`. The prior worker's official checksum verification is corroborated locally; this review did not independently redownload the official checksum.

The production executable reports build 55485cb379f7; the GitHub v3.4.1 source tag resolves to ef9ca44dee7fe3e25089dbfc49c69e9eff83ba5a. These must not be described as identical source commits. Instead, every relevant packaged config/table/notice was hashed as a Git blob and independently compared with a freshly fetched, untruncated upstream tag tree. All six match: config.ocio; filmic_desat65cube.spi3d; filmic_to_0-70_1-03.spi1d; srgb.spi1d; srgb_inv.spi1d; OpenColorIO.txt. Exact IDs and SHA-256 values are in verification-final.json and source-evidence.json.

The final script constrains OCIO and asserts evaluator and asset hashes before transformation. Packaged input identity and the reviewed invocation contract should not be described as an OS-level per-file read trace.

## Documented BSD notice route and its scope

The pinned upstream config's header identifies the Filmic configuration's author and acknowledgements and directs readers to ocio-license.txt. Blender's 2017 Filmic introduction added those Filmic attribution lines while retaining that licensing cross-reference and added the relevant Filmic transforms. A fresh upstream read of commit 0434efa09ddc451675ce8f8e8e7df61f5cc34e89 verifies that `release/text/ocio-license.txt` was renamed without changes to `release/license/OpenColorIO.txt`. The unchanged blob is `3dfcc78df9ef0cfa7b8f79f9e374beb549f5a15c`, exactly the 1520-byte notice bundled in the verified evaluator.

This provides affirmative source evidence for documenting a BSD-3-Clause notice route for the pinned Blender OCIO configuration and its associated tables. The notice permits source/binary redistribution, with or without modification, subject to retaining/reproducing the copyright, conditions and disclaimer and observing the non-endorsement condition. The defensible notice package should retain the exact full Sony Pictures Imageworks Inc., et al. notice and the Filmic attribution, identify the precise upstream blobs and transform recipe, and state the generated data's relationship to those inputs.

Scope qualification: the four numeric table files do not carry individual licensing headers. Applying the notice to them is an inference from their inclusion in and explicit references from this configuration, its attribution, and its licensing cross-reference, reinforced by the original Filmic integration and unchanged notice relocation. Do not represent this as an individual legal opinion or newly obtained grant from each contributor. A notice file alone does not independently establish every contributor's title, resolve unrelated assets or dependencies, license the gist implementation, or complete release-wide compliance. This review does not decide whether numeric outputs are independently copyrightable or whether any particular notice would be legally mandatory; it records a conservative, evidence-backed notice route.

The byte match does not establish which Blender version generated the historical Bevy LUT. There is no evidence here of that historical evaluator version or that every Blender configuration revision is numerically identical. No original-asset replacement, publication, or blanket rights clearance follows from this review.

## Sources and reproducible review

- [Bevy v0.18.1 recipe](https://github.com/bevyengine/bevy/blob/v0.18.1/crates/bevy_core_pipeline/src/tonemapping/luts/info.txt)
- [Stimulus gist, pinned revision](https://gist.github.com/DGriffin91/e119bf32b520e219f6e102a6eba4a0cf/a9aa0fe151d98ba0f706f8225fce919b1d0fb2b9)
- [Conversion gist, pinned revision](https://gist.github.com/DGriffin91/49401c43378b58bce32059291097d4ca/820776c9c34fbc5ce9e9cca868b85441a382897f)
- [Pinned Blender config](https://github.com/blender/blender/blob/ef9ca44dee7fe3e25089dbfc49c69e9eff83ba5a/release/datafiles/colormanagement/config.ocio)
- [Pinned notice](https://github.com/blender/blender/blob/ef9ca44dee7fe3e25089dbfc49c69e9eff83ba5a/release/license/OpenColorIO.txt)
- [Unchanged license relocation](https://github.com/blender/blender/commit/0434efa09ddc451675ce8f8e8e7df61f5cc34e89)
- [Filmic introduction](https://github.com/blender/blender/commit/3cd27374ee53bc2ae3fd08b0b73c0a3118f81020)
- [Official Blender 3.4 archive directory](https://download.blender.org/release/Blender3.4/)

The independently authored verify.py reads only the reference and candidate files, does not invoke Blender, and writes its report to stdout. verification-final.json records its successful assertions. source-evidence.json preserves the relevant fresh connector observations and exact source IDs. This review writes only its separate review directory and makes no changes to the repository, reference LUT, production assets or external state.

The independent checker takes explicit `--base` and `--reference` paths and defaults to the final `run-5` and `run-6` directories. It emits the upstream-relative reference label, never the supplied absolute paths. For example: `python verify.py --base ./reproduction-work --reference ./bevy_core_pipeline-0.18.1/src/tonemapping/luts/Blender_-11_12.ktx2 > verification-final.json`. The base directory must contain the complete working inputs and generated outputs; the text-only evidence packet intentionally omits the evaluator and image binaries.
