//! Read-only git access: scopes, changed files, and diffs.
//!
//! See `specs/review-model.md`. Every call here only reads — it never commits,
//! stages, or mutates the worktree or refs.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::model::{ChangeKind, ChangedFile, FileStatus, Scope};

/// Run `git -C <repo> <args>` and return stdout. Errors on non-zero exit.
fn git(repo: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "core.quotepath=false"])
        .args(args)
        .output()
        .with_context(|| format!("running git {args:?}"))?;
    if !out.status.success() {
        bail!("git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Like [`git`], but returns stdout even on non-zero exit (e.g. `diff --no-index`).
fn git_lenient(repo: &Path, args: &[&str]) -> String {
    Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "core.quotepath=false"])
        .args(args)
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

/// Run `git -C <repo> <args>` and return its trimmed stdout, or `None` if the command fails to
/// spawn, exits non-zero, or prints nothing. The one-line query workhorse for `rev-parse`/`merge-base`.
fn git_line(repo: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(repo).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let line = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!line.is_empty()).then_some(line)
}

/// Whether `git -C <repo> <args>` spawns and exits zero. The predicate workhorse for existence checks.
fn git_ok(repo: &Path, args: &[&str]) -> bool {
    Command::new("git").arg("-C").arg(repo).args(args).output().is_ok_and(|o| o.status.success())
}

/// Stage `path` into the index (`git add`). Returns whether git succeeded.
pub fn stage(repo: &Path, path: &str) -> bool {
    git_ok(repo, &["add", "--", path])
}

/// Unstage `path` from the index (`git reset`, leaving the working tree untouched).
pub fn unstage(repo: &Path, path: &str) -> bool {
    git_ok(repo, &["reset", "-q", "--", path])
}

/// Whether `path` is inside a git work tree.
pub fn is_repo(path: &Path) -> bool {
    git_ok(path, &["rev-parse", "--is-inside-work-tree"])
}

/// The git top-level of `path`, or `None` if it is not a repo.
pub fn toplevel(path: &Path) -> Option<PathBuf> {
    git_line(path, &["rev-parse", "--show-toplevel"]).map(PathBuf::from)
}

/// The current branch name, or `None` on a detached HEAD or non-repo. Used to resolve the PR
/// for the worktree's branch (`specs/forge-host.md`).
pub fn current_branch(repo: &Path) -> Option<String> {
    git_line(repo, &["rev-parse", "--abbrev-ref", "HEAD"]).filter(|b| b != "HEAD")
}

/// The full SHA of `HEAD` (the newest commit), or `None` on an unborn branch or non-repo.
pub fn head_commit(repo: &Path) -> Option<String> {
    git_line(repo, &["rev-parse", "HEAD"])
}

/// The `(owner, name)` of the worktree's `origin` if it is a GitHub remote, else `None`. Read
/// locally so the PR fetch needs no `gh repo view` round-trip (`specs/forge-host.md`).
pub fn github_slug(repo: &Path) -> Option<(String, String)> {
    parse_github_slug(&git_line(repo, &["remote", "get-url", "origin"])?)
}

/// `(owner, name)` from a remote URL pointing at github.com, else `None`. Splits the URL into
/// host and path so an SSH host alias or a trailing slash can't bleed into the parsed name.
/// Accepts the `github.com-<alias>` host form that multi-account `~/.ssh/config` setups use.
fn parse_github_slug(url: &str) -> Option<(String, String)> {
    let (host, path) = split_remote(url)?;
    if host != "github.com" && !host.starts_with("github.com-") {
        return None;
    }
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, name) = path.split_once('/')?;
    // A GitHub repo path is exactly `owner/name`; drop anything past it defensively.
    let name = name.split('/').next().unwrap_or(name);
    (!owner.is_empty() && !name.is_empty()).then(|| (owner.to_string(), name.to_string()))
}

/// Split a git remote URL into `(host, path)` for the two forms git emits: a scheme URL
/// (`https://`, `ssh://`, `git://`, with optional `user@` and `:port`) and the scp-like
/// `user@host:path`.
fn split_remote(url: &str) -> Option<(&str, &str)> {
    if let Some((_, rest)) = url.split_once("://") {
        let rest = rest.split_once('@').map_or(rest, |(_, r)| r); // drop `user@`
        let (hostport, path) = rest.split_once('/')?;
        let host = hostport.split(':').next().unwrap_or(hostport); // drop `:port`
        Some((host, path))
    } else {
        // scp-like `[user@]host:path` — the first `:` splits host from path.
        let (hostpart, path) = url.split_once(':')?;
        let host = hostpart.split_once('@').map_or(hostpart, |(_, h)| h);
        Some((host, path))
    }
}

