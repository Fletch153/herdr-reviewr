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

-- Record the on-disk stat a buffer was last synced against (load, write, checktime reload).
local function stamp(bufnr)
  local st = vim.uv.fs_stat(vim.api.nvim_buf_get_name(bufnr))
  if st then
    vim.b[bufnr].reviewr_disk = { sec = st.mtime.sec, nsec = st.mtime.nsec, size = st.size }
  end
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

  -- Keep the disk stamps aligned with nvim's own sync points, so poll() below only ever
  -- fires in nvim's blind spot. FileChangedShellPost covers checktime reloads, which fire
  -- neither BufReadPost nor TextChanged.
  vim.api.nvim_create_autocmd({ "BufReadPost", "BufWritePost", "FileChangedShellPost" }, {
    group = grp,
    callback = function(a)
      if file_buf(a.buf) then
        stamp(a.buf)
      end
    end,
  })
end

-- The host's poll tick. checktime sweeps ordinary agent writes, but nvim's timestamp check
-- compares mtime (seconds + nanoseconds) and mode only — never size or content — so a write
-- that preserves the file's mtime exactly (cp -p, rsync -t, restoring a saved copy) is
-- invisible to it forever; retries never heal. The stamps recorded above let this catch the
-- detectable slice of that shadow (same mtime, different size) and apply the same
-- FileChangedShell policy: clean buffers reload, in-flight user edits win and write out.
-- An mtime-preserving write of the exact same byte count stays invisible (content hashing
-- is not worth the per-tick cost).
function M.poll()
  vim.cmd("silent! checktime")
  for _, buf in ipairs(vim.api.nvim_list_bufs()) do
    if vim.api.nvim_buf_is_loaded(buf) and file_buf(buf) then
      local st = vim.uv.fs_stat(vim.api.nvim_buf_get_name(buf))
      local s = vim.b[buf].reviewr_disk
      if st and not s then
        stamp(buf)
      elseif st and (s.sec ~= st.mtime.sec or s.nsec ~= st.mtime.nsec) then
        -- mtime moved: the checktime above already saw everything we can see. Adopt.
        stamp(buf)
      elseif st and s.size ~= st.size then
        vim.api.nvim_buf_call(buf, function()
          if vim.bo[buf].modified then
            vim.cmd("silent! update!")
          else
            vim.cmd("silent! edit!")
          end
        end)
        stamp(buf)
      end
    end
  end
end

return M
