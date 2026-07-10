#!/usr/bin/env bash
# Live gate: p toggles the markdown preview rendered<->raw from the FILES pane, both ways —
# matching the header hint and the header chip (toggle_md_view). Opening with p keeps focus on
# the files pane (browsing stays live), so a second p must still close it; previously p was a
# one-way ON binding (open_preview) and only a Tab-into-the-diff-then-p or a chip click closed it.
set -uo pipefail
SOCK=rvmdtoggle
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

printf '# Doc\n\nMDBODYX\n' > "$REPO/a.md"
printf 'base\n'           > "$REPO/b.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
printf '\nMDADDX\n' >> "$REPO/a.md"
printf 'BX\n'       >> "$REPO/b.txt"

tui_start
wait_for "a.md"
wait_for "MDBODYX"   # a.md selected, raw view (files pane focused at startup)

# p from the files pane renders.
keys p; sleep 0.8
frame | grep -q "\[md view\]" || fail "p did not open the rendered preview"
frame | grep -q "MDADDX" || fail "the preview did not render the markdown"
echo "ok 1 - p opens the preview from the files pane"

# p again from the files pane must CLOSE it (toggle), returning to raw.
keys p; sleep 0.8
frame | grep -q "\[md view\]" && fail "a second p did not close the preview (one-way toggle)"
frame | grep -q "\[raw\]" || fail "closing the preview did not return the raw chip"
echo "ok 2 - a second p closes the preview from the files pane"

keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all md-toggle assertions passed"
