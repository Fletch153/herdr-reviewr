---
loop_build_artefact: v1
slug: nvim-harden-quality
base_branch: base-select-v060
verify_cmd: "cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && bash scripts/lua-tests.sh && bash scripts/tui-all.sh"
gate_cmd: "cargo test --quiet && bash scripts/lua-tests.sh"
default_duration: 4h
build_budget: 20m
stop_after_empty_cycles: 2
lens_box_seconds: 900
lenses:
  - {lens: scenario-matrix, context: fresh}
  - {lens: bug-hunt, context: fresh}
  - {lens: race-audit, context: fresh}
  - {lens: simplify, context: fresh}
  - {lens: scenario-matrix, context: fresh}
  - {lens: flake-hunt, context: fresh}
  - {lens: edge-hardening, context: fresh}
build_checkpoints:
  - {name: lua-wrapper, cmd: "bash scripts/lua-tests.sh"}
  - {name: matrix-current, cmd: "test -s artifacts/audit-matrix.md"}
  - {name: baseline, cmd: "cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && bash scripts/lua-tests.sh && bash scripts/tui-all.sh"}
run_notes: |
  Repo root: /home/michael/.local/src/herdr-reviewr — run EVERYTHING from there.
  This is a REFINEMENT loop on an already-built, already-green branch: the scaffolding
  (scripts/lua-tests.sh, artifacts/audit-matrix.md, 37 tui gates) exists. The build phase
  only refreshes the matrix and re-confirms baseline — do NOT rebuild scaffolding.
  The Lua suite wrapper scripts/lua-tests.sh runs:
    REVIEWR_DIR="$PWD/nvim" HERDR_BIN_PATH="$PWD/nvim/tests/stub_herdr.sh" \
    REVIEWR_STUB_LOG=$(mktemp) HERDR_PANE_ID=wY:pSIDEBAR HERDR_TAB_ID=wY:t1 \
    HERDR_WORKSPACE_ID=wY HERDR_PLUGIN_CONTEXT_JSON='{"focused_pane_id":"wY:pFOCUS"}' \
    nvim --headless -u NONE -l nvim/tests/run.lua
  Live probes: source scripts/tui-lib.sh AFTER setting a unique SOCK per script (collisions
  hang gates); helpers: tui_start/keys/frame/wait_for/wait_gone/esc/click/locate/locate_right.
  Gates build+run target/debug — NEVER deploy to the plugin dir mid-run (the user's live
  pane); the deploy recipe is finalize-only (see Termination).
  NEVER pattern-kill `nvim --embed` outside a gate's own fixture cwd (tui-lib.sh's teardown
  already scopes this) — the user has live reviewer panes running as embeds.
  Negative/teeth tests: `git diff > /tmp/x.patch; git checkout -- <paths>; run; git apply /tmp/x.patch`
  — NEVER `git stash` (a foreign stash may exist; do not touch the stash list at all).
  Commits: plain-sentence subjects, body explains the why, NO AI attribution, unsigned is fine.
  Known nvim 0.12.3 quirk: an external write in the same wall-clock second as the last-stored
  file state is persistently missed by checktime — fixtures that need detection must
  future-date mtime (vim.uv.fs_utime) like run.lua's conflict block does.
  tmux send-keys cannot send kitty-encoded keys; send raw CSI-u bytes with
  `keys -l "$(printf '\033[105;5u')"` (ctrl+i example) — crossterm parses CSI u unconditionally.
  The full sweep runs backgrounded (~10 min); poll it, don't block a tick on it.
---

# Harden + tidy the embedded-nvim review mode — loop-build artefact

> Runner: the branch is already built and green. Refine it through the lens cycle
> until the clock runs out or two full cycles find nothing. This is a HARDENING +
> BEHAVIOUR-PRESERVING-CLEANUP loop over existing features — bug fixes, race
> elimination, edge-case coverage, determinism, and constrained simplification.
> **No new features. No missing functionality implemented. No behaviour changes.**
> Every product change is either a bug fix or a strictly behaviour-preserving
> tidy, and every fix that a probe can pin down becomes a permanent gate.

## Goal

The embedded-nvim review mode on `base-select-v060` works in daily use but the
user keeps finding rough edges under real interaction (drill in/out, tab-switch
mid-highlight, comment then flip, revert while walking, delete a file underneath
the editor, restart mid-anything). Recent sessions fixed a run of deletion bugs
(deleted-file ticks, All-files staged-deletion visibility, editor resurrecting a
deleted-underneath file) and the All-files review walk. This loop keeps probing
every feature interaction like a human would — finding and fixing races,
flakiness, and edge cases — and, where the code is *fragile because it leans on
timing* or is *over-engineered*, tightens it (event-driven over racey, simpler
over baroque) **without changing what the tool does**. The product is a hardened
branch, an extended permanent gate suite, and an up-to-date coverage matrix.

