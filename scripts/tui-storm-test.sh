#!/usr/bin/env bash
# Live gate for input storms racing the walk protocol: a burst of Enters/BSpaces at a file
# boundary advances exactly ONE file (stale boundary verdicts from the editor's lagging
# buffer are dropped, never marking unseen files reviewed); a revert-fired boundary tolerates
# a trailing Enter; a double-tap insert flips once and fires once; and the walk stays correct
# across a poll entries-rebuild fed by an external write.
set -uo pipefail
SOCK=rvstorm
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

seq -f 'a%g' 30 > "$REPO/src/fa.txt"
seq -f 'b%g' 30 > "$REPO/src/fb.txt"
seq -f 'c%g' 30 > "$REPO/src/fc.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
sed -i 's/^a2$/MARKA1/; s/^a25$/MARKA2/' "$REPO/src/fa.txt"
sed -i 's/^b3$/MARKB1/; s/^b26$/MARKB2/' "$REPO/src/fb.txt"
sed -i 's/^c5$/MARKC1/' "$REPO/src/fc.txt"

tui_start
wait_for "MARKA1"
wait_for "unchanged lines"

# 1. Enter storm at the forward file boundary. fa has 2 hunks; one Enter steps to the last,
#    then THREE Enters in one burst hit the boundary. EXPECTED: exactly one file advance —
#    fa marked (1 reviewed), fb open on its first hunk; fb/fc NOT silently marked or skipped
#    (the 2nd/3rd boundary verdicts were computed against fa's stale buffer).
keys Tab; sleep 0.4
keys Enter; sleep 0.8          # in-file step to MARKA2 (the last hunk)
frame | grep -q "file reviewed" && fail "the in-file step advanced"
keys Enter Enter Enter         # the storm: one real boundary + two stale repeats
sleep 2.5
frame | grep -q "all files reviewed" && fail "the storm marked the whole changeset reviewed"
frame | grep -qF "MARKB1" || fail "the storm did not land on fb"
frame | grep -q "1 reviewed" || fail "expected exactly one reviewed tick after the storm"
echo "ok 1 - an Enter storm at the boundary advances exactly one file"

# 2. BSpace storm at the backward boundary. From fb's first hunk, TWO BSpaces in one burst.
#    EXPECTED: one retreat to fa (on its last hunk); the stale 2nd must not wrap toward fc.
keys BSpace BSpace
sleep 2
frame | grep -qF "MARKA2" || fail "the backward storm did not land on fa"
frame | grep -qF "MARKC1" && fail "the stale BSpace wrapped the walk to fc"
echo "ok 2 - a BSpace storm retreats exactly one file"

# 3. Revert + Enter interleave: reverting fa's last remaining hunk fires the same boundary
#    nav; an Enter right behind it is stale. EXPECTED: a single advance to fb, fc untouched;
#    fa fully reverted on disk.
keys Space r h; sleep 1        # revert MARKA2 (the cursor sits there after the place-last entry)
frame | grep -q "no hunk under cursor" && fail "the cursor was not on fa's last hunk"
keys BSpace; sleep 0.8         # in-file step back to MARKA1 (now the only hunk)
keys Space r h Enter           # revert the last hunk (fires nav next) + an immediate stale Enter
sleep 2.5
frame | grep -qF "MARKB1" || fail "the revert boundary did not advance to fb"
frame | grep -qF "MARKC1" && fail "the stale Enter behind the revert skipped fb"
grep -q "MARKA" "$REPO/src/fa.txt" && fail "fa did not fully revert on disk"
echo "ok 3 - revert-at-boundary plus a trailing Enter advances exactly once"

# 4. Double-tap insert flip: ii in one burst inside the locked view. EXPECTED: one flip to
#    the plain view, insert mode entered ONCE — the typed text lands with no stray literal i.
keys i i
wait_gone "unchanged lines"
sleep 0.6
keys -l "XYZPAYLOAD"
esc
wait_for "XYZPAYLOAD"
for _ in $(seq 20); do grep -q "XYZPAYLOAD" "$REPO/src/fb.txt" 2>/dev/null && break; sleep 0.25; done
grep -q "XYZPAYLOAD" "$REPO/src/fb.txt" || fail "the flipped insert did not autosave"
grep -q "iXYZPAYLOAD" "$REPO/src/fb.txt" && fail "the second insert intent double-fired a literal i"
echo "ok 4 - a double-tap insert flips once and fires once"

# 5. Walk across a poll entries-rebuild: an external (agent) write lands a NEW hunk in fc
#    while fb is under review; after the rebuild the boundary advance must open fc with the
#    fresh hunk painted. EXPECTED: MARKC2 visible once the walk reaches fc.
keys -l "$(printf '\033[105;5u')"   # ctrl+i (kitty CSI u): back to the Changes review of fb
wait_for "unchanged lines"
sed -i 's/^c20$/MARKC2/' "$REPO/src/fc.txt"
sleep 1.2                            # > poll: entries rebuilt with the fresh fc
for _ in $(seq 8); do                # walk fb's hunks to the boundary, then advance
  frame | grep -qF "MARKC1" && break
  keys Enter; sleep 0.6
done
frame | grep -qF "MARKC1" || fail "the walk never reached fc after the rebuild"
frame | grep -qF "MARKC2" || fail "fc opened without the freshly-written hunk"
echo "ok 5 - the walk advances correctly across a poll entries-rebuild"

# 6. Insert + Enter in one burst at the last hunk: the editor (still locked) emits the insert
#    intent AND a boundary nav before the flip publishes. EXPECTED: the flip wins — the nav is
#    a locked-view verdict arriving after the tab moved (dropped by its view tag), the walk
#    does NOT advance, and the typed payload lands in fc, never in the file the stale nav
#    would have wrapped to.
keys Enter; sleep 0.8               # in-file step from MARKC1 to MARKC2 (fc's last hunk)
keys i Enter                        # one burst: authoring intent + a stale boundary verdict
sleep 2
frame | grep -qF "MARKB1" && fail "the stale nav behind the insert flip advanced the walk"
keys -l "FLIPRACE"
esc
for _ in $(seq 20); do grep -q "FLIPRACE" "$REPO/src/fc.txt" 2>/dev/null && break; sleep 0.25; done
grep -q "FLIPRACE" "$REPO/src/fc.txt" || fail "the flipped insert did not land in fc"
grep -q "FLIPRACE" "$REPO/src/fb.txt" && fail "the insert landed in fb (the stale nav's file)"
echo "ok 6 - an insert+Enter burst flips without advancing the walk"

keys Tab; sleep 0.3
keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all storm assertions passed"
