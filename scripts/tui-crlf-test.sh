#!/usr/bin/env bash
# Live gate for CRLF (dos) files. nvim stores a dos file's lines with the trailing \r stripped
# (it is the fileformat marker, not content), but the base blob is split on \n and keeps every
# line's \r. Unmatched, that \r makes every line differ so the whole file ghost-diffs in the
# editor gutter — even though the built-in pane, which keeps \r on both sides, shows only the
# real edit. (1) The Changes view must diff a CRLF file by content: unchanged lines fold, only
# the edited line is marked. (2) Autosaving a CRLF file in All files must keep its \r\n endings.
set -uo pipefail
SOCK=rvcrlf
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

# A 30-line pure-CRLF file; one line edited with \r\n preserved (byte-exact, no EOL drift).
python3 - "$REPO/src/crlf.txt" <<'PY'
import sys
open(sys.argv[1], 'wb').write(b''.join(('crlf line %02d\r\n' % i).encode() for i in range(1, 31)))
PY
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
python3 - "$REPO/src/crlf.txt" <<'PY'
import sys
p = sys.argv[1]
d = open(p, 'rb').read()  # read before the 'wb' open truncates the file
d = d.replace(b'crlf line 15\r\n', b'CRLF EDITED 15\r\n')
open(p, 'wb').write(d)
PY

tui_start
wait_for "crlf.txt"
wait_for "CRLF EDITED 15"

# 1. The Changes view diffs by content, not by line ending: the unchanged CRLF lines fold away
#    and only the edited line carries a modification sign. Pre-fix every line shows '~' and no
#    fold ever appears, so this wait is the teeth.
wait_for "unchanged lines"
frame | grep -qE '~ +15 CRLF EDITED 15' || fail "the real CRLF edit lacks its modification sign"
frame | grep -qE '~ +[0-9]+ crlf line 0[1-9]' && fail "an unchanged CRLF line is painted as a change (ghost diff)"
frame | grep -q '\^M' && fail "a carriage return leaked into the diff as ^M"
echo "ok 1 - a CRLF file diffs by content, not by its line endings"

# 2. Autosave preserves CRLF: edit the open file in All files, let the autosave fire, and the
#    file stays pure \r\n on disk (nvim's dos fileformat), never silently rewritten to LF.
keys 2; sleep 0.8            # Changes (files focus) -> All files tab (digit needs files focus)
keys Tab; sleep 0.3          # files -> editor (crlf.txt stays open, now plain/editable)
keys G; sleep 0.3            # last line, clear of the diff folds
keys A                       # append at end of line
keys -l " MARK"
esc                          # InsertLeave fires the autosave
# Event-wait on disk for the autosave to land, then assert the endings survived.
for _ in $(seq 40); do grep -q "MARK" "$REPO/src/crlf.txt" && break; sleep 0.25; done
grep -q "MARK" "$REPO/src/crlf.txt" || fail "the All-files edit never autosaved to disk"
python3 - "$REPO/src/crlf.txt" <<'PY' || fail "autosave corrupted the CRLF line endings"
import sys
d = open(sys.argv[1], 'rb').read()
# Removing every \r\n pair must leave no bare \n behind, and some \r\n must remain.
assert b'\n' not in d.replace(b'\r\n', b''), "a bare LF remains after autosave"
assert b'\r\n' in d, "no CRLF left after autosave"
PY
echo "ok 2 - autosave preserves the file's CRLF line endings"

keys Tab; sleep 0.3          # editor -> files, so q quits instead of recording a macro
keys q; sleep 0.3; keys y 2>/dev/null || true
wait_session_end
echo "# all CRLF assertions passed"
