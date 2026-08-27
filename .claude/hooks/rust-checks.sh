#!/usr/bin/env bash
#
# Formats and lints after a change to a Rust file.
#
# Run as a PostToolUse hook on Edit, Write and MultiEdit. It does the two
# things this repository asks of every change before it is committed, at the
# moment the change is made rather than at the end of a long session, so that a
# lint is fixed while the reason for the code is still in mind.
#
# What it does:
#   * nothing at all unless a .rs file was written
#   * cargo fmt --all, and says so where it moved something, because the file
#     on disk is then not the file the editor just wrote
#   * cargo clippy --all-targets, and refuses the change where it complains,
#     handing the complaints back to be fixed
#
# --all-targets and not --lib: the examples and the tests are where several
# real mistakes have been caught, and a lint that only reads the library is a
# lint that misses them.
#
# Set TOOLBOX_SKIP_CLIPPY=1 to keep the formatting and drop the lint, for a run
# of many small edits where waiting ten seconds each time is worse than waiting
# once at the end.

set -uo pipefail

payload=$(cat)
tool=$(printf '%s' "$payload" | jq -r '.tool_name // empty' 2>/dev/null)
file=$(printf '%s' "$payload" | jq -r '.tool_input.file_path // empty' 2>/dev/null)

root="${CLAUDE_PROJECT_DIR:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)}"
cd "$root" || exit 0

# An editing tool says which file it wrote. A shell command does not, and a
# good deal of the editing here is done by a shell command -- a heredoc, a
# script, sed -- so for those the question is asked of the working tree
# instead: has any Rust file changed since the last commit.
case "$tool" in
Bash)
    git status --porcelain -- '*.rs' 2>/dev/null | grep -q . || exit 0
    ;;
*)
    case "$file" in
    *.rs) ;;
    *) exit 0 ;;
    esac
    ;;
esac

notes=""

before=$(git diff --name-only 2>/dev/null)
if ! fmt=$(cargo fmt --all 2>&1); then
    printf 'cargo fmt could not run:\n%s\n' "$fmt" >&2
    exit 2
fi
after=$(git diff --name-only 2>/dev/null)
if [ "$before" != "$after" ]; then
    notes="cargo fmt reformatted files: what is on disk is not exactly what was written, so read before editing again."
fi

if [ "${TOOLBOX_SKIP_CLIPPY:-0}" != "1" ]; then
    if ! lint=$(cargo clippy --all-targets --quiet 2>&1); then
        printf 'cargo clippy is unhappy with this change:\n\n%s\n' "$lint" >&2
        exit 2
    fi
    if printf '%s' "$lint" | grep -qE '^(warning|error)'; then
        printf 'cargo clippy warns about this change:\n\n%s\n' "$lint" >&2
        exit 2
    fi
fi

if [ -n "$notes" ]; then
    printf '{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"%s"}}\n' "$notes"
fi
exit 0
