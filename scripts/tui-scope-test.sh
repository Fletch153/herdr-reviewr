#!/usr/bin/env bash
# Live gate: comments pin to the scope+base they were authored under; scope flips re-diff the
# editor; the comments list's Enter restores the authoring view.
set -uo pipefail
SOCK=rvscope
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

printf 'base one\nbase two\nbase three\n' > "$REPO/src/f1.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
git -C "$REPO" checkout -qb feature
printf 'COMMITTED CHANGE\n' >> "$REPO/src/f1.txt"
printf 'new file line\n' > "$REPO/src/f2.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm B
printf 'UNCOMMITTED TAIL\n' >> "$REPO/src/f1.txt"

tui_start
wait_for "f1.txt"
wait_for "UNCOMMITTED TAIL"

# 1. Comment on the change under the (default) commit scope.
keys Tab; sleep 0.4
keys Space r c; sleep 0.3
keys -l "commit-scope note"; keys Enter
wait_for "╭─ comment"
wait_for "Send (1)"
echo "ok 1 - card under commit scope"

# 2. Branch scope: different diff — the card must hide; the editor re-diffs vs the merge-base
#    (the committed change now paints too).
keys Tab; sleep 0.3
keys b
wait_gone "╭─ comment"
wait_for "COMMITTED CHANGE"
frame | grep -q "+.*COMMITTED CHANGE" || fail "committed change not painted under branch scope"
echo "ok 2 - card hidden + re-diffed under branch scope"

# 3. Last-turn scope (empty here — the stub has no turn) parks the editor on the empty
#    greeter — the previously open buffer must NOT linger (the empty-changeset contract;
#    tui-empty-test.sh pins the full round trip). Step 4's list-restore then proves the
#    park is reversible.
keys t
wait_for "no changes in scope"
echo "ok 3 - empty last-turn scope parks on the greeter"

# 4. The comments list restores the authoring scope on Enter and the card returns.
keys l
wait_for "Comments (1)"
keys Enter
wait_gone "Comments (1)"
wait_for "╭─ comment"
frame | grep -q "\[commit\]" || fail "scope chip did not restore to commit"
echo "ok 4 - list jump restores scope and card"

# The list jump landed focus in the editor — Tab back to the files pane so q quits instead
# of starting a macro recording inside nvim.
keys Tab; sleep 0.3
keys q; sleep 0.3; keys y 2>/dev/null || true
wait_session_end
echo "# all scope-pinning assertions passed"
