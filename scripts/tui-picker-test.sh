#!/usr/bin/env bash
# Live gate: filter, commit picker, branch picker, + send-path, and bracket resize keys.
set -uo pipefail
SOCK=rvpick
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

printf 'one\n' > "$REPO/src/aaa_first.txt"
printf 'two\n' > "$REPO/src/zzz_last.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
git -C "$REPO" checkout -qb feature
printf 'B-CHANGE\n' >> "$REPO/src/aaa_first.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm B
printf 'C-CHANGE\n' >> "$REPO/src/zzz_last.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm C
printf 'UNCOMMITTED\n' >> "$REPO/src/aaa_first.txt"
printf 'UNCOMMITTED\n' >> "$REPO/src/zzz_last.txt"

tui_start
wait_for "aaa_first.txt"
wait_for "UNCOMMITTED"

# 1. Filter: / narrows the list (right pane only — the editor title still shows the open
#    file); Esc clears it.
keys /
keys -l "zzz"
sleep 0.5
[ -z "$(locate_right 'aaa_first')" ] || fail "filter did not narrow the list"
[ -n "$(locate_right 'zzz_last')" ] || fail "filter dropped the matching row"
keys Enter
sleep 0.3
esc
sleep 0.3
[ -n "$(locate_right 'aaa_first')" ] || fail "clearing the filter did not restore the rows"
echo "ok 1 - filter narrows and clears"

# 2. Commit picker: clicking the commit chip opens it; picking the older commit re-scopes.
read -r CP RP <<< "$(locate "[uncommitted]")"
[ -n "${CP:-}" ] || fail "no commit chip in the header"
click "$CP" "$RP"
wait_for "Compare with commit"
keys j; sleep 0.2; keys j   # row 0 is Uncommitted, so: Uncommitted -> C -> B (the older commit)
keys Enter
wait_gone "Compare with commit"
wait_gone "[uncommitted]"
# vs commit B, zzz_last's committed C-CHANGE is part of the diff: open it and expect green.
read -r CZ RZ <<< "$(locate_right "zzz_last.txt")"
[ -n "${CZ:-}" ] || fail "zzz_last row missing after re-scope"
click "$CZ" "$RZ"
for _ in $(seq 60); do frame | grep -qE '\+ +[0-9]+ C-CHANGE' && break; sleep 0.25; done
frame | grep -qE '\+ +[0-9]+ C-CHANGE' || fail "committed C-CHANGE not painted green vs commit B"
echo "ok 2 - commit picker re-scopes the changeset (committed line paints green)"

# 3. Branch picker: b for branch scope, click the base chip, overlay opens, esc closes.
keys b
sleep 0.5
read -r CB RB <<< "$(locate "auto:")"   # the base chip reads [>auto:<branch>] by default
[ -n "${CB:-}" ] || fail "no base chip visible on branch scope"
click "$CB" "$RB"
wait_for "Compare with branch"
esc
wait_gone "Compare with branch"
echo "ok 3 - branch picker opens from the base chip and closes"

# 4. + sends the highlighted FILE's path to the agent (highlight one first — the scope flip
#    left the cursor on the dir row).
read -r CF RF <<< "$(locate_right "zzz_last.txt")"
[ -n "${CF:-}" ] || fail "no zzz_last row to highlight"
click "$CF" "$RF"
sleep 0.3
: > "$REVIEWR_STUB_LOG"
keys +
for _ in $(seq 40); do grep -q "send wY:pFOCUS" "$REVIEWR_STUB_LOG" 2>/dev/null && break; sleep 0.25; done
grep -q "send wY:pFOCUS .*src/" "$REVIEWR_STUB_LOG" || { echo "stub log: $(cat "$REVIEWR_STUB_LOG")"; fail "+ did not deliver the path to the agent"; }
echo "ok 4 - + sends the file path"

# 5. [ and ] resize the panes from the keyboard.
D0=$(div_col)
keys [
sleep 0.4
D1=$(div_col)
keys ]
sleep 0.4
D2=$(div_col)
{ [ "$D1" != "$D0" ] && [ "$D2" != "$D1" ]; } || fail "[/] did not resize ($D0 -> $D1 -> $D2)"
echo "ok 5 - bracket keys resize the panes"

keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all picker/filter assertions passed"
