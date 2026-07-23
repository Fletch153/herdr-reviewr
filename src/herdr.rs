//! herdr host integration: resolve the agent pane and send to it.
//!
//! See `specs/herdr-host.md`. Uses the herdr CLI via `$HERDR_BIN_PATH`. Only the
//! agent-send export depends on this module; browsing and clipboard do not.

use std::env;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde_json::Value;

pub(crate) fn herdr_bin() -> String {
    env::var("HERDR_BIN_PATH").unwrap_or_else(|_| "herdr".to_string())
}

fn herdr(args: &[&str]) -> Result<String> {
    let out = Command::new(herdr_bin())
        .args(args)
        .output()
        .with_context(|| format!("running herdr {args:?}"))?;
    if !out.status.success() {
        // herdr ≥0.7.5 fails with a non-zero exit AND the JSON error envelope on stdout
        // (stderr stays empty — e.g. `pane_not_found`), so the envelope carries the message;
        // usage errors and older hosts speak on stderr. Surface whichever does.
        if let Err(e) = ok_or_api_error(String::from_utf8_lossy(&out.stdout).into_owned()) {
            bail!("herdr {args:?} failed: {e}");
        }
        bail!("herdr {args:?} failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    ok_or_api_error(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Surface a JSON `{"error": {...}}` envelope on stdout as the failure it is; pass anything else
/// through unchanged (not every call returns JSON — `pane send-text` prints nothing on success).
/// Kept on the success path too: pre-0.7.5 hosts reported failures this way *with a zero exit*
/// (e.g. a Send to a stale pane returned `agent_not_found` and still exited 0), so the exit
/// status alone has never been trustworthy.
pub(crate) fn ok_or_api_error(stdout: String) -> Result<String> {
    if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(&stdout)
        && let Some(err) = obj.get("error")
    {
        let msg = err
            .get("message")
            .and_then(Value::as_str)
            .map_or_else(|| err.to_string(), str::to_owned);
        bail!("{msg}");
    }
    Ok(stdout)
}

/// The (tab, workspace, pane) id trio identifying this sidebar in the herdr environment.
fn agent_env() -> (Option<String>, Option<String>, Option<String>) {
    (
        env::var("HERDR_TAB_ID").ok(),
        env::var("HERDR_WORKSPACE_ID").ok(),
        env::var("HERDR_PANE_ID").ok(),
    )
}

/// The pane the sidebar was opened from — the agent the user was on at toggle time. herdr passes
/// it as `focused_pane_id` inside `HERDR_PLUGIN_CONTEXT_JSON`. This is the tie-breaker when a tab
/// holds more than one agent: Send goes to the one the sidebar reviews, not "ambiguous, give up".
fn focused_pane_id() -> Option<String> {
    parse_focused_pane(&env::var("HERDR_PLUGIN_CONTEXT_JSON").ok()?)
}

/// `focused_pane_id` out of the plugin context JSON. Split out from [`focused_pane_id`] so the
/// parse is testable without the environment.
fn parse_focused_pane(context_json: &str) -> Option<String> {
    serde_json::from_str::<Value>(context_json)
        .ok()?
        .get("focused_pane_id")
        .and_then(Value::as_str)
        .map(String::from)
}

/// The agents herdr currently lists. The one place the `agent list` call and its envelope
/// parsing live, shared by pane and status resolution.
fn agent_list() -> Result<Vec<Value>> {
    parse_agents(&herdr(&["agent", "list"])?)
}

/// The agent pane to send to: the agent in this tab, else the sole workspace agent.
///
/// Returns an error when no agent resolves, or when the choice is ambiguous
/// (two agents and none shares the tab).
pub fn resolve_agent_pane() -> Result<String> {
    let (tab, ws, me) = agent_env();
    let focused = focused_pane_id();
    pick_agent_pane(
        &agent_list()?,
        focused.as_deref(),
        tab.as_deref(),
        ws.as_deref(),
        me.as_deref(),
    )
    .context("no unambiguous agent in this tab or workspace")
}

/// The agents array from `herdr agent list`. The CLI's exact envelope is not pinned
/// by the spike notes, so accept a bare array, `result.agents`, or `agents`.
fn parse_agents(json: &str) -> Result<Vec<Value>> {
    let value: Value = serde_json::from_str(json).context("parsing agent list")?;
    if let Some(array) = value.as_array() {
        return Ok(array.clone());
    }
    value
        .get("result")
        .and_then(|r| r.get("agents"))
        .or_else(|| value.get("agents"))
        .and_then(Value::as_array)
        .cloned()
        .context("agent list has no agents array")
}

/// The resolved agent's `agent_status` (`idle`/`working`/`blocked`/`done`/`unknown`), for
/// turn tracking (`specs/herdr-host.md`). `Ok(None)` when no agent resolves, so the caller
/// treats an absent or ambiguous agent the same as a missing herdr — turn tracking pauses.
pub fn resolved_agent_status() -> Result<Option<String>> {
    let (tab, ws, me) = agent_env();
    let focused = focused_pane_id();
    Ok(pick_agent(&agent_list()?, focused.as_deref(), tab.as_deref(), ws.as_deref(), me.as_deref())
        .and_then(|a| a.get("agent_status").and_then(Value::as_str).map(String::from)))
}

/// The pane to send to: the sidebar's own agent, else the unique agent in this tab, else the
/// sole workspace agent.
fn pick_agent_pane(
    agents: &[Value],
    focused: Option<&str>,
    tab: Option<&str>,
    ws: Option<&str>,
    me: Option<&str>,
) -> Option<String> {
    pick_agent(agents, focused, tab, ws, me).and_then(pane_id)
}

/// The agent the sidebar was opened from (`focused`), else the unique agent in this tab, else the
/// sole workspace agent. Preferring `focused` disambiguates a tab with several agents — Send goes
/// to the one this sidebar reviews. `me` is our own pane, excluded throughout — herdr lists the
/// reviewr sidebar as an agent, so without this the real agent looks ambiguous in our own tab.
fn pick_agent<'a>(
    agents: &'a [Value],
    focused: Option<&str>,
    tab: Option<&str>,
    ws: Option<&str>,
    me: Option<&str>,
) -> Option<&'a Value> {
    agent_by_pane(agents, focused, me)
        .or_else(|| sole_agent(agents, "tab_id", tab, me))
        .or_else(|| sole_agent(agents, "workspace_id", ws, me))
}

/// The real agent whose `pane_id` is `want` (the sidebar's originating agent), never our own pane
/// `me`. `None` when `want` is absent, is `me`, or names a pane that is not a live agent — so a
/// stale focus hint falls through to tab/workspace resolution rather than misfiring.
fn agent_by_pane<'a>(
    agents: &'a [Value],
    want: Option<&str>,
    me: Option<&str>,
) -> Option<&'a Value> {
    let want = want?;
    if Some(want) == me {
        return None;
    }
    agents.iter().find(|a| a.get("agent").is_some() && pane_id(a).as_deref() == Some(want))
}

