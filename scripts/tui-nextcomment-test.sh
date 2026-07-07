#!/usr/bin/env bash
# Live gate: the header "next comment" button walks comments ACROSS files, moving BOTH the editor
# buffer and the file-list selection together. The editor owns n/N (its own search), so in nvim
# mode the button is the surface for cross-file comment navigation: at a file's last comment it
# advances to the next file that carries a comment, opening it and moving the sidebar selection,
# and it wraps from the last file back to the first. Regression lock for the user-requested
# feature (a walk that leaves the current file, unlike jump_comment's within-file n/N).
set -uo pipefail
SOCK="nextc$$"
export TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

printf 'a1\na2\na3\n' > "$REPO/src/alpha.txt"
printf 'b1\nb2\nb3\n' > "$REPO/src/bravo.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm base
printf 'ALPHA_MARK\n' >> "$REPO/src/alpha.txt"   # one added line per file — the comment anchor
printf 'BRAVO_MARK\n' >> "$REPO/src/bravo.txt"

# The selected file-list row fills edge-to-edge with the cursor color (surface2 focused, surface1
# when the editor holds focus). Isolate the bare list row — the name also appears in the editor
# title (src/<name>) and any :e cmdline — and match either fill as a background (48;2; prefix).
SELFILL='48;2;(69;71;90|88;91;112)'
list_row() { framee | grep -aF "$1" | grep -avF "src/$1" | grep -avF ':e '; }
row_selected() { list_row "$1" | grep -qaE "$SELFILL"; }

# "on file N" = the editor shows that file's content marker AND the sidebar highlights its row.
# Content markers (grep the frame), never file names — the name shows in both panes.
on_alpha() { frame | grep -qF ALPHA_MARK && row_selected alpha.txt; }
on_bravo() { frame | grep -qF BRAVO_MARK && row_selected bravo.txt; }
wait_alpha() { for _ in $(seq 16); do on_alpha && return 0; sleep 0.25; done; return 1; }
wait_bravo() { for _ in $(seq 16); do on_bravo && return 0; sleep 0.25; done; return 1; }

click_next() {
  read -r C R < <(locate 'next comment')
  [ -n "${C:-}" ] || fail "the next-comment button is not visible in the header"
  click "$C" "$R"
}

tui_start
wait_for "alpha.txt"
wait_for "ALPHA_MARK"                           # Changes tab (default) opens the first file, alpha
row_selected alpha.txt || fail "the list did not start with alpha selected"

# --- seed a comment on each file --------------------------------------------------------------
keys Tab; sleep 0.4                             # focus the editor
esc
keys -l 'G'; sleep 0.2                          # cursor on the added line (ALPHA_MARK)
keys Space r c; sleep 0.4
keys -l 'ANOTE'; keys Enter; sleep 0.5
wait_for "╭─ comment"
frame | grep -qF "ANOTE" || fail "the comment on alpha did not paint"

esc
keys -l ':e src/bravo.txt'; keys Enter; sleep 0.8
wait_bravo || fail "opening bravo did not move the editor and sidebar together"
keys -l 'G'; sleep 0.2
keys Space r c; sleep 0.4
keys -l 'BNOTE'; keys Enter; sleep 0.5
wait_for "BNOTE" || fail "the comment on bravo did not paint"
echo "ok - a comment exists on file 1 (alpha) and file 2 (bravo)"

# --- normalize onto file 1 via the button. Setup leaves us on bravo, so the loop always clicks
# at least once; reaching alpha is a CROSS-file jump, which lands the cursor on alpha's comment
# (deterministic start for the assertions below). Within-file steps may intervene; bounded. ------
reached=0
for _ in 1 2 3 4; do
  on_alpha && { reached=1; break; }
  click_next
  wait_alpha && { reached=1; break; }
done
[ "$reached" = 1 ] || fail "the next-comment button never reached file 1 (alpha)"

# --- A: from file 1's comment, one click advances to file 2 — editor buffer AND sidebar ---------
click_next
wait_bravo || fail "A: next-comment did not advance from file 1 to file 2 (editor + sidebar)"
on_alpha && fail "A: the editor/sidebar are still on file 1 after advancing"
echo "ok A - the next-comment button walks file 1 -> file 2 (editor buffer + sidebar selection)"

# --- B: from file 2 (the last file), the next click wraps back to file 1 -----------------------
click_next
wait_alpha || fail "B: next-comment did not wrap from the last file back to file 1"
echo "ok B - the next-comment button wraps from the last file back to file 1"

# --- C: and forward again re-advances to file 2, confirming a stable cycle ---------------------
click_next
wait_bravo || fail "C: next-comment did not advance file 1 -> file 2 on the second lap"
echo "ok C - the next-comment button cycles file 1 <-> file 2 deterministically"

# The walk leaves the editor focused, so hand focus back to the files pane before quitting —
# a bare `q` to nvim would start a macro recording, not quit the host. `esc` debounces so the
# Escape can't merge with the following Tab into an Alt-chord.
esc
keys Tab; sleep 0.4
keys q; sleep 0.3; keys y 2>/dev/null || true
wait_session_end
echo "# all next-comment cross-file walk assertions passed"
