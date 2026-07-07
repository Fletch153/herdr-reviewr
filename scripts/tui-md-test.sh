#!/usr/bin/env bash
# Live gate for the sticky markdown view: the header chip names the view CURRENTLY showing
# ([raw] / [md view]) and toggles on click or p; while the view is on it follows the file
# selection (markdown files render, non-markdown files show the editor, the preference
# survives the detour), and the file list stays fully navigable — nothing goes modal.
set -uo pipefail
SOCK=rvmd
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

printf '# BigHeading\n\nplain md body\n' > "$REPO/ANOTES.md"
printf '# ZotherHead\n\nzother body\n' > "$REPO/BNOTES.md"
printf 'zz base\n' > "$REPO/zz.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
printf '\nmdbodyaddition\n' >> "$REPO/ANOTES.md"
printf '\nzotheraddition\n' >> "$REPO/BNOTES.md"
printf 'ZZCHANGE\n' >> "$REPO/zz.txt"

tui_start
wait_for "ANOTES.md"
wait_for "BigHeading"

# 1. The chip names the current view: raw at first.
frame | grep -q "\[raw\]" || fail "no [raw] chip while the raw file shows"
echo "ok 1 - the chip names the raw view"

# 2. p renders: markdown syntax gone, content readable, chip flips to the rendered state.
keys p; sleep 0.8
frame | grep -q "# BigHeading" && fail "the rendered view still shows raw markdown syntax"
frame | grep -q "BigHeading" || fail "the rendered view lost the heading text"
frame | grep -q "mdbodyaddition" || fail "the rendered view lost the body"
frame | grep -q "\[md view\]" || fail "the chip did not flip to [md view]"
echo "ok 2 - p renders and the chip names the view"

# 3. STICKY + LIVE: j selects the next markdown file — it renders too, no modal freeze.
keys j; sleep 1
frame | grep -q "# ZotherHead" && fail "the next markdown file arrived raw (view not sticky)"
frame | grep -q "ZotherHead" || fail "the next markdown file did not render"
frame | grep -q "zotheraddition" || fail "the next file's body is missing from the render"
echo "ok 3 - the rendered view follows the selection"

# 4. A non-markdown file shows the editor as normal; the chip hides; the preference stays.
keys j; sleep 1
wait_for "ZZCHANGE"
frame | grep -q "\[md view\]" && fail "the chip showed for a non-markdown file"
keys k; sleep 1
frame | grep -q "# ZotherHead" && fail "the markdown view did not survive the non-md detour"
frame | grep -q "ZotherHead" || fail "returning to a markdown file lost the render"
echo "ok 4 - non-markdown files pass through; the preference survives"

# 5. The chip toggles by click, both ways.
read -r COL ROW <<< "$(locate '[md view]')"
[ -n "${COL:-}" ] || fail "cannot locate the [md view] chip"
click "$((COL + 2))" "$ROW"; sleep 0.8
frame | grep -q "# ZotherHead" || fail "clicking the chip did not return to raw"
read -r COL ROW <<< "$(locate '[raw]')"
[ -n "${COL:-}" ] || fail "cannot locate the [raw] chip"
click "$((COL + 2))" "$ROW"; sleep 0.8
frame | grep -q "# ZotherHead" && fail "clicking the [raw] chip did not render"
echo "ok 5 - the chip toggles by click"

# 6. Esc from the diff pane leaves the rendered view.
keys Tab; sleep 0.4
esc; sleep 0.5
frame | grep -q "# ZotherHead" || fail "esc in the rendered pane did not return to raw"
keys Tab; sleep 0.4
echo "ok 6 - esc closes from the pane"

# 7. p on a non-markdown file answers with the guard status.
keys j; sleep 0.8
wait_for "ZZCHANGE"
keys p; sleep 0.5
frame | grep -q "markdown view is for markdown files" || fail "p on a non-markdown file gave no guard status"
echo "ok 7 - non-markdown files refuse the view"

keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all markdown-view assertions passed"
