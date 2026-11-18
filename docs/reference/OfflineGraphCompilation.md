# Offline graph compilation

The offline routing graph is the road network the SDK routes over when no
network connection is available. `rustic-maps-offline-graph-compiler` compiles one
OpenStreetMap extract into a single `RMOG` archive.
`rustic-maps-offline-graph-archive` reads that archive back with full validation on
every target, and `rustic-maps-core` answers offline routes over it with A\*.

The compiler is a maintainer and CI tool. It never ships inside the SDK, and
it stays native-only because its OpenStreetMap PBF reader does not build for
wasm. The archive format lives in the wasm-viable
`rustic-maps-offline-graph-archive` crate, which the compiler re-exports under its
own `archive` path.

## Inputs and output

The compiler reads one OpenStreetMap `.osm.pbf` extract. The extract must
contain the nodes, ways and turn-restriction relations of the region you want
to route in. Anything outside the extract does not exist for the graph.

```console
rustic-maps-offline-graph-compiler region.osm.pbf region.rmog
```

After writing the file, the tool re-reads it through the validating reader
and prints the node, edge, turn-restriction and lane-guidance counts
(`crates/rustic-maps-offline-graph-compiler/src/main.rs:41`). A compile whose bytes
the reader rejects exits with a failure instead of leaving an unusable
archive behind.

## Container layout

An archive is one little-endian file. Format version 1 opens with a fixed
header, then a table of contents that names every resource, then the resource
payloads in canonical order:

```text
offset  0   magic "RMOG"                                4 bytes
offset  4   format version                              4 bytes
offset  8   table-of-contents entry count               4 bytes
offset 12   table of contents, 48 bytes per entry:
              resource name                            32 ASCII bytes, NUL-padded
              resource offset                          u64, a multiple of 8
              resource byte length                     u64
            resource payloads start on 8-byte boundaries and are separated
            by up to 7 zero bytes; the file length is padded to the same
            8-byte alignment
```

The format constants live in
`crates/rustic-maps-offline-graph-archive/src/archive.rs`: `ARCHIVE_MAGIC` (`"RMOG"`),
`ARCHIVE_FORMAT_VERSION` (`1`), `RESOURCE_NAME_BYTES` (`32`),
`ARCHIVE_RESOURCE_ALIGNMENT` (`8`), `FILE_HEADER_BYTES` (`12`) and
`TABLE_OF_CONTENTS_ENTRY_BYTES` (`48`).

The table of contents is self-describing in the narrow sense the reader
enforces: each entry stores the resource name and its byte span, and the
reader requires the names in the fixed canonical order rather than searching
for them.

| Resource | Record width | Payload |
|---|---|---|
| `nodes` | 8 | quantized coordinate records |
| `edges` | 20 | directed edges |
| `lane_guidances` | 8 | lane guidance records |
| `instruction_strings` | blob | concatenated UTF-8 instruction text |
| `instruction_lookup_indices` | 4 | byte offsets into `instruction_strings`, one per turn restriction |
| `turn_restrictions` | 16 | restricted turns |

The list is `CANONICAL_RESOURCE_NAMES` in `archive.rs`. The entry count is
fixed at `RESOURCE_COUNT`, so a new resource can only arrive through a format
version bump; readers stay strict instead of silently ignoring unknown
sections.

A node record stores latitude and longitude as fixed-point `u32` values at
1e-7 degree resolution, offset by 90 and 180 degrees so the whole globe stays
non-negative (`COORDINATE_QUANTUM_PER_DEGREE`, `LATITUDE_OFFSET_DEGREES`,
`LONGITUDE_OFFSET_DEGREES`, and `NodeRecord::from_geographic_degrees` in
`archive.rs`).

An edge record stores the source and target node index, the distance in
meters, the duration in milliseconds, and one packed `encoded_value` column:

```text
bits  0..3    road class (0 motorway, 1 trunk, 2 primary, 3 secondary,
              4 tertiary, 5 unclassified, 6 residential, 7 service)
bits  3..8    speed in 5 km/h cells (SPEED_QUANTUM_KILOMETERS_PER_HOUR)
bit   8       forward access (along the OpenStreetMap way node order)
bit   9       backward access (against that order)
bits 10..32   reserved, must be zero
```

The bit offsets are named constants in `archive.rs`, from
`ROAD_CLASS_BIT_OFFSET` through `ACCESS_BACKWARD_BIT_OFFSET`, and
`SPEED_CELL_LIMIT` caps the speed column at 155 km/h.
Packing the three columns keeps the routing hot path branch-free.

`instruction_strings` holds one phrase per recorded turn restriction, in
restriction order. `instruction_lookup_indices` holds one entry per
restriction: a byte offset into that blob, or `INSTRUCTION_ABSENT_INDEX`
(`u32::MAX`) when no text exists. `instruction_text` resolves an entry to the
text between its offset and the next non-absent offset.

