#!/usr/bin/env bash
# Live-sync CONFLICT gate: the FileChangedShell "modified" branch — the heart of the
# "conflict = user wins" contract (reviewr.live comment; lib.rs nvim_sync notes). tui-live-test
# covers agent-write-into-a-CLEAN-buffer (silent reload) and edit-into-a-quiet-file (autosave);
# it never exercises the COLLISION: an agent overwrites the file underneath while the user holds
# an UNSAVED edit (still in insert, so no InsertLeave/TextChanged autosave has fired yet). Policy:
# the buffer is KEPT (not reloaded over the user), and the scheduled forced `update!` lands the
# user's text on disk — the user's typing is the newest intent, so it wins the write race.
#
# Characterization only (no product change). Teeth: flip live.lua's FileChangedShell `modified`
# branch to `vim.v.fcs_choice = "reload"` and assertion 1 goes RED (the agent's content reloads
# over the user's unsaved edit).
set -uo pipefail
SOCK=rvconflict
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

# --poll 100 so a checktime sweep lands within the collision window.
tui_start() {
  $TMUX new-session -d -x 200 -y 50 "cd '$REPO' && exec '$BIN' --editor nvim --poll 100" \
    || { echo "FAIL: tmux session"; exit 1; }
}

printf 'line a\nline b\nline c\nline d\nline e\nline f\n' > "$REPO/src/one.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
printf 'TAIL CHANGE\n' >> "$REPO/src/one.txt"

tui_start
wait_for "one.txt"
wait_for "TAIL CHANGE"

# All files (plain, editable) -> focus editor -> insert an UNSAVED marker -> STAY in insert
# (TextChangedI is intentionally not hooked, so the buffer is modified and NOT yet autosaved).
keys 2; sleep 0.8
keys Tab; sleep 0.4
keys i; keys -l "USER_WINS_MARK "; sleep 0.4
frame | grep -qF "USER_WINS_MARK" || fail "the user's insert-mode edit never showed in the buffer"

# A truly concurrent agent process overwrites the WHOLE file underneath.
printf 'AGENT_CLOBBER_A\nAGENT_CLOBBER_B\nAGENT_CLOBBER_C\n' > "$REPO/src/one.txt"

# Let several poll ticks (checktime -> FileChangedShell) sweep the collision.
sleep 0.9

# 1. USER WINS in the buffer: the unsaved edit survives; the agent's overwrite must NOT have
#    reloaded over it. (This is the teeth assertion: a "reload" policy makes it RED.)
frame | grep -qF "USER_WINS_MARK" \
  || fail "the user's unsaved edit was reload-clobbered by the concurrent agent write"
frame | grep -qF "AGENT_CLOBBER_A" \
  && fail "the agent's overwrite replaced the buffer (the user did not win)"
echo "ok 1 - unsaved user edit survived the concurrent agent overwrite (kept, not reloaded)"

# 2. USER WINS on disk: leaving insert fires the autosave, and the scheduled forced update!
#    writes the user's buffer over the agent's concurrent write.
esc
for _ in $(seq 40); do grep -qF "USER_WINS_MARK" "$REPO/src/one.txt" && break; sleep 0.25; done
grep -qF "USER_WINS_MARK" "$REPO/src/one.txt" \
  || fail "the user's edit never reached disk after leaving insert"
grep -qF "AGENT_CLOBBER_A" "$REPO/src/one.txt" \
  && fail "the agent's concurrent write survived on disk (the user did not win the write race)"
echo "ok 2 - user's edit clobbered the agent's write on disk (user wins, one autosave wide)"

# Quit focus-correctly: leaving insert autosaved (buffer clean), so Tab back to the files pane
# and q quits without a ConfirmQuit prompt; tolerate the prompt if a stray edit lingers.
keys Tab; sleep 0.3
keys q; sleep 0.3
$TMUX has-session 2>/dev/null && { keys y 2>/dev/null || true; }
wait_session_end
echo "# all live-sync conflict assertions passed"
