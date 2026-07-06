-- reviewr.nvim: leave review comments while navigating/editing in nvim, then batch-send them to
-- the herdr Claude agent. Red/green diff comes from the user's gitsigns (inline) plus `:ReviewrDiff`
-- (nvim's built-in diff against the base). Commands and keymaps are wired in plugin/reviewr.lua.

local M = {}

local comments = require("reviewr.comments")
local format = require("reviewr.format")
local agent = require("reviewr.agent")

-- The git ref the diff/changed-file views compare against. v1 uses HEAD (the reviewer's default
-- Commit@HEAD): shows uncommitted work as red/green.
local function base_ref()
  return "HEAD"
end

-- Send every un-sent comment to the agent as one tagged review, then mark them sent.
function M.send()
  local pending = comments.pending()
  if #pending == 0 then
    vim.notify("reviewr: nothing new to send", vim.log.levels.INFO)
    return
  end
  local ok, err = agent.send(format.format_all(pending))
  if ok then
    comments.mark_sent()
    vim.notify(("reviewr: sent %d comment(s) to the agent"):format(#pending))
  else
    vim.notify("reviewr: send failed — " .. (err or "unknown"), vim.log.levels.ERROR)
  end
end

-- Open the current file's diff against the base in nvim's built-in diff mode (red/green), with the
-- base on the left and the working file on the right.
function M.diff()
  local abs = vim.api.nvim_buf_get_name(0)
  if abs == "" then
    vim.notify("reviewr: no file in this buffer", vim.log.levels.WARN)
    return
  end
  local rel = vim.fn.fnamemodify(abs, ":.")
  local ft = vim.bo.filetype
  local base = base_ref()
  local content = vim.fn.systemlist({ "git", "show", base .. ":" .. rel })
  if vim.v.shell_error ~= 0 then
    vim.notify(("reviewr: no %s version of %s"):format(base, rel), vim.log.levels.WARN)
    return
  end
  vim.cmd("diffthis")
  vim.cmd("leftabove vnew")
  local scratch = vim.api.nvim_get_current_buf()
  vim.bo[scratch].buftype = "nofile"
  vim.bo[scratch].bufhidden = "wipe"
  vim.bo[scratch].swapfile = false
  vim.api.nvim_buf_set_lines(scratch, 0, -1, false, content)
  vim.bo[scratch].filetype = ft
  vim.api.nvim_buf_set_name(scratch, ("%s [%s]"):format(rel, base))
  vim.cmd("diffthis")
  vim.cmd("wincmd p")
end

return M
