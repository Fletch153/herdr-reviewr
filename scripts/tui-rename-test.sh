#!/usr/bin/env bash
# Live gate: a renamed file diffs against its old path (the host's rename map), not as one
# big insertion.
set -uo pipefail
SOCK=rvren
source "$(dirname "$0")/tui-lib.sh"

{ for i in $(seq 1 12); do printf 'stable content %02d\n' "$i"; done; } > "$REPO/src/old_name.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
git -C "$REPO" mv src/old_name.txt src/new_name.txt
sed -i 's/stable content 06/EDITED CONTENT 06/' "$REPO/src/new_name.txt"
git -C "$REPO" add -A   # staged rename so git status reports R

tui_start
wait_for "new_name.txt"
wait_for "EDITED CONTENT 06"
sleep 1
frame | grep -q "+ stable content 02" && fail "rename painted an unchanged line as an addition"
frame | grep -q "~ EDITED CONTENT 06" || fail "the real edit lacks its modification sign"
frame | grep -q "stable content 06" || fail "the replaced old line is not shown as a virtual line"
keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# rename renders as a modification, not an insertion"
