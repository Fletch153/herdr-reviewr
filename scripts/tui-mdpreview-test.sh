#!/usr/bin/env bash
# Live gate: a markdown file deleted underneath the reviewer while its preview is showing must
# not blank the preview pane. The rendered view falls back to the BASE content (what is being
# removed), mirroring the raw editor's show_deleted() path, rather than reading the now-empty
# worktree and painting a blank pane with the [md view] chip still lit.
set -uo pipefail
SOCK=rvmdprev
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

printf '# Doc\n\nMDBASEBODY\n' > "$REPO/a.md"
printf 'other base\n'        > "$REPO/b.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
printf '\nMDNEWCHANGE\n' >> "$REPO/a.md"   # a.md is now a changed markdown file
printf 'BCHANGE\n'       >> "$REPO/b.txt"

tui_start
wait_for "a.md"
wait_for "MDBASEBODY"   # a.md opens first (sorts before b.txt); raw view shows the body

# Turn the rendered preview on; it shows the worktree content (base + the change).
keys p; sleep 0.8
frame | grep -q "\[md view\]" || fail "preview did not turn on"
frame | grep -q "MDNEWCHANGE" || fail "preview did not render the worktree markdown"
echo "ok 1 - markdown preview renders the worktree content"

# Delete a.md underneath the reviewer while the preview is up; let the poll observe it.
rm "$REPO/a.md"
sleep 1.5

# The preview must NOT be blank: it falls back to the base content (MDBASEBODY, what is being
# removed). The worktree-only change line is gone with the file.
frame | grep -q "MDBASEBODY" || fail "the preview blanked on delete-underneath (no base fallback)"
echo "ok 2 - a deleted-underneath markdown previews its base content, not a blank pane"

keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all md-preview assertions passed"
