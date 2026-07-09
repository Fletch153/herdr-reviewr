#!/usr/bin/env bash
# Live gate: a file open in the editor that is deleted underneath must STAY deleted when the
# reviewer switches away. The view-switch autosave must not write the orphaned buffer back to
# disk (nvim marks it modified on deletion), which would resurrect a file the user removed.
set -uo pipefail
SOCK=rvres
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

printf 'pub fn a() {}\n' > "$REPO/mod.rs"
printf 'other\n' > "$REPO/other.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A

tui_start
wait_for "All files"
keys 2; sleep 0.8   # All files lists committed files
wait_for "mod.rs"

# Open mod.rs while it still exists (a normal editable buffer with content).
read -r CP RP <<< "$(locate_right 'mod.rs')"
[ -n "${CP:-}" ] || fail "mod.rs row missing"
click "$CP" "$RP"
wait_for "pub fn a"

# Delete it underneath the open buffer, let a poll/checktime observe the deletion.
rm "$REPO/mod.rs"
sleep 1.2

# Switch to another file — this runs the leaving-buffer autosave.
read -r OC OR <<< "$(locate_right 'other.txt')"
[ -n "${OC:-}" ] || fail "other.txt row missing"
click "$OC" "$OR"; sleep 1.0

[ -e "$REPO/mod.rs" ] && fail "mod.rs was resurrected on disk by the view-switch autosave"
echo "ok 1 - deleting an open file underneath is not undone by switching away"

keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all resurrection assertions passed"
