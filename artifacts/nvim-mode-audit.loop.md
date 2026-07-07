---
loop_build_artefact: v1
slug: nvim-mode-audit
base_branch: base-select-v060
verify_cmd: "cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && bash scripts/lua-tests.sh && bash scripts/tui-all.sh"
gate_cmd: "cargo test --quiet && bash scripts/lua-tests.sh"
default_duration: 3h
build_budget: 30m
stop_after_empty_cycles: 2
lens_box_seconds: 900
lenses:
  - {lens: scenario-matrix, context: fresh}
  - {lens: race-audit, context: fresh}
  - {lens: scenario-matrix, context: fresh}
  - {lens: flake-hunt, context: fresh}
  - {lens: edge-hardening, context: fresh}
build_checkpoints:
  - {name: lua-wrapper, cmd: "bash scripts/lua-tests.sh"}
  - {name: matrix-seed, cmd: "test -s artifacts/audit-matrix.md"}
  - {name: baseline, cmd: "cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && bash scripts/lua-tests.sh && bash scripts/tui-all.sh"}
run_notes: |
  Repo root: /home/michael/.local/src/herdr-reviewr — run EVERYTHING from there.
  The Lua suite needs its env; the build phase creates scripts/lua-tests.sh with exactly:
    REVIEWR_DIR="$PWD/nvim" HERDR_BIN_PATH="$PWD/nvim/tests/stub_herdr.sh" \
    REVIEWR_STUB_LOG=$(mktemp) HERDR_PANE_ID=wY:pSIDEBAR HERDR_TAB_ID=wY:t1 \
    HERDR_WORKSPACE_ID=wY HERDR_PLUGIN_CONTEXT_JSON='{"focused_pane_id":"wY:pFOCUS"}' \
    nvim --headless -u NONE -l nvim/tests/run.lua
  Live probes: source scripts/tui-lib.sh AFTER setting a unique SOCK per script (collisions
  hang gates); helpers: tui_start/keys/frame/wait_for/wait_gone/esc/click/locate/locate_right.
  Gates build+run target/debug — NEVER deploy to the plugin dir mid-run (the user's live
  pane); the deploy recipe is finalize-only (see Termination).
  Negative tests: `git diff > /tmp/x.patch; git checkout -- <paths>; run; git apply /tmp/x.patch`
  — NEVER `git stash` (a foreign stash may exist; do not touch the stash list at all).
  Commits: plain-sentence subjects, body explains the why, NO AI attribution, unsigned is fine.
  Known nvim 0.12.3 quirk: an external write in the same wall-clock second as the last-stored
  file state is persistently missed by checktime — fixtures that need detection must
  future-date mtime (vim.uv.fs_utime) like run.lua's conflict block does.
  tmux send-keys cannot send kitty-encoded keys; send raw CSI-u bytes with
  `keys -l "$(printf '\033[105;5u')"` (ctrl+i example) — crossterm parses CSI u unconditionally.
---

# Harden the embedded-nvim review mode — loop-build artefact

> Runner: build the audit scaffolding below, then refine through the lens cycle
> until the clock runs out or two full cycles find nothing. This is a HARDENING
> loop over existing features — bug fixes, race elimination, edge-case coverage,
> and tactical refactors where a design is the root cause of fragility. No new
> features. Definition of done for the *build* is the Acceptance Criteria.

## Goal

The 50 unpushed commits on `base-select-v060` (`fork/base-select-v060..HEAD`,
first commit `ec35fd1` "Add an nvim companion-editor mode") built the entire
embedded-nvim review mode. It works in daily use but "doesn't feel 100%": the
user wants every feature interaction probed like a human would — drill in, drill
back out, switch tabs mid-highlight, leave a comment then flip views, revert
while walking, restart mid-anything — with races, flakiness, and edge cases
found and fixed. The loop's product is a hardened branch plus a permanent,
extended gate suite and a coverage matrix documenting what was probed.

## Acceptance criteria

- [ ] `scripts/lua-tests.sh` exists (the exact incantation from run_notes,
      `set -euo pipefail`, exits non-zero on any FAIL) and is green.
- [ ] `artifacts/audit-matrix.md` exists, seeded from the feature inventory in
      Resources: one row per feature, columns for interaction dimensions
      (tab × focus × view-state × timing × file-state), each cell marked
      `covered` (name the existing gate), `probed` (this loop), or `open`.
      Existing coverage from the 22 gates in scripts/tui-all.sh is mapped in.
