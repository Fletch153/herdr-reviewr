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
end, { desc = "Reviewr: diff this file vs the base" })

local map = vim.keymap.set
map("x", "<leader>rc", ":ReviewrComment<CR>", { silent = true, desc = "Reviewr: comment on selection" })
map("n", "<leader>rc", ":ReviewrComment<CR>", { silent = true, desc = "Reviewr: comment on line" })
map("n", "<leader>rl", "<Cmd>ReviewrList<CR>", { silent = true, desc = "Reviewr: comments list" })
map("n", "<leader>rs", "<Cmd>ReviewrSend<CR>", { silent = true, desc = "Reviewr: send to agent" })
map("n", "<leader>rd", "<Cmd>ReviewrDiff<CR>", { silent = true, desc = "Reviewr: diff vs base" })
