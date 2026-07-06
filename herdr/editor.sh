#!/usr/bin/env bash
# reviewr `editor` pane entrypoint: run nvim as the review editor. nvim listens on a per-workspace
# socket so the reviewr sidebar can open files in it (nvim --server <sock> --remote-send). The
# bundled reviewr.nvim is put on the runtimepath for comment/send/diff. Launched by
# `herdr plugin pane open --entrypoint editor --cwd <repo>`, so PWD is the repo under review.
set -uo pipefail

# herdr runs plugin commands with a minimal PATH; make nvim resolve on common installs.
export PATH="/opt/homebrew/bin:/usr/local/bin:/home/linuxbrew/.linuxbrew/bin:/usr/bin:/bin:${PATH:-}"

statedir="${HERDR_PLUGIN_STATE_DIR:-${TMPDIR:-/tmp}}"
mkdir -p "$statedir" 2>/dev/null
sock="$statedir/nvim-${HERDR_WORKSPACE_ID:-default}.sock"
rm -f "$sock" 2>/dev/null # a stale socket from a prior editor pane would block --listen

rtp="${HERDR_PLUGIN_ROOT:-}/nvim"
exec nvim --listen "$sock" --cmd "set runtimepath^=$rtp"
