---
Status: Current
Created: 2026-06-23
Last edited: 2026-07-02
---

# herdr host

How herdr-reviewr runs inside herdr, finds its repo, sends comments to the agent, exports to the clipboard, and tracks the agent's turns for the `last-turn` scope.

## Overview

herdr-reviewr ships as a herdr plugin (`herdr-plugin.toml`): a `sidebar` pane entrypoint that runs the binary, a `toggle` action bound to a key, and a `worktree.created` event that auto-opens it for a freshly created worktree. Opening and closing the pane is herdr's job; the binary just runs in it.

The toggle action opens the sidebar with a configurable placement, defaulting to a right split; see [Sidebar placement](#sidebar-placement).

The toggle script (`herdr/sidebar.sh`) opens the sidebar for the focused pane's repo, or closes it if one is already open in the workspace (tracked in `HERDR_PLUGIN_STATE_DIR`). It is bound in user config with `[[keys.command]] type = "plugin_action"`. The pane runs the binary by **absolute path** under the plugin root (`$HERDR_PLUGIN_ROOT/bin/herdr-reviewr`), since the pane's cwd is the repo under review, not the plugin root, and the binary is not on `PATH`. On `herdr plugin install` the build step (`herdr/install.sh`) downloads the prebuilt binary for the platform from the matching GitHub Release into that `bin/` dir; `herdr plugin link` skips the build, so a local checkout populates the same `bin/` itself (`cargo build --release && cp target/release/herdr-reviewr bin/`).

### Sidebar placement

The `toggle` action opens the sidebar with the placement and direction from reviewr's config file, defaulting to a right split. `sidebar.sh` reads the file on every invocation, so an edit takes effect on the next toggle.

```toml
# $HERDR_PLUGIN_CONFIG_DIR/config.toml
toggle_placement = "overlay"   # split | overlay | zoomed | tab   (default: split)
toggle_direction = "down"      # right | down — split only        (default: right)
```

| key | values | default | meaning |
| --- | --- | --- | --- |
| `toggle_placement` | `split`, `overlay`, `zoomed`, `tab` | `split` | how the toggle opens the sidebar |
| `toggle_direction` | `right`, `down` | `right` | split orientation; ignored by the other placements |

Each placement maps to one `herdr plugin pane open` shape (`../docs/herdr-api-notes.md`):

| placement | herdr pane selector | direction | focus on toggle | covers the pane |
| --- | --- | --- | --- | --- |
| `split` | `--target-pane` | `right` beside, `down` below | agent keeps focus | no — tiled beside |
| `tab` | `--workspace` | ignored | reviewr takes focus | no — its own tab |
| `overlay` | active pane (no selector) | ignored | reviewr takes focus | yes — floats on top |
| `zoomed` | `--target-pane` | ignored | reviewr takes focus (herdr-forced) | yes — maximized |

The `split` command keeps today's shape:

```
herdr plugin pane open --plugin persiyanov.reviewr --entrypoint sidebar \
  --placement split --direction right --target-pane <pane> --cwd <repo> --no-focus
```

| # | Always true | A consumer observes it as |
| --- | --- | --- |
| P1 | The toggle opens the placement named by `toggle_placement`, defaulting to `split`. | The configured placement appears on the key press. |
| P2 | Each key defaults independently: an unknown or missing `toggle_placement` opens `split`, an unknown or missing `toggle_direction` opens `right`; neither errors. | A typo in one key still toggles, using that key's default. |
| P3 | `toggle_direction` changes only `split` — `right` beside the pane, `down` below it. | With `tab`/`overlay`/`zoomed`, direction has no visible effect. |
| P4 | The `worktree.created` event opens reviewr only for `split` and `tab`; `overlay`/`zoomed` open nothing. | A new worktree auto-shows the sidebar only in split and tab modes. |
| P5 | reviewr never takes focus on the `worktree.created` event, in any placement. | A fresh worktree never moves the keyboard off the agent. |
| P6 | A manual toggle keeps focus on the agent for `split`, and moves focus to reviewr for `tab`/`overlay`/`zoomed`. | The keyboard lands where that placement is usable. |
| P7 | At most one reviewr pane exists per workspace; a second toggle closes it, whatever placement opened it. | Toggling never stacks two sidebars, even after the config changed between opens. |

**T1 — placement changed between open and close**

1. `toggle_placement = split`; the user toggles → a right split opens; its pane id is written to the per-workspace state file.
2. The user edits the config to `toggle_placement = overlay`.
3. The user toggles → the script finds the live split pane and closes it, whatever placement opened it, and clears the state file (→ P7).
4. The user toggles again → the script reads `overlay` and opens an overlay (→ P1).

**T2 — worktree.created with a covering placement**

1. `toggle_placement = zoomed`.
2. A worktree is created; the event fires with no focused pane.
3. The script reads `zoomed`, not an auto-open placement → opens nothing and writes no state (→ P4).
4. The user later toggles → a zoomed pane opens over the now-focused agent and takes focus (→ P6).

### Repo discovery

The binary reviews one worktree: the pane's working directory, normalized to its git top-level with `git rev-parse --show-toplevel`. If that path is not a git repo, it shows an empty state rather than failing.

### Sending to the agent

The `split`, `overlay`, and `zoomed` placements all open in the agent's tab, so reviewr shares it; the `tab` placement gives reviewr its own tab, so the shared-tab signal is absent and `Send` resolves through the workspace fallback below. `Send` always hands over every written comment at once. To send, the binary:

- resolves the target from `herdr agent list`: the agent in the sidebar's `$HERDR_TAB_ID`, else the sole agent in its `$HERDR_WORKSPACE_ID`;
- writes all comment blocks into that pane with `herdr pane send-text <agent_pane> "<text>"`, without submitting (herdr ≥0.7.5; 0.7.5 removed the earlier `agent send` — its `send-keys` replacement takes key names only, and `agent prompt` submits, which Send must never do);
- focuses that pane with `herdr agent focus <agent_pane>`, so you add context and press enter.

If no agent resolves, or there are two and none shares the tab, the send fails and the status says so; the comments stay in the list. Clipboard copy (also the whole set) still works.

### Clipboard

The clipboard export uses the OS utility (`pbcopy` on macOS). The binary runs where the terminal renders, a local Ghostty, so the clipboard is the user's machine.

### Turn tracking

The `last-turn` scope (`review-model.md`) needs to know when the agent's turn starts. reviewr learns this by polling, not by subscribing: every worktree poll also reads the resolved agent's `agent_status` from `herdr agent list`, and the agent entering `working` from a resting status — `idle` or `done` — is a turn start. The agent is resolved the same way as for a send. The status is one of `idle`, `working`, `blocked`, `done`, or `unknown` (herdr socket API).

A `blocked`→`working` step is a mid-turn resume after a permission or input prompt, and an `unknown`→`working` step is a transient overlay clearing, not a fresh instruction — neither re-baselines, so a turn that spans a prompt stays one turn.

On a turn start, reviewr snapshots the worktree and holds it as a candidate baseline. The candidate is promoted to the live baseline the first poll on which that turn has changed a file — so a turn that edits nothing never moves the baseline, and the previous turn stays on screen. The live baseline is the old side of every `last-turn` diff until the next change-producing turn replaces it.

The snapshot is non-disruptive. reviewr writes a tree from the worktree through a temporary index (`GIT_INDEX_FILE`), never touching the real index, the worktree, or any branch. The tree captures tracked and untracked content via `git add -A`, which respects `.gitignore` — so `last-turn`, like every scope, never surfaces ignored paths (`review-model.md`). It keeps the live baseline as a private ref under `refs/reviewr/turn-base/<worktree-key>` — outside `refs/heads`, so it never appears in a branch list — keyed by the worktree path so sibling worktrees sharing one ref store do not collide. The ref persists, so reopening the sidebar resumes the same baseline.

## Failure semantics

- The send path needs the herdr CLI; browsing diffs and the clipboard export do not, so the core works from a plain shell minus the agent send.
- If the clipboard utility or `herdr pane send-text` fails, the export reports an error and the comment stays in the list (see `review-model.md`). herdr ≥0.7.5 reports CLI failures as a non-zero exit with the JSON error envelope on stdout; pre-0.7.5 hosts exited 0 with the envelope, so both are surfaced.
- With `tab` placement, `Send` cannot use the shared-tab signal, so it resolves the sole agent in the workspace; when the workspace holds more than one agent it fails and the status says so, while `split`/`overlay`/`zoomed` still disambiguate by the shared tab.
- Turn tracking needs the agent status from the herdr CLI; without it the `last-turn` scope stays empty, while `uncommitted` and `branch` are unaffected.
- A turn that starts and ends within one poll interval — or whose start is masked by a transient `unknown` status — is never seen entering `working`, so its start is not snapshotted; `last-turn` then shows the changes accumulated since the last observed turn start, more than one turn, never lines the agent did not write.
- A crash between the snapshot and the ref update leaves an orphaned tree object, which git garbage-collects; `git update-ref` is atomic, so the baseline ref is never half-written.
- The sidebar assumes one instance per worktree; two instances on the same worktree write the same per-worktree ref under last-writer-wins, which is harmless since both compute the same baseline.

## Non-goals

These are not built here; the architecture only stays open to them.

- No server-side clipboard under herdr-over-SSH; the export targets the machine the binary runs on.
- No event subscription — turn tracking polls `agent_status`; reviewr does not open the herdr socket or subscribe to `pane.agent_status_changed`.

## Decisions

- A herdr plugin, not raw pane splits — the official plugin system (`herdr-plugin.toml` with pane entrypoints, actions, and events) gives the keybind, the right-split sidebar, and worktree autolaunch, and is installable/shareable via `herdr plugin install`. Rejected: a user-config `[[keys.command]]` shell script driving `herdr pane split`, which can't declare an entrypoint pane or an event hook.
- Pane command by absolute path under the plugin root, not a relative path or a bare name — a split pane runs with the repo as its cwd, so `./target/release/herdr-reviewr` resolves against the wrong directory, and the prebuilt binary is not on `PATH`; it is invoked as `$HERDR_PLUGIN_ROOT/bin/herdr-reviewr`.
- Prebuilt binaries over build-on-install — `herdr/install.sh` downloads a release binary so users need no Rust toolchain and the install is fast; building from source stays the path for `herdr plugin link` and contributors.
- Send via the herdr CLI, not the raw socket — `$HERDR_BIN_PATH pane send-text` / `agent focus/list` is the documented, transport-stable interface.
- Browsing and clipboard need no herdr — only the agent-send export and `last-turn` tracking depend on herdr, so the rest of the review loop degrades gracefully without it.
- Poll `agent_status`, not subscribe to events — the existing worktree poll already runs every couple of seconds and the CLI already lists agent status, so reading it there adds no socket plumbing or listener thread; the cost is missing a turn shorter than a poll. Rejected: a `pane.agent_status_changed` socket subscription, precise but heavier — a persistent socket connection and a listener thread.
- Snapshot through a temporary index into a private ref, not a stash or a branch — a temp-index `write-tree` captures the worktree without touching the index, worktree, or any branch, and a `refs/reviewr/` ref keeps the tree from being garbage-collected while staying out of branch lists. Rejected: `git stash`, which mutates the worktree; a real branch, which pollutes the user's refs.
- Placement and direction in `config.toml`, parsed by `sidebar.sh`, not passed through herdr — a `plugin_action` keybind has no argument channel, and the pane binary runs *inside* the pane so it cannot choose its own placement; the shell that calls `herdr plugin pane open` is the only place the decision can be made. Rejected: a per-keybind argument (herdr offers none) and a binary-emitted placement (too late — the binary isn't consulted before the pane opens).
- One config file, shared with `theme`/`keep` (`theme.md`) — placement joins the file reviewr already owns rather than a second file. `sidebar.sh` matches the two top-level string keys with a line read, not a TOML parser, since that is all it needs. Rejected: a dedicated placement file.
- `overlay` and `zoomed` never auto-open — a covering pane over a just-created worktree hides the fresh agent, and `overlay` has no pane to attach to on the event (herdr binds it to the active pane, and the event has none). Rejected: auto-opening a fallback split for the covering modes — it collides with the single-pane toggle state and overrides the user's choice of full-screen-on-demand. Reversal: herdr gains a targetable, non-covering overlay.
- Focus follows the placement's usability — `split` stays ambient (agent keeps the keyboard), the tab/overlay/zoomed placements take focus on a manual toggle, and the event never steals focus. Rejected: a blanket `--no-focus`, which leaves an overlay routing keystrokes to the pane it hides.
- `toggle_direction` offers `right|down` only — mirrors herdr's `split` direction set; herdr does not split a pane left or up.

## Open decisions

- None. The `herdr agent list` envelope is confirmed as `result.agents[]` with snake_case `pane_id`/`tab_id`/`workspace_id`, and `agent_status` is one of `idle`, `working`, `blocked`, `done`, or `unknown` (herdr socket API; `idle`/`working`/`done`/`unknown` also seen live, herdr 0.7.1). The resolver keeps a small shape hedge defensively and excludes its own pane, since herdr lists the reviewr sidebar as an agent — a non-agent pane carries `agent_status: unknown` and no `agent` field.

## Related specs

- `./overview.md`
- `./review-model.md`
- `./theme.md`
