#!/usr/bin/env bash
# Live gate: symlinks in the changeset degrade gracefully in the embedded editor.
# Three hostile-but-legal symlinks, all listed in Changes:
#   - link_in.txt   — a committed symlink re-pointed inside the tree (staged content change);
#   - link_out.txt  — a NEW symlink to a file OUTSIDE the repo (/etc/hostname);
#   - link_broken.txt — a NEW symlink to a non-existent target.
# Contract (graceful degradation, not a feature): each lists without crashing the reviewer,
# opens in the nvim editor without hanging/crashing the pane, the reviewer stays responsive
# (tab switch works), and quits through its own path with no orphan embed. A symlink blob is
# the target-path *text* (git mode 120000); the editor must not wedge on following/resolving it.
set -uo pipefail
SOCK=rvsymlink
source "$(dirname "$0")/tui-lib.sh"

# A real regular file that lives in the tree, plus another to re-point at.
printf 'alpha\nbeta\ngamma\n' > "$REPO/src/real.txt"
printf 'one\ntwo\n'          > "$REPO/src/other.txt"
ln -s real.txt "$REPO/src/link_in.txt"       # committed symlink -> real.txt
git -C "$REPO" add -A && git -C "$REPO" commit -qm A

ln -sf other.txt "$REPO/src/link_in.txt"     # re-point: staged symlink content change
ln -s /etc/hostname "$REPO/src/link_out.txt" # NEW symlink outside the repo
ln -s nonexistent_xyz "$REPO/src/link_broken.txt" # NEW broken symlink
git -C "$REPO" add -A

tui_start
wait_for "link_broken.txt"
sleep 1
# All three symlinks must appear in the Changes list — no crash deriving the changeset.
frame | grep -q "link_in.txt"     || fail "the re-pointed in-tree symlink is missing from Changes"
frame | grep -q "link_out.txt"    || fail "the outside-repo symlink is missing from Changes"
frame | grep -q "link_broken.txt" || fail "the broken symlink is missing from Changes"
echo "ok 0 - all three symlinks list in Changes without crashing the changeset derivation"

# Open each symlink in the editor; after each, the reviewer must still be alive and painting
# its list (the pane header/list survives), i.e. nvim did not wedge following the link.
for name in link_broken.txt link_out.txt link_in.txt; do
  read -r COL ROW <<< "$(locate_right "$name")"
  [ -n "${COL:-}" ] || fail "cannot locate $name in the Changes list"
  click "$COL" "$ROW"; sleep 0.6
  keys Enter; sleep 0.6
  # Liveness: the file list still renders every entry (pane not frozen/blanked by a wedge).
  frame | grep -q "link_broken.txt" || fail "the reviewer stopped painting its list after opening $name"
done
echo "ok 1 - opening each symlink leaves the reviewer responsive (no editor wedge)"

# Responsiveness proof: switch to All files (its tree root renders) and back to Changes
# (the symlinks reappear) — cross-tab liveness with a symlink open.
keys 2; sleep 0.6
frame | grep -q "src/" || fail "All-files tab did not render its tree after a symlink was open"
keys 1; sleep 0.6
frame | grep -q "link_broken.txt" || fail "Changes tab did not re-render its symlink list on return"
echo "ok 2 - tab switch works with a symlink open (reviewer not wedged)"

# Focus-correct quit from the files pane; no orphan embed (tui_cleanup warns loudly if any).
esc; keys q; sleep 0.4; keys y 2>/dev/null || true
wait_session_end
echo "# all symlink assertions passed"