/// The unique agent whose `key` equals `want`, ignoring our own pane `me`; `None` if zero
/// or many remain.
fn sole_agent<'a>(
    agents: &'a [Value],
    key: &str,
    want: Option<&str>,
    me: Option<&str>,
) -> Option<&'a Value> {
    let want = want?;
    let mut matches = agents
        .iter()
        // Only real agents are send targets — herdr also lists plain shell/editor panes (no
        // `agent` field, `agent_status` "unknown"), which must never be picked or counted toward
        // ambiguity, or a Send lands in an editor instead of the agent's input.
        .filter(|a| a.get("agent").is_some())
        .filter(|a| a.get(key).and_then(Value::as_str) == Some(want))
        .filter(|a| pane_id(a).as_deref() != me);
    let first = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some(first)
}

/// The `pane_id` of an agent entry.
fn pane_id(agent: &Value) -> Option<String> {
    agent.get("pane_id").and_then(Value::as_str).map(String::from)
}

/// The `pane_id` of the unique agent for `key`/`want`, ignoring `me`; a thin pane-returning
/// wrapper over [`sole_agent`].
#[cfg(test)]
fn sole_pane(agents: &[Value], key: &str, want: Option<&str>, me: Option<&str>) -> Option<String> {
    sole_agent(agents, key, want, me).and_then(pane_id)
}

