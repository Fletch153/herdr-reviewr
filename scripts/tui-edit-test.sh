#!/usr/bin/env bash
# Live tmux gate for the comment edit/resolve lifecycle in nvim mode: multiline compose,
# edit-in-place (un-sent only), the sent guard, list editing, the Esc/Alt aliasing guard, and
# batch resolve. Same conventions as tui-test.sh; exits non-zero on the first failure.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/debug/herdr-reviewr"
TMUX="tmux -L rvedit"
TMP="$(mktemp -d)"
trap '$TMUX kill-server 2>/dev/null || true; rm -rf "$TMP"' EXIT
command -v tmux >/dev/null || { echo "SKIP: tmux not installed"; exit 0; }
command -v nvim >/dev/null || { echo "SKIP: nvim not installed"; exit 0; }
(cd "$ROOT" && cargo build 2>/dev/null) || { echo "FAIL: cargo build"; exit 1; }

REPO="$TMP/repo"; mkdir -p "$REPO/src"
git -C "$REPO" init -qb main
git -C "$REPO" config user.email t@t; git -C "$REPO" config user.name t; git -C "$REPO" config commit.gpgsign false
printf 'line a\nline b\nline c\nline d\nline e\n' > "$REPO/src/one.txt"
printf 'other a\nother b\n' > "$REPO/src/two.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
printf 'CHANGE ONE\n' >> "$REPO/src/one.txt"
printf 'CHANGE TWO\n' >> "$REPO/src/two.txt"

export HERDR_BIN_PATH="$ROOT/nvim/tests/stub_herdr.sh" REVIEWR_STUB_LOG="$TMP/stub.log"
export HERDR_PANE_ID=wY:pSIDEBAR HERDR_TAB_ID=wY:t1 HERDR_WORKSPACE_ID=wY
export HERDR_PLUGIN_CONTEXT_JSON='{"focused_pane_id":"wY:pFOCUS"}' HERDR_PLUGIN_ROOT="$ROOT"
export XDG_CONFIG_HOME="$TMP/x" XDG_DATA_HOME="$TMP/xd" XDG_STATE_HOME="$TMP/xs"
mkdir -p "$XDG_CONFIG_HOME/nvim"
printf "vim.g.mapleader = ' '\nvim.o.number = true\n" > "$XDG_CONFIG_HOME/nvim/init.lua"

$TMUX new-session -d -x 200 -y 50 "cd '$REPO' && exec '$BIN' --editor nvim --poll 500" \
  || { echo "FAIL: tmux session"; exit 1; }
keys() { $TMUX send-keys -t0 "$@"; }
frame() { $TMUX capture-pane -pt0; }
fail() { echo "FAIL - $*"; frame; exit 1; }
wait_for() { for _ in $(seq 60); do frame | grep -qF "$1" && return 0; sleep 0.25; done; fail "waiting for: $1"; }
wait_gone() { for _ in $(seq 60); do frame | grep -qF "$1" || return 0; sleep 0.25; done; fail "stuck: $1"; }
# A REAL Escape: give the terminal a beat so the next key can't merge into Alt+<key>.
esc() { keys Escape; sleep 0.2; }

wait_for "one.txt"; wait_for "CHANGE ONE"

# 1. Multiline note: Ctrl+J inserts a newline in the composer; the card shows both lines.
keys Tab; sleep 0.4
keys Space r c; sleep 0.3
keys -l "first line of note"
keys C-j
keys -l "second line of note"
keys Enter
wait_for "╭─ comment"
wait_for "first line of note"
wait_for "second line of note"
echo "ok 1 - multiline note renders in the card"

# 2. Edit under the cursor: space re prefills the composer; save updates the card.
keys Space r e
wait_for "edit ·"
wait_for "first line of note"
keys C-e
keys -l " AMENDED"
keys Enter
wait_for "AMENDED"
wait_gone "edit ·"
echo "ok 2 - re edits the un-sent comment in place"

# 3. Send, then re must refuse (sent comments are resolve-only).
keys Space r s
wait_for "sent 1 to agent"
keys Space r e
wait_for "sent — resolve only"
echo "ok 3 - sent comments refuse editing"

# 4. Second comment on the other file; the list's e edits the un-sent one.
keys Tab; sleep 0.3
keys j
wait_for "CHANGE TWO"
keys Tab; sleep 0.3
keys Space r c; sleep 0.3
keys -l "note on two"; keys Enter
wait_for "note on two"
wait_for "Send (1)"
keys Space r l
wait_for "Comments (2)"
keys j
keys e
wait_for "edit ·"
esc
wait_for "Comments (2)"
esc
wait_gone "Comments ("
echo "ok 4 - list e opens the composer; esc returns to the list, esc again closes"

# 5. Esc/Alt aliasing guard: without the kitty protocol, "Esc then Space" in one burst arrives
#    as Alt+Space. That must be inert in the list — it once toggled a checkbox, and a fast
#    trailing r then RESOLVED a comment the user never targeted.
keys Space r l
wait_for "Comments (2)"
keys Escape Space   # one burst — deliberately mergeable
sleep 0.5
frame | grep -qF "Comments (2)" || fail "aliased Alt+Space closed or mutated the list"
frame | grep -qF "selected" && fail "aliased Alt+Space toggled a list checkbox"
echo "ok 5 - an aliased Esc+Space burst is inert in the list"

# 6. Batch resolve: select all, r resolves everything; the cards clear; Send (0).
keys a
keys r
wait_gone "Comments ("
wait_gone "╭─ comment"
wait_for "Send (0)"
echo "ok 6 - batch resolve clears the store and the cards"

keys Tab; sleep 0.3
keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all edit-lifecycle assertions passed"
