#!/usr/bin/env bash
# Live gate: the All files review walk (Enter) steps through EVERY file — changed or not —
# and skips ignored-directory placeholders instead of wedging on them. The Changes walk is
# covered by tui-nav-test.sh; this one guards the "all files, file by file" behaviour.
set -uo pipefail
SOCK=rvallwalk
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

# Root-level files so each is a top-level list row (no src/ tree collapse to expand). Sorted
# order: .gitignore, aaa.txt, bbb.txt, ccc.txt, then the ignored `target/` placeholder LAST —
# so the wedge (if any) only bites after the two unchanged files, and a wrap back to the
# changed file proves the placeholder was skipped.
printf 'AAAMARK\n' > "$REPO/aaa.txt"   # the only CHANGED file
printf 'BBBMARK\n' > "$REPO/bbb.txt"   # UNCHANGED (tracked)
printf 'CCCMARK\n' > "$REPO/ccc.txt"   # UNCHANGED (tracked)
printf 'target/\n' > "$REPO/.gitignore"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
printf 'AAA-CHANGED\n' >> "$REPO/aaa.txt"
mkdir -p "$REPO/target"; printf 'IGNOREDJUNK\n' > "$REPO/target/junk.txt"  # ignored, on disk

tui_start
wait_for "aaa.txt"
keys 2; sleep 0.8   # All files tab

# Open the changed file and focus the editor so Enter drives the walk.
read -r CP RP <<< "$(locate_right 'aaa.txt')"
[ -n "${CP:-}" ] || fail "aaa.txt row not visible in the All files list"
click "$CP" "$RP"
wait_for "AAA-CHANGED"
keys Tab; sleep 0.4

# Walk forward. The two UNCHANGED files must both be visited (all-files, not changeset), no
# ignored file inside target/ is ever opened, and the walk must wrap back onto the changed file
# rather than wedge on the ignored `target/` placeholder that follows ccc.txt in sort order.
saw_bbb=0; saw_ccc=0; wrapped=0
for _ in $(seq 8); do
  keys Enter; sleep 0.8
  f="$(frame)"
  grep -q "BBBMARK" <<<"$f" && saw_bbb=1
  grep -q "CCCMARK" <<<"$f" && saw_ccc=1
  grep -q "IGNOREDJUNK" <<<"$f" && fail "the walk opened an ignored file under target/"
  # Once we have left the changed file, seeing it again is a full wrap past the placeholder.
  { [ "$saw_bbb" = 1 ] && grep -q "AAA-CHANGED" <<<"$f"; } && wrapped=1
done
[ "$saw_bbb" = 1 ] || fail "the walk never visited the unchanged file bbb.txt"
[ "$saw_ccc" = 1 ] || fail "the walk never visited the unchanged file ccc.txt"
[ "$wrapped" = 1 ] || fail "the walk wedged: never wrapped back past the ignored target/ placeholder"
echo "ok 1 - All files Enter walks every file and skips the ignored directory placeholder"

keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all all-files-walk assertions passed"
