#!/usr/bin/env bash
# Live gate: an empty changeset parks the editor on the empty-state scratch — on first entry
# AND when returning from All files (the stale-buffer carryover a live user report caught:
# 1 -> 2 opened a file -> 1 kept it up, reading as "there are changes").
set -uo pipefail
SOCK=rvempty
source "$(dirname "$0")/tui-lib.sh"

# A clean tree: the worktree has content (so All files opens a real file) but the changeset
# is empty (everything committed). Root-level: the All files tree starts with directories
# collapsed, and the auto-open needs a visible file row.
{ for i in $(seq 1 8); do printf 'settled line %02d\n' "$i"; done; } > "$REPO/settled.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm base

tui_start
# 1) First entry: empty Changes shows the empty state, not a blank leftover.
wait_for "no changes in scope"

# 2) All files: the tree auto-opens the real file (content marker — names show in both panes).
keys 2
wait_for "settled line 03"
frame | grep -qF "no changes in scope" && fail "the empty-state scratch leaked into All files"

# 3) Back to Changes: the empty state must re-assert; the file's content must leave the editor.
keys 1
wait_for "no changes in scope"
frame | grep -qF "settled line 03" && fail "empty Changes kept the All-files buffer up"

# 4) Round trip again: the park reset the published-view memory, so the same file re-opens.
keys 2
wait_for "settled line 03"
keys 1
wait_for "no changes in scope"

keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# empty changeset re-asserts the editor empty state across tab switches"
