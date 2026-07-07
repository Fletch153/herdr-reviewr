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

local map = vim.keymap.set
map("x", "<leader>rc", ":ReviewrComment<CR>", { silent = true, desc = "Reviewr: comment on selection" })
map("n", "<leader>rc", ":ReviewrComment<CR>", { silent = true, desc = "Reviewr: comment on line" })
map("n", "<leader>re", "<Cmd>ReviewrEdit<CR>", { silent = true, desc = "Reviewr: edit comment" })
map("n", "<leader>rx", "<Cmd>ReviewrDelete<CR>", { silent = true, desc = "Reviewr: delete comment" })
map("n", "<leader>rr", "<Cmd>ReviewrResolve<CR>", { silent = true, desc = "Reviewr: resolve comment" })
map("n", "<leader>rl", "<Cmd>ReviewrList<CR>", { silent = true, desc = "Reviewr: comments list" })
map("n", "<leader>rs", "<Cmd>ReviewrSend<CR>", { silent = true, desc = "Reviewr: send to agent" })
map("n", "<leader>ry", "<Cmd>ReviewrYank<CR>", { silent = true, desc = "Reviewr: copy comments" })
map("n", "<leader>rd", "<Cmd>ReviewrDiff<CR>", { silent = true, desc = "Reviewr: diff vs base" })

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
