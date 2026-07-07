# Audit matrix — embedded-nvim review mode

Worklist + coverage ledger for the nvim-mode-audit hardening loop. One row per
feature (inventory from `artifacts/nvim-mode-audit.loop.md` → Resources); one
column per interaction dimension. Cell values:

- `covered: <gate>` — an existing gate in `scripts/tui-all.sh` (or `nvim/tests/run.lua`)
  genuinely exercises this feature along this dimension.
- `probed: <pass>` — probed by this loop (cycle.pass noted); gate promoted if a bug fell out.
- `open` — nobody has driven it; risk-ranked worklist below.
- `n/a` — dimension doesn't meaningfully apply.

Dimensions: **tab** (Changes/All files/PR + switches mid-state) · **focus**
(files pane ⇄ editor, Tab toggle) · **view-state** (md_view, focused/plain,
folds, lock, dead panel) · **timing** (poll ticks, restarts, agent writes,
rapid input, interleavings) · **file-state** (renames, EOL, unicode, binary,
empty, huge, vanishing).

| # | Feature | tab | focus | view-state | timing | file-state |
|---|---------|-----|-------|------------|--------|------------|
| 1 | Embed lifecycle (spawn, dead panel, respawn, `:q`, quit confirm, theme) | open (death on non-Changes tab) | covered: tui-death | covered: tui-death (theme, decorations) | probed: c1.p2 (BUG found+fixed — respawn consumed pending work against stale view memory; gate tui-death step 4); probed: c1.p4 (orphaned embeds — see Log; harness fixed, tui_cleanup reaps loudly; residual: host death by signal can strand a wedged nvim, in-process fix out of scope); open (restart racing first open) | n/a |
| 2 | Diff paint (signs, line paint, virt_lines, statuscolumn, breakindent) | covered: tui-test | n/a | covered: tui-wrap (wrap gap), tui-test (folds+signs) | open (paint after rapid file switches) | covered: tui-unicode, tui-eol; open (huge diff, very long lines) |
| 3 | Context folds (foldexpr/foldtext, zx on scope change) | covered: tui-test | n/a | covered: tui-test | covered: tui-scope (re-diff recompute) | open (folds on huge diff) |
| 4 | View model (plain stamping, BufEnter re-derive, deleted scratch, rename map) | covered: tui-rename | covered: tui-test | covered: tui-test (deleted scratch) | open (unusual entry paths: jumplist/Ctrl-o, `:e other`, tags) | covered: tui-rename; open (rename onto deleted, case-only rename) |
| 5 | Read-only Changes (lock, insert/paste flip, pending input, rd lift) | covered: tui-lock (flip lands All files) | covered: tui-lock | covered: tui-undo (lock inert), tui-split (rd lift) | probed: c1.p1 (double-tap flip — guard added, gate tui-storm step 4); c1.p3 (BUG found+fixed — dead-editor paste silently swallowed, now honest status; gate tui-death step 3c. Pending input cannot outlive its frame: set only while alive, fired same-frame or delivered via the respawn branch c1.p2 resets — code-walked, no live window) | open (flip on deleted/renamed file) |
| 6 | Review walk + reviewed ticks (Enter/BS, files-pane Enter, wrap, persistence) | covered: tui-nav (All-files walk, cross-tab ticks) | covered: tui-nav (files-pane Enter) | covered: tui-nav | probed: c1.p1 (BUG found+fixed — stale nav; gate tui-storm covers Enter/BS storms, walk+revert interleave, walk across rebuild); c1.p2 (BUG found+fixed — insert-flip vs boundary nav; navs now carry their view, gate tui-storm step 6) | covered: tui-persist (content change clears tick); open (tick on renamed file) |
| 7 | Hunk revert (`space rh`, shapes, EOL flip, last-hunk advance) | covered: tui-revert (last-hunk advance) | n/a | covered: tui-revert (through lock) | open (revert racing agent write / poll refresh) | covered: run.lua (all shapes, added-file refusal), tui-eol; open (revert on rename) |
| 8 | Comments (rc/re/rx/rr/rl/rs/ry, cards, composer, jump) | covered: tui-scope (pin to scope+base) | covered: tui-mouse (card click) | covered: tui-edit (compose/edit/sent guard) | open (comment→flip→revert sequences; anchors surviving revert/edit) | covered: tui-unicode (composer) |
| 9 | Live sync (autosave, checktime, FileChangedShell policy, conflict) | covered: tui-live | n/a | covered: tui-live | covered: run.lua (conflict, user wins); probed: c1.p3 (same-wall-clock-second write SAFE — nvim compares mtime nanoseconds; BUG found+fixed — nvim never compares size, so an mtime-preserving write (cp -p/rsync -t) was invisible forever → live.poll size check, gates tui-live 4a/4b; quit mid-insert flushes to disk, gate tui-live step 5) | open (agent deletes open file mid-edit) |
| 10 | Comment persistence (comments ref, seed, empty delete, rev-guarded) | covered: tui-persist | n/a | n/a | open (quit racing pending write; two panes one repo) | n/a |
| 11 | Markdown view (sticky md_view, chip, `p`, scroll routing) | open (md_view held across tab switches) | covered: tui-md (files focus stays live) | covered: tui-md (sticky, chip labels, non-md passthrough) | open (md_view during restart/death) | open (huge md, md with unicode) |
| 12 | Clipboard (provider→OSC52, cache pastes, host export fallback) | n/a | n/a | n/a | open (OSC52 mid-frame interleave; rapid yank storm) | covered: tui-clip; run.lua (linewise trailing \n) |
| 13 | Ctrl+i return, Tab focus toggle, 1/2/3, per-tab stash | covered: tui-lock 2c (ctrl+i), tui-test (1/2/3) | covered: tui-test (Tab toggle) | n/a | open (stash swap mid-action; tab switch mid-highlight/mid-compose) | probed: c1.p5 (USER-REPORT BUG found+fixed — returning to an empty Changes kept the All-files buffer up; editor now parks on the reviewr://empty scratch, gate tui-empty) |
| 14 | Scope/base (b/t/C, pickers, re-diff in place, rename push) | covered: tui-scope | covered: tui-picker | covered: tui-scope | open (scope flip racing poll) | covered: tui-rename; probed: c1.p5 (zero-commit repo: untracked file diffs against the empty tree, both tabs render; detached HEAD: clean tree shows the empty state, live edit re-lists — both pass, no bug) |
| 15 | EOL (nofixendofline, note+sign, eol revert, byte-exact base) | covered: tui-eol | n/a | covered: tui-eol | n/a | covered: tui-eol, run.lua; open (CRLF content) |
| 16 | Host UI interop (mouse routing, divider, resize, filter, help, chips) | covered: tui-mouse, tui-md (chip) | covered: tui-mouse | covered: tui-split (divider), tui-picker (filter/resize) | open (drag during repaint; click storm; narrow terminal) | covered: tui-trio (backspace delete) |

Gate scripts not cited above still count toward coverage of their primary rows:
tui-edit (8), tui-trio (8, 16), tui-undo (5), tui-picker (14, 16), tui-death (1),
tui-wrap (2), tui-test (2, 3, 4, 13).

## Open cells, ranked by risk

State-carrying features × timing rank highest (that's where every past live bug
lived). The scenario-matrix lens works top-down, 3–5 cells per pass, preferring
clusters that share a fixture.

1. ~~**6×timing**~~ — DONE c1.p1 (gate tui-storm): Enter/BS storms at boundaries, walk+revert interleave, walk across poll entries-rebuild.
2. ~~**5×timing**~~ — DONE c1.p3 (gate tui-death step 3c): dead-editor paste now honestly dropped with a status; pending-input-across-restart proven frame-local by code walk (no live window survives c1.p2's dedup reset — only a failed respawn strands it, and it then fires into the manually-restarted plain view, judged acceptable).
3. ~~**9×timing**~~ — DONE c1.p3 (gates tui-live 4a/4b/5): natural same-second writes are safe (nvim compares mtime nsec); the REAL shadow was mtime-exact writes — nvim never compares size, fixed with live.poll's size check. Residual (documented, unfixed): mtime-exact + byte-identical-length content swap stays invisible (needs per-tick hashing, not warranted). Quit mid-insert flushes via the forced wall!.
4. **13×timing** — per-tab stash swap mid-action (compose, filter, md_view); tab switch mid-anything.
5. **10×timing** — quit racing the rev-guarded persist write; two panes on one repo.
6. **8×timing** — comment → flip → revert sequences; anchors surviving revert and agent edits.
7. **11×tab / 11×timing** — md_view held across tab switches, restart, death.
8. **4×timing** — lock/paint after jumplist, Ctrl-o, `:e`, tags entry paths.
9. ~~**1×timing** (death with pending work)~~ — DONE c1.p2 (gate tui-death step 4: external kill + comment jump); still open: restart racing first open.
10. **7×timing** — revert racing agent write / poll refresh.
11. **file-state batch A** — rename onto deleted, case-only rename, tick/revert/flip on renamed files (4, 6, 7 × file-state).
12. **file-state batch B** — huge diff (5k lines), very long lines, huge md (2, 3, 11 × file-state).
13. **file-state batch C** — ~~empty repo / zero commits / detached HEAD~~ DONE c1.p5 (both pass live, no bug; the adjacent USER-REPORT empty-changeset bug fixed + gate tui-empty); still open: agent deletes open file; CRLF (9, 15 × file-state).
14. **16×timing** — drag during repaint; narrow terminal (120×30) sweep (c1.p4 ran the full
    tui-test flow at 120×30: all 19 assertions pass, no layout bug — drag-during-repaint and
    the other gates at narrow size still open).
15. **12×timing** — OSC52 interleaving; yank storm.

## Log

- 2026-07-07 c1.p5 (edge-hardening, class 1 + USER REPORT): the live user report (empty
  changeset: Changes → All files opens a file → back to Changes keeps that file up) reproduced
  live on first try. Root cause: nvim_sync's no-selection branch only did a plain re-present
  (and only when last_focus wasn't already plain) — returning from All files, last_focus was
  Some(false), so nothing was sent at all and the previous buffer stayed. By design the Changes
  pane presents the changeset, so an empty changeset must not show a file: added a reusable
  reviewr://empty scratch (diff.lua show_empty, same nameless-nofile shape as the deleted
  scratch, unlisted, says "no changes in scope") and a show_empty engine command that autosaves
  the leaving buffer first; the host parks on it whenever Changes has no selection — first
  entry, tab return, or the last change reverting away mid-session — guarded by a new
  NvimSession.parked_empty flag, and clears the published-view memory so the next real open
  republishes fully (src/lib.rs, src/nvim/mod.rs). Permanent gate scripts/tui-empty-test.sh
  (first-entry greeter, round-trip re-assert, content-marker absence, second round trip proving
  the dedup reset) wired into tui-all.sh, green twice with clean quits. Class-1 extras probed
  live (throwaway script, deleted): zero-commit repo with one untracked file — both tabs
  render, the file diffs against the empty tree, no crash; detached HEAD — clean tree shows
  the empty state, All files opens worktree content, a live edit re-lists in Changes. Both
  pass, no product bug. cargo tests + lua suite green; fmt/clippy clean
  (struct_excessive_bools on NvimSession allowed, same as App).
- 2026-07-07 c1.p4 (flake-hunt): 2 full tui-all sweeps + 2 lua sweeps — result lines byte-identical,
  all green. But the suite was silently leaking WEDGED `nvim --embed` orphans (~1 per few sweeps;
  three found alive, one 6h old — parked in ep_poll, RPC-dead, stdio re-pointed to /dev/null,
  immune to HUP/TERM). Root cause chain: three gates' final quits were FAKE — persist and eol
  Tab'd INTO the editor before q (q = macro record), scope pressed q with the editor focused
  after the list jump — so the reviewer never quit, the EXIT trap's `tmux kill-server` SIGHUP'd
  the live host (reap never ran; host has no signal handler by design — unsafe_code=forbid, no
  libc), and nvim occasionally deadlocks inside its own HUP teardown (nvim-internal; bare
  EOF/HUP exits fine 15/15, only state-laden teardowns wedge). Fixes (harness at root): the 3
  fake quits now real (focus-correct q); tui-rename gained its missing quit; tui-persist's
  restart sleeps → wait_session_end (a lingering session also makes -t0 ambiguous); tui-lib's
  trap → tui_cleanup (waits for an in-flight quit so the host's reap SIGKILLs nvim, then
  kill-server, then loudly reaps any embed still cwd'd in the gate fixture). Verified: persist
  6× green no-leak (was 1-in-6), two post-fix full sweeps green with embeds=0 after every gate.
  PRODUCT residual (documented, out of scope here): host death by signal skips reap entirely —
  a real pane close can strand a wedged nvim; a proper fix needs a SIGHUP/SIGTERM handler or
  PDEATHSIG at spawn, both blocked by unsafe_code=forbid + no-new-crates. Maintainer call.
  Also: tui-test at 120×30 — 19/19 pass, no geometry assumptions in the representative gate.
- 2026-07-07 build: matrix seeded; 20 gates + run.lua mapped; 24 open cells across 15 ranked entries.
- 2026-07-07 c1.p3 (scenario-matrix d2): probed ranked 2 remainder + ranked 3 (ranked 4 untouched —
  budget went to two product bugs). (a) 9×timing: pinned-mtime probes proved nvim's checktime
  compares mtime sec+nsec+mode but NEVER size — natural same-second writes reload fine (nsec
  drift), refining the run_notes quirk to exact-(sec,nsec) aliasing (utime-pinned fixtures collide
  at nsec=0; wall-clock writes don't); the real exposure was mtime-preserving writes (cp -p /
  rsync -t restores) which stayed invisible forever, size change included. Fix: live.lua stamps
  each buffer's disk stat at nvim's sync points (BufReadPost/BufWritePost/FileChangedShellPost)
  and a new live.poll() (host poll now calls it instead of raw checktime) reloads on
  same-mtime/different-size using the existing FileChangedShell policy; mtime-exact same-size
  content swaps remain invisible by design (hashing not warranted). Gates tui-live 4a/4b.
  (b) 5×timing: a paste aimed at a dead editor fell through to input_paste and vanished silently
  (no status, no dead-panel hint it was consumed) — now answers "editor is not running — paste
  dropped (r restarts)" (src/lib.rs paste arm); gate tui-death step 3c (kill, paste, status,
  nothing on disk, nothing resurfacing after r). Pending-input-across-restart: code-walked as
  frame-local (see ranked 2). (c) quit-during-pending-writes: PASS live — quit's forced wall!
  lands a mid-insert-mode edit on disk before the pane closes; gate tui-live step 5. 360 cargo
  tests + lua suite green; fmt/clippy clean; tui-live and tui-death green individually.
- 2026-07-07 c1.p2 (race-audit, subsystem 1: notification pipeline): enumerated intent-vs-sync
  interleavings; TWO races reproduced deterministically and fixed. (a) insert+Enter in one
  editor batch: the boundary nav emitted by the still-locked buffer landed after the flip put
  the tab on All files, so the walk advanced and the fired insert key landed in the WRONG file
  — navs now carry the emitting buffer's view ("focused"/"plain"; plugin/reviewr.lua, diff.lua
  revert path) and the host drops view↔tab mismatches (src/lib.rs); gate tui-storm step 6.
  (b) nvim_sync computed same_view/same_path/same_cards BEFORE the auto-respawn cleared the
  session's view memory, so the work item that triggered the respawn (comment-jump goto,
  place-last, pending input) fired into the fresh editor's empty [No Name] buffer and was
  consumed while the file only opened a frame later — the respawn now resets all three dedup
  flags (src/lib.rs); gate tui-death step 4 (external kill + jump into a taller-than-screen
  file). Blind-tightened (unreproducible in isolation, reasoning airtight): nvim_goto now
  carries its file and is dropped if the shown diff moved between the click and the sync
  (src/app.rs, src/lib.rs). run.lua nav checks assert the view field. 360 cargo tests + lua
  suite green; storm/death/nav/revert/lock gates green individually.
- 2026-07-07 c1.p1 (scenario-matrix): probed 6×timing + 5×timing storms. BUG (reproduced
  deterministically, fixed): the walk's `nav` boundary intent carried no file, so an Enter
  storm at a file boundary marked the ENTIRE changeset reviewed ("all files reviewed" after
  3 fast Enters) — every stale boundary verdict advanced the host again. Fix: nav payload
  now carries the buffer's file (plugin/reviewr.lua, diff.lua revert path); the host drops
  Changes-tab navs whose file ≠ diff_path (src/lib.rs) — All-files navs stay
  host-authoritative. Also hardened: a repeat insert tap after the flip published can no
  longer double-fire a literal key into the unlocked buffer (lib.rs insert arm). New
  permanent gate scripts/tui-storm-test.sh (5 steps) wired into tui-all.sh; run.lua nav
  checks now assert the file field. cargo test 360 green, lua suite green, storm/nav/lock/
  revert gates green individually.
