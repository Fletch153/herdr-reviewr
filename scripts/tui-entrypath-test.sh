#!/usr/bin/env bash
# Live gate: entry-path view-model sync via NATIVE jumps — TAG jumps (Ctrl-]) and the jumplist
# (Ctrl-o), the two native entry paths no other gate drives (tui-jump/tui-livejump use only `:e`).
# After such a jump the view-model facts must all agree with the buffer nvim actually shows:
# (1) the sidebar selection, (2) the plain/focused stamp AND its PAINT (folds + signs, or none),
# (3) no stale paint bleeding from the previous buffer. Also pins the never-root-caused "No tag
# file" report to nvim's own honest error (reviewr keys the tag path nvim always has; a failed
# Ctrl-] leaves the selection + buffer intact and does not wedge the pane), and the plain-view
# entry path (a jump under All files stamps the jumped-to buffer plain, matching the tab, not the
# file's changeset membership). Covers matrix Row 4 (view model) + Row 2 (diff paint) timing/open.
set -uo pipefail
SOCK="entrypath$$"
export TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

seq -f 'a%g' 20 > "$REPO/src/alpha.txt"
seq -f 'b%g' 20 > "$REPO/src/beta.txt"
printf 'g1\ng2\nGAMMA_STABLE\ng4\ng5\n' > "$REPO/src/gamma.txt"   # committed, never changed
# A real ctags-format tags file at the repo root (cwd), so Ctrl-] resolves via &tags=...,tags.
# Line-number ex-commands (not /patterns/) so no destination content leaks into the tags buffer.
printf '!_TAG_FILE_SORTED\t0\t/coll/\nbetadef\tsrc/beta.txt\t10\ngammadef\tsrc/gamma.txt\t3\n' \
  > "$REPO/tags"
git -C "$REPO" add -A && git -C "$REPO" commit -qm base
# alpha's two changes ARE the tag identifiers, so a Ctrl-] from a changed line jumps out.
sed -i 's/^a10$/betadef/; s/^a15$/gammadef/' "$REPO/src/alpha.txt"
sed -i 's/^b10$/BETA_UNIQUE_EDIT/' "$REPO/src/beta.txt"

# Selected file-list row fill (surface2 focused / surface1 editor-focused). The name appears in
# both panes; isolate the bare list row and match either fill as a background. See tui-jump-test.
SELFILL='48;2;(69;71;90|88;91;112)'
list_row() { framee | grep -aF "$1" | grep -avF "src/$1" | grep -avF ':e '; }
row_selected() { list_row "$1" | grep -qaE "$SELFILL"; }
wait_row_selected() { for _ in $(seq 40); do row_selected "$1" && return 0; sleep 0.2; done; fail "the file list never highlighted $1"; }
# Control keys as CSI-u (crossterm parses CSI u unconditionally; a raw 0x1d is not reliably
# decoded as Ctrl-] outside the 0x01..0x1a letter range — see run_notes). mod 5 = Ctrl.
ctrl_bracket() { keys -l "$(printf '\033[93;5u')"; }    # ']' = 93 -> Ctrl-] (tag jump)
ctrl_o()       { keys -l "$(printf '\033[111;5u')"; }   # 'o' = 111 -> Ctrl-o (jumplist back)

tui_start
wait_for "alpha.txt"
wait_for "betadef"                              # Changes opens alpha, focused view shows its change
wait_for "unchanged lines"                      # focused-view fold sentinel present on alpha
row_selected alpha.txt || fail "did not start with alpha selected"

# --- T: a TAG jump (Ctrl-]) to an in-changeset file moves the selection AND paints focused ------
# EXPECT: editor shows beta's change, the sidebar follows to beta, beta renders FOCUSED (fold
# sentinel + its changed line), alpha is no longer selected, and NO "No tag file" error appears.
keys Tab; sleep 0.4                             # focus the editor (normal mode, cursor on 1st change)
esc
keys -l '/betadef'; keys Enter; sleep 0.3       # cursor squarely on the identifier
ctrl_bracket; sleep 0.9
frame | grep -q "BETA_UNIQUE_EDIT" || fail "T: the Ctrl-] tag jump to beta did not land"
frame | grep -qiE 'no tags? file|E43[36]|tag not found' && fail "T: a legit tag jump reported a tag error"
wait_row_selected beta.txt
row_selected alpha.txt && fail "T: the list still highlights alpha after the tag jump to beta"
frame | grep -q "unchanged lines" || fail "T: beta did not render in the focused view (no fold) after the jump"
echo "ok T - Ctrl-] to an in-changeset file: selection follows, focused paint lands"

# --- O: Ctrl-o (jumplist back) returns to alpha with alpha's paint, no stale beta paint ---------
# EXPECT: editor back on alpha (betadef shown), sidebar back to alpha, alpha's focused fold
# present, and beta's changed line no longer on screen (no stale paint from the jumped buffer).
ctrl_o; sleep 0.9
frame | grep -q "betadef" || fail "O: Ctrl-o did not return to alpha"
frame | grep -q "BETA_UNIQUE_EDIT" && fail "O: beta's content is stale on screen after returning to alpha"
wait_row_selected alpha.txt
row_selected beta.txt && fail "O: the list still highlights beta after Ctrl-o back to alpha"
frame | grep -q "unchanged lines" || fail "O: alpha lost its focused fold paint after the jumplist round trip"
echo "ok O - Ctrl-o returns to alpha: selection + focused paint restored, no stale beta paint"

