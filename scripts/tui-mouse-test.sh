#!/usr/bin/env bash
# Human-style mouse probe: click files, click the grid, wheel both panes, header buttons.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/debug/herdr-reviewr"
TMUX="tmux -L rvmouse"
TMP="$(mktemp -d)"
trap '$TMUX kill-server 2>/dev/null || true; rm -rf "$TMP"' EXIT
REPO="$TMP/repo"; mkdir -p "$REPO/src"
git -C "$REPO" init -qb main
git -C "$REPO" config user.email t@t; git -C "$REPO" config user.name t; git -C "$REPO" config commit.gpgsign false
{ for i in $(seq 1 60); do printf 'alpha line %02d\n' "$i"; done; } > "$REPO/src/long.txt"
printf 'shorty one\n' > "$REPO/src/zz_short.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
printf 'CHANGED HEAD\n' >> "$REPO/src/long.txt"
printf 'CHANGED SHORT\n' >> "$REPO/src/zz_short.txt"
export HERDR_BIN_PATH="$ROOT/nvim/tests/stub_herdr.sh" REVIEWR_STUB_LOG="$TMP/stub.log"
export HERDR_PANE_ID=wY:pSIDEBAR HERDR_TAB_ID=wY:t1 HERDR_WORKSPACE_ID=wY
export HERDR_PLUGIN_CONTEXT_JSON='{"focused_pane_id":"wY:pFOCUS"}'
export HERDR_PLUGIN_ROOT="$ROOT"
export XDG_CONFIG_HOME="$TMP/xdg" XDG_DATA_HOME="$TMP/xdgd" XDG_STATE_HOME="$TMP/xdgs"
mkdir -p "$XDG_CONFIG_HOME/nvim"; printf "vim.g.mapleader = ' '\nvim.o.number = true\n" > "$XDG_CONFIG_HOME/nvim/init.lua"
$TMUX new-session -d -x 200 -y 50 "cd '$REPO' && exec '$BIN' --editor nvim --poll 500"
keys() { $TMUX send-keys -t0 "$@"; }
frame() { $TMUX capture-pane -pt0; }
wait_for() { for _ in $(seq 60); do frame | grep -qF "$1" && return 0; sleep 0.25; done; echo "TIMEOUT waiting: $1"; frame; exit 1; }
wait_gone() { for _ in $(seq 60); do frame | grep -qF "$1" || return 0; sleep 0.25; done; echo "STUCK: $1"; frame; exit 1; }
# SGR mouse: button 0 press/release, 64/65 wheel up/down. col;row are 1-based.
click() { keys -l "$(printf '\033[<0;%d;%dM' "$1" "$2")"; sleep 0.15; keys -l "$(printf '\033[<0;%d;%dm' "$1" "$2")"; }
wheel_down() { keys -l "$(printf '\033[<65;%d;%dM' "$1" "$2")"; }
# Find (col,row) of the first occurrence of $1 in the frame (1-based).
locate() {
  frame | awk -v pat="$1" 'BEGIN{IGNORECASE=0} { i = index($0, pat); if (i>0) { print i, NR; exit } }'
}
# Match only in the FILE-LIST pane (right of the pane divider) — list-row text also appears
# in the editor pane (title, statusline). Cell coordinates are unicode-aware via python.
locate_right() {
  frame | python3 -c "
import sys
pat = sys.argv[1]
for nr, line in enumerate(sys.stdin.read().splitlines(), 1):
    div = line.find('\u2502\u2502')
    if div < 0:
        continue
    i = line.find(pat, div + 2)
    if i >= 0:
        print(i + 1, nr)
        break
" "$1"
}

wait_for "long.txt"
wait_for "CHANGED HEAD"

# 1. Click the zz_short.txt row in the file list → it opens in the editor.
read -r C R <<< "$(locate_right "zz_short.txt")"
[ -n "${C:-}" ] || { echo "FAIL: cannot locate zz_short row"; frame; exit 1; }
click "$C" "$R"
wait_for "shorty one"
echo "PASS 1: clicking a file row opens it"

# 2. Click inside the grid → focus moves to the editor (footer hint flips).
read -r C2 R2 <<< "$(locate "shorty one")"
click "$C2" "$R2"
wait_for "keys go to the editor"
echo "PASS 2: grid click focuses the editor"

# 3. Back to the files pane (tab keys live there in nvim mode), All files, open the long
#    file (the tree auto-reveals the open file, so rows are already visible), then wheel over
#    the grid: the plain view scrolls.
keys Tab; sleep 0.3
keys 2; sleep 0.8
# The All-files tree starts collapsed: clicking the dir row expands it (also a mouse-path
# assertion in its own right), then the file row appears.
read -r CD RD <<< "$(locate_right "src/")"
[ -n "${CD:-}" ] || { echo "FAIL: no src/ dir row on All files"; frame; exit 1; }
click "$CD" "$RD"
wait_for "long.txt"
echo "PASS 3a: clicking the dir row expands the tree"
read -r CL RL <<< "$(locate_right "long.txt")"
[ -n "${CL:-}" ] || { echo "FAIL: no long.txt row after expansion"; frame; exit 1; }
click "$CL" "$RL"
# Vim buffer-cursor memory: the file was auto-opened at startup with its change at EOF, so
# the plain reopen restores that spot — the view sits near the bottom. Wheel UP to scroll.
wait_for "alpha line 2"
read -r CG RG <<< "$(locate "alpha line 2")"
for _ in 1 2 3 4 5 6 7 8; do keys -l "$(printf '\033[<64;%d;%dM' "$CG" "$RG")"; sleep 0.1; done
wait_for "alpha line 03"
echo "PASS 3: wheel over the grid scrolls the editor"

# 4. Header: click the Send button with no comments → status says so.
read -r CS RS <<< "$(locate "Send (0)")"
click "$CS" "$RS"
wait_for "no comments to send"
echo "PASS 4: header send click routes to the host store"

# 5. Click the scope chip cycles the scope label.
read -r CC RC <<< "$(locate "[commit]")"
if [ -n "${CC:-}" ]; then
  click "$CC" "$RC"
  wait_gone "[commit]"
  echo "PASS 5: scope chip click cycles the scope"
else
  echo "SKIP 5: no [commit] chip visible"
  frame | head -2
fi

keys q; sleep 0.3; keys y 2>/dev/null || true
echo "MOUSE PROBE PASSED"
