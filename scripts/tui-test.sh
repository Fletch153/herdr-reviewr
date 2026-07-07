#!/usr/bin/env bash
# Live end-to-end test of the embedded-nvim review mode, driven through tmux: runs the real
# binary against a fixture repo with a stubbed herdr, sends keys, and asserts on captured
# frames. Requires tmux and nvim; exits non-zero on the first failed assertion, dumping the
# final frame. Run from anywhere: paths resolve from this script's location.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/debug/herdr-reviewr"
TMUX="tmux -L rvt"
TMP="$(mktemp -d)"
FAILED=0
N=0

say() { printf '%s\n' "$*"; }
ok() { N=$((N + 1)); say "ok $N - $*"; }
fail() {
  say "FAIL - $*"
  say "--- last frame ---"
  $TMUX capture-pane -pt0 2>/dev/null || true
  FAILED=1
  exit 1
}

cleanup() {
  $TMUX kill-server 2>/dev/null || true
  rm -rf "$TMP"
}
trap cleanup EXIT

command -v tmux >/dev/null || { say "SKIP: tmux not installed"; exit 0; }
command -v nvim >/dev/null || { say "SKIP: nvim not installed"; exit 0; }

say "# building"
(cd "$ROOT" && cargo build 2>/dev/null) || fail "cargo build"
[ -x "$BIN" ] || fail "binary missing at $BIN"

# --- fixture repo: two committed files, one with an uncommitted change --------------------
REPO="$TMP/repo"
mkdir -p "$REPO/src"
git -C "$REPO" init -q
git -C "$REPO" config user.email t@t
git -C "$REPO" config user.name t
git -C "$REPO" config commit.gpgsign false
# hello.txt is long enough that the Changes view's context folding is observable: the change
# goes at the top, so lines 1-4 stay visible and the tail folds.
{
  printf 'alpha line one\nalpha line two\nalpha line three\n'
  for i in $(seq 4 14); do printf 'alpha filler %d\n' "$i"; done
} > "$REPO/src/hello.txt"
printf 'bravo line one\nbravo line two\n' > "$REPO/src/other.txt"
printf 'UGONE alpha\nUGONE beta\n' > "$REPO/src/uu_gone.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm base
rm "$REPO/src/uu_gone.txt" # deleted in the worktree: must render as the red base view
sed -i '1i uncommitted change ZQX' "$REPO/src/hello.txt"
sed -i '/alpha filler 9/d' "$REPO/src/hello.txt" # a deletion: renders as a red virtual line
printf 'uncommitted change ZQY\n' >> "$REPO/src/other.txt"
# An untracked (added) file: absent from the base, must render fully green ("+" signs).
printf 'NFXALPHA one\nNFXALPHA two\n' > "$REPO/src/zz_new.txt"

# --- environment: stub herdr, plugin root = this checkout ---------------------------------
export HERDR_BIN_PATH="$ROOT/nvim/tests/stub_herdr.sh"
export REVIEWR_STUB_LOG="$TMP/stub.log"
: > "$REVIEWR_STUB_LOG"
export HERDR_PANE_ID=wY:pSIDEBAR HERDR_TAB_ID=wY:t1 HERDR_WORKSPACE_ID=wY
export HERDR_PLUGIN_CONTEXT_JSON='{"focused_pane_id":"wY:pFOCUS"}'
export HERDR_PLUGIN_ROOT="$ROOT"
export HERDR_REVIEW_LOG="$TMP/reviewr.log"

NVIM_BEFORE=$(pgrep -c -f 'nvim --embed' 2>/dev/null || true)
NVIM_BEFORE=${NVIM_BEFORE:-0}

