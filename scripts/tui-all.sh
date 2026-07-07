#!/usr/bin/env bash
# Run every live tmux gate in sequence; the regression sweep for the embedded-nvim mode.
set -uo pipefail
DIR="$(cd "$(dirname "$0")" && pwd)"
rc=0
for gate in tui-test.sh tui-edit-test.sh tui-scope-test.sh tui-mouse-test.sh \
            tui-death-test.sh tui-rename-test.sh tui-split-test.sh tui-picker-test.sh tui-unicode-test.sh tui-trio-test.sh \
            tui-wrap-test.sh tui-undo-test.sh tui-live-test.sh tui-lock-test.sh tui-eol-test.sh \
            tui-nav-test.sh tui-persist-test.sh tui-md-test.sh tui-revert-test.sh \
            tui-clip-test.sh tui-storm-test.sh tui-empty-test.sh tui-stash-test.sh \
            tui-cbracket-test.sh tui-vishl-test.sh tui-jump-test.sh tui-livejump-test.sh; do
  echo "== $gate"
  if ! bash "$DIR/$gate"; then
    echo "== $gate FAILED"
    rc=1
  fi
done
[ "$rc" -eq 0 ] && echo "== all gates passed"
exit $rc
