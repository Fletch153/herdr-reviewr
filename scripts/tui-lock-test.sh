#!/usr/bin/env bash
# Live gate for the read-only Changes view's authoring flip: insert-entry keys and pastes in
# the locked view flip to All files on the same file and land as real input once the plain
# view unlocks; everything else mutating stays honestly inert (E21), on screen and on disk.
set -uo pipefail
SOCK=rvlock
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

printf 'line a\nline b\nline c\nline d\nline e\nline f\nline g\nline h\nline i\nline j\nline k\nline l\n' > "$REPO/src/one.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
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

keys Tab; sleep 0.3
keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all lock/flip assertions passed"
