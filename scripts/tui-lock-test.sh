#!/usr/bin/env bash
# Live gate for the read-only Changes view's authoring flip: insert-entry keys and pastes in
# the locked view flip to All files on the same file and land as real input once the plain
# view unlocks; everything else mutating stays honestly inert (E21), on screen and on disk.
set -uo pipefail
SOCK=rvlock
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

printf 'line a\nline b\nline c\nline d\nline e\nline f\nline g\nline h\nline i\nline j\nline k\nline l\n' > "$REPO/src/one.txt"
# A top-level file that sorts first: in the collapsed All-files tree the cursor must NOT be
# left on it after a flip (the poll opens whatever file row the cursor rests on).
printf 'decoy content\n' > "$REPO/AAA-decoy.txt"
printf 'two base\n' > "$REPO/src/two.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
printf 'CHANGE TWO\n' >> "$REPO/src/two.txt"
printf 'TAIL CHANGE\n' >> "$REPO/src/one.txt"

tui_start
wait_for "one.txt"
wait_for "TAIL CHANGE"
wait_for "unchanged lines" # the fold marker doubles as the Changes-view sentinel

# 1. dd in the locked view is inert: E21 on screen, the buffer and the disk untouched.
keys Tab; sleep 0.4
keys d d; sleep 0.6
frame | grep -q "E21" || fail "dd in the locked Changes view did not answer E21"
grep -q "TAIL CHANGE" "$REPO/src/one.txt" || fail "dd mutated the file on disk"
echo "ok 1 - mutating keys are inert in Changes"

# 2. i flips to All files (fold marker gone = plain view) and typing lands + autosaves.
keys i
wait_gone "unchanged lines"
sleep 0.5
keys -l "FLIPPED "
esc
wait_for "FLIPPED"
for _ in $(seq 20); do grep -q "FLIPPED" "$REPO/src/one.txt" 2>/dev/null && break; sleep 0.25; done
grep -q "FLIPPED" "$REPO/src/one.txt" || fail "the flipped insert did not autosave"
echo "ok 2 - i flips to All files and the insert lands"

# 2b. The flip revealed and selected the file in the tree: several polls later the editor
#     still shows it (an unselected cursor on the decoy row would re-open the decoy).
sleep 2
frame | grep -q "FLIPPED" || fail "the view drifted off the flipped file after a poll"
frame | grep -q "decoy content" && fail "the poll re-opened the file under the stale cursor"
echo "ok 2b - the flip selects the file in the tree (view is poll-stable)"

# 2c. ctrl+i (kitty CSI u encoding, straight to the app) returns to the Changes review of
#     the same file, with the fresh edit in the diff.
keys -l "$(printf '\033[105;5u')"
wait_for "unchanged lines"
frame | grep -q "FLIPPED" || fail "the review lost the fresh edit after ctrl+i"
echo "ok 2c - ctrl+i returns to the review with the edit included"

# 3. o flips too, opening a line below the cursor.
keys Tab; sleep 0.3
keys 1; sleep 0.8
keys Tab; sleep 0.3
wait_for "unchanged lines"
keys o
wait_gone "unchanged lines"
sleep 0.5
keys -l "OPENED-LINE"
esc
wait_for "OPENED-LINE"
echo "ok 3 - o flips and opens a line"

# 4. A bracketed paste into the locked view flips and lands as literal text.
keys Tab; sleep 0.3
keys 1; sleep 0.8
keys Tab; sleep 0.3
wait_for "unchanged lines"
keys -l "$(printf '\033[200~PASTED-BLOCK\033[201~')"
wait_gone "unchanged lines"
wait_for "PASTED-BLOCK"
for _ in $(seq 20); do grep -q "PASTED-BLOCK" "$REPO/src/one.txt" 2>/dev/null && break; sleep 0.25; done
grep -q "PASTED-BLOCK" "$REPO/src/one.txt" || fail "the flipped paste did not autosave"
echo "ok 4 - a paste flips and lands"

# 5. User-driven jumps inherit the active view: :e another changed file in Changes arrives
#    locked and painted; the same jump in All files arrives editable.
keys Tab; sleep 0.3
keys 1; sleep 0.8
keys Tab; sleep 0.3
keys -l ":e src/two.txt"; keys Enter; sleep 0.8
frame | grep -qE '\+ *[0-9]+ CHANGE TWO' || fail "a jump inside Changes did not paint the entered buffer"
keys x; sleep 0.5
frame | grep -q "E21" || fail "a jump inside Changes left the entered buffer editable"
echo "ok 5 - a jump inside Changes arrives locked and painted"
keys Tab; sleep 0.3
keys 2; sleep 0.8
keys Tab; sleep 0.3
keys -l ":e src/one.txt"; keys Enter; sleep 0.8
keys x; sleep 0.5
frame | grep -q "E21" && fail "a jump inside All files arrived locked"
echo "ok 6 - a jump inside All files arrives editable"

keys Tab; sleep 0.3
keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all lock/flip assertions passed"
