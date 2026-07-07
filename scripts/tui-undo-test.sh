#!/usr/bin/env bash
# Live gate for undo scoping in the review surface: undo covers this session only. With the
# user's 'undofile' a fresh buffer would open with history from earlier sessions — the host
# pushes noundofile. The Changes view is locked outright (undo inert there); in All files a
# checktime reload of an agent-changed file IS undoable (vim's default 'undoreload', kept
# deliberately: an explicit u repaints visibly, is redoable, and is the recovery hatch for
# the agent-blind-overwrite race — clearing history there turned that race into silent loss).
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

# 5. In All files, a reload is an explicit, visible, redoable undo step: u reverts to the
#    pre-agent buffer (the instant autosave persists that choice), C-r brings it back.
keys Tab; sleep 0.3
keys 2; sleep 0.8
keys Tab; sleep 0.4
keys u; sleep 0.8
frame | grep -qF "AGENT CHANGE" && fail "undo did not revert the reload"
frame | grep -qF "MINE" || fail "undo lost the user's own edit"
for _ in $(seq 20); do grep -q "AGENT CHANGE" "$REPO/src/one.txt" 2>/dev/null || break; sleep 0.25; done
grep -q "AGENT CHANGE" "$REPO/src/one.txt" && fail "the reverted state did not autosave"
keys C-r; sleep 0.8
wait_for "AGENT CHANGE"
for _ in $(seq 20); do grep -q "AGENT CHANGE" "$REPO/src/one.txt" 2>/dev/null && break; sleep 0.25; done
grep -q "AGENT CHANGE" "$REPO/src/one.txt" || fail "redo did not autosave the agent change back"
echo "ok 3 - a reload undoes explicitly, autosaves, and redoes"

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
