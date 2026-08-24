# Disk-based MLD storage: Phase 0 findings

Reading only, no code written. Answers the four Phase 0 questions and records
what the plan assumes that the repository does not currently support.

## 1. What chipper's assembly phase produces

`assembly::assemble_connected(&CellGraph, cell_of_node, sizes) -> LevelDirectory`
(`src/assembly.rs:422`).

**It merges.** Per level it runs `agglomerate` (greedy merge of cells joined by
an arc, up to a size bound), then `refine` (8 rounds moving cells between
neighbours to even the sizes out), then `contract` to build the next level's
cell graph. `fragments()` first splits any bisection cell that fell into
disconnected pieces, because a cell a search must cross without leaving cannot
be in two pieces.

So **the assembled tree has variable fan-out**, decided by the size bounds
passed as `--level-sizes`. Nothing about it is binary.

**The output is the full tree, not a leaf assignment.**

```rust
pub struct LevelDirectory {
    base: Vec<CellId>,          // the level-0 cell of each node
    parents: Vec<Vec<CellId>>,  // per level, the parent cell of each cell below
}
```

That is a parent array per level. Children are derivable by inverting
`parents_on_level(level)`, which `Customization::level` already does to build
`built_from`.

**It is already persisted.** `chipper::serialize::write_level_directory`
(`src/chipper/bin/serialize.rs:114`) writes it with rkyv, and every consumer in
the repo reads it back with `io::read_from_file`. Milestone 1 as written —
"serialize the assembled partition tree" — is therefore mostly done. What is
missing is the *derived* material the plan lists beside it: child offsets, per
level cut marks, per cell boundary node counts, and cell bounding boxes. None
of those are stored today.

**Key width per level is already computed and is not one bit per level.**
`PackedPartition::of` (`src/packed_partition.rs:63`) gives each level
`bits_for(directory.cells_on_level(level))` bits. On europe.ptv the six levels
want 76 bits in total. The plan's warning is correct and the arithmetic for it
already exists; `begins_at[]` in that function is exactly the per-level key
layout the plan wants versioned into the skeleton header.

## 2. Inventory

### Reusable as-is

| what | where | note |
|---|---|---|
| packed cell-path key | `packed_partition.rs` | see below — this is the key format |
| assembled tree | `level_directory.rs` | rkyv, persisted by chipper |
| bulk-loaded R-tree | `r_tree.rs` | already packed, by z-order |
| LRU | `lru.rs` | `push` returns the evicted entry, `pop_lru` |
| byte-budgeted cache pattern | `path_unpacking.rs` | `Unpacker` already does bytes-not-entries over that LRU |
| correctness oracle | `src/sound/bin` | checks a level's tables against plain Dijkstra on the base graph |
| k-way merge, Huffman, z-order, bbox, bin packing | `k_way_merge_iterator.rs`, `huffman_code.rs`, `space_filling_curve.rs`, `bounding_box.rs`, `bin_pack.rs` | present |

**`PackedPartition` is the packed cell-path key the plan asks for.** One u128
per node, levels laid down finest-first so the coarse levels occupy the high
bits. Ordering words therefore orders lexicographically by cell path from the
root, which means **a subtree is already a contiguous range of keys**. It also
provides `cell_in(word, level)`, `same_cell_at`, `query_level`, and
`highest_different_level` as an xor plus a leading-zero count.

What is missing is a *per-cell* key type. The word is per node; the block map
needs the key of a cell, which is the node word with the levels below it
masked off. That is a small new type over the existing `begins_at[]` layout,
not a new design.

### Present but not what the plan assumes

**`PartitionID` is the wrong tree.** `src/partition_id.rs` is a binary tree id
— `left_child`, `right_child`, `parent_at_level`, level packed into a u32. It
belongs to the inertial-flow bisection, not to the assembled tree. The plan
lists it under "reusable" and also states that no format consumer may depend
on bisection internals; those two are in conflict. Keys must come from
`PackedPartition` over the assembled `LevelDirectory`, and `PartitionID`
should not appear in the format at all.

**The R-tree is not serializable.** `RTree` has no `Archive` derive, so storing
one inside a block is new work. Bulk loading, however, already exists: it sorts
by z-order and packs bottom up. The plan's "extend it with bulk loading (STR or
Hilbert) if not present" can be dropped to "make it serializable"; whether
z-order packs well enough to keep is a measurement, not a rewrite.

