#!/usr/bin/env bash
# Live gate (from probe c2.p6, matrix 13×timing): per-tab stash swap mid-action.
# Contracts:
#  (a) `/`-filter typed in Changes is LEFT-PANE STATE → per-tab (set_tab doc: "each tab
#      keeps its own ... left-pane state"). Switching 2 shows All files UNfiltered
#      (zz.txt visible, no "/fa" in the pane title); switching back 1 restores the
#      filter text and the filtered list exactly.
#  (d) ctrl+i (kitty CSI-u) mid-filter-typing is IGNORED (Mode::Filter block has no
#      ctrl arm) — no half tab-switch, filter box intact.
#  (b) md_view is a STICKY GLOBAL PREFERENCE (app.rs md_view doc): 2 opens a non-md
#      file raw (no [md view] chip); back on 1 the md file re-renders with the chip.
#  (c) tab switch mid-compose: composing captures every char, so `2` lands IN the
#      draft and set_tab's composing() guard makes a switch impossible — the draft is
#      never silently lost.
set -uo pipefail
SOCK=rvstash
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

printf '# BigHeading\n\nplain md body\n' > "$REPO/ANOTES.md"
{ for i in $(seq 1 8); do printf 'fa line %02d\n' "$i"; done; } > "$REPO/fa.txt"
{ for i in $(seq 1 8); do printf 'zz line %02d\n' "$i"; done; } > "$REPO/zz.txt"
{ for i in $(seq 1 8); do printf 'only line %02d\n' "$i"; done; } > "$REPO/onlyall.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm base
printf '\nmdbodyaddition\n' >> "$REPO/ANOTES.md"
printf 'FACHANGE\n' >> "$REPO/fa.txt"
printf 'ZZCHANGE\n' >> "$REPO/zz.txt"
# onlyall.txt stays committed-clean: a row ONLY the All files tab lists — the tab telltale.

tui_start
wait_for "ANOTES.md"
wait_for "BigHeading"

# (a) filter in Changes, round trip. Filter to fa: zz.txt row leaves the list.
keys -l "/"
sleep 0.3
keys -l "fa"
wait_for "/fa"
frame | grep -qF "zz.txt" && fail "the Changes filter did not hide zz.txt"
keys Enter # confirm filter (Normal mode, query kept)
sleep 0.3
keys 2
wait_for "onlyall.txt" # All files' own (unfiltered) list must show the clean file
frame | grep -qF "/fa" && fail "the Changes filter text leaked into All files' filter box"
frame | grep -qF "zz.txt" || fail "the Changes filter hid zz.txt in All files' list"
keys 1
wait_for "/fa" # filter text restored...
frame | grep -qF "zz.txt" && fail "the restored Changes filter lost its filtered list"
echo "ok a - the / filter is per-tab: no leakage out, restored exactly on return"

# (d) ctrl+i mid-filter-typing in All files: ignored, no half-switch.
keys 2
wait_for "onlyall.txt"
keys -l "/"
sleep 0.3
keys -l "only"
wait_for "/only"
keys -l "$(printf '\033[105;5u')" # ctrl+i, kitty CSI-u
sleep 0.8
frame | grep -qF "/only" || fail "ctrl+i mid-filter clobbered the filter box"
frame | grep -qF "onlyall.txt" || fail "ctrl+i mid-filter half-switched away from All files"
keys Escape # clear All files' filter
sleep 0.3
echo "ok d - ctrl+i mid-filter is ignored"

# (b) md_view round trip. Back in Changes, clear its filter, open the md file, render.
keys 1
wait_for "/fa"
keys -l "/"
sleep 0.3
keys Escape
sleep 0.3
frame | grep -qF "zz.txt" || fail "clearing the Changes filter did not restore the list"
keys Up # fa.txt -> ANOTES.md
wait_for "mdbodyaddition"
keys p
sleep 0.8
frame | grep -qF "[md view]" || fail "p did not flip the chip to [md view]"
frame | grep -qF "# BigHeading" && fail "the rendered view still shows raw markdown"
keys 2
wait_for "only line 03" # All files' own selection (non-md) opens raw
frame | grep -qF "[md view]" && fail "the [md view] chip showed on a non-md file"
keys 1
wait_for "BigHeading"
sleep 0.5
frame | grep -qF "[md view]" || fail "returning to Changes lost the sticky md view"
frame | grep -qF "# BigHeading" && fail "returning to Changes lost the RENDERED view"
echo "ok b - md_view sticky across the tab round trip, chip honest on non-md"

# (c) composer mid-switch: `2` goes into the draft; the tab cannot change.
keys Down # ANOTES.md -> fa.txt
wait_for "FACHANGE"
keys Tab # focus the editor
sleep 0.4
keys Space r c
sleep 1
keys -l "DRAFTMARK"
sleep 0.5
keys 2
sleep 0.8
frame | grep -qF "DRAFTMARK2" || fail "the 2 mid-compose did not land in the draft"
frame | grep -qF "onlyall.txt" && fail "the tab switched away mid-compose"
keys Escape # dismiss the composer, draft discarded deliberately by the user
sleep 0.5
frame | grep -qF "DRAFTMARK" && fail "the composer did not dismiss"
echo "ok c - mid-compose 2 lands in the draft, no tab switch, no silent loss"

keys Tab # back to the files pane: q must be the reviewer quit, not a macro record
sleep 0.4
keys q
sleep 0.3
keys y 2>/dev/null || true
wait_session_end
echo "# per-tab stash mid-action probe complete"
