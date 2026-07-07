#!/usr/bin/env bash
# Live gate for live view sync: agent writes to an OPEN file appear without any interaction
# (host poll sweeps checktime; the live FileChangedShell policy reloads clean buffers and
# repaints marks), and user edits hit disk the moment they exist (InsertLeave/TextChanged
# instant autosave) — no view switch needed in either direction. Also pins the poll's blind
# spot coverage: an mtime-preserving write (cp -p flavor) reloads via live.poll's size check
# (nvim's own timestamp compare never sees it), and quitting mid-insert flushes the pending
# write to disk before the pane closes.
set -uo pipefail
SOCK=rvlive
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

printf 'line a\nline b\nline c\nline d\nline e\nline f\nline g\nline h\n' > "$REPO/src/one.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
printf 'TAIL CHANGE\n' >> "$REPO/src/one.txt"

tui_start
wait_for "one.txt"
wait_for "TAIL CHANGE"

# 1. The agent appends while the file is open and NOTHING is pressed: the new line must
#    appear on its own, painted as a change.
printf 'AGENT LIVE\n' >> "$REPO/src/one.txt"
wait_for "AGENT LIVE"
frame | grep -qE '\+ *[0-9]+ AGENT LIVE' || fail "the live-reloaded line is not painted as an addition"
echo "ok 1 - an agent write to the open file appears and repaints with no interaction"

# 2. User edits save instantly: leaving insert puts the text on disk with no switch.
keys 2; sleep 0.8
keys Tab; sleep 0.4
keys i; keys -l "USERLIVE "; esc
wait_for "USERLIVE"
for _ in $(seq 20); do grep -q "USERLIVE" "$REPO/src/one.txt" 2>/dev/null && break; sleep 0.25; done
grep -q "USERLIVE" "$REPO/src/one.txt" || fail "the insert-mode edit did not autosave instantly"
echo "ok 2 - leaving insert saves to disk immediately"

# 3. Normal-mode changes save instantly too (dd, no switch).
keys d d
for _ in $(seq 20); do grep -q "USERLIVE" "$REPO/src/one.txt" 2>/dev/null || break; sleep 0.25; done
grep -q "USERLIVE" "$REPO/src/one.txt" && fail "the normal-mode dd did not autosave instantly"
echo "ok 3 - a normal-mode change saves to disk immediately"

# Same-size in-place rewrite through the SAME inode (sed -i swaps inodes and can reset the
# mode, which nvim's timestamp check DOES compare — the steps below pin mtimes explicitly).
rewrite() {
  local content
  content="$(sed "$1" "$REPO/src/one.txt")"
  printf '%s\n' "$content" > "$REPO/src/one.txt"
}

# 4. nvim's checktime compares mtime seconds+nanoseconds, never size. First pin a write to
#    the same SECOND with drifted nanoseconds: still reloads (the same-wall-clock-second
#    agent write is safe). Then a size-changing write with the mtime pinned EXACTLY (the
#    cp -p / rsync -t restore flavor, invisible to checktime forever): live.poll's size
#    check must reload and repaint it.
painted() { frame | grep -qE '\+ *[0-9]+ '"$1"; }
keys -l "$(printf '\033[105;5u')"   # ctrl+i (kitty CSI u): back to the Changes review
for _ in $(seq 60); do painted "AGENT LIVE" && break; sleep 0.25; done
painted "AGENT LIVE" || fail "ctrl+i did not return to the painted Changes view"
rewrite 's/^line d$/MARKT1/'        # same byte count as "line d"
SEC=$(stat -c %Y "$REPO/src/one.txt")
touch -d "@$SEC" "$REPO/src/one.txt"
wait_for "MARKT1"
echo "ok 4a - a same-second write with nanosecond drift still reloads"
rewrite 's/^MARKT1$/MARKSZPLUS/'    # +4 bytes
touch -d "@$SEC" "$REPO/src/one.txt"
wait_for "MARKSZPLUS"
# A replaced line paints as a modification (~), with the base line as a virt_line above it.
frame | grep -qE '~ *[0-9]+ MARKSZPLUS' || fail "the size-check reload did not repaint"
echo "ok 4b - an mtime-preserving write with a size change reloads via the size check"

# 5. Quit during a pending write: type in insert mode and quit WITHOUT leaving insert (no
#    InsertLeave autosave ran) — the quit path's forced wall! must land the text on disk
#    before the pane closes.
keys i                               # authoring flip: All files, unlocked, insert entered
for _ in $(seq 60); do painted "AGENT LIVE" || break; sleep 0.25; done   # plain view clears paint
painted "AGENT LIVE" && fail "the insert flip never reached the plain view"
sleep 0.6
keys -l "QUITRACE"
read -r C R < <(locate_right "one.txt")
[ -n "${C:-}" ] || fail "could not locate one.txt in the file list"
click "$C" "$R"; sleep 0.5           # focus the files pane; nvim stays in insert, modified
keys q; sleep 0.4; keys y 2>/dev/null || true
for _ in $(seq 40); do $TMUX has-session 2>/dev/null || break; sleep 0.25; done
grep -q "QUITRACE" "$REPO/src/one.txt" || { echo "FAIL - quit lost the pending insert-mode write"; exit 1; }
echo "ok 5 - quit flushes a pending insert-mode write to disk"
echo "# all live-sync assertions passed"
