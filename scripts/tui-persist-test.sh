#!/usr/bin/env bash
# Live gate for comment persistence: comments survive the pane being closed and reopened
# (the store is written to the repo's private comments ref on every mutation), and deleting
# the last comment persists the empty state — a restart must NOT resurrect it.
set -uo pipefail
SOCK=rvpersist
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

printf 'line a\nline b\nline c\nline d\nline e\n' > "$REPO/src/one.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
printf 'CHANGE ONE\n' >> "$REPO/src/one.txt"

tui_start
wait_for "one.txt"
wait_for "CHANGE ONE"

# 1. Leave a comment, then quit the pane.
keys Tab; sleep 0.4
keys Space r c; sleep 0.3
keys -l "survives restarts"
keys Enter
wait_for "╭─ comment"
wait_for "survives restarts"
keys Tab; sleep 0.3
keys q; sleep 0.5; keys y 2>/dev/null || true
wait_session_end
echo "ok 1 - comment left, pane closed"

# 2. A fresh pane on the same repo restores the comment (card + Send counter).
tui_start
wait_for "one.txt"
wait_for "CHANGE ONE"
wait_for "survives restarts"
frame | grep -q "Send (1)" || fail "the restored comment is not counted as un-sent"
echo "ok 2 - the comment survives the restart"

# 3. Delete it; a further restart must stay empty (the deletion persisted too).
keys Tab; sleep 0.4
keys Space r x
wait_gone "survives restarts"
keys Tab; sleep 0.3
keys q; sleep 0.5; keys y 2>/dev/null || true
wait_session_end
tui_start
wait_for "one.txt"
wait_for "CHANGE ONE"
sleep 1
frame | grep -q "survives restarts" && fail "a deleted comment came back after restart"
frame | grep -q "Send (0)" || fail "expected an empty send counter after the persisted delete"
echo "ok 3 - deleting the last comment persists"

# 4. Reviewed ticks persist across a restart too.
read -r COL ROW <<< "$(locate_right 'one.txt')"
[ -n "${COL:-}" ] || fail "cannot locate one.txt in the file list"
click "$COL" "$ROW"; sleep 0.5
keys Enter; sleep 0.6
frame | grep -q "1 reviewed" || fail "Enter on the file row did not mark it reviewed"
frame | grep -q "✓" || fail "no tick rendered for the reviewed file"
keys q; sleep 0.5; keys y 2>/dev/null || true
wait_session_end
tui_start
wait_for "one.txt"
wait_for "CHANGE ONE"
frame | grep -q "✓" || fail "the reviewed tick did not survive the restart"
frame | grep -q "1 reviewed" || fail "the reviewed counter did not survive the restart"
echo "ok 4 - reviewed ticks survive a restart"

# 5. A content change drops the tick on the next poll (user/agent edit or branch switch).
printf 'POST-REVIEW CHANGE\n' >> "$REPO/src/one.txt"
for _ in $(seq 20); do frame | grep -q "reviewed" || break; sleep 0.25; done
frame | grep -q "reviewed" && fail "the tick outlived a content change"
echo "ok 5 - a content change clears the tick"

# Focus is already on the files pane (step 4's click) — a Tab here would move INTO the
# editor and this q would type a macro-record key instead of quitting.
keys q; sleep 0.3; keys y 2>/dev/null || true
wait_session_end
echo "# all persistence assertions passed"
