#!/usr/bin/env bash
# Live gate: deleted-file handling in the All files tab — a deletion stays listed with its 'D'
# even after its removal is staged (git ls-files would drop it), and its reviewed tick survives
# the background rescan (prune must not drop a file that is gone from disk).
set -uo pipefail
SOCK=rvdel
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

printf 'gone\n' > "$REPO/del.txt"
printf 'keep\n' > "$REPO/keep.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
rm "$REPO/del.txt"   # unstaged deletion

# first non-space cell after the pane divider on the row containing $1 (its change marker)
marker_cell() {
  frame | python3 -c "
import sys
for nr,ln in enumerate(sys.stdin.read().splitlines(),1):
    d=ln.find('││')
    if d<0: continue
    seg=ln[d+2:]
    if '$1' in seg:
        j=len(seg)-len(seg.lstrip())
        print(d+2+j+1, nr); break
"
}
row_listed() { [ -n "$(locate_right "$1")" ]; }

tui_start
wait_for "del.txt"
keys 2; sleep 0.8   # All files

# 1. The deletion is listed with a 'D' marker.
row_listed "del.txt" || fail "del.txt not listed in All files"
read -r MC MR <<< "$(marker_cell del.txt)"
[ -n "${MC:-}" ] || fail "could not find del.txt's marker cell"
echo "ok 1 - deleted file listed in All files"

# 2. Clicking the marker stages the deletion; the row must NOT vanish (it leaves git ls-files).
click "$MC" "$MR"
for _ in $(seq 20); do frame | grep -q "staged del.txt" && break; sleep 0.2; done
frame | grep -q "staged del.txt" || fail "marker click did not stage the deletion"
sleep 1.0   # let a background rescan rebuild the list
row_listed "del.txt" || fail "the staged deletion vanished from All files"
echo "ok 2 - a staged deletion stays visible in All files"

# 3. Ticking the deleted file sticks across the background rescan (prune must keep it).
read -r RC RR <<< "$(locate_right 'del.txt')"
click "$RC" "$RR"; sleep 0.4
keys Enter; sleep 0.5           # Enter on the file row toggles reviewed
frame | grep -qE '[0-9]+ reviewed' || fail "the deleted file did not tick"
sleep 1.5                        # >=2 background rescans (poll 500ms) + prune
frame | grep -qE '[0-9]+ reviewed' || fail "the deleted file's tick was undone by the rescan"
echo "ok 3 - a deleted file stays reviewed across a rescan"

keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all deleted-file assertions passed"
