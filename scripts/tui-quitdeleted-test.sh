#!/usr/bin/env bash
# Live gate: quitting while a deleted-file (show_deleted) scratch is on screen must quit
# immediately, not mis-prompt ConfirmQuit. The scratch is a read-only nofile presentation
# buffer that set_lines marks 'modified'; it must not be counted as unsaved work.
set -uo pipefail
SOCK=rvquitdel
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

printf 'gone body\n' > "$REPO/gone.txt"
printf 'keep body\n' > "$REPO/keep.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
rm "$REPO/gone.txt"                    # gone.txt is a deletion in the changeset
printf 'KEEPCHANGE\n' >> "$REPO/keep.txt"

tui_start
wait_for "gone.txt"

# Select the deleted file so the editor shows its all-red show_deleted scratch.
read -r CP RP <<< "$(locate_right 'gone.txt')"
[ -n "${CP:-}" ] || fail "gone.txt row not found"
click "$CP" "$RP"; sleep 0.6

# q from the files pane: nothing is genuinely unsaved, so it must end the session with no
# intermediate 'y/↵ quit · esc cancel' confirm prompt.
frame | grep -q "quit · esc cancel" && fail "confirm prompt was already showing"
keys q
for _ in $(seq 12); do $TMUX has-session 2>/dev/null || break; sleep 0.25; done
if $TMUX has-session 2>/dev/null; then
  frame | grep -q "quit · esc cancel" && { keys y 2>/dev/null; fail "quitting from a deleted-file view mis-prompted ConfirmQuit"; }
  keys y 2>/dev/null || true
  fail "the session did not end on a single q from a deleted-file view"
fi
echo "ok 1 - q from a deleted-file view quits immediately, no spurious ConfirmQuit"
echo "# all quit-deleted assertions passed"
