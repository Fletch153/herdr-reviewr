#!/usr/bin/env bash
# Run every live tmux gate in sequence; the regression sweep for the embedded-nvim mode.
set -uo pipefail
DIR="$(cd "$(dirname "$0")" && pwd)"
rc=0
for gate in tui-test.sh tui-edit-test.sh tui-scope-test.sh tui-mouse-test.sh \
            tui-death-test.sh tui-rename-test.sh tui-split-test.sh; do
  echo "== $gate"
  if ! bash "$DIR/$gate"; then
    echo "== $gate FAILED"
    rc=1
  fi
done
[ "$rc" -eq 0 ] && echo "== all gates passed"
exit $rc
