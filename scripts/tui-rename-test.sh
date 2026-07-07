#!/usr/bin/env bash
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/debug/herdr-reviewr"
TMUX="tmux -L rvren"
TMP="$(mktemp -d)"
command -v tmux >/dev/null || { echo "SKIP: tmux not installed"; exit 0; }
command -v nvim >/dev/null || { echo "SKIP: nvim not installed"; exit 0; }
(cd "$ROOT" && cargo build 2>/dev/null) || { echo "FAIL: cargo build"; exit 1; }
trap '$TMUX kill-server 2>/dev/null || true; rm -rf "$TMP"' EXIT
REPO="$TMP/repo"; mkdir -p "$REPO/src"
git -C "$REPO" init -qb main; git -C "$REPO" config user.email t@t; git -C "$REPO" config user.name t; git -C "$REPO" config commit.gpgsign false
{ for i in $(seq 1 12); do printf 'stable content %02d\n' "$i"; done; } > "$REPO/src/old_name.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
git -C "$REPO" mv src/old_name.txt src/new_name.txt
sed -i 's/stable content 06/EDITED CONTENT 06/' "$REPO/src/new_name.txt"
git -C "$REPO" add -A   # staged rename so git status reports R
export HERDR_BIN_PATH="$ROOT/nvim/tests/stub_herdr.sh" REVIEWR_STUB_LOG="$TMP/stub.log"
export HERDR_PANE_ID=wY:pSIDEBAR HERDR_TAB_ID=wY:t1 HERDR_WORKSPACE_ID=wY
export HERDR_PLUGIN_CONTEXT_JSON='{"focused_pane_id":"wY:pFOCUS"}' HERDR_PLUGIN_ROOT="$ROOT"
export XDG_CONFIG_HOME="$TMP/x" XDG_DATA_HOME="$TMP/xd" XDG_STATE_HOME="$TMP/xs"
mkdir -p "$XDG_CONFIG_HOME/nvim"; printf "vim.g.mapleader = ' '\n" > "$XDG_CONFIG_HOME/nvim/init.lua"
$TMUX new-session -d -x 200 -y 50 "cd '$REPO' && exec '$BIN' --editor nvim --poll 500"
frame() { $TMUX capture-pane -pt0; }
wait_for() { for _ in $(seq 60); do frame | grep -qF "$1" && return 0; sleep 0.25; done; echo "TIMEOUT: $1"; frame; exit 1; }
wait_for "new_name.txt"
wait_for "EDITED CONTENT 06"
sleep 1
frame | grep -q "+ stable content 02" && { echo "FAIL: rename painted an unchanged line as an addition"; frame; exit 1; }
frame | grep -q "~ EDITED CONTENT 06" || { echo "FAIL: the real edit lacks its modification sign"; frame; exit 1; }
frame | grep -q "stable content 06" || { echo "FAIL: the replaced old line is not shown as a virtual line"; frame; exit 1; }
echo "# rename renders as a modification, not an insertion"