# The embedded nvim must be plugin-clean regardless of the developer's own config, so the run
# is reproducible: XDG paths point into the fixture (reviewr.nvim still loads via the rtp).
# The fixture config sets a space leader (like the target user's), which also proves the
# embedded editor loads the user config before the plugin maps bind.
export XDG_CONFIG_HOME="$TMP/xdg-config" XDG_DATA_HOME="$TMP/xdg-data" XDG_STATE_HOME="$TMP/xdg-state"
mkdir -p "$XDG_CONFIG_HOME/nvim" "$XDG_DATA_HOME" "$XDG_STATE_HOME"
printf "vim.g.mapleader = ' '\n" > "$XDG_CONFIG_HOME/nvim/init.lua"

say "# starting the reviewer under tmux"
$TMUX new-session -d -x 220 -y 50 "cd '$REPO' && exec '$BIN' --editor nvim --poll 500" \
  || fail "tmux session"

keys() { $TMUX send-keys -t0 "$@"; }
frame() { $TMUX capture-pane -pt0; }
wait_for() {
  for _ in $(seq 60); do
    frame 2>/dev/null | grep -qF "$1" && return 0
    sleep 0.25
  done
  fail "waiting for: $1"
}
wait_gone() {
  for _ in $(seq 60); do
    frame 2>/dev/null | grep -qF "$1" || return 0
    sleep 0.25
  done
  fail "waiting for disappearance of: $1"
}

# 1. The reviewer paints: tab bar + the fixture files on the right, and auto-opens the first
#    file. Waiting for its content also settles the initial cursor, so every later key
#    navigates from a known row (no stale-frame races).
wait_for "1 Changes"
wait_for "hello.txt"
ok "reviewer paints with the file list"

# 2. The auto-opened file shows the embedded nvim's Changes view: the change and its context
#    are visible, the unchanged tail is folded away, and the cursor sits on the first change.
wait_for "alpha line one"
wait_for "unchanged lines"
# The deleted line no longer exists in the buffer — it must render as a virtual line.
wait_for "alpha filler 9"
ok "file opens focused on the diff (context, folds, deleted line shown)"

# 3. Keys reach nvim: Tab focuses the editor, insert-typing lands, Esc leaves insert.
keys Tab
keys i
keys -l "XYZTEST "
keys Escape
wait_for "XYZTEST"
ok "typed text lands in the editor"

# 3b. Mode-aware Tab: in insert mode, Tab must TYPE (not switch focus); after Esc it switches.
keys i
keys Tab
keys -l "TABBED"
keys Escape
wait_for "TABBED"
ok "insert-mode Tab types instead of switching focus"

# 4. Switching files with a modified buffer: nvim's default `hidden` keeps the edits safe in
#    the background (no prompt, nothing lost) — vim-native; the quit guard covers data loss.
keys Tab # back to the file list (normal mode now)
sleep 0.5
keys j # select the second file
wait_for "bravo line one"
ok "second file opens; the modified buffer hides in the background"

# 5. Switching back shows the unsaved edits intact — nothing was discarded.
keys k
wait_for "XYZTEST"
ok "unsaved edits survive the file switch"

# 5b. A worktree-deleted file shows the base content as a red scratch view — not a phantom
#     "[New File]" buffer (which would provoke LSP complaints).
keys j # other.txt
wait_for "bravo line one"
keys j # uu_gone.txt (deleted)
wait_for "UGONE alpha"
frame | grep -q "_ UGONE" || fail "deleted file lacks the _ deletion signs"
frame | grep -q "New File" && fail "deleted file opened as a phantom [New File]"
# The list row for a scope-deleted file strikes through its name (SGR 9).
$TMUX capture-pane -pet0 | grep "uu_gone" | grep -qE '(\[|;)9m' \
  || fail "deleted file's list row lacks strikethrough"
ok "deleted file renders as the red base view (struck through in the list)"

# 5c. An added (untracked) file is fully green: every line carries the "+" add sign.
keys j # zz_new.txt (sorts last)
wait_for "NFXALPHA one"
frame | grep -q "+ NFXALPHA" || fail "added file lacks the + add signs"
ok "added file renders fully green"