## Acceptance criteria

- [ ] `scripts/lua-tests.sh` exists and is green (it does today).
- [ ] `artifacts/audit-matrix.md` is refreshed: every gate currently wired into
      `scripts/tui-all.sh` (37 today, including `tui-del`, `tui-resurrect`,
      `tui-allwalk`, `tui-reviewtick`) is mapped onto the cells it genuinely
      covers, and `open` cells are re-ranked by risk. This is the scenario-matrix
      lens's worklist.
- [ ] The full `verify_cmd` is green at baseline (it is today — a red baseline
      means the environment broke; stop and say so, do not "fix" by weakening a gate).

## How to build it

Tiny build — the scaffolding already exists; the value is entirely in the lenses:
1. Run `scripts/lua-tests.sh` green (checkpoint `lua-wrapper`).
2. Refresh `artifacts/audit-matrix.md`: re-map the current 37 gates onto their
   cells (add the four gates landed since the last run), mark newly-covered cells,
   and re-rank remaining `open` cells by risk (state-carrying features × timing
   dimensions rank highest). Commit the refreshed matrix.
3. Run the full `verify_cmd` once for the `baseline` checkpoint.

## Lenses

The 7-pass cycle leads with live scenario probing (the dominant ask — twice per
rotation), a static correctness review, an adversarial concurrency review, a
constrained behaviour-preserving simplify pass, then a determinism hunt and
hostile-input hardening. All lenses run in fresh subagents; the artefact +
`artifacts/audit-matrix.md` + `LOG.md` are the briefing. Subagents leave edits
uncommitted and report ≤10 lines. A pass that finds nothing actionable within its
box changes nothing (an empty pass).

### scenario-matrix   (custom — fresh)
- why: features were gated individually; the bugs that remain live in the
  *interactions* (drill in → back out, tab-switch with state held, comment + flip
  + revert sequences, delete-underneath + switch) — exactly the "not 100%" reports.
- intent: every plausible user sequence behaves; every probe that finds a bug
  becomes a permanent gate.
- method: read `artifacts/audit-matrix.md`; pick the 3–5 highest-risk `open`
  cells (prefer clusters sharing a fixture). Write ONE throwaway probe under
  `scripts/` (unique SOCK, tui-lib.sh, human-cadence sleeps) driving those
  sequences end-to-end; state the expected outcome per step BEFORE running.
  Divergence → minimal product fix, then promote the probe (or extend the nearest
  gate) into `scripts/tui-all.sh` permanently and teeth-check it RED pre-fix.
  Update the matrix cells either way; delete throwaway scripts fully promoted.
- look for: stale-state carryover across tabs/views (stash swaps, cursor, reveal),
  lock/paint wrong after unusual entry paths (jumplist, tags, :e, Ctrl-o),
  walk/revert/flip interleavings, comment anchors surviving edits and reverts,
  md_view × everything, selection vs poll drift, deleted/renamed files × walk/tick.
- must not: add features; touch out-of-scope paths; leave a red probe as a
  permanent gate without the fix.

### bug-hunt   (palette `/code-review` — fresh)
- why: `scenario-matrix`, `race-audit`, and `edge-hardening` find bugs you can
  *reproduce by driving the UI*; this lens finds the correctness bugs that don't
  need a live repro — logic errors, off-by-one, wrong/over-broad guards, unhandled
  error paths, a `Result`/`Option` swallowed, a match arm that can't be reached or
  one that's missing — by reading the code directly.
- intent: static correctness holes in existing behaviour are found, confirmed, and
  fixed at the root, without changing what the feature does.
- method: run `/code-review` at **high** effort (not max) over the modules the
  recent passes touched, and — rotating by pass (track in LOG.md) — the
  highest-risk untouched module (`src/app.rs`, `src/lib.rs`, `src/nvim/mod.rs`,
  `nvim/lua/reviewr/{diff,live,comments}.lua`, `src/export.rs`). Diff scope uses
  the stored `base_sha`. For each finding, ADVERSARIALLY VERIFY before touching
  code: state the concrete input/interleaving that triggers the wrong output, and
  only proceed if it holds — false positives must churn nothing. Confirmed bug →
  minimal root-cause fix + a regression test (cargo/lua unit where it isolates the
  logic; a tui gate when it only shows through the UI), teeth-checked RED pre-fix.
