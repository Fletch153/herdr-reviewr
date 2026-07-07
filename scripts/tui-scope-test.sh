#!/usr/bin/env bash
# Human-style probe: comments pin to the scope+base they were authored under; scope flips
# re-diff the editor; the list's Enter restores the authoring view.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/debug/herdr-reviewr"
TMUX="tmux -L rvscope"
TMP="$(mktemp -d)"
trap '$TMUX kill-server 2>/dev/null || true; rm -rf "$TMP"' EXIT
REPO="$TMP/repo"; mkdir -p "$REPO/src"
git -C "$REPO" init -qb main
git -C "$REPO" config user.email t@t; git -C "$REPO" config user.name t; git -C "$REPO" config commit.gpgsign false
printf 'base one\nbase two\nbase three\n' > "$REPO/src/f1.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
git -C "$REPO" checkout -qb feature
printf 'COMMITTED CHANGE\n' >> "$REPO/src/f1.txt"
printf 'new file line\n' > "$REPO/src/f2.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm B
printf 'UNCOMMITTED TAIL\n' >> "$REPO/src/f1.txt"
export HERDR_BIN_PATH="$ROOT/nvim/tests/stub_herdr.sh" REVIEWR_STUB_LOG="$TMP/stub.log"
export HERDR_PANE_ID=wY:pSIDEBAR HERDR_TAB_ID=wY:t1 HERDR_WORKSPACE_ID=wY
export HERDR_PLUGIN_CONTEXT_JSON='{"focused_pane_id":"wY:pFOCUS"}'
export HERDR_PLUGIN_ROOT="$ROOT"
export XDG_CONFIG_HOME="$TMP/xdg" XDG_DATA_HOME="$TMP/xdgd" XDG_STATE_HOME="$TMP/xdgs"
mkdir -p "$XDG_CONFIG_HOME/nvim"; printf "vim.g.mapleader = ' '\nvim.o.number = true\n" > "$XDG_CONFIG_HOME/nvim/init.lua"
$TMUX new-session -d -x 200 -y 50 "cd '$REPO' && exec '$BIN' --editor nvim --poll 500"
keys() { $TMUX send-keys -t0 "$@"; }
frame() { $TMUX capture-pane -pt0; }
save() { frame > "$1"; }
wait_for() { for _ in $(seq 60); do frame | grep -qF "$1" && return 0; sleep 0.25; done; echo "TIMEOUT waiting: $1"; frame; exit 1; }
wait_gone() { for _ in $(seq 60); do frame | grep -qF "$1" || return 0; sleep 0.25; done; echo "STUCK: $1"; frame; exit 1; }

# Commit scope (default): f1 shows the uncommitted tail as the change.
wait_for "f1.txt"
wait_for "UNCOMMITTED TAIL"
# 1. Comment on the change under Commit scope.
keys Tab; sleep 0.4
keys Space r c; sleep 0.3
keys -l "commit-scope note"; keys Enter
wait_for "╭─ comment"
wait_for "Send (1)"
save "$TMP/s1-commit-card.txt"
echo "PASS 1: card under commit scope"

# 2. Branch scope: different diff — the card must hide; highlighting re-diffs vs merge-base
#    (the committed change now paints too).
keys Tab; sleep 0.3
keys b
wait_gone "╭─ comment"
wait_for "COMMITTED CHANGE"
frame | grep -q "+.*COMMITTED CHANGE" || { echo "FAIL: committed change not painted as + under branch scope"; frame | grep -n "COMMITTED"; exit 1; }
save "$TMP/s2-branch-nocard.txt"
echo "PASS 2: card hidden + re-diffed under branch scope"

# 3. Last-turn scope: must not error; the pane keeps rendering.
keys t; sleep 1
frame | grep -q "f1.txt" || { echo "FAIL: pane lost after last-turn switch"; frame; exit 1; }
save "$TMP/s3-lastturn.txt"
echo "PASS 3: last-turn scope renders"

# 4. The comments list restores the authoring scope on Enter and the card returns.
keys l
wait_for "Comments (1)"
keys Enter
wait_gone "Comments (1)"
wait_for "╭─ comment"
frame | grep -q "\[commit\]" || { echo "FAIL: scope chip did not restore to commit"; frame | head -3; exit 1; }
save "$TMP/s4-restored.txt"
echo "PASS 4: list jump restores scope and card"

keys q; sleep 0.3; keys y
echo "SCOPE PROBE PASSED"
