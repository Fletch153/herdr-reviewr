-- Format review comments as the tagged `<review>` block the agent consumes. A port of
-- `src/export.rs`: the preamble, then one numbered `<comment>` per note with `<ref>`, `<code>`,
-- and `<note>`. v1 sends plain file content (no `<base>` diff hunk).

local M = {}

local PREAMBLE = [[The user has left the following review comments. Please carefully consider and resolve each one. When done, print a compact status table (#, location, status, resolution) — for each comment give a 1–2 line resolution of what you changed, or a short answer if it was a question, or a brief note with context if it needs a follow-up. Keep it short and concise. When a comment has a <base>, its <code> is a unified-diff hunk (the +/- lines) taken against that git ref — run `git diff <base> -- <file>` for the full change; a comment without a <base> is plain file content.]]

-- Comment note for export: drop \r, trim trailing space, and drop blank lines (mirrors
-- export.rs::normalize_text) so a multi-line note stays compact inside <note>.
local function normalize_note(text)
  local kept = {}
  for line in (text:gsub("\r", "") .. "\n"):gmatch("(.-)\n") do
    local trimmed = line:gsub("%s+$", "")
    if trimmed:gsub("%s", "") ~= "" then
      kept[#kept + 1] = trimmed
    end
  end
  return table.concat(kept, "\n")
end

-- `file:line` or `file:lo-hi` for a range.
local function location(c)
  if c.hi and c.hi > c.lo then
    return ("%s:%d-%d"):format(c.file, c.lo, c.hi)
  end
  return ("%s:%d"):format(c.file, c.lo)
end

function M.format_comment(n, c)
  return ("<comment n=\"%d\">\n<ref>%s</ref>\n<code>\n%s\n</code>\n<note>%s</note>\n</comment>")
    :format(n, location(c), c.code, normalize_note(c.note))
end

-- The whole review: preamble + every comment (sorted by file then start line, numbered from 1)
-- inside a <review> container.
function M.format_all(comments)
  local sorted = vim.deepcopy(comments)
  table.sort(sorted, function(a, b)
    if a.file ~= b.file then
      return a.file < b.file
    end
    return a.lo < b.lo
  end)
  local blocks = {}
  for i, c in ipairs(sorted) do
    blocks[#blocks + 1] = M.format_comment(i, c)
  end
  return ("<review>\n%s\n\n%s\n</review>"):format(PREAMBLE, table.concat(blocks, "\n\n"))
end

return M
