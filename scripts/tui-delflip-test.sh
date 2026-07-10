#!/usr/bin/env bash
# Live gate: a file open+painted in the locked Changes view that is DELETED underneath must switch
# to the all-red deleted scratch, and the flip/paste authoring keys on it must stay inert (no
# resurrection). Regression lock for the same-view dedup existence fold (src/lib.rs nvim_sync):
# without it the early return kept the stale working-copy buffer up (TAIL CHANGE) while the
# file-list marked the file deleted, and show_deleted never ran. Covers Row 5 file-state
# (flip-on-deleted), Row 2/4 file-state x tab (deleted scratch survives a tab round-trip).
# STATED expectations:
#   0. mod.rs opens in Changes: painted (TAIL CHANGE visible), locked (x -> E21).
#   1. rm mod.rs; poll converts the editor to the deleted scratch: base content, "_" del signs,
#      TAIL CHANGE gone (TAIL is worktree-only, not in base). File absent on disk.
#   2. i on the deleted scratch: nomodifiable, no edit-maps -> E21, NO flip to All files, NO insert.
#      mod.rs stays deleted (not resurrected).
#   3. bracketed paste on the deleted scratch: the Changes paste-arm flips + delivers via nvim_paste,
#      which errors on a nomodifiable buffer -> inert. RESURRECT text never reaches disk; mod.rs absent.
#   4. tab switch 1<->2 with a deleted file selected: deleted scratch survives, no crash, no stale
#      TAIL paint bleeding back, no resurrection.
set -uo pipefail
SOCK=rvdelflip
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

# poll 100 to force poll/checktime races.
tui_start() {
  $TMUX new-session -d -x 200 -y 50 "cd '$REPO' && exec '$BIN' --editor nvim --poll 100" \
    || { echo "FAIL: tmux session"; exit 1; }
}

{ for i in $(seq 1 15); do printf 'base line %02d\n' "$i"; done; } > "$REPO/mod.rs"
printf 'keep\n' > "$REPO/keep.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
printf 'TAIL CHANGE\n' >> "$REPO/mod.rs"

tui_start
wait_for "mod.rs"
wait_for "TAIL CHANGE"
wait_for "unchanged lines"   # Changes fold sentinel: mod.rs is the focused locked view

# 0. locked: x is inert (E21), disk untouched.
keys Tab; sleep 0.3
keys x; sleep 0.5
frame | grep -q "E21" || fail "x in the locked Changes view did not answer E21"
grep -q "TAIL CHANGE" "$REPO/mod.rs" || fail "x mutated the file on disk"
echo "ok 0 - mod.rs open, painted, locked in Changes"

# 1. delete underneath; the poll turns it into the deleted scratch.
rm "$REPO/mod.rs"
wait_gone "TAIL CHANGE"          # worktree-only line gone once the deleted scratch (base) shows
wait_for "base line 15"          # base content is what the deleted scratch renders
[ -e "$REPO/mod.rs" ] && fail "mod.rs still on disk after rm"
echo "ok 1 - deleted file becomes the all-red deleted scratch"

# 2. i on the deleted scratch: E21, no flip, no resurrection.
keys i; sleep 0.6
frame | grep -q "E21" || fail "i on the deleted scratch did not answer E21 (flipped or inserted?)"
[ -e "$REPO/mod.rs" ] && fail "i on the deleted scratch RESURRECTED mod.rs"
echo "ok 2 - i on a deleted file is inert (E21), no resurrection"

# 3. bracketed paste on the deleted scratch: inert, never reaches disk.
esc
keys -l "$(printf '\033[200~RESURRECT\033[201~')"; sleep 0.8
[ -e "$REPO/mod.rs" ] && fail "paste RESURRECTED mod.rs"
echo "ok 3 - paste on a deleted file never resurrects it"

# 4. tab switch with a deleted file selected: scratch survives, no crash, no stale TAIL paint.
keys 2; sleep 0.6
keys 1; sleep 0.6
frame | grep -q "TAIL CHANGE" && fail "stale worktree TAIL paint bled back after a tab round-trip"
$TMUX has-session 2>/dev/null || fail "reviewer died on the deleted-file tab round-trip"
[ -e "$REPO/mod.rs" ] && fail "tab round-trip RESURRECTED mod.rs"
echo "ok 4 - deleted scratch survives a tab round-trip, no resurrection"

keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all delflip probe assertions passed"
