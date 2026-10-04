# Regional package fixture

`region-synthetic.zip` is a deliberately synthetic, stored ZIP32 package used by
local import and new-flight transaction tests. It contains a 3 × 3 FSDM grid for
level 9 / x 900 / y 180 with every sample exactly 350 WGS84 ellipsoidal metres,
and a short synthetic CC0 license-text fixture. It is not surveyed terrain and
has no external inputs. The manifest contains the SHA-256 of both payloads.
`https://example.test/terrain` is inert test metadata and is never fetched.
