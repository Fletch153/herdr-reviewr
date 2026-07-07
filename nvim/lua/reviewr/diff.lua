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

-- Renamed paths in the reviewer's changeset, `{ [new_rel] = old_rel }` — pushed by the host on
-- every view sync so a renamed file diffs against its old path's content instead of reading
-- as one big insertion (the built-in pane's semantics).
M._renames = {}

function M.set_renames(map)
  M._renames = type(map) == "table" and map or {}
end

-- The base blob's lines for `rel` at `base_ref()`. A renamed file's content lives at its old
-- path in the base, so that is tried before concluding the file is new. A path absent from a
-- resolvable base is an added/untracked file: diff against an empty base so the whole file
-- shows green (the reviewer's semantics). nil only when the base itself doesn't resolve (no
-- repo, bad ref) — then there is nothing meaningful to diff. Kept as a list so deleted lines
-- can be rendered back as virtual lines.
local function show_blob(spec)
  local raw = vim.fn.system({ "git", "show", spec })
  if vim.v.shell_error ~= 0 then
    return nil
  end
  -- Split the raw blob so the trailing-newline byte survives: an EOL-only difference is a
  -- real change git lists, and the view must be able to say so.
  local eol = raw == "" or raw:sub(-1) == "\n"
  local lines = vim.split(eol and raw:sub(1, -2) or raw, "\n", { plain = true })
  if raw == "" then
    lines = {}
  end
  return lines, eol
end

local function base_lines(rel)
  local lines, eol = show_blob(base_ref() .. ":" .. rel)
  if lines then
    return lines, eol
  end
  local old = M._renames[rel]
  if old then
    lines, eol = show_blob(base_ref() .. ":" .. old)
    if lines then
      return lines, eol
    end
  end
  vim.fn.system({ "git", "rev-parse", "--verify", "--quiet", base_ref() .. "^{tree}" })
  if vim.v.shell_error == 0 then
    return {}, true
  end
  return nil
end

-- Changed line ranges per buffer ({ {lo, hi}, ... }, 1-based inclusive, buffer side), kept by
-- `refresh` for the fold expression and the first-change jump.
M._hunks = {}

-- The view the host last established ("focused" | "plain" | nil before any). Presentation is
-- stamped per buffer at open time, but the user navigates freely inside nvim (tag jumps,
-- jumplist, :e) onto buffers stamped under the other view or never stamped — BufEnter (see
-- enable()) re-derives their presentation from this, so one session can't mix views.
M._view = nil

-- Unchanged lines further than this from a change fold away in the focused (Changes) view —
-- the same context the reviewer's own diff pane shows.
local CONTEXT = 3

-- Recompute and repaint the diff markers for `bufnr` (default: current). A no-op on scratch/
-- unnamed buffers; a buffer in plain mode (opened from All files — the file as it exists now,
-- undecorated) keeps no marks at all; clears markers when there is no base (new file) so stale
-- paint never lingers.
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
  if vim.b[bufnr].reviewr_plain then
    return
  end
  local rel = vim.fn.fnamemodify(abs, ":.") -- relative to cwd; the review pane's cwd is the repo root
  local base, base_eol = base_lines(rel)
  if not base then
    return
  end
  -- Join with a trailing newline on each non-empty side: a plain concat makes the last base
  -- line look modified whenever lines are appended at EOF, painting an unchanged line green
  -- (and mis-anchoring the first-change jump). An empty side stays "" so an added file still
  -- diffs as pure insertion.
  local old = #base > 0 and (table.concat(base, "\n") .. "\n") or ""
  local cur = table.concat(vim.api.nvim_buf_get_lines(bufnr, 0, -1, false), "\n")
  if cur ~= "" then
    cur = cur .. "\n"
  end
  local ok, hunks = pcall(vim.diff, old, cur, { result_type = "indices" })
  if not ok or type(hunks) ~= "table" then
    return
  end
  local last = vim.api.nvim_buf_line_count(bufnr)
  local ranges = M._hunks[bufnr]
  for _, h in ipairs(hunks) do
    -- {start_a, count_a, start_b, count_b}: *_a is the base, *_b the buffer (1-based, 0 = at boundary).
    local start_a, count_a, start_b, count_b = h[1], h[2], h[3], h[4]
    -- What revert_hunk needs, carried per hunk: the raw indices (lo below CLAMPS start_b for
    -- pure deletions and would destroy the revert arithmetic) and the base text itself, so a
    -- revert never re-reads git (no TOCTOU against a moving base). no_base marks an added
    -- file — deleting everything is not "back to base", so revert refuses there.
    local revert = {
      start_a = start_a,
      count_a = count_a,
      start_b = start_b,
      count_b = count_b,
      base_text = {},
      no_base = #base == 0,
    }
    for i = start_a, math.min(start_a + count_a - 1, #base) do
      revert.base_text[#revert.base_text + 1] = base[i]
    end
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
      -- del: the boundary line itself is context (red sign only, no line paint) — folds and
      -- stepping treat it as a hunk, the gutter must not paint it green.
      revert.lo, revert.hi, revert.del = l, l, true
      ranges[#ranges + 1] = revert
      vim.api.nvim_buf_set_extmark(bufnr, ns, l - 1, 0, {
        sign_text = "_",
        sign_hl_group = "DiffDelete",
      })
    else
      revert.lo, revert.hi = start_b, start_b + count_b - 1
      ranges[#ranges + 1] = revert
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
            -- The gutter statuscolumn paints fill and sign cells, but the number cell keeps
            -- LineNr unless painted here (it covers wrapped rows too). Low priority so a
            -- comment accent's number_hl stays on top.
            number_hl_group = "ReviewrGutterAdd",
            priority = 90,
          })
        end
      end
    end
  end
  -- A final-newline difference is invisible to the line diff above but is a real change git
  -- lists the file for — without a marker the file reads "unchanged yet still listed".
  -- Mirrors git's "\ No newline at end of file" note; a hunk entry keeps it steppable and
  -- unfolded (del: context-style, no green gutter).
  if #base > 0 and last > 0 and vim.bo[bufnr].endofline ~= base_eol then
    local note = base_eol and "\\ no newline at end of file (base has one)"
      or "\\ newline at end of file (base has none)"
    -- eol: revert flips 'endofline' back to the base's instead of touching lines.
    ranges[#ranges + 1] = { lo = last, hi = last, del = true, eol = true, base_eol = base_eol }
    pcall(vim.api.nvim_buf_set_extmark, bufnr, ns, last - 1, 0, {
      virt_lines = { { { note, "DiffChange" } } },
      sign_text = "~",
      sign_hl_group = "DiffChange",
    })
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

-- Step the cursor to the next / previous change hunk in this buffer — the reviewer's Space
-- walk and the ]c/[c maps. Returns false when no further hunk lies in that direction (the
-- reviewer then advances to the next file) or the buffer has no hunk data at all.
function M.next_change()
  local ranges = M._hunks[vim.api.nvim_get_current_buf()]
  if not ranges or #ranges == 0 then
    return false
  end
  local cur = vim.fn.line(".")
  for _, r in ipairs(ranges) do
    if r.lo > cur then
      vim.api.nvim_win_set_cursor(0, { math.min(r.lo, vim.api.nvim_buf_line_count(0)), 0 })
      vim.cmd("silent! normal! zvzz")
      return true
    end
  end
  return false
end

function M.prev_change()
  local ranges = M._hunks[vim.api.nvim_get_current_buf()]
  if not ranges or #ranges == 0 then
    return false
  end
  local cur = vim.fn.line(".")
  for i = #ranges, 1, -1 do
    if ranges[i].hi < cur then
      vim.api.nvim_win_set_cursor(
        0,
        { math.min(ranges[i].lo, vim.api.nvim_buf_line_count(0)), 0 }
      )
      vim.cmd("silent! normal! zvzz")
      return true
    end
  end
  return false
end

-- Land on the buffer's last hunk — the backward walk's entry point into a file (`focus()`
-- already lands forward entries on the first hunk). Fired by the host after the sync that
-- opened the file publishes, so it wins over focus()'s first-hunk placement.
function M.last_change()
  local ranges = M._hunks[vim.api.nvim_get_current_buf()]
  if not ranges or #ranges == 0 then
    return false
  end
  local last = ranges[#ranges]
  vim.api.nvim_win_set_cursor(0, { math.min(last.lo, vim.api.nvim_buf_line_count(0)), 0 })
  vim.cmd("silent! normal! zvzz")
  return true
end

-- Revert the hunk under the cursor to the base text (<leader>rh): one set_lines (a single
-- undo block), an immediate forced write (the BufWritePost refresh repaints), and the lock
-- restored. Reverting the file's last hunk reports "nav next" — the reviewer moves on and
-- the now-unchanged file drops from Changes on the host's next poll.
function M.revert_hunk()
  local bufnr = vim.api.nvim_get_current_buf()
  if vim.bo[bufnr].buftype ~= "" or vim.api.nvim_buf_get_name(bufnr) == "" then
    vim.notify("nothing to revert here", vim.log.levels.INFO)
    return false
  end
  local ranges = M._hunks[bufnr]
  local cur = vim.fn.line(".")
  local hunk
  for _, r in ipairs(ranges or {}) do
    if r.lo <= cur and cur <= r.hi then
      hunk = r
      break
    end
  end
  if not hunk then
    vim.notify("no hunk under the cursor", vim.log.levels.INFO)
    return false
  end
  if hunk.no_base then
    vim.notify("no base to revert to (added file)", vim.log.levels.INFO)
    return false
  end
  local was_locked = not vim.bo[bufnr].modifiable
  vim.bo[bufnr].modifiable = true
  if hunk.eol then
    vim.bo[bufnr].endofline = hunk.base_eol
  elseif hunk.count_b > 0 then
    vim.api.nvim_buf_set_lines(bufnr, hunk.start_b - 1, hunk.start_b - 1 + hunk.count_b, false, hunk.base_text)
  elseif hunk.start_b == 0 and vim.api.nvim_buf_line_count(bufnr) == 1 and vim.fn.getline(1) == "" then
    -- An emptied-but-existing file is one phantom empty line, not "content at line 0":
    -- replace the whole buffer or the phantom line would survive as a trailing blank.
    vim.api.nvim_buf_set_lines(bufnr, 0, -1, false, hunk.base_text)
  else
    -- A pure deletion: re-insert after start_b (0 = at the top, line_count = at EOF).
    vim.api.nvim_buf_set_lines(bufnr, hunk.start_b, hunk.start_b, false, hunk.base_text)
  end
  vim.cmd("silent! update!") -- the write's refresh autocmd repaints marks and folds
  if was_locked then
    vim.bo[bufnr].modifiable = false
  end
  if #(M._hunks[bufnr] or {}) == 0 then
    -- That was the file's last hunk: hand the walk to the next changed file. The file and
    -- the presentation ride along so the host can drop the verdict if its view moved past it.
    local rel = vim.fn.fnamemodify(vim.api.nvim_buf_get_name(bufnr), ":.")
    local view = vim.b[bufnr].reviewr_plain and "plain" or "focused"
    require("reviewr.comments").notify("nav", { dir = "next", file = rel, view = view })
  else
    vim.notify("hunk reverted — undo in All files", vim.log.levels.INFO)
  end
  return true
end

-- The Changes-tab view: fold unchanged regions away (the reviewer's hunk view, vim-native —
-- `zR`/`zo` reveal the rest) and land the cursor on the first change. A no-op for a file with
-- no changes vs the base.
local function apply_folds(win)
  local function setw(name, value)
    vim.api.nvim_set_option_value(name, value, { win = win })
  end
  setw("foldmethod", "expr")
  setw("foldexpr", "v:lua.require'reviewr.diff'.foldexpr(v:lnum)")
  setw("foldtext", "v:lua.require'reviewr.diff'.foldtext()")
  setw("foldenable", true)
  setw("foldlevel", 0)
end

-- Drop our expression folds (and only ours — a user-configured foldmethod is left alone).
local function drop_folds(win)
  local expr = vim.api.nvim_get_option_value("foldexpr", { win = win })
  if expr:find("reviewr", 1, true) then
    vim.api.nvim_set_option_value("foldmethod", "manual", { win = win })
    vim.api.nvim_set_option_value("foldenable", false, { win = win })
  end
end

-- nvim cannot paint 'breakindent' whitespace: on a wrapped green/red line the continuation
-- row's indent keeps the normal background, an unhighlighted gap inside the line
-- (neovim/neovim#26392 — no extmark reaches that region; only native diff mode does). While
-- our line paint is active the indent is dropped so highlighted lines wrap edge-to-edge; the
-- user's value returns with the plain view. 'showbreak' is left alone: its marker is the
-- wrap cue, two cells rather than an indent's width.
local function drop_breakindent(win)
  if vim.w[win].reviewr_saved_bri == nil then
    vim.w[win].reviewr_saved_bri = vim.api.nvim_get_option_value("breakindent", { win = win })
  end
  vim.api.nvim_set_option_value("breakindent", false, { win = win })
end

local function restore_breakindent(win)
  local saved = vim.w[win].reviewr_saved_bri
  if saved ~= nil then
    vim.api.nvim_set_option_value("breakindent", saved, { win = win })
    vim.w[win].reviewr_saved_bri = nil
  end
end

-- Gutter painting for highlighted lines. line_hl/sign_hl cover the text area and the sign
-- cell of the FIRST screen row only: wrapped rows leave the sign column dark, and the number
-- cell keeps LineNr everywhere — a distracting unpainted strip inside a green/red line. A
-- 'statuscolumn' closes it, set ONCE globally by the host (embed-only): the function renders
-- the stock layout wherever nothing is painted (plain views, foreign buffers — their _hunks
-- are empty), so it needs no per-window lifecycle (window-local option copies reset on
-- buffer switches, which made save/restore unsound). Hot first rows keep `%s` so third-party
-- signs still render; hot wrapped rows paint edge to edge; the number cell itself is painted
-- by `number_hl_group` on the line's extmark (it covers wrapped rows too).
local function gutter_group(bufnr, lnum)
  if vim.api.nvim_buf_get_name(bufnr):find("reviewr://deleted/", 1, true) then
    return "ReviewrGutterDel"
  end
  for _, r in ipairs(M._hunks[bufnr] or {}) do
    if r.lo <= lnum and lnum <= r.hi and not r.del then
      return "ReviewrGutterAdd"
    end
  end
  return nil
end
M._gutter_group = gutter_group -- exposed for the headless checks

-- The template per screen row (pure; virtnum: 0 = the line's first row, > 0 = wrapped
-- continuation, < 0 = virt_lines rows — comment cards and red removed lines render there
-- and must keep the stock, unpainted gutter).
function M._gutter_template(bufnr, lnum, virtnum, num)
  if virtnum < 0 then
    return "%s" .. num
  end
  local grp = gutter_group(bufnr, lnum)
  if not grp then
    return "%s" .. num
  end
  if virtnum > 0 then
    return "%#" .. grp .. "#"
  end
  return "%s%#" .. grp .. "#" .. num
end

function M.statuscolumn()
  local ok, out = pcall(function()
    local win = vim.g.statusline_winid
    -- Mirror the stock gutter: a number segment only where the user shows numbers.
    local num = (vim.wo[win].number or vim.wo[win].relativenumber) and "%=%l " or ""
    return M._gutter_template(vim.api.nvim_win_get_buf(win), vim.v.lnum, vim.v.virtnum, num)
  end)
  return ok and out or "%s%=%l "
end

function M.gutter_enable()
  vim.o.statuscolumn = "%!v:lua.require'reviewr.diff'.statuscolumn()"
end

-- The focused (Changes) view is a review surface, not an authoring one: the buffer is locked
-- ('nomodifiable') so stray keys, undo, and paste cannot mutate what the agent wrote; insert
-- intent flips to All files instead. Locking keys on the VIEW, not on hunk count — a
-- hunk-less Changes file (e.g. right after its last hunk was reverted) is still review
-- surface. Save/restore is nil-guarded like the breakindent pair and gated to real named
-- file buffers: a cancelled `:confirm edit` runs focus() against whatever buffer stayed
-- current — possibly the user's scratch — and must never lock it. Public because the rd
-- split lifts the lock for its lifetime (dp/do write the working buffer).
local function lockable(bufnr)
  return vim.bo[bufnr].buftype == "" and vim.api.nvim_buf_get_name(bufnr) ~= ""
end

-- Insert-entry keys that carry authoring intent out of the locked view (comments.edit_intent
-- flips to All files). Buffer-local, installed once with the first lock, never removed: in
-- the plain view they pass the key through untouched, so there is no add/remove lifecycle to
-- leak. Everything else mutating (dd, x, p, u, ...) answers E21 honestly.
local INSERT_KEYS = { "i", "I", "a", "A", "o", "O", "gi" }

local function install_edit_maps(bufnr)
  if vim.b[bufnr].reviewr_edit_maps then
    return
  end
  vim.b[bufnr].reviewr_edit_maps = true
  for _, k in ipairs(INSERT_KEYS) do
    vim.keymap.set("n", k, function()
      require("reviewr.comments").edit_intent(k)
    end, { buffer = bufnr, nowait = true })
  end
end

function M.lock(bufnr)
  if not lockable(bufnr) or vim.b[bufnr].reviewr_split_active then
    return -- the rd split is an editing surface: re-syncs while it is open must not re-lock
  end
  if vim.b[bufnr].reviewr_saved_ma == nil then
    vim.b[bufnr].reviewr_saved_ma = vim.bo[bufnr].modifiable
  end
  vim.bo[bufnr].modifiable = false
  install_edit_maps(bufnr)
end

function M.unlock(bufnr)
  local saved = vim.b[bufnr].reviewr_saved_ma
  if saved ~= nil then
    vim.bo[bufnr].modifiable = saved
    vim.b[bufnr].reviewr_saved_ma = nil
  end
end

function M.focus()
  local bufnr = vim.api.nvim_get_current_buf()
  local win = vim.api.nvim_get_current_win()
  M._view = "focused"
  vim.b[bufnr].reviewr_plain = false
  M.lock(bufnr) -- before the early return: hunk-less files are read-only too
  M.refresh(bufnr)
  local ranges = M._hunks[bufnr]
  if not ranges or #ranges == 0 then
    restore_breakindent(win) -- nothing painted here: an earlier buffer's drop must not linger
    return
  end
  apply_folds(win)
  drop_breakindent(win)
  local first = math.min(ranges[1].lo, vim.api.nvim_buf_line_count(bufnr))
  vim.api.nvim_win_set_cursor(win, { first, 0 })
end

-- Switch the current buffer between the review presentations without moving the cursor:
-- focused (Changes — marks + folds) or plain (All files — the file exactly as it exists now,
-- no decoration). Also the scope-changed re-diff path, hence the fold recompute (`zx`).
function M.set_view(focused)
  local bufnr = vim.api.nvim_get_current_buf()
  local win = vim.api.nvim_get_current_win()
  M._view = focused and "focused" or "plain"
  vim.b[bufnr].reviewr_plain = not focused
  if focused then -- keyed on the view, never on the hunks branch below
    M.lock(bufnr)
  else
    M.unlock(bufnr)
  end
  M.refresh(bufnr) -- plain: clears every mark; focused: repaints vs the current base
  local ranges = M._hunks[bufnr]
  if focused and ranges and #ranges > 0 then
    apply_folds(win)
    drop_breakindent(win)
    vim.cmd("silent! normal! zx")
  elseif vim.api.nvim_buf_get_name(bufnr):find("reviewr://deleted/", 1, true) then
    -- The deleted scratch stays all-red in either view (its marks never clear): the
    -- breakindent gap must stay closed too.
    drop_breakindent(win)
  else
    drop_folds(win)
    restore_breakindent(win)
  end
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
      number_hl_group = "ReviewrGutterDel",
      priority = 90,
    })
  end
  M._hunks[buf] = {}
  M.unfocus() -- a fully-deleted file has nothing to fold
  M._view = "focused" -- the deleted view belongs to the Changes tab
  drop_breakindent(vim.api.nvim_get_current_win()) -- every line here is painted red
end

-- The Changes tab has nothing to show (an empty changeset): park on a reusable read-only
-- scratch that says so, instead of leaving another tab's buffer up — a visible file there
-- reads as "this changeset has changes". Same nameless-scheme nofile shape as the deleted
-- scratch, and unlisted: a housekeeping buffer, not one the user should cycle onto.
function M.show_empty()
  local name = "reviewr://empty"
  local buf = vim.fn.bufnr("^" .. vim.fn.fnameescape(name) .. "$")
  if buf == -1 then
    buf = vim.api.nvim_create_buf(false, true) -- unlisted scratch
    vim.api.nvim_buf_set_name(buf, name)
  end
  vim.bo[buf].modifiable = true
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, { "no changes in scope" })
  vim.bo[buf].modifiable = false
  vim.bo[buf].buftype = "nofile"
  vim.bo[buf].swapfile = false
  vim.api.nvim_set_current_buf(buf)
  M._hunks[buf] = {}
  M.unfocus() -- nothing to fold or paint
  M._view = "focused" -- the empty state belongs to the Changes tab
end

-- The reviewer's scope/base changed while this file stays open: re-diff against the new base
-- and, when our review folds are active, recompute them for the new hunks (`zx` re-evaluates
-- expression folds); the cursor stays put.
-- Leave the focused view when a file is opened outside the Changes tab: mark the buffer
-- plain (no diff decoration — All files shows the file as it exists now), clear its marks,
-- and drop our folds (and only ours — a user-configured foldmethod is left alone).
function M.unfocus()
  local bufnr = vim.api.nvim_get_current_buf()
  M._view = "plain"
  vim.b[bufnr].reviewr_plain = true
  M.unlock(bufnr) -- nil-guarded: show_deleted's own nomodifiable scratch is left alone
  M.refresh(bufnr) -- plain flag set: clears the buffer's marks
  local win = vim.api.nvim_get_current_win()
  drop_folds(win)
  restore_breakindent(win)
end

-- Install the autocmd that keeps the markers current. Scoped to review-mode nvim because this
-- module is only on the runtimepath there (editor.sh); safe to call once at plugin load.
function M.enable()
  local grp = vim.api.nvim_create_augroup("ReviewrDiff", { clear = true })
  -- FileChangedShellPost: a checktime reload (the live-sync path for agent writes) fires
  -- neither BufReadPost nor TextChanged, but the marks must repaint against the new content.
  vim.api.nvim_create_autocmd(
    { "BufReadPost", "BufWritePost", "InsertLeave", "TextChanged", "FileChangedShellPost" },
    {
      group = grp,
      callback = function(a)
        M.refresh(a.buf)
      end,
    }
  )
  -- User-driven navigation (tag jump, C-o/C-i, :e) lands on buffers whose stamped
  -- presentation may belong to the other view — or to none. Re-derive it from the active
  -- view so a jump in All files never shows a locked, painted buffer and a jump in Changes
  -- never shows an editable one. Named file buffers only; the rd split keeps its lift.
  vim.api.nvim_create_autocmd("BufEnter", {
    group = grp,
    callback = function(a)
      local view = M._view
      if not view or vim.bo[a.buf].buftype ~= "" or vim.api.nvim_buf_get_name(a.buf) == "" then
        return
      end
      local want_plain = view == "plain"
      local locked = vim.bo[a.buf].modifiable == false or vim.b[a.buf].reviewr_split_active
      if vim.b[a.buf].reviewr_plain == want_plain and want_plain ~= locked then
        return -- already presented under this view
      end
      vim.b[a.buf].reviewr_plain = want_plain
      if want_plain then
        M.unlock(a.buf)
      else
        M.lock(a.buf)
      end
      M.refresh(a.buf)
    end,
  })
end

return M
