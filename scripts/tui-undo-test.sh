#!/usr/bin/env bash
# Live gate for undo scoping in the review surface: undo must cover the user's own edits since
# load and nothing more. With the user's 'undofile' a fresh buffer would open with history from
# earlier sessions, and a checktime reload of an agent-changed file is itself an undo step
# ('undoreload') — one u (or a client Undo button) then reverts the agent's work and the
# autosave persists the reversion. The host pushes noundofile + undoreload=0, and the Changes
# view is locked outright — editing (and undo) live in All files.
set -uo pipefail
SOCK=rvundo
TUI_INIT_EXTRA="vim.o.number = true
vim.o.undofile = true"
source "$(dirname "$0")/tui-lib.sh"

printf 'line a\nline b\nline c\nline d\nline e\nline f\nline g\nline h\nline i\nline j\nline k\nline l\n' > "$REPO/src/one.txt"
printf 'other\n' > "$REPO/src/two.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
printf 'TAIL CHANGE\n' >> "$REPO/src/one.txt"

tui_start
wait_for "one.txt"
wait_for "TAIL CHANGE"

# 1. The user edits in All files (the authoring surface); flipping views autosaves to disk.
keys 2; sleep 0.8
keys Tab; sleep 0.4
keys i; keys -l "MINE "; esc
wait_for "MINE"
keys Tab; sleep 0.3
keys 1; sleep 0.8
grep -q "MINE" "$REPO/src/one.txt" || fail "the user edit did not autosave before the agent change"

# 2. The agent appends to the saved file on disk; the next view round trip reloads it.
printf 'AGENT CHANGE\n' >> "$REPO/src/one.txt"
keys 2; sleep 0.8; keys 1; sleep 0.8
wait_for "AGENT CHANGE"

# 3. In the locked Changes view undo is inert outright (E21) — nothing can revert.
keys Tab; sleep 0.4
keys u; sleep 0.8
frame | grep -qF "AGENT CHANGE" || fail "undo in the locked Changes view reverted the agent's change"
echo "ok 1 - undo is inert in the read-only Changes view"

# 4. ...and the diff stays a diff: unchanged early lines remain folded, not expanded.
frame | grep -qF "line b" && fail "the fold over unchanged lines is gone (file expanded)"
echo "ok 2 - the folded hunk view survives the undo attempt"

# 5. The real undo-scoping property, exercised where editing lives: in All files, undo after
#    the reload must not revert the agent's work (reload cleared history; no undofile past).
keys Tab; sleep 0.3
keys 2; sleep 0.8
keys Tab; sleep 0.4
keys u; sleep 0.8
frame | grep -qF "AGENT CHANGE" || fail "undo after a reload reverted the agent's change"
frame | grep -qF "MINE" || fail "undo after a reload reverted the user's saved edit"
echo "ok 3 - undo cannot reach past the reload of the agent's change"

# 6. In-session undo/redo still work on the user's own edits.
keys i; keys -l "XYZQ "; esc
wait_for "XYZQ"
keys u; sleep 0.5
wait_gone "XYZQ"
keys C-r; sleep 0.5
wait_for "XYZQ"
keys u; sleep 0.5
wait_gone "XYZQ"
echo "ok 4 - the user's own edits undo and redo normally"

keys Tab; sleep 0.3
keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all undo-scoping assertions passed"
