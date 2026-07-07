#!/usr/bin/env bash
# Live gate for the wrap-gap workaround: nvim can't paint 'breakindent' whitespace on wrapped
# rows (neovim/neovim#26392), so a wrapped green line would carry an unhighlighted indent gap.
# The focused view drops breakindent (painted lines wrap edge-to-edge); the plain view hands
# the user's indent back. Config mirrors the reported setup: breakindent + number + listchars.
set -uo pipefail
SOCK=rvwrap
TUI_INIT_EXTRA="vim.o.number = true
vim.o.breakindent = true
vim.o.list = true
vim.opt.listchars = { tab = '> ' }"
source "$(dirname "$0")/tui-lib.sh"

printf 'short base\n' > "$REPO/src/w.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
printf '\tADDED %s tail-end\n' "$(python3 -c "print('x'*180)")" >> "$REPO/src/w.txt"

tui_start
wait_for "w.txt"
wait_for "ADDED"
sleep 0.5

# 1. Changes: the wrapped row continues right after the 6-cell gutter (sign+number) — no
#    unpainted breakindent gap between the gutter and the text.
frame | grep -qE '^│ {6}x' || fail "continuation row still carries a breakindent gap"
frame | grep -E '^│ {6} +x' | grep -q tail-end && fail "continuation row is indented in the focused view"
echo "ok 1 - focused view wraps the added line edge-to-edge"

# 2. ...and that row is actually painted: the DiffAdd background starts at its first text cell.
framee | grep -E 'tail-end' | grep -q $'\x1b\[48;2;0;85;35mx' \
  || fail "continuation row text is not painted DiffAdd green"
echo "ok 2 - the continuation row carries the diff background"

# 3. All files: the user's breakindent comes back (number col 4 + tab indent 8 = 12 cells).
keys 2; sleep 0.8
frame | grep -qE '^│ {12}x' || fail "plain view did not restore the user's breakindent"
echo "ok 3 - plain view restores the user's breakindent"

# 4. Back to Changes: the drop reapplies through the view-sync path, not just first open.
keys 1; sleep 0.8
frame | grep -qE '^│ {6}x' || fail "refocusing did not drop breakindent again"
echo "ok 4 - refocusing drops breakindent again"

keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all wrap-gap assertions passed"
