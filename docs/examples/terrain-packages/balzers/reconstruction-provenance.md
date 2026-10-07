# Balzers terrain reconstruction, recipe 1

Local reconstruction from Liechtenstein_Terrain_Package_v1.zip (6551156 bytes).
Source archive SHA-256: fc4de5f479d769b0aa6f028ab965a8798f9c11bdbae157bffc57ecd45df66b2a
Original manifest SHA-256: 34e8759340c315c745a9d83746944d082bb94b8139e2806756749572b7d53384

Selection: geographic-quadtree L10 roots (1077,244) and (1078,244), with every
child through L13: 2 + 8 + 32 + 128 = 170 tiles. x increases eastward, y southward;
each L10 cell spans 180/1024 = 0.17578125 degrees in both axes. Bounds (W,S,E,N):
9.31640625, 46.93359375, 9.66796875, 47.109375 degrees. Approximately 26.7 km east-west
by 19.5 km north-south near Balzers; these are approximate dimensions, not survey
measurements. The original source's Balzers sample (47.068 N, 9.501 E) is inside.
It is not the rectangle's exact center. Complete source cells constrain placement.

All 765 input DEMs and all 3 original documentation files are verified before
selection. Selected DEM bytes and all original documentation bytes are unchanged.
No DEM rebake, resampling, new datum conversion, height adjustment, downloaded
input, imagery, buildings, airport data or executable content is introduced.
Original manifest source records (including credits/license references) are retained
verbatim as field values. The original documentation describes the larger source:
its 765-tile counts, nine roots and full-area verification are inherited evidence,
not a claim that this subset includes those tiles. Its notice's SOURCE_PROVENANCE.json
reference refers to upstream evidence preserved in docs/source-provenance.md.

The original license bundle and notices remain authoritative source material.
This reconstruction verifies byte integrity and complete selected tile families;
it does not repeat the upstream GeoTIFF/geoid survey, establish legal clearance,
accept terms, authorize redistribution or qualify commercial/Steam distribution.
GLO-90 is a reflective DSM, not bare earth or navigation/airport survey data.
The matching 10-arcminute geoid has unquantified vertical accuracy. A 65 x 65
runtime grid is interpolated source detail, not new measurements. Outside the
subset, existing global fallback remains. Package replay/aircraft restrictions remain.

This is a new artifact, package balzers-glo90-rebuilt@1.0.0. The lost historical
Balzers ZIP was reported as 1461931 bytes with SHA-256 d9ec9ef06dbc9ad1d878a36dddd395b7627cde7d681093c8cb31835dcce93cc6.
Those bytes were not recovered; no byte equality or recovered publication approval
is claimed. Stored ZIP entries, sorted payload paths, UTF-8/LF JSON, fixed timestamps
and regular 0644 attributes make this recipe independent of zlib compression versions.
No URL is guessed. Optional catalog construction is offline metadata only and cannot
verify that an explicitly supplied public destination exists or contains these bytes.
