#!/usr/bin/env bash
# Live gate for the editor death/respawn lifecycle: :qa! inside nvim, the dead panel,
# auto-respawn on the next open, manual r restart, and an external kill raced by a comment
# jump — with the file, base decorations, comment cards, the palette-matched card theme, and
# pending work (the jump's goto) surviving each path; no orphaned embeds.
set -uo pipefail
SOCK=rvdeath
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"
NVIM_BEFORE=$(pgrep -c -f 'nvim --embed' 2>/dev/null || true); NVIM_BEFORE=${NVIM_BEFORE:-0}

printf 'aaa\nbbb\nccc\n' > "$REPO/src/one.txt"
printf 'xxx\nyyy\n' > "$REPO/src/two.txt"
# Tall many-hunk file for step 4: its focused view outsizes the grid, so the file top and the
# last hunk are never on screen together (sorts after one/two — the early steps stay put).
seq -f 'd%g' 400 > "$REPO/src/zdeep.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
printf 'CHANGE ONE\n' >> "$REPO/src/one.txt"
printf 'CHANGE TWO\n' >> "$REPO/src/two.txt"
sed -i 's/^d5$/MARKTOP/' "$REPO/src/zdeep.txt"
for n in $(seq 20 13 380); do sed -i "s/^d$n\$/edit$n/" "$REPO/src/zdeep.txt"; done
sed -i 's/^d400$/MARKDEEP/' "$REPO/src/zdeep.txt"

PEACH="38;2;250;179;135" # the default palette's peach — the card title accent
card_is_themed() { framee | grep "comment ·" | grep -q "$PEACH"; }

tui_start
wait_for "one.txt"
wait_for "CHANGE ONE"
keys Tab; sleep 0.4
keys Space r c; sleep 0.3
keys -l "survives death"; keys Enter
wait_for "╭─ comment"
card_is_themed || fail "card not themed on first paint"
echo "ok 0 - themed card before any death"

# 1. Kill the editor from inside (:qa!) → the dead panel shows.
keys -l ":qa!"; keys Enter
wait_for "the editor (nvim) exited"
echo "ok 1 - dead panel on :qa!"

# 2. Selecting a file → one automatic respawn: file opens, cards re-push, theme holds.
keys Tab; sleep 0.3
keys j
wait_for "CHANGE TWO"
wait_gone "the editor (nvim) exited"
keys k
wait_for "╭─ comment"
wait_for "survives death"
card_is_themed || fail "card lost the reviewer theme after auto-respawn"
echo "ok 2 - auto-respawn restores file, card, and theme"

# 3. Kill again; recover via the dead panel's manual r this time.
keys Tab; sleep 0.3
keys -l ":qa!"; keys Enter
wait_for "the editor (nvim) exited"
keys r
wait_gone "the editor (nvim) exited"
wait_for "CHANGE ONE"
wait_for "╭─ comment"
card_is_themed || fail "card lost the reviewer theme after manual r restart"
echo "ok 3 - manual restart restores file, card, and theme"

# 3b. The respawned Changes view is locked again (read-only review surface). Focus is still
#     on the editor pane — the dead panel r was pressed there.
keys x; sleep 0.5
frame | grep -q "E21" || fail "the respawned Changes buffer accepted an edit"
echo "ok 3b - respawn re-applies the read-only lock"

# 4. A comment jump racing a respawn: kill the editor from OUTSIDE (agent crash flavor) with
#    the comments list open, then jump to a comment deep in a taller-than-screen file. The
#    respawn must re-publish the open BEFORE delivering the jump — the goto lands on the
#    comment's line (MARKDEEP visible), never fired into the fresh editor's empty buffer and
#    lost (which would leave the view at the file top).
keys Tab; sleep 0.3
keys j j                             # files-pane cursor: one -> two -> zdeep (auto-opens)
wait_for "MARKTOP"
keys Tab; sleep 0.3                  # focus the editor
keys G; sleep 0.5                    # cursor on the last hunk (line 400 = MARKDEEP)
keys Space r c; sleep 0.3
keys -l "deep jump target"; keys Enter
wait_for "comment added"             # the card itself renders below the last line, off-screen
keys Space r l
wait_for "Comments (2)"
keys j                               # the deep comment (store order: after "survives death")
PANE_PID=$($TMUX list-panes -t0 -F '#{pane_pid}')
NVPID=$(pgrep -P "$PANE_PID" nvim || true)
[ -n "$NVPID" ] || fail "no embedded nvim child to kill"
kill -9 "$NVPID"; sleep 1.2          # death noticed on the next tick, list still open
keys Enter                           # jump_to_comment -> goto + auto-respawn in one frame
wait_for "MARKDEEP"
frame | grep -qF "MARKTOP" && fail "the respawn dropped the jump (view sits at the file top)"
card_is_themed || fail "card lost the reviewer theme after the jump-triggered respawn"
echo "ok 4 - a comment jump that triggers the respawn still lands on its line"

# 5. Quit; no orphaned embeds.
keys Tab; sleep 0.3; keys q; sleep 0.3; keys y 2>/dev/null || true
for _ in $(seq 40); do $TMUX has-session 2>/dev/null || break; sleep 0.25; done
NVIM_AFTER=$(pgrep -c -f 'nvim --embed' 2>/dev/null || true); NVIM_AFTER=${NVIM_AFTER:-0}
[ "$NVIM_AFTER" -le "$NVIM_BEFORE" ] || fail "orphaned nvim ($NVIM_BEFORE -> $NVIM_AFTER)"
echo "# all death/respawn assertions passed"
