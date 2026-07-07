-- The comment bridge. The reviewer (the host embedding this nvim) owns the single comment
-- store — the same store behind the built-in pane, the header's Send counter, and the `l`
-- comments list. This module is the editor half of the protocol:
--   editor → host: report comment intents (`rpcnotify`) with the anchor under the cursor or
--     visual range — the host opens its composer / targets the covered comment.
--   host → editor: `apply(items)` paints the store's cards for the open file as inline boxes
--     (virt_lines), styled like the reviewer's own comment cards.

local M = {}
local ns = vim.api.nvim_create_namespace("reviewr_comments")

-- The host's RPC channel: the attached UI (the reviewer). nil when nvim runs standalone or
-- headless (tests) — then notify() reports failure and apply() can still be driven directly.
local function chan()
  local uis = vim.api.nvim_list_uis()
  return uis[1] and uis[1].chan or nil
end

-- Report a comment intent to the host. Returns false when no host is attached.
function M.notify(action, payload)
  local c = chan()
  if not c or c == 0 then
    return false
  end
  vim.rpcnotify(c, "reviewr", action, payload or vim.empty_dict())
  return true
end

-- Path relative to the cwd — the reviewer spawns this nvim with cwd = the repo root, so this
-- IS the repo-relative path (the same assumption diff.lua's refresh makes). Deliberately no
-- `git rev-parse` fallback: the anchor runs on the keypress-to-composer critical path, and a
-- subprocess there widens the window in which typed keys still route to normal mode.
local function relpath(abs)
  return vim.fn.fnamemodify(abs, ":.")
end

-- What the current buffer's [lo, hi] anchors to, in the host's comment model: repo-relative
-- file, which diff side the lines live on, and the captured snippet. A `reviewr://deleted/`
-- scratch (worktree-missing file) anchors old-side — its buffer *is* the base content; every
-- other buffer anchors new-side at its own line numbers. Snippets mirror the built-in pane:
-- marker-prefixed on the Changes view (`+` inside a change hunk, space context, `-` on the
-- deleted scratch), plain code in the All-files view (`b:reviewr_plain`).
function M.anchor(lo, hi)
  local bufnr = vim.api.nvim_get_current_buf()
  local name = vim.api.nvim_buf_get_name(bufnr)
  if name == "" then
    return nil
  end
  lo = lo or vim.fn.line(".")
  hi = hi or lo
  if lo > hi then
    lo, hi = hi, lo
  end
  local deleted = name:match("^reviewr://deleted/(.+)$")
  local file = deleted or relpath(name)
  local text = vim.api.nvim_buf_get_lines(bufnr, lo - 1, hi, false)
  local plain = vim.b[bufnr].reviewr_plain
  local hunks = require("reviewr.diff")._hunks[bufnr] or {}
  local lines = {}
  for i, l in ipairs(text) do
    if deleted then
      lines[i] = "-" .. l
    elseif plain then
      lines[i] = l
    else
      local n = lo + i - 1
      local marker = " "
      for _, r in ipairs(hunks) do
        if n >= r.lo and n <= r.hi then
          marker = "+"
          break
        end
      end
      lines[i] = marker .. l
    end
  end
  return {
    file = file,
    side = deleted and "old" or "new",
    start = lo,
    ["end"] = hi,
    lines = table.concat(lines, "\n"),
  }
end

-- <leader>rc: ask the host to open its composer for a new comment on [lo, hi] (the cursor
-- line when no range).
function M.comment(lo, hi)
  local a = M.anchor(lo, hi)
  if not a then
    vim.notify("reviewr: this buffer has no file to comment on", vim.log.levels.WARN)
    return
  end
  if not M.notify("comment", a) then
    vim.notify("reviewr: no reviewer attached", vim.log.levels.WARN)
  end
end

-- The visual-mode rc: read the selection from the live marks and leave visual mode. Bound via
-- `<Cmd>` so it runs atomically on the keypress — a `:`-style map replays through the cmdline,
-- widening the window in which further typed keys still execute as normal-mode commands.
function M.comment_visual()
  local lo, hi = vim.fn.line("v"), vim.fn.line(".")
  vim.api.nvim_feedkeys(vim.api.nvim_replace_termcodes("<Esc>", true, false, true), "n", false)
  M.comment(math.min(lo, hi), math.max(lo, hi))
end

-- <leader>re / rx / rr: edit / delete / resolve the host's comment covering the cursor line.
function M.act(action)
  local a = M.anchor()
  if not a then
    vim.notify("reviewr: this buffer has no file to comment on", vim.log.levels.WARN)
    return
  end
  if not M.notify(action, { file = a.file, side = a.side, line = a.start }) then
    vim.notify("reviewr: no reviewer attached", vim.log.levels.WARN)
  end
end

-- Greedy display-cell wrap (comments are short; word-boundary niceties aren't worth the
-- dependency). Always returns at least one piece so an empty line still draws a box row.
local function wrap(s, width)
  if s == "" then
    return { "" }
  end
  local out, cur, w = {}, "", 0
  for _, ch in ipairs(vim.fn.split(s, "\\zs")) do
    local cw = vim.fn.strdisplaywidth(ch)
    if w + cw > width and cur ~= "" then
      out[#out + 1] = cur
      cur, w = "", 0
    end
    cur = cur .. ch
    w = w + cw
  end
  out[#out + 1] = cur
  return out
end

local function trunc(s, max)
  if vim.fn.strdisplaywidth(s) <= max then
    return s
  end
  local out = ""
  for _, ch in ipairs(vim.fn.split(s, "\\zs")) do
    if vim.fn.strdisplaywidth(out .. ch) > math.max(max - 1, 1) then
      break
    end
    out = out .. ch
  end
  return out .. "…"
end

-- The host pushed the open file's comments: paint each as the reviewer's inline card — a
-- quiet box titled with the comment's location (peach accent) spliced under the last anchored
-- line, plus the accent on the anchored line numbers. Old-side items paint only on the
-- deleted scratch, whose buffer shares their (base) line numbers. Clears first, so a push of
-- fewer/no items erases stale cards.
function M.apply(items)
  local bufnr = vim.api.nvim_get_current_buf()
  vim.api.nvim_buf_clear_namespace(bufnr, ns, 0, -1)
  if type(items) ~= "table" or #items == 0 then
    return
  end
  local buf_side = vim.api.nvim_buf_get_name(bufnr):match("^reviewr://deleted/") and "old"
    or "new"
  local last = vim.api.nvim_buf_line_count(bufnr)
  local win = vim.api.nvim_get_current_win()
  local info = vim.fn.getwininfo(win)[1] or {}
  local width = math.max((info.width or 80) - (info.textoff or 0), 20)
  local indent = (" "):rep(2)
  local box_w = math.max(width - 2, 10)
  local text_w = math.max(box_w - 4, 1)
  for _, c in ipairs(items) do
    local lo = math.min(math.max(c.start or 1, 1), last)
    local hi = math.min(math.max(c["end"] or lo, lo), last)
    if c.side == buf_side then
      for l = lo, hi do
        pcall(vim.api.nvim_buf_set_extmark, bufnr, ns, l - 1, 0, {
          number_hl_group = "ReviewrCommentLine",
        })
      end
    end
    local label = trunc((" comment · %s "):format(c.location or ""), box_w - 3)
    local fill = math.max(box_w - 3 - vim.fn.strdisplaywidth(label), 0)
    local virt = {
      {
        { indent .. "╭─", "ReviewrCardBorder" },
        { label, "ReviewrCardTitle" },
        { ("─"):rep(fill) .. "╮", "ReviewrCardBorder" },
      },
    }
    for _, logical in ipairs(vim.split(tostring(c.text or ""), "\n", { plain = true })) do
      for _, piece in ipairs(wrap(logical, text_w)) do
        local gap = (" "):rep(math.max(text_w - vim.fn.strdisplaywidth(piece), 0))
        virt[#virt + 1] = {
          { indent .. "│ ", "ReviewrCardBorder" },
          { piece, "ReviewrCardBody" },
          { gap .. " │", "ReviewrCardBorder" },
        }
      end
    end
    virt[#virt + 1] = { { indent .. "╰" .. ("─"):rep(box_w - 2) .. "╯", "ReviewrCardBorder" } }
    pcall(vim.api.nvim_buf_set_extmark, bufnr, ns, hi - 1, 0, {
      virt_lines = virt,
      virt_lines_above = false,
    })
  end
end

return M
