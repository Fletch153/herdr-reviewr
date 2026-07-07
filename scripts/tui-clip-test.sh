#!/usr/bin/env bash
# Live gate for clipboard plumbing: a "+ yank inside the embed reaches the terminal clipboard
# as OSC 52 through the host (tmux captures it into a buffer here), returns instantly (no
# "waiting for OSC 52 response" hang), and the host's own ry comment export takes the same
# path when no clipboard tool is present.
set -uo pipefail
SOCK=rvclip
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

printf 'line a\nCLIPTARGET base\nline c\n' > "$REPO/src/one.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
sed -i 's/^CLIPTARGET base$/CLIPTARGET CHANGED/' "$REPO/src/one.txt"

tui_start
$TMUX set -s set-clipboard on
wait_for "one.txt"
wait_for "CLIPTARGET CHANGED"

# 1. "+yy in the embed lands in the terminal clipboard via the host — and does not hang.
keys Tab; sleep 0.4
keys -l '"+yy'
ok=""
for _ in $(seq 20); do
  if $TMUX show-buffer 2>/dev/null | grep -q "CLIPTARGET CHANGED"; then ok=1; break; fi
  sleep 0.25
done
[ -n "$ok" ] || fail "the + yank never reached the terminal clipboard (tmux buffer empty)"
frame | grep -qi "waiting for OSC" && fail "the embed still queries the terminal for OSC 52"
echo "ok 1 - a + yank reaches the terminal clipboard instantly"

# 2. The host's ry (copy all comments) rides the same OSC 52 path.
keys Space r c; sleep 0.3
keys -l "clipnote for the gate"
keys Enter
wait_for "clipnote for the gate"
keys Space r y; sleep 1
$TMUX show-buffer 2>/dev/null | grep -q "clipnote for the gate" || fail "ry did not reach the terminal clipboard"
echo "ok 2 - the host comment export copies via OSC 52"

keys Tab; sleep 0.3
keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all clipboard assertions passed"
