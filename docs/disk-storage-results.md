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
| 4096 KiB | 66 | 107.8 MiB | 1197µs | 7055µs | 1.3 |
| 512 KiB | 491 | 108.0 MiB | 262µs | 5957µs | 1.6 |
| **64 KiB** | **3,633** | **108.2 MiB** | **130µs** | **5669µs** | 1.7 |
| 16 KiB | 13,217 | 108.8 MiB | 196µs | 6182µs | 2.0 |
| 4 KiB | 45,088 | 110.3 MiB | 320µs | 6314µs | 2.7 |

The plan's four megabytes is **9.2 times worse than the best**, and its smallest
suggested size, one megabyte, is still two to three times worse. A 4 MiB block
is read, decoded and parsed to hand back one table of a few hundred bytes.

Below 64 KiB it turns: reads a query climb faster than each read gets cheaper,
and the file grows, since a smaller block gives lz4 less to work with — 108.2
MiB at 64 KiB against 110.3 at 4 KiB.

**Recommendation: 64 KiB.** The curve is shallow between 64 and 512, so a build
with a reason to prefer fewer, larger blocks loses little at 512, and steep
above that.

## Memory budget: which levels to hold

Levels cost, unpacked, from the coarsest down: 18.3, 40.0, 61.0, 93.1, 125.0
and 186.4 MiB, so from the top 18.3, 58.2, 119.2, 212.4, 337.4 and 523.8.

At 64 KiB blocks, 4,800 pairs, half the budget available to hold levels:

| budget | held | held bytes | cache | open | median | p95 | reads a query | hit rate |
|---|---|---|---|---|---|---|---|---|
| 75 MiB | L5 | 18.3 MiB | 56.7 MiB | 5.6s | 211µs | 6171µs | 5.2 | 94.9% |
| **150 MiB** | **L4+** | **58.2 MiB** | **91.8 MiB** | 5.4s | **130µs** | 5680µs | 1.7 | 97.1% |
| 300 MiB | L3+ | 119.2 MiB | 180.8 MiB | 5.4s | 124µs | 5598µs | 1.5 | 93.9% |
| 700 MiB | L1+ | 337.4 MiB | 362.6 MiB | 5.5s | 95µs | 5508µs | 0.6 | 83.7% |

**Holding the two coarsest levels is nearly all of what there is to gain.** 75
to 150 MiB cuts the median by 38% and reads a query from 5.2 to 1.7; 150 to 300
buys 5%. Going to 700 buys another 27%, but for 337 MiB of held tables, and by
then the hit rate has *fallen* to 84% because the cache is holding fine tables
a query touches once.

Opening a store is about 5.5s at any budget, nearly all of it reading and
unpacking the levels to be held.

## Against the same query in memory

Both engines over the same 4,800 pairs, 64 KiB blocks, a 150 MiB budget, the
cells in memory fully customized first. Medians by Dijkstra rank:

| rank | in memory | paged | |
|---|---|---|---|
| 2^4 | 3.1µs | 3.3µs | 1.07× |
| 2^6 | 7.5µs | 7.8µs | 1.05× |
| 2^8 | 16.1µs | 34.0µs | 2.12× |
| 2^10 | 32.7µs | 71.1µs | **2.17×** |
| 2^12 | 64.5µs | 115.2µs | 1.79× |
| 2^14 | 140.2µs | 211.3µs | 1.51× |
| 2^16 | 289.7µs | 391.0µs | 1.35× |
| 2^18 | 584.1µs | 715.5µs | 1.23× |
| 2^20 | 1331.8µs | 1401.9µs | 1.05× |
| 2^22 | 2981.6µs | 2933.4µs | 0.98× |
| 2^24 | 7008.6µs | 6305.3µs | 0.90× |
| **all** | **75.2µs** | **137.1µs** | **1.82×** |

**A store of 108 MiB answers within a factor of two of a customization holding
about a gigabyte and a half.**

The curve has a shape worth reading. Below rank 2^7 there is almost nothing in
it: a short search stays in the cells around its ends, which are cached after
the first touch. It peaks at 2^10, where a search has started reaching cells it
has not seen and has not yet reached the levels that are held outright. Above
2^19 it closes, and at the very top the paged store is *faster* — the held
levels are one run of memory apiece, where the customization reaches through a
vector of boxes per cell.

**Every one of the 4,800 answers equals the same query run on the original,
un-renumbered instance**, through two renumberings, the packing, lz4 and the
cache.

