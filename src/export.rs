//! Formatting comments and exporting them to the agent or clipboard.
//!
//! See `specs/review-model.md`. The review is sent as a tagged `<review>` block: an
//! instruction preamble, then one numbered `<comment>` per note carrying a `<ref>`
//! location, the `<code>` snippet, and the reviewer's `<note>`. The agent is asked to
//! resolve each and report a status table.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::herdr;
use crate::model::Comment;

/// The instruction that leads the review, telling the agent to resolve each comment and report.
const PREAMBLE: &str = "The user has left the following review comments. Please carefully \
consider and resolve each one. When done, print a compact status table (#, location, status, \
resolution) — for each comment give a 1–2 line resolution of what you changed, or a short \
answer if it was a question, or a brief note with context if it needs a follow-up. Keep it \
short and concise. When a comment has a <base>, its <code> is a unified-diff hunk (the +/- \
lines) taken against that git ref — run `git diff <base> -- <file>` for the full change; a \
comment without a <base> is plain file content.";

/// One comment as its tagged block: the numbered `<comment>` with `<ref>`, an optional `<base>`
/// (the git ref a Changes hunk is diffed against, so the agent can reproduce it), `<code>`, and
/// `<note>`.
pub fn format_comment(n: usize, comment: &Comment) -> String {
    let base = if comment.diff_anchored {
        comment.base.as_deref().map_or_else(String::new, |b| format!("<base>{b}</base>\n"))
    } else {
        String::new()
    };
    format!(
        "<comment n=\"{n}\">\n<ref>{}</ref>\n{base}<code>\n{}\n</code>\n<note>{}</note>\n</comment>",
        comment.location(),
        comment.lines,
        normalize_text(&comment.text),
    )
}

