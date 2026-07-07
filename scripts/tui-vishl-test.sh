#!/usr/bin/env bash
# Live gate (from probe c2/run2 p1, matrix 13×timing): a visual-mode highlight vs a
# user-driven focus/tab switch. A visual selection lives only while the editor is the
# input target; handing that target away (Tab to the files pane, or a tab switch — which
# must Tab out first, since 1/2/3 forward to nvim while the editor is focused) must drop
# the editor back to normal. Left in visual mode it persists behind the host's back: a
# returning Tab, or a same-file tab switch that republishes in place (sync_view never
# leaves visual), lands right back in the selection and the next j/k extends it.
# Contracts:
#  (1) Tab out of a visual selection -> the editor is normal (no VISUAL indicator).
#  (2) Tab back in with NO tab switch -> still normal (the round trip cleared it once).
#  (3) A tab switch out and back (Changes->All files->Changes, same file, the in-place
#      sync_view path) -> no stale VISUAL leaks into either view; the Changes file returns.
#  (4) Back in the editor, j/k NAVIGATE (they don't silently extend a stale selection):
#      the VISUAL indicator never reappears from a plain motion.
set -uo pipefail
SOCK=rvvishl
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

# xx.txt is dirty -> it lists in Changes AND in All files; a Changes<->All files switch
# lands on the SAME path, exercising the in-place sync_view republish that never :edits.
{ for i in $(seq 1 20); do printf 'xx line %02d\n' "$i"; done; } > "$REPO/xx.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm base
printf 'XXCHANGE\n' >> "$REPO/xx.txt"

tui_start
wait_for "xx.txt"
wait_for "XXCHANGE"

# (1) Enter Visual Line over three rows in the Changes editor, then Tab out.
keys Tab; sleep 0.5            # focus the nvim editor
keys V j j
wait_for "VISUAL LINE"        # the selection is genuinely live (guards a vacuous test)
keys Tab                      # hand focus to the files pane
wait_gone "VISUAL LINE" || fail "Tab-out left the editor stuck in visual mode"
echo "ok 1 - Tab-out of a visual selection drops the editor to normal"

# (2) Tab straight back in — no tab switch — and confirm it did not resurrect the selection.
keys Tab; sleep 0.5
frame | grep -qF -- "-- VISUAL LINE --" && fail "Tab back in resurrected the visual selection"
echo "ok 2 - a Tab round trip does not resurrect the selection"

# (3) Re-select, Tab out, switch to All files (same file, in-place republish), and back.
keys V j j
wait_for "VISUAL LINE"
keys Tab                      # to files pane (drops visual per contract 1)
wait_gone "VISUAL LINE"
keys 2                        # All files — republishes xx.txt in place via sync_view
wait_for "XXCHANGE"
frame | grep -qF -- "-- VISUAL LINE --" && fail "a stale visual highlight leaked into All files"
keys 1                        # back to Changes
wait_for "XXCHANGE"
frame | grep -qF -- "-- VISUAL LINE --" && fail "a stale visual highlight leaked back into Changes"
echo "ok 3 - a tab switch out and back leaks no stale highlight, restores the file"

# (4) Back in the editor, a plain j/k must navigate, never extend a resurrected selection.
keys Tab; sleep 0.5
keys j j
sleep 0.4
frame | grep -qF -- "-- VISUAL LINE --" && fail "j/k after the round trip extended a stale selection"
echo "ok 4 - j/k navigate normally after the highlight round trip"

keys Tab; sleep 0.3           # files pane: q is the reviewer quit, not a macro record
keys q; sleep 0.3
keys y 2>/dev/null || true
wait_session_end
echo "# visual-highlight vs tab-switch probe complete"
