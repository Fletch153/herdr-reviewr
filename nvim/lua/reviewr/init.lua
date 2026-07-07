-- reviewr.nvim: the editor half of the herdr reviewer's nvim mode. Comments, send, and the
-- list live in the HOST (see comments.lua for the bridge); this module keeps the pieces that
-- are purely editor-side. Commands and keymaps are wired in plugin/reviewr.lua.

local M = {}

-- The git ref the diff views compare against: published by the reviewer per scope
-- (`g:reviewr_base`); `HEAD` covers standalone nvim use.
local function base_ref()
  local b = vim.g.reviewr_base
  if type(b) == "string" and b ~= "" then
    return b
  end
  return "HEAD"
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
