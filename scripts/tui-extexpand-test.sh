#!/usr/bin/env bash
# Live gate: `.` reveal-by-extension. From the All-files tree (folders collapsed by default),
# `.rs⏎` expands every folder that holds a .rs file so all .rs files show, while a folder whose
# only file is a different extension stays collapsed. An empty `.⏎` un-spams: it collapses every
# reveal back to the pre-reveal tree. A sibling of `x`/expand_changes, keyed on file extension.
set -uo pipefail
SOCK=rvextexp
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

mkdir -p "$REPO/alpha" "$REPO/beta" "$REPO/gamma"
printf 'fn a() {}\n' > "$REPO/alpha/one.rs"
printf 'package b\n' > "$REPO/beta/two.go"
printf 'fn c() {}\n' > "$REPO/gamma/three.rs"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A

# True iff $1 shows in the FILE-LIST pane (right of the divider) — not the editor pane.
list_has() { [ -n "$(locate_right "$1")" ]; }

tui_start
wait_for "Files"        # the file-list pane painted (Changes tab, "no changes")
keys 2                   # All files: the whole worktree tree, folders collapsed by default
wait_for "alpha"
wait_for "gamma"

# Collapsed at rest: the .rs files nested inside are not shown yet.
list_has "one.rs"   && fail "alpha started expanded (one.rs already visible)"
list_has "three.rs" && fail "gamma started expanded (three.rs already visible)"

# `.rs⏎` reveals every folder holding a .rs file.
keys -l "."; sleep 0.3
frame | grep -qF "Files  ." || fail "the . reveal prompt did not open"
keys -l "rs"; sleep 0.3
frame | grep -qF "Files  .rs" || fail "the typed extension did not echo in the prompt"
keys Enter; sleep 0.9
list_has "one.rs"   || fail ".rs did not reveal alpha/one.rs"
list_has "three.rs" || fail ".rs did not reveal gamma/three.rs"
# The .go folder holds no .rs file, so it stays collapsed — its file is not revealed.
list_has "two.go"   && fail ".rs wrongly revealed the .go-only folder"
echo "ok 1 - .rs reveals every folder holding a .rs file, leaves .go-only folders collapsed"

# Empty `.⏎` un-spams: collapse every reveal back to the pre-reveal tree.
keys -l "."; sleep 0.3
keys Enter; sleep 0.9
list_has "one.rs"   && fail "empty .⏎ did not collapse alpha back"
list_has "three.rs" && fail "empty .⏎ did not collapse gamma back"
list_has "alpha"    || fail "the tree vanished after collapse (alpha gone)"
echo "ok 2 - empty .⏎ collapses every reveal back to the pre-reveal tree"

keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all ext-expand assertions passed"