- look for: correctness, not style (that's `simplify`); real defects, not
  hypotheticals.
- must not: "fix" a low-confidence or unverified finding — log it in LOG.md for a
  later pass instead; change behaviour to satisfy a finding that is actually a
  feature request; touch out-of-scope paths.

### race-audit   (custom — fresh)
- why: the mode is a distributed system (host event loop, nvim RPC notifications,
  a 500 ms poll, the agent writing files underneath) — its worst bugs are ordering
  bugs, and several have already been found this way.
- intent: no interleaving of host events, editor notifications, polls, and
  external writes can corrupt state, lose input, or act on a stale target.
- method: one subsystem per pass, rotating (track in LOG.md):
  1. notification pipeline — ordering of insert/nav/clipboard intents vs
     sync_view/goto/place_last/fire_pending_input; dedup early-returns;
     stale-intent guards (edit_here / nav path mismatch).
  2. poll tick vs user input — checktime storms, entries rebuild racing
     selection/reveal, per-tab stash swaps mid-action.
  3. live-sync — autosave (save_live) vs agent write vs revert_hunk vs undo;
     FileChangedShell branches; forced-write scheduling; quit during pending writes.
  4. editor lifecycle — death/respawn with pending input or mid-flip; theme
     re-push; restart racing the first open; resize during sync.
  5. persistence + clipboard — comments-ref write timing vs quit; two panes on one
     repo (known last-writer-wins clobber; a `#[ignore]`d repro exists); OSC52
     mid-frame interleaving.
  For the chosen subsystem: read the actual code paths, enumerate interleavings
  adversarially, and for each suspected window ATTEMPT A LIVE REPRO (probe with
  `--poll 100`, rapid key sequences, concurrent file writes). Confirmed → fix
  (ordering contract, guard, or event-driven conversion) + regression gate.
  Suspected-but-unreproducible → document the window and argument in LOG.md; fix
  blind only if the reasoning is airtight and the fix strictly tightens.
- must not: introduce locks/sleeps/bare timeouts as duct tape — fix ordering by
  design (ordered notifications, single-writer rules, event triggers), matching
  the existing contracts.

### simplify   (palette `/simplify` — fresh)
- why: the user wants fragile-because-timing and over-engineered code tidied, but
  ONLY behaviour-preserving and never as an architectural rewrite. Deep restructure
  is out unless it is the root cause of a confirmed bug (then it belongs to
  scenario-matrix/race-audit, not here).
- intent: the same behaviour, driven more robustly and expressed more simply.
- method: invoke `/simplify` over the in-scope diff/module the previous passes
  touched (and adjacent code they exposed). Prefer, in order: (a) replace a
  fragile timing dependency — a sleep, a fixed delay, a "hope the poll caught it"
  — with an explicit event/trigger where one already exists in the codebase's
  vocabulary; (b) collapse a shallow pass-through, dedup a copy-paste, delete a
  speculative abstraction or dead branch; (c) tighten an over-broad guard. Each
  change must keep the full `verify_cmd` green — and if it touches nvim lua,
  paint, or key/mouse routing, run the nearest `tui-*-test.sh` before considering
  it done (the fast gate_cmd alone won't catch a paint/routing regression).
- look for: duplicated logic across `src/app.rs`/`src/lib.rs`, timing-based waits
  that a real event already precedes, options/branches no caller reaches,
  guards broader than their invariant.
- must not: change any observable behaviour, public function contracts, on-disk
  formats (refs, blobs), or gate assertions; perform a module/interface reshape;
  "simplify" by removing a feature or a safety check. When unsure whether a change
  is behaviour-preserving, treat it as out of scope and skip it (empty pass).

### flake-hunt   (custom — fresh)
- why: the user explicitly wants "no flakiness"; a 50 %-flake (nvim mtime aliasing)
  was root-caused here before, and a flaky gate suite silently rots verification.
- intent: the full suite is deterministic — repeated green sweeps, any intermittent
  failure root-caused to product-vs-test and fixed at the root.
- method: run `bash scripts/tui-all.sh` and `bash scripts/lua-tests.sh` back to
  back (≥2×, 3× budget permitting). Any failure: re-run that gate alone 5×, capture
  frames/logs, root-cause. Product race → fix product (outranks the timebox; carry
  into LOG.md if needed). Test artefact (sleep-tuned assertion, fixture aliasing,
  socket collision) → fix the harness properly (event-wait not sleep, future-dated
  mtimes), never by widening sleeps blindly. Note: a switch-teardown "reviewer did
  not quit" has flaked once in-suite while passing in isolation — if it recurs,
  root-cause the quit/teardown ordering, don't paper over it.
- must not: mark anything "known flaky" and move on; delete assertions to pass.

### edge-hardening   (custom — fresh)
- why: the mode assumes a friendly repo; real trees have renames+edits, no-EOL
  files, unicode, binaries, symlinks, huge diffs, empty repos — past bugs (EOL
  ghost diffs, rename big-insertion, deleted-file class) came from exactly this.
- intent: hostile-but-legal inputs degrade gracefully (correct paint, no hangs,
  honest statuses), never corrupt state or crash the pane.
- method: one input class per pass, rotating (track in LOG.md): (1) empty repo /
  zero commits / detached HEAD; (2) unicode paths+content, very long lines, CRLF;
  (3) rename+edit combos, rename onto deleted, case-only renames; (4) binary files
  and size extremes (a 5k-line diff, a 0-byte file); (5) symlinks, nested dirs,
  files vanishing mid-review. Build the fixture, drive the affected features live
  (probe), fix, promote gates for anything that broke. Update the matrix's
  file-state column.
- must not: "handle" an input by silently skipping features that should work.

## Termination

- 4 h wall-clock refinement budget (`default_duration`), 20 m build cap. Override
  the duration at launch (e.g. `… 2h`).
- Converged = 2 consecutive all-empty cycles → stop early (`stop_after_empty_cycles`).
- **Finalize — DEVIATION from the default flow, per the user's standing rules
  (no squash, no sign, no PR; push is to the user's FORK only):**
  1. Do NOT squash, do NOT sign, do NOT open any PR. Per-pass commits are preserved.
  2. Ensure the tree is clean and `verify_cmd` green (normal stop-reached duties apply).
  3. `git switch base-select-v060 && git merge --no-ff loop-build/nvim-harden-quality
     -m "merge loop-build/nvim-harden-quality: embedded-nvim hardening + tidy"`.
  4. Deploy: `cargo build --release`, then
     `DEST=/home/michael/.config/herdr/plugins/github/persiyanov.reviewr-e87b654a74f8`,
     `rm -f "$DEST/bin/herdr-reviewr" && cp target/release/herdr-reviewr "$DEST/bin/herdr-reviewr"`
     (rm first: the running pane holds the binary — plain cp gets ETXTBSY),
     `rsync -a --delete nvim/ "$DEST/nvim/"`, verify md5s match.
  5. Push: `git push fork base-select-v060` (remote `fork` = Fletch153/herdr-reviewr;
     NEVER push to `origin` = persiyanov upstream).
  6. PushNotification summarizing: passes run, bugs fixed (count + one-liners),
     tidies made, gates added, matrix coverage delta, "restart the reviewer pane".
  7. `phase = done`.
- Draft path (build never green — near-impossible; baseline is green today): stop,
  notify, leave the branch unmerged, deploy nothing, push nothing.

## Scope

- in-scope: `src/`, `nvim/`, `scripts/`, `tests/`, `artifacts/audit-matrix.md`,
  `README.md` (doc corrections only).
- out-of-scope: `Cargo.toml` dependency additions (dev-deps for tests are fine —
  no new runtime crates), `.github/`, the deployed plugin directory (finalize
  only), `origin` (upstream — never push), the stash list, any architectural
  reshape not demanded by a confirmed bug.

## Resources & context

**Recently hardened (do NOT re-litigate; these are done and gated):** per-tab
reviewed ticks; next-comment button removed; Uncommitted base follows HEAD; poll
500 ms; All-files review walk steps every file (editor + file-pane) skipping dir
placeholders; deleted files stay tickable across rescans; All-files lists the
worktree ∪ changeset so a staged deletion stays visible; the view-switch autosave
(`reviewr.live.save_live()`) no longer resurrects a file deleted underneath it.
Gates for these: `tui-del`, `tui-resurrect`, `tui-allwalk`, `tui-reviewtick`.

**Feature inventory** (matrix seed — the nvim-era features):
1. Embed lifecycle: spawn/eager start, dead-editor panel + `r` restart, one-shot
   auto-respawn, `:q` reopen, quit confirmation, theme push.
2. Diff paint: signs, DiffAdd line paint, red virt_lines (old side), gutter
   statuscolumn (wrapped rows, number cells), breakindent drop/restore.
3. Context folds: foldexpr/foldtext, `zx` recompute on scope change.
4. View model: focused/plain stamping (`b:reviewr_plain`, `M._view`), BufEnter
   re-derive, deleted-file scratch (`reviewr://deleted/`), rename mapping.
5. Read-only Changes: lock lifecycle, insert/paste flip (`i a o … gi`), pending
   input firing after the plain sync, tree reveal+selection, `rd` split lift.
6. Review walk: Enter/BS maps (buffer guards, quickfix passthrough), `]c`/`[c`,
   files-pane Enter (toggle mark + advance; Space retired), cross-file
   advance/retreat with marking (both tabs), place-last, changeset/all-files wrap.
   Reviewed ticks: per-tab now, persisted `refs/reviewr/reviewed/<key>` (v2),
   pruned by content hash only (deletions survive).
7. Hunk revert (`space rh`): all hunk shapes, EOL flip, added-file refusal,
   last-hunk advance, lock restore.
8. Comments: rc/re/rx/rr/rl/rs/ry, boxed cards + number accents, card-row click
   fallback, composer overlay, list overlay, jump-to-comment, sent semantics.
9. Live sync: InsertLeave/TextChanged autosave, poll checktime, FileChangedShell
   policy (reload/keep+force-write/deleted), conflict = user wins, save_live guard.
10. Comment persistence: `refs/reviewr/comments/<worktree-key>` blob, seed on
    start, ref delete on empty, revision-guarded writes (two-pane clobber known).
11. Markdown view: sticky `md_view`, state chip, `p` from files pane, scroll
    routing, non-md passthrough.
12. Clipboard: embed `g:clipboard` → host OSC52, cache-answered pastes, fallback.
13. Ctrl+i All-files→Changes return; Tab focus toggle; 1/2/3 tab switch; per-tab
    stash swaps.
14. Scope/base: b/t/C, base+commit chips/pickers (row-0 Uncommitted), re-diff in
    place, `g:reviewr_base`, rename push.
15. EOL: `nofixendofline`, EOL-note virt line + `~` sign, eol hunk revert,
    byte-exact base splitting.
16. Host UI interop: mouse→nvim grid routing, divider drag, `[`/`]` resize, `/`
    filter, search, stage-marker click, backspace delete-file, `x` expand, `?`
    help, md chip, Send button.

**Architecture map**: `src/lib.rs` event loop + key/mouse routing + NvimSession +
notification dispatch + nvim_sync; `src/app.rs` App state (tabs, stash, entries,
walk, revert-advance, persist, md_view, reviewed prune); `src/nvim/mod.rs` engine,
resolve_nvim, switch payloads (`silent! call luaeval('…save_live()') | …`);
`src/export.rs` clipboard+OSC52; `nvim/lua/reviewr/diff.lua` paint/lock/walk/
revert/views; `live.lua` sync policy + save_live; `comments.lua` anchors/cards/
intents; `plugin/reviewr.lua` maps + clipboard provider. Tests: `tests/*.rs`
(cargo), `nvim/tests/run.lua` (~130 checks), `scripts/tui-*-test.sh` (37 gates via
`scripts/tui-all.sh`).

**Known-fragile / open (check early):**
- Two-pane comment-ref write is last-writer-wins with no CAS — a `#[ignore]`d
  repro exists in `tests/git_repo.rs`; the user runs multiple panes, so this bites.
  A CAS + merge is the real fix (race-audit subsystem 5) but is a behaviour-
  affecting change to persistence — flag it, do not silently reshape.
- tui-wrap step-5 SGR parser carries bg per row only (fragile to layout change).
- nvim same-second checktime miss (see run_notes) — low product exposure; a probe
  of agent-write-then-immediate-poll would confirm.
- Intermittent "No tag file"/LSP tags report — never root-caused; if a probe can
  reproduce (All files → immediate Ctrl-]), fix; document the attempt either way.
- A switch-teardown "reviewer did not quit" flaked once in-suite (passed in
  isolation) — flake-hunt owns it if it recurs.
- `feedkeys("", "x")` exits insert mode by design — assert passthrough by outcome,
  not `mode()`. Gates assert via `frame | grep` — file names appear in BOTH panes,
  so grep content markers, not names.

**Prior art**: every existing gate is a pattern library (fixtures, flip sequences,
paste bytes, mouse synthesis). Commit bodies in this repo are unusually informative
— read `git log --oneline fork/base-select-v060..HEAD` (and recent deletion-fix
commits) before re-deriving intent. The prior hardening run's ledger lives in
`.loop-build/LOG.md` (git-excluded) — a fresh pass reads the matrix, not that log.
