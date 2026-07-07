#!/usr/bin/env bash
# Headless run of the nvim-side test suite (nvim/tests/run.lua).
# run.lua exits via :cquit 1 on any failure, so set -e propagates it.
set -euo pipefail
cd "$(cd "$(dirname "$0")/.." && pwd)"
REVIEWR_DIR="$PWD/nvim" HERDR_BIN_PATH="$PWD/nvim/tests/stub_herdr.sh" \
REVIEWR_STUB_LOG=$(mktemp) HERDR_PANE_ID=wY:pSIDEBAR HERDR_TAB_ID=wY:t1 \
HERDR_WORKSPACE_ID=wY HERDR_PLUGIN_CONTEXT_JSON='{"focused_pane_id":"wY:pFOCUS"}' \
nvim --headless -u NONE -l nvim/tests/run.lua
