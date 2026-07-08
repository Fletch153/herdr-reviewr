#!/usr/bin/env bash
# Live gate: a diff-anchored comment card renders ONLY under the Changes (Diff) view it was
# authored against, and never leaks onto another tab's buffer. Regression lock for the
# tab-switch card leak: switching to a never-visited All files tab (empty stash -> no selected
# file) took nvim_sync's "no selection" early-return, which re-presented the leftover buffer
# plain via sync_view but skipped comments.apply — so the Changes comment card stayed painted
# on the All-files buffer while the store was unchanged (Send count intact). apply() is the
# only writer of ns=reviewr_comments, so the early return must clear it.
#
# Gated on the STORE (Send N) as the authoritative signal that the comment survives every
# repaint — card pixels are asserted only after each transition settles.
set -uo pipefail
SOCK=rvcardflip
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

printf 'L1\nL2\nL3\nL4\nL5\nL6\nL7\nL8\nL9\nL10\n' > "$REPO/src/one.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm base
printf 'L1\nHUNKA\nL3\nL4\nL5\nL6\nL7\nL8\nHUNKB\nL10\n' > "$REPO/src/one.txt"

tui_start
wait_for "one.txt"; wait_for "HUNKA"; wait_for "HUNKB"

# setup: comment on HUNKA (focus() lands the cursor on the first change).
keys Tab; sleep 0.4          # focus editor (Changes, focused diff)
keys Escape; sleep 0.2
keys Space r c; sleep 0.4
keys -l 'ANOTE_HUNKA'; keys Enter; sleep 0.6
wait_for "╭─ comment"
frame | grep -qF "ANOTE_HUNKA" || fail "setup: comment card did not paint on HUNKA"
wait_for "Send (1)"
echo "ok setup - comment on HUNKA, Send (1)"

# --- 1. tab-switch to a never-visited All files (empty stash): the card must NOT leak -------
keys Tab; sleep 0.4          # editor -> files pane focus (so 1/2/3 switch tabs)
keys -l 2; sleep 1.0         # All files tab, no file selected yet
frame | grep -qF "Send (1)" || fail "1: store lost the comment on the tab switch"
frame | grep -qF "ANOTE_HUNKA" \
  && fail "1: BUG - the Changes comment card leaked onto the empty All-files buffer"
echo "ok 1 - no comment-card leak on the empty All-files tab"

# --- 2. back to Changes: the card must return (clearing it must not lose it) ----------------
keys -l 1; sleep 1.0         # Changes tab
wait_for "Send (1)"
wait_for "ANOTE_HUNKA"
echo "ok 2 - comment card returns to Changes after the round trip"

# --- 3. insert-flip (same file selected) hides the diff-anchored card; ctrl+i restores it ---
keys Tab; sleep 0.3          # files -> editor focus
keys Escape; sleep 0.2
keys i; sleep 0.9            # insert-flip -> All files, same file, plain view, insert mode
keys Escape; sleep 0.6
frame | grep -qF "ANOTE_HUNKA" && fail "3: diff-anchored card leaked into the flipped All-files view"
frame | grep -qF "Send (1)" || fail "3: store lost the comment on the insert-flip"
keys -l "$(printf '\033[105;5u')"; sleep 0.9   # ctrl+i: back to Changes
wait_for "ANOTE_HUNKA"
echo "ok 3 - card hides on the insert-flip and returns on ctrl+i"

# --- 4. revert the OTHER hunk (HUNKB, below): the comment on HUNKA survives -----------------
keys Escape; keys -l '/HUNKB'; keys Enter; sleep 0.3
keys Space r h; sleep 1.0
wait_gone "HUNKB"
grep -q '^HUNKB$' "$REPO/src/one.txt" && fail "4: HUNKB was not reverted on disk"
sleep 0.6
frame | grep -qF "ANOTE_HUNKA" \
  || fail "4: reverting the hunk below dropped the comment card on HUNKA"
frame | grep -qF "Send (1)" || fail "4: store lost the comment on the revert"
echo "ok 4 - comment survives reverting the hunk below it"

keys Escape; sleep 0.2
keys Tab; sleep 0.3
keys q; sleep 0.3; keys y 2>/dev/null || true
wait_session_end
echo "# all card-flip assertions passed"
