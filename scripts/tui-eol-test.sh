#!/usr/bin/env bash
# Live gate for final-newline handling. (1) An EOL-only difference is a real change git lists
# the file for — the diff view must SAY so (the "\ newline at end of file" note) instead of
# showing an apparently unchanged file that stays listed forever. (2) The embed itself must
# never manufacture such a diff: editing a no-EOL file preserves its missing final newline
# (nofixendofline), so a fully undone edit leaves the file byte-identical and unlisted.
set -uo pipefail
SOCK=rveol
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

# one.txt: no-EOL base; the "agent" appends a line (which adds a newline after line h).
printf 'line a\nline b\nline c\nline d\nline e\nline f\nline g\nline h' > "$REPO/src/one.txt"
# two.txt: no-EOL base with an inline change only (bytes written exactly, no EOL drift).
printf 'alpha\nbeta' > "$REPO/src/two.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
printf '\nTAIL CHANGE\n' >> "$REPO/src/one.txt"
python3 - "$REPO/src/two.txt" <<'PY'
import sys
p = sys.argv[1]
open(p, 'w').write('alpha\nBETA')
PY

tui_start
wait_for "one.txt"
wait_for "TAIL CHANGE"

# 1. Undo the agent's change in All files; the leftover EOL difference must be SHOWN.
keys Tab; sleep 0.4
keys i
sleep 1.0
esc
keys -l "/TAIL CHANGE"; keys Enter; sleep 0.3
keys d d
sleep 1.0
keys Tab; sleep 0.3
keys 1; sleep 1.5
frame | grep -q "newline at end of file" || fail "the EOL-only difference is not shown in the diff view"
echo "ok 1 - an EOL-only difference renders its own marker"

# 2. two.txt: undoing an inline edit leaves the no-EOL file byte-identical (never re-listed).
keys j
wait_for "BETA"
keys Tab; sleep 0.4
keys i
sleep 1.0
esc
keys -l "/BETA"; keys Enter; sleep 0.3
keys c w; keys -l "beta"; esc
sleep 1.0
git -C "$REPO" diff --quiet -- src/two.txt || fail "the embed rewrote two.txt's final-newline byte"
keys Tab; sleep 0.3
keys 1; sleep 1.5
frame | grep -qE "two\.txt" && fail "the fully undone file is still listed as changed"
echo "ok 2 - the embed preserves a missing final newline (undone file drops off)"

keys Tab; sleep 0.3
keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all EOL assertions passed"
