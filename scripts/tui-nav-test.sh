#!/usr/bin/env bash
# Live gate for the review walk: Space/Enter step hunk to hunk inside the read-only Changes
# pane and advance to the next changed file past the last hunk; Backspace mirrors backward,
# entering the previous file on its LAST hunk; on All files the walk moves file to file
# through the changeset, and Space stays the user's leader (never navigation).
set -uo pipefail
SOCK=rvnav
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

seq -f 'a%g' 30 > "$REPO/src/one.txt"
seq -f 'b%g' 30 > "$REPO/src/two.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
sed -i 's/^a2$/ONEHUNK1/; s/^a25$/ONEHUNK2/' "$REPO/src/one.txt"
sed -i 's/^b3$/TWOHUNK1/; s/^b26$/TWOHUNK2/' "$REPO/src/two.txt"

tui_start
wait_for "one.txt"
wait_for "ONEHUNK1"
wait_for "unchanged lines" # the fold marker doubles as the Changes-view sentinel

# 1. Space is RETIRED as a review key: in the files pane it neither marks nor advances.
keys Space; sleep 0.8
frame | grep -q "file reviewed" && fail "Space still marks/advances from the files pane"
frame | grep -q "ONEHUNK1" || fail "Space disturbed the open file"
echo "ok 1 - Space is retired as the review key"

# 2. Enter inside the editor steps the hunks first (focus() started on hunk 1 of 2), and only
#    past the last hunk marks the file reviewed and opens the next changed file.
keys Tab; sleep 0.4
keys Enter; sleep 0.8
frame | grep -q "file reviewed" && fail "the first Enter skipped the hunk walk and advanced"
frame | grep -q "ONEHUNK1" || fail "the hunk step left the file"
keys Enter
wait_for "file reviewed"
wait_for "TWOHUNK1"
echo "ok 2 - Enter walks the hunks, then marks and advances"

# 3. Enter is the same walk: one step for two.txt's second hunk, the next finishes the review.
keys Enter; sleep 0.8
frame | grep -q "all files reviewed" && fail "Enter advanced without walking two.txt's hunks"
keys Enter
wait_for "all files reviewed"
frame | grep -q "TWOHUNK1" || fail "finishing the review should keep the last file open"
echo "ok 3 - Enter walks and finishes the review"

# 4. Backspace mirrors the walk. From two.txt's last hunk: one in-file step, then the boundary
#    opens the PREVIOUS file on its LAST hunk — so exactly one more in-file step must remain
#    before the next boundary (a first-hunk landing would leave immediately and fail below).
keys BSpace; sleep 0.8
frame | grep -q "TWOHUNK1" || fail "Backspace lost the open file"
keys BSpace
wait_for "ONEHUNK1"
keys BSpace; sleep 0.8
frame | grep -q "TWOHUNK1" && fail "the backward entry landed on the first hunk, not the last"
frame | grep -q "ONEHUNK1" || fail "the in-file backward step left the file"
keys BSpace
wait_for "TWOHUNK1"
echo "ok 4 - Backspace walks back and enters files on their last hunk"

# 5. All files: no hunks, so the walk moves file to file through the changeset.
keys Tab; sleep 0.3
keys 2; sleep 0.8
keys Tab; sleep 0.3
wait_gone "unchanged lines"
keys Enter
wait_for "ONEHUNK1"
keys BSpace
wait_for "TWOHUNK1"
echo "ok 5 - Enter/Backspace walk the changeset in All files"

# 6. Space in the All files pane is the user's LEADER, never navigation.
keys Space; sleep 0.8
frame | grep -q "TWOHUNK1" || fail "Space navigated in All files (the leader is broken)"
esc
echo "ok 6 - Space stays the leader in All files"

# 7. Ticks are per-FILE, not per-view: both files were reviewed by the walk in the Changes
#    tab — their ✓ must show in the All files tree too.
keys Tab; sleep 0.4
frame | grep -q "✓" || fail "no reviewed ticks in the All files tree"
echo "ok 7 - ticks show across tabs"

# 8. Enter on a file row in the files pane toggles its tick directly.
keys 1; sleep 0.8
frame | grep -q "2 reviewed" || fail "expected both files reviewed before the toggle"
keys Enter; sleep 0.6
frame | grep -q "1 reviewed" || fail "Enter on a reviewed file row did not clear its tick"
keys Enter; sleep 0.6
frame | grep -q "2 reviewed" || fail "Enter on a file row did not re-mark it"
echo "ok 8 - Enter in the files pane toggles the reviewed mark"

keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all nav assertions passed"
