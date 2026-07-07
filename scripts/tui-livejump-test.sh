#!/usr/bin/env bash
# Live gate: the buf-adopt path (sidebar follows a native editor jump) composed with live-sync
# (agent writes to open files) and with a burst of jumps. tui-jump-test covers a single native
# jump; this pins the CROSS-SUBSYSTEM interactions that neither it nor tui-live-test exercise:
#   RACE/RACE2 - the agent live-writes the file the editor jumped onto; the reloaded line paints,
#                the comment card (keyed on the adopted diff_path) survives, the selection holds.
#   BOUNCE     - a burst of native jumps drains FIFO and converges last-wins on the final buffer.
#   CS-ENTRY   - a file the editor already sits on ENTERS the changeset later (an agent write turns
#                an unchanged, jumped-to file into a changed one). A checktime reload fires no
#                BufEnter, so the buf-adopt path never re-reports it; nvim_sync reconciles the
#                selection to the editor's real buffer so the sidebar still follows. Regression
#                lock for the desync where the list stayed on the old file while the editor showed
#                the now-changed one.
# Runs at --poll 100 so the live-sync poll tick and the buf-adopt reconcile collide tightly.
set -uo pipefail
SOCK="rvlj$$"
export TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

# Tightened-poll start (tui_start hardcodes 500; 100 stresses the poll-vs-adopt collision).
tui_start() {
  $TMUX new-session -d -x 200 -y 50 \
    "cd '$REPO' && exec '$BIN' --editor nvim --poll 100" \
    || { echo "FAIL: tmux session"; exit 1; }
}

printf 'a1\na2\na3\n' > "$REPO/src/alpha.txt"
printf 'b1\nb2\nb3\nb4\n' > "$REPO/src/beta.txt"
printf 'g1\ng2\nGAMMA_STABLE\n' > "$REPO/src/gamma.txt"   # committed, initially unchanged
git -C "$REPO" add -A && git -C "$REPO" commit -qm base
printf 'ALPHA_CHANGE\n' >> "$REPO/src/alpha.txt"
printf 'BETA_CHANGE\n' >> "$REPO/src/beta.txt"

# The selected file-list row fills edge-to-edge with the cursor color (surface2 focused, surface1
# when the editor holds focus). The file name appears in BOTH panes, so isolate the bare list row
# and look for either fill as a background (the `48;2;` prefix excludes the same colors used as
# foregrounds on borders/titles). See tui-jump-test for the same technique.
SELFILL='48;2;(69;71;90|88;91;112)'
list_row() { framee | grep -aF "$1" | grep -avF "src/$1" | grep -avF ':e '; }
row_selected() { list_row "$1" | grep -qaE "$SELFILL"; }
wait_row_selected() { for _ in $(seq 40); do row_selected "$1" && return 0; sleep 0.2; done; fail "the file list never highlighted $1"; }
painted() { frame | grep -qE '[+~] *[0-9]+ '"$1"; }

tui_start
wait_for "alpha.txt"
wait_for "ALPHA_CHANGE"
row_selected alpha.txt || fail "did not start with alpha selected"

# Native jump to beta (in-changeset): buf-adopt moves diff_path + list selection to beta.
keys Tab; sleep 0.4
esc
keys -l ':e src/beta.txt'; keys Enter; sleep 0.8
frame | grep -q "BETA_CHANGE" || fail "the :e jump to beta did not land"
wait_row_selected beta.txt
echo "ok setup - adopted beta as the selection via a native jump"

# Author a comment on beta (anchored to the jumped-to buffer, keyed to diff_path=beta).
keys -l '1G'; sleep 0.2
keys Space r c; sleep 0.4
keys -l 'JLNOTE'; keys Enter; sleep 0.5
wait_for "╭─ comment"
frame | grep -qF "JLNOTE" || fail "the comment on the adopted file did not paint"
echo "ok comment - JLNOTE painted on the adopted buffer"