- [ ] The full `verify_cmd` is green at baseline (it is today — a red baseline
      means the environment broke, stop and say so).

## How to build it

Small, deliberate build — the value is in the lenses:
1. Write `scripts/lua-tests.sh` (wrapper above). Run it green.
2. Write `artifacts/audit-matrix.md`: enumerate the 16 features (Resources),
   map each of the 22 existing gates onto the cells they genuinely cover, mark
   everything else `open`. Rank `open` cells by risk (state-carrying features ×
   timing dimensions rank highest). This file is the scenario-matrix lens's
   worklist and is committed/updated by every pass that probes cells.
3. Run the full verify_cmd once for the baseline checkpoint.

## Lenses

The cycle leads with live scenario probing (the user's dominant ask — it appears
twice per rotation), alternated with an adversarial concurrency review, a
determinism hunt, and hostile-input hardening. All lenses run in fresh
subagents; the artefact plus `artifacts/audit-matrix.md` plus `LOG.md` are the
briefing. Subagents leave edits uncommitted and report ≤10 lines.

### scenario-matrix   (custom — fresh)
- why in this cycle: the features were gated individually; the bugs that remain
  live in the *interactions* (drill in → back out, tab-switch with state held,
  comment + flip + revert sequences) — exactly what the user reports as "not 100%".
- intent: every plausible user sequence behaves; every probe that finds a bug
  becomes a permanent gate.
- method: read `artifacts/audit-matrix.md`; pick the 3–5 highest-risk `open`
  cells (prefer clusters sharing a fixture). Write ONE throwaway probe script
  under `scripts/` (unique SOCK, tui-lib.sh, human-cadence sleeps) driving those
  sequences end-to-end; state the expected outcome per step BEFORE running.
  Divergence → minimal product fix (tactical refactor allowed when the design is
  the cause), then promote the probe (or extend the nearest existing gate) into
  `scripts/tui-all.sh` permanently. Update the matrix cells with findings either
  way; delete throwaway scripts that were fully promoted.
- look for: stale-state carryover across tabs/views (stash swaps, cursor,
  reveal), lock/paint wrong after unusual entry paths (jumplist, tags, :e,
  Ctrl-o), walk/revert/flip interleavings, comment anchors surviving edits and
  reverts, md_view × everything, selection vs poll drift.
- must not: add features; touch out-of-scope paths; leave a red probe as a
  permanent gate without the fix.

### race-audit   (custom — fresh)
- why in this cycle: the mode is a distributed system (host event loop, nvim
  RPC notifications, a 500ms poll, the agent writing files underneath) — its
  worst bugs are ordering bugs, and several were already found this way.
- intent: no interleaving of host events, editor notifications, polls, and
  external writes can corrupt state, lose input, or act on a stale target.
- method: one subsystem per pass, rotating in this order (track in LOG.md):
  1. notification pipeline — ordering of insert/nav/clipboard intents vs
     sync_view/goto/place_last/fire_pending_input; the dedup early-return
     conditions; stale-intent guards (edit_here path mismatch).
  2. poll tick vs user input — checktime storms, entries rebuild racing
     selection/reveal, per-tab stash swaps mid-action.
  3. live-sync — autosave vs agent write vs revert_hunk vs undo; FileChangedShell
     branches; the forced-write scheduling; quit during pending writes.
  4. editor lifecycle — death/respawn with pending input or mid-flip; theme
     re-push; restart racing the first open; resize during sync.
  5. persistence + clipboard — comments-ref write timing vs quit; two panes on
     one repo; OSC52 mid-frame interleaving.
  For the chosen subsystem: read the actual code paths, enumerate interleavings
  adversarially, and for each suspected window ATTEMPT A LIVE REPRO (probe
  script with tightened poll `--poll 100`, rapid key sequences, concurrent file
  writes from the script). Confirmed → fix (ordering contract, guard, or
  tactical refactor) + regression gate. Suspected-but-unreproducible → document
  the window and the argument in LOG.md; only fix blind if the reasoning is
  airtight and the fix is strictly tightening.
- must not: introduce locks/sleeps as duct tape — fix ordering by design
  (ordered notifications, single-writer rules), matching the existing contracts.