/// Write literal text into the agent pane's input, without submitting. herdr ≥0.7.5 spells this
/// `pane send-text` — 0.7.5 removed the old `agent send` (its `send-keys` replacement takes key
/// names only, and `agent prompt` would submit, which Send must never do).
pub fn send_text(pane: &str, text: &str) -> Result<()> {
    herdr(&["pane", "send-text", pane, text])?;
    Ok(())
}

/// Focus the agent pane so the reviewer can add context and submit.
pub fn focus(pane: &str) -> Result<()> {
    herdr(&["agent", "focus", pane])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ok_or_api_error, parse_agents, parse_focused_pane, pick_agent_pane, sole_pane};
    use serde_json::{Value, json};

    #[test]
    fn the_focused_pane_agent_wins_over_an_ambiguous_tab() {
        // Two real Claude agents share the tab; the sidebar was opened from p2, so p2 wins
        // instead of the resolution giving up as ambiguous.
        let agents = vec![agent("w8:p1", "w8:t1", "w8"), agent("w8:p2", "w8:t1", "w8")];
        assert_eq!(
            pick_agent_pane(&agents, Some("w8:p2"), Some("w8:t1"), Some("w8"), None),
            Some("w8:p2".to_string())
        );
        // With no focus hint the tab is still ambiguous — unchanged behavior.
        assert_eq!(pick_agent_pane(&agents, None, Some("w8:t1"), Some("w8"), None), None);
    }

    #[test]
    fn a_stale_or_self_focus_hint_falls_back_to_tab_resolution() {
        let agents = vec![agent("w8:p1", "w8:t1", "w8")];
        // Focus names a pane no longer listed -> fall back to the sole tab agent.
        assert_eq!(
            pick_agent_pane(&agents, Some("w8:gone"), Some("w8:t1"), Some("w8"), None),
            Some("w8:p1".to_string())
        );
        // Focus equal to our own pane is never targeted -> fall back.
        assert_eq!(
            pick_agent_pane(&agents, Some("w8:p1"), Some("w8:t1"), Some("w8"), Some("w8:p1")),
            None
        );
    }

    #[test]
    fn parse_focused_pane_reads_the_plugin_context() {
        let ctx = r#"{"workspace_id":"wY","tab_id":"wY:t1","focused_pane_id":"wY:pD","focused_pane_agent":"claude"}"#;
        assert_eq!(parse_focused_pane(ctx), Some("wY:pD".to_string()));
        assert_eq!(parse_focused_pane("{}"), None);
        assert_eq!(parse_focused_pane("not json"), None);
    }

    #[test]
    fn an_error_envelope_fails_even_though_herdr_exits_zero() {
        // Pre-0.7.5 hosts returned this on stdout WITH a zero exit; ≥0.7.5 pairs the same
        // envelope with a non-zero exit. Either way the envelope is the failure.
        let err = r#"{"error":{"code":"agent_not_found","message":"agent target x not found"},"id":"cli:agent:send"}"#;
        let e = ok_or_api_error(err.to_string()).unwrap_err();
        assert!(e.to_string().contains("not found"), "surfaces the herdr message: {e}");
        // A normal result envelope and any non-JSON output pass through unchanged.
        let ok = r#"{"result":{"agents":[]},"type":"agent_list"}"#;
        assert_eq!(ok_or_api_error(ok.to_string()).unwrap(), ok);
        assert_eq!(ok_or_api_error("plain".to_string()).unwrap(), "plain");
    }

    #[test]
    fn a_non_agent_pane_in_the_tab_is_never_a_send_target() {
        // A plain shell/editor pane (no `agent` field) shares our tab beside the real agent.
        let code = json!({ "agent_status": "unknown", "pane_id": "w8:p9", "tab_id": "w8:t1", "workspace_id": "w8" });
        let agents = vec![agent("w8:p1", "w8:t1", "w8"), code];
        // It is ignored, so the real agent still resolves rather than looking ambiguous.
        assert_eq!(
            pick_agent_pane(&agents, None, Some("w8:t1"), Some("w8"), None),
            Some("w8:p1".to_string())
        );
    }

    /// One agent entry shaped like the real `herdr agent list` output (api notes).
    fn agent(pane: &str, tab: &str, ws: &str) -> Value {
        json!({
            "agent": "claude",
            "agent_status": "working",
            "cwd": "/repo",
            "pane_id": pane,
            "tab_id": tab,
            "workspace_id": ws,
            "focused": true
        })
    }

    #[test]
    fn sole_pane_picks_unique_match() {
        let agents = vec![agent("w8:p1", "w8:t1", "w8"), agent("w9:p1", "w9:t1", "w9")];
        assert_eq!(sole_pane(&agents, "tab_id", Some("w8:t1"), None), Some("w8:p1".to_string()));
        assert_eq!(sole_pane(&agents, "tab_id", Some("nope"), None), None);
    }

    #[test]
    fn sole_pane_is_none_when_ambiguous() {
        let agents = vec![agent("w8:p1", "w8:t1", "w8"), agent("w8:p2", "w8:t2", "w8")];
        assert_eq!(sole_pane(&agents, "workspace_id", Some("w8"), None), None);
    }

    #[test]
    fn the_reviewr_pane_excludes_itself_so_the_real_agent_resolves() {
        // herdr lists our own sidebar pane (w8:p5) as an agent alongside the real one (w8:p1),
        // both in our tab. Excluding our pane leaves the real agent unambiguous.
        let agents = vec![agent("w8:p1", "w8:t1", "w8"), agent("w8:p5", "w8:t1", "w8")];
        assert_eq!(
            pick_agent_pane(&agents, None, Some("w8:t1"), Some("w8"), Some("w8:p5")),
            Some("w8:p1".to_string())
        );
    }

    #[test]
    fn parse_agents_accepts_bare_array_and_result_envelope() {
        let a = agent("w8:p1", "w8:t1", "w8");
        let bare = json!([a]).to_string();
        assert_eq!(parse_agents(&bare).unwrap().len(), 1);
        let wrapped =
            json!({ "result": { "agents": [agent("w8:p1", "w8:t1", "w8")] } }).to_string();
        assert_eq!(parse_agents(&wrapped).unwrap().len(), 1);
    }

    #[test]
    fn pick_prefers_the_tab_agent_over_the_workspace() {
        let agents = vec![agent("w8:p1", "w8:t1", "w8"), agent("w8:p2", "w8:t2", "w8")];
        // Both share workspace w8; our tab is w8:t2, so its pane wins.
        assert_eq!(
            pick_agent_pane(&agents, None, Some("w8:t2"), Some("w8"), None),
            Some("w8:p2".to_string())
        );
    }

    #[test]
    fn pick_falls_back_to_the_sole_workspace_agent() {
        let agents = vec![agent("w8:p1", "w8:t1", "w8")];
        // No agent shares our tab, but exactly one is in the workspace.
        assert_eq!(
            pick_agent_pane(&agents, None, Some("w8:tX"), Some("w8"), None),
            Some("w8:p1".to_string())
        );
    }

    #[test]
    fn pick_is_none_when_the_workspace_is_ambiguous() {
        let agents = vec![agent("w8:p1", "w8:t1", "w8"), agent("w8:p2", "w8:t2", "w8")];
        // Neither shares our tab and the workspace has two — refuse to guess.
        assert_eq!(pick_agent_pane(&agents, None, Some("w8:tZ"), Some("w8"), None), None);
    }
}
