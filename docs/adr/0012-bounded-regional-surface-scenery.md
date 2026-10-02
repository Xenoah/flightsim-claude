# ADR-0012: Independently bounded offline regional surface scenery

- Status: implementation on the surface-detail development branch; application acceptance tracked separately
- Date: 2026-10-02

## Decision

Add a separate FSSC v1 `.fsscenery` database in `flightsim-world` and the offline
`flightsim-scenerygen` importer in `flightsim-tilegen`. Do not extend airport FSAP,
load PBF at runtime, require a network service or mix scenery into physical terrain.
The public feature records retain OSM way IDs, zero-altitude WGS84 coordinates,
classified road centrelines, simple building footprints and selected landcover
polygons. Polygon triangulation is offline; coordinate transformations use core's
`LocalFrame`/ECEF interfaces. OSM elevation tags are not used.

Building heights distinguish direct `height`, `building:levels` × 3 m, and a 9 m
visual fallback. Road widths likewise identify source versus class fallback.
Neither fallback nor a rendered building facade is surveyed information.

The importer explicitly omits unsupported multipolygon relations and their member
ways, including old-style relations whose tags live on an outer member. This
prevents a member outline from filling a real hole. It also omits elevated/tunnel
roads and raised buildings rather than draping them as if they were ground-level.
These are reported omissions, not geographic completeness. v1 neither invents
missing streets/buildings nor carries addresses, road names or contributor metadata.

## Independent bounds

The source is held once in an immutable byte buffer, capped at 64MiB. Two sequential
PBF passes use the repository's safe vendored parser and never `IndexedReader`.
Each pass permits at most 512 blocks and 5,000,000 elements. The validated
uncompressed size is charged before decoding, with a cumulative 256 MiB per-pass
limit. Each decompressed block is separately bounded below 32 MiB by the parser. Candidate way count is 50,000,
candidate node references and total relation references are capped at 1,000,000,
as are retained points. Oversized individual ways are counted and omitted before
reference allocation. Limit exhaustion of aggregate work is an error, not a silent
partial successful database. These are algorithm/data bounds, not an OS sandbox
or a frame-time/total-process-memory guarantee.

The runtime file has a 48-byte little-endian header with exact payload size, class
counts, point/index counts, metadata length and FNV-1a checksum. Unknown versions,
flags/classes/provenance values, bad sizes, trailing bytes, invalid indices,
nonfinite/out-of-range coordinates, nonzero altitude, dimensions, duplicate source
IDs and regional extents beyond 200 km are rejected. The 32 MiB payload, 50,000 total
features, 1,000,000 points, 3,000,000 indices and 512 points per feature bounds apply
independently of header claims. All metadata is bounded and includes the relevant
OSM attribution and ODbL URI. The checksum detects corruption; it is not source
authentication. Fixture manifests also record cryptographic SHA-256 identities.

Before quadratic polygon validation, the full database is charged a conservative
`3*n² + 10*n` work estimate per polygon and rejected above 50 million units. Ring
simplicity, consistent triangle winding, unique triangles, coverage area, paired
interior edges, complete boundary coverage and valid interior diagonals are checked.
A query scans at most 50,000 conservative zero-altitude ECEF bounds, then ranks
and filters by actual local-tangent-plane line/polygon distance, returning at most
4,096 references with deterministic class/source-ID ties. Polygon interiors have
distance zero. Its temporary candidates and exact distance work are bounded by
the database feature and point limits. Loading and
queries are bounded synchronous operations, not a measured frame-time guarantee.
Renderer residency/build budgets remain a separate responsibility.

## Rights and packaging

The code remains under the project code licence. OSM-derived databases are ODbL 1.0
and must remain separately identifiable, with attribution, licence notice and
machine-readable derivative database available without restrictive extra terms.
The public Geofabrik extracts omit contributor user names/IDs and changeset IDs.
No raw PBF is required by or bundled into runtime. An optional regional fixture is
separate from the default build and commercial-staging allowlist.

Reviewed primary sources on 2026-10-02:

- [OSM copyright](https://www.openstreetmap.org/copyright)
- [ODbL 1.0, sections3.1 and4.2–4.7](https://opendatacommons.org/licenses/odbl/1-0/)
- [OSMF attribution guidance, databases and computer simulations](https://osmfoundation.org/wiki/Licence/Attribution_Guidelines)
- [Geofabrik Liechtenstein extract and metadata policy](https://download.geofabrik.de/europe/liechtenstein.html)

Commercial use is allowed by ODbL, subject to those obligations; this is not a
Steam release approval or a substitute for final distribution review.

## Limits

This is regional OSM geometry, not a global detailed world, imagery,
photogrammetry, complete multipolygon support, procedural city generator,
road-routing database, collision scenery or real building appearance.
The independent Copernicus GLO-90 demo terrain is a reflective DSM with coarse
matching EGM2008 correction. It is not bare-earth DTM, surveyed building bases,
full-resolution textures, or evidence that unrendered areas are detailed.