# --- G: a TAG jump OUTSIDE the changeset keeps the selection put, editor is not yanked back -----
# EXPECT: editor shows gamma (out of changeset, no list row), the sidebar STAYS on alpha, a poll
# cycle does not yank the editor back, AND the editor pane TITLE follows to gamma (the title names
# the file nvim actually shows, not the stale changeset file the sidebar still points at).
esc
keys -l '/gammadef'; keys Enter; sleep 0.3
ctrl_bracket; sleep 0.9
frame | grep -q "GAMMA_STABLE" || fail "G: the Ctrl-] jump to the out-of-changeset gamma did not land"
sleep 0.8                                       # a poll cycle must not yank it back
frame | grep -q "GAMMA_STABLE" || fail "G: the host yanked the editor off the out-of-changeset file"
row_selected alpha.txt || fail "G: the sidebar selection was lost on an out-of-changeset tag jump"
# The pane border row (herdr-drawn '┌' box, not nvim's grid) must name the jumped-to file.
title_row() { frame | grep -aF '┌'; }
title_row | grep -qaF 'gamma.txt' || fail "G: the editor title did not follow the jump (stale title still on the changeset file)"
title_row | grep -qaF 'alpha.txt' && fail "G: the editor title still shows the stale changeset file after the jump"
echo "ok G - Ctrl-] outside the changeset: selection put, buffer held, and the title follows to gamma"

# --- N: the 'No tag file' intermittent — Ctrl-] with a tag miss, then with NO tags file ---------
# EXPECT (N1): tags file present but the word is not a tag -> nvim's own 'tag not found' (E426);
# EXPECT (N2): tags file removed -> 'no tags file' (E433). Each is a ONE-LINE cmdline error (not a
# hit-enter prompt), and each must leave the reviewer intact: editor stays on alpha, selection
# unchanged. This is nvim's honest report of the fixture's tag state, NOT a reviewr-injected
# message — reviewr keys the same tag path nvim always has. (Two errors stacked WITHOUT a dismiss
# raise nvim's own "Press ENTER" prompt, a modal state that swallows keys until cleared — expected
# nvim behavior, so the probe clears the cmdline between the two deliberate misses with esc.)
ctrl_o; sleep 0.6                               # back to alpha
esc
keys -l '2G'; sleep 0.2                         # line 2 = "a2", not a tag identifier (tags present)
ctrl_bracket; sleep 0.6
N1="$(frame | grep -aioE 'tag not found|E426|E433|no tags? file' | head -1)"
[ -n "$N1" ] || fail "N1: expected nvim to report a tag miss with the tags file present"
frame | grep -q "betadef" || fail "N1: a failed tag jump moved the editor off alpha"
row_selected alpha.txt || fail "N1: a failed tag jump disturbed the sidebar selection"
esc; sleep 0.3                                  # clear the one-line error before the next miss
rm -f "$REPO/tags"
ctrl_bracket; sleep 0.6                          # now: no tags file at all
N2="$(frame | grep -aioE 'no tags? file|E433|tag not found|E426' | head -1)"
[ -n "$N2" ] || fail "N2: expected nvim to report the missing tags file"
frame | grep -q "betadef" || fail "N2: a no-tags-file Ctrl-] moved the editor off alpha"
row_selected alpha.txt || fail "N2: a no-tags-file Ctrl-] disturbed the sidebar selection"
esc; sleep 0.3
echo "ok N - failed Ctrl-] is nvim's honest one-line error, selection+buffer intact (N1='${N1}' N2='${N2}')"

# --- P: entry path under the PLAIN view (All files) — the jumped-to buffer must be plain --------
# EXPECT: switch to All files (plain), Ctrl-] to beta -> editor shows beta, sidebar follows, and
# the buffer is PLAIN (no focused fold sentinel, no diff signs) — the stamp matches the tab, not
# the file's changeset membership. That this tab switch + jump work at all also proves the failed
# tag jumps above did NOT wedge the reviewer. Re-add the tags file for the jump to resolve.
printf '!_TAG_FILE_SORTED\t0\t/coll/\nbetadef\tsrc/beta.txt\t10\n' > "$REPO/tags"
keys Tab; sleep 0.4                              # editor -> files pane focus (so digits switch tabs)
keys -l 2; sleep 0.9                             # All files tab
wait_gone "unchanged lines"                      # All files opens plain: no focused fold anywhere
keys Tab; sleep 0.4                              # focus editor
esc
keys -l '/betadef'; keys Enter; sleep 0.3        # alpha's identifier is visible in All files too
ctrl_bracket; sleep 0.9
frame | grep -q "BETA_UNIQUE_EDIT" || fail "P: the Ctrl-] tag jump did not land in All files"
wait_row_selected beta.txt
frame | grep -q "unchanged lines" && fail "P: the jumped-to buffer rendered FOCUSED under the All-files (plain) view"
echo "ok P - a native jump under All files stamps the jumped-to buffer plain, selection follows"

esc
keys Tab; sleep 0.3
keys q; sleep 0.3; keys y 2>/dev/null || true
wait_session_end
echo "# all entry-path view-model assertions passed"
