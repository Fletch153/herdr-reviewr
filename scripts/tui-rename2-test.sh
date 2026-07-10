#!/usr/bin/env bash
# Live gate: the two rename edge cases in Row 4 file-state, one fixture, both git-consistent.
#   - case-only rename (git mv Foo.txt foo.txt + one edit → R Foo.txt foo.txt): on the
#     case-sensitive fs the old/new paths are distinct, the rename map resolves the base against
#     Foo.txt, and only the edited line diffs (its old base line shows as a red virt line).
#   - rename-onto-deleted (git rm a.txt; git mv c.txt a.txt): a.txt already existed in HEAD, so
#     git does NOT mis-pair it as a rename of c.txt (which would silently drop a.txt's real change
#     and c.txt's deletion). git reports M a.txt (+10 −10) + D c.txt; the reviewer reflects exactly
#     that — a.txt paints as a full modification (~ mod signs over its new content), c.txt stays a
#     listed deletion. This is a characterization/regression lock (no product fix — behaviour is
#     already correct and consistent with the built-in pane).
set -uo pipefail
SOCK=rvrename2
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

{ for i in $(seq 1 12); do printf 'foo line %02d\n' "$i"; done; } > "$REPO/src/Foo.txt"
{ for i in $(seq 1 10); do printf 'alpha line %02d\n' "$i"; done; } > "$REPO/src/a.txt"
{ for i in $(seq 1 10); do printf 'gamma line %02d\n' "$i"; done; } > "$REPO/src/c.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
# case-only rename + edit
git -C "$REPO" mv src/Foo.txt src/foo.txt
sed -i 's/foo line 06/CASE EDIT 06/' "$REPO/src/foo.txt"
# rename-onto-deleted: rm a, mv c onto a (git: M a.txt +10 −10, D c.txt)
git -C "$REPO" rm -q src/a.txt
git -C "$REPO" mv src/c.txt src/a.txt
git -C "$REPO" add -A

tui_start
wait_for "foo.txt"
sleep 1

# 0. All three change rows listed; a.txt is a MODIFICATION (not a false rename of c.txt) and
#    c.txt is a live deletion — the rename-onto-deleted did not collapse the two.
[ -n "$(locate_right 'foo.txt')" ] || fail "foo.txt (case-only rename) not listed"
[ -n "$(locate_right 'a.txt')" ]   || fail "a.txt (rename-onto-deleted, modified) not listed"
[ -n "$(locate_right 'c.txt')" ]   || fail "c.txt (deleted) not listed"
frame | grep -qE 'a\.txt .*−10' || fail "a.txt not shown as a full modification (−10 base lines) — rename-onto-deleted was mis-paired as a rename"
echo "ok 0 - all three rows listed; a.txt modified (−10), c.txt deleted, no false rename pairing"

# 1. case-only rename paint: base resolves against Foo.txt via the rename map → only line 6 changed,
#    and its old base line 'foo line 06' renders as a red virt line. Base failing to resolve would
#    paint the whole file as an addition (no old-side virt line at all).
read -r COL ROW <<< "$(locate_right 'foo.txt')"
click "$COL" "$ROW"; sleep 0.6
wait_for "CASE EDIT 06"
frame | grep -q "foo line 06" || fail "case-only rename: old-side base line missing (rename map did not resolve Foo.txt)"
echo "ok 1 - case-only rename paints against its old-path base (Foo.txt)"

# 2. rename-onto-deleted paint: a.txt opens as a full modification — its new (gamma) content carries
#    the '~' modification sign on every line (not the '+' of a pure add, not the undecorated look of
#    a pure rename). This proves the editor treats it as M, matching git and the built-in pane.
read -r COL ROW <<< "$(locate_right 'a.txt')"
click "$COL" "$ROW"; sleep 0.6
wait_for "gamma line 01"
frame | grep -qE '~ +1 gamma line 01' || fail "rename-onto-deleted: a.txt's new content lacks the ~ modification sign (not painted as a modification)"
echo "ok 2 - rename-onto-deleted: a.txt paints as a modification (~ signs on its new content)"

# Quit from a list-row click (focus on the files pane so q quits, not a nvim count).
read -r COL ROW <<< "$(locate_right 'foo.txt')"; click "$COL" "$ROW"; sleep 0.3
esc
keys q; sleep 0.4; keys y 2>/dev/null || true
wait_session_end
echo "# all rename-edge-case assertions passed"
