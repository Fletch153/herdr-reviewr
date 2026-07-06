-- Inline red/green diff for the review buffer, dependency-free (no gitsigns needed). Compares the
-- buffer against its base git ref with nvim's built-in `vim.diff` and paints sign-column markers
-- plus line highlights: added (DiffAdd), changed (DiffChange), deleted (DiffDelete boundary).
-- Refreshed on read/write/edit via an autocmd, so browsing files in the reviewer shows changes the
-- same way the built-in diff pane does.

local M = {}
local ns = vim.api.nvim_create_namespace("reviewr_diff")

-- The git ref the diff compares against: the reviewer publishes its scope's old side into
-- `g:reviewr_base` with every open/scope change (branch merge-base, turn-baseline tree,
-- picked commit); `HEAD` covers standalone nvim use.
local function base_ref()
  local b = vim.g.reviewr_base
  if type(b) == "string" and b ~= "" then
    return b
  end
  return vim.env.REVIEWR_BASE or "HEAD"
end

-- The base blob's lines for `rel` at `base_ref()`. A path absent from a resolvable base is an
-- added/untracked file: diff against an empty base so the whole file shows green (the
-- reviewer's semantics). nil only when the base itself doesn't resolve (no repo, bad ref) —
-- then there is nothing meaningful to diff. Kept as a list so deleted lines can be rendered
-- back as virtual lines.
local function base_lines(rel)
  local out = vim.fn.systemlist({ "git", "show", base_ref() .. ":" .. rel })
  if vim.v.shell_error == 0 then
    return out
  end
  vim.fn.system({ "git", "rev-parse", "--verify", "--quiet", base_ref() .. "^{tree}" })
  if vim.v.shell_error == 0 then
    return {}
  end
  return nil
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
  local base = base_lines(rel)
  if not base then
    return
  end
  local cur = table.concat(vim.api.nvim_buf_get_lines(bufnr, 0, -1, false), "\n")
  local ok, hunks = pcall(vim.diff, table.concat(base, "\n"), cur, { result_type = "indices" })
  if not ok or type(hunks) ~= "table" then
    return
  end
  local last = vim.api.nvim_buf_line_count(bufnr)
  local ranges = M._hunks[bufnr]
  for _, h in ipairs(hunks) do
    -- {start_a, count_a, start_b, count_b}: *_a is the base, *_b the buffer (1-based, 0 = at boundary).
    local start_a, count_a, start_b, count_b = h[1], h[2], h[3], h[4]
    -- The old side: removed/replaced base lines render back as red virtual lines — above the
    -- new text of a modification, at the boundary of a pure deletion (git-diff semantics).
    if count_a > 0 then
      local virt = {}
      for i = start_a, math.min(start_a + count_a - 1, #base) do
        virt[#virt + 1] = { { base[i], "DiffDelete" } }
      end
      local row, above
      if count_b == 0 and start_b > 0 then
        row, above = math.min(start_b, last) - 1, false -- below the line the deletion follows
      else
        row, above = math.max(start_b, 1) - 1, true
      end
      pcall(vim.api.nvim_buf_set_extmark, bufnr, ns, row, 0, {
        virt_lines = virt,
        virt_lines_above = above,
      })
    end
    if count_b == 0 then
      -- Pure deletion: no buffer line to color, so flag the surviving boundary line.
      local l = math.min(math.max(start_b, 1), last)
      ranges[#ranges + 1] = { lo = l, hi = l }
      vim.api.nvim_buf_set_extmark(bufnr, ns, l - 1, 0, {
        sign_text = "_",
        sign_hl_group = "DiffDelete",
      })
    else
      ranges[#ranges + 1] = { lo = start_b, hi = start_b + count_b - 1 }
      -- The new side is green whether added or replacing (old text shows red above); the sign
      -- still distinguishes a pure add from a modification.
      local sign = count_a == 0 and "+" or "~"
      for i = 0, count_b - 1 do
        local l = start_b - 1 + i
        if l >= 0 and l < last then
          vim.api.nvim_buf_set_extmark(bufnr, ns, l, 0, {
            sign_text = sign,
            sign_hl_group = "DiffAdd",
            line_hl_group = "DiffAdd",
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

-- Show a file that exists only in the base (deleted in the worktree) as a read-only, all-red
-- scratch view of the base content. Deliberately not a real `:edit` of the missing path: a
-- phantom buffer with a filetype would attach the user's LSP ("file is not included anywhere
-- in the module tree" noise); a nameless-scheme nofile buffer attaches nothing.
function M.show_deleted(rel)
  local name = "reviewr://deleted/" .. rel
  local buf = vim.fn.bufnr("^" .. vim.fn.fnameescape(name) .. "$")
  if buf == -1 then
    buf = vim.api.nvim_create_buf(true, true) -- listed scratch
    vim.api.nvim_buf_set_name(buf, name)
  end
  local lines = base_lines(rel) or {}
  vim.bo[buf].modifiable = true
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  vim.bo[buf].modifiable = false
  vim.bo[buf].buftype = "nofile"
  vim.bo[buf].swapfile = false
  vim.api.nvim_set_current_buf(buf)
  vim.api.nvim_buf_clear_namespace(buf, ns, 0, -1)
  for l = 0, #lines - 1 do
    vim.api.nvim_buf_set_extmark(buf, ns, l, 0, {
      sign_text = "_",
      sign_hl_group = "DiffDelete",
      line_hl_group = "DiffDelete",
    })
  end
  M._hunks[buf] = {}
  M.unfocus() -- a fully-deleted file has nothing to fold
end

-- The reviewer's scope/base changed while this file stays open: re-diff against the new base
-- and, when our review folds are active, recompute them for the new hunks (`zx` re-evaluates
-- expression folds); the cursor stays put.
function M.rebase()
  local bufnr = vim.api.nvim_get_current_buf()
  M.refresh(bufnr)
  local win = vim.api.nvim_get_current_win()
  local expr = vim.api.nvim_get_option_value("foldexpr", { win = win })
  if expr:find("reviewr", 1, true) then
    local ranges = M._hunks[bufnr]
    if not ranges or #ranges == 0 then
      M.unfocus() -- nothing changed vs the new base: show the plain file
    else
      vim.cmd("silent! normal! zx")
    end
  end
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
