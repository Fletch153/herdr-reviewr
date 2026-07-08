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
sleep 0.5                          # let a poll rebuild settle on the shifted list
row_selected anchor.txt || fail "I1/I2: the index-shifting churn dragged the selection off anchor.txt"
frame | grep -qF "ANCHORBODY" || fail "I1: the diff pane is not showing anchor.txt after churn+swap"
row_selected aaa.txt && fail "I2: the selection jumped onto the earlier-sorting new entry (raw-index bug)"
echo "ok R1 - anchor selection survives an index-shifting churn across a 2->1 round trip"

# Round 2: churn aaa.txt OUT while on All files, then return to Changes. The Changes stash was frozen
# with aaa.txt present; on return, reload must reconcile to the now-absent entry without stranding
# the cursor on a vanished row.
keys 2; sleep 0.2
churn_out
keys 1; sleep 0.5
row_selected anchor.txt || fail "R2: removing an entry while stashed stranded the Changes cursor"
frame | grep -qF "ANCHORBODY" || fail "R2: the diff pane lost anchor.txt after an entry left while stashed"
frame | grep -qF "aaa.txt" && fail "R2: the removed entry still lists on Changes after a poll rebuild"
echo "ok R2 - a stash-time entry removed while away reconciles cleanly on return"

# Round 3: filter Changes to anchor, verify it does not bleed onto All files, and that the
# committed-clean file never crosses into the Changes list. Interleave a churn under the filter.
keys -l "/"; sleep 0.3
keys -l "an"; sleep 0.3
wait_for "/an"
frame | grep -qF "zzzonly.txt" && fail "I3: the committed-clean AllFiles-only file leaked into Changes"
keys Enter; sleep 0.3          # confirm filter (Normal mode, query kept)
churn_in AAACHURN3            # a poll rebuild under the active filter must respect it
sleep 0.5
frame | grep -qF "aaa.txt" && fail "I4-pre: the /an filter did not hide the churned-in aaa.txt on Changes"
keys 2; sleep 0.4
wait_for "zzzonly.txt"        # All files lists the clean file (its own, unfiltered view)
frame | grep -qF "/an" && fail "I4: the Changes /an filter text leaked into the All files pane"
keys 1; sleep 0.4
wait_for "/an"                # filter restored on return
frame | grep -qF "ANCHORBODY" || fail "I1: anchor.txt not shown after the filtered round trip"
echo "ok R3 - per-tab filter holds under churn, no clean-file bleed, restored on return"

keys Escape; sleep 0.3        # clear filter
keys q; sleep 0.4; keys y 2>/dev/null || true
wait_session_end
echo "# poll-vs-swap probe complete (all invariants held)"