### flake-hunt   (custom — fresh)
- why in this cycle: the user explicitly wants "no flakiness"; one 50%-flake
  (nvim mtime aliasing) was already root-caused here — others may lurk, and a
  flaky gate suite silently rots the whole verification story.
- intent: the full suite is deterministic — N consecutive green sweeps, and any
  intermittent failure is root-caused to product-vs-test and fixed at the root.
- method: run `bash scripts/tui-all.sh` and `bash scripts/lua-tests.sh` 3×
  back-to-back (budget permitting; at minimum 2×). Any failure: re-run that gate
  alone 5×, capture frames/logs, root-cause. Product race → fix product (this
  outranks the timebox — carry into LOG.md for the next pass if needed). Test
  artifact (sleep-tuned assertion, fixture aliasing, socket collision) → fix the
  harness properly (event-wait instead of sleep, future-dated mtimes), never by
  widening sleeps blindly. Once per run, also sweep at a narrow size (edit a
  copy of tui-lib.sh's geometry? no — run one representative gate with
  `-x 120 -y 30` via a temporary variant) and triage what breaks.
- must not: mark anything "known flaky" and move on; delete assertions to make
  suites pass.

### edge-hardening   (custom — fresh)
- why in this cycle: the mode assumes a friendly repo; real trees have renames+
  edits, no-EOL files, unicode, binaries, symlinks, huge diffs, empty repos —
  several past bugs (EOL ghost diffs, rename big-insertion) came from exactly
  this class.
- intent: hostile-but-legal inputs degrade gracefully (correct paint, no hangs,
  honest statuses), never corrupt state or crash the pane.
- method: one input class per pass, rotating (track in LOG.md): (1) empty repo /
  zero commits / detached HEAD; (2) unicode paths+content, very long lines,
  CRLF; (3) rename+edit combos, rename onto deleted, case-only renames;
  (4) binary files and size extremes (a 5k-line diff, a 0-byte file);
  (5) symlinks, nested submodule-ish dirs, files vanishing mid-review.
  Build the fixture, drive the affected features live (probe script), fix,
  promote gates for anything that broke. Update the matrix's file-state column.
- must not: "handle" an input by silently skipping features that should work.

## Termination

- 3h wall-clock refinement budget (`default_duration`), 30m build cap.
- Converged = 2 consecutive all-empty cycles → stop early (`stop_after_empty_cycles`).
- **Finalize — DEVIATION from the default flow, per the user's standing rules
  (no pushes, no PRs, no history rewrites):**
  1. Do NOT squash, do NOT sign, do NOT push, do NOT open any PR.
  2. Ensure the tree is clean and `verify_cmd` green (the normal stop-reached
     duties apply).
  3. `git switch base-select-v060 && git merge --no-ff loop-build/nvim-mode-audit
     -m "merge loop-build/nvim-mode-audit: embedded-nvim hardening audit"`
     (per-pass commits preserved).
  4. Deploy: `cargo build --release`, then
     `DEST=/home/michael/.config/herdr/plugins/github/persiyanov.reviewr-e87b654a74f8`,
     `rm "$DEST/bin/herdr-reviewr" && cp target/release/herdr-reviewr "$DEST/bin/herdr-reviewr"`
     (rm first: the running pane holds the binary — plain cp gets ETXTBSY),
     `rsync -a --delete nvim/ "$DEST/nvim/"`, verify md5s match.
  5. PushNotification summarizing: passes run, bugs fixed (count + one-liners),
     gates added, matrix coverage delta, "restart the reviewer pane".
  6. `phase = done`.
- Draft path (build never green — near-impossible since baseline is green
  today): stop, notify, leave the branch unmerged, deploy nothing.

## Scope

- in-scope: `src/`, `nvim/`, `scripts/`, `tests/`, `artifacts/audit-matrix.md`,
  `README.md` (doc corrections only).
- out-of-scope: `Cargo.toml` dependency additions (dev-deps for tests included —
  no new crates at all), `.github/`, the deployed plugin directory (finalize
  only), any remote operation (fetch is fine; push never), the stash list.

## Resources & context

**Feature inventory** (seed for the matrix — the 16 nvim-era features):
1. Embed lifecycle: spawn/eager start, dead-editor panel + `r` restart,
   one-shot auto-respawn, `:q` reopen, quit confirmation, theme push.
2. Diff paint: signs, DiffAdd line paint, red virt_lines (old side), gutter
   statuscolumn (wrapped rows, number cells), breakindent drop/restore.
