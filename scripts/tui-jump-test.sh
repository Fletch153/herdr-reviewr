#!/usr/bin/env bash
# Live gate for native-jump comment rendering. The embedded editor can change its own current
# buffer without the host asking — a tag jump / Ctrl-], the jumplist, or :e — and can land on a
# file outside the changeset. Comment cards are keyed on the editor's REAL buffer, so a comment
# made or deleted after such a jump must still paint on the buffer it belongs to, immediately,
# without switching the file out and back. Regression lock for the user-reported desync where a
# comment on the jumped-to file was stored but never painted (and deleting one of two looked
# like "removed 2").
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

tui_start
wait_for "alpha.txt"

# --- A: in-changeset native jump paints a new comment's card immediately ---------------------
# All Files (the user's reported context); alpha opens plain, then :e jumps to beta behind the
# host's back (diff_path stays alpha until the editor's buf report moves it).
keys 2; sleep 0.5
wait_for "ALPHA_CHANGE"
keys Tab; sleep 0.4
esc
keys -l ':e src/beta.txt'; keys Enter; sleep 0.8
frame | grep -q "BETA_CHANGE" || fail "the :e jump to beta did not land"

keys -l '1G'; sleep 0.2
keys Space r c; sleep 0.4
keys -l 'BETANOTE_A'; keys Enter; sleep 0.5
wait_for "╭─ comment"
frame | grep -qF "BETANOTE_A" || fail "A: comment card on the jumped-to buffer did not paint"
echo "ok A - a comment on the jumped-to changeset file paints without switching out and back"

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

# --- C: a jump OUTSIDE the changeset gets its cards without a diff view or a yank -------------
# Changes tab: entries are the changed files only, so gamma (unchanged) has no diff row. The
# editor must stay on gamma (the host must not re-open a changed file over it) and still render
# a comment made on it.
keys 1; sleep 0.5
wait_gone "GAMMA_STABLE"                       # Changes shows a changed file, not gamma
esc                                            # editor keeps focus across the tab switch
keys -l ':e src/gamma.txt'; keys Enter; sleep 0.8
wait_for "GAMMA_STABLE"
sleep 0.8                                       # a poll cycle: the host must not yank it back
frame | grep -q "GAMMA_STABLE" || fail "C: the host yanked the editor off the out-of-changeset file"
keys -l '3G'; sleep 0.2
keys Space r c; sleep 0.4
keys -l 'GAMMANOTE'; keys Enter; sleep 0.5
wait_for "╭─ comment"
frame | grep -qF "GAMMANOTE" || fail "C: comment card on the out-of-changeset file did not paint"
echo "ok C - a comment on an out-of-changeset jump target paints and the editor is not yanked"

keys Escape
keys Tab; sleep 0.3
keys q; sleep 0.3; keys y 2>/dev/null || true
wait_session_end
echo "# all native-jump comment-render assertions passed"