**`io.rs` reads and writes whole files only.** `read_from_file`,
`write_to_file`, `read_vec_from_file`, `write_vec_to_file`, all rkyv. There is
no positional read and no mapping. Both are new.

**Four dependencies are new**: `memmap2` and the three codecs (`flate2`,
`zstd`, `lz4`). None are in `Cargo.toml` today.

## 3. Where `CellDistances` comes from

Produced by `Customization::tabulate` (`src/customization.rs`), held in
`tabulated: Vec<Vec<OnceLock<Box<CellDistances>>>>` indexed by level then cell.
Customization is on demand: nothing is tabulated until `distances_of(level,
cell)` is called. `examples/customize.rs` drives it eagerly, which is what a
packer would do — 5.86s for europe.ptv on eight threads.

```rust
pub struct CellDistances {
    pub border_nodes: Vec<u32>,          // b entries, ascending
    matrix: Vec<u32>,                    // b * b, row major
    transposed: Vec<u32>,                // b * b, the same table transposed
    place_of: FxHashMap<NodeID, usize>,  // b entries
}
```

Two things the plan should account for:

**The transpose is a full second copy.** The 505 MiB of "raw distance volume"
is `matrix` plus `transposed`, not one table. It exists because a backward
search wants a column and a column is a strided read. On disk only one is
needed; whether the runtime rebuilds the other on block load, stores both, or
transposes per block are three different answers with different sizes and
different decode costs, and the 30-40% compression target should say which
baseline it is against.

**`place_of` is most of the level-0 overhead the plan wants to eliminate.** A
level-0 cell has about 6 border nodes: the matrix is ~144 B while the map
reserves ~136 B and the four `Vec` headers plus the `Box` cost ~144 B more.
That is the 42%. It disappears on disk as the plan says, and it can also
disappear in memory: `border_nodes` is already sorted ascending, so `place_of`
is a binary search over it. That is a separate, smaller change worth doing on
its own, and it should be measured against query latency before being adopted,
since the map is on the query hot path and was made a map deliberately.

## 4. The one real conflict: node ordering

Design decision 4 says node ids are assigned "in DFS order of the assembled
tree; boundary nodes first within each cell", and that "every subtree is a
contiguous ID range".

`NodeOrdering::of` (`src/node_ordering.rs:67`) already renumbers, and it does
something different:

```rust
to_old.sort_unstable_by_key(|&node| partition.word(node));      // by cell path
to_old.sort_by_key(|&node| Reverse(border_level[node]));        // then, stably
```

The border-level sort is applied last and to the whole array, so **border level
is the primary key**. Every level-5 border node in the graph comes first, then
every level-4 border node, and so on, with interior nodes last. Within each of
those groups the cells stay side by side.

The consequence for the plan: **a cell's nodes are split across up to
`levels + 1` disjoint ranges, and no subtree is a contiguous id range.** This
is OSRM's `makePermutation` and it is deliberate — an overlay search touches a
few hundred thousand border nodes out of eighteen million, and front-loading
them puts every per-node array's working set in its first few megabytes. It was
adopted because it measured faster.

The two orders want opposite things:

- *border-major* (current): good cache locality for the overlay query.
- *cell-major* (plan): contiguous subtree id ranges, which is what lets a block
  hold a subtree's nodes as a range and what makes "absent key range means not
  downloaded" work at node granularity.

Both properties are available within a cell by swapping the two sort keys —
cell path primary, border level secondary — which keeps subtrees contiguous and
still puts a cell's border nodes at the front of that cell. What it gives up is
the global front-loading.

### Measured

Both orderings are now selectable (`TOOLBOX_CELL_MAJOR=1`), so the question was
answered rather than argued.

**Does cell-major give the invariant?** `examples/ordering_check.rs` asks, for
every cell of every level, whether the numbers its nodes were given form one
run. europe.ptv:

| level | cells | in one run, border-major | in one run, cell-major |
|---|---|---|---|
| 0 | 497,965 | 86 (0.0%) | 497,965 (100%) |
| 1 | 98,375 | 0 | 98,375 (100%) |
| 2 | 24,384 | 0 | 24,384 (100%) |
| 3 | 2,440 | 0 | 2,440 (100%) |
| 4 | 245 | 0 | 245 (100%) |
| 5 | 26 | 0 | 26 (100%) |

Under the current order a cell is not merely broken up, it is spread over
almost the whole numbering: the worst cell at level 0 spans 18,002,197 numbers
to hold about thirty nodes. Under cell-major every cell at every level is
exactly one run. The invariant is available and it is free to obtain.

