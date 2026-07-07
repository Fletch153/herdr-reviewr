#!/usr/bin/env bash
# Live gate for live view sync: agent writes to an OPEN file appear without any interaction
# (host poll sweeps checktime; the live FileChangedShell policy reloads clean buffers and
# repaints marks), and user edits hit disk the moment they exist (InsertLeave/TextChanged
# instant autosave) — no view switch needed in either direction.
set -uo pipefail
SOCK=rvlive
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

printf 'line a\nline b\nline c\nline d\nline e\nline f\nline g\nline h\n' > "$REPO/src/one.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
printf 'TAIL CHANGE\n' >> "$REPO/src/one.txt"

tui_start
wait_for "one.txt"
wait_for "TAIL CHANGE"

# 1. The agent appends while the file is open and NOTHING is pressed: the new line must
#    appear on its own, painted as a change.
printf 'AGENT LIVE\n' >> "$REPO/src/one.txt"
wait_for "AGENT LIVE"
frame | grep -qE '\+ *[0-9]+ AGENT LIVE' || fail "the live-reloaded line is not painted as an addition"
echo "ok 1 - an agent write to the open file appears and repaints with no interaction"

# 2. User edits save instantly: leaving insert puts the text on disk with no switch.
keys 2; sleep 0.8
keys Tab; sleep 0.4
keys i; keys -l "USERLIVE "; esc
wait_for "USERLIVE"
for _ in $(seq 20); do grep -q "USERLIVE" "$REPO/src/one.txt" 2>/dev/null && break; sleep 0.25; done
grep -q "USERLIVE" "$REPO/src/one.txt" || fail "the insert-mode edit did not autosave instantly"
echo "ok 2 - leaving insert saves to disk immediately"

# 3. Normal-mode changes save instantly too (dd, no switch).
keys d d
for _ in $(seq 20); do grep -q "USERLIVE" "$REPO/src/one.txt" 2>/dev/null || break; sleep 0.25; done
grep -q "USERLIVE" "$REPO/src/one.txt" && fail "the normal-mode dd did not autosave instantly"
echo "ok 3 - a normal-mode change saves to disk immediately"

keys Tab; sleep 0.3
keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all live-sync assertions passed"
