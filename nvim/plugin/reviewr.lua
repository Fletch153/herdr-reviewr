-- reviewr.nvim entry point: auto-sourced when this plugin's dir is on the runtimepath (the herdr
-- reviewr plugin injects it in nvim review mode). Defines the review commands and <leader>r
-- keymaps. Modules are required lazily so a load error never breaks nvim startup.
--
-- Comments live in the HOST (the embedding reviewer): every comment command here reports an
-- intent over RPC; the host owns the store, the composer, the Send counter, and the list.

if vim.g.loaded_reviewr then
  return
end
vim.g.loaded_reviewr = true

-- The inline comment cards, styled to match the reviewer's own (it re-colors these groups to
-- its palette on attach; the links are the standalone/headless fallback).
vim.api.nvim_set_hl(0, "ReviewrCardTitle", { link = "Title", default = true })
vim.api.nvim_set_hl(0, "ReviewrCardBorder", { link = "NonText", default = true })
vim.api.nvim_set_hl(0, "ReviewrCardBody", { link = "Normal", default = true })
vim.api.nvim_set_hl(0, "ReviewrCommentLine", { link = "Title", default = true })
-- The gutter of a highlighted diff line (statuscolumn + number cell) matches the line paint.
vim.api.nvim_set_hl(0, "ReviewrGutterAdd", { link = "DiffAdd", default = true })
vim.api.nvim_set_hl(0, "ReviewrGutterDel", { link = "DiffDelete", default = true })

-- Clipboard: the embed has no terminal to answer OSC 52 queries, so nvim's own OSC 52
-- provider (the natural pick on a tool-less remote box) hangs every `"+` access for ~10s
-- ("waiting for OSC 52 response"). Copies instead hand the text to the host, which emits
-- OSC 52 through the real terminal it owns; pastes answer instantly from a local cache —
-- content from outside the embed arrives as a terminal paste, never as a register read.
local clip = { ["+"] = { {}, "v" }, ["*"] = { {}, "v" } }
local function clip_copy(reg)
  return function(lines, regtype)
    clip[reg] = { lines, regtype }
    require("reviewr.comments").notify("clipboard", { text = table.concat(lines, "\n") })
  end
end
local function clip_paste(reg)
  return function()
    return clip[reg][1], clip[reg][2]
  end
end
vim.g.clipboard = {
  name = "reviewr-host",
  copy = { ["+"] = clip_copy("+"), ["*"] = clip_copy("*") },
  paste = { ["+"] = clip_paste("+"), ["*"] = clip_paste("*") },
}
vim.g.loaded_clipboard_provider = nil -- re-resolve in case startup already touched a register

local cmd = vim.api.nvim_create_user_command

cmd("ReviewrComment", function(opts)
  local lo = opts.range > 0 and opts.line1 or nil
  local hi = opts.range > 0 and opts.line2 or nil
  require("reviewr.comments").comment(lo, hi)
end, { range = true, desc = "Reviewr: comment on the selection/line" })

cmd("ReviewrEdit", function()
  require("reviewr.comments").act("edit")
end, { desc = "Reviewr: edit the comment under the cursor" })

cmd("ReviewrDelete", function()
  require("reviewr.comments").act("delete")
end, { desc = "Reviewr: delete the comment under the cursor" })

cmd("ReviewrResolve", function()
  require("reviewr.comments").act("resolve")
end, { desc = "Reviewr: resolve the comment under the cursor" })

cmd("ReviewrList", function()
  require("reviewr.comments").notify("list")
end, { desc = "Reviewr: open the reviewer's comments list" })

cmd("ReviewrSend", function()
  require("reviewr.comments").notify("send")
end, { desc = "Reviewr: send un-sent comments to the agent" })

cmd("ReviewrYank", function()
  require("reviewr.comments").notify("yank")
end, { desc = "Reviewr: copy all comments to the clipboard" })

cmd("ReviewrDiff", function()
  require("reviewr.init").diff()
end, { desc = "Reviewr: split-diff this file vs the base" })