**What it costs.** `ranks time -e mld` over 4,800 pairs drawn across the rank
axis, two rounds, medians in microseconds:

| Dijkstra rank | no renumbering | border-major | cell-major | cell vs border |
|---|---|---|---|---|
| 2^4 | 3.8 | 3.5 | 3.0 | **-13%** |
| 2^6 | 9.2 | 8.3 | 7.2 | **-13%** |
| 2^8 | 19.3 | 16.7 | 15.8 | **-6%** |
| 2^9 | 27.3 | 22.8 | 22.6 | -1% |
| 2^10 | 38.5 | 30.6 | 31.2 | +2% |
| 2^12 | 74.5 | 57.2 | 65.5 | +14% |
| 2^14 | 159.4 | 110.8 | 143.0 | +29% |
| 2^16 | 323.6 | 212.2 | 300.7 | +42% |
| 2^18 | 644.0 | 414.4 | 600.0 | +45% |
| 2^20 | 1397.3 | 929.8 | 1348.3 | +45% |
| 2^24 | 7021.8 | 5245.8 | 6786.5 | +29% |
| **all** | **86.0** | **63.4** | **76.5** | **+21%** |

Renumbering at all is worth 26% against none; cell-major keeps 11 of those 26.

**The loss is entirely at long range, and short routes are faster.** Below
rank 2^9 cell-major wins by up to 13%, because a short search stays inside a
few cells and cell-major is exactly what puts a cell's nodes together. The
crossover is at 2^10. Above it the overlay walks border nodes of coarse cells
scattered across the graph, which is the case border-major exists for, and the
gap opens to 45%.

Building the numbering costs 1.30s against 1.15s, which is nothing.

### What this means for the design

The cost falls precisely on the levels the plan already intends to pin in
memory. Nothing on the paged path is worse under cell-major and most of it is
better. So option 3 is not a compromise, it is the shape of the answer: number
by cell path so that every block holds a run, and deal with the coarse-level
locality separately, where the data is resident anyway and a second numbering
or a compact overlay addressing costs a permutation at load rather than a
translation per lookup.

It is also worth saying that this 21% is the price in the *in-memory* engine.
On the device the same invariant decides whether a block can hold a node range
at all. A read path that faults and decompresses is not competing with 63
microseconds, and contiguity is likely to be worth more there than it costs
here. This measurement bounds the cost; it does not settle the trade on its
own.

Three ways out, in increasing cost:

1. **Cell-major everywhere**, and accept whatever the query loses. Cheap to
   try: the change is swapping two lines, and `renumber` and `ranks` already
   exist to measure it end to end. **Measure this before choosing anything
   else** — the loss may be small, and if it is, the format gets its invariant
   for free.
2. **Two numberings**: cell-major on disk, permuted to border-major when the
   pinned section is loaded. Costs a permutation at load and a translation
   wherever the two meet.
3. **Hybrid**: cell-major globally, border-major within the pinned levels
   (L3+), which are memory-resident anyway. Paged blocks still get border-first
   within each block, so the locality argument mostly survives where it is
   cheap to keep.

## Smaller notes

- `LevelDirectory::cells_on_level` scans for the maximum, so it is O(cells) per
  call. `PackedPartition::of` and the packer both call it per level; worth
  caching in the directory if the packer becomes hot.
- `LevelDirectory::cell_of(node, level)` walks up one parent array per level.
  Anything bulk should read `PackedPartition` instead — this was worth 245ms to
  125ms per level in the customization work.
- `sound` verifies one level at a time against the base graph and is the right
  oracle for Phases 1 to 3. It re-customizes as it goes, so it is slow on
  coarse levels (level 3 takes about 100s on europe.ptv).
- `CellDistances::bytes()` and `Level::bytes()` now exist, so the packer can
  report the before-and-after against the reference table directly.

## What Phase 1 actually needs, restated

1. Derived tree material that is not stored today: child offsets, level cut
   marks, per-cell boundary node counts, cell bounding boxes.
2. A per-cell key type over the existing `begins_at[]` bit layout, plus that
   layout in the skeleton header.
3. Node ordering: measured, see section 4. Cell-major gives the invariant at
   every level and costs 21% of median query time in the in-memory engine, all
   of it above rank 2^10. Recommendation: take cell-major for the format and
   handle coarse-level locality in the pinned section.

Everything else Phase 1 lists already exists in some form.
