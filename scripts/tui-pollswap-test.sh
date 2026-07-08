#!/usr/bin/env bash
# Live gate (race-audit c1.p2, subsystem 2: poll tick vs user input). Regression lock for the
# poll-rebuild-vs-tab-switch window: a churning changeset rebuilt by the poll, interleaved with
# tab switches 1<->2. The stash gate churns nothing; the livejump gate never leaves the Changes
# tab. This collides both: files enter/leave the changeset (index-shifting the entries) WHILE the
# user round-trips 1<->2, at --poll 100 so a rebuild lands around each swap.
# Teeth: replacing reload()'s anchor-based file_cursor re-derivation (app.rs ~993) with a raw
# first_file_row snap turns R1 RED (the earlier-sorting churned-in entry steals the selection);
# GREEN on the real code. The invariant it guards is "reload re-anchors the cursor by PATH, never
# by raw index," so an entries rebuild can never land a selection on the wrong file.
# Invariants:
#   I1 no wrong-file: cursor anchored on anchor.txt still shows ANCHORBODY after churn+swap.
#   I2 index-shift immune: an earlier-sorting file entering the changeset must not drag the
#      selection onto it (anchor design re-derives file_cursor by path, not raw index).
#   I3 no cross-tab entries bleed: a committed-clean file (AllFiles-only) never appears on Changes.
#   I4 no cross-tab filter bleed: a Changes "/an" filter never shows on AllFiles.
#   I5 no crash: the reviewer quits through its own path.
set -uo pipefail
SOCK="rvps$$"
export TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

# On failure, quit the reviewer through its OWN path first so the shared cleanup sees the session
# end gracefully — no SIGHUP-wedge, no loud orphan-embed reap. Dump the frame BEFORE quitting so
# the failure stays diagnosable. Best-effort (focus may be anywhere on a mid-gate fail); if the
# quit doesn't take, tui_cleanup still reaps the fixture embed. Overrides tui-lib fail() here only.
fail() {
  echo "FAIL - $*"
  frame
  if $TMUX has-session 2>/dev/null; then
    keys Escape 2>/dev/null; sleep 0.2; keys q 2>/dev/null; sleep 0.4; keys y 2>/dev/null || true
  fi
  exit 1
}

tui_start() {
  $TMUX new-session -d -x 200 -y 50 \
    "cd '$REPO' && exec '$BIN' --editor nvim --poll 100" \
    || { echo "FAIL: tmux session"; exit 1; }
}

# All tracked at base. Membership in the Changes set is toggled by append (in) / checkout (out).
printf 'an1\nan2\nANCHORBODY\n' > "$REPO/src/anchor.txt"     # kept-selected file
printf 'a0\n'                    > "$REPO/src/aaa.txt"        # sorts FIRST; churned in/out
printf 'z0\n'                    > "$REPO/zzzonly.txt"        # committed-clean, AllFiles-only telltale
git -C "$REPO" add -A && git -C "$REPO" commit -qm base
printf 'ANCHOR_CHANGE\n' >> "$REPO/src/anchor.txt"           # anchor.txt is in the changeset from the start

# Isolate the file-list row background fill (selected row), same technique as tui-livejump-test.
SELFILL='48;2;(69;71;90|88;91;112)'
list_row() { framee | grep -aF "$1" | grep -avF "src/$1" | grep -avF ':e '; }
row_selected() { list_row "$1" | grep -qaE "$SELFILL"; }
wait_row_selected() { for _ in $(seq 40); do row_selected "$1" && return 0; sleep 0.2; done; fail "list never highlighted $1"; }

churn_in()  { printf '%s\n' "$1" >> "$REPO/src/aaa.txt"; }        # aaa.txt joins the changeset
churn_out() { git -C "$REPO" checkout -- src/aaa.txt; }            # aaa.txt leaves the changeset

tui_start
wait_for "anchor.txt"
wait_for "ANCHORBODY"
row_selected anchor.txt || fail "did not start with anchor.txt selected"
echo "ok setup - anchor.txt selected, ANCHORBODY visible"

