---
loop_build_artefact: v1
slug: nvim-parallel-hunt
base_branch: base-select-v060
verify_cmd: "cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && bash scripts/lua-tests.sh && bash scripts/tui-all.sh"
default_duration: 3h
build_budget: 15m
stop_after_empty_cycles: 2
# NON-STANDARD LOOP SHAPE — see "## Loop protocol". This artefact does NOT use the
# stock one-lens-per-tick cursor. `finders` below all run IN PARALLEL every loop.
finder_model: opus
finder_effort: xhigh
finders:
  - {finder: interaction, context: fresh}
  - {finder: concurrency, context: fresh}
  - {finder: correctness-edge, context: fresh}
build_checkpoints:
  - {name: baseline, cmd: "cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && bash scripts/lua-tests.sh && bash scripts/tui-all.sh"}
run_notes: |
  Repo root: /home/michael/.local/src/herdr-reviewr — run EVERYTHING from there.
  Branch is already built and green (44 tui gates + cargo + lua). Build phase only
  re-baselines; do NOT rebuild scaffolding.

  THIS IS A PARALLEL FIND -> BATCH-FIX LOOP, not the stock cursor loop. One loop =
  (1) fan out the 3 read-only finders concurrently; (2) orchestrator collects,
  dedups, filters, triages worth-fixing; (3) orchestrator fixes the whole accepted
  batch + adds a regression gate per fix; (4) run verify_cmd once; green -> one
  batch commit. Empty loop = nothing survived triage.

  FINDER DISPATCH: the 3 finders run at model=opus, effort=xhigh, cold/fresh
  context, and must return STRUCTURED findings (the schema in "## Finding schema").
  The plain Agent tool has no effort knob, so dispatch the find phase through the
  workflow fan-out (`parallel([...])` with `agent(prompt,{model:'opus',
  effort:'xhigh', schema: FINDING_ARRAY})`) — that is the only way to honour xhigh
  and get validated structured output. One workflow run per loop returns the three
  finders' arrays; the orchestrator (this session) does all triage + fixing +
  gating + committing INLINE (subagents never fix, never commit).

  FINDERS ARE SOURCE-READ-ONLY: they may spawn nvim and write THROWAWAY probes in a
  scratch dir to CONFIRM a bug live, but must NEVER edit tracked source (src/,
  nvim/) or leave tracked changes. They report; the orchestrator fixes.

  Live probe harness: `source scripts/tui-lib.sh` AFTER setting a unique SOCK;
  helpers tui_start/keys/frame/wait_for/wait_gone/esc/click/locate/locate_right.
  NEVER pattern-kill `nvim --embed` outside a gate fixture's own cwd — the user has
  LIVE reviewer panes running as embeds; tui-lib.sh's teardown already scopes this.
  Lua suite wrapper is scripts/lua-tests.sh. Known nvim 0.12.3 quirk: an external
  write in the SAME wall-clock second as the last-stored file state is missed by
  checktime — future-date mtime (`vim.uv.fs_utime`) in fixtures needing detection.
  tmux send-keys can't send kitty CSI-u keys; send raw bytes: `keys -l "$(printf
  '\033[105;5u')"`. Commits: plain-sentence subjects, body explains why, NO AI
  attribution, unsigned (--no-gpg-sign) is fine. Never `git stash`; never touch the
  stash list. The full sweep runs ~10 min — background it, poll the log.

  KNOWN-FINDINGS LEDGER: keep `.loop-build/known-findings.md` — every finding that
  was FIXED, REJECTED (not-a-bug/out-of-scope), or DEFERRED (real but out-of-lens/
  behaviour-changing). Feed it to every finder each loop as "already triaged — do
  NOT re-report these", so fresh xhigh finders don't burn tokens re-surfacing the
  same non-actionable items.
---

# Parallel bug-hunt on the embedded-nvim review mode — loop-build artefact

> Runner: the branch is built and green. Each loop fans out 3 read-only finders in
> parallel (opus, xhigh), then YOU (the orchestrator) dedup, triage, and fix the
> whole accepted batch, add a regression gate per fix, run the full gate once, and
> commit the batch. Repeat until the clock runs out or two consecutive loops find
> nothing worth fixing. **Bug fixes + behaviour-preserving tidies only. No new
> features, no missing-functionality, no behaviour changes.**

## Goal

