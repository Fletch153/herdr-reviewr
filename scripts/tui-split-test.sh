#!/usr/bin/env bash
RC=0
# Human-style probe: space rd split diff inside the embed (open, read, close, recover),
# divider drag resize, and the help overlay.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/debug/herdr-reviewr"
TMUX="tmux -L rvsplit"
TMP="$(mktemp -d)"
command -v tmux >/dev/null || { echo "SKIP: tmux not installed"; exit 0; }
command -v nvim >/dev/null || { echo "SKIP: nvim not installed"; exit 0; }
(cd "$ROOT" && cargo build 2>/dev/null) || { echo "FAIL: cargo build"; exit 1; }
trap '$TMUX kill-server 2>/dev/null || true; rm -rf "$TMP"' EXIT
REPO="$TMP/repo"; mkdir -p "$REPO/src"
git -C "$REPO" init -qb main; git -C "$REPO" config user.email t@t; git -C "$REPO" config user.name t; git -C "$REPO" config commit.gpgsign false
{ for i in $(seq 1 10); do printf 'row %02d ORIGINAL\n' "$i"; done; } > "$REPO/src/f.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
sed -i 's/row 05 ORIGINAL/row 05 REWRITTEN/' "$REPO/src/f.txt"
export HERDR_BIN_PATH="$ROOT/nvim/tests/stub_herdr.sh" REVIEWR_STUB_LOG="$TMP/stub.log"
export HERDR_PANE_ID=wY:pSIDEBAR HERDR_TAB_ID=wY:t1 HERDR_WORKSPACE_ID=wY
export HERDR_PLUGIN_CONTEXT_JSON='{"focused_pane_id":"wY:pFOCUS"}' HERDR_PLUGIN_ROOT="$ROOT"
export XDG_CONFIG_HOME="$TMP/x" XDG_DATA_HOME="$TMP/xd" XDG_STATE_HOME="$TMP/xs"
mkdir -p "$XDG_CONFIG_HOME/nvim"; printf "vim.g.mapleader = ' '\n" > "$XDG_CONFIG_HOME/nvim/init.lua"
$TMUX new-session -d -x 200 -y 50 "cd '$REPO' && exec '$BIN' --editor nvim --poll 500"
keys() { $TMUX send-keys -t0 "$@"; }
frame() { $TMUX capture-pane -pt0; }
wait_for() { for _ in $(seq 60); do frame | grep -qF "$1" && return 0; sleep 0.25; done; echo "TIMEOUT: $1"; frame; exit 1; }
wait_gone() { for _ in $(seq 60); do frame | grep -qF "$1" || return 0; sleep 0.25; done; echo "STUCK: $1"; frame; exit 1; }
div_col() { frame | python3 -c "
import sys
for line in sys.stdin.read().splitlines()[2:5]:
    d = line.find('││')
    if d >= 0: print(d); break"; }

wait_for "f.txt"; wait_for "row 05 REWRITTEN"

# 1. space rd opens the side-by-side diff: base content on the left of the split.
keys Tab; sleep 0.4
keys Space r d
wait_for "row 05 ORIGINAL"       # the base version, visible only via the split
wait_for "row 05 REWRITTEN"
echo "PASS 1: rd shows base and worktree side by side"

# 2. Close the split with :q; the whole split must dissolve back to the working file with
#    its inline decorations (the scratch title "txt [<ref>]" gone, no leftover diff mode).
keys -l ":q"
sleep 0.3
keys Enter
sleep 0.8
if frame | grep -q "~ row 05 REWRITTEN" && ! frame | grep -q "txt \["; then
  echo "PASS 2: :q dissolves the split back to the decorated working file"
else
  echo "FAIL 2: bad state after :q"; RC=1
  frame | sed -n '2,12p' | cut -c1-70
fi

# 2b. Reopen; close from the SCRATCH side this time: same clean recovery.
keys Space r d
wait_for "txt ["
keys C-w h
sleep 0.3
keys -l ":q"; keys Enter
sleep 0.8
if frame | grep -q "~ row 05 REWRITTEN" && ! frame | grep -q "txt \["; then
  echo "PASS 2b: closing the scratch side recovers identically"
else
  echo "FAIL 2b: bad state after closing the scratch side"; RC=1
  frame | sed -n '2,12p' | cut -c1-70
fi

# 3. Divider drag: grab the pane divider and pull it left; the split point must move.
D0=$(div_col)
X=$((D0 + 1)) # 1-based SGR col of the divider
keys -l "$(printf '\033[<0;%d;20M' "$X")"
for dx in 10 20 30; do keys -l "$(printf '\033[<32;%d;20M' "$((X - dx))")"; sleep 0.1; done
keys -l "$(printf '\033[<0;%d;20m' "$((X - 30))")"
sleep 0.7
D1=$(div_col)
if [ -n "$D1" ] && [ "$D1" -lt "$D0" ]; then
  echo "PASS 3: divider drag resized the panes ($D0 -> $D1)"
else
  echo "FAIL 3: divider did not move ($D0 -> ${D1:-none})"; RC=1
fi

# 4. Help overlay from the files pane; q closes it.
keys Tab; sleep 0.3
keys ?
wait_for "Editor (nvim)"
keys q
wait_gone "Editor (nvim)"
echo "PASS 4: help overlay opens and closes"

keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# split/divider/help assertions done (rc=$RC)"; exit $RC
