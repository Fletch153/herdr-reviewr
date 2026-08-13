-- Headless test runner for reviewr.nvim. Run with:
--   REVIEWR_DIR=<repo>/nvim HERDR_BIN_PATH=<stub> REVIEWR_STUB_LOG=<log> \
--   HERDR_PANE_ID=wY:pSIDEBAR HERDR_TAB_ID=wY:t1 HERDR_WORKSPACE_ID=wY \
--   HERDR_PLUGIN_CONTEXT_JSON='{"focused_pane_id":"wY:pFOCUS"}' \
--   nvim --headless -u NONE -l <repo>/nvim/tests/run.lua
-- Exits non-zero if any check fails.

vim.opt.runtimepath:prepend(assert(vim.env.REVIEWR_DIR, "REVIEWR_DIR unset"))

local failures = 0
local function check(name, cond, detail)
  if cond then
    print("ok   - " .. name)
  else
    failures = failures + 1
    print("FAIL - " .. name .. (detail and ("\n        " .. tostring(detail)) or ""))
  end
end

-- Work in a fresh non-git dir so comments.anchor's relpath falls back to cwd-relative.
local root = vim.fn.tempname()
vim.fn.mkdir(root, "p")
vim.cmd("cd " .. vim.fn.fnameescape(root))

local comments = require("reviewr.comments")
local diff = require("reviewr.diff")