Harden the embedded-nvim review mode faster than the sequential loop did, by
running the three bug-finding perspectives concurrently and having the orchestrator
apply all worthwhile fixes in one gated batch per loop. The prior sequential run
found its two real bugs (stale editor paint on delete-underneath; markdown scroll
bleeding across tabs) via live interaction probing; this loop keeps that strength
but parallelises the search and centralises the fix decision, so more ground is
covered per hour and duplicate/cosmetic findings are filtered once by a single
judgment rather than acted on pass-by-pass. Product = a hardened branch + an
extended permanent gate suite.

## Acceptance criteria

- [ ] Full `verify_cmd` green at baseline (it is today — a red baseline means the
      environment broke; stop and say so, never "fix" by weakening a gate).
- [ ] `.loop-build/known-findings.md` exists and is maintained across loops.
- [ ] Every ACCEPTED finding that gets fixed lands with a minimal root-cause fix +
      a regression test/gate that was teeth-checked RED before the fix.

## How to build it

Tiny build (scaffolding exists):
1. Run the full `verify_cmd` once for the `baseline` checkpoint (must be green).
2. Seed `.loop-build/known-findings.md` from the prior run's non-bugs so the
   finders don't re-surface them: the cosmetic "N of M reviewed" status denominator
   (user scoped OUT); the FileChangedShell "modified" vim.schedule unreachable
   window; the auto_respawned reset-timing window (non-corrupting, `r` recovers);
   the two-pane comment-ref clobber (real, but the CAS+merge fix is behaviour-
   affecting — DEFERRED, needs the user); the nvim `virt_lines_above` first-line
   clip (generic nvim limitation); the nvim paint path lacking a binary/too-large
   guard (degrades gracefully via pcall — DEFERRED).

## Loop protocol (replaces the stock cursor loop)

One **loop** = one full iteration below. Track loop index, findings, and
accepted/fixed counts in `STATE.json`; append to `LOG.md` each loop.

1. **Clock.** `now=$(date +%s)`; `now >= deadline_epoch` -> **Stop reached**.
2. **Snapshot** for a scoped revert: tree is clean here (last loop committed or was
   empty). `git ls-files --others --exclude-standard | sort >
   .loop-build/untracked_before.txt`.
3. **Fan out the 3 finders in parallel** (workflow fan-out; model=opus,
   effort=xhigh; each `fresh`). Hand each: its finder definition from `## Finders`,
   `run_notes`, `## Scope`, the current `.loop-build/known-findings.md` (do-not-
   re-report list), and the `## Finding schema`. Each returns a findings array
   (may be empty). Finders are SOURCE-READ-ONLY (probe to confirm, never edit
   tracked source, never commit).
4. **Collect + dedup.** Merge the three arrays. Dedup by root cause (same
   file/function + same symptom counts once even if two finders raised it; note
   the corroboration — it raises confidence).
5. **Triage each finding** against `## Triage rubric` -> FIX | DEFER | REJECT.
   Append DEFER/REJECT (with the reason) and FIX (after it lands) to
   `.loop-build/known-findings.md`.
6. **Batch-fix all FIX findings (orchestrator, inline).** For each: reproduce if
   the finder only suspected it; write the MINIMAL root-cause fix (behaviour-
   preserving / bug-only, in-scope); add a regression test — a `tests/*.rs` or
   `nvim/tests/run.lua` unit when the logic isolates, else a `scripts/tui-*-test.sh`
   gate wired into `scripts/tui-all.sh` — and TEETH-CHECK it RED before the fix,
   GREEN after. Behaviour-preserving tidies that a finding exposes (fragile-timing
   -> event, dead branch) are allowed in the same batch under the same bar.
7. **Gate once (authoritative).** Run the full `verify_cmd`. 
   - **Green** -> one **batch commit** (`git add -A && git commit --no-gpg-sign`),
     subject = `batch: <N fixes> — <short theme>`, body enumerating each fix + its
     gate. `last_green_sha = HEAD`.
   - **Red** -> identify the culprit fix (revert candidates one at a time from the
     staged batch, re-gate), repair or DROP that one fix (log it DEFERRED), re-gate
     until green, then commit the surviving batch. NEVER commit red; NEVER weaken a
     gate to pass.
   - **Empty loop** (no FIX findings) -> no commit.
8. **Convergence + continue.** Empty loop -> `consecutive_empty_loops += 1`; any
   batch commit -> reset to 0. `>= stop_after_empty_cycles (2)` -> **Stop reached**
   (`stopped_by: converged`). Else `now >= deadline_epoch` -> **Stop reached**
   (`stopped_by: deadline`); otherwise continue to the next loop.

