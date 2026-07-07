#!/usr/bin/env bash
# Live gate for hunk revert (space rh): the hunk under the cursor reverts to base on screen
# AND on disk through the lock; reverting a file's last hunk advances to the next changed
# file and the reverted file drops from Changes on the next poll.
set -uo pipefail
SOCK=rvrevert
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

seq -f 'a%g' 30 > "$REPO/src/one.txt"
seq -f 'b%g' 30 > "$REPO/src/two.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
sed -i 's/^a2$/ONEHUNK1/; s/^a25$/ONEHUNK2/' "$REPO/src/one.txt"
sed -i 's/^b3$/TWOHUNK1/' "$REPO/src/two.txt"

tui_start
wait_for "one.txt"
wait_for "ONEHUNK1"
wait_for "unchanged lines"
frame | grep -q "2 changed" || fail "expected 2 changed files at start"

# 1. rh reverts the hunk under the cursor (focus() starts on hunk 1) — screen, disk, lock.
keys Tab; sleep 0.4
keys Space r h; sleep 1
frame | grep -q "ONEHUNK1" && fail "the reverted hunk is still on screen"
grep -q "ONEHUNK1" "$REPO/src/one.txt" && fail "the reverted hunk is still on disk"
grep -q "^a2$" "$REPO/src/one.txt" || fail "the base line did not come back"
keys x; sleep 0.5
frame | grep -q "E21" || fail "the buffer is editable after the revert"
echo "ok 1 - rh reverts the hunk through the lock"

# 2. Reverting the file's LAST hunk advances to the next changed file, and the clean file
#    drops from Changes on the next poll.
keys Enter; sleep 0.5   # walk to the remaining hunk
keys Space r h
wait_for "TWOHUNK1"
grep -q "^a25$" "$REPO/src/one.txt" || fail "the second hunk did not revert on disk"
wait_for "1 changed"
echo "ok 2 - the last hunk's revert advances and the file leaves Changes"

# 3. The final file reverts to a fully clean tree.
keys Space r h; sleep 1
grep -q "^b3$" "$REPO/src/two.txt" || fail "two.txt did not revert on disk"
wait_for "0 changed"
echo "ok 3 - the changeset reverts to clean"

keys Tab; sleep 0.3
keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all revert assertions passed"