Lane guidance records carry an edge index and a lane count. The resource has
its slot in the canonical order, but the compiler emits no records; see
[Lane guidance](#lane-guidance) for why.

## Why the format is not flatdata

The `flatdata` crate was evaluated first and rejected for one reason: its Rust
generation is not hermetic. A `build.rs` must call `flatdata::generate`,
which shells out to `python3` and `pip` and installs a pinned generator from
PyPI before it runs, so every workspace build would depend on the network and
a third-party Python toolchain.

This project instead pins its own minimal self-describing container with the
same resource names and validation semantics. `ARCHIVE_FORMAT_VERSION` is the
seam for a byte-compatible migration should a hermetic Rust generator appear.

## Compile pipeline

1. **Collect.** One sequential pass over the PBF records node positions,
   drivable ways (OSM id, ordered node references, tags) and turn-restriction
   relation candidates (`crates/rustic-maps-offline-graph-compiler/src/compiler.rs`).
2. **Profile.** The tag rules in `profile.rs` decide which ways become edges.
   `highway` selects the road class and default free-flow speed; the drivable
   values are `motorway`, `trunk`, `primary`, `secondary`, `tertiary`,
   `unclassified`, `residential` and `service`, while `living_street`
   borrows residential and every `*_link` inherits its parent class. A plain
   integer `maxspeed` between 5 and 155 km/h overrides the class default;
   richer values fall back to it. `access=no` and `access=private` drop the
   way. `oneway=yes`, `1` and `true` keep only the node-order direction,
   `-1` and `reverse` keep only the reverse direction, and any other value
   keeps both.
3. **Build topology.** Only nodes referenced by a surviving way enter the
   graph, in ascending OpenStreetMap node id order. Each way segment emits one
   directed edge per allowed direction. The distance is the haversine distance
   between the two quantized node records, and the duration divides that
   distance by the stored, quantized speed, so the archive re-derives its own
   durations exactly.
4. **Resolve turn restrictions.** A `type=restriction` relation with a
   `from` way, a `via` node and a `to` way maps onto one of five kinds:
   `no_left_turn`, `no_right_turn`, `no_u_turn`, `no_straight_on` and
   `only_straight_on`. The compiler resolves each candidate to edge and node
   indices and skips candidates it cannot resolve, instead of failing the
   compile.
5. **Encode.** The archive builder validates every cross-reference and writes
   the bytes (`encode_deterministic` in `archive.rs`).

### Lane guidance

The compiler emits no lane guidance records, and that is a deliberate
refusal. OpenStreetMap `turn:lanes` tags describe which maneuvers each lane
permits (for example `through|through|right`), while the archive's
`LaneGuidanceRecord` stores a lane count per edge. A counts-only record can
represent neither the per-lane maneuvers nor a recommended lane, so producing
records from today's tags would drop information or invent it. The resource
keeps its slot so the layout stays byte-stable for a future lane-geometry
extension.

## Determinism

Two compiles of the same extract produce byte-identical archives. The ordering
rules that make this true are fixed in the pipeline and documented in the
compiler's module docs:

- surviving nodes keep ascending OpenStreetMap node id order;
- edges sort by (source node index, target node index, originating way id);
- turn restrictions sort by (from edge index, via node index, to edge index),
  and instruction strings follow that same restriction order;
- the writer emits resources in the canonical order and writes every
  alignment gap as zero bytes.

## Committed fixtures

Two fixtures are committed beside the compiler, and always-on tests guard
both against drift:

- `fixtures/osm/routing_grid.osm.pbf` is the small routing-grid extract in
  which three ways survive, one way is dropped by `access=private`, one
  restriction applies, and eight nodes become six. Its test compares the
  committed bytes with the canonical generator's output.
- `fixtures/rmog/routing_grid.rmog` is that extract compiled once by the
  compiler binary. Its test regenerates the archive in process, compares the
  bytes, and then re-validates them through `from_bytes`, which is the same
  loader the browser uses.

Set `MAP_OFFLINE_GRAPH_COMPILER_REGENERATE_FIXTURE` or
`MAP_OFFLINE_GRAPH_COMPILER_REGENERATE_ARCHIVE_FIXTURE` to rewrite a fixture
from the canonical source. Both switches live in the compiler's
`tests/common/mod.rs`.

The round-trip test pins the archive's observable content: node order and
coordinates, edge endpoints and canonical order, one-way direction, road
classes, speeds, durations, the restriction record, and the instruction
string with its lookup index. A separate test proves two compiles agree byte
for byte.

## Reading and validation

`OfflineRoutingNetworkGraph::from_bytes` parses and fully validates an
archive before returning it. Corruption never reaches the routing engine and
never causes undefined behavior. Every failure is one of three typed
reasons, carried by `SDKError::OfflineRoutingFailure` and defined in
`crates/geospatial-types/src/error.rs`:

| Reason | Raised when |
|---|---|
| `ArchiveMalformed` | the input is shorter than the header, the magic is wrong, the entry count is not the canonical six, a name is wrong or out of order, an offset is misaligned or below the previous resource's end, a span exceeds the file, a payload length is not a whole number of records, packed enum values are unknown, reserved bits are set, or the instruction blob is not UTF-8 |
| `VersionUnsupported` | the magic matches but the format version is not this crate's |
| `IndexOutOfRange` | an edge, restriction or lane-guidance record references a node or edge outside the archive, a lookup offset lands past the blob or off a UTF-8 character boundary, or the lookup count differs from the restriction count |

The builder enforces the same cross-reference checks in
`encode_deterministic`, so an out-of-range graph fails at compile time with
the same typed vocabulary instead of producing bytes the reader would reject.

### Native memory-mapped path

On native targets, `MemoryMappedArchive::open` maps the file and validates
the file-level alignment invariants before handing out bytes:

- the mapped region covers at least the 12-byte header;
- its length is a multiple of `ARCHIVE_RESOURCE_ALIGNMENT`, because the
  writer always pads the archive to that boundary;
- the mapped base address honors the same alignment, which keeps the
  table-of-contents `u64` reads naturally aligned. Page-granular mapping
  provides this, and the check validates it rather than assuming it.

The mapped bytes then go through the same `from_bytes` validation as
in-memory bytes. Both load paths share one implementation, so a route from
mapped file bytes and a route from in-memory bytes of the same archive are
identical. Missing, unreadable or unmappable files surface as
`ArchiveMalformed`: the typed vocabulary has no I/O cause, and an archive that
cannot be mapped is unusable in the same way a malformed one is.
`offline_routing_network_graph_from_mapped_file` combines the mapping and
validation in one call.

## Wasm path

The browser has no `mmap`, so the wasm target loads the graph from in-memory
bytes only. `OfflineGraphSource::Memory` takes the archive as a byte vector
and is the only source on wasm; an `OfflineGraphSource::FilePath` load is
rejected with a typed `ProviderRequestRejected` message that names the memory
source to use instead
(`crates/rustic-maps-core/src/offline_routing.rs`).

The committed compiled fixture serves same-origin from the playground dev
server, and the wasm bindings expose `offlineRouteFromArchiveBytes`, which
takes the archive bytes plus a start and end coordinate, validates the bytes
through the same reader and answers a route. It is the browser half of the
contract's `offline_route_from_archive_bytes` free helper: both surfaces
delegate to the same `rustic-maps-core` implementation, so the native and wasm
answers cannot drift.

## Routing consumer

`ArchiveOfflineRoutingProvider` in `rustic-maps-core` implements the
`OfflineRoutingProvider` trait:

- **Load.** It accepts either a file path (native, memory-mapped) or bytes
  (every target), validates the archive, and builds a half-edge adjacency
  plus the turn-restriction rules once. A failed load leaves any previously
  loaded graph in place.
- **Snap.** It snaps each request point to the nearest graph node within a
  named 50 m tolerance (`NODE_SNAP_TOLERANCE_METERS`) and rejects coordinates
  beyond it.
- **Search.** It runs A\* with a haversine heuristic that stays admissible for
  both weights: straight-line distance for the `Shortest` strategy, and that
  distance divided by the fastest storable edge speed for `Fastest`.
  `dijkstra_reference_search` is the same search with a zero heuristic. It is
  a benchmark baseline only and nothing routes through it.
- **Restrictions.** A restriction is evaluated against the settled path's
  incoming edge, which is exact unless a node can be reached on several
  best-cost paths through differently restricted incoming edges, in which case
  it over-restricts rather than understating.
- **Maneuvers.** Step boundaries come from the geometry: a junction whose
  signed bearing change leaves the straight band opens a turn step, with the
  band constants `STRAIGHT_ON_BAND_DEGREES`, `SLIGHT_TURN_BAND_DEGREES`,
  `STANDARD_TURN_BAND_DEGREES` and `SHARP_TURN_BAND_DEGREES` classifying
  slight, standard, sharp and U-turn maneuvers. A road-class or speed change
  without a turn opens a `Continue` step. The archive carries no street names,
  so step texts stay name-free (`"Turn right"`, `"U-turn"`) and are
  end-anchored on the banner while the maneuver type stays anchored at the
  step's opening junction.

`OfflineFallbackRoutingProvider` wraps an online routing provider with this
one. A successful online answer passes through untouched. When the online
request fails for a request with at least two waypoints, it calculates each
consecutive waypoint pair offline, joins the legs, sums distance and duration,
and unions the bounding boxes.

## Verification gates

The offline graph suite runs without network access:

- the committed extract and compiled archive match their canonical
  generators byte for byte;
- the compiler round trip asserts every archived field;
- two compiles of one extract are byte-identical;
- truncation, bad magic, a corrupted version, set reserved bits and
  out-of-range references each fail with the expected typed reason;
- mapped-file and in-memory loads of the same archive answer identical
  routes;
- lane-tagged ways compile to edges without fabricating lane guidance;
- the fixture's right-angle streets materialize the expected derived turns.
