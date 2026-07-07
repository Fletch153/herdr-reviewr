#!/usr/bin/env bash
# Live gate for the mouse surface (synthesized SGR): file-row click opens, grid click focuses
# the editor, dir-row click expands the tree, the wheel scrolls the buffer, the header Send
# button routes to the host store, and the scope chip cycles.
set -uo pipefail
SOCK=rvmouse
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

{ for i in $(seq 1 60); do printf 'alpha line %02d\n' "$i"; done; } > "$REPO/src/long.txt"
printf 'shorty one\n' > "$REPO/src/zz_short.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
printf 'CHANGED HEAD\n' >> "$REPO/src/long.txt"
printf 'CHANGED SHORT\n' >> "$REPO/src/zz_short.txt"

tui_start
wait_for "long.txt"
wait_for "CHANGED HEAD"

# 1. Click the zz_short.txt row in the file list → it opens in the editor.
read -r C R <<< "$(locate_right "zz_short.txt")"
[ -n "${C:-}" ] || fail "cannot locate the zz_short row"
click "$C" "$R"
wait_for "shorty one"
echo "ok 1 - clicking a file row opens it"

# 2. Click inside the grid → focus moves to the editor (footer hint flips).
read -r C2 R2 <<< "$(locate "shorty one")"
click "$C2" "$R2"
wait_for "keys go to the editor"
echo "ok 2 - grid click focuses the editor"

# 3. Back to the files pane (tab digits live there in nvim mode), All files, expand the tree
#    by clicking the dir row, open the long file, and wheel over the grid. Vim buffer-cursor
#    memory places the reopen at its last spot (EOF change), so the wheel scrolls UP.
keys Tab; sleep 0.3
keys 2; sleep 0.8
read -r CD RD <<< "$(locate_right "src/")"
[ -n "${CD:-}" ] || fail "no src/ dir row on All files"
click "$CD" "$RD"
wait_for "long.txt"
echo "ok 3a - clicking the dir row expands the tree"
read -r CL RL <<< "$(locate_right "long.txt")"
click "$CL" "$RL"
wait_for "alpha line 2"
read -r CG RG <<< "$(locate "alpha line 2")"
for _ in 1 2 3 4 5 6 7 8; do wheel_up "$CG" "$RG"; sleep 0.1; done
wait_for "alpha line 03"
echo "ok 3 - wheel over the grid scrolls the editor"

# 4. Header: click the Send button with no comments → status says so.
read -r CS RS <<< "$(locate "Send (0)")"
click "$CS" "$RS"
wait_for "no comments to send"
echo "ok 4 - header send click routes to the host store"

# 5. Click the scope chip cycles the scope label.
read -r CC RC <<< "$(locate "[commit]")"
[ -n "${CC:-}" ] || fail "no [commit] chip visible"
click "$CC" "$RC"
wait_gone "[commit]"
echo "ok 5 - scope chip click cycles the scope"

keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all mouse assertions passed"
