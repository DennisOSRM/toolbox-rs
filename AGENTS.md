# AGENTS.md

A library of data structures and algorithms, plus four binaries (`chipper`,
`scaffold`, `graph_plier`, `solver`). Rust 2024 edition.

## Building and testing

```
cargo build
cargo test
cargo fmt
cargo clippy --all-features
```

CI runs all four on every pull request, on stable and nightly, on Linux,
macOS and Windows. Run them locally before pushing.

## Code

- One module per file in `src/`, exported from `src/lib.rs`. A binary lives in
  its own directory next to the modules it drives.
- Tests go in an inline `#[cfg(test)] mod tests` at the foot of the file they
  cover. Cover the edges, not only the happy path.
- No `unsafe`. It has been removed from this codebase on purpose; if a change
  seems to need it, say so instead of adding it.
- Benchmarks are criterion, under `benches/`.

## Routing changes

A change to the routing code — the searches over the cells, the customization,
the partition, the heaps they sit on — ships with a plot showing what it did.
That includes a change that turns out to do nothing: "no measurable effect" is a
result, and a plot is how it is shown rather than asserted. A speedup claimed in
a commit message and nowhere else is not a measurement.

```
ranks sample -g graph.toolbox -s 200 -o pairs.csv
ranks time -g graph.toolbox -i pairs.csv -e dijkstra -o d.csv
ranks time -g graph.toolbox -d levels.bin -i pairs.csv -e mld --warmup 4800 -o m.csv
cat d.csv <(tail -n +2 m.csv) > timings.csv
Rscript scripts/rank_plot.R timings.csv ranks.png
```

Give `--warmup` the number of pairs in the file. The overlay is worked out as it
is asked for, so without it the run measures customizing cells rather than
searching them, and the cost lands on whichever pairs happened to come first.

Hold the change against the code it replaces on the same machine in the same
sitting, and say which instance the numbers came from. A rank axis is the point:
a change that helps a query across a continent and hurts one across a town is a
trade to show, not an average to hide. Put the plot in the pull request beside
the reasoning.

## Commits

Write the subject as a plain sentence naming the algorithm or the structure
the change touches — "Read a single rkyv value from a file", "Add a packed
partition", "Use NodeID for the node containers in the graph searches". Not a
conventional-commit prefix.

Name what was done, not what it was worth. "Halve what a query costs" says
nothing about what was changed to halve it, and somebody later asking where
the packed partition came from, or which commit narrowed the arcs, will not
find it by reading the subjects. The measurement belongs in the body, where
there is room to say what it was measured against.

The body says why the change is worth making and what it rules out; wrap it
at 72 columns.

Do not sign the commit as an agent. No `Co-authored-by:` line, no
`Claude-Session:` trailer, no tool footer — the commit message is about the
change alone.

## Pull requests

Say in the PR description that it was written with agent assistance. Do not
link the session: a transcript holds the whole of what was said to get there,
which is more than provenance and not ours to publish. That the description
says how the change was made is what a reviewer needs; the reasoning behind
it belongs in the description itself, written out, rather than behind a link.

The title follows the same rule as a commit subject: name the algorithm or
the structure, not the effect. A pull request doing several things names the
ones a reviewer would search for rather than reaching for the sum of them.

Name the types, the traits and the algorithms, in the words the code uses. A
reader scanning a list of merged pull requests should be able to tell what
moved without opening any of them, and should be able to find this one again
by searching for the thing it changed.

The test is whether the title would still make sense to somebody who has never
read the description. "Draw everything from one cache" fails it: there is no
"everything" in the code and no reader can guess what a cache was drawn from.
"Add Pool, one byte-budgeted LRU shared by every kind of block" passes, because
`Pool` and LRU are searchable and the sentence says what was built.

Metaphor is for the body, where there is room to say what it stands for. A
title that reads well and identifies nothing is worse than a plain one.

Otherwise the description carries the reasoning: what the change does, why,
what it leaves for later, and which PR it is stacked on if any. Lead it with
the change itself — the signatures, the fields, the behaviour — and put the
motivation after; a reviewer opens a diff already knowing they want to see
what moved.

A hook formats and lints every change to a `.rs` file as it is made, whether
it was written by an editing tool or by a shell command -- a heredoc, a script,
`sed` -- since a good deal of the editing here is the latter and a hook that
only watches `Edit` watches half of it:
`.claude/hooks/rust-checks.sh` runs `cargo fmt --all` and then
`cargo clippy --all-targets`, and refuses the change where clippy complains, so
a lint is fixed while the reason for the code is still in mind rather than in a
sweep at the end. It says so when the formatter moved something, since what is
then on disk is not what was written. `TOOLBOX_SKIP_CLIPPY=1` keeps the
formatting and drops the lint, for a run of many small edits.

Releases are cut by release-plz from `main`; do not bump the version in
`Cargo.toml` by hand.
