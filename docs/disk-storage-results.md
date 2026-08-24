# Disk-based MLD storage: what the measurements say

All on europe.ptv: 18,010,173 nodes, 42,188,664 arcs, 6 levels, the boundary
directory, cells numbered in key order and nodes by cell path. 623,435 cells,
66,267,046 table entries. Four physical cores.

Every number here is measured, and every table read back out of a store was
compared with what went into it. Where a measurement contradicted the plan the
measurement is what is written down.

## The store

| | | |
|---|---|---|
| tables as four-byte entries | 252.8 MiB | 100% |
| packed at one width per table | 111.0 MiB | 44% |
| framing beside them | 2.5 MiB | 1% |
| a block file, lz4 | **107.8 MiB** | **43%** |
| the cell tree, shipped whole | 19.5 MiB | |

## Encoding: one width per table, no row minimum

Four schemes counted over every table:

| | | |
|---|---|---|
| the least of a row, then two bytes an entry | 161.1 MiB | 64% |
| the least, then a width per row | 126.5 MiB | 50% |
| the least, then a width per table | 130.7 MiB | 52% |
| **a width per table and no least** | **112.5 MiB** | **44%** |

Two findings, both against the plan:

**Two-byte deltas are the wrong shape for this data.** They want a row whose
values sit close together, and the coarse levels do not: at the top level only
8.4% of rows fit, 2.88 million entries escape, and the result is *larger than
raw* — 13.1 MiB against 9.1.

**The least of a row is always nought.** A row holds the distance from a node
to every border node of its cell, itself among them, and that one is nought.
Over all 4,783,400 rows not one had a smallest reachable distance above nought.
Storing it costs 18.2 MiB and buys nothing.

A width per row is 14 MiB smaller in the entries and was still not taken: rows
are read at random, so it wants four bytes a row saying where each begins,
which gives the 14 back and costs 4 more.

## Codecs: worth 9 per cent, so a latency choice

| codec | on disk | share of raw | to read the whole store |
|---|---|---|---|
| stored | 113.3 MiB | 44.8% | 16.2ms |
| **lz4** | **107.8 MiB** | **42.7%** | **19.9ms** |
| deflate 6 | 103.6 MiB | 41.0% | 336.3ms |
| zstd 3 | 103.9 MiB | 41.1% | 130.2ms |
| zstd 19 | 102.8 MiB | 40.7% | 126.5ms |

The best saves nine per cent over storing the blocks as they are, and zstd at
19 beats zstd at 3 by one megabyte in a hundred and three. That is not the
codecs failing: the entries are already packed to the narrowest width that
holds them, so most of what a compressor looks for was taken out before it
arrived. Compression is the second bite and the first was the larger.

**Recommendation: lz4.** Five per cent for about four milliseconds over a
straight copy of the whole store, against nine per cent for a hundred and ten.
The codec byte a block is what lets a build that would rather ship ten
megabytes less use zstd for the levels where a fault is rare.

## Block size: 64 KiB, not 4 MiB

At a budget of 150 MiB, medians over 4,800 pairs:

| block target | blocks | on disk | median | p95 | reads a query |
|---|---|---|---|---|---|
| 4096 KiB | 66 | 107.8 MiB | 2118µs | 9061µs | 1.9 |
| 512 KiB | 491 | 108.0 MiB | 635µs | 7069µs | 2.2 |
| **64 KiB** | **3,633** | **108.2 MiB** | **437µs** | **6829µs** | 2.5 |
| 16 KiB | 13,217 | 108.8 MiB | 507µs | 6957µs | 3.0 |
| 4 KiB | 45,088 | 110.3 MiB | 752µs | 7321µs | 4.2 |

The plan's four megabytes is **4.8 times worse than the best** and its smallest
suggested size, one megabyte, is still two to three times worse. A 4 MiB block
is read, decoded and parsed to hand back one table of a few hundred bytes.

Below 64 KiB it turns: reads a query climb faster than each read gets cheaper,
and the file grows, since a smaller block gives lz4 less to work with — 108.2
MiB at 64 KiB against 110.3 at 4 KiB.

**Recommendation: 64 KiB**, and the curve is shallow between 64 and 512, so a
build with a reason to prefer fewer, larger blocks loses little at 512.

## Memory budget: which levels to hold

Levels cost, unpacked, from the coarsest down: 18.3, 40.0, 61.0, 93.1, 125.0
and 186.4 MiB, so from the top 18.3, 58.2, 119.2, 212.4, 337.4 and 523.8.

At 4 MiB blocks, 4,800 pairs, half the budget available to hold levels:

| budget | held | held bytes | cache | open | median | p95 | reads a query | hit rate |
|---|---|---|---|---|---|---|---|---|
| 75 MiB | L5 | 18.3 MiB | 56.7 MiB | 5.6s | 3397µs | 12229µs | 3.8 | 95.3% |
| 150 MiB | L4+ | 58.2 MiB | 91.8 MiB | 5.4s | 2173µs | 8632µs | 1.9 | 96.9% |
| 300 MiB | L3+ | 119.2 MiB | 180.8 MiB | 5.5s | 2192µs | 8688µs | 1.6 | 93.8% |
| 700 MiB | L1+ | 337.4 MiB | 362.6 MiB | 5.8s | 953µs | 6873µs | 0.5 | 82.2% |

**Every answer at every budget matched the same search over the cells in
memory**, 4,800 pairs each.

Holding the two coarsest levels is most of what there is to gain: 75 to 150
MiB halves the median, and 150 to 300 changes nothing worth naming. Going to
700 halves it again, but that is 337 MiB of held tables to save a millisecond,
and by then the hit rate has *fallen* to 82% because the cache is holding fine
tables that a query touches once.

Opening a store is about 5.5s at any budget, nearly all of it reading and
unpacking the levels to be held.

## Against the same query in memory

The in-memory search is 63µs at the median under the border-first numbering
and 76µs under cell-path. The paged store is **437µs at 64 KiB blocks and a
150 MiB budget**, so between five and seven times slower.

That is the price of the whole exercise and it is worth stating plainly: a
device holding a hundred and eight megabytes answers in about four hundred
microseconds where a server holding one and a half gigabytes answers in
seventy.

## What has not been measured

- **Cold page cache.** Every one of these runs read a file the operating
  system already had. A device faulting from flash will be slower, and the
  block-size curve will move: the smaller the block, the more the fixed cost of
  a read matters.
- **Regions.** Nothing here is sparse; the whole of Europe was present.
- **The pinned section is not mmapped.** Held levels are read and unpacked at
  open, which is the 5.5s. Mapping them would trade that for a slower first
  touch.
- **zstd with a trained dictionary**, which the plan wanted for the small-frame
  case. With 64 KiB blocks there is a case for it that there was not at 4 MiB.
