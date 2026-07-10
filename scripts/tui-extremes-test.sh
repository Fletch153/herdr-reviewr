#!/usr/bin/env bash
# THROWAWAY probe: size/content extremes (class 4). One fixture, four hostile-but-legal files all
# listed in Changes: a huge (5000-line) modification, a 0-byte added file, a binary (NUL) added
# file, and a 20000-char single-line file. Expected: every file lists, the editor renders/paints/
# folds the huge diff WITHOUT hanging or crashing, opening the binary/zero/long files never crashes
# the pane, the reviewer stays responsive (tab switch still works), and it quits with no orphan embed.
set -uo pipefail
SOCK=rvextremes
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

# huge.txt: 5000 lines committed, then 3 scattered edits => a real modification whose folds must
# collapse the ~4994 unchanged lines. Stresses paint (extmarks) + foldexpr/foldtext on a big buffer.
python3 - "$REPO/huge.txt" <<'PY'
import sys
open(sys.argv[1], "w").write("".join(f"line {i}\n" for i in range(1, 5001)))
PY
git -C "$REPO" add -A && git -C "$REPO" commit -qm base
python3 - "$REPO/huge.txt" <<'PY'
import sys
p = sys.argv[1]; ls = open(p).read().splitlines()
ls[1] = "line 2 EDITED"; ls[2500] = "line 2501 EDITED"; ls[4999] = "line 5000 EDITED"
open(p, "w").write("\n".join(ls) + "\n")
PY

# zero.txt: 0-byte added. bin.dat: NUL bytes (binary) added. long.txt: one 20000-char line added.
: > "$REPO/zero.txt"
printf 'A\000B\000C\000\001\002\003more\000bytes\n' > "$REPO/bin.dat"
python3 -c "open('$REPO/long.txt','w').write('x'*20000 + '\n')"

row_present() { [ -n "$(locate_right "$1")" ]; }
open_listed() {  # click the list row for $1 (must be listed) and give the editor a beat
  local c r; read -r c r <<< "$(locate_right "$1")"
  [ -n "${c:-}" ] || fail "$1 not listed"
  click "$c" "$r"
}

tui_start
wait_for "Changes"

# STEP 0: all four extreme files list in Changes (uncommitted scope).
for f in huge.txt zero.txt bin.dat long.txt; do
  for _ in $(seq 40); do row_present "$f" && break; sleep 0.25; done
  row_present "$f" || fail "step0: $f not listed in Changes"
done
echo "ok 0 - huge / zero / binary / long-line files all list in Changes"

# STEP 1: open huge.txt. Expected: the edited line paints AND folds collapse the unchanged bulk
# (foldtext "unchanged lines"); the reviewer does not hang; the session stays alive.
open_listed huge.txt
wait_for "line 2 EDITED"
frame | grep -qF "unchanged lines" || fail "step1: huge diff did not fold its unchanged bulk"
$TMUX has-session 2>/dev/null || fail "step1: reviewer died opening the huge file"
echo "ok 1 - huge diff paints its edit and folds the unchanged bulk (no hang, session alive)"

# STEP 2: with the huge buffer behind us, open the 0-byte, binary, and long-line files in turn.
# Expected: each opens without crashing the pane; the session survives every one.
open_listed zero.txt; sleep 0.5
$TMUX has-session 2>/dev/null || fail "step2: reviewer died opening the 0-byte file"
open_listed bin.dat; sleep 0.8
$TMUX has-session 2>/dev/null || fail "step2: reviewer died opening the binary file"
open_listed long.txt; sleep 0.8
$TMUX has-session 2>/dev/null || fail "step2: reviewer died opening the 20000-char line"
frame | grep -qF "Changes" || fail "step2: pane lost its chrome after the extreme files"
echo "ok 2 - zero-byte / binary / very-long-line files each open without crashing the pane"

# STEP 3: the reviewer is still RESPONSIVE to input after all the extremes — switch tabs.
keys 2
wait_for "All files"
echo "ok 3 - reviewer still processes input (switched to All files) after the extremes"

keys 1; sleep 0.3
keys q; sleep 0.3; keys y 2>/dev/null || true
wait_session_end
echo "# extremes probe complete"