-- anchor(): new-side buffers report cwd-relative paths and marker-prefixed snippets — `+`
-- inside a change hunk, space context (the Changes view's unified-diff shape).
local buf = vim.api.nvim_create_buf(true, false)
vim.api.nvim_buf_set_name(buf, root .. "/y.rs")
vim.api.nvim_buf_set_lines(buf, 0, -1, false, { "l1", "l2", "l3", "l4" })
vim.api.nvim_set_current_buf(buf)
diff._hunks[buf] = { { lo = 2, hi = 2 } }
local a = comments.anchor(1, 2)
check("anchor reports the cwd-relative file", a and a.file == "y.rs", vim.inspect(a))
check("anchor is new-side with the 1-based range", a and a.side == "new" and a.start == 1 and a["end"] == 2)
check("anchor snippet carries diff markers", a and a.lines == " l1\n+l2", a and a.lines)

-- A plain (All files) buffer captures undecorated code.
vim.b[buf].reviewr_plain = true
local ap = comments.anchor(2, 3)
check("plain-view snippet has no markers", ap and ap.lines == "l2\nl3", ap and ap.lines)
vim.b[buf].reviewr_plain = false

-- Cursor-line default and inverted ranges normalize.
vim.api.nvim_win_set_cursor(0, { 3, 0 })
local ac = comments.anchor()
check("anchor defaults to the cursor line", ac and ac.start == 3 and ac["end"] == 3)
local ai = comments.anchor(4, 2)
check("inverted ranges normalize", ai and ai.start == 2 and ai["end"] == 4)

-- The deleted-file scratch anchors old-side under its repo-relative name.
local dbuf = vim.api.nvim_create_buf(true, true)
vim.api.nvim_buf_set_name(dbuf, "reviewr://deleted/src/gone.rs")
vim.api.nvim_buf_set_lines(dbuf, 0, -1, false, { "old1", "old2" })
vim.api.nvim_set_current_buf(dbuf)
local ad = comments.anchor(1, 2)
check("deleted scratch anchors old-side", ad and ad.side == "old" and ad.file == "src/gone.rs")
check("deleted snippet is all-removed", ad and ad.lines == "-old1\n-old2", ad and ad.lines)

-- Headless (no attached UI): intents cannot reach a host.
check("notify without a host reports false", comments.notify("comment", {}) == false)

-- apply(): the host's cards paint as boxed virt_lines under the anchor plus the line-number
-- accent; a later empty push clears everything.
vim.api.nvim_set_current_buf(buf)
comments.apply({
  { start = 1, ["end"] = 2, side = "new", text = "needs a guard", location = "y.rs:1-2", sent = false },
})
local ns = vim.api.nvim_get_namespaces()["reviewr_comments"]
local marks = vim.api.nvim_buf_get_extmarks(buf, ns, 0, -1, { details = true })
local card, accents
accents = 0
for _, m in ipairs(marks) do
  local d = m[4]
  if d.virt_lines then
    card = d
    check("card sits under the anchor's last line", m[2] == 1)
  elseif d.number_hl_group == "ReviewrCommentLine" then
    accents = accents + 1
  end
end
check("both anchored lines carry the accent", accents == 2, accents)
check("a card rendered", card ~= nil)
if card then
  local flat = {}
  for _, line in ipairs(card.virt_lines) do
    local s = ""
    for _, chunk in ipairs(line) do
      s = s .. chunk[1]
    end
    flat[#flat + 1] = s
  end
  check("card top titles the location", flat[1]:find("╭─", 1, true) and flat[1]:find("comment · y.rs:1-2", 1, true), flat[1])
  check("card body holds the note", flat[2]:find("needs a guard", 1, true) ~= nil, flat[2])
  check("card closes its box", flat[#flat]:find("╰", 1, true) ~= nil, flat[#flat])
end
comments.apply({})
check("an empty push clears the cards", #vim.api.nvim_buf_get_extmarks(buf, ns, 0, -1, {}) == 0)

-- Old-side cards skip the accent on a live (new-side) buffer but still box the note.
comments.apply({
  { start = 1, ["end"] = 1, side = "old", text = "gone", location = "y.rs:1 (removed)", sent = true },
})
local marks2 = vim.api.nvim_buf_get_extmarks(buf, ns, 0, -1, { details = true })
local accent2, card2 = 0, 0
for _, m in ipairs(marks2) do
  if m[4].virt_lines then
    card2 = card2 + 1
  elseif m[4].number_hl_group then
    accent2 = accent2 + 1
  end
end
check("old-side card renders without the accent", card2 == 1 and accent2 == 0)
comments.apply({})

-- act() translates a cursor hit on a painted card back to the comment's original anchor:
-- a comment anchored past EOF paints clamped at the last line, and acting there must send
-- the stored anchor and side — not the cursor position, which the host could never match.
vim.api.nvim_set_current_buf(buf)
comments.apply({
  { start = 9, ["end"] = 10, side = "new", text = "stale", location = "y.rs:9-10", sent = false },
})
local acted = {}
local act_notify = comments.notify
comments.notify = function(action, payload)
  acted[#acted + 1] = { action = action, payload = payload }
  return true
end
vim.api.nvim_win_set_cursor(0, { 4, 0 }) -- 4-line buffer: the card clamps to line 4
comments.act("resolve")
check(
  "act through a clamped card sends the original anchor",
  #acted == 1 and acted[1].payload.line == 9 and acted[1].payload.side == "new",
  vim.inspect(acted)
)

-- An old-side card boxed on the live buffer must act with the comment's side, not the buffer's.
comments.apply({
  { start = 2, ["end"] = 2, side = "old", text = "gone", location = "y.rs:2 (removed)", sent = true },
})
vim.api.nvim_win_set_cursor(0, { 2, 0 })
comments.act("resolve")
check(
  "act through an old-side card sends side=old",
  #acted == 2 and acted[2].payload.side == "old" and acted[2].payload.line == 2,
  vim.inspect(acted[2])
)

-- Off-card lines keep the plain cursor anchor.
vim.api.nvim_win_set_cursor(0, { 1, 0 })
comments.act("resolve")
check(
  "act off-card keeps the cursor anchor",
  #acted == 3 and acted[3].payload.line == 1 and acted[3].payload.side == "new",
  vim.inspect(acted[3])
)
comments.notify = act_notify
comments.apply({})

-- The focused (Changes) view's fold expression: changed lines and their 3-line context stay
-- visible (0); everything else folds (1); buffers without hunk data never fold.
local fbuf = vim.api.nvim_create_buf(false, true)
vim.api.nvim_set_current_buf(fbuf)
diff._hunks[fbuf] = { { lo = 10, hi = 12 } }
check("far-away lines fold", diff.foldexpr(3) == 1 and diff.foldexpr(20) == 1)
check("context stays visible", diff.foldexpr(7) == 0 and diff.foldexpr(15) == 0)
check("changed lines stay visible", diff.foldexpr(10) == 0 and diff.foldexpr(12) == 0)
check("edge of context folds", diff.foldexpr(6) == 1 and diff.foldexpr(16) == 1)
diff._hunks[fbuf] = {}
check("no hunks, no folds", diff.foldexpr(3) == 0)

-- next_change/prev_change walk the hunks and report exhaustion (the reviewer's Space walk).
local sbuf = vim.api.nvim_create_buf(true, false)
vim.api.nvim_buf_set_name(sbuf, root .. "/steps.txt")
local lines = {}
for i = 1, 20 do
  lines[i] = "line " .. i
end
vim.api.nvim_buf_set_lines(sbuf, 0, -1, false, lines)
vim.api.nvim_set_current_buf(sbuf)
diff._hunks[sbuf] = { { lo = 5, hi = 6 }, { lo = 12, hi = 12 } }
vim.api.nvim_win_set_cursor(0, { 1, 0 })
check("next_change hops to the first hunk", diff.next_change() == true and vim.fn.line(".") == 5)
check("next_change hops to the second hunk", diff.next_change() == true and vim.fn.line(".") == 12)
check("next_change reports exhaustion", diff.next_change() == false)
vim.api.nvim_win_set_cursor(0, { 20, 0 })
check("prev_change hops back", diff.prev_change() == true and vim.fn.line(".") == 12)
check("prev_change hops again", diff.prev_change() == true and vim.fn.line(".") == 5)
check("prev_change reports exhaustion", diff.prev_change() == false)
diff._hunks[sbuf] = nil
check("no hunk data: next_change is a quiet no-op", diff.next_change() == false)

-- comment_visual reads the LIVE selection marks (bound via <Cmd>, so visual mode is still
-- active when it runs) and reports the normalized range.
vim.api.nvim_set_current_buf(sbuf)
local sent = {}
local real_notify = comments.notify
comments.notify = function(action, payload)
  sent[#sent + 1] = { action = action, payload = payload }
  return true
end
vim.api.nvim_win_set_cursor(0, { 2, 0 })
vim.api.nvim_feedkeys("Vj", "x", false) -- visual-line 2..3, still active (no <Esc> yet)
comments.comment_visual()
comments.notify = real_notify
vim.api.nvim_feedkeys("", "x", false) -- flush the queued <Esc> (the real event loop does this)
check("comment_visual reports the visual range", #sent == 1 and sent[1].payload.start == 2 and sent[1].payload["end"] == 3, vim.inspect(sent))
check("comment_visual leaves visual mode", not vim.fn.mode():find("[vV]"), vim.fn.mode())

-- The rc maps must stay <Cmd>-bound: a `:`-style map replays through the cmdline and widens
-- the race in which keys typed right after rc execute as normal-mode commands instead of
-- landing in the host's composer.
vim.cmd("runtime! plugin/reviewr.lua")
local rc_n = vim.fn.maparg("<leader>rc", "n")
check("normal rc maps through <Cmd>", rc_n:find("<Cmd>", 1, true) ~= nil, rc_n)
local rc_x = vim.fn.maparg("<leader>rc", "x")
check("visual rc maps through <Cmd>", rc_x:find("<Cmd>", 1, true) ~= nil, rc_x)

-- Renamed files: without the host's rename map the file reads as one big insertion (its path
-- is absent at the base); with it, the diff runs against the old path's content.
local gitroot = root .. "/renrepo"
vim.fn.mkdir(gitroot, "p")
local function git(args)
  vim.fn.system(vim.list_extend({ "git", "-C", gitroot }, args))
end
git({ "init", "-qb", "main" })
git({ "config", "user.email", "t@t" })
git({ "config", "user.name", "t" })
git({ "config", "commit.gpgsign", "false" })
vim.fn.writefile({ "s1", "s2", "s3", "s4" }, gitroot .. "/old.txt")
git({ "add", "-A" })
git({ "commit", "-qm", "A" })
vim.fn.rename(gitroot .. "/old.txt", gitroot .. "/new.txt")
vim.fn.writefile({ "s1", "EDIT", "s3", "s4" }, gitroot .. "/new.txt")
vim.cmd("cd " .. vim.fn.fnameescape(gitroot))
vim.cmd("edit new.txt")
local rbuf = vim.api.nvim_get_current_buf()
diff.refresh(rbuf)
local h = diff._hunks[rbuf]
check("a rename without the map reads as one insertion", #h == 1 and h[1].lo == 1 and h[1].hi == 4, vim.inspect(h))
diff.set_renames({ ["new.txt"] = "old.txt" })
diff.refresh(rbuf)
h = diff._hunks[rbuf]
check("the rename map narrows the diff to the real edit", #h == 1 and h[1].lo == 2 and h[1].hi == 2, vim.inspect(h))
diff.set_renames({})

-- The wrap-gap workaround: nvim can't paint 'breakindent' whitespace on wrapped rows
-- (neovim/neovim#26392), so views that paint lines drop the option and hand the user's value
-- back with the plain view; a paint-less focus clears a leftover drop instead of keeping it.
local wwin = vim.api.nvim_get_current_win()
vim.wo[wwin].breakindent = true
diff.focus()
check("focus drops breakindent while lines are painted", vim.wo[wwin].breakindent == false)
diff.focus() -- reapplying must not adopt the dropped value as "the user's"
diff.set_view(false)
check("the plain view restores the user's breakindent", vim.wo[wwin].breakindent == true)
diff.set_view(true)
check("the focused view drops breakindent again", vim.wo[wwin].breakindent == false)
diff.unfocus()
check("unfocus restores breakindent", vim.wo[wwin].breakindent == true)
diff.show_deleted("old.txt")
check("the all-red deleted view drops breakindent", vim.wo[wwin].breakindent == false)
vim.api.nvim_set_current_buf(rbuf)
diff.focus() -- painted again (drop active) before moving to a clean file
vim.fn.writefile({ "c1" }, gitroot .. "/clean.txt")
git({ "add", "clean.txt" })
git({ "commit", "-qm", "B" })
vim.cmd("edit clean.txt")
diff.focus()
check("a paint-less focus clears the lingering drop", vim.wo[wwin].breakindent == true)

-- The focused view locks the buffer ('nomodifiable' — a review surface, keyed on the view,
-- not on hunk count); the plain view hands the user's modifiable back; save/restore is
-- nil-guarded so reapplying never adopts the lock as "the user's" value, and unfocus leaves
-- the deleted scratch's own nomodifiable alone. Scratches are never locked.
check("a hunk-less focus still locks the buffer", vim.bo.modifiable == false)
vim.api.nvim_set_current_buf(rbuf)
diff.focus()
check("focus locks the changed buffer", vim.bo[rbuf].modifiable == false)
diff.focus() -- reapply: the lock must not become "the user's" saved value
diff.set_view(false)
check("the plain view restores modifiable", vim.bo[rbuf].modifiable == true)
diff.set_view(true)
check("refocusing locks again", vim.bo[rbuf].modifiable == false)
diff.unfocus()
check("unfocus unlocks", vim.bo[rbuf].modifiable == true)
diff.show_deleted("old.txt")
check("the deleted scratch stays locked through its unfocus", vim.bo.modifiable == false)
local ubuf = vim.api.nvim_create_buf(true, false)
vim.api.nvim_set_current_buf(ubuf)
vim.api.nvim_buf_set_lines(ubuf, 0, -1, false, { "user scratch" })
diff.focus()
check("an unnamed scratch is never locked", vim.bo[ubuf].modifiable == true)

-- The rd split is an editing surface (dp/do write the working buffer): opening it lifts the
-- focused lock; dissolving it from either side re-asserts the lock. Uses clean.txt — the
-- split needs a file that exists at base (new.txt is the rename's post-move name).
vim.cmd("edit clean.txt")
local cbuf = vim.api.nvim_get_current_buf()
diff.focus()
local init = require("reviewr.init")
init.diff()
check("the rd split lifts the lock", vim.bo[cbuf].modifiable == true)
for _, w in ipairs(vim.api.nvim_list_wins()) do
  if vim.api.nvim_win_get_buf(w) == cbuf then
    vim.api.nvim_set_current_win(w)
  end
end
vim.cmd("only") -- closes the scratch side; bufhidden=wipe funnels into the teardown
vim.wait(500, function()
  return vim.bo[cbuf].modifiable == false
end)
check("split teardown re-locks the working buffer", vim.bo[cbuf].modifiable == false)
diff.unfocus()

-- The gutter statuscolumn (global, set once by the host): hot rows (hunk lines, every
-- deleted-view line) paint the full gutter group; everything else renders the stock layout.
require("reviewr.diff").gutter_enable()
check(
  "gutter_enable installs the global statuscolumn",
  vim.o.statuscolumn:find("reviewr", 1, true) ~= nil,
  vim.o.statuscolumn
)
vim.api.nvim_set_current_buf(rbuf)
diff.focus()
local g1 = diff._hunks[rbuf][1]
check("a changed line paints the gutter", diff._gutter_group(rbuf, g1.lo) == "ReviewrGutterAdd")
check("a line outside every hunk keeps the stock gutter", diff._gutter_group(rbuf, 999) == nil)
diff.set_view(false)
check("the plain view clears its hunks (stock gutter everywhere)", diff._gutter_group(rbuf, g1.lo) == nil)
diff.show_deleted("old.txt")
local gbuf = vim.api.nvim_get_current_buf()
check("the deleted view paints its gutter red", diff._gutter_group(gbuf, 1) == "ReviewrGutterDel")
check(
  "the deleted view keeps the breakindent drop in the plain presentation too",
  (function()
    diff.set_view(false)
    local plain = vim.wo.breakindent == false
    diff.set_view(true)
    return plain and vim.wo.breakindent == false
  end)()
)
diff.unfocus()

-- Insert intent: in the locked view the insert-entry maps report authoring intent to the
-- host; in the plain view the same maps pass the key through (never removed — presentation
-- truth is the buffer flag). Scratches never get the maps.
vim.api.nvim_set_current_buf(rbuf)
diff.focus()
check("the locked buffer has the insert-intent maps", vim.fn.maparg("i", "n", false, true).buffer == 1)
local isent = {}
local keep_notify = comments.notify
comments.notify = function(action, payload)
  isent[#isent + 1] = { action = action, payload = payload }
  return true
end
comments.edit_intent("i")
comments.notify = keep_notify
check(
  "edit_intent reports the file and key",
  #isent == 1 and isent[1].action == "insert" and isent[1].payload.key == "i" and isent[1].payload.file == "new.txt",
  vim.inspect(isent)
)
diff.set_view(false)
comments.edit_intent("i") -- plain view: transparent passthrough into insert mode
-- (the "x" flush leaves insert mode by design, so assert by effect: typed text landed)
vim.api.nvim_feedkeys("PTX" .. vim.api.nvim_replace_termcodes("<Esc>", true, false, true), "n", false)
vim.api.nvim_feedkeys("", "x", false)
check(
  "plain-view edit_intent passes the key through",
  vim.fn.getline("."):find("PTX", 1, true) ~= nil,
  vim.fn.getline(".")
)
vim.cmd("silent! undo")
vim.api.nvim_set_current_buf(ubuf)
local umap = vim.fn.maparg("i", "n", false, true)
check("a scratch has no insert-intent maps", umap.buffer ~= 1, vim.inspect(umap))

-- Gutter template branches: virt_lines rows (cards, red removed lines) keep the stock
-- unpainted gutter; deletion-boundary lines are context, not green; wrapped hot rows paint
-- edge to edge. Re-focus first: the plain flip above cleared rbuf's hunks.
vim.api.nvim_set_current_buf(rbuf)
diff.focus()
local g2 = diff._hunks[rbuf][1]
check("a virt_lines row keeps the stock gutter", diff._gutter_template(rbuf, g2.lo, -1, "%=%l ") == "%s%=%l ")
check("a hot wrapped row paints edge to edge", diff._gutter_template(rbuf, g2.lo, 1, "%=%l ") == "%#ReviewrGutterAdd#")
check("a hot first row keeps %s and paints the fill", diff._gutter_template(rbuf, g2.lo, 0, "%=%l ") == "%s%#ReviewrGutterAdd#%=%l ")
diff._hunks[rbuf] = { { lo = 3, hi = 3, del = true } }
check("a deletion boundary line keeps the stock gutter", diff._gutter_group(rbuf, 3) == nil)
diff.refresh(rbuf) -- recompute real hunks (the fixture edit is a whole-file insertion here)

-- The rd split survives focused re-syncs without re-locking (scope changes, view flips).
vim.cmd("edit clean.txt")
local spbuf = vim.api.nvim_get_current_buf()
diff.focus()
init.diff()
check("the open split lifts the lock", vim.bo[spbuf].modifiable == true)
diff.set_view(true)
check("a focused re-sync does not re-lock the open split", vim.bo[spbuf].modifiable == true)
for _, w in ipairs(vim.api.nvim_list_wins()) do
  if vim.api.nvim_win_get_buf(w) == spbuf then
    vim.api.nvim_set_current_win(w)
  end
end
vim.cmd("only")
vim.wait(500, function()
  return vim.bo[spbuf].modifiable == false
end)
check("split teardown still re-locks", vim.bo[spbuf].modifiable == false)
diff.unfocus()

-- User-driven navigation (tag jumps, :e, jumplist) re-derives presentation from the active
-- view: a never-stamped (or other-view) buffer entered under Changes arrives locked and
-- painted; entered under All files it arrives editable and unpainted.
vim.api.nvim_set_current_buf(rbuf)
diff.set_view(true) -- active view: focused
vim.fn.writefile({ "n1", "n2" }, gitroot .. "/nav.txt") -- untracked: an all-green changed file
vim.cmd("edit nav.txt")
local navbuf = vim.api.nvim_get_current_buf()
check("a jump under Changes locks the entered buffer", vim.bo[navbuf].modifiable == false)
check("...and paints it", #(diff._hunks[navbuf] or {}) > 0, vim.inspect(diff._hunks[navbuf]))
diff.set_view(false) -- active view: plain (stamps nav.txt)
vim.api.nvim_set_current_buf(rbuf) -- rbuf was stamped focused
check("a jump under All files unlocks the entered buffer", vim.bo[rbuf].modifiable == true)
check("...and clears its marks", #(diff._hunks[rbuf] or {}) == 0)
vim.api.nvim_set_current_buf(navbuf)
diff.unfocus()

-- Live sync (reviewr.live): the host sweeps `checktime` on its poll; the FileChangedShell
-- policy must reload clean buffers silently, keep in-flight user edits (mtime updated so the
-- next save wins without the blocking W12 prompt), and keep the buffer on deletion.
require("reviewr.live").enable()
local lpath = gitroot .. "/live.txt"
vim.fn.writefile({ "l1", "l2" }, lpath)
vim.cmd("edit live.txt")
local lbuf = vim.api.nvim_get_current_buf()
vim.fn.writefile({ "l1", "l2", "EXTERNAL" }, lpath)
vim.cmd("silent! checktime")
check("a clean buffer reloads on checktime", vim.fn.getline(3) == "EXTERNAL")
vim.api.nvim_buf_set_lines(lbuf, 0, 1, false, { "USERLINE" })
vim.fn.writefile({ "clobbered" }, lpath)
-- Future-dated mtime: nvim's change detection persistently misses an external write landing
-- in the same wall-clock second as the state it stored at the last reload (reproduced ~50%
-- on 0.12.3 — whether this script crosses a second boundary between the two writes). The
-- subject here is the FileChangedShell policy, not that detection heuristic, so make the
-- timestamp unambiguous. Real agent writes land seconds apart and later writes self-heal.
local bump = os.time() + 5
vim.uv.fs_utime(lpath, bump, bump)
vim.cmd("silent! checktime")
check(
  "an in-flight user edit survives an external write",
  vim.fn.getline(1) == "USERLINE" and vim.bo[lbuf].modified,
  vim.fn.getline(1)
)
vim.wait(2000, function()
  vim.cmd("silent! checktime") -- the host re-fires checktime on every 500ms poll tick
  return (vim.fn.readfile(lpath)[1] or "") == "USERLINE"
end, 200)
check(
  "the conflict resolves to the user's version on disk",
  vim.fn.readfile(lpath)[1] == "USERLINE",
  vim.inspect({ disk = vim.fn.readfile(lpath), buf = vim.fn.getline(1), mod = vim.bo.modified })
)
vim.fn.delete(lpath)
vim.cmd("silent! checktime")
check("deletion underneath keeps the buffer content", vim.fn.getline(1) == "USERLINE")
vim.cmd("silent! bwipeout!")

vim.cmd("cd " .. vim.fn.fnameescape(root))

-- Clipboard: `"+`/`"*` never touch a terminal (the embed has none to answer) — copies report
-- a host intent and pastes answer instantly from the cache. No OSC 52 hang, ever.
local csent = {}
local keep_clip_notify = comments.notify
comments.notify = function(action, payload)
  csent[#csent + 1] = { action = action, payload = payload }
  return true
end
vim.fn.setreg("+", { "CLIPLINE1", "CLIPLINE2" })
check(
  "a plus-register write reports a clipboard intent",
  -- linewise registers carry a trailing empty element: the copied text ends in a newline,
  -- exactly what a line yank means on a clipboard.
  #csent == 1 and csent[1].action == "clipboard" and csent[1].payload.text == "CLIPLINE1\nCLIPLINE2\n",
  vim.inspect(csent)
)
check(
  "a plus-register read answers from the cache",
  vim.deep_equal(vim.fn.getreg("+", 1, true), { "CLIPLINE1", "CLIPLINE2" })
)
comments.notify = keep_clip_notify

-- The Enter/Backspace review walk: hunk to hunk inside the focused view, a "nav" intent to
-- the host at the file boundary (and immediately in the plain view, which has no hunks).
-- Foreign buffers (quickfix, help, scratches) keep the keys' native meaning.
local wroot = root .. "/walkrepo"
vim.fn.mkdir(wroot, "p")
local function wgit(args)
  vim.fn.system(vim.list_extend({ "git", "-C", wroot }, args))
end
wgit({ "init", "-qb", "main" })
wgit({ "config", "user.email", "t@t" })
wgit({ "config", "user.name", "t" })
wgit({ "config", "commit.gpgsign", "false" })
vim.fn.writefile({ "a1", "a2", "a3", "a4", "a5", "a6", "a7", "a8", "a9" }, wroot .. "/w.txt")
wgit({ "add", "-A" })
wgit({ "commit", "-qm", "W" })
vim.fn.writefile({ "a1", "EDIT2", "a3", "a4", "a5", "a6", "EDIT7", "a8", "a9" }, wroot .. "/w.txt")
vim.cmd("cd " .. vim.fn.fnameescape(wroot))
vim.cmd("edit w.txt")
local wbuf = vim.api.nvim_get_current_buf()
diff.focus()
local wh = diff._hunks[wbuf]
check("the walk fixture has two hunks", wh and #wh == 2 and wh[1].lo == 2 and wh[2].lo == 7, vim.inspect(wh))
check("focus lands on the first hunk", vim.fn.line(".") == 2)
local cr_map = vim.fn.maparg("<CR>", "n", false, true)
local bs_map = vim.fn.maparg("<BS>", "n", false, true)
check("the walk maps are installed", cr_map.callback ~= nil and bs_map.callback ~= nil)
local navs = {}
local keep_nav_notify = comments.notify
comments.notify = function(action, payload)
  navs[#navs + 1] = { action = action, payload = payload }
  return true
end
cr_map.callback()
check("Enter steps to the next hunk", vim.fn.line(".") == 7 and #navs == 0, vim.fn.line("."))
cr_map.callback()
check(
  "Enter past the last hunk reports nav next with its file and view",
  #navs == 1
    and navs[1].action == "nav"
    and navs[1].payload.dir == "next"
    and navs[1].payload.file == "w.txt"
    and navs[1].payload.view == "focused",
  vim.inspect(navs)
)
bs_map.callback()
check("Backspace steps to the previous hunk", vim.fn.line(".") == 2 and #navs == 1, vim.fn.line("."))
bs_map.callback()
check(
  "Backspace before the first hunk reports nav prev",
  #navs == 2 and navs[2].payload.dir == "prev",
  vim.inspect(navs)
)
check("last_change lands on the final hunk", diff.last_change() == true and vim.fn.line(".") == 7)
diff.set_view(false) -- plain view: no hunks, the walk is file-to-file
cr_map.callback()
check(
  "the plain view walks straight to the next file, tagged plain",
  #navs == 3 and navs[3].payload.dir == "next" and navs[3].payload.view == "plain",
  vim.inspect(navs)
)
local foreign = vim.api.nvim_create_buf(false, true) -- scratch: not ours, keys stay native
vim.api.nvim_set_current_buf(foreign)
cr_map.callback()
vim.api.nvim_feedkeys("", "x", false) -- flush the passthrough
check("a foreign scratch never reports nav", #navs == 3, vim.inspect(navs))
comments.notify = keep_nav_notify
vim.cmd("silent! bwipeout! " .. wbuf)

-- revert_hunk (<leader>rh): restore the hunk under the cursor to the base text — buffer AND
-- disk — through the lock; reverting a file's LAST hunk hands the walk to the next file.
local rroot = root .. "/revrepo"
vim.fn.mkdir(rroot, "p")
local function rgit(args)
  vim.fn.system(vim.list_extend({ "git", "-C", rroot }, args))
end
rgit({ "init", "-qb", "main" })
rgit({ "config", "user.email", "t@t" })
rgit({ "config", "user.name", "t" })
rgit({ "config", "commit.gpgsign", "false" })
local rbase = { "r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8", "r9", "r10" }
vim.fn.writefile(rbase, rroot .. "/r.txt")
vim.fn.writefile({ "t1", "t2", "t3" }, rroot .. "/t.txt")
vim.fn.writefile({ "e1", "e2", "e3" }, rroot .. "/e.txt")
vim.fn.writefile({ "m1", "m2" }, rroot .. "/m.txt")
rgit({ "add", "-A" })
rgit({ "commit", "-qm", "R" })
-- r.txt: a replace (r2->XREPL), a mid-file pure deletion (r5), a pure insertion (XINS).
vim.fn.writefile({ "r1", "XREPL", "r3", "r4", "r6", "r7", "XINS", "r8", "r9", "r10" }, rroot .. "/r.txt")
vim.fn.writefile({ "t2", "t3" }, rroot .. "/t.txt") -- top-of-file deletion
vim.fn.writefile({ "e1", "e2" }, rroot .. "/e.txt") -- EOF deletion
vim.fn.writefile({}, rroot .. "/m.txt") -- emptied (0 bytes)
vim.fn.writefile({ "brand new" }, rroot .. "/a.txt") -- added: no base, revert must refuse
vim.cmd("cd " .. vim.fn.fnameescape(rroot))

local rnavs = {}
local keep_rev_notify = comments.notify
comments.notify = function(action, payload)
  rnavs[#rnavs + 1] = { action = action, payload = payload }
  return true
end

vim.cmd("edit r.txt")
local revbuf = vim.api.nvim_get_current_buf()
diff.focus()
local rh = diff._hunks[revbuf]
check(
  "revert data rides every hunk",
  rh and #rh == 3 and rh[1].base_text[1] == "r2" and rh[2].del and rh[2].base_text[1] == "r5" and #rh[3].base_text == 0,
  vim.inspect(rh)
)
-- Scope the notify capture to the revert sequence: opening the file above emits a legitimate
-- `buf` report (the editor tells the host which file it now shows), which is not what this
-- check is about. What remains asserts the reverts themselves emit exactly the walk handoff.
rnavs = {}
vim.api.nvim_win_set_cursor(0, { 7, 0 }) -- XINS: a pure insertion reverts to nothing
check("insertion hunk reverts", diff.revert_hunk() == true and vim.fn.getline(7) == "r8")
check("...still locked and repainted", vim.bo[revbuf].modifiable == false and #diff._hunks[revbuf] == 2)
vim.api.nvim_win_set_cursor(0, { 4, 0 }) -- the r5 deletion boundary
check("mid-file deletion reverts", diff.revert_hunk() == true and vim.fn.getline(5) == "r5")
vim.api.nvim_win_set_cursor(0, { 2, 0 })
check("replace hunk reverts", diff.revert_hunk() == true and vim.fn.getline(2) == "r2")
check("the full revert is byte-exact on disk", vim.deep_equal(vim.fn.readfile(rroot .. "/r.txt"), rbase))
check(
  "the last hunk's revert hands the walk onward with its file and view",
  #rnavs == 1
    and rnavs[1].action == "nav"
    and rnavs[1].payload.dir == "next"
    and rnavs[1].payload.file == "r.txt"
    and rnavs[1].payload.view == "focused",
  vim.inspect(rnavs)
)
vim.api.nvim_win_set_cursor(0, { 1, 0 })
check("no hunk under the cursor refuses", diff.revert_hunk() == false)

vim.cmd("edit t.txt")
diff.focus()
vim.api.nvim_win_set_cursor(0, { 1, 0 })
check("top-of-file deletion reverts", diff.revert_hunk() == true and vim.fn.getline(1) == "t1")
check("...on disk too", vim.deep_equal(vim.fn.readfile(rroot .. "/t.txt"), { "t1", "t2", "t3" }))

vim.cmd("edit e.txt")
diff.focus()
vim.api.nvim_win_set_cursor(0, { 2, 0 })
check("EOF deletion reverts", diff.revert_hunk() == true and vim.fn.getline(3) == "e3")

vim.cmd("edit m.txt")
diff.focus()
check("an emptied file reverts whole", diff.revert_hunk() == true and vim.fn.getline(1) == "m1" and vim.api.nvim_buf_line_count(0) == 2)
check("...without a phantom trailing line on disk", vim.deep_equal(vim.fn.readfile(rroot .. "/m.txt"), { "m1", "m2" }))

vim.cmd("edit a.txt")
diff.focus()
vim.api.nvim_win_set_cursor(0, { 1, 0 })
check("an added file refuses to revert", diff.revert_hunk() == false and vim.fn.getline(1) == "brand new")

comments.notify = keep_rev_notify
vim.cmd("silent! bwipeout!")

vim.cmd("cd " .. vim.fn.fnameescape(root))

-- ReviewrDoctor's pane resolution (sends go through the host; this mirrors its picker):
-- the focused pane wins over an otherwise-ambiguous tab.
local agent = require("reviewr.agent")
local pane, err = agent.resolve_pane()
check("resolve_pane picks the focused agent", pane == "wY:pFOCUS", err)

if failures > 0 then
  print(("\n%d failure(s)"):format(failures))
  vim.cmd("cquit 1")
else
  print("\nall reviewr.nvim tests passed")
  vim.cmd("quitall!")
end
