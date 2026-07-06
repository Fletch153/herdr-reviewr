-- The review comment store: accumulate comments over selections, mark them in the buffer, list
-- them, and expose the un-sent ones for a batch send. Comments live for the nvim session.

local M = {}
local ns = vim.api.nvim_create_namespace("reviewr_comments")

-- Each item: { file = <repo-relative>, abs, lo, hi, code, note, sent }.
M.items = {}

local function git_root()
  local out = vim.fn.systemlist({ "git", "rev-parse", "--show-toplevel" })
  if vim.v.shell_error ~= 0 then
    return nil
  end
  return out[1]
end

-- Path relative to the git root (matches the reviewer's <ref> paths), else relative to cwd.
local function relpath(abs)
  local root = git_root()
  if root and abs:sub(1, #root + 1) == root .. "/" then
    return abs:sub(#root + 2)
  end
  return vim.fn.fnamemodify(abs, ":.")
end

local function mark(bufnr, lo, hi, note)
  if not vim.api.nvim_buf_is_valid(bufnr) then
    return
  end
  for l = lo, hi do
    vim.api.nvim_buf_set_extmark(bufnr, ns, l - 1, 0, {
      sign_text = "▌",
      sign_hl_group = "DiagnosticSignInfo",
    })
  end
  vim.api.nvim_buf_set_extmark(bufnr, ns, hi - 1, 0, {
    virt_text = { { "  ▶ " .. note, "Comment" } },
    virt_text_pos = "eol",
  })
end

-- Append a comment without any UI — the shared path for interactive `add` and the tests.
function M.record(bufnr, abs, lo, hi, code, note)
  local item =
    { file = relpath(abs), abs = abs, lo = lo, hi = hi, code = code, note = note, sent = false }
  M.items[#M.items + 1] = item
  mark(bufnr, lo, hi, note)
  return item
end

-- Comment on lines [lo, hi] (a normal-mode command has no range, so default to the cursor line),
-- prompting for the note.
function M.add(lo, hi)
  local bufnr = vim.api.nvim_get_current_buf()
  local abs = vim.api.nvim_buf_get_name(bufnr)
  if abs == "" then
    vim.notify("reviewr: this buffer has no file to comment on", vim.log.levels.WARN)
    return
  end
  lo = lo or vim.fn.line(".")
  hi = hi or lo
  local code = table.concat(vim.api.nvim_buf_get_lines(bufnr, lo - 1, hi, false), "\n")
  vim.ui.input({ prompt = ("Review note (%s:%d): "):format(relpath(abs), lo) }, function(note)
    if not note or note:gsub("%s", "") == "" then
      return
    end
    local item = M.record(bufnr, abs, lo, hi, code, note)
    vim.notify(("reviewr: comment %d — %s:%d"):format(#M.items, item.file, lo))
  end)
end

function M.list()
  if #M.items == 0 then
    vim.notify("reviewr: no comments yet", vim.log.levels.INFO)
    return
  end
  local qf = {}
  for _, c in ipairs(M.items) do
    qf[#qf + 1] = {
      filename = c.abs,
      lnum = c.lo,
      col = 1,
      text = (c.sent and "[sent] " or "") .. c.note,
    }
  end
  vim.fn.setqflist(qf, "r")
  vim.cmd("copen")
end

-- The un-sent comments, for a batch send.
function M.pending()
  local p = {}
  for _, c in ipairs(M.items) do
    if not c.sent then
      p[#p + 1] = c
    end
  end
  return p
end

function M.mark_sent()
  for _, c in ipairs(M.items) do
    c.sent = true
  end
end

return M