/// Commits local `HEAD` is ahead and behind `other` (a commit-ish), or `None` if `other` is
/// not present locally. Backs the PR `sync` indicator (`specs/forge-host.md`).
pub fn ahead_behind(repo: &Path, other: &str) -> Option<(u32, u32)> {
    let out = git_line(repo, &["rev-list", "--left-right", "--count", &format!("HEAD...{other}")])?;
    let mut it = out.split_whitespace();
    let ahead = it.next()?.parse().ok()?;
    let behind = it.next()?.parse().ok()?;
    Some((ahead, behind))
}

/// Whether `git_ref` resolves in `repo`.
fn ref_exists(repo: &Path, git_ref: &str) -> bool {
    git_ok(repo, &["rev-parse", "--verify", "--quiet", git_ref])
}

pub fn base_ref(repo: &Path, base: Option<&str>) -> Option<String> {
    if let Some(b) = base
        && !b.is_empty()
        && ref_exists(repo, b)
    {
        return Some(b.to_string());
    }
    if let Some(near) = nearest_ancestor_branch(repo) {
        return Some(near);
    }
    if let Some(head) =
        git_line(repo, &["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"])
        && ref_exists(repo, &head)
    {
        return Some(head);
    }
    ["origin/main", "origin/master", "main", "master"]
        .into_iter()
        .find(|cand| ref_exists(repo, cand))
        .map(String::from)
}