3. Context folds: foldexpr/foldtext, `zx` recompute on scope change.
4. View model: focused/plain stamping (`b:reviewr_plain`, `M._view`), BufEnter
   re-derive, deleted-file scratch (`reviewr://deleted/`), rename mapping.
5. Read-only Changes: lock lifecycle, insert/paste flip (`i a o … gi`), pending
   input firing after the plain sync, tree reveal+selection, `rd` split lift.
6. Review walk: Enter/BS maps (buffer guards, quickfix passthrough), `]c`/`[c`,
   files-pane Enter (toggle mark + advance; Space is retired), cross-file
   advance/retreat with marking (both tabs), place-last, changeset wrap.
   Reviewed ticks: per-file (never per-view), persisted in
   `refs/reviewr/reviewed/<key>`, pruned only on content change.
7. Hunk revert (`space rh`): all hunk shapes, EOL flip, added-file refusal,
   last-hunk advance, lock restore.
8. Comments: rc/re/rx/rr/rl/rs/ry, boxed cards + number accents, card-row click
   fallback (`end+1`), composer overlay, list overlay, jump-to-comment, sent
   semantics.
9. Live sync: InsertLeave/TextChanged autosave, poll checktime, FileChangedShell
   policy (reload/keep+force-write/deleted), conflict = user wins.
10. Comment persistence: `refs/reviewr/comments/<worktree-key>` blob, seed on
    start, ref delete on empty, revision-guarded per-tick writes.
11. Markdown view: sticky `md_view`, state-labeled chip, `p` from files pane,
    scroll routing, non-md passthrough, chip hidden for non-md.
12. Clipboard: embed `g:clipboard` provider → host OSC52 (`\x1b]52;c;<b64>\x07`),
    cache-answered pastes, host export tool-else-OSC52 fallback.
13. Ctrl+i All-files→Changes return; Tab focus toggle (normal-ish only);
    1/2/3 tab switches; per-tab stash swaps.
14. Scope/base: b/t/C, base+commit chips/pickers, re-diff in place,
    `g:reviewr_base`, rename push (`set_renames`).
15. EOL: `nofixendofline`, EOL-note virt line + `~` sign, eol hunk revert,
    byte-exact base splitting (`show_blob`).
16. Host UI interop: mouse→nvim grid routing (drag/wheel/prompts), divider
    drag, `[`/`]` resize, `/` filter, search, stage-marker click, backspace
    delete-file, `x` expand, `?` help, md chip, Send button.

**Architecture map**: `src/lib.rs` event loop + key/mouse routing + NvimSession
+ notification dispatch + nvim_sync; `src/app.rs` App state (tabs, stash,
entries, walk, revert-advance, persist, md_view); `src/nvim/mod.rs` engine,
resolve_nvim, payloads (`silent! update! | …`); `src/export.rs` clipboard+OSC52;
`nvim/lua/reviewr/diff.lua` paint/lock/walk/revert/views; `live.lua` sync
policy; `comments.lua` anchors/cards/intents; `plugin/reviewr.lua` maps +
clipboard provider. Tests: `tests/*.rs` (cargo), `nvim/tests/run.lua` (~130
checks), `scripts/tui-*-test.sh` (22 gates via `scripts/tui-all.sh`).

**Known-fragile spots** (prior audit + open questions — check these early):
- tui-wrap step-5 SGR parser carries bg per row only (works via border
  re-emission; fragile to layout change).
- statuscolumn omits `%C` (foldcolumn users) — cosmetic, note only.
- nvim same-second checktime miss (see run_notes) — product exposure believed
  low; a scenario probe of agent-write-then-immediate-poll would confirm.
- Intermittent "No tag file"/LSP tags report from the user — never root-caused;
  if a probe can reproduce (flip to All files → immediate Ctrl-]), fix; document
  the attempt either way.
- `feedkeys("", "x")` exits insert mode by design — assert passthrough effects
  by outcome, not `mode()`.
- Gates assert via `frame | grep`; file names appear in BOTH panes — grep for
  content markers, not names.

**Prior art**: every existing gate is a pattern library (fixtures, flip
sequences, paste bytes, mouse synthesis). `git log --oneline
fork/base-select-v060..HEAD` narrates every feature and its known caveats —
commit bodies are unusually informative in this repo; read them before
re-deriving intent.
