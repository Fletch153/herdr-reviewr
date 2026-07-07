#!/usr/bin/env bash
# Live gate for the wrap-gap workaround: nvim can't paint 'breakindent' whitespace on wrapped
# rows (neovim/neovim#26392), so a wrapped green line would carry an unhighlighted indent gap.
# The focused view drops breakindent (painted lines wrap edge-to-edge); the plain view hands
# the user's indent back. Config mirrors the reported setup: breakindent + number + listchars.
set -uo pipefail
SOCK=rvwrap
TUI_INIT_EXTRA="vim.o.number = true
vim.o.breakindent = true
vim.o.list = true
vim.opt.listchars = { tab = '> ' }"
source "$(dirname "$0")/tui-lib.sh"

printf 'short base\n' > "$REPO/src/w.txt"
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
printf '\tADDED %s tail-end\n' "$(python3 -c "print('x'*180)")" >> "$REPO/src/w.txt"

tui_start
wait_for "w.txt"
wait_for "ADDED"
sleep 0.5

# 1. Changes: the wrapped row continues right after the 6-cell gutter (sign+number) — no
#    unpainted breakindent gap between the gutter and the text.
frame | grep -qE '^│ {6}x' || fail "continuation row still carries a breakindent gap"
frame | grep -E '^│ {6} +x' | grep -q tail-end && fail "continuation row is indented in the focused view"
echo "ok 1 - focused view wraps the added line edge-to-edge"

# 2. ...and that row is actually painted: the DiffAdd background is active on the row (it
#    now starts at the gutter, so the code appears at the row start, not at the text).
framee | grep -E 'tail-end' | grep -q '48;2;0;85;35' \
  || fail "continuation row is not painted DiffAdd green"
echo "ok 2 - the continuation row carries the diff background"

# 3. All files: the user's breakindent comes back (number col 4 + tab indent 8 = 12 cells).
keys 2; sleep 0.8
frame | grep -qE '^│ {12}x' || fail "plain view did not restore the user's breakindent"
echo "ok 3 - plain view restores the user's breakindent"

# 4. Back to Changes: the drop reapplies through the view-sync path, not just first open.
keys 1; sleep 0.8
frame | grep -qE '^│ {6}x' || fail "refocusing did not drop breakindent again"
echo "ok 4 - refocusing drops breakindent again"

# 5. The gutter itself is painted on hot rows: the first row's sign+number cells and the
#    wrapped row's full lead-in all carry the DiffAdd background (no dark strip at the left).
framee > "$TMP/esc-frame.txt"
python3 - "$TMP/esc-frame.txt" <<'PY' || fail "gutter cells unpainted on a hot row"
import re, sys
rows = open(sys.argv[1]).read().split('\n')
GREEN = '0;85;35'
def cells(row):
    bg = None; out = []
    for tok in re.split(r'(\x1b\[[0-9;]*m)', row):
        if tok.startswith('\x1b['):
            codes = tok[2:-1].split(';') if len(tok) > 3 else ['0']
            i = 0
            while i < len(codes):
                c = codes[i]
                if c == '48' and i + 4 < len(codes) and codes[i+1] == '2':
                    bg = ';'.join(codes[i+2:i+5]); i += 5; continue
                if c in ('49', '0', ''):
                    bg = None
                i += 1
        else:
            out.extend((ch, bg) for ch in tok)
    return out
def gutter_ok(pat):
    for r in rows:
        if pat in r:
            cs = cells(r)
            start = next(i for i, (ch, _) in enumerate(cs) if ch == '│') + 1
            seg = cs[start:start + 8]  # gutter (6) + first text cells
            dark = [(i, ch, bg) for i, (ch, bg) in enumerate(seg) if bg != GREEN]
            if dark:
                print(f"{pat}: dark gutter cells {dark}")
                return False
            return True
    print(f"{pat}: row not found")
    return False
ok = gutter_ok('tail-end') and gutter_ok('ADDED')
sys.exit(0 if ok else 1)
PY
echo "ok 5 - the gutter is painted across sign and number cells on hot rows"

keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all wrap-gap assertions passed"
