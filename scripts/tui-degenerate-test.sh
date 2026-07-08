#!/usr/bin/env bash
# Live gate: degenerate git states degrade gracefully (edge-hardening class 1).
#   Phase A — unborn repo (zero commits, no HEAD): the whole changeset is an untracked file.
#     Commit scope has no HEAD to anchor to, so the base must fall back to the empty tree —
#     the editor paints the added file fully green (the "+" add sign on every line), exactly
#     as on a committed repo. Before the fix it published an unresolvable "HEAD" and the file
#     opened undecorated (diff.lua's refresh bails when base:path won't resolve).
#   Phase B — detached HEAD (a valid HEAD, no branch name): a clean checkout parks on the
#     empty-state greeter, and a live edit re-lists in Changes. HEAD resolves here, so this
#     path was always graceful; the phase pins it so a base-resolution refactor can't regress
#     the detached case while fixing the unborn one.
set -uo pipefail
SOCK=rvdegen
source "$(dirname "$0")/tui-lib.sh"

# --- Phase A: unborn repo -------------------------------------------------------------------
# tui-lib inits $REPO with NO commit, so it is already unborn. One untracked file, root-level
# so the auto-open has a visible row without expanding a directory.
{ for i in $(seq 1 6); do printf 'UNBORNADD line %02d\n' "$i"; done; } > "$REPO/newborn.txt"

tui_start
# The file lists (host side) with its add count, and auto-opens in the editor.
wait_for "newborn.txt"
wait_for "UNBORNADD line 03"
# The teeth: an added file on an unborn repo is fully green — every line carries the "+" sign.
frame | grep -q "+ UNBORNADD" || fail "unborn-repo added file lacks the + add signs (base did not fall back to the empty tree)"
echo "ok 1 - unborn repo: added file paints green against the empty-tree base"

keys q; sleep 0.3; keys y 2>/dev/null || true
wait_session_end

# --- Phase B: detached HEAD -----------------------------------------------------------------
# Commit the tree, then detach: HEAD now resolves to a commit but there is no branch name.
git -C "$REPO" add -A && git -C "$REPO" commit -qm base
git -C "$REPO" checkout -q --detach HEAD

tui_start
# A clean detached checkout has nothing changed vs HEAD -> the empty-state greeter, not a
# leftover buffer or a crash.
wait_for "no changes in scope"
echo "ok 2 - detached HEAD: clean tree parks on the empty greeter"

# A live edit re-lists in Changes (the poll diffs the worktree against the detached HEAD).
printf 'DETACHEDEDIT tail\n' >> "$REPO/newborn.txt"
wait_for "DETACHEDEDIT tail"
frame | grep -qF "no changes in scope" && fail "empty greeter lingered after a live edit re-listed the file"
echo "ok 3 - detached HEAD: a live edit re-lists in Changes"

keys q; sleep 0.3; keys y 2>/dev/null || true
wait_session_end
echo "# degenerate git states degrade gracefully (unborn + detached HEAD)"