# RACE: the agent appends to beta (the adopted, open buffer) with NOTHING pressed. The new line
# live-reloads and paints; the JLNOTE card survives (cards key on diff_path=beta, which the poll's
# entries rebuild must not move); the list stays on beta.
printf 'AGENT_ONBETA\n' >> "$REPO/src/beta.txt"
wait_for "AGENT_ONBETA"
painted "AGENT_ONBETA" || fail "RACE: the live agent write to the adopted file did not paint as an addition"
frame | grep -qF "JLNOTE" || fail "RACE: the live reload of the adopted file dropped the comment card"
row_selected beta.txt || fail "RACE: the poll's entries rebuild desynced the adopted selection off beta"
echo "ok RACE - live agent-write to the adopted buffer: line paints, card survives, selection holds"

# RACE2: a second comment while the live write is fresh, then another write. Both cards survive.
keys Escape; keys -l 'G'; sleep 0.2
keys Space r c; sleep 0.4
keys -l 'JLNOTE2'; keys Enter; sleep 0.5
wait_for "JLNOTE2"
printf 'AGENT_ONBETA2\n' >> "$REPO/src/beta.txt"
wait_for "AGENT_ONBETA2"
frame | grep -qF "JLNOTE" || fail "RACE2: first card lost after a second live write"
frame | grep -qF "JLNOTE2" || fail "RACE2: second card lost after a second live write"
row_selected beta.txt || fail "RACE2: selection desynced after a second live write"
echo "ok RACE2 - two cards survive interleaved live writes on the adopted buffer"

# BOUNCE: a burst of native jumps must converge last-wins on the final buffer. Fire the :e jumps
# back-to-back with only a terminal beat between them (no poll-length settle), so several "buf"
# intents are in flight together, then assert the sidebar converges on the LAST target.
esc
keys -l ':e src/alpha.txt'; keys Enter; sleep 0.15
keys -l ':e src/beta.txt'; keys Enter; sleep 0.15
keys -l ':e src/alpha.txt'; keys Enter; sleep 0.15
keys -l ':e src/beta.txt'; keys Enter          # final target: beta
wait_row_selected beta.txt
row_selected alpha.txt && fail "BOUNCE: the burst left the selection on a stale jump target"
frame | grep -qF "JLNOTE" || fail "BOUNCE: beta's comment card was lost after the jump burst"
echo "ok BOUNCE - a burst of native jumps converges last-wins on beta with its card intact"

# CS-ENTRY: jump to gamma, which is OUTSIDE the changeset. The selection must stay put (beta) and
# the editor must not be yanked back — the documented out-of-changeset behavior. THEN the agent
# modifies gamma, turning it into a changed file. Its checktime reload fires no BufEnter, so the
# buf-adopt path never re-reports it; the nvim_sync reconcile must move the selection to gamma so
# the sidebar follows the buffer the editor actually shows.
esc
keys -l ':e src/gamma.txt'; keys Enter; sleep 0.8
frame | grep -q "GAMMA_STABLE" || fail "CS-ENTRY: the :e jump to gamma did not land"
sleep 0.4                                       # a poll cycle: the out-of-cs file must not be yanked
row_selected beta.txt || fail "CS-ENTRY: an out-of-changeset jump lost the selection off beta"
row_selected gamma.txt && fail "CS-ENTRY: an out-of-changeset file was adopted as the selection prematurely"
printf 'GAMMA_NOWCHANGED\n' >> "$REPO/src/gamma.txt"
wait_for "GAMMA_NOWCHANGED"
wait_row_selected gamma.txt                     # the reconcile moved the selection onto the now-changed buffer
row_selected beta.txt && fail "CS-ENTRY: the sidebar still highlights the old file after gamma entered the changeset"
echo "ok CS-ENTRY - a file that enters the changeset while the editor sits on it takes the selection"

esc                                             # beat after Escape so Tab can't merge into Alt+Tab
keys Tab; sleep 0.4
keys q; sleep 0.4; keys y 2>/dev/null || true
wait_session_end
echo "# all live-jump cross-subsystem assertions passed"
