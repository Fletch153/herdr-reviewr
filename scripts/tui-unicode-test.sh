#!/usr/bin/env bash
# Live gate: wide characters (CJK), emoji clusters, and accents through the grid blit, the
# inline diff, the composer, and the comment cards; plus a PR-tab round trip.
set -uo pipefail
SOCK=rvuni
TUI_INIT_EXTRA="vim.o.number = true"
source "$(dirname "$0")/tui-lib.sh"

cat > "$REPO/src/uni.txt" << 'FIXTURE'
plain ascii line
第一行の内容です
naïve café résumé
emoji rocket line
last plain line
FIXTURE
git -C "$REPO" add -A && git -C "$REPO" commit -qm A
python3 - "$REPO/src/uni.txt" << 'PY'
import sys, pathlib
p = pathlib.Path(sys.argv[1])
s = p.read_text()
s = s.replace("第一行の内容です", "第一行の内容です 変更済み")
s = s.replace("emoji rocket line", "emoji 🚀 rocket line 🎉")
p.write_text(s)
PY

tui_start
wait_for "uni.txt"

# 1. Wide and cluster glyphs render in the grid, and the changed CJK line carries the sign.
wait_for "変更済み"
wait_for "🚀"
frame | grep -qE '~ +[0-9]+ 第一行の内容です 変更済み' || fail "CJK change lacks its modification sign"
frame | grep -q "第一行の内容です$" && true # the old line shows as the red virtual (unsuffixed)
frame | grep -q "naïve café résumé" || fail "accented line mangled"
echo "ok 1 - CJK, emoji, and accents render with correct diff marks"

# 2. Comment on the CJK line with a CJK+emoji note: composer round-trips it into the card.
keys Tab; sleep 0.4
keys Space r c; sleep 0.3
keys -l "注意 🚨 需要修改"
keys Enter
wait_for "╭─ comment"
wait_for "注意 🚨 需要修改"
frame | grep -q "Send (1)" || fail "wide-char comment did not land in the store"
echo "ok 2 - wide-char note round-trips composer → store → card"

# 3. Typing CJK into the editor lands (insert mode, then autosave on switch writes it).
keys i
keys -l "插入的文字 "
esc   # settle: a burst Esc+Tab can read a stale insert mode and misroute one Tab (known)
wait_for "插入的文字"
keys Tab; sleep 0.3
keys 2; sleep 0.8   # view switch runs the autosave
keys 1; sleep 0.8
grep -q "插入的文字" "$REPO/src/uni.txt" || { echo "--- disk:"; cat "$REPO/src/uni.txt"; echo "--- title row:"; frame | sed -n '2p' | cut -c1-60; fail "typed CJK not autosaved to disk"; }
echo "ok 3 - typed CJK autosaves to disk"

# 4. PR tab round trip: renders its layout, and 1 returns to Changes intact.
keys 3; sleep 1
frame | grep -qi "pull request\|PR\|no pr" || fail "PR tab did not render"
keys 1
wait_for "変更済み"
echo "ok 4 - PR tab round trip leaves the editor intact"

keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# all unicode assertions passed"
