#!/usr/bin/env bash
# Live gate: renamed-file (staged R) × {paint, reviewed-tick, hunk-revert, revert-advance}.
# Locks the rename dimension of Rows 5/6/7:
#   - the edit paints against the OLD path's base (rename map), not as one big insertion;
#   - a reviewed tick lands on a renamed file (keyed by the new-path content);
#   - reverting the hunk writes the base to the NEW path and never resurrects the old path;
#   - reverting a rename's LAST changed hunk marks it reviewed (the walk contract) and, because
#     a pure rename stays a change, the file remains listed and ticked.
set -uo pipefail
SOCK=rvrenamefile
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

{ for i in $(seq 1 12); do printf 'stable content %02d\n' "$i"; done; } > "$REPO/src/old_name.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
git -C "$REPO" mv src/old_name.txt src/new_name.txt
sed -i 's/stable content 06/EDITED CONTENT 06/' "$REPO/src/new_name.txt"
git -C "$REPO" add -A   # staged rename → git status R

tui_start
wait_for "new_name.txt"
wait_for "EDITED CONTENT 06"
sleep 1
frame | grep -q "1 changed" || fail "expected exactly 1 changed file (the rename) at start"
frame | grep -qE "~ +6 EDITED CONTENT 06" || fail "the edit lacks its modification sign (old-path base not resolved for the rename)"
frame | grep -q "stable content 06" || fail "the old-side line is not shown as a virtual line for the rename"
echo "ok 0 - rename listed and painted against its old-path base"

# 1. Reviewed-tick the renamed file in Changes (click its row + Enter) — the tick lands, keyed
#    by the new-path content hash.
read -r COL ROW <<< "$(locate_right 'new_name.txt')"
[ -n "${COL:-}" ] || fail "cannot locate new_name.txt in the Changes list"
click "$COL" "$ROW"; sleep 0.4
keys Enter
wait_for "1 reviewed"
echo "ok 1 - reviewed tick lands on a renamed file"

# 2. Revert the edited hunk (Tab into the editor lands the cursor on the first change).
#    The base line must come back at the NEW path on disk; the old path must stay deleted.
keys Tab; sleep 0.4
keys Space r h; sleep 1.2
frame | grep -q "EDITED CONTENT 06" && fail "the reverted edit is still on screen"
grep -q "^EDITED CONTENT 06$" "$REPO/src/new_name.txt" && fail "the reverted edit is still on disk"
grep -q "^stable content 06$" "$REPO/src/new_name.txt" || fail "the base line did not return on disk at the NEW path"
[ -e "$REPO/src/old_name.txt" ] && fail "revert resurrected the old path (rename undone)"
echo "ok 2 - revert writes the base to the new path; the old path stays gone"

# 3. That was the rename's last changed hunk → the walk marks it reviewed (revert-advance
#    contract). A pure rename is still a change, so the file stays listed AND ticked.
wait_for "1 reviewed"
frame | grep -q "new_name.txt" || fail "the pure rename vanished from Changes after the revert"
echo "ok 3 - the last-hunk revert marks the rename reviewed and it stays listed"

# Quit: focus is in the editor (last Tab) — normalize to the files pane first so q quits.
keys Escape; sleep 0.2; keys Tab; sleep 0.3
keys q; sleep 0.4; keys y 2>/dev/null || true
wait_session_end
echo "# all rename-file assertions passed"
