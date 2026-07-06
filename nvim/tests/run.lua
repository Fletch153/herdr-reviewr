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

-- format.format_all: wrapper, preamble, sort-by-file-then-line + 1-based numbering, ref shapes,
-- and note normalization (blank lines dropped).
local format = require("reviewr.format")
local text = format.format_all({
  { file = "src/a.rs", lo = 10, hi = 10, code = "let x = 1;", note = "rename x" },
  { file = "src/a.rs", lo = 2, hi = 3, code = "fn f() {}", note = "\n  doc me \n\n" },
})
check("review wrapper", text:find("<review>", 1, true) and text:find("</review>", 1, true))
check("preamble present", text:find("carefully consider and resolve", 1, true) ~= nil)
check("first comment is the earlier line", text:find('<comment n="1">\n<ref>src/a.rs:2%-3</ref>') ~= nil, text)
check("single-line ref", text:find("<ref>src/a.rs:10</ref>", 1, true) ~= nil)
-- normalize_note mirrors export.rs::normalize_text: trailing space trimmed and blank lines
-- dropped, but leading indent kept — so "\n  doc me \n\n" becomes "  doc me".
check("note normalized (blank lines dropped, indent kept)", text:find("<note>  doc me</note>", 1, true) ~= nil, text)

-- comments store: record -> pending -> mark_sent.
local comments = require("reviewr.comments")
local buf = vim.api.nvim_create_buf(false, true)
vim.api.nvim_buf_set_lines(buf, 0, -1, false, { "a", "b", "c" })
comments.record(buf, "/tmp/x.rs", 1, 2, "a\nb", "note1")
check("record adds a pending comment", #comments.pending() == 1)
comments.mark_sent()
check("mark_sent clears pending", #comments.pending() == 0)

-- delete_at removes exactly the comment covering the line, in the right buffer.
local buf2 = vim.api.nvim_create_buf(false, true)
vim.api.nvim_buf_set_name(buf2, "/tmp/y.rs")
vim.api.nvim_buf_set_lines(buf2, 0, -1, false, { "l1", "l2", "l3", "l4" })
comments.record(buf2, "/tmp/y.rs", 1, 2, "l1\nl2", "first")
comments.record(buf2, "/tmp/y.rs", 4, 4, "l4", "second")
check("delete_at misses an uncommented line", comments.delete_at(buf2, 3) == nil)
local removed = comments.delete_at(buf2, 2)
check("delete_at removes the covering comment", removed and removed.note == "first")
local left = 0
for _, c in ipairs(comments.items) do
  if c.abs == "/tmp/y.rs" then
    left = left + 1
  end
end
check("the other comment survives the delete", left == 1)

-- yank copies the pending payload into the registers and marks sent.
local text_before = #comments.pending()
check("a pending comment remains for yank", text_before >= 1)
require("reviewr.init").yank()
local reg = vim.fn.getreg('"')
check("yank writes the tagged payload", reg:find("<review>", 1, true) ~= nil, reg)
check("yank marks comments sent", #comments.pending() == 0)

-- clear drops everything.
comments.clear()
check("clear empties the store", #comments.items == 0)

-- agent.send resolves via the focused pane (the tab is otherwise ambiguous) and delivers the
-- payload through the stub.
local agent = require("reviewr.agent")
local ok, err = agent.send("PAYLOAD-XYZ")
check("send succeeds via the stub", ok == true, err)
local log = table.concat(vim.fn.readfile(assert(vim.env.REVIEWR_STUB_LOG)), "\n")
check("delivered to the focused pane", log:find("send wY:pFOCUS", 1, true) ~= nil, log)
check("delivered the payload", log:find("PAYLOAD-XYZ", 1, true) ~= nil, log)

-- An error envelope (herdr still exits 0) is surfaced as a failed send.
vim.env.REVIEWR_STUB_MODE = "error"
local ok2, err2 = agent.send("NOPE")
check("error envelope is a failed send", ok2 == false and err2 ~= nil, err2)

if failures > 0 then
  print(("\n%d failure(s)"):format(failures))
  vim.cmd("cquit 1")
else
  print("\nall reviewr.nvim tests passed")
  vim.cmd("quitall!")
end
