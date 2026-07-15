#!/usr/bin/env bash
# Live gate: the Backspace overlay's reset option. For a changed (modified or deleted) file the
# confirm overlay offers `r` to reset the file to the review base — overwrite the working-tree
# file with the diff's base content — instead of only deleting it. A new/untracked file has no
# base, so its overlay stays the plain delete Yes/No and `r` does nothing there.
set -uo pipefail
SOCK=rvreset
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

printf 'KEEPLINE\n' > "$REPO/mod.txt"
printf 'delbody\n'  > "$REPO/del.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
printf 'DIRTYLINE\n' >> "$REPO/mod.txt"   # mod.txt is now Modified (base + one added line)
rm "$REPO/del.txt"                         # del.txt is now a working-tree Deletion
printf 'brand new\n' > "$REPO/new.txt"     # untracked — no base to reset to

pick() { read -r C R <<< "$(locate_right "$1")"; [ -n "${C:-}" ] || fail "$1 row not found"; click "$C" "$R"; sleep 0.4; }

tui_start
wait_for "mod.txt"
keys 2; sleep 0.8            # All files: mod.txt, del.txt, new.txt all listed at the root
wait_for "del.txt"
wait_for "new.txt"

# 1. A modified file: reset restores the committed (base) content, dropping the edit.
pick "mod.txt"
keys BSpace; sleep 0.5
frame | grep -qF "r reset" || fail "the reset option was not offered for a modified file"
keys r; sleep 0.9
grep -qxF 'KEEPLINE'  "$REPO/mod.txt" || fail "reset did not restore the base content of mod.txt"
grep -qF  'DIRTYLINE' "$REPO/mod.txt" && fail "reset left the uncommitted edit on disk"
echo "ok 1 - reset restores a modified file to the review base"

# 2. A deleted file: reset brings it back with its base content.
pick "del.txt"
keys BSpace; sleep 0.5
frame | grep -qF "r reset" || fail "the reset option was not offered for a deleted file"
keys r; sleep 0.9
[ -f "$REPO/del.txt" ] || fail "reset did not bring back the deleted file"
grep -qxF 'delbody' "$REPO/del.txt" || fail "the restored file has the wrong content"
echo "ok 2 - reset brings back a deleted file"

# 3. An untracked file: no base, so the overlay is the plain delete (no reset), and `d` deletes.
pick "new.txt"
keys BSpace; sleep 0.5
frame | grep -qF "r reset" && fail "reset was wrongly offered for an untracked file"
frame | grep -qF "y / enter" || fail "the plain delete overlay was not shown for an untracked file"
keys d; sleep 0.9
[ -f "$REPO/new.txt" ] && fail "d did not delete the untracked file"
echo "ok 3 - an untracked file gets the plain delete overlay (no reset), d deletes it"

keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all reset-overlay assertions passed"
