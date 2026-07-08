#!/usr/bin/env bash
# Live gate: reviewed ticks are INDEPENDENT between the Changes and All files tabs — a file
# ticked in one is not ticked in the other — while a content change still unticks it in BOTH.
# The header renders "<n> reviewed" only when the CURRENT tab's count > 0 (header_suffix), so
# the regex `[0-9]+ reviewed` is the authoritative per-tab signal: the bare ✓ glyph is ambiguous
# (the footer's "✓ checks" uses it too) and the "file reviewed" status has no leading digit.
set -uo pipefail
SOCK=rvrevtick
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

# count_gone: wait until the header shows no per-tab reviewed count (current tab has 0 ticks).
count_gone() { for _ in $(seq 24); do frame | grep -qaE '[0-9]+ reviewed' || return 0; sleep 0.2; done; return 1; }

printf 'line a\nline b\nline c\n' > "$REPO/one.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
printf 'CHANGE ONE\n' >> "$REPO/one.txt"   # one.txt is changed, so it lists in BOTH tabs

tui_start
wait_for "one.txt"
wait_for "CHANGE ONE"

# 1. Tick one.txt in Changes (Enter on its file row marks it reviewed).
read -r COL ROW <<< "$(locate_right 'one.txt')"
[ -n "${COL:-}" ] || fail "cannot locate one.txt in the Changes list"
click "$COL" "$ROW"; sleep 0.4
keys Enter
wait_for "1 reviewed"
echo "ok 1 - one.txt ticked in Changes (1 reviewed)"

# 2. Switch to All files: the SAME file is NOT ticked here — the tabs are independent.
keys 2
wait_for "one.txt"
count_gone || fail "the Changes tick leaked into All files (a reviewed count is shown)"
echo "ok 2 - the Changes tick does not appear in All files"

# 3. Tick one.txt in All files too; All files now carries its own independent tick.
read -r COL ROW <<< "$(locate_right 'one.txt')"
[ -n "${COL:-}" ] || fail "cannot locate one.txt in the All files list"
click "$COL" "$ROW"; sleep 0.4
keys Enter
wait_for "1 reviewed"
echo "ok 3 - one.txt ticked separately in All files"

# 4. Back in Changes: its own tick is intact, untouched by the All files tick.
keys 1
wait_for "one.txt"
wait_for "1 reviewed"
echo "ok 4 - the Changes tick is still set, independent of All files"

# 5. A content change unticks it in BOTH tabs (each stored the pre-change content hash).
printf 'POST-REVIEW CHANGE\n' >> "$REPO/one.txt"
count_gone || fail "the Changes tick outlived a content change"
keys 2
wait_for "one.txt"
count_gone || fail "the All files tick outlived a content change"
echo "ok 5 - a content change unticks it in both tabs"

# Focus is on the files pane (last action was a tab switch, never into the editor) — q quits.
keys q; sleep 0.4; keys y 2>/dev/null || true
wait_session_end
echo "# all reviewed-tick per-tab assertions passed"
