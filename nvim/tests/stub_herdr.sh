#!/usr/bin/env bash
# A fake `herdr` for the reviewr.nvim headless tests. It mimics the real CLI's key quirk — every
# call exits 0, reporting failure only via a JSON `{"error":...}` envelope on stdout — and records
# `agent send` invocations to $REVIEWR_STUB_LOG so a test can assert what was delivered.
#
# The listed tab holds two Claude agents, so resolution is ambiguous *unless* the focused-pane
# preference kicks in — that's what the test checks. Set REVIEWR_STUB_MODE=error to make `send`
# return an error envelope.
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
  "agent send")
    pane="${3:-}"
    shift 3 || true
    printf 'send %s %s\n' "$pane" "$*" >>"${REVIEWR_STUB_LOG:?}"
    if [ "${REVIEWR_STUB_MODE:-}" = "error" ]; then
      echo '{"error":{"code":"agent_not_found","message":"agent target not found"},"id":"cli:agent:send"}'
    else
      echo '{"id":"cli:agent:send","result":{"ok":true},"type":"agent_send"}'
    fi
    ;;
  "agent focus")
    echo '{"id":"cli:agent:focus","result":{"ok":true}}'
    ;;
  *)
    echo "{\"error\":{\"message\":\"stub: unhandled ${*}\"}}"
    ;;
esac
exit 0
