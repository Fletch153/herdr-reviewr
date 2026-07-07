#!/usr/bin/env bash
# Live gate: the space rd side-by-side diff dissolves cleanly from either side (no stranding
# on the base scratch, no leftover diff mode); divider drag resizes; help overlay works.
set -uo pipefail
SOCK=rvsplit
source "$(dirname "$0")/tui-lib.sh"

{ for i in $(seq 1 10); do printf 'row %02d ORIGINAL\n' "$i"; done; } > "$REPO/src/f.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
# Two changed lines: 2c reverts row 05 via `do`, and the file must STILL be a changed file
# afterwards (otherwise the host correctly flips it to the plain view and 2d's lock assertion
# would be testing the wrong thing).
sed -i -e 's/row 05 ORIGINAL/row 05 REWRITTEN/' -e 's/row 08 ORIGINAL/row 08 ALSO/' "$REPO/src/f.txt"

tui_start
wait_for "f.txt"
wait_for "row 05 REWRITTEN"

# 1. space rd opens the side-by-side diff: base content appears via the split's scratch.
keys Tab; sleep 0.4
keys Space r d
wait_for "row 05 ORIGINAL"
wait_for "txt ["
echo "ok 1 - rd shows base and worktree side by side"

# 2. :q dissolves the whole split back to the decorated working file (the scratch title
#    "txt [<ref>]" gone; the inline ~ marker present — no leftover diff-mode folding).
keys -l ":q"; keys Enter
sleep 0.8
frame | grep -q "~ row 05 REWRITTEN" || fail "working file not restored with inline marks after :q"
frame | grep -q "txt \[" && fail ":q left the base scratch on screen"
echo "ok 2 - :q dissolves the split back to the decorated working file"

# 2b. Reopen; close from the SCRATCH side this time: same clean recovery.
keys Space r d
wait_for "txt ["
keys C-w h
sleep 0.3
keys -l ":q"; keys Enter
sleep 0.8
frame | grep -q "~ row 05 REWRITTEN" || fail "working file not restored after closing the scratch side"
frame | grep -q "txt \[" && fail "scratch still on screen after closing its window"
echo "ok 2b - closing the scratch side recovers identically"

# 2c. The split is an editing surface even though Changes is read-only: `do` on the working
#     side pulls the base line back in (the split lifts the lock), and after teardown the
#     working buffer is locked again (typing is inert).
keys Space r d
wait_for "txt ["
keys -l "/REWRITTEN"; keys Enter
sleep 0.3
keys d o
sleep 0.5
frame | grep -q "row 05 REWRITTEN" && fail "do did not pull the base line (split still locked?)"
echo "ok 2c - do edits the working buffer inside the split"
keys -l ":q"; keys Enter
sleep 0.8
keys x
sleep 0.5
frame | grep -q "E21" || fail "the working buffer accepted an edit after split teardown"
echo "ok 2d - teardown re-locks the working buffer"

# 3. Divider drag: grab the pane divider and pull it left; the split point must move.
D0=$(div_col)
X=$((D0 + 1))
press "$X" 20
for dx in 10 20 30; do drag "$((X - dx))" 20; sleep 0.1; done
release "$((X - 30))" 20
sleep 0.7
D1=$(div_col)
{ [ -n "$D1" ] && [ "$D1" -lt "$D0" ]; } || fail "divider did not move ($D0 -> ${D1:-none})"
echo "ok 3 - divider drag resized the panes ($D0 -> $D1)"

# 4. Help overlay from the files pane; q closes it.
keys Tab; sleep 0.3
keys ?
wait_for "Editor (nvim)"
keys q
wait_gone "Editor (nvim)"
echo "ok 4 - help overlay opens and closes"

keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all split/divider/help assertions passed"