## A measurement that was wrong, and is corrected here

Everything above was first measured with the wrong pairs. The pair file holds
node ids of the instance they were drawn on, and the store is built on a
renumbered one, so the ids named different nodes: of 4,800 pairs, **none** gave
the same distance on both instances, and the rank beside each belonged to
somebody else.

It was caught by a number that did not fit — the in-memory engine measured
341µs where the same engine on the same pairs measures 75µs — and confirmed by
comparing the two instances' answers pair by pair. The pairs are now put
through the numbering that `renumber` wrote.

The conclusions held: 64 KiB was the best block size before and after, and the
two coarsest levels were the ones worth holding before and after. The numbers
did not: the paged store was reported at five to seven times the in-memory
query and is under two.

## How much memory to give it

`paged_query` was run over 600 pairs (every eighth of the 4,800, so the rank
spread is kept) at budgets from 1 to 256 MiB, 64 KiB blocks, `pinned_share`
0.5. `scripts/budget_plot.R` draws it from the summary `TOOLBOX_SUMMARY`
writes; the data is `docs/plots/budgets.csv` and the picture
`docs/plots/budgets.png`.

| budget | held | for cache | median | 95th | reads/query | hit rate |
|--------|------|-----------|--------|------|-------------|----------|
| 1 MiB | — | 1.0 MiB | 6.03× | 16,300× | 43.9 | 46.7% |
| 2 MiB | — | 2.0 MiB | 4.58× | 16,700× | 42.0 | 56.6% |
| 4 MiB | — | 4.0 MiB | 4.25× | 11,100× | 35.8 | 73.5% |
| 8 MiB | — | 8.0 MiB | 4.41× | 2,200× | 20.0 | 89.6% |
| 16 MiB | — | 16.0 MiB | 3.85× | 225× | 10.0 | 94.5% |
| 32 MiB | — | 32.0 MiB | 3.57× | 125× | 8.3 | 95.0% |
| 64 MiB | L5+ | 45.7 MiB | 2.99× | 92.8× | 5.9 | 93.8% |
| 80 MiB | L5+ | 61.7 MiB | 2.77× | 88.1× | 4.9 | 94.7% |
| 96 MiB | L5+ | 77.7 MiB | 2.42× | 85.7× | 3.8 | 95.9% |
| **112 MiB** | L5+ | 93.7 MiB | **1.27×** | 83.8× | **0.50** | 99.5% |
| 128 MiB | L4+ | 69.8 MiB | 1.32× | 81.3× | 0.48 | 99.1% |
| 192 MiB | L4+ | 133.8 MiB | 1.48× | 80.5× | 0.48 | 99.1% |
| 256 MiB | L3+ | 136.8 MiB | 1.27× | 81.7× | 0.41 | 98.1% |

The curve is a cliff, not a slope. From 1 to 96 MiB the median only walks from
6.0× to 2.4×, because a query's cells are scattered across the levels and a
cache that cannot hold the working set evicts what the next query wants. At
112 MiB the working set fits: reads fall from 3.8 a query to 0.5 and the
median lands at 1.27×. Above that nothing more is bought — 256 MiB is 1.27×,
the same, and the spread between 112, 128, 192 and 256 is run-to-run noise.

**112 MiB is the size to pick**, and 128 MiB if a round number is wanted. Below
the cliff the choice hardly matters: 32 MiB and 4 MiB are both about 4× and
neither is close to the knee.

Two things the medians hide:

- **The tail is where a small budget hurts.** At 1 MiB the 95th is 1.22
  *seconds* against 75µs in memory. It is under 10ms from 32 MiB up, and the
  95th flattens at ~85× well before the median does — the 95th is the block
  reads a long query cannot avoid, and 16 MiB already removes most of them.
- **Holding levels is not what wins.** 112 MiB holds only level 5 (18.3 MiB)
  and is as fast as 128 MiB, which holds levels 4 and 5 (58.2 MiB) and has
  *less* cache for it. Cache size is the parameter; the held levels ride along.
  A second sweep with `pinned_share` 0 confirmed it: at every budget where a
  level was actually held, the cache-only run was within noise.

The saturation point depends on how many distinct queries share the cache. The
same sweep over 120 pairs put the cliff at 128 MiB and reported 1.04× there,
because a fifth of the queries have a fifth of the working set. A server
answering a wider spread than 600 pairs should expect the cliff further right.

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
  case. At 64 KiB blocks there is a case for it that there was not at 4 MiB.
