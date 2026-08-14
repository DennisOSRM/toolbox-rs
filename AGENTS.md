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

## Commits

Write the subject as a plain sentence saying what the change does — "Read a
single rkyv value from a file", not a conventional-commit prefix. The body
says why the change is worth making and what it rules out; wrap it at 72
columns.

Do not sign the commit as an agent. No `Co-authored-by:` line, no
`Claude-Session:` trailer, no tool footer — the commit message is about the
change alone.

## Pull requests

Say in the PR description that it was written with agent assistance, and link
the session there. That is the place for it, and it keeps the history clean
while the provenance stays where a reviewer looks for it.

Otherwise the description carries the reasoning: what the change does, why,
what it leaves for later, and which PR it is stacked on if any.

Releases are cut by release-plz from `main`; do not bump the version in
`Cargo.toml` by hand.
