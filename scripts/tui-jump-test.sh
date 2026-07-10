#!/usr/bin/env bash
# Live gate: the file viewer AND comment cards follow the editor across a native buffer jump.
# The embedded editor can change its own current buffer without the host asking — a tag jump /
# Ctrl-], the jumplist, or :e — and can land on a file outside the changeset. The host must move
# its selection (diff_path + file-list cursor) to the file the editor actually shows, so the
# sidebar highlights it and comment cards paint on the right buffer. Regression lock for the
# user-reported desync: the sidebar stayed on the old file, a comment on the jumped-to file was
# stored but never painted, and deleting one of two looked like "removed 2".
set -uo pipefail
SOCK="jump$$"
export TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

printf 'a1\na2\na3\n' > "$REPO/src/alpha.txt"
printf 'b1\nb2\nb3\nb4\n' > "$REPO/src/beta.txt"
printf 'g1\ng2\nGAMMA_STABLE\n' > "$REPO/src/gamma.txt"   # committed, never changed
git -C "$REPO" add -A && git -C "$REPO" commit -qm base
printf 'ALPHA_CHANGE\n' >> "$REPO/src/alpha.txt"
printf 'BETA_CHANGE\n' >> "$REPO/src/beta.txt"

# The selected file-list row fills edge-to-edge with the cursor color: surface2 when the list is
# focused, a step softer (surface1) when the editor holds focus. That background fill is the one
# list-pane marker distinguishing the selected file; the file name itself appears in BOTH panes,
# so isolate the bare list row ("beta.txt", not the editor title's "src/beta.txt" or the ":e"
# cmdline) and look for either fill as a background (the `48;2;` prefix excludes the same colors
# used as foregrounds on borders/titles).
SELFILL='48;2;(69;71;90|88;91;112)'
list_row() { framee | grep -aF "$1" | grep -avF "src/$1" | grep -avF ':e '; }
row_selected() { list_row "$1" | grep -qaE "$SELFILL"; }
wait_row_selected() { for _ in $(seq 40); do row_selected "$1" && return 0; sleep 0.2; done; fail "the file list never highlighted $1"; }
# List-FOCUSED selected-row fill only (surface2, 88;91;112); the editor-focused row is a step
# softer (surface1, 69;71;90). The teardown uses this to confirm focus actually reached the list
# before quitting: `q` quits only from a focused list — forwarded to a focused editor it starts a
# nvim macro recording and never quits.
list_focused() { list_row "$1" | grep -qaE '48;2;88;91;112'; }

tui_start
wait_for "alpha.txt"
wait_for "ALPHA_CHANGE"                        # Changes tab (default) opens the first file, alpha
row_selected alpha.txt || fail "the list did not start with alpha selected"

# --- A: a native jump to an in-changeset file moves the sidebar selection (the PRIMARY fix) ---
keys Tab; sleep 0.4                            # focus the editor
esc
keys -l ':e src/beta.txt'; keys Enter; sleep 0.8
frame | grep -q "BETA_CHANGE" || fail "the :e jump to beta did not land"
wait_row_selected beta.txt                     # sidebar followed the editor to beta
row_selected alpha.txt && fail "A: the file list still highlights the previously open file"
echo "ok A - the sidebar selection follows the editor to the jumped-to file"

# --- A2: cards are downstream of the selection — a comment on beta paints immediately ---------
keys -l '1G'; sleep 0.2
keys Space r c; sleep 0.4
keys -l 'BETANOTE_A'; keys Enter; sleep 0.5
wait_for "╭─ comment"
frame | grep -qF "BETANOTE_A" || fail "A2: comment card on the jumped-to buffer did not paint"
echo "ok A2 - a comment on the jumped-to file paints immediately"

# --- B: deleting one of two comments leaves the survivor painted -----------------------------
keys Escape; keys -l '3G'; sleep 0.2
keys Space r c; sleep 0.4
keys -l 'BETANOTE_B'; keys Enter; sleep 0.5
wait_for "BETANOTE_B"
keys Escape; keys -l '1G'; sleep 0.2
keys Space r x; sleep 0.6
wait_gone "BETANOTE_A"
frame | grep -qF "BETANOTE_B" || fail "B: deleting one comment dropped the survivor's card"
echo "ok B - deleting one of two comments leaves the survivor's card painted"

# --- C: a jump OUTSIDE the changeset paints cards without a diff view, yank, or lost selection -
# gamma is unchanged, so it has no list row. The editor must stay on gamma (not be yanked back),
# the sidebar selection must stay put (leave-as-is, still on beta), and a comment on gamma must
# still paint.
esc
keys -l ':e src/gamma.txt'; keys Enter; sleep 0.8
wait_for "GAMMA_STABLE"
sleep 0.8                                       # a poll cycle: the host must not yank it back
frame | grep -q "GAMMA_STABLE" || fail "C: the host yanked the editor off the out-of-changeset file"
row_selected beta.txt || fail "C: the sidebar selection was lost on an out-of-changeset jump"
# The user-reported leak: cards key on the buffer the editor actually shows (nvim_card_file), not
# the changeset selection (diff_path, still beta). The bare jump to gamma must show no stale card
# from beta. (This alone does not catch the diff_path regression — apply doesn't repaint until a
# store bump — but it locks "a jump shows no leaked card" against jump-time repaint regressions.)
frame | grep -qF "BETANOTE_B" && fail "C1: the in-changeset comment card leaked onto the out-of-changeset buffer on jump"
echo "ok C1 - the jump shows no stale in-changeset comment card on the out-of-changeset buffer"
keys -l '3G'; sleep 0.2
keys Space r c; sleep 0.4
keys -l 'GAMMANOTE'; keys Enter; sleep 0.5
wait_for "╭─ comment"
# Authoring a comment on gamma bumps the store, re-firing apply on the gamma buffer. Keyed on
# diff_path (=beta) that repaint painted beta's BETANOTE_B here and dropped GAMMANOTE — the exact
# user-reported leak. Assert the leak's ABSENCE first (it names the bug), then that gamma's own
# card did paint; both must hold at once.
frame | grep -qF "BETANOTE_B" && fail "C2: the in-changeset comment card leaked onto the out-of-changeset buffer after a comment there"
frame | grep -qF "GAMMANOTE" || fail "C: comment card on the out-of-changeset file did not paint"
echo "ok C - an out-of-changeset jump keeps the selection put, paints its own cards without leaking the in-changeset comment, and is not yanked"

esc
# Hand focus to the file list before quitting. Tab toggles focus, but the host drops it back into
# the editor when nvim's mode grid hasn't settled to normal yet — a fast Tab right after the
# composer closes lands in that window, forwarding `q` into the editor as a macro-record and
# wedging the quit (the "reviewer did not quit" flake). So send Tab only while the editor still
# holds focus and confirm the list took it by its surface2 fill — an event-wait, not a blind sleep.
for _ in $(seq 20); do list_focused beta.txt && break; keys Tab; sleep 0.25; done
list_focused beta.txt || fail "could not hand focus to the file list for a clean quit"
keys q; sleep 0.3; keys y 2>/dev/null || true
wait_session_end
echo "# all native-jump follow assertions passed"
