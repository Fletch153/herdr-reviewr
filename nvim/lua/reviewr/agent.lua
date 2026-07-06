-- Resolve the herdr agent pane and send text to it. A faithful port of `src/herdr.rs`: prefer the
-- pane the sidebar was opened from (`focused_pane_id`), else the sole agent in this tab, else the
-- sole agent in the workspace, excluding our own pane and non-agent panes. herdr exits 0 even on
-- failure (reporting a JSON `{"error":...}` envelope), so success is judged from the envelope.

local M = {}

local function herdr_bin()
  return vim.env.HERDR_BIN_PATH or "herdr"
end

-- Run `herdr <args>` synchronously. Returns (decoded_or_text, err): err is non-nil on a non-zero
-- exit or an `{"error":...}` envelope.
local function herdr(args)
  local cmd = { herdr_bin() }
  vim.list_extend(cmd, args)
  local res = vim.system(cmd, { text = true }):wait()
  if res.code ~= 0 then
    return nil, ("herdr exited %d: %s"):format(res.code, (res.stderr or ""):gsub("%s+$", ""))
  end
  local out = res.stdout or ""
  local ok, decoded = pcall(vim.json.decode, out)
  if ok and type(decoded) == "table" and decoded.error then
    local msg = type(decoded.error) == "table" and decoded.error.message or "herdr error"
    return nil, msg
  end
  return (ok and decoded) or out, nil
end

local function focused_pane_id()
  local ctx = vim.env.HERDR_PLUGIN_CONTEXT_JSON
  if not ctx then
    return nil
  end
  local ok, decoded = pcall(vim.json.decode, ctx)
  return ok and type(decoded) == "table" and decoded.focused_pane_id or nil
end

-- The agents array, accepting a bare array, `result.agents`, or `agents` (matches parse_agents).
local function agent_list()
  local decoded, err = herdr({ "agent", "list" })
  if err then
    return nil, err
  end
  local agents
  if vim.islist(decoded) then
    agents = decoded
  elseif type(decoded) == "table" then
    agents = (decoded.result and decoded.result.agents) or decoded.agents
  end
  if type(agents) ~= "table" then
    return nil, "agent list has no agents array"
  end
  return agents, nil
end

-- The real agent whose pane_id is `want` (the sidebar's originating agent), never our own `me`.
local function agent_by_pane(agents, want, me)
  if not want or want == me then
    return nil
  end
  for _, a in ipairs(agents) do
    if a.agent ~= nil and a.pane_id == want then
      return a
    end
  end
  return nil
end

-- The unique real agent whose `key` == `want`, excluding `me`; nil if zero or many.
local function sole_agent(agents, key, want, me)
  if not want then
    return nil
  end
  local found
  for _, a in ipairs(agents) do
    if a.agent ~= nil and a[key] == want and a.pane_id ~= me then
      if found then
        return nil
      end
      found = a
    end
  end
  return found
end

local function pick_agent(agents)
  local me, tab, ws = vim.env.HERDR_PANE_ID, vim.env.HERDR_TAB_ID, vim.env.HERDR_WORKSPACE_ID
  return agent_by_pane(agents, focused_pane_id(), me)
    or sole_agent(agents, "tab_id", tab, me)
    or sole_agent(agents, "workspace_id", ws, me)
end

-- Resolve the target agent's pane id, or (nil, err).
function M.resolve_pane()
  local agents, err = agent_list()
  if not agents then
    return nil, err
  end
  local a = pick_agent(agents)
  if not a then
    return nil, "no unambiguous agent in this tab or workspace"
  end
  return a.pane_id, nil
end

-- Fill the agent pane's input with `text` (without submitting) and focus it. Returns (true) or
-- (false, err) — err set when no agent resolves or herdr rejects the send.
function M.send(text)
  local pane, err = M.resolve_pane()
  if not pane then
    return false, err
  end
  local _, serr = herdr({ "agent", "send", pane, text })
  if serr then
    return false, serr
  end
  pcall(herdr, { "agent", "focus", pane }) -- focus is best-effort; never fails the send
  return true, nil
end

-- Exposed for the headless tests.
M._internal = { pick_agent = pick_agent, sole_agent = sole_agent, agent_by_pane = agent_by_pane }

return M