pub fn nearest_ancestor_branch(repo: &Path) -> Option<String> {
    let own_twin = current_branch(repo).map(|b| format!("origin/{b}"));
    lineage_branches(repo)
        .into_iter()
        .find(|(dist, name, _)| *dist >= 1 && Some(name.as_str()) != own_twin.as_deref())
        .map(|(_, name, _)| name)
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AncestorBranches {
    pub local: Vec<String>,
    pub remote: Vec<String>,
}

pub fn ancestor_branches(repo: &Path) -> AncestorBranches {
    let rows = lineage_branches(repo);
    let names = |local: bool| {
        rows.iter().filter(|(_, _, l)| *l == local).map(|(_, n, _)| n.clone()).collect()
    };
    AncestorBranches { local: names(true), remote: names(false) }
}

fn lineage_branches(repo: &Path) -> Vec<(usize, String, bool)> {
    let current = current_branch(repo);
    let mut rows: Vec<(usize, String, bool)> = Vec::new();
    for (dist, refname) in merged_ref_distances(repo) {
        if refname.ends_with("/HEAD") {
            continue;
        }
        if let Some(name) = refname.strip_prefix("refs/heads/") {
            if current.as_deref() == Some(name) {
                continue;
            }
            rows.push((dist, name.to_string(), true));
        } else if let Some(name) = refname.strip_prefix("refs/remotes/") {
            rows.push((dist, name.to_string(), false));
        }
    }
    rows.sort();
    rows
}

/// `(distance-below-HEAD, refname)` for every ref merged into `HEAD`. git ≥2.41 computes all
/// distances in ONE `for-each-ref` via `%(ahead-behind:HEAD)` (`behind` = commits in `HEAD` but
/// not the ref = the distance); on a big monorepo the previous per-ref `rev-list --count` fork —
/// hundreds of subprocesses on the UI thread — froze the plugin for >10s on every branch-scope
/// reload. Older git rejects the atom (empty output); the caller detects that and forks per ref.
fn merged_ref_distances(repo: &Path) -> Vec<(usize, String)> {
    let batched = git(
        repo,
        &[
            "for-each-ref",
            "--merged",
            "HEAD",
            // "<ahead> <behind> <refname>"; refname has no spaces, so it is the 3rd field.
            "--format=%(ahead-behind:HEAD) %(refname)",
            "refs/heads",
            "refs/remotes/origin",
        ],
    )
    .unwrap_or_default();
    // Fast path detection: the first line's second field is the numeric `behind` count. Empty
    // output (old git errored on the atom, or no merged refs) drops to the per-ref fallback.
    let fast = batched
        .lines()
        .next()
        .and_then(|l| l.split(' ').nth(1))
        .is_some_and(|behind| behind.parse::<usize>().is_ok());
    if fast {
        return batched
            .lines()
            .filter_map(|line| {
                // "<ahead> <behind> <refname>"; a refname never contains a space.
                let mut it = line.split(' ');
                let (_ahead, behind, refname) = (it.next(), it.next()?, it.next()?);
                Some((behind.parse().ok()?, refname.to_string()))
            })
            .collect();
    }
    // Fallback (git <2.41): list the merged refs, then one `rev-list --count` per ref.
    let names = git(
        repo,
        &[
            "for-each-ref",
            "--merged",
            "HEAD",
            "--format=%(refname)",
            "refs/heads",
            "refs/remotes/origin",
        ],
    )
    .unwrap_or_default();
    names
        .lines()
        .filter_map(|refname| Some((commit_distance(repo, refname)?, refname.to_string())))
        .collect()
}

fn commit_distance(repo: &Path, git_ref: &str) -> Option<usize> {
    git_line(repo, &["rev-list", "--count", &format!("{git_ref}..HEAD")])?.parse().ok()
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CommitRef {
    pub sha: String,
    pub short: String,
    pub date: String,
    pub author: String,
    pub title: String,
}

pub fn recent_commits(repo: &Path, limit: usize) -> Vec<CommitRef> {
    let max = format!("--max-count={limit}");
    git(repo, &["log", "HEAD", "--date=short", "--format=%H%x1f%h%x1f%ad%x1f%an%x1f%s", &max])
        .map(|out| {
            out.lines()
                .filter_map(|line| {
                    let mut f = line.split('\u{1f}');
                    Some(CommitRef {
                        sha: f.next()?.to_string(),
                        short: f.next()?.to_string(),
                        date: f.next().unwrap_or_default().to_string(),
                        author: f.next().unwrap_or_default().to_string(),
                        title: f.next().unwrap_or_default().to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The old side of a scope's diff against the worktree. `None` means `HEAD` (the
/// uncommitted default).
fn range(repo: &Path, scope: Scope, base: Option<&str>) -> Option<String> {
    match scope {
        // Last-turn diffs vs a snapshot tree (resolved by `changed_against_tree`), not a
        // committed range.
        Scope::LastTurn => None,
        // Branch diffs the worktree against the merge-base, so it shows committed branch
        // work and the working tree together — a superset of uncommitted (review-model.md).
        Scope::Branch => merge_base(repo, base),
        Scope::Commit => base.map(str::to_owned),
    }
}

/// The merge-base commit of `base` and `HEAD`, the old side of a branch-scope diff.
pub fn merge_base(repo: &Path, base: Option<&str>) -> Option<String> {
    let base = base_ref(repo, base)?;
    git_line(repo, &["merge-base", &base, "HEAD"])
}

/// The content of `path` at `rev` (`git show <rev>:<path>`). Empty when the path does
/// not exist at that rev — an added file against its old side, say.
pub fn file_content(repo: &Path, rev: &str, path: &str) -> String {
    git_lenient(repo, &["show", &format!("{rev}:{path}")])
}

// --- turn baseline (last-turn scope) -------------------------------------------
//
// See `specs/herdr-host.md`. The snapshot is non-disruptive: it writes a tree object
// from the worktree through a temporary index, never touching the real index, the
// worktree, or any branch, and persists the baseline under a private `refs/reviewr/`
// ref keyed by the worktree path.

/// A non-disruptive snapshot of the worktree as a tree object. Seeds a temporary index
/// from the repo's real index so unchanged files keep their cached hash, then `add -A`
/// and `write-tree`. Captures staged, unstaged, and untracked content alike. Touches
/// only the object database and the temp index — never the real index or any ref.
pub fn snapshot_worktree(repo: &Path) -> Result<String> {
    let git_dir = PathBuf::from(git(repo, &["rev-parse", "--absolute-git-dir"])?.trim());
    let tmp_index = git_dir.join("reviewr-turn-index");
    let real_index = git_dir.join("index");
    // Clear any temp index a prior hard crash left, then drop it on every exit path via the
    // guard, so even a failed snapshot leaves nothing behind in the git dir.
    let _ = std::fs::remove_file(&tmp_index);
    let _guard = TempIndex(&tmp_index);
    // Seed from the real index so git's stat cache lets unchanged files skip hashing;
    // a fresh repo may have no index yet, so start empty in that case.
    if real_index.exists() {
        std::fs::copy(&real_index, &tmp_index).context("seeding the snapshot index")?;
    }
    git_with_index(repo, &tmp_index, &["add", "-A"])?;
    let tree = git_with_index(repo, &tmp_index, &["write-tree"])?;
    Ok(tree.trim().to_string())
}

/// Removes a temporary index on drop, so a snapshot that fails midway never leaves one behind.
struct TempIndex<'a>(&'a Path);

impl Drop for TempIndex<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.0);
    }
}

/// Like [`git`], but runs against a throwaway index via `GIT_INDEX_FILE` so the snapshot
/// never disturbs the repo's real index.
fn git_with_index(repo: &Path, index: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "core.quotepath=false"])
        .args(args)
        .env("GIT_INDEX_FILE", index)
        .output()
        .with_context(|| format!("running git {args:?}"))?;
    if !out.status.success() {
        bail!("git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A stable per-worktree key for the baseline ref, from the absolute top-level path, so
/// sibling worktrees sharing one ref store do not collide. FNV-1a keeps it deterministic
/// across rebuilds — a std `DefaultHasher` is seeded per process and is not.
pub fn worktree_key(repo: &Path) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in repo.to_string_lossy().bytes() {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// The private ref holding a worktree's turn baseline — outside `refs/heads`, so it
/// never appears in a branch list.
fn baseline_ref(key: &str) -> String {
    format!("refs/reviewr/turn-base/{key}")
}

/// The persisted turn baseline tree for this worktree, if a baseline exists.
pub fn read_baseline_ref(repo: &Path, key: &str) -> Option<String> {
    git_line(repo, &["rev-parse", "--verify", "--quiet", &baseline_ref(key)])
}

/// Persist the turn baseline tree under the worktree's private ref. `update-ref` is
/// atomic, so the baseline is never half-written.
pub fn write_baseline_ref(repo: &Path, key: &str, sha: &str) -> Result<()> {
    git(repo, &["update-ref", &baseline_ref(key), sha])?;
    Ok(())
}

/// Private refs holding a worktree's persisted review state (JSON blobs), so an accidentally
/// closed pane never loses its review. Same storage shape as the turn baseline: repo-local,
/// worktree-keyed, atomic via `update-ref`, gone with the repo.
fn comments_ref(key: &str) -> String {
    format!("refs/reviewr/comments/{key}")
}

fn reviewed_ref(key: &str) -> String {
    format!("refs/reviewr/reviewed/{key}")
}

/// Persist a JSON blob under a private ref (hash the blob, then an atomic ref update).
/// Non-repos and read-only object stores surface as an error the caller may log and ignore —
/// the state then simply stays session-local.
fn write_state_blob(repo: &Path, refname: &str, json: &str) -> Result<()> {
    use std::io::Write;
    use std::process::Stdio;
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["hash-object", "-w", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawning git hash-object")?;
    child
        .stdin
        .take()
        .context("git hash-object stdin")?
        .write_all(json.as_bytes())
        .context("writing state blob")?;
    let out = child.wait_with_output().context("running git hash-object")?;
    if !out.status.success() {
        bail!("git hash-object failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let sha = String::from_utf8_lossy(&out.stdout).trim().to_string();
    git(repo, &["update-ref", refname, &sha])?;
    Ok(())
}

/// The persisted comment-store JSON for this worktree, if any.
pub fn read_comments_blob(repo: &Path, key: &str) -> Option<String> {
    git(repo, &["cat-file", "blob", &comments_ref(key)]).ok()
}

pub fn write_comments_blob(repo: &Path, key: &str, json: &str) -> Result<()> {
    write_state_blob(repo, &comments_ref(key), json)
}

/// Drop the worktree's persisted comment store (the last comment was deleted or exported).
pub fn delete_comments_blob(repo: &Path, key: &str) {
    let _ = git(repo, &["update-ref", "-d", &comments_ref(key)]);
}

/// The persisted reviewed-ticks JSON for this worktree, if any.
pub fn read_reviewed_blob(repo: &Path, key: &str) -> Option<String> {
    git(repo, &["cat-file", "blob", &reviewed_ref(key)]).ok()
}

pub fn write_reviewed_blob(repo: &Path, key: &str, json: &str) -> Result<()> {
    write_state_blob(repo, &reviewed_ref(key), json)
}

/// Drop the worktree's persisted reviewed ticks (the last mark was cleared or pruned).
pub fn delete_reviewed_blob(repo: &Path, key: &str) {
    let _ = git(repo, &["update-ref", "-d", &reviewed_ref(key)]);
}

/// git's well-known empty-tree object, used as the diff base when a repo has no commits.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// `HEAD` when the repo has a commit, else the empty tree (a commitless repo has no HEAD).
/// The shared "no explicit base" fallback: both `changed_files` (the built-in pane) and the
/// nvim editor's `g:reviewr_base` resolve through it, so an unborn repo diffs against the
/// empty tree everywhere instead of publishing an unresolvable `HEAD`.
pub fn diff_base(repo: &Path) -> String {
    if git(repo, &["rev-parse", "--verify", "-q", "HEAD"]).is_ok() {
        "HEAD".to_string()
    } else {
        EMPTY_TREE.to_string()
    }
}

/// The changed files for `scope`, sorted by path. `base` overrides the branch base ref.
/// `last-turn` is resolved separately by [`changed_against_tree`], so it lists nothing here.
pub fn changed_files(
    repo: &Path,
    scope: Scope,
    base: Option<&str>,
    status: &StatusSnapshot,
) -> Result<Vec<ChangedFile>> {
    changed_files_cached(repo, scope, base, status, &mut AdditionsCache::default())
}

/// [`changed_files`] with a caller-held [`AdditionsCache`], so a polling caller pays to read
/// an untracked file's content only when that file actually changed.
pub fn changed_files_cached(
    repo: &Path,
    scope: Scope,
    base: Option<&str>,
    status: &StatusSnapshot,
    cache: &mut AdditionsCache,
) -> Result<Vec<ChangedFile>> {
    let (numstat, name_status) = match scope {
        // Commit compares the worktree against the chosen commit. With none (an unborn repo has
        // no HEAD to default to), fall back to the empty tree so a fresh `git init` lists its
        // files instead of erroring — the old uncommitted-scope behavior, now the tip default.
        Scope::Commit => {
            let r = range(repo, scope, base).unwrap_or_else(|| diff_base(repo));
            (
                git(repo, &["diff", &r, "--numstat", "-z"])?,
                git(repo, &["diff", &r, "--name-status", "-z"])?,
            )
        }
        Scope::Branch => match range(repo, scope, base) {
            Some(r) => (
                git(repo, &["diff", &r, "--numstat", "-z"])?,
                git(repo, &["diff", &r, "--name-status", "-z"])?,
            ),
            None => return Ok(Vec::new()),
        },
        Scope::LastTurn => return Ok(Vec::new()),
    };
    // that `git diff` never reports.
    let untracked =
        matches!(scope, Scope::Branch | Scope::Commit).then(|| status.untracked_paths());
    Ok(assemble(repo, &numstat, &name_status, untracked, cache))
}

/// The changed files between the turn baseline `tree` and the live worktree, for
/// `last-turn`. Snapshots the worktree now and diffs tree-against-tree, so staged,
/// unstaged, untracked, and committed-this-turn changes all show, with no phantom
/// deletion for a file that is untracked at both ends (which a tree-vs-worktree diff
/// would mis-report). Untracked files ride in the current snapshot, so no separate
/// untracked pass is needed.
pub fn changed_against_tree(repo: &Path, tree: &str) -> Result<Vec<ChangedFile>> {
    let current = snapshot_worktree(repo)?;
    let numstat = git(repo, &["diff", tree, &current, "--numstat", "-z"])?;
    let name_status = git(repo, &["diff", tree, &current, "--name-status", "-z"])?;
    Ok(assemble(repo, &numstat, &name_status, None, &mut AdditionsCache::default()))
}

/// One entry in the `All files` worktree listing: a path plus whether git ignores it and
/// whether it is a (lazily-expanded) directory placeholder (specs/file-list.md).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub path: String,
    pub ignored: bool,
    pub is_dir: bool,
}

/// Every entry in the worktree for the `All files` tab (specs/file-list.md): tracked files
/// (`git ls-files`), untracked-not-ignored files, and the ignored entries from
/// `git status --ignored` — a wholly-ignored directory collapsed to one `is_dir` placeholder,
/// an individually-ignored file as itself. `.git` is never reported. Deduped and sorted; `-z`
/// keeps paths with spaces or special characters verbatim.
pub fn all_files(repo: &Path, status: &StatusSnapshot) -> Result<Vec<WorktreeEntry>> {
    let tracked = git(repo, &["ls-files", "-z"])?;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for path in tracked.split('\0').filter(|s| !s.is_empty()) {
        if seen.insert(path.to_string()) {
            out.push(WorktreeEntry { path: path.to_string(), ignored: false, is_dir: false });
        }
    }
    for path in status.untracked_paths() {
        if seen.insert(path.clone()) {
            out.push(WorktreeEntry { path, ignored: false, is_dir: false });
        }
    }
    for (path, is_dir) in ignored_entries(repo)? {
        if seen.insert(path.clone()) {
            out.push(WorktreeEntry { path, ignored: true, is_dir });
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/// The ignored entries from `git status --ignored`: a wholly-ignored directory comes back as
/// `dir/` (mapped to `is_dir = true`), an individually-ignored file as itself.
fn ignored_entries(repo: &Path) -> Result<Vec<(String, bool)>> {
    let status = git(repo, &["status", "--ignored=traditional", "--porcelain", "-z"])?;
    // Only `!!` records; tracked/untracked come from the passes above. A trailing `/` marks a
    // wholly-ignored directory (mapped to `is_dir`), anything else an individually-ignored file.
    Ok(porcelain_records(&status)
        .into_iter()
        .filter(|(xy, _)| *xy == "!!")
        .map(|(_, path)| match path.strip_suffix('/') {
            Some(dir) => (dir.to_string(), true),
            None => (path.to_string(), false),
        })
        .collect())
}

/// The immediate children of a wholly-ignored directory, for lazy expansion in `All files`
/// (specs/file-list.md). Everything under an ignored directory is ignored, so this reads the
/// filesystem directly; sub-directories come back as `is_dir` placeholders to expand in turn.
/// An unreadable directory yields no children rather than failing the reload, so expansion is
/// best-effort.
pub fn list_ignored_dir(repo: &Path, dir: &str) -> Vec<WorktreeEntry> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(repo.join(dir)) else { return out };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else { continue };
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        out.push(WorktreeEntry { path: format!("{dir}/{name}"), ignored: true, is_dir });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    out
}

/// Build the sorted `ChangedFile` list from `git diff` numstat + name-status output,
/// optionally appending untracked files (which a `git diff` never reports).
fn assemble(
    repo: &Path,
    numstat: &str,
    name_status: &str,
    untracked: Option<Vec<String>>,
    cache: &mut AdditionsCache,
) -> Vec<ChangedFile> {
    let counts = parse_numstat(numstat);
    let mut seen = HashSet::new();
    let mut files = Vec::new();
    for (kind, path, previous_path) in parse_name_status(name_status) {
        if !seen.insert(path.clone()) {
            continue;
        }
        let (additions, deletions) = counts.get(&path).copied().unwrap_or((0, 0));
        files.push(ChangedFile { path, kind, additions, deletions, previous_path });
    }

    if let Some(untracked) = untracked {
        // Untracked-not-ignored files list as additions.
        cache.retain_paths(&untracked);
        for path in untracked {
            if seen.insert(path.clone()) {
                let additions = cache.additions(repo, &path);
                files.push(ChangedFile {
                    path,
                    kind: ChangeKind::Untracked,
                    additions,
                    deletions: 0,
                    previous_path: None,
                });
            }
        }
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

/// One `git status --porcelain -z --untracked-files=all` walk, held raw. A reload collects it
/// once and threads it through the changed-file assembly, the staging markers, and the worktree
/// listing — the repo-sized status scan is paid once per tick, not once per consumer. The `-z`
/// form is NUL-delimited and never quotes or escapes a path, so names with spaces or special
/// characters survive verbatim — no trimming or unquoting. `--untracked-files=all` lists each
/// file inside a brand-new directory instead of collapsing it to one `dir/` entry, so the
/// files in a freshly-created folder are reviewable individually (.gitignore still applies).
#[derive(Debug)]
pub struct StatusSnapshot {
    raw: String,
}

impl StatusSnapshot {
    pub fn collect(repo: &Path) -> Result<Self> {
        Ok(Self { raw: git(repo, &["status", "--porcelain", "-z", "--untracked-files=all"])? })
    }

    /// The raw porcelain bytes — the change-digest's cheap "did anything move?" input.
    pub fn raw(&self) -> &str {
        &self.raw
    }

    /// Every non-ignored record path of this walk — the files whose worktree bytes can differ
    /// from the last tick without the porcelain text itself changing (a modified file edited
    /// again, an untracked file appended to). The change digest stats each of these.
    pub fn record_paths(&self) -> Vec<&str> {
        porcelain_records(&self.raw)
            .into_iter()
            .filter(|(xy, _)| *xy != "!!")
            .map(|(_, path)| path)
            .collect()
    }

    /// The untracked (`??`) paths of this walk.
    fn untracked_paths(&self) -> Vec<String> {
        porcelain_records(&self.raw)
            .into_iter()
            .filter(|(xy, _)| *xy == "??")
            .map(|(_, path)| path.to_string())
            .collect()
    }

    /// Each file's working-tree state vs `HEAD`, keyed by path: the status letter and whether it
    /// is staged. Untracked is `('?', false)`; ignored (`!!`) is skipped; a staged file takes the
    /// index letter `X`, an unstaged one the worktree letter `Y`.
    pub fn file_statuses(&self) -> std::collections::HashMap<String, FileStatus> {
        let mut out = std::collections::HashMap::new();
        for (xy, path) in porcelain_records(&self.raw) {
            if xy == "!!" {
                continue;
            }
            let bytes = xy.as_bytes();
            let (x, y) = (bytes[0] as char, bytes[1] as char);
            let file = if xy == "??" {
                FileStatus { marker: '?', staged: false }
            } else {
                let staged = x != ' ';
                FileStatus { marker: if staged { x } else { y }, staged }
            };
            out.insert(path.to_string(), file);
        }
        out
    }
}

/// The `(xy, path)` of each `git status --porcelain -z` record. Each record is `XY␠PATH`; the
/// first three bytes (status + space) are ASCII, so the slices land on char boundaries. A
/// rename/copy carries its source in a second NUL field, consumed here so records stay aligned.
/// Callers keep the status codes they want (`??` for untracked, `!!` for ignored).
fn porcelain_records(status: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    let mut it = status.split('\0');
    while let Some(entry) = it.next() {
        if entry.len() < 3 {
            continue; // trailing empty field, or a malformed short record
        }
        let xy = &entry[..2];
        if xy.contains('R') || xy.contains('C') {
            it.next();
        }
        out.push((xy, &entry[3..]));
    }
    out
}

/// Addition count of an untracked file: its line count, which is what `git diff` against
/// nothing reports (0 for empty or binary). Read locally rather than shelling
/// `git diff --no-index` per file — with `--untracked-files=all` a large untracked tree
/// would otherwise fork git once per file on every poll and freeze the UI.
/// Line-addition counts for untracked files, memoized by `(size, mtime)`. The `+N` annotation
/// requires reading each untracked file's bytes; on a worktree with thousands of them that
/// per-tick re-read dominated reload, so the count is served from here while the file's stat
/// is unchanged. A same-size same-mtime content rewrite is indistinguishable, but nanosecond
/// mtimes make that practically impossible outside a deliberate construction.
#[derive(Debug, Default)]
pub struct AdditionsCache {
    entries: HashMap<String, (u64, Option<std::time::SystemTime>, u32)>,
}

impl AdditionsCache {
    fn additions(&mut self, repo: &Path, path: &str) -> u32 {
        let Ok(meta) = std::fs::metadata(repo.join(path)) else {
            return 0; // vanished mid-tick — same as a failed read below
        };
        let (size, mtime) = (meta.len(), meta.modified().ok());
        if let Some(&(s, t, n)) = self.entries.get(path)
            && s == size
            && t == mtime
        {
            return n;
        }
        let n = untracked_additions(repo, path);
        self.entries.insert(path.to_string(), (size, mtime, n));
        n
    }

    /// Drop entries whose paths are no longer untracked, so churn cannot grow the map.
    fn retain_paths(&mut self, live: &[String]) {
        let live: HashSet<&str> = live.iter().map(String::as_str).collect();
        self.entries.retain(|p, _| live.contains(p.as_str()));
    }
}

fn untracked_additions(repo: &Path, path: &str) -> u32 {
    let Ok(bytes) = std::fs::read(repo.join(path)) else { return 0 };
    if bytes.is_empty() || bytes.contains(&0) {
        return 0; // empty, or binary (a NUL byte) — git reports no line additions
    }
    // Lines = newline count, plus one for a final line with no trailing newline. A plain
    // byte count is fine for one already-read file; no need for the bytecount crate.
    #[allow(clippy::naive_bytecount)]
    let newlines = bytes.iter().filter(|&&b| b == b'\n').count();
    let trailing = usize::from(bytes.last() != Some(&b'\n'));
    (newlines + trailing) as u32
}

// --- pure parsers (unit-tested without a repo) ---------------------------------

/// Map of new-path to `(additions, deletions)` from `git diff --numstat -z`.
///
/// Under `-z` a non-rename record is `ADDS\tDELS\tPATH\0`; a rename/copy record is
/// `ADDS\tDELS\t\0OLD\0NEW\0` — the counts ride the front, then old and new arrive as
/// their own NUL fields (no `=>` arrow, no brace factoring). Binary files emit `-`/`-`,
/// which parse to 0. The counts key under the new path, matching `parse_name_status`.
fn parse_numstat(out: &str) -> HashMap<String, (u32, u32)> {
    let mut map = HashMap::new();
    let mut it = out.split('\0');
    while let Some(field) = it.next() {
        // `splitn(3)` keeps any tabs inside the path (verbatim under `-z`) intact.
        let mut parts = field.splitn(3, '\t');
        let add = parts.next().unwrap_or("0").parse().unwrap_or(0);
        let del = parts.next().unwrap_or("0").parse().unwrap_or(0);
        match parts.next() {
            // Non-rename: the path rode this same field.
            Some(path) if !path.is_empty() => {
                map.insert(path.to_string(), (add, del));
            }
            // Rename/copy: the next two fields are the old and new paths.
            Some(_) => {
                let _old = it.next();
                if let Some(new) = it.next().filter(|n| !n.is_empty()) {
                    map.insert(new.to_string(), (add, del));
                }
            }
            // No tab fields — a trailing empty record after the final NUL.
            None => {}
        }
    }
    map
}

/// `(kind, path, previous_path)` from `git diff --name-status -z`. Under `-z` each record is
/// `STATUS\0PATH\0`, except a rename/copy is `R<score>\0OLD\0NEW\0` (status, then old and new
/// as separate fields). A rename or copy takes the new path and carries its old path; every
/// other kind has `previous_path == None`. Copy folds into `Renamed` — a copy's old content
/// lives at the old path exactly like a rename, which is what `content_sides` reads.
fn parse_name_status(out: &str) -> Vec<(ChangeKind, String, Option<String>)> {
    let mut rows = Vec::new();
    let mut it = out.split('\0');
    while let Some(status) = it.next() {
        let row = match status.chars().next() {
            Some('A') => it.next().map(|p| (ChangeKind::Added, p.to_string(), None)),
            Some('D') => it.next().map(|p| (ChangeKind::Deleted, p.to_string(), None)),
            Some('R' | 'C') => {
                let old = it.next();
                it.next().map(|new| (ChangeKind::Renamed, new.to_string(), old.map(str::to_string)))
            }
            // Modified, type-changed, etc.; also skips the trailing empty record.
            Some(_) => it.next().map(|p| (ChangeKind::Modified, p.to_string(), None)),
            None => None,
        };
        if let Some((kind, path, prev)) = row
            && !path.is_empty()
        {
            rows.push((kind, path, prev));
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::{ChangeKind, parse_github_slug, parse_name_status, parse_numstat};

    #[test]
    fn github_slug_parses_every_remote_form_and_rejects_non_github() {
        let g = parse_github_slug;
        let owned = |o: &str, n: &str| Some((o.to_string(), n.to_string()));
        // HTTPS, with and without `.git` and a trailing slash.
        assert_eq!(g("https://github.com/owner/repo.git"), owned("owner", "repo"));
        assert_eq!(g("https://github.com/owner/repo"), owned("owner", "repo"));
        assert_eq!(g("https://github.com/owner/repo/"), owned("owner", "repo"));
        // scp-like SSH, and the `ssh://` scheme form with a port.
        assert_eq!(g("git@github.com:owner/repo.git"), owned("owner", "repo"));
        assert_eq!(g("ssh://git@github.com/owner/repo.git"), owned("owner", "repo"));
        assert_eq!(g("ssh://git@github.com:22/owner/repo.git"), owned("owner", "repo"));
        assert_eq!(g("git://github.com/owner/repo"), owned("owner", "repo"));
        // Multi-account SSH host alias (`~/.ssh/config` `Host github.com-work`).
        assert_eq!(g("git@github.com-work:owner/repo.git"), owned("owner", "repo"));
        // Non-GitHub and GitHub Enterprise hosts are not github.com.
        assert_eq!(g("git@gitlab.com:owner/repo.git"), None);
        assert_eq!(g("git@github.company.com:owner/repo.git"), None);
        assert_eq!(g("https://github.company.com/owner/repo.git"), None);
        // A host with no repo segment has no slug.
        assert_eq!(g("https://github.com/owner"), None);
    }

    #[test]
    fn numstat_parses_counts_and_ignores_binary() {
        let m = parse_numstat("18\t8\tsrc/a.rs\0-\t-\tassets/logo.png\0");
        assert_eq!(m["src/a.rs"], (18, 8));
        assert_eq!(m["assets/logo.png"], (0, 0));
    }

    #[test]
    fn numstat_keys_renames_under_the_new_path() {
        // Under `-z` a rename is `ADDS\tDELS\t\0OLD\0NEW`: old and new are their own fields,
        // no `=>` arrow or brace form. Counts must key under the new path.
        let m = parse_numstat("3\t1\t\0src/old.rs\0src/new.rs\0");
        assert_eq!(m["src/new.rs"], (3, 1));
        assert!(!m.contains_key("src/old.rs"));
    }

    #[test]
    fn numstat_dir_removing_rename_has_no_double_slash() {
        // Regression: the old brace parser produced `a//file.rs` here, so counts never matched.
        let m = parse_numstat("4\t2\t\0a/b/file.rs\0a/file.rs\0");
        assert_eq!(m["a/file.rs"], (4, 2));
        assert!(!m.contains_key("a//file.rs"));
    }

    #[test]
    fn numstat_handles_a_mixed_stream() {
        // binary, plain, rename, in sequence — the rename lookahead must stay aligned.
        // `\x00` (= NUL) is used as the separator so the digits after it read clearly.
        let m = parse_numstat("-\t-\tlogo.png\x009\t1\tsrc/a.rs\x005\t4\t\x00o.rs\x00n.rs\x00");
        assert_eq!(m["logo.png"], (0, 0));
        assert_eq!(m["src/a.rs"], (9, 1));
        assert_eq!(m["n.rs"], (5, 4));
    }

    #[test]
    fn name_status_kinds_and_rename_target() {
        let rows =
            parse_name_status("M\0src/a.rs\0A\0src/b.rs\0D\0src/c.rs\0R100\0old.rs\0new.rs\0");
        assert_eq!(rows[0], (ChangeKind::Modified, "src/a.rs".to_string(), None));
        assert_eq!(rows[1], (ChangeKind::Added, "src/b.rs".to_string(), None));
        assert_eq!(rows[2], (ChangeKind::Deleted, "src/c.rs".to_string(), None));
        assert_eq!(
            rows[3],
            (ChangeKind::Renamed, "new.rs".to_string(), Some("old.rs".to_string()))
        );
    }

    #[test]
    fn name_status_copy_keeps_the_new_path() {
        // A copy carries old + new like a rename; it must key under the new path, not collapse
        // to a Modified entry on the source path.
        let rows = parse_name_status("C75\0orig.rs\0copy.rs\0");
        assert_eq!(
            rows[0],
            (ChangeKind::Renamed, "copy.rs".to_string(), Some("orig.rs".to_string()))
        );
    }
}
