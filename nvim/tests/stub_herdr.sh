#!/usr/bin/env bash
# A fake `herdr` for the reviewr.nvim headless tests, mimicking the herdr ≥0.7.5 CLI: text lands
# via `pane send-text <pane> <text>` (EMPTY stdout on success), and failures print a JSON
# `{"error":...}` envelope on stdout AND exit non-zero. (Pre-0.7.5 hosts had `agent send` and
# reported failures with a zero exit; 0.7.5 removed that subcommand — exactly the breakage the
# send path must survive.) Deliveries are recorded to $REVIEWR_STUB_LOG so a test can assert
# what was sent.
#
# The listed tab holds two Claude agents, so resolution is ambiguous *unless* the focused-pane
# preference kicks in — that's what the test checks. Set REVIEWR_STUB_MODE=error to make
# `pane send-text` return an error envelope.
set -euo pipefail

case "${1:-} ${2:-}" in
  "agent list")
    cat <<'JSON'
{"id":"cli:agent:list","result":{"agents":[
  {"agent":"claude","agent_status":"idle","pane_id":"wY:pFOCUS","tab_id":"wY:t1","workspace_id":"wY"},
  {"agent":"claude","agent_status":"working","pane_id":"wY:pOTHER","tab_id":"wY:t1","workspace_id":"wY"},
  {"agent_status":"unknown","pane_id":"wY:pSIDEBAR","tab_id":"wY:t1","workspace_id":"wY"}
]},"type":"agent_list"}
JSON
    ;;
  "pane send-text")
    pane="${3:-}"
    shift 3 || true
    printf 'send %s %s\n' "$pane" "$*" >>"${REVIEWR_STUB_LOG:?}"
    if [ "${REVIEWR_STUB_MODE:-}" = "error" ]; then
      echo '{"error":{"code":"pane_not_found","message":"pane target not found"},"id":"cli:request"}'
      exit 1
    fi
    # Success is EMPTY stdout on the real ≥0.7.5 CLI — deliberately print nothing.
    ;;
  "agent focus")
    echo '{"id":"cli:agent:focus","result":{"ok":true}}'
    ;;
  *)
    # Unknown subcommand — including the REMOVED `agent send`: the real CLI prints usage and
    # fails. An envelope + non-zero exit models it closely enough for the tests.
    echo "{\"error\":{\"message\":\"stub: unhandled ${*}\"}}"
    exit 1
    ;;
esac
exit 0
