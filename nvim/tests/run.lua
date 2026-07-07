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
