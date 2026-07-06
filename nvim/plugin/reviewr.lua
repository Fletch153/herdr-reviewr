-- reviewr.nvim entry point: auto-sourced when this plugin's dir is on the runtimepath (the herdr
-- reviewr plugin injects it in nvim review mode). Defines the review commands and <leader>r
-- keymaps. Modules are required lazily so a load error never breaks nvim startup.

if vim.g.loaded_reviewr then
  return
end
vim.g.loaded_reviewr = true

local cmd = vim.api.nvim_create_user_command

cmd("ReviewrComment", function(opts)
  local lo = opts.range > 0 and opts.line1 or nil
  local hi = opts.range > 0 and opts.line2 or nil
  require("reviewr.comments").add(lo, hi)
end, { range = true, desc = "Reviewr: comment on the selection/line" })

cmd("ReviewrList", function()
  require("reviewr.comments").list()
end, { desc = "Reviewr: list review comments" })

cmd("ReviewrSend", function()
  require("reviewr.init").send()
end, { desc = "Reviewr: send comments to the agent" })

cmd("ReviewrDiff", function()
  require("reviewr.init").diff()
end, { desc = "Reviewr: split-diff this file vs the base" })

cmd("ReviewrDelete", function()
  local removed = require("reviewr.comments").delete_at(0, vim.fn.line("."))
  if removed then
    vim.notify(("reviewr: deleted comment — %s:%d"):format(removed.file, removed.lo))
  else
    vim.notify("reviewr: no comment under the cursor", vim.log.levels.WARN)
  end
end, { desc = "Reviewr: delete the comment under the cursor" })

cmd("ReviewrClear", function()
  require("reviewr.comments").clear()
  vim.notify("reviewr: cleared all comments")
end, { desc = "Reviewr: drop every comment" })

cmd("ReviewrYank", function()
  require("reviewr.init").yank()
end, { desc = "Reviewr: copy pending comments to the clipboard" })

-- Diagnose why a send can't reach the agent: prints the herdr env this pane sees and the resolved
-- agent pane (or the exact resolution error). Run `:ReviewrDoctor` when `:ReviewrSend` fails.
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
map("n", "<leader>rl", "<Cmd>ReviewrList<CR>", { silent = true, desc = "Reviewr: comments list" })
map("n", "<leader>rs", "<Cmd>ReviewrSend<CR>", { silent = true, desc = "Reviewr: send to agent" })
map("n", "<leader>rd", "<Cmd>ReviewrDiff<CR>", { silent = true, desc = "Reviewr: diff vs base" })
map("n", "<leader>rx", "<Cmd>ReviewrDelete<CR>", { silent = true, desc = "Reviewr: delete comment" })