# Round 1: bring aaa.txt (sorts first) INTO the changeset, forcing every later entry's index up by
# one, WHILE round-tripping 2 then 1. The poll rebuild must re-anchor the cursor by path.
churn_in AAACHURN1
keys 2; sleep 0.15                 # -> All files (mid-churn)
churn_in AAACHURN2
keys 1; sleep 0.15                 # -> back to Changes (mid-churn)
# Event-wait (not a fixed sleep) for the poll rebuild to re-anchor the cursor by PATH: under
# full-suite load the rebuild+repaint can lag well past a bare 0.5s. This does NOT weaken I1/I2 —
# a real raw-index race would re-anchor onto aaa.txt and NEVER re-select anchor.txt, so the wait
# would time out and fail; and the aaa.txt guard below still fires on the wrong-file bug.
wait_row_selected anchor.txt       # I1/I2: selection settles back on anchor.txt after the shift
wait_for "ANCHORBODY"              # I1: diff pane repaints anchor.txt (a separate pane, can lag the list)
row_selected aaa.txt && fail "I2: the selection jumped onto the earlier-sorting new entry (raw-index bug)"
echo "ok R1 - anchor selection survives an index-shifting churn across a 2->1 round trip"

# Round 2: churn aaa.txt OUT while on All files, then return to Changes. The Changes stash was frozen
# with aaa.txt present; on return, reload must reconcile to the now-absent entry without stranding
# the cursor on a vanished row.
keys 2; sleep 0.2
churn_out
keys 1
# Two event-waits, not a fixed sleep: first the rebuild must drop the reconciled entry, THEN the
# selection must settle back on anchor.txt (its highlight can repaint a frame after aaa's row text
# clears). A real strand-on-vanished-row bug leaves anchor unselected, so wait_row_selected times
# out and fails — the invariant keeps its teeth.
wait_gone "aaa.txt"          # poll rebuild reconciles the removed entry off Changes
wait_row_selected anchor.txt # R2: the cursor is not stranded — it settles on anchor.txt
wait_for "ANCHORBODY"        # R2: diff pane repaints anchor.txt after the reconcile (pane can lag)
frame | grep -qF "aaa.txt" && fail "R2: the removed entry still lists on Changes after a poll rebuild"
echo "ok R2 - a stash-time entry removed while away reconciles cleanly on return"

# Round 3: filter Changes to anchor, verify it does not bleed onto All files, and that the
# committed-clean file never crosses into the Changes list. Interleave a churn under the filter.
keys -l "/"; sleep 0.3
keys -l "an"; sleep 0.3
wait_for "/an"
frame | grep -qF "zzzonly.txt" && fail "I3: the committed-clean AllFiles-only file leaked into Changes"
keys Enter; sleep 0.3          # confirm filter (Normal mode, query kept)
churn_in AAACHURN3            # a poll rebuild under the active filter must respect it (aaa stays hidden)
# Event-wait with teeth (not a fixed sleep): append a fresh line to the SELECTED file so the next
# poll re-renders its diff. When ANCHORMARK3 appears, a rebuild has run AFTER the churn — and the
# same rebuild recomputed the changeset, so if the /an filter were bleeding aaa.txt would already
# list. A bare sleep could pass before the rebuild even picked up the churn, masking a bleed.
printf 'ANCHORMARK3\n' >> "$REPO/src/anchor.txt"
wait_for "ANCHORMARK3"
frame | grep -qF "aaa.txt" && fail "I4-pre: the /an filter did not hide the churned-in aaa.txt on Changes"
keys 2; sleep 0.4
wait_for "zzzonly.txt"        # All files lists the clean file (its own, unfiltered view)
frame | grep -qF "/an" && fail "I4: the Changes /an filter text leaked into the All files pane"
keys 1; sleep 0.4
wait_for "/an"                # filter restored on return
wait_for "ANCHORBODY"        # I1: anchor.txt diff repaints after the filtered round trip (pane can lag)
echo "ok R3 - per-tab filter holds under churn, no clean-file bleed, restored on return"

keys Escape; sleep 0.3        # clear filter
keys q; sleep 0.4; keys y 2>/dev/null || true
wait_session_end
echo "# poll-vs-swap probe complete (all invariants held)"
