#!/usr/bin/env bash
# PROBE: markdown-view scroll routing (Diff-focus j/PageDown scrolls the render) and
# cross-tab preview-scroll isolation (a tall md scrolled in Changes must NOT bleed its
# scroll onto a DIFFERENT md file selected in All files).
set -uo pipefail
SOCK=rvmdscroll
source "$(dirname "$0")/tui-lib.sh"

# a.md: very tall (200 bullets) — ATOP at top, ABOT near the bottom.
{ echo "- ATOPMARKER"; for i in $(seq 2 199); do printf -- '- a%03d\n' "$i"; done; echo "- ABOTMARKER"; } > "$REPO/a.md"
# b.md: moderately tall (80 bullets) — taller than the viewport so it HAS a max scroll,
# but far shorter than a.md's scroll offset — BTOP at top.
{ echo "- BTOPMARKER"; for i in $(seq 2 79); do printf -- '- b%03d\n' "$i"; done; echo "- BBOTMARKER"; } > "$REPO/b.md"
git -C "$REPO" add -A && git -C "$REPO" commit -qm base
# make both CHANGED so both list in Changes
printf -- '- a-extra\n' >> "$REPO/a.md"
printf -- '- b-extra\n' >> "$REPO/b.md"

tui_start
wait_for "a.md"
wait_for "b.md"

# Phase A: give All files its OWN selection (b.md), Changes keeps a.md. Raw, md off.
keys 2; sleep 0.8          # -> All files
wait_for "a.md"
keys j; sleep 0.8          # cursor a.md -> b.md in All files
keys 1; sleep 0.8          # -> Changes (a.md selected)
echo "ok A - tabs seeded: Changes=a.md, All files=b.md"

# Phase B: render a.md and scroll it (scroll routing).
keys p; sleep 0.8
frame | grep -q "ATOPMARKER" || fail "a.md did not render (ATOPMARKER missing)"
frame | grep -q "\[md view\]" || fail "chip did not flip to [md view]"
echo "ok B1 - p renders a.md at the top"
keys Tab; sleep 0.4        # focus the diff/preview pane
keys PageDown; keys PageDown; keys PageDown; keys PageDown; sleep 0.8
frame | grep -q "ATOPMARKER" && fail "scroll routing dead: ATOPMARKER still shown after PageDown"
frame | grep -q "a090" || fail "scroll routing dead: PageDown did not advance into a.md's body"
echo "ok B2 - PageDown scrolls the rendered markdown (ATOP gone, deep body shown)"

# Phase C: switch to All files -> b.md (sticky md_view still on). It must show at ITS top.
keys 2; sleep 1.0
frame | grep -q "BTOPMARKER" || fail "cross-tab scroll bleed: b.md not at its top (BTOPMARKER hidden)"
frame | grep -q "ABOTMARKER" && fail "a.md content leaked onto the b.md render"
echo "ok C - All files' b.md renders at its own top (no preview-scroll bleed)"

esc                        # close the preview (md-diff focus swallows q)
keys Tab; sleep 0.3        # hand focus to the files pane so q quits the reviewer
keys q; sleep 0.3; keys y 2>/dev/null || true
wait_session_end
echo "# all mdscroll assertions passed"
