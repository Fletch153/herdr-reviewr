#!/usr/bin/env bash
# Live gate for the editor death/respawn lifecycle: :qa! inside nvim, the dead panel,
# auto-respawn on the next open, manual r restart — with the file, base decorations, comment
# cards, and the palette-matched card theme surviving each path; no orphaned embeds.
set -uo pipefail
SOCK=rvdeath
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"
NVIM_BEFORE=$(pgrep -c -f 'nvim --embed' 2>/dev/null || true); NVIM_BEFORE=${NVIM_BEFORE:-0}

printf 'aaa\nbbb\nccc\n' > "$REPO/src/one.txt"
printf 'xxx\nyyy\n' > "$REPO/src/two.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
printf 'CHANGE ONE\n' >> "$REPO/src/one.txt"
printf 'CHANGE TWO\n' >> "$REPO/src/two.txt"

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

# 4. Quit; no orphaned embeds.
keys Tab; sleep 0.3; keys q; sleep 0.3; keys y 2>/dev/null || true
for _ in $(seq 40); do $TMUX has-session 2>/dev/null || break; sleep 0.25; done
NVIM_AFTER=$(pgrep -c -f 'nvim --embed' 2>/dev/null || true); NVIM_AFTER=${NVIM_AFTER:-0}
[ "$NVIM_AFTER" -le "$NVIM_BEFORE" ] || fail "orphaned nvim ($NVIM_BEFORE -> $NVIM_AFTER)"
echo "# all death/respawn assertions passed"