Invariant: the branch is `verify_cmd`-green after every committed loop; a red batch
never lands.

## Finders

All three run every loop, in parallel, cold/fresh, model=opus effort=xhigh,
SOURCE-READ-ONLY (report only). Each returns findings per `## Finding schema`. Each
is told the do-not-re-report ledger.

### interaction (read-only finder)
- hunts: bugs that live in user-sequence INTERACTIONS — drill in/out, tab-switch
  mid-highlight, comment then flip then revert, delete/rename a file underneath the
  editor, restart mid-anything, md_view × switches, selection-vs-poll drift. The
  prior run's two real bugs were here.
- method: pick a cluster of plausible sequences; write ONE throwaway probe in a
  scratch dir (unique SOCK, tui-lib.sh, human-cadence event-waits, `--poll 100` to
  force poll races); state expected outcome per step BEFORE running; where actual
  diverges, that's a finding — capture the exact repro steps, expected vs actual,
  and the file/function you believe is responsible (from reading the code). Confirm
  live before reporting `confidence: confirmed`.
- must not: edit tracked source; report a cosmetic-only nit as high severity;
  re-report a ledger item.

### concurrency (read-only finder)
- hunts: ordering / race bugs across the distributed surface — host event loop,
  nvim RPC notifications, the 500 ms poll, agent writes underneath. Subsystems:
  notification pipeline (intents vs sync/goto/place_last/pending_input, dedup
  early-returns, stale-target guards); poll vs input (checktime storms, entries
  rebuild racing selection/reveal, per-tab stash swaps mid-action); live-sync
  (save_live vs agent write vs revert vs undo, FileChangedShell branches); editor
  lifecycle (death/respawn with pending input, restart racing first open, resize
  during sync); persistence+clipboard (comment-ref write timing, two-pane, OSC52).
- method: read the actual code paths, enumerate interleavings adversarially; for
  each suspected window ATTEMPT A LIVE REPRO (throwaway probe, `--poll 100`, rapid
  keys, concurrent file writes). Report confirmed races with the exact interleaving;
  report a suspected-unreproved window as `confidence: suspected` with the argument.
- must not: propose a lock/sleep as the fix (note it, but the design fix is
  ordering/event/guard); edit tracked source; re-report a ledger item.

### correctness-edge (read-only finder)
- hunts: (a) STATIC correctness bugs that need no live repro — logic errors,
  off-by-one, wrong/over-broad guards, an unhandled Result/Option, an unreachable or
  missing match arm, index math that can panic/wrap, state that desyncs across the
  tab-stash swap; AND (b) HOSTILE-INPUT edge cases — empty repo/detached HEAD, very
  long lines, binary/0-byte, huge diffs, symlinks, rename-onto-deleted, files
  vanishing mid-review — that could paint wrong, hang, or corrupt state.
- method: for (a) read the state-heavy modules (`src/app.rs`, `src/lib.rs`,
  `src/nvim/mod.rs`, the lua) and adversarially verify each candidate — state the
  concrete input that yields the wrong output; only report if it holds. For (b)
  build a hostile fixture and drive the affected feature live (throwaway probe).
- must not: report style as a bug (that's a tidy, note it separately with low
  severity); "fix" by proposing a feature; edit tracked source; re-report a ledger
  item.

## Finding schema

Each finder returns a JSON array; each finding:
```
{
  "title":       "one-line summary",
  "finder":      "interaction" | "concurrency" | "correctness-edge",
  "area":        "path/to/file.rs:LINE or module::fn",
  "class":       "short-kebab (e.g. stale-paint, ordering, off-by-one, edge-binary)",
  "severity":    "high" | "medium" | "low",
  "confidence":  "confirmed" | "suspected",   // confirmed = reproduced live this loop
  "repro":       "exact steps or probe description that triggers it",
  "expected":    "what should happen",
  "actual":      "what actually happens",
  "worth_fixing":true,                          // finder's recommendation
  "fix_sketch":  "suggested minimal root-cause fix (optional)"
}
```

## Triage rubric (orchestrator)

Decide FIX / DEFER / REJECT for each deduped finding:
- **REJECT** if: out-of-scope path; not-a-bug (intended behaviour or documented
  graceful degradation); cosmetic-only that the user has scoped out; a
  behaviour-change / feature-request dressed as a bug; on the ledger already.