/// Comment text for export: drop `\r`, trim trailing space per line, and drop blank
/// lines so a multi-line comment stays a compact note inside its `<note>` tag.
fn normalize_text(text: &str) -> String {
    text.replace('\r', "")
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The whole review: the preamble and every comment (sorted by file then start line, numbered
/// from 1) wrapped in a `<review>` container.
pub fn format_all(comments: &[&Comment]) -> String {
    let mut sorted = comments.to_vec();
    sorted.sort_by(|a, b| a.file.cmp(&b.file).then(a.start.cmp(&b.start)));
    let blocks = sorted
        .iter()
        .enumerate()
        .map(|(i, c)| format_comment(i + 1, c))
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("<review>\n{PREAMBLE}\n\n{blocks}\n</review>")
}

/// A destination comments can be exported to. Export succeeds or errors as a whole.
pub trait ExportTarget {
    fn export(&self, text: &str) -> Result<()>;
    fn label(&self) -> &'static str;
    /// Whether dispatching to this target marks the sent comments as sent (so the next Send skips
    /// them). True for the agent; false for the clipboard, which is a manual grab of everything.
    fn marks_sent(&self) -> bool {
        true
    }
}

/// A clipboard tool and the args that make it read stdin into the system clipboard. Tried in
/// order — the first one present on `PATH` wins. macOS ships `pbcopy`; Linux needs one of these
/// installed (Wayland `wl-copy`, or X11 `xclip`/`xsel`). OSC 52 and Windows are roadmap.
const CLIPBOARD_TOOLS: &[(&str, &[&str])] = &[
    ("pbcopy", &[]),
    ("wl-copy", &[]),
    ("xclip", &["-selection", "clipboard"]),
    ("xsel", &["--clipboard", "--input"]),
];

/// The system clipboard, via the first available platform clipboard tool.
#[derive(Debug)]
pub struct Clipboard;

impl ExportTarget for Clipboard {
    fn label(&self) -> &'static str {
        "clipboard"
    }

    fn marks_sent(&self) -> bool {
        false
    }

    fn export(&self, text: &str) -> Result<()> {
        // A local clipboard tool wins when present; without one (typical on a remote or
        // headless box, where a tool could only reach the box's own selection anyway) the
        // text goes out as OSC 52 — the terminal at the other end owns the real clipboard.
        let Some((cmd, args)) = select_tool(CLIPBOARD_TOOLS, crate::proc::on_path) else {
            return osc52_copy(text);
        };
        let mut child = Command::new(cmd)
            .args(args)
            .stdin(Stdio::piped())
            .spawn()
            .with_context(|| format!("spawning {cmd}"))?;
        child
            .stdin
            .as_mut()
            .with_context(|| format!("{cmd} stdin unavailable"))?
            .write_all(text.as_bytes())
            .with_context(|| format!("writing to {cmd}"))?;
        if !child.wait().with_context(|| format!("waiting for {cmd}"))?.success() {
            bail!("{cmd} exited non-zero");
        }
        Ok(())
    }
}

/// Write `text` to the terminal clipboard as an OSC 52 sequence on stdout. Write-only (no
/// terminal reply needed), so it works from a TUI in raw mode and travels through SSH and
/// multiplexers that pass OSC 52 on.
pub fn osc52_copy(text: &str) -> Result<()> {
    let mut out = std::io::stdout().lock();
    write!(out, "\x1b]52;c;{}\x07", base64(text.as_bytes())).context("writing OSC 52")?;
    out.flush().context("flushing OSC 52")?;
    Ok(())
}

/// Standard base64 (RFC 4648 with padding) — hand-rolled to keep the dependency tree flat.
fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// The first clipboard tool the `present` predicate accepts, preserving list order.
fn select_tool(
    tools: &'static [(&'static str, &'static [&'static str])],
    present: impl Fn(&str) -> bool,
) -> Option<(&'static str, &'static [&'static str])> {
    tools.iter().copied().find(|(cmd, _)| present(cmd))
}

/// The agent pane: fill its input via `herdr agent send`, then focus it.
#[derive(Debug)]
pub struct Agent;

impl ExportTarget for Agent {
    fn label(&self) -> &'static str {
        "agent"
    }

    fn export(&self, text: &str) -> Result<()> {
        let pane = herdr::resolve_agent_pane()?;
        herdr::send_text(&pane, text)?;
        // Focus is a convenience once the text is delivered; a focus failure must NOT fail the
        // export, or the comments stay unconsumed and the next Send duplicates the whole review.
        let _ = herdr::focus(&pane);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{CLIPBOARD_TOOLS, base64, format_all, format_comment, select_tool};
    use crate::model::{Comment, Scope, Side};

    #[test]
    fn base64_matches_rfc4648_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64("héllo\n".as_bytes()), "aMOpbGxvCg==");
    }

    #[test]
    fn clipboard_tool_selection_prefers_list_order_and_can_be_empty() {
        // None present -> no tool (the caller surfaces the "install one" error).
        assert!(select_tool(CLIPBOARD_TOOLS, |_| false).is_none());
        // Only an X11 tool present -> it's chosen, with its selection args.
        assert_eq!(
            select_tool(CLIPBOARD_TOOLS, |c| c == "xclip"),
            Some(("xclip", &["-selection", "clipboard"][..]))
        );
        // When several are present, earlier in the list wins (pbcopy over xclip).
        assert_eq!(
            select_tool(CLIPBOARD_TOOLS, |c| c == "pbcopy" || c == "xclip").map(|(cmd, _)| cmd),
            Some("pbcopy")
        );
    }

    fn comment(file: &str, side: Side, start: u32, end: u32, lines: &str, text: &str) -> Comment {
        Comment {
            file: file.into(),
            side,
            start,
            end,
            lines: lines.into(),
            text: text.into(),
            diff_anchored: true,
            scope: Scope::Commit,
            base: None,
            sent: false,
        }
    }

    #[test]
    fn block_tags_the_ref_code_and_note() {
        let c = comment(
            "extruct/core/llm_registry.py",
            Side::New,
            40,
            41,
            "from .x import y\nregister(y)",
            "this import path looks wrong",
        );
        assert_eq!(
            format_comment(1, &c),
            "<comment n=\"1\">\n<ref>extruct/core/llm_registry.py:40-41</ref>\n\
             <code>\nfrom .x import y\nregister(y)\n</code>\n\
             <note>this import path looks wrong</note>\n</comment>"
        );
    }

    #[test]
    fn a_changes_comment_carries_its_diff_base() {
        let mut c = comment("f.rs", Side::New, 1, 2, " ctx\n+new", "why?");
        c.base = Some("abc1234def".into());
        let out = format_comment(1, &c);
        assert!(out.contains("<base>abc1234def</base>"), "the diff base ref is included: {out}");
    }

    #[test]
    fn an_all_files_comment_omits_the_base() {
        let mut c = comment("f.rs", Side::New, 1, 1, "plain line", "note");
        c.diff_anchored = false; // File-view (All files) content, not a diff
        c.base = Some("abc1234".into());
        let out = format_comment(1, &c);
        assert!(!out.contains("<base>"), "plain content has no diff base: {out}");
    }

    #[test]
    fn removed_side_marks_the_ref() {
        let c = comment("a.rs", Side::Old, 38, 38, "    cleanup()", "still needed");
        assert_eq!(
            format_comment(2, &c),
            "<comment n=\"2\">\n<ref>a.rs:38 (removed)</ref>\n\
             <code>\n    cleanup()\n</code>\n<note>still needed</note>\n</comment>"
        );
    }

    #[test]
    fn multiline_text_keeps_breaks_but_drops_blank_lines() {
        let c = comment("a.rs", Side::New, 1, 1, "x", "first line\n\n  \nsecond line\n");
        assert_eq!(
            format_comment(1, &c),
            "<comment n=\"1\">\n<ref>a.rs:1</ref>\n<code>\nx\n</code>\n\
             <note>first line\nsecond line</note>\n</comment>"
        );
    }

    #[test]
    fn all_wraps_a_preamble_and_numbers_comments_sorted_by_file_then_start() {
        let b = comment("b.rs", Side::New, 5, 5, "+x", "two");
        let a2 = comment("a.rs", Side::New, 20, 20, "+y", "later");
        let a1 = comment("a.rs", Side::New, 3, 3, "+z", "earlier");
        let out = format_all(&[&b, &a2, &a1]);
        assert!(out.starts_with("<review>\n"), "opens with the review container: {out}");
        assert!(out.ends_with("\n</review>"), "closes the container: {out}");
        assert!(out.contains("resolve each one"), "carries the instruction preamble");
        // Sorted a.rs:3, a.rs:20, b.rs:5 → numbered 1, 2, 3.
        let n1 = out.find("n=\"1\"").unwrap();
        let n2 = out.find("n=\"2\"").unwrap();
        let n3 = out.find("n=\"3\"").unwrap();
        assert!(n1 < n2 && n2 < n3, "comments numbered in sorted order");
        assert!(out.find("<ref>a.rs:3</ref>").unwrap() < out.find("<ref>a.rs:20</ref>").unwrap());
        assert!(out.contains("<ref>b.rs:5</ref>"));
    }
}
