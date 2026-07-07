#!/usr/bin/env bash
# Human-style probe of the editor death/respawn lifecycle: :qa! inside nvim, the dead panel,
# auto-respawn on the next open, manual r restart, and theme/card/base survival across each.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/debug/herdr-reviewr"
TMUX="tmux -L rvdeath"
TMP="$(mktemp -d)"
command -v tmux >/dev/null || { echo "SKIP: tmux not installed"; exit 0; }
command -v nvim >/dev/null || { echo "SKIP: nvim not installed"; exit 0; }
(cd "$ROOT" && cargo build 2>/dev/null) || { echo "FAIL: cargo build"; exit 1; }
NVIM_BEFORE=$(pgrep -c -f 'nvim --embed' 2>/dev/null || true); NVIM_BEFORE=${NVIM_BEFORE:-0}
trap '$TMUX kill-server 2>/dev/null || true; rm -rf "$TMP"' EXIT
REPO="$TMP/repo"; mkdir -p "$REPO/src"
git -C "$REPO" init -qb main; git -C "$REPO" config user.email t@t; git -C "$REPO" config user.name t; git -C "$REPO" config commit.gpgsign false
printf 'aaa\nbbb\nccc\n' > "$REPO/src/one.txt"
printf 'xxx\nyyy\n' > "$REPO/src/two.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
printf 'CHANGE ONE\n' >> "$REPO/src/one.txt"
printf 'CHANGE TWO\n' >> "$REPO/src/two.txt"
export HERDR_BIN_PATH="$ROOT/nvim/tests/stub_herdr.sh" REVIEWR_STUB_LOG="$TMP/stub.log"
export HERDR_PANE_ID=wY:pSIDEBAR HERDR_TAB_ID=wY:t1 HERDR_WORKSPACE_ID=wY
export HERDR_PLUGIN_CONTEXT_JSON='{"focused_pane_id":"wY:pFOCUS"}' HERDR_PLUGIN_ROOT="$ROOT"
export XDG_CONFIG_HOME="$TMP/x" XDG_DATA_HOME="$TMP/xd" XDG_STATE_HOME="$TMP/xs"
mkdir -p "$XDG_CONFIG_HOME/nvim"; printf "vim.g.mapleader = ' '\nvim.o.number = true\n" > "$XDG_CONFIG_HOME/nvim/init.lua"
$TMUX new-session -d -x 200 -y 50 "cd '$REPO' && exec '$BIN' --editor nvim --poll 500"
keys() { $TMUX send-keys -t0 "$@"; }
frame() { $TMUX capture-pane -pt0; }
framee() { $TMUX capture-pane -pet0; }
wait_for() { for _ in $(seq 60); do frame | grep -qF "$1" && return 0; sleep 0.25; done; echo "TIMEOUT: $1"; frame; exit 1; }
wait_gone() { for _ in $(seq 60); do frame | grep -qF "$1" || return 0; sleep 0.25; done; echo "STUCK: $1"; frame; exit 1; }
PEACH="38;2;250;179;135"
card_is_themed() { framee | grep "comment ·" | grep -q "$PEACH"; }

wait_for "one.txt"; wait_for "CHANGE ONE"
keys Tab; sleep 0.4
keys Space r c; sleep 0.3
keys -l "survives death"; keys Enter
wait_for "╭─ comment"
card_is_themed || { echo "FAIL 0: card not themed on first paint"; exit 1; }
echo "PASS 0: themed card before any death"

# 1. Kill the editor from inside (:qa!) → the dead panel shows.
keys -l ":qa!"; keys Enter
wait_for "the editor (nvim) exited"
echo "PASS 1: dead panel on :qa!"

# 2. Click/select the other file → one automatic respawn: file opens, card set re-pushes,
#    diff decorations return, theme holds.
keys Tab; sleep 0.3
keys j    # two.txt (selection change reopens via the watcher)
wait_for "CHANGE TWO"
wait_gone "the editor (nvim) exited"
keys k    # back to one.txt: its card must re-render after the respawn
wait_for "╭─ comment"
wait_for "survives death"
card_is_themed || { echo "FAIL 2: card lost the reviewer theme after auto-respawn"; exit 1; }
echo "PASS 2: auto-respawn restores file, card, and theme"

# 3. Kill again; recover via the dead panel's manual r this time.
keys Tab; sleep 0.3
keys -l ":qa!"; keys Enter
wait_for "the editor (nvim) exited"
keys r    # focus is already Diff (we were in the editor when it died)
wait_gone "the editor (nvim) exited"
wait_for "CHANGE ONE"
wait_for "╭─ comment"
card_is_themed || { echo "FAIL 3: card lost the reviewer theme after manual r restart"; exit 1; }
echo "PASS 3: manual restart restores file, card, and theme"

# 4. No orphans: exactly zero embedded nvims after quit.
keys Tab; sleep 0.3; keys q; sleep 0.3; keys y 2>/dev/null || true
for _ in $(seq 40); do $TMUX has-session 2>/dev/null || break; sleep 0.25; done
NVIM_AFTER=$(pgrep -c -f 'nvim --embed' 2>/dev/null || true); NVIM_AFTER=${NVIM_AFTER:-0}
[ "$NVIM_AFTER" -le "$NVIM_BEFORE" ] || { echo "FAIL: orphaned nvim ($NVIM_BEFORE -> $NVIM_AFTER)"; exit 1; }
echo "# all death/respawn assertions passed"
