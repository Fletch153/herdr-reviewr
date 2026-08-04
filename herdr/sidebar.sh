#!/usr/bin/env bash
# Open / toggle the reviewr sidebar with the configured placement (default: right split, see
# specs/herdr-host.md#sidebar-placement). Invoked by herdr with the plugin
# runtime env set (HERDR_BIN_PATH, HERDR_PANE_ID, HERDR_WORKSPACE_ID, HERDR_PLUGIN_*,
# HERDR_PLUGIN_CONTEXT_JSON, and HERDR_PLUGIN_EVENT_JSON for events).
#
#   sidebar.sh toggle   key action: open the sidebar, or close it if already open
#   sidebar.sh open     event hook: open the sidebar if not already open (e.g. worktree.created)
#
# No `set -e`: a transient jq/herdr hiccup must not silently abort the toggle; each step is
# tolerant and results are checked explicitly.
set -uo pipefail

# herdr runs plugin commands with a minimal PATH; ensure jq/git resolve on common installs.
export PATH="/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:${PATH:-}"

mode="${1:-toggle}"
H="${HERDR_BIN_PATH:-herdr}"

ws="${HERDR_WORKSPACE_ID:-}"
pane="${HERDR_PANE_ID:-}"
cwd=""
if [ -n "${HERDR_PLUGIN_CONTEXT_JSON:-}" ]; then
  cwd=$(printf '%s' "$HERDR_PLUGIN_CONTEXT_JSON" | jq -r '.focused_pane_cwd // empty' 2>/dev/null)
  # A plugin pane's cwd is its own checkout under herdr's plugins dir — machinery, not a
  # review target. Its checkout IS a git repo, so the repo guard below cannot catch it: with
  # a plugin pane focused, the sidebar opened on that plugin's repo and showed "0 changed".
  plugins_dir=$(dirname "$(dirname "${HERDR_PLUGIN_ROOT:-/nonexistent}")")
  case "$cwd" in "$plugins_dir"/*) cwd="" ;; esac
  [ -n "$cwd" ] || cwd=$(printf '%s' "$HERDR_PLUGIN_CONTEXT_JSON" |
    jq -r '.worktree.checkout_path // .workspace_cwd // empty' 2>/dev/null)
fi

# An event fires without a focused pane; target the new worktree's workspace from the payload
# (worktree.created shape: .data.workspace.workspace_id, .data.workspace.worktree.checkout_path).
if [ -n "${HERDR_PLUGIN_EVENT_JSON:-}" ]; then
  ev="$HERDR_PLUGIN_EVENT_JSON"
  ws=$(printf '%s' "$ev" | jq -r '.data.workspace.workspace_id // .data.worktree.open_workspace_id // empty' 2>/dev/null)
  cwd=$(printf '%s' "$ev" | jq -r '.data.workspace.worktree.checkout_path // .data.worktree.path // empty' 2>/dev/null)
  pane=""
fi

# A workspace is required to key state and target the split; without it, do nothing rather
# than collide every workspace on a shared `pane-default` state file.
[ -n "$ws" ] || exit 0

statedir="${HERDR_PLUGIN_STATE_DIR:-${TMPDIR:-/tmp}}"
mkdir -p "$statedir" 2>/dev/null
state="$statedir/pane-$ws"

# Is a sidebar we opened still alive in this workspace?
existing=""
if [ -f "$state" ]; then
  prev=$(cat "$state" 2>/dev/null)
  if [ -n "$prev" ] && "$H" pane list --workspace "$ws" 2>/dev/null \
      | jq -e --arg p "$prev" '.result.panes[] | select(.pane_id == $p)' >/dev/null 2>&1; then
    existing="$prev"
  else
    rm -f "$state" 2>/dev/null # stale (closed via `q`)
  fi
fi

# Already open: toggle closes it; open is idempotent (don't stack a duplicate pane).
if [ -n "$existing" ]; then
  if [ "$mode" = "toggle" ]; then
    "$H" plugin pane close "$existing" >/dev/null 2>&1
    rm -f "$state" 2>/dev/null
  fi
  exit 0
fi

# Only open inside a git repo.
[ -n "$cwd" ] && git -C "$cwd" rev-parse --show-toplevel >/dev/null 2>&1 || exit 0

# Placement/direction come from reviewr's config (default: right split); an unknown value falls
# back to its default (specs/herdr-host.md#sidebar-placement). The file is re-read every run.
placement="split"
direction="right"
conf="${HERDR_PLUGIN_CONFIG_DIR:-}/config.toml"
if [ -n "${HERDR_PLUGIN_CONFIG_DIR:-}" ] && [ -f "$conf" ]; then
  p=$(sed -n "s/^[[:space:]]*toggle_placement[[:space:]]*=[[:space:]]*[\"']\([^\"']*\)[\"'].*/\1/p" "$conf" 2>/dev/null | tail -n1)
  d=$(sed -n "s/^[[:space:]]*toggle_direction[[:space:]]*=[[:space:]]*[\"']\([^\"']*\)[\"'].*/\1/p" "$conf" 2>/dev/null | tail -n1)
  case "$p" in split | overlay | zoomed | tab) placement="$p" ;; esac
  case "$d" in right | down) direction="$d" ;; esac
fi

# The worktree.created event (mode=open) auto-opens only the non-covering placements: a covering
# pane over a fresh worktree hides the agent, and overlay has no pane to attach to on an event.
if [ "$mode" = "open" ] && [ "$placement" != "split" ] && [ "$placement" != "tab" ]; then
  exit 0
fi

# Focus stays on the agent for an event or a split (ambient sidebar); a manual toggle into a
# covering or tab placement focuses reviewr so it can be driven.
focus=--no-focus
[ "$mode" = "toggle" ] && [ "$placement" != "split" ] && focus=--focus

# Placement decides the target: split/zoomed attach to a pane, tab to the workspace, overlay to
# the active pane (no selector). For a split/zoomed event with no focused pane, use the target
# workspace's first pane.
case "$placement" in
split | zoomed)
  if [ -z "$pane" ]; then
    pane=$("$H" pane list --workspace "$ws" 2>/dev/null | jq -r '.result.panes[0].pane_id // empty' 2>/dev/null)
  fi
  [ -n "$pane" ] || exit 0
  set -- --placement "$placement" --target-pane "$pane"
  [ "$placement" = "split" ] && set -- "$@" --direction "$direction"
  ;;
tab)
  set -- --placement tab --workspace "$ws"
  ;;
overlay)
  set -- --placement overlay
  ;;
*)
  exit 0 # unreachable: placement is validated above; guard against a future value leaking $@
  ;;
esac

new=$("$H" plugin pane open --plugin "${HERDR_PLUGIN_ID:-persiyanov.reviewr}" --entrypoint sidebar \
  "$@" --cwd "$cwd" "$focus" 2>/dev/null \
  | jq -r '.result.plugin_pane.pane.pane_id // empty' 2>/dev/null)
[ -n "$new" ] && printf '%s' "$new" > "$state"
