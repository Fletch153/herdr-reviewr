-- Live sync between buffers and the working tree, for the embedded review editor: the agent
-- writes files while they're open here, and the user edits them in All files. Both directions
-- propagate without interaction — the host fires `checktime` on its poll, this module saves
-- the user's edits the moment they exist and decides file-changed conflicts without ever
-- raising vim's blocking W12 prompt (invisible in an embedded UI).
--
-- Enabled by the host on every engine start (embed-only — standalone nvim keeps vim's
-- defaults); the cleared augroup makes re-enabling idempotent.
local M = {}

local function file_buf(bufnr)
  return vim.bo[bufnr].buftype == "" and vim.api.nvim_buf_get_name(bufnr) ~= ""
end

function M.enable()
  local grp = vim.api.nvim_create_augroup("ReviewrLive", { clear = true })

  -- Instant autosave: a user edit is on disk the moment it exists in normal mode (TextChanged
  -- fires per change; insert-mode batches land on InsertLeave). Keeping buffers unmodified is
  -- also what lets the checktime reload path stay silent.
  vim.api.nvim_create_autocmd({ "InsertLeave", "TextChanged" }, {
    group = grp,
    callback = function(a)
      if file_buf(a.buf) and vim.bo[a.buf].modified then
        vim.api.nvim_buf_call(a.buf, function()
          -- Forced: a plain :update on a file changed since read raises the blocking
          -- "really write (y/n)?" prompt — resolution is the same documented policy
          -- (the user's typing is the newest intent).
          vim.cmd("silent! update!")
        end)
      end
    end,
  })

  -- File-changed policy, replacing the interactive default: a clean buffer reloads silently
  -- (the agent's write appears). A buffer with in-flight user edits keeps them AND writes
  -- them out immediately (forced — the plain write would raise the same blocking W12 prompt
  -- the policy exists to remove): the race window is one autosave wide and the user's typing
  -- is the newest intent, so the conflict resolves deterministically the moment it is seen.
  -- A deletion underneath keeps the buffer without rewriting it: the reviewer presents
  -- deleted files through its own scratch view, and only a fresh user edit recreates it.
  vim.api.nvim_create_autocmd("FileChangedShell", {
    group = grp,
    callback = function(a)
      if vim.v.fcs_reason == "deleted" then
        vim.v.fcs_choice = ""
      elseif vim.bo[a.buf].modified then
        vim.v.fcs_choice = ""
        vim.schedule(function() -- writing is not allowed inside this autocmd
          if vim.api.nvim_buf_is_valid(a.buf) then
            vim.api.nvim_buf_call(a.buf, function()
              vim.cmd("silent! update!")
            end)
          end
        end)
      else
        vim.v.fcs_choice = "reload"
      end
    end,
  })
end

return M
