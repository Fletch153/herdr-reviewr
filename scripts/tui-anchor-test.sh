#!/usr/bin/env bash
# Live gate: the SURVIVAL invariant (specs/review-model.md — "Flag stale comments, never
# auto-drop"; model.rs — "removed by export or delete — never by a refresh"). A comment is
# removed ONLY by an explicit resolve/delete/export — NEVER silently by an edit, autosave,
# refresh, poll, or hunk revert. The frozen NvimAnchor (Comment.start/end are 1-based buffer
# lines captured at compose time; app.rs build_comment) deliberately does NOT re-bind to edited
# text — the `lines` snippet is the authoritative anchor — so on a pure edit nvim's extmark
# gravity drifts the card until the next store change re-presents it. That drift is expected;
# the only correctness bar is that the comment SURVIVES in the store.
#
# Gated on the STORE (the header's `Send (N)` unsent count) as the authoritative signal —
# card pixels drift/flake and are asserted only AFTER a Changes<->All-files flip forces a
# re-present. Covers the four sequences tui-cardflip does not: (1) insert lines ABOVE a
# comment, (2) edit the comment's OWN anchored line, (3) revert a hunk that CONTAINS the
# anchored line AND a hunk ABOVE it, (4) a poll refresh from a concurrent agent write
# elsewhere in the file. tui-cardflip already covers reverting the hunk BELOW a comment.
set -uo pipefail
SOCK=rvanchor
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

# one.txt: three well-separated single-line hunks (HTOP/HMID/HBOT, 10 lines apart) plus a
# stable context line CANCHOR2 that never changes — reverting any one or two hunks never
# empties the file off-screen (HBOT keeps it in the changeset throughout).
{
  for i in $(seq 1 30); do
    case $i in
      5)  echo HTOP0 ;;
      15) echo HMID0 ;;
      19) echo CANCHOR2 ;;
      25) echo HBOT0 ;;
      *)  printf 'c%02d\n' "$i" ;;
    esac
  done
} > "$REPO/src/one.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm base
sed -i 's/^HTOP0$/HTOP1/; s/^HMID0$/HMID1/; s/^HBOT0$/HBOT1/' "$REPO/src/one.txt"

tui_start
wait_for "one.txt"; wait_for "HTOP1"; wait_for "HMID1"; wait_for "HBOT1"

# --- setup: a diff-anchored comment on HMID1 (the middle hunk) in Changes -------------------
keys Tab; sleep 0.4                 # focus editor (Changes, focused diff)
keys Escape; sleep 0.2
keys -l '/HMID1'; keys Enter; sleep 0.3
keys Space r c; sleep 0.4
keys -l 'DNOTE_HMID'; keys Enter; sleep 0.6
wait_for "╭─ comment"
frame | grep -qF "DNOTE_HMID" || fail "setup: diff comment card did not paint on HMID1"
wait_for "Send (1)"
echo "ok setup - diff comment on HMID1, Send (1)"

# helpers: flip the active tab away and back to force a full re-present; leave editor focused.
# (digit tab-switches only fire with the FILES pane focused — editor focus forwards to nvim.)
flip_changes_roundtrip() {
  keys Tab; sleep 0.3               # editor -> files focus
  keys -l 2; sleep 0.8              # All files
  keys -l 1; sleep 0.8              # back to Changes
  keys Tab; sleep 0.3               # files -> editor focus
}
flip_allfiles_roundtrip() {
  keys Tab; sleep 0.3               # editor -> files focus
  keys -l 1; sleep 0.8              # Changes
  keys -l 2; sleep 0.8              # back to All files
  keys Tab; sleep 0.3               # files -> editor focus
}

# === Sequence 3 (team-lead #3): revert a hunk ABOVE the comment, then the comment's OWN hunk.
# tui-cardflip covers reverting the hunk BELOW; this covers above + containing.

# --- 3a. revert HTOP1 (a hunk ABOVE the anchored HMID1) -------------------------------------
keys -l '/HTOP1'; keys Enter; sleep 0.3
keys Space r h; sleep 1.0
wait_gone "HTOP1"
grep -q '^HTOP1$' "$REPO/src/one.txt" && fail "3a: HTOP1 was not reverted on disk"
grep -q '^HTOP0$' "$REPO/src/one.txt" || fail "3a: the base line HTOP0 did not come back"
sleep 0.4
frame | grep -qF "Send (1)" || fail "3a: reverting the hunk above silently dropped the comment"
frame | grep -qF "DNOTE_HMID" || fail "3a: reverting the hunk above dropped the card on HMID1"
flip_changes_roundtrip
wait_for "Send (1)"; wait_for "DNOTE_HMID"
echo "ok 3a - comment survives reverting a hunk ABOVE it"

