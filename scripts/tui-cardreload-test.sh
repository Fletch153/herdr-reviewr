#!/usr/bin/env bash
# Live gate: an inline comment CARD survives an agent write to the open file. The store already
# survives (the SURVIVAL invariant, covered by tui-anchor). But an agent write triggers a
# checktime reload that clears ALL extmarks in the buffer — diff.lua repaints its own diff marks
# from its reload autocmd, so the comment card layer must repaint too, WITHOUT a tab flip.
set -uo pipefail
SOCK=rvcardreload
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

{
  for i in $(seq 1 30); do
    case $i in
      15) echo CBASE15 ;;      # becomes the comment's anchored (changed, visible) line
      *)  printf 'c%02d\n' "$i" ;;
    esac
  done
} > "$REPO/src/one.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm base
sed -i 's/^CBASE15$/CANCHOR/' "$REPO/src/one.txt"   # the anchored line is itself the hunk (visible)

tui_start
wait_for "one.txt"; wait_for "CANCHOR"

# Comment on CANCHOR in the Changes editor; the card must paint.
keys Tab; sleep 0.4
keys Escape; sleep 0.2
keys -l '/CANCHOR'; keys Enter; sleep 0.3
keys Space r c; sleep 0.4
keys -l 'CARDNOTE'; keys Enter; sleep 0.6
wait_for "╭─ comment"
frame | grep -qF "CARDNOTE" || fail "setup: the comment card did not paint on CANCHOR"
wait_for "Send (1)"
echo "ok setup - comment card on CANCHOR, Send (1)"

# An agent appends far from the anchor; the host poll live-reloads the buffer (size change).
printf 'AGENTWRITE\n' >> "$REPO/src/one.txt"
wait_for "AGENTWRITE"
frame | grep -qF "Send (1)" || fail "the agent write dropped the comment from the store"

# WITHOUT any tab flip, the card must still be on screen (repainted after the reload).
sleep 0.6
frame | grep -qF "CARDNOTE" || fail "the comment card vanished after the agent-write reload (no repaint)"
echo "ok 1 - the comment card survives an agent-write reload without a re-present"

keys Escape; sleep 0.2
keys Tab; sleep 0.3
keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all card-reload assertions passed"
