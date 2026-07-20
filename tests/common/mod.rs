#![allow(dead_code, unreachable_pub)]

use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

pub struct Repo {
    dir: TempDir,
}

impl Repo {
    pub fn init() -> Self {
        let repo = Self { dir: TempDir::new().expect("tempdir") };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.git(&["config", "user.email", "test@herdr.test"]);
        repo.git(&["config", "user.name", "Test"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn path_buf(&self) -> PathBuf {
        self.dir.path().to_path_buf()
    }

    pub fn git(&self, args: &[&str]) -> String {
        let out = Command::new("git").arg("-C").arg(self.path()).args(args).output().expect("git");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    pub fn write(&self, rel: &str, contents: &str) {
        let path = self.path().join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(path, contents).expect("write");
    }

    pub fn remove(&self, rel: &str) {
        std::fs::remove_file(self.path().join(rel)).expect("remove");
    }

    pub fn commit_all(&self, message: &str) {
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", message]);
    }
}

use herdr_reviewr::forge::{
    Check, CheckStatus, Comment, CommentKind, Merge, PrSnapshot, PrState, PrView, Sync,
};

/// A PR comment for navigator fixtures; `Finding`-kind with a `path:line` anchor.
pub fn pr_comment(author: &str, anchor: &str, body: &str) -> Comment {
    Comment {
        kind: CommentKind::Finding,
        author: author.into(),
        author_is_bot: false,
        anchor: anchor.into(),
        body: body.into(),
        snippet: None,
        created_at: format!("2026-07-19T12:00:0{}Z", body.len() % 10),
        is_resolved: false,
        is_outdated: false,
        reply_count: 0,
    }
}

/// An open, in-sync PR snapshot view with the given checks and comments.
pub fn pr_view(checks: Vec<Check>, comments: Vec<Comment>) -> PrView {
    PrView::Pr(Box::new(PrSnapshot {
        number: 7,
        title: "fixture pr".into(),
        url: "https://example.invalid/pr/7".into(),
        state: PrState::Open,
        is_draft: false,
        base_ref: "main".into(),
        merge: Merge::Clean,
        sync: Sync::InSync,
        checks,
        comments,
        truncated: false,
    }))
}

pub fn pr_check(name: &str, status: CheckStatus, url: Option<&str>) -> Check {
    Check { name: name.into(), status, url: url.map(String::from) }
}
