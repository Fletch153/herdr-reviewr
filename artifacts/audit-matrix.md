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
| 1 | Embed lifecycle (spawn, dead panel, respawn, `:q`, quit confirm, theme) | open (death on non-Changes tab) | covered: tui-death | covered: tui-death (theme, decorations) | open (death with pending input / mid-flip; restart racing first open) | n/a |
| 2 | Diff paint (signs, line paint, virt_lines, statuscolumn, breakindent) | covered: tui-test | n/a | covered: tui-wrap (wrap gap), tui-test (folds+signs) | open (paint after rapid file switches) | covered: tui-unicode, tui-eol; open (huge diff, very long lines) |
| 3 | Context folds (foldexpr/foldtext, zx on scope change) | covered: tui-test | n/a | covered: tui-test | covered: tui-scope (re-diff recompute) | open (folds on huge diff) |
| 4 | View model (plain stamping, BufEnter re-derive, deleted scratch, rename map) | covered: tui-rename | covered: tui-test | covered: tui-test (deleted scratch) | open (unusual entry paths: jumplist/Ctrl-o, `:e other`, tags) | covered: tui-rename; open (rename onto deleted, case-only rename) |
| 5 | Read-only Changes (lock, insert/paste flip, pending input, rd lift) | covered: tui-lock (flip lands All files) | covered: tui-lock | covered: tui-undo (lock inert), tui-split (rd lift) | probed: c1.p1 (double-tap flip — guard added, gate tui-storm step 4); open (paste mid-restart; pending input surviving restart) | open (flip on deleted/renamed file) |
| 6 | Review walk + reviewed ticks (Enter/BS, files-pane Enter, wrap, persistence) | covered: tui-nav (All-files walk, cross-tab ticks) | covered: tui-nav (files-pane Enter) | covered: tui-nav | probed: c1.p1 (BUG found+fixed — stale nav; gate tui-storm covers Enter/BS storms, walk+revert interleave, walk across rebuild) | covered: tui-persist (content change clears tick); open (tick on renamed file) |
| 7 | Hunk revert (`space rh`, shapes, EOL flip, last-hunk advance) | covered: tui-revert (last-hunk advance) | n/a | covered: tui-revert (through lock) | open (revert racing agent write / poll refresh) | covered: run.lua (all shapes, added-file refusal), tui-eol; open (revert on rename) |
| 8 | Comments (rc/re/rx/rr/rl/rs/ry, cards, composer, jump) | covered: tui-scope (pin to scope+base) | covered: tui-mouse (card click) | covered: tui-edit (compose/edit/sent guard) | open (comment→flip→revert sequences; anchors surviving revert/edit) | covered: tui-unicode (composer) |
| 9 | Live sync (autosave, checktime, FileChangedShell policy, conflict) | covered: tui-live | n/a | covered: tui-live | covered: run.lua (conflict, user wins); open (same-second agent write → poll — known nvim quirk; quit during pending writes) | open (agent deletes open file mid-edit) |
| 10 | Comment persistence (comments ref, seed, empty delete, rev-guarded) | covered: tui-persist | n/a | n/a | open (quit racing pending write; two panes one repo) | n/a |
| 11 | Markdown view (sticky md_view, chip, `p`, scroll routing) | open (md_view held across tab switches) | covered: tui-md (files focus stays live) | covered: tui-md (sticky, chip labels, non-md passthrough) | open (md_view during restart/death) | open (huge md, md with unicode) |
| 12 | Clipboard (provider→OSC52, cache pastes, host export fallback) | n/a | n/a | n/a | open (OSC52 mid-frame interleave; rapid yank storm) | covered: tui-clip; run.lua (linewise trailing \n) |
| 13 | Ctrl+i return, Tab focus toggle, 1/2/3, per-tab stash | covered: tui-lock 2c (ctrl+i), tui-test (1/2/3) | covered: tui-test (Tab toggle) | n/a | open (stash swap mid-action; tab switch mid-highlight/mid-compose) | n/a |
| 14 | Scope/base (b/t/C, pickers, re-diff in place, rename push) | covered: tui-scope | covered: tui-picker | covered: tui-scope | open (scope flip racing poll) | covered: tui-rename |
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
2. **5×timing** — pending input surviving restart; paste mid-restart. (double-fire + flip-vs-sync DONE c1.p1, gate tui-storm step 4.)
3. **9×timing** — same-second agent write missed by checktime (known nvim quirk — confirm product exposure); quit during pending writes.
4. **13×timing** — per-tab stash swap mid-action (compose, filter, md_view); tab switch mid-anything.
5. **10×timing** — quit racing the rev-guarded persist write; two panes on one repo.
6. **8×timing** — comment → flip → revert sequences; anchors surviving revert and agent edits.
7. **11×tab / 11×timing** — md_view held across tab switches, restart, death.
8. **4×timing** — lock/paint after jumplist, Ctrl-o, `:e`, tags entry paths.
9. **1×timing** — editor death with pending input; restart racing first open.
10. **7×timing** — revert racing agent write / poll refresh.
11. **file-state batch A** — rename onto deleted, case-only rename, tick/revert/flip on renamed files (4, 6, 7 × file-state).
12. **file-state batch B** — huge diff (5k lines), very long lines, huge md (2, 3, 11 × file-state).
13. **file-state batch C** — empty repo / zero commits / detached HEAD; agent deletes open file; CRLF (9, 15 × file-state).
14. **16×timing** — drag during repaint; narrow terminal (120×30) sweep.
15. **12×timing** — OSC52 interleaving; yank storm.

## Log

- 2026-07-07 build: matrix seeded; 20 gates + run.lua mapped; 24 open cells across 15 ranked entries.
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
