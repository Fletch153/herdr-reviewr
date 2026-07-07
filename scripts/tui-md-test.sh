#!/usr/bin/env bash
# Live gate for the markdown view toggle: the [md view] header chip appears for a markdown
# file, p (or clicking the chip) renders it with the built-in viewer OVER the editor grid,
# and p/esc/[raw] returns to the raw buffer. Non-markdown files never offer the chip.
set -uo pipefail
SOCK=rvmd
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

printf '# BigHeading\n\nplain md body\n' > "$REPO/NOTES.md"
printf 'zz base\n' > "$REPO/zz.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
printf '\nmdbodyaddition\n' >> "$REPO/NOTES.md"
printf 'ZZCHANGE\n' >> "$REPO/zz.txt"

tui_start
wait_for "NOTES.md"
wait_for "BigHeading"

# 1. The chip offers the view for a markdown file.
frame | grep -q "\[md view\]" || fail "no [md view] chip for the markdown file under the cursor"
echo "ok 1 - the chip appears for markdown"

# 2. p renders the viewer: raw markdown syntax gone, content still readable, chip flips.
keys p; sleep 0.8
frame | grep -q "# BigHeading" && fail "the preview still shows raw markdown syntax"
frame | grep -q "BigHeading" || fail "the preview lost the heading text"
frame | grep -q "mdbodyaddition" || fail "the preview lost the body"
frame | grep -q "\[raw\]" || fail "the chip did not flip to [raw]"
echo "ok 2 - p renders the markdown view"

# 3. p returns to the raw editor buffer.
keys p; sleep 0.8
wait_for "# BigHeading"
frame | grep -q "\[md view\]" || fail "the chip did not flip back"
echo "ok 3 - p toggles back to raw"

# 4. The chip is clickable both ways.
read -r COL ROW <<< "$(locate '[md view]')"
[ -n "${COL:-}" ] || fail "cannot locate the [md view] chip"
click "$((COL + 2))" "$ROW"; sleep 0.8
frame | grep -q "# BigHeading" && fail "clicking the chip did not open the viewer"
read -r COL ROW <<< "$(locate '[raw]')"
[ -n "${COL:-}" ] || fail "cannot locate the [raw] chip"
click "$((COL + 2))" "$ROW"; sleep 0.8
wait_for "# BigHeading"
echo "ok 4 - the chip toggles by click"

# 5. A non-markdown file: no chip, and p answers with the guard status.
keys j; sleep 0.8
wait_for "ZZCHANGE"
frame | grep -q "\[md view\]" && fail "the chip appeared for a non-markdown file"
keys p; sleep 0.5
frame | grep -q "preview is for markdown files" || fail "p on a non-markdown file gave no guard status"
echo "ok 5 - non-markdown files refuse the view"

keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all markdown-view assertions passed"