- **DEFER** (log, don't fix now) if: real but the only fix is behaviour-affecting or
  an architectural reshape (e.g. two-pane comment CAS) — these need the user; or
  `suspected` + low-severity + not reproducible.
- **FIX** if: (`confirmed`) OR (`suspected` AND high-severity AND the reasoning is
  airtight and the fix strictly tightens), AND the fix is behaviour-preserving or a
  clear bug correction, AND in-scope. Prefer confirmed. Corroboration by 2 finders
  raises priority. When unsure whether a change preserves behaviour, DEFER — never
  ship an uncertain behaviour change to look productive.

## Termination

- `default_duration` (passed at launch, e.g. `3h`) wall-clock budget; 15 m build cap.
- Converged = 2 consecutive loops with zero FIX findings -> stop early.
- **Finalize — DEVIATION (user's standing rules: no squash, no sign, no PR; push to
  the FORK only):**
  1. Ensure tree clean + `verify_cmd` green (a committed loop already leaves it so).
  2. `git switch base-select-v060 && git merge --no-ff loop-build/nvim-parallel-hunt
     -m "merge loop-build/nvim-parallel-hunt: parallel bug-hunt"`.
  3. Deploy: `cargo build --release`;
     `DEST=/home/michael/.config/herdr/plugins/github/persiyanov.reviewr-e87b654a74f8`;
     `rm -f "$DEST/bin/herdr-reviewr" && cp target/release/herdr-reviewr "$DEST/bin/herdr-reviewr"`
     (rm first — the running pane holds the binary, ETXTBSY); `rsync -a --delete
     nvim/ "$DEST/nvim/"`; verify md5s match.
  4. `git push fork base-select-v060` (remote `fork` = Fletch153/herdr-reviewr;
     NEVER `origin` = persiyanov upstream).
  5. PushNotification: loops run, findings raised vs fixed vs deferred/rejected,
     bugs fixed (one-liners), gates added, "restart the reviewer pane".
  6. `phase = done`.
- Draft path (baseline never green — near-impossible): stop, notify, merge nothing,
  deploy nothing, push nothing.

## Scope

- in-scope: `src/`, `nvim/`, `scripts/`, `tests/`, `artifacts/audit-matrix.md`,
  `README.md` (doc only).
- out-of-scope: `Cargo.toml` runtime deps, `.github/`, the deployed plugin dir
  (finalize only), `origin`, the stash list, any architectural reshape not demanded
  by a confirmed bug (behaviour-affecting fixes are DEFERRED to the user).

## Resources & context

**Already fixed / gated this session — do NOT re-report (ledger seeds):** per-tab
reviewed ticks; Uncommitted base follows HEAD; poll 500 ms; All-files walk steps
every file (editor + file-pane) skipping dir placeholders; deleted files stay
tickable; All files lists worktree ∪ changeset so a staged deletion stays visible;
save_live no longer resurrects a file deleted underneath; stale-paint on
delete-underneath a Changes file (last_existed dedup); markdown scroll now per-tab;
rename/revert-race/extremes/symlink/rename-edge all characterization-gated. Gates
(44): tui-del, tui-resurrect, tui-allwalk, tui-reviewtick, tui-renamefile,
tui-revrace, tui-extremes, tui-delflip, tui-mdscroll, tui-symlink, tui-rename2, +
the base suite.

**Architecture map**: `src/lib.rs` event loop + key/mouse routing + NvimSession
(now with `last_existed`) + notification dispatch + nvim_sync; `src/app.rs` App
state (tabs, per-tab stash incl. `preview_scroll`, entries, walk, revert-advance,
persist, md_view, reviewed prune-by-hash); `src/nvim/mod.rs` engine + switch
payloads (`silent! call luaeval('…save_live()') | …`); `src/export.rs`
clipboard+OSC52; `nvim/lua/reviewr/diff.lua` paint/lock/walk/revert/views;
`live.lua` sync policy + save_live; `comments.lua` anchors/cards/intents;
`plugin/reviewr.lua` maps + clipboard provider. Tests: `tests/*.rs`,
`nvim/tests/run.lua` (~130 checks), `scripts/tui-*-test.sh` (44 gates via
`scripts/tui-all.sh`).

**Feature inventory + fragile spots**: see `artifacts/audit-matrix.md` (kept as a
coverage reference, not a cursor worklist). Commit bodies in this repo are unusually
informative — `git log --oneline fork/base-select-v060..HEAD` narrates every
feature and its caveats.