-- Diagnose why a send can't reach the agent: prints the herdr env this pane sees and the resolved
-- agent pane (or the exact resolution error). Run `:ReviewrDoctor` when a send fails.
cmd("ReviewrDoctor", function()
  local lines = { "reviewr.nvim doctor:" }
  local function add(k, v)
    lines[#lines + 1] = ("  %-20s %s"):format(k, tostring(v))
  end
  add("HERDR_PANE_ID", vim.env.HERDR_PANE_ID or "(unset)")
  add("HERDR_TAB_ID", vim.env.HERDR_TAB_ID or "(unset)")
  add("HERDR_WORKSPACE_ID", vim.env.HERDR_WORKSPACE_ID or "(unset)")
  add("HERDR_BIN_PATH", vim.env.HERDR_BIN_PATH or "(unset -> herdr)")
  local ctx = vim.env.HERDR_PLUGIN_CONTEXT_JSON
  local focus = "(no context)"
  if ctx then
    local ok, d = pcall(vim.json.decode, ctx)
    focus = (ok and type(d) == "table" and d.focused_pane_id) or "(none)"
  end
  add("focused_pane_id", focus)
  local pane, err = require("reviewr.agent").resolve_pane()
  add("resolved agent pane", pane or ("ERROR: " .. tostring(err)))
  vim.notify(table.concat(lines, "\n"), pane and vim.log.levels.INFO or vim.log.levels.WARN)
end, { desc = "Reviewr: diagnose agent/send resolution" })

-- Inline red/green diff vs the base, refreshed as you browse and edit (no gitsigns needed).
require("reviewr.diff").enable()

-- Both rc maps go through <Cmd> (never `:` replay): the compose intent must reach the host on
-- the keypress itself, so the keys typed right after land in the host's composer, not here.
local map = vim.keymap.set
map("x", "<leader>rc", "<Cmd>lua require('reviewr.comments').comment_visual()<CR>", { silent = true, desc = "Reviewr: comment on selection" })
map("n", "<leader>rc", "<Cmd>ReviewrComment<CR>", { silent = true, desc = "Reviewr: comment on line" })
map("n", "<leader>re", "<Cmd>ReviewrEdit<CR>", { silent = true, desc = "Reviewr: edit comment" })
map("n", "<leader>rx", "<Cmd>ReviewrDelete<CR>", { silent = true, desc = "Reviewr: delete comment" })
map("n", "<leader>rr", "<Cmd>ReviewrResolve<CR>", { silent = true, desc = "Reviewr: resolve comment" })
map("n", "<leader>rl", "<Cmd>ReviewrList<CR>", { silent = true, desc = "Reviewr: comments list" })
map("n", "<leader>rs", "<Cmd>ReviewrSend<CR>", { silent = true, desc = "Reviewr: send to agent" })
map("n", "<leader>ry", "<Cmd>ReviewrYank<CR>", { silent = true, desc = "Reviewr: copy comments" })
map("n", "<leader>rd", "<Cmd>ReviewrDiff<CR>", { silent = true, desc = "Reviewr: diff vs base" })
map("n", "<leader>rh", "<Cmd>lua require('reviewr.diff').revert_hunk()<CR>", { silent = true, desc = "Reviewr: revert the hunk under the cursor" })

-- Hunk hops in the focused (Changes) view; quiet no-ops when there is nothing further. In a
-- real diff-mode window (:ReviewrDiff's split) the native ]c/[c behavior is kept.
map("n", "]c", function()
  if vim.wo.diff then
    vim.cmd("normal! ]c")
  else
    require("reviewr.diff").next_change()
  end
end, { silent = true, desc = "Reviewr: next change" })
map("n", "[c", function()
  if vim.wo.diff then
    vim.cmd("normal! [c")
  else
    require("reviewr.diff").prev_change()
  end
end, { silent = true, desc = "Reviewr: previous change" })

-- Enter / Backspace: the review walk. Forward steps to the next hunk; past the last hunk it
-- asks the host to advance to the next changed file. Backspace mirrors it (previous hunk,
-- then the previous file, landing on that file's last hunk). In the plain (All files) view
-- there are no hunks, so the walk moves file to file through the changeset. Space is NOT
-- mapped here — it is the user's leader; the host translates it to this walk only inside the
-- read-only Changes pane.
local function review_walk(dir)
  if vim.wo.diff then
    vim.cmd("normal! " .. (dir > 0 and "]c" or "[c"))
    return
  end
  local name = vim.api.nvim_buf_get_name(0)
  local ours = (vim.bo.buftype == "" and name ~= "")
    or name:find("reviewr://deleted/", 1, true)
  if not ours then
    -- quickfix, help, telescope, cmdwin…: keep the key's native meaning there.
    local key = dir > 0 and "\r" or vim.keycode("<BS>")
    vim.api.nvim_feedkeys(key, "n", false)
    return
  end
  local diff = require("reviewr.diff")
  local stepped -- no and/or chain: a false step must not fall through to the other direction
  if dir > 0 then
    stepped = diff.next_change()
  else
    stepped = diff.prev_change()
  end
  if not stepped then
    -- The boundary verdict is about THIS buffer: send its file so the host can drop a
    -- verdict that raced past the open it just published (an Enter storm at a file
    -- boundary must advance once, not mark every file it never showed).
    local rel = name:match("^reviewr://deleted/(.+)$") or vim.fn.fnamemodify(name, ":.")
    require("reviewr.comments").notify("nav", { dir = dir > 0 and "next" or "prev", file = rel })
  end
end
map("n", "<CR>", function()
  review_walk(1)
end, { silent = true, desc = "Reviewr: walk forward (hunk, then next file)" })
map("n", "<BS>", function()
  review_walk(-1)
end, { silent = true, desc = "Reviewr: walk back (hunk, then previous file)" })