# 8b. All files shows the file as it exists now — no diff decoration; Changes re-decorates.
keys 2 # All files tab (tree starts collapsed)
sleep 0.5
keys Right # expand src/
sleep 0.3
keys j
keys j
keys j # zz_new.txt in the full tree (src/ dir, hello, other, zz_new)
wait_for "NFXALPHA one"
wait_gone "+ NFXALPHA" # marks clear once the plain open repaints
ok "All files renders the plain file (no diff marks)"
keys 1 # back to Changes: the same buffer re-decorates in place
wait_for "+ NFXALPHA"
ok "Changes re-decorates the same buffer"
keys k # back past the deleted file...
wait_for "UGONE alpha"
keys k # ...to the second file for the comment flow
wait_for "bravo line one"

# 6. Comment flow: space rc in the editor asks the HOST to open its composer; the saved note
#    lands in the reviewer's one store — the same store behind the header count, the list, and
#    send — and paints back into the editor as the boxed inline card.
keys Tab
sleep 0.3
keys Space r c
# Type at human speed WITHOUT waiting for the composer to paint: the compose intent races the
# next keystrokes (rc round-trips through nvim), and every character must land in the host's
# composer — leaked keys would execute as normal-mode vim commands instead.
sleep 0.25
keys -l "needs a guard"
keys Enter
wait_for "╭─ comment · src/other.txt:3"
wait_for "needs a guard"
frame | grep -q "Send (1)" || fail "the header Send count did not pick up the comment"
ok "comment composes in the host and paints as an inline card"

# 6b. The comments list overlay (space rl routes to the host): grouped and jumpable; esc closes.
keys Space r l
wait_for "Comments (1)"
keys Escape
wait_gone "Comments (1)"
ok "comments list opens from the editor"

# 7. Send: space rs dispatches the un-sent comments through the host's one send path; the
#    count zeroes and the comment stays tracked (sent, resolve-only).
keys Space r s
for _ in $(seq 40); do
  grep -q "send wY:pFOCUS" "$REVIEWR_STUB_LOG" 2>/dev/null && break
  sleep 0.25
done
grep -q "send wY:pFOCUS" "$REVIEWR_STUB_LOG" || fail "send did not reach the stub agent"
grep -q "<review>" "$REVIEWR_STUB_LOG" || fail "payload missing the <review> wrapper"
wait_gone "Send (1)"
frame | grep -q "Send (0)" || fail "the header Send count did not reset after dispatch"
ok "send delivers the review payload to the agent"

# 7b. Delete under the cursor (space rx): the card clears from the editor.
keys Space r x
wait_gone "needs a guard"
ok "delete under the cursor clears the inline card"

# 8. Space steps the editor through the file's change hunks before advancing the file.
keys Tab # back to the files pane
sleep 0.3
keys k # hello.txt (opens; the cursor lands on its first change)
wait_for "XYZTEST"
keys Space # hop to the second hunk (the deletion boundary) — must NOT advance the file yet
sleep 0.6
frame | grep -q "bravo line one" && fail "space advanced the file instead of stepping the hunk"
keys Space # exhausted: hello is marked reviewed and the next unreviewed file opens
wait_for "bravo line one"
ok "space steps through hunks, then advances to the next file"

# 9. Quit: q from the files pane; the modified buffer raises the confirm; y quits; no orphans.
keys q
wait_for "quit anyway"
ok "quit guards on the unsaved editor buffer"
keys y
for _ in $(seq 40); do
  $TMUX has-session 2>/dev/null || break
  sleep 0.25
done
$TMUX has-session 2>/dev/null && fail "reviewer did not quit"
NVIM_AFTER=$(pgrep -c -f 'nvim --embed' 2>/dev/null || true)
NVIM_AFTER=${NVIM_AFTER:-0}
[ "$NVIM_AFTER" -le "$NVIM_BEFORE" ] || fail "orphaned nvim --embed ($NVIM_BEFORE -> $NVIM_AFTER)"
ok "clean quit, no orphaned nvim"

say "# all $N live assertions passed"
