-- Inline red/green diff for the review buffer, dependency-free (no gitsigns needed). Compares the
-- buffer against its base git ref with nvim's built-in `vim.diff` and paints sign-column markers
-- plus line highlights: added (DiffAdd), changed (DiffChange), deleted (DiffDelete boundary).
-- Refreshed on read/write/edit via an autocmd, so browsing files in the reviewer shows changes the
-- same way the built-in diff pane does.

local M = {}
local ns = vim.api.nvim_create_namespace("reviewr_diff")

-- The base ref the diff compares against. v1 uses HEAD — the reviewer's default Commit@HEAD scope,
-- i.e. uncommitted work shows as changes. (Overridable later if the reviewer passes its scope.)
local function base_ref()
  return vim.env.REVIEWR_BASE or "HEAD"
end

-- The base blob for `rel` at `base_ref()`, or nil when the file is untracked/new (nothing to diff).
local function base_text(rel)
  local out = vim.fn.systemlist({ "git", "show", base_ref() .. ":" .. rel })
  if vim.v.shell_error ~= 0 then
    return nil
  end
  return table.concat(out, "\n")
end

-- Changed line ranges per buffer ({ {lo, hi}, ... }, 1-based inclusive, buffer side), kept by
-- `refresh` for the fold expression and the first-change jump.
M._hunks = {}

-- Unchanged lines further than this from a change fold away in the focused (Changes) view —
-- the same context the reviewer's own diff pane shows.
local CONTEXT = 3

-- Recompute and repaint the diff markers for `bufnr` (default: current). A no-op on scratch/
-- unnamed buffers; clears markers when there is no base (new file) so stale paint never lingers.
function M.refresh(bufnr)
  bufnr = bufnr or vim.api.nvim_get_current_buf()
  if not vim.api.nvim_buf_is_valid(bufnr) or vim.bo[bufnr].buftype ~= "" then
    return
  end
  local abs = vim.api.nvim_buf_get_name(bufnr)
  if abs == "" then
    return
  end
  vim.api.nvim_buf_clear_namespace(bufnr, ns, 0, -1)
  M._hunks[bufnr] = {}
  local rel = vim.fn.fnamemodify(abs, ":.") -- relative to cwd; the review pane's cwd is the repo root
  local base = base_text(rel)
  if not base then
    return
  end
  local cur = table.concat(vim.api.nvim_buf_get_lines(bufnr, 0, -1, false), "\n")
  local ok, hunks = pcall(vim.diff, base, cur, { result_type = "indices" })
  if not ok or type(hunks) ~= "table" then
    return
  end
  local last = vim.api.nvim_buf_line_count(bufnr)
  local ranges = M._hunks[bufnr]
  for _, h in ipairs(hunks) do
    -- {start_a, count_a, start_b, count_b}: *_a is the base, *_b the buffer (1-based, 0 = at boundary).
    local count_a, start_b, count_b = h[2], h[3], h[4]
    if count_b == 0 then
      -- Pure deletion: no buffer line to mark, so flag the surviving line at the boundary.
      local l = math.min(math.max(start_b, 1), last)
      ranges[#ranges + 1] = { lo = l, hi = l }
      vim.api.nvim_buf_set_extmark(bufnr, ns, l - 1, 0, {
        sign_text = "_",
        sign_hl_group = "DiffDelete",
      })
    else
      ranges[#ranges + 1] = { lo = start_b, hi = start_b + count_b - 1 }
      local added = count_a == 0
      local sign = added and "+" or "~"
      local hl = added and "DiffAdd" or "DiffChange"
      for i = 0, count_b - 1 do
        local l = start_b - 1 + i
        if l >= 0 and l < last then
          vim.api.nvim_buf_set_extmark(bufnr, ns, l, 0, {
            sign_text = sign,
            sign_hl_group = hl,
            line_hl_group = hl,
          })
        end
      end
    end
  end
end

-- Fold expression for the focused view: unchanged lines outside the context window of every
-- change fold to level 1; changed lines and their context stay visible. Buffers without hunk
-- data (unchanged or never refreshed) never fold.
function M.foldexpr(lnum)
  local ranges = M._hunks[vim.api.nvim_get_current_buf()]
  if not ranges or #ranges == 0 then
    return 0
  end
  for _, r in ipairs(ranges) do
    if lnum >= r.lo - CONTEXT and lnum <= r.hi + CONTEXT then
      return 0
    end
  end
  return 1
end

function M.foldtext()
  return ("╶─ %d unchanged lines ─╴"):format(vim.v.foldend - vim.v.foldstart + 1)
end

-- The Changes-tab view: fold unchanged regions away (the reviewer's hunk view, vim-native —
-- `zR`/`zo` reveal the rest) and land the cursor on the first change. A no-op for a file with
-- no changes vs the base.
function M.focus()
  local bufnr = vim.api.nvim_get_current_buf()
  M.refresh(bufnr)
  local ranges = M._hunks[bufnr]
  if not ranges or #ranges == 0 then
    return
  end
  local win = vim.api.nvim_get_current_win()
  local function setw(name, value)
    vim.api.nvim_set_option_value(name, value, { win = win })
  end
  setw("foldmethod", "expr")
  setw("foldexpr", "v:lua.require'reviewr.diff'.foldexpr(v:lnum)")
  setw("foldtext", "v:lua.require'reviewr.diff'.foldtext()")
  setw("foldenable", true)
  setw("foldlevel", 0)
  local first = math.min(ranges[1].lo, vim.api.nvim_buf_line_count(bufnr))
  vim.api.nvim_win_set_cursor(win, { first, 0 })
end

-- Leave the focused view when a file is opened outside the Changes tab: drop our folds (and
-- only ours — a user-configured foldmethod is left alone).
function M.unfocus()
  local win = vim.api.nvim_get_current_win()
  local expr = vim.api.nvim_get_option_value("foldexpr", { win = win })
  if expr:find("reviewr", 1, true) then
    vim.api.nvim_set_option_value("foldmethod", "manual", { win = win })
    vim.api.nvim_set_option_value("foldenable", false, { win = win })
  end
end

-- Install the autocmd that keeps the markers current. Scoped to review-mode nvim because this
-- module is only on the runtimepath there (editor.sh); safe to call once at plugin load.
function M.enable()
  local grp = vim.api.nvim_create_augroup("ReviewrDiff", { clear = true })
  vim.api.nvim_create_autocmd({ "BufReadPost", "BufWritePost", "InsertLeave", "TextChanged" }, {
    group = grp,
    callback = function(a)
      M.refresh(a.buf)
    end,
  })
end

return M
