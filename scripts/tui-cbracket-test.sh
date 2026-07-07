#!/usr/bin/env bash
# Live gate for jump-back in the embedded editor. Ctrl+] jumps into a definition; the pair —
# Ctrl+o (native) and Ctrl+[ (kitty-distinct) — must jump back. The host must forward Ctrl+[
# as a raw <C-[> termcode, not the <>-notation nvim_input flattens straight back to Esc, so a
# user's `<C-[>` map fires. And plain Esc must NOT be mistaken for Ctrl+[ (the classic embed
# trap: a bare `<C-[>` map also matches <Esc> unless an <Esc> map shadows it). The fixture
# below mirrors a real config: `<C-[>` -> `<C-o>` plus an `<Esc>` shadow.
set -uo pipefail
SOCK="cbracket$$"
export TUI_INIT_EXTRA=$'vim.keymap.set("n","<C-[>","<C-o>",{})\nvim.keymap.set("n","<Esc>","<cmd>nohlsearch<CR>",{})'
source "$(dirname "$0")/tui-lib.sh"

# One tall, fully-changed file: a jumplist entry lands at the top after we leap to the bottom.
seq -f 'old%03g' 1 200 > "$REPO/src/tall.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm base
seq -f 'TOPLINE%03g' 1 199 > "$REPO/src/tall.txt"
echo 'BOTTOMMARKER' >> "$REPO/src/tall.txt"

tui_start
wait_for "TOPLINE001"

# 1. Ctrl+[ (kitty CSI-u, raw bytes) jumps back to the top after G — proving the key reaches
#    nvim distinct from Esc, so the user's `<C-[>` map fires.
keys Tab; sleep 0.4                            # focus the editor
keys G; sleep 0.6
frame | grep -q "BOTTOMMARKER" || fail "G did not reach the bottom"
keys -l "$(printf '\033[91;5u')"; sleep 0.6    # Ctrl+[ kitty-encoded
frame | grep -q "TOPLINE001" || fail "Ctrl+[ did not jump back to the top of the jumplist"
echo "ok 1 - Ctrl+[ jumps back (distinct from Esc)"

# 2. Ctrl+o makes the same jump (the always-available native fallback).
keys G; sleep 0.6
frame | grep -q "BOTTOMMARKER" || fail "G did not reach the bottom (second leap)"
keys C-o; sleep 0.6
frame | grep -q "TOPLINE001" || fail "Ctrl+o did not jump back"
echo "ok 2 - Ctrl+o jumps back"

# 3. Plain Esc must NOT jump — it hits its own <Esc> map (nohlsearch), leaving the view put.
#    A jump here would mean Esc was misrouted as Ctrl+[ (the distinction was lost).
keys G; sleep 0.6
frame | grep -q "BOTTOMMARKER" || fail "G did not reach the bottom (third leap)"
keys Escape; sleep 0.5
frame | grep -q "BOTTOMMARKER" || fail "Esc was misrouted as Ctrl+[ and jumped the view"
echo "ok 3 - Esc stays Esc (no jump)"

# Clean quit — focus the file list, then q.
keys Tab; sleep 0.3
keys q
wait_session_end
echo "# all jump-back assertions passed"
