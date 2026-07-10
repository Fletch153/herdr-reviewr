#!/usr/bin/env bash
# Live gate: revert racing agent write / poll refresh (Row 7 x timing) and editor
# paint following rapid LIST j/k switches (Row 2 x timing). Forced --poll 100 to
# squeeze the reload() window against revert's forced write + nav-next walk. Locks
# that (a) rapid selection changes leave no stale intermediate paint, (b) a revert
# coexists with a concurrent agent write to another file, (c) a last-hunk revert
# advances the walk cleanly while polls churn the changeset underneath.
set -uo pipefail
SOCK=rvrevrace
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

seq -f 'a%g' 30 > "$REPO/src/one.txt"
seq -f 'b%g' 30 > "$REPO/src/two.txt"
seq -f 'c%g' 30 > "$REPO/src/three.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
sed -i 's/^a2$/ONEHUNK/' "$REPO/src/one.txt"
sed -i 's/^b3$/TWOHUNK/' "$REPO/src/two.txt"
sed -i 's/^c4$/THREEHUNK/' "$REPO/src/three.txt"

# --poll 100 (tui_start hardcodes 500); drive tmux directly.
$TMUX new-session -d -x 200 -y 50 "cd '$REPO' && exec '$BIN' --editor nvim --poll 100" \
  || { echo "FAIL: tmux session"; exit 1; }
wait_for "one.txt"
wait_for "ONEHUNK"
frame | grep -q "3 changed" || fail "expected 3 changed files at start"

# --- Row 2 x timing: rapid j/k lands editor paint on the FINAL file (list sorts
# alphabetically: one, three, two). EXPECT: rapid j j settles on two.txt showing
# TWOHUNK painted, with NO stale THREEHUNK (intermediate) or ONEHUNK (start) paint.
keys j; keys j; sleep 1.2
frame | grep -q "TWOHUNK" || fail "rapid j/k did not settle editor paint on two.txt"
frame | grep -q "THREEHUNK" && fail "stale three.txt paint survived rapid switch to two.txt"
frame | grep -q "ONEHUNK" && fail "stale one.txt paint survived rapid switch to two.txt"
frame | grep -qE '~ *[0-9]+ TWOHUNK' || fail "two.txt change not painted after rapid switch"
echo "ok 0 - rapid LIST j/k settles editor paint on the final file"

# --- Row 7: revert one.txt's hunk while an agent writes a DIFFERENT file, under poll ---
# EXPECT: revert of one.txt lands base 'a2' on disk (forced update!); the concurrent
# agent change to three.txt survives and stays listed; no lost revert, no crash.
keys k; keys k; sleep 0.8              # two -> three -> one (back to top)
frame | grep -q "ONEHUNK" || fail "did not return to one.txt"
keys Tab; sleep 0.4                    # focus editor
printf 'c%g\n' $(seq 30) | sed 's/^c9$/THREEAGENT/' > "$REPO/src/three.txt"  # agent write, other file
keys Space r h; sleep 1.2
grep -q "^a2$" "$REPO/src/one.txt" || fail "revert did not restore base a2 on disk"
grep -q "ONEHUNK" "$REPO/src/one.txt" && fail "reverted hunk still on disk"
for _ in $(seq 40); do frame | grep -q "THREEAGENT" && break; sleep 0.25; done
# three.txt must still be a changed file after the concurrent write + our revert poll churn.
grep -q "THREEAGENT" "$REPO/src/three.txt" || fail "agent write to three.txt was clobbered"
echo "ok 1 - revert of one.txt coexists with a concurrent agent write to three.txt"

# --- Row 7: revert one.txt's (now last) hunk -> walk advances, under --poll 100 churn ---
# EXPECT: one.txt drops from Changes; selection/walk advances to a still-changed file;
# editor paints that file's real change; changeset never strands on a clean/wrong file.
esc
keys Space r h; sleep 1.5             # one.txt had only ONEHUNK left -> last hunk -> nav next
frame | grep -q "ONEHUNK" && fail "one.txt hunk still visible after last-hunk revert"
# after walk, editor should show a genuine remaining change (two.txt or three.txt), painted.
frame | grep -qE '(TWOHUNK|THREEAGENT)' || fail "walk did not land editor on a remaining changed file"
grep -q "^a2$" "$REPO/src/one.txt" || fail "one.txt base not fully restored after last revert"
echo "ok 2 - last-hunk revert advances the walk cleanly under poll churn"

esc; keys Tab; sleep 0.4             # leave editor -> files pane for host-level quit
keys q; sleep 0.3; keys y 2>/dev/null || true
wait_session_end 2>/dev/null || true
echo "ok - revrace probe complete"
