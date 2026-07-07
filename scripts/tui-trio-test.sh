#!/usr/bin/env bash
# Live gate: rr resolve-under-cursor, ry yank feedback (with or without a clipboard tool),
# and the Backspace file-delete confirmation.
set -uo pipefail
SOCK=rvtrio
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"
printf 'aaa\nbbb\n' > "$REPO/src/keep.txt"
printf 'ddd\neee\n' > "$REPO/src/dispose.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
printf 'KEEP CHANGE\n' >> "$REPO/src/keep.txt"
printf 'DISPOSE CHANGE\n' >> "$REPO/src/dispose.txt"
tui_start
wait_for "keep.txt"
wait_for "DISPOSE CHANGE"

# 1. Comment + space rr resolves under the cursor (card + count clear).
keys Tab; sleep 0.4
keys Space r c; sleep 0.3
keys -l "resolve me"; keys Enter
wait_for "╭─ comment"
wait_for "Send (1)"
keys Space r r
wait_gone "╭─ comment"
wait_for "Send (0)"
wait_for "resolved (0 left)"
echo "ok 1 - rr resolves the comment under the cursor"

# 1b. Clicking the CARD lands the cursor on the line below the anchor (virt_lines behavior);
#     rr must still find the comment from there.
keys Space r c; sleep 0.3
keys -l "card click"; keys Enter
wait_for "card click"
loc=$(locate "card click")
click "$(echo $loc | cut -d' ' -f1)" "$(echo $loc | cut -d' ' -f2)"
sleep 0.5
keys Space r r
wait_gone "╭─ comment"
wait_for "Send (0)"
echo "ok 1b - rr resolves after a click on the card itself"

# 2. space ry yank: with no clipboard tool it must surface the error, not wedge; with one it
#    reports the copy. Either way the reviewer stays interactive.
keys Space r c; sleep 0.3
keys -l "yank me"; keys Enter
wait_for "yank me"
keys Space r y
for _ in $(seq 40); do frame | grep -qE "copied 1 comment|clipboard failed|no clipboard tool" && break; sleep 0.25; done
frame | grep -qE "copied 1 comment|clipboard failed|no clipboard tool" || fail "ry gave no feedback"
keys j; keys k   # still interactive?
echo "ok 2 - ry reports its outcome and the app stays live"

# 3. Backspace on a file row asks before deleting; y deletes the file from disk.
keys Tab; sleep 0.3
read -r C R <<< "$(locate_right "dispose.txt")"
click "$C" "$R"; sleep 0.3
keys BSpace
wait_for "elete"   # Delete/delete confirm text
keys y
sleep 0.8
[ ! -f "$REPO/src/dispose.txt" ] || fail "confirmed delete left the file on disk"
echo "ok 3 - backspace delete confirms and removes the file"

keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all resolve/yank/delete assertions passed"
