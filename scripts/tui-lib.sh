# Shared harness for the live tmux gates (tui-*-test.sh). Source AFTER setting SOCK (a unique
# tmux socket name per gate); optionally set TUI_INIT_EXTRA to append lines to the fixture
# nvim config (e.g. "vim.o.number = true").
#
# Provides: ROOT/BIN/TMP, a git-initialized $REPO to populate, the stubbed herdr environment,
# tui_start (tmux session running the reviewer), key/frame helpers with stale-frame-safe
# waits, SGR mouse synthesis, right-pane text location, and a fail() that dumps the frame.
# Cleanup (tmux server + TMP) runs on exit.

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/debug/herdr-reviewr"
TMUX="tmux -L ${SOCK:?set SOCK before sourcing tui-lib.sh}"
TMP="$(mktemp -d)"
trap '$TMUX kill-server 2>/dev/null || true; rm -rf "$TMP"' EXIT

command -v tmux >/dev/null || { echo "SKIP: tmux not installed"; exit 0; }
command -v nvim >/dev/null || { echo "SKIP: nvim not installed"; exit 0; }
(cd "$ROOT" && cargo build 2>/dev/null) || { echo "FAIL: cargo build"; exit 1; }

REPO="$TMP/repo"
mkdir -p "$REPO/src"
git -C "$REPO" init -qb main
git -C "$REPO" config user.email t@t
git -C "$REPO" config user.name t
git -C "$REPO" config commit.gpgsign false

export HERDR_BIN_PATH="$ROOT/nvim/tests/stub_herdr.sh" REVIEWR_STUB_LOG="$TMP/stub.log"
: > "$REVIEWR_STUB_LOG"
export HERDR_PANE_ID=wY:pSIDEBAR HERDR_TAB_ID=wY:t1 HERDR_WORKSPACE_ID=wY
export HERDR_PLUGIN_CONTEXT_JSON='{"focused_pane_id":"wY:pFOCUS"}' HERDR_PLUGIN_ROOT="$ROOT"
export XDG_CONFIG_HOME="$TMP/xdg-config" XDG_DATA_HOME="$TMP/xdg-data" XDG_STATE_HOME="$TMP/xdg-state"
mkdir -p "$XDG_CONFIG_HOME/nvim" "$XDG_DATA_HOME" "$XDG_STATE_HOME"
{
  printf "vim.g.mapleader = ' '\n"
  printf '%s\n' "${TUI_INIT_EXTRA:-}"
} > "$XDG_CONFIG_HOME/nvim/init.lua"

tui_start() {
  $TMUX new-session -d -x 200 -y 50 "cd '$REPO' && exec '$BIN' --editor nvim --poll 500" \
    || { echo "FAIL: tmux session"; exit 1; }
}

keys() { $TMUX send-keys -t0 "$@"; }
frame() { $TMUX capture-pane -pt0; }
framee() { $TMUX capture-pane -pet0; }
fail() { echo "FAIL - $*"; frame; exit 1; }
# Wait for text to (dis)appear; always wait for something ABSENT before the triggering key.
wait_for() { for _ in $(seq 60); do frame | grep -qF "$1" && return 0; sleep 0.25; done; fail "waiting for: $1"; }
wait_gone() { for _ in $(seq 60); do frame | grep -qF "$1" || return 0; sleep 0.25; done; fail "stuck: $1"; }
# A REAL Escape: give the terminal a beat so the next key can't merge into Alt+<key>.
esc() { keys Escape; sleep 0.2; }

# SGR mouse synthesis (1-based cell coordinates).
click() { keys -l "$(printf '\033[<0;%d;%dM' "$1" "$2")"; sleep 0.15; keys -l "$(printf '\033[<0;%d;%dm' "$1" "$2")"; }
drag() { keys -l "$(printf '\033[<32;%d;%dM' "$1" "$2")"; }
press() { keys -l "$(printf '\033[<0;%d;%dM' "$1" "$2")"; }
release() { keys -l "$(printf '\033[<0;%d;%dm' "$1" "$2")"; }
wheel_up() { keys -l "$(printf '\033[<64;%d;%dM' "$1" "$2")"; }
wheel_down() { keys -l "$(printf '\033[<65;%d;%dM' "$1" "$2")"; }

# (col, row) of the first occurrence of $1 anywhere in the frame.
locate() {
  frame | python3 -c "
import sys
pat = sys.argv[1]
for nr, line in enumerate(sys.stdin.read().splitlines(), 1):
    i = line.find(pat)
    if i >= 0:
        print(i + 1, nr)
        break
" "$1"
}

# (col, row) of $1 in the FILE-LIST pane only (right of the pane divider) — list-row text also
# appears in the editor pane (title, statusline).
locate_right() {
  frame | python3 -c "
import sys
pat = sys.argv[1]
for nr, line in enumerate(sys.stdin.read().splitlines(), 1):
    div = line.find('││')
    if div < 0:
        continue
    i = line.find(pat, div + 2)
    if i >= 0:
        print(i + 1, nr)
        break
" "$1"
}

# The column of the pane divider (for drag-resize assertions).
div_col() {
  frame | python3 -c "
import sys
for line in sys.stdin.read().splitlines()[2:6]:
    d = line.find('││')
    if d >= 0:
        print(d)
        break"
}
