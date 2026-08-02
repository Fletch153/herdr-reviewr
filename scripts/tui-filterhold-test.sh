#!/usr/bin/env bash
# Live gate: a filter box left open survives a mouse tab round-trip (a live user report: the
# title kept painting the query after a tab click, but typing `x` fired the expand binding
# instead of landing in the search).
set -uo pipefail
SOCK=rvfhold
source "$(dirname "$0")/tui-lib.sh"

mkdir -p "$REPO/src"
printf 'base\n' > "$REPO/src/evm.rs"
git -C "$REPO" add -A && git -C "$REPO" commit -qm base
printf 'EDIT\n' >> "$REPO/src/evm.rs"

tui_start
wait_for "evm.rs"

# 1) An open box (caret showing) survives the round-trip and keeps taking keys.
keys -l "/zzz"
wait_for "/zzz▏"
read -r C2 R2 <<< "$(locate '2 All files')"
click "$C2" "$R2"
wait_gone "/zzz▏"
read -r C1 R1 <<< "$(locate '1 Changes')"
click "$C1" "$R1"
wait_for "/zzz▏"
keys -l "x"
wait_for "/zzzx▏"
echo "ok 1 - the open box returns open and typing continues the query"

# 2) An Enter-confirmed box stays closed: the caret is gone and stays gone.
keys Enter
wait_gone "/zzzx▏"
click "$C2" "$R2"; sleep 0.5
click "$C1" "$R1"; sleep 0.5
frame | grep -qF "/zzzx▏" && fail "a confirmed box reopened"
frame | grep -qF "/zzzx" || fail "the confirmed query stopped shaping the list"
echo "ok 2 - a confirmed box stays closed with its query kept"

esc; sleep 0.3
keys q; sleep 0.3; keys y 2>/dev/null || true
echo "# the filter box survives tab round-trips exactly as shown"
