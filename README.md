# herdr-reviewr

A code-review sidebar for [herdr](https://herdr.dev). Your agent writes the code; you read its
diff in a pane beside the chat, leave comments on the lines, and send the notes back — without
leaving the terminal.

![demo](assets/demo.gif)

What you get, in one persistent pane pointed at a git worktree:

- **A diff to review** — the agent's changed files, syntax-highlighted, scoped to a *commit*
  (defaulting to the tip — the uncommitted view), a *branch*, or the *last turn*.
- **Line comments that stay put** — select a range, write a note; it renders as a card under the
  code instead of hiding behind a marker.
- **One keystroke back to the agent** — **Send** drops every comment into the agent's input as
  `path:start-end — comment`, ready for you to add context and hit enter.
- **More when you need it** — browse the whole worktree, not just the diff, and read the branch's
  open pull request without switching windows.
- **Themed to match your terminal** — 18 named palettes (Catppuccin, Dracula, Nord, Gruvbox,
  Tokyo Night, Rosé Pine, Solarized, and more, in dark and light), one config line away.

It **never edits your worktree** and sends nothing on its own. Its only write to git is a private
`last-turn` baseline ref under `refs/reviewr/`. The **PR** tab reads GitHub but never posts there.

## Requirements

- **herdr ≥ 0.7.0** (the plugin system).
- **git** on `PATH`.
- A **truecolor (24-bit)** terminal with Unicode box-drawing support; a light or dark theme to
  match it (see [Theme](#theme)).
- **macOS or Linux.**
- **`gh`** (the GitHub CLI), authenticated — *optional*, only for the **PR** tab. Everything else
  works without it.

## Install

From the herdr marketplace — a prebuilt binary, no Rust toolchain:

```bash
herdr plugin install persiyanov/herdr-reviewr
```

The sidebar **auto-opens for a newly created worktree** — installing the plugin is enough. To
toggle it on demand, bind a key to the **reviewr: toggle sidebar** action in your herdr config
(keybindings live in user config, not the plugin manifest):

```toml
[[keys.command]]
key = "cmd+r"
type = "plugin_action"
command = "persiyanov.reviewr.toggle"   # <plugin_id>.<action_id> — note the id, not the name
```

`cmd+…` chords reach herdr; macOS swallows `alt+…`. With no key bound, run it once with
`herdr plugin action invoke toggle --plugin persiyanov.reviewr`.

## Quick start

The core loop takes five keys. Open the sidebar next to your agent and:

1. **Pick a file.** The agent's changed files are in the right pane. `j` / `k` moves the cursor;
   the diff opens on the left as you go.
2. **Focus the diff.** Press `Tab` to move from the file list into the diff.
3. **Select the lines.** Press `v`, then `j` / `k` to extend the selection (or click-drag).
4. **Comment.** Press `c`, type your note, `Enter` to save. It stays on screen as a card under
   the line.
5. **Send.** When you're done, press `s`. Every comment lands in the agent's input as
   `path:start-end — comment` — you add context and send.

The footer always shows the keys that work right now, so you can learn it by using it. The tables
below are the full reference.

## Controls

**Getting around**

| Key | Action |
| --- | --- |
| `1` `2` `3` | Switch tab — Changes / All files / PR |
| `b` `t` | Switch scope — branch / last turn |
| `C` | Compare against a commit — defaults to the tip (the working-tree / uncommitted view, shown as `[commit] [uncommitted]`); click the commit chip to pick an older commit and diff the worktree against it |
| base chip | Click the base chip to pick the branch-scope diff base from this checkout's fork lineage — ancestors of `HEAD`, nearest first, local then `origin/*` (chip-click only, like the commit picker) |
| `p` | On a markdown file, toggle a rendered preview (headings, tables, lists) instead of the diff; `p` / `esc` returns to the diff to comment |
| `+` | Send the highlighted file's `@path` into the agent's chat input (numpad or top-row `+`), ready to submit |
| `space` | Walk the review. In the diff (Changes tab), step to the next change block; past the last block the file is marked reviewed (dims + `✓`) and you land on the next unreviewed file's first block. On the file list it marks the whole file reviewed and jumps to the next. The mark clears if the agent edits that file again; the header shows `N changed · M reviewed` |
| `j` `k` · `↑` `↓` | Move the cursor in the focused pane |
| `PageUp` `PageDown` | Move a page · `Ctrl+U` `Ctrl+D` move a half-page |
| `Tab` | Switch focus between the file list and the diff |
| `→` `←` | Expand / collapse a directory or expand a fold; otherwise scroll the diff sideways |
| `w` | Toggle line wrap |
| `/` | Filter the file tree by name (type to filter, `esc` clears, `enter` keeps it) |
| `.` | Reveal by extension — type an extension then `enter` to expand every folder holding a file of that type (e.g. `.rs` shows every Rust file); an empty `.` `enter` collapses those reveals back. `esc` cancels |
| `]` `[` | Widen / narrow the file list |
| `⌫` Backspace | Delete the file or folder under the cursor from the working tree — asks to confirm first (`y`/`enter` deletes, `n`/`esc` cancels) |
| `?` | Show every keybinding in an overlay (`j`/`k` scroll, `esc` closes) |
| `r` | Refresh now |
| `q` | Quit |

**Reviewing** (in the diff)

| Key | Action |
| --- | --- |
| `v` | Start a line selection, then `j` / `k` to extend (or click-drag) |
| `c` | Comment on the selection — or on the current line |
| `e` | Open the file in `$EDITOR` — or edit the comment under the cursor, if any |
| `r` | Resolve (remove) the comment under the cursor — drops it from the list |
| `d` | Delete the comment under the cursor |
| `n` `N` | Jump to the next / previous comment |
| `l` | List every comment, grouped by view + base (`── commit … ──` / `── branch … ──` / `── All files ──`); fresh (un-sent) comments show in green. `space` checks a row, `a` checks all, `r` resolves the checked set (or the cursor row), `enter`/click jumps to a comment — switching to its commit/branch first |
| `s` | Send the **un-sent** comments to the agent in a `<review>` block (asks it to resolve each and report a status table); sent comments become resolve-only. A Changes comment carries its `+/-` hunk; an All-files comment carries plain code |
| `y` | Copy all comments to the clipboard (does not mark them sent) |
| `esc` | Clear the selection |

Comments are scoped to the diff they were made against: a comment made while comparing one
commit/branch shows only under that view + base, and cycling commits or branches reveals only
that diff's comments. The `l` list spans all of them and clicking one takes you back to its diff.

**In the comment box**

| Key | Action |
| --- | --- |
| `Enter` | Save · `Esc` cancel |
| `Shift+Enter` · `Alt+Enter` · `Ctrl+J` | Insert a newline |

Plus the usual caret moves: arrows, `Home` / `End`, `Ctrl+A` / `Ctrl+E`, word-jump with
`Alt+b` / `Alt+f`, and `Ctrl+W` / `Ctrl+U` / `Ctrl+K` to delete by word or to the line edge.

**PR tab** (read-only)

| Key | Action |
| --- | --- |
| `j` `k` | Move through checks and comments |
| `PageUp` `PageDown` | Scroll the selected comment |
| `o` | Open the PR in your browser |
| `r` | Refresh |

herdr is mouse-native, so clicking a file, dragging to select lines, clicking a tab or the `Send`
button, and the scroll wheel all work too.

## The three tabs

- **Changes** — the changed files for the active scope, with `+/-` stats; pick a file to read its
  syntax-highlighted diff. This is where you review and comment.
- **All files** — browse the whole worktree tree, not only what changed; the diff pane renders any
  file's current content. Git-ignored paths show too, dimmed — a wholly-ignored directory
  (`target/`, `node_modules/`) is one collapsed row that loads its contents only when you expand
  it. You can comment here as well.
- **PR** — a read-only mirror of the branch's open pull request, read from GitHub via `gh`: its
  state (draft / open / merged / closed, mergeability, unpushed-commit sync), its checks with a
  pass/fail rollup, and its comments (reviews, inline findings, plain comments, newest first, with
  `resolved` / `outdated` markers). `o` opens it in the browser. It only reads GitHub — never
  posts, resolves, re-runs, or merges.

## Diff scopes

- **commit** (the default) — the working tree vs a chosen commit. It opens at the **tip** (`HEAD`),
  which is the working-tree / uncommitted view (staged, unstaged, and untracked) — shown in the
  header as `[commit] [uncommitted]`. Click the commit chip to pick an older commit and compare the
  working tree against it.
- **branch** — the working tree vs the merge-base with the base branch: `--base` (or the config
  `base` key) if set, else the **nearest ancestor branch** — the branch this one forked from — so
  the default diff is this branch's own work, not everything inherited from mainline; else the
  repository trunk (`origin/HEAD`, e.g. `origin/develop`, then `origin/main` → `origin/master` →
  `main` → `master`) when nothing forks below `HEAD`. A superset of the uncommitted view that adds
  the branch's committed work. The header's base chip shows the effective base; click it
  to pick a different base from this checkout's fork lineage — handy for stacked branches. The
  picker lists only ancestors of `HEAD`, nearest fork first, local branches then `origin/*`
  (picking the remote keeps its `origin/` label).
- **last turn** — only what the agent changed since its most recent turn started (see
  [Limitations](#limitations)).

Every scope respects `.gitignore`, so build output never clutters **Changes**. To review a file,
track it in git — an ignored-but-intentional file (a plan, a sample env) belongs in the repo,
where it shows as a change and ages out once committed. **All files** can still browse any ignored
path, dimmed, even untracked ones.

## Configuration

CLI flags on the pane command:

| Flag | Default | Meaning |
| --- | --- | --- |
| `--poll <ms>` | `2000` | worktree poll interval (min `200`) |
| `--base <ref>` | auto | base branch for `branch` scope |
| `--theme <name>` | `catppuccin` | UI + syntax theme (see below) |
| `--wrap <on\|off>` | `on` | soft-wrap long diff lines (`w` toggles at runtime) |
| `--icons <on\|off>` | `off` | Nerd Font file/folder icons in the tree (needs a Nerd Font) |

### Icons

The file tree can show Nerd Font filetype and folder glyphs, colored by type. It's **off by
default** because the glyphs only render on a terminal using a patched Nerd Font (e.g. Ghostty's
built-in fallback, or a Nerd Font in your terminal) — without one they appear as boxes. Enable it
in reviewr's config file (read once at startup; the CLI flag wins):

```toml
# $HERDR_PLUGIN_CONFIG_DIR/config.toml
icons = true
```

### Base

The `branch`-scope base can also be pinned in reviewr's config file (read once at startup;
the CLI flag wins):

```toml
# $HERDR_PLUGIN_CONFIG_DIR/config.toml
base = "origin/develop"
```

### Theme

One theme colors the whole UI — chrome and syntax together. Set it in reviewr's config file
(re-read on refresh, so editing it and refreshing re-themes without relaunch):

```toml
# $HERDR_PLUGIN_CONFIG_DIR/config.toml
theme = "tokyo-night"
```

`--theme` overrides the config file (handy for a dev run). Use a name your terminal's light/dark
matches — a light theme on a dark terminal (or the reverse) reads poorly, since the pane keeps
the terminal's background. Available:

- **Dark:** `catppuccin`, `catppuccin-frappe`, `catppuccin-macchiato`, `dracula`, `nord`,
  `gruvbox`, `one-dark`, `solarized`, `monokai`, `tokyo-night`, `rose-pine`.
- **Light:** `catppuccin-latte`, `gruvbox-light`, `one-light`, `solarized-light`, `github-light`,
  `tokyo-night-day`, `rose-pine-dawn`.

Names match herdr's where both ship a palette. An unknown name falls back to `catppuccin`.

### Editor (embedded nvim)

By default the left pane is a read-only diff view. Set `editor = "nvim"` to replace it with a
**real Neovim editor embedded in the same pane**: reviewr spawns `nvim --embed` as a child
process, hosts its UI in the diff pane's rectangle, and routes your keys and mouse to it. The
file list stays exactly as it is; selecting a file (click or `j`/`k`) opens it in the editor.
Your own nvim config loads — your colorscheme, keymaps and plugins all work.

```toml
# $HERDR_PLUGIN_CONFIG_DIR/config.toml
editor = "nvim"   # default: the built-in read-only diff view
```

How it behaves:

- **Focus**: `Tab` switches between the file list and the editor. Inside the editor, Tab only
  switches back in normal/visual mode — while inserting or on the cmdline it types, and every
  other key goes to nvim (`<C-i>` jumplist etc. work under the kitty keyboard protocol).
- **Review**: the bundled `reviewr.nvim` provides inline red/green vs the base (no gitsigns
  needed). The **Changes view is read-only** — a review surface. Insert-entry keys
  (`i`/`a`/`o`/…) and pastes there flip to All files at the same spot and land as real input;
  every other mutating key answers `E21`. **`Enter`/`Backspace` walk the review**: hunk to
  hunk, then on to the next/previous changed file (backward entries land on the file's last
  hunk); in All files the pair walks file to file, marking each file it leaves. `Enter` on a
  file row in the list marks it reviewed directly, and `]c`/`[c` hop hunks in place. Reviewed
  ticks are a property of the file, not the view — they show in every tab and scope, persist
  across restarts (a private ref, like comments), and drop only when the file's content
  changes (your edit, the agent's, or a branch switch). **`<leader>rh` reverts the hunk under the
  cursor** to the base (buffer and disk, one undo block); reverting a file's last hunk moves
  on and the clean file drops from Changes. Deliberate limits of the flip: macros and `.`
  can't repeat it, and counts/registers (`3i`, `"aI`) don't carry across it.
- **Comments** live in the reviewer itself — one store behind the header's **Send (n)**
  button, the `l` list (jump, edit, resolve, batch-resolve) and the send path: `<leader>rc`
  comments on a line or visual selection (the reviewer's composer opens over the editor),
  `<leader>re` edits an un-sent comment (sent ones are resolve-only), `<leader>rx`/`<leader>rr`
  delete/resolve under the cursor, `<leader>rl`/`<leader>rs`/`<leader>ry` open the list / send
  to the agent (also `s` and the header button) / copy all. Saved comments paint back into the
  editor as inline boxed cards, styled to the reviewer's theme, and **persist across pane
  restarts** (a private ref in the repo — an accidentally closed pane keeps its review).
  `:ReviewrDiff` opens a split diff (`dp`/`do` editable); `:ReviewrDoctor` debugs agent wiring.
- **Live sync**: your edits autosave the moment they exist (per normal-mode change, and on
  leaving insert), agent writes to open files appear on the reviewer's next poll, and a
  write-under-your-edit conflict resolves to your version — your typing is the newest intent.
  Quitting saves everything savable first; the quit confirmation only appears for buffers that
  genuinely can't write. Closing the reviewer (or its herdr pane, however hard) always takes
  the embedded nvim with it — it is a child process, so an orphaned editor is impossible.
- **Markdown**: `p` (or the header chip, which names the view currently showing) toggles a
  markdown file between the built-in rendered view and the raw editor. The choice is sticky:
  while on, every markdown file you select renders, non-markdown files show the editor as
  normal, and the file list stays fully navigable.
- **Clipboard**: `"+`/`"*` yanks inside the embed copy through the host — OSC 52 out the real
  terminal when no clipboard tool is installed (the only mechanism that reaches your actual
  clipboard over SSH). Pastes from outside arrive as terminal pastes; `"+p` pastes what was
  last yanked in the embed and never blocks on a terminal query.
- Not in this mode (the editor owns the pane): per-file comment badges in the tree.

Requires `nvim` on `PATH` — without it reviewr logs a note and keeps the diff view.

### Sidebar placement

By default the toggle opens reviewr as a split to the right of your agent. You can change how it
opens by setting `toggle_placement` in the same config file. reviewr re-reads the file on every
toggle, so a change takes effect the next time you press the key.

```toml
# $HERDR_PLUGIN_CONFIG_DIR/config.toml
toggle_placement = "overlay"   # split | overlay | zoomed | tab   (default: split)
toggle_direction = "down"      # right | down — split only        (default: right)
```

- **`split`** sits next to your agent and leaves the keyboard with it. Set `toggle_direction` to
  put reviewr on the right (the default) or below.
- **`overlay`** covers the whole tab with reviewr and hands it the keyboard. Toggle again to drop
  back to your agent.
- **`zoomed`** fills the tab the same way as overlay and hands reviewr the keyboard.
- **`tab`** opens reviewr in its own tab and hands it the keyboard.

When you create a new worktree, reviewr auto-opens only for `split` and `tab`. With `overlay` or
`zoomed` it stays out of the way until you press the toggle yourself. Any value it does not
recognize falls back to the default.

## Limitations

This is a focused, young tool. The known constraints, honestly:

**Terminal & theme**
- **Truecolor required** — colors are 24-bit RGB with no 256/8-color fallback; basic terminals
  render wrong.
- **Theme must match the terminal** — the pane keeps the terminal's background, so a light theme
  on a dark terminal (or the reverse) reads poorly. There's no auto light/dark detection yet, so
  you set the theme to match by hand.
- **Add / remove are red / green** — no secondary cue for colorblind users yet.
- Unicode box-drawing glyphs are required (no Nerd Font needed).

**Platform**
- **macOS and Linux only** — no Windows.
- **Clipboard export** uses `pbcopy` (macOS) or `wl-copy` / `xclip` / `xsel` (Linux); if none is
  installed it says so and you use **Send** instead. (OSC 52 and Windows are roadmap.)

**herdr coupling**
- **Send** needs a resolvable agent pane — the agent in your tab, or the sole agent in the
  workspace; otherwise it no-ops and keeps your comments. Browsing and diffing need no herdr.
- **last turn is poll-based** (2 s default): a turn that starts and finishes inside one poll is
  never snapshotted on its own, so the scope shows everything since the last *observed* turn start
  — never lines the agent didn't write, but possibly more than one turn.

**PR tab (GitHub)**
- **GitHub-only and read-only** — needs an authenticated `gh` and a GitHub remote; without either
  it shows one remediation line and the rest of the app (Changes, All files) is unaffected.
- **Mirrors only the branch's *open* PR** — a merged or closed PR shows as history; comment
  surfaces are capped at one page (100 rows each), with a `+more on GitHub ↗` marker when there's
  more.

**Review model**
- **Comments are in-memory and single-session** — closing the pane loses any you haven't sent or
  copied out.
- **Bulk only, consume-on-success** — Send (or copy-to-clipboard) delivers the whole set and clears
  it: no duplicates, no per-comment send. A failure leaves everything in place.
- **No line-number rebasing** — a comment's diff snippet, not its line number, keeps it locatable;
  stale comments are flagged, never silently dropped.
- **One sidebar per worktree** — two on the same worktree race the baseline ref, last writer wins.

**Budgets**
- Files over **2 MB** or **50,000 lines** show a "too large" notice; **binary** files aren't
  diffed.

## Building from source

For contributors. `herdr plugin link` skips the download build step, so place a locally built
binary where the pane command looks for it — `$HERDR_PLUGIN_ROOT/bin/herdr-reviewr`:

```bash
git clone https://github.com/persiyanov/herdr-reviewr
cd herdr-reviewr
just install   # build release → bin/herdr-reviewr, ad-hoc re-signed on macOS
herdr plugin link .
```

`just install` replaces the binary with a fresh file and ad-hoc re-signs it. On Apple Silicon that
matters: overwriting a code-signed binary in place invalidates its signature, and macOS then
SIGKILLs it at launch — so a plain `cp target/release/herdr-reviewr bin/` makes the pane open and
close instantly.

**The dev loop** after the first link:

1. Edit the code.
2. `just install` — rebuilds and re-signs the binary under `bin/`.
3. **Relaunch the sidebar** — toggle it off and back on with your keybind. The open pane keeps
   running the *old* process until you relaunch it, so a rebuild alone changes nothing on screen.

This works only while the plugin is **linked**, not installed from the marketplace. Check with
`herdr plugin list`: a `github:…` source means the pane runs a *downloaded* binary under
`~/.config/herdr/plugins/github/`, so local rebuilds never appear no matter how often you
`just install`. Switch a GitHub install to a dev link:

```bash
herdr plugin uninstall persiyanov.reviewr   # config is keyed by id and survives
herdr plugin link .
```

### Testing

`cargo test` covers the model, routing, and the engine (including integration tests against a
real `nvim --embed`). The interactive behavior is gated by live tmux harnesses that drive the
real binary with synthesized keys and SGR mouse events and assert on captured frames:

```bash
scripts/tui-all.sh          # the full sweep (each gate also runs standalone)
```

| Gate | Covers |
| --- | --- |
| `tui-test.sh` | paint, focus, typing, autosave, deleted/added files, plain All files, compose/send, hunk stepping, quit guard, no orphans |
| `tui-edit-test.sh` | multiline compose, edit-in-place, sent-is-resolve-only, list editing, Esc/Alt aliasing, batch resolve |
| `tui-scope-test.sh` | comment scope pinning, re-diff on scope flips, list jump restoring the authoring view |
| `tui-mouse-test.sh` | row/dir/grid clicks, wheel scrolling, header Send, scope chip |
| `tui-death-test.sh` | `:qa!` death, auto-respawn, manual restart, card theme survival, no orphans |
| `tui-rename-test.sh` | renamed files diff against their old path |
| `tui-split-test.sh` | `rd` split dissolving from either side, divider drag, help overlay |

Shared plumbing lives in `scripts/tui-lib.sh`. Conventions that keep the gates race-free: only
wait for text that was absent before the triggering key (`wait_for`/`wait_gone`), and send a
real Escape via `esc` — a bare Escape immediately followed by another key merges into
`Alt+<key>` in terminals without the kitty protocol. The reviewr.nvim Lua suite runs headless:
see the header of `nvim/tests/run.lua`.

## Roadmap

Customizable keybindings, structured (JSON) export, in-diff search, a side-by-side split view,
mark-file-reviewed, OSC light/dark theme autodetect, more themes (`kanagawa`, `vesper`,
`everforest`, `ayu`, a dark `github`), a `terminal`-following palette, and OSC 52 clipboard.

## Design

The living design lives in [`specs/`](specs/) — one concept per doc, always current.

## License

[MIT](LICENSE). Syntax highlighting via [syntect](https://github.com/trishume/syntect) and
[two-face](https://github.com/CosmicHorrorDev/two-face); most themes' syntax colors come from
two-face's bundled set.

Bundled `.tmTheme` syntax files in `assets/`, each under its own license:

- [Catppuccin Mocha](https://github.com/catppuccin/bat) — MIT.
- [Tokyo Night](https://github.com/folke/tokyonight.nvim) (`tokyo-night`, `tokyo-night-day`) — Apache-2.0.
- [Rosé Pine](https://github.com/rose-pine/tm-theme) (`rose-pine`, `rose-pine-dawn`) — MIT.