# --- 3b. revert HMID1 (the hunk that CONTAINS the anchored line) ----------------------------
keys -l '/HMID1'; keys Enter; sleep 0.3
keys Space r h; sleep 1.0
wait_gone "HMID1"
grep -q '^HMID1$' "$REPO/src/one.txt" && fail "3b: HMID1 was not reverted on disk"
grep -q '^HMID0$' "$REPO/src/one.txt" || fail "3b: the base line HMID0 did not come back"
sleep 0.4
frame | grep -qF "Send (1)" || fail "3b: reverting the comment's OWN hunk silently dropped it"
flip_changes_roundtrip
wait_for "Send (1)"
echo "ok 3b - comment survives reverting the hunk that CONTAINS its anchored line"

# === Sequence 4 (team-lead #4): a poll refresh (concurrent agent write elsewhere) ----------
# The comment is shown in Changes; an agent appends far from the anchor; the host's poll sweep
# live-reloads the buffer. The store must not move.
printf 'AGENTPOLL4\n' >> "$REPO/src/one.txt"
wait_for "AGENTPOLL4"
frame | grep -qF "Send (1)" || fail "4: a poll refresh from an agent write dropped the comment"
flip_changes_roundtrip
wait_for "Send (1)"; wait_for "DNOTE_HMID"
echo "ok 4 - comment survives a poll refresh from a concurrent agent write"

# === Sequences 1 & 2 (team-lead #1, #2): All-files content edits with instant autosave. ----
# A content-anchored comment on the stable CANCHOR2 line, then insert lines above it and edit
# its own text. Done after the reverts so the earlier hunk fixture is undisturbed.
keys Tab; sleep 0.3                 # editor -> files focus
keys -l 2; sleep 0.8                # All files (cursor lands on the collapsed src/ dir)
keys Right; sleep 0.4               # expand src/
keys Down; sleep 0.5                # select one.txt -> diff_path set, plain/unlocked/editable
frame | grep -qF "src/one.txt" || fail "setup: one.txt not selected in All files"
keys Tab; sleep 0.3                 # focus editor
keys Escape; sleep 0.2
keys -l '/CANCHOR2'; keys Enter; sleep 0.3
keys Space r c; sleep 0.4
keys -l 'CNOTE_ANCHOR'; keys Enter; sleep 0.6
wait_for "CNOTE_ANCHOR"
wait_for "Send (2)"
echo "ok setup - content comment on CANCHOR2 in All files, Send (2)"

# --- 1. insert two lines ABOVE the comment, autosave ---------------------------------------
keys -l '/CANCHOR2'; keys Enter; sleep 0.3
keys O; sleep 0.3                   # open a line ABOVE CANCHOR2, enter insert
keys -l 'INSERTED_ABOVE_1'; keys Enter; keys -l 'INSERTED_ABOVE_2'
esc                                 # InsertLeave -> instant autosave
for _ in $(seq 20); do grep -q 'INSERTED_ABOVE_1' "$REPO/src/one.txt" && break; sleep 0.25; done
grep -q 'INSERTED_ABOVE_1' "$REPO/src/one.txt" || fail "1: the insert-above did not autosave"
frame | grep -qF "Send (2)" || fail "1: inserting lines above the comment dropped it"
flip_allfiles_roundtrip
wait_for "Send (2)"; wait_for "CNOTE_ANCHOR"
echo "ok 1 - comment survives inserting lines ABOVE it (autosave)"

# --- 2. edit the comment's OWN anchored line's text, autosave ------------------------------
keys -l '/CANCHOR2'; keys Enter; sleep 0.3
keys A; sleep 0.2                   # append at end of the anchored line
keys -l '_EDITED'; esc             # InsertLeave -> autosave
for _ in $(seq 20); do grep -q '^CANCHOR2_EDITED$' "$REPO/src/one.txt" && break; sleep 0.25; done
grep -q '^CANCHOR2_EDITED$' "$REPO/src/one.txt" || fail "2: the own-line edit did not autosave"
frame | grep -qF "Send (2)" || fail "2: editing the comment's own anchored line dropped it"
flip_allfiles_roundtrip
wait_for "Send (2)"; wait_for "CNOTE_ANCHOR"
echo "ok 2 - comment survives editing its OWN anchored line (autosave)"

# --- teardown: quit through the reviewer's own path (two unsent comments -> confirm) --------
keys Escape; sleep 0.2
keys Tab; sleep 0.3                 # editor -> files focus (q is a host key there)
keys q; sleep 0.3; keys y 2>/dev/null || true
wait_session_end
echo "# all anchor-survival assertions passed"
