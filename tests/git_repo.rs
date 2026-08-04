mod common;

use std::collections::HashMap;

use common::Repo;
use herdr_reviewr::git::{
    StatusSnapshot, base_ref, changed_against_tree, file_content, merge_base, read_baseline_ref,
    read_comments_blob, recent_commits, snapshot_worktree, worktree_key, write_baseline_ref,
    write_comments_blob,
};
use herdr_reviewr::model::{ChangeKind, ChangedFile, Scope};

fn by_path(files: &[ChangedFile]) -> HashMap<&str, &ChangedFile> {
    files.iter().map(|f| (f.path.as_str(), f)).collect()
}

// The production callers collect one StatusSnapshot per reload and thread it through; these
// wrappers do the same per call so the test bodies stay expressed in terms of the operation.
fn changed_files(
    repo: &std::path::Path,
    scope: Scope,
    base: Option<&str>,
) -> anyhow::Result<Vec<ChangedFile>> {
    let snap = StatusSnapshot::collect(repo)?;
    herdr_reviewr::git::changed_files(repo, scope, base, &snap)
}

fn all_files(repo: &std::path::Path) -> anyhow::Result<Vec<herdr_reviewr::git::WorktreeEntry>> {
    let snap = StatusSnapshot::collect(repo)?;
    herdr_reviewr::git::all_files(repo, &snap)
}

#[test]
fn lists_every_change_kind_with_stats() {
    let r = Repo::init();
    r.write("keep.rs", "fn a() {}\n");
    r.write("gone.rs", "fn g() {}\n");
    r.write("edit.rs", "one\ntwo\nthree\n");
    r.commit_all("init");

    r.write("edit.rs", "one\nTWO\nthree\nfour\n");
    r.write("added.rs", "new\n");
    r.git(&["add", "added.rs"]);
    r.remove("gone.rs");
    r.write("untracked.rs", "u\n");

    let files = changed_files(r.path(), Scope::Commit, Some("HEAD")).unwrap();
    let files = by_path(&files);

    assert_eq!(files["edit.rs"].kind, ChangeKind::Modified);
    assert_eq!(files["added.rs"].kind, ChangeKind::Added);
    assert_eq!(files["gone.rs"].kind, ChangeKind::Deleted);
    assert_eq!(files["untracked.rs"].kind, ChangeKind::Untracked);
    assert!(files["edit.rs"].additions >= 1, "additions counted");
    assert!(files["edit.rs"].deletions >= 1, "deletions counted");
}

#[test]
fn file_content_reads_the_committed_version_not_the_worktree() {
    let r = Repo::init();
    r.write("a.rs", "alpha\nbeta\ngamma\n");
    r.commit_all("init");
    r.write("a.rs", "alpha\nBETA\ngamma\n");

    assert_eq!(file_content(r.path(), "HEAD", "a.rs"), "alpha\nbeta\ngamma\n");
}

#[test]
fn file_content_is_empty_for_a_path_absent_at_that_rev() {
    let r = Repo::init();
    r.write("seed.rs", "x\n");
    r.commit_all("init");
    r.write("fresh.rs", "line one\nline two\n");

    assert_eq!(file_content(r.path(), "HEAD", "fresh.rs"), "");
    let files = changed_files(r.path(), Scope::Commit, Some("HEAD")).unwrap();
    assert_eq!(by_path(&files)["fresh.rs"].additions, 2);
}

#[test]
fn merge_base_is_the_branch_point() {
    let r = Repo::init();
    r.write("base.rs", "1\n");
    r.commit_all("base");
    let branch_point = r.git(&["rev-parse", "HEAD"]).trim().to_string();
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("base.rs", "2\n");
    r.commit_all("diverge");

    assert_eq!(merge_base(r.path(), Some("main")), Some(branch_point));
}

#[test]
fn branch_scope_is_a_superset_of_uncommitted() {
    let r = Repo::init();
    r.write("base.rs", "1\n");
    r.commit_all("base");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("committed.rs", "new\n");
    r.commit_all("feature work");
    r.write("dirty.rs", "wip\n");
    r.write("untracked.rs", "scratch\n");

    let branch = changed_files(r.path(), Scope::Branch, Some("main")).unwrap();
    let names: Vec<&str> = branch.iter().map(|f| f.path.as_str()).collect();
    assert!(names.contains(&"committed.rs"), "branch shows committed work");
    assert!(names.contains(&"dirty.rs"), "branch shows uncommitted edits");
    assert!(names.contains(&"untracked.rs"), "branch shows untracked files");

    let uncommitted = changed_files(r.path(), Scope::Commit, Some("HEAD")).unwrap();
    for f in &uncommitted {
        assert!(names.contains(&f.path.as_str()), "branch contains uncommitted {}", f.path);
    }
}

#[test]
fn branch_scope_equals_uncommitted_when_head_is_the_base() {
    let r = Repo::init();
    r.write("base.rs", "1\n");
    r.commit_all("base");
    r.write("base.rs", "1\nchanged\n");

    let branch = changed_files(r.path(), Scope::Branch, Some("main")).unwrap();
    assert!(branch.iter().any(|f| f.path == "base.rs"), "branch is not empty at the base");
}

#[test]
fn ignored_paths_never_enter_changes() {
    let r = Repo::init();
    r.write(".gitignore", "ignored/\nbuild/\n");
    r.commit_all("init");
    r.write("ignored/note.md", "scratch\n");
    r.write("build/out.o", "junk\n");

    let has_ignored = |files: &[ChangedFile]| {
        files.iter().any(|f| f.path.starts_with("ignored/") || f.path.starts_with("build/"))
    };
    assert!(
        !has_ignored(&changed_files(r.path(), Scope::Commit, Some("HEAD")).unwrap()),
        "uncommitted"
    );
    assert!(!has_ignored(&changed_files(r.path(), Scope::Branch, Some("main")).unwrap()), "branch");

    let base = snapshot_worktree(r.path()).unwrap();
    r.write("ignored/note.md", "scratch v2\n");
    assert!(!has_ignored(&changed_against_tree(r.path(), &base).unwrap()), "last-turn");
}

#[test]
fn branch_scope_falls_back_to_master_when_main_is_absent() {
    let r = Repo::init();
    r.write("base.rs", "1\n");
    r.commit_all("base");
    r.git(&["branch", "-m", "main", "master"]);
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("feature.rs", "x\n");
    r.commit_all("feature work");

    let files = changed_files(r.path(), Scope::Branch, None).unwrap();
    assert!(files.iter().any(|f| f.path == "feature.rs"), "resolved master as the base ref");
}

#[test]
fn branch_scope_auto_base_is_the_nearest_fork_not_mainline() {
    let r = Repo::init();
    r.write("a.rs", "1\n");
    r.commit_all("A");
    r.git(&["checkout", "-q", "-b", "parent"]);
    r.write("b.rs", "1\n");
    r.commit_all("B");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("c.rs", "1\n");
    r.commit_all("C");

    assert_eq!(
        base_ref(r.path(), None).as_deref(),
        Some("parent"),
        "auto base is the nearest fork"
    );
    let changed = changed_files(r.path(), Scope::Branch, None).unwrap();
    let files = by_path(&changed);
    assert!(files.contains_key("c.rs"), "feature's own change is shown");
    assert!(!files.contains_key("b.rs"), "parent's inherited change is not in the diff");

    assert_eq!(base_ref(r.path(), Some("main")).as_deref(), Some("main"), "explicit base wins");

    r.git(&["checkout", "-q", "main"]);
    assert_eq!(
        base_ref(r.path(), None).as_deref(),
        Some("main"),
        "no nearer fork → trunk fallback"
    );
}

// A live report from a large monorepo: a stale snapshot of the mainline (fully merged into
// HEAD, e.g. `origin/develop-fresh`) sat nearer in the merged-ref ranking than the true fork
// parent — which, being the moving trunk, is never itself merged into HEAD — so the auto base
// diffed 27 mainline commits into the branch changeset. The trunk must compete, ranked by how
// far HEAD sits above their merge-base (the distance the diff base actually lands at).
#[test]
fn auto_base_prefers_trunk_over_a_stale_merged_snapshot() {
    let r = Repo::init();
    r.write("a.rs", "1\n");
    r.commit_all("A");
    // The stale snapshot: the mainline as it was at A, still an ancestor of the branch.
    r.git(&["update-ref", "refs/remotes/origin/develop-fresh", "main"]);
    r.write("b.rs", "1\n");
    r.commit_all("B");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("f.rs", "1\n");
    r.commit_all("F");
    // The trunk moved on after the fork: its tip is NOT an ancestor of the branch.
    r.git(&["checkout", "-q", "main"]);
    r.write("c.rs", "1\n");
    r.commit_all("C");
    r.git(&["update-ref", "refs/remotes/origin/develop", "main"]);
    r.git(&["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/develop"]);
    r.git(&["checkout", "-q", "feature"]);

    // Trunk's diff base is the fork point (1 commit below HEAD); the snapshot's is its own
    // tip (2 below). Nearest diff base wins.
    assert_eq!(
        base_ref(r.path(), None).as_deref(),
        Some("origin/develop"),
        "the trunk outranks a stale merged snapshot of itself"
    );
    let changed = changed_files(r.path(), Scope::Branch, None).unwrap();
    let files = by_path(&changed);
    assert!(files.contains_key("f.rs"), "the branch's own change is shown");
    assert!(!files.contains_key("b.rs"), "mainline commits below the fork stay out");

    // The picker offers the trunk too — it was absent entirely while only merged refs listed.
    let a = herdr_reviewr::git::ancestor_branches(r.path());
    assert!(
        a.remote.contains(&"origin/develop".to_string()),
        "the trunk is offered in the branch picker: {:?}",
        a.remote
    );
}

#[test]
fn rename_is_reported_at_the_new_path() {
    let r = Repo::init();
    r.write("old_name.rs", "stable contents that survive the move\n");
    r.commit_all("init");
    r.git(&["mv", "old_name.rs", "new_name.rs"]);

    let files = changed_files(r.path(), Scope::Commit, Some("HEAD")).unwrap();
    let renamed = files.iter().find(|f| f.kind == ChangeKind::Renamed).expect("a renamed file");
    assert_eq!(renamed.path, "new_name.rs");
    assert_eq!(renamed.previous_path.as_deref(), Some("old_name.rs"));
}

#[test]
fn a_directory_removing_rename_keeps_its_stats() {
    let r = Repo::init();
    r.write("a/b/file.rs", "one\ntwo\nthree\nfour\nfive\nsix\n");
    r.commit_all("init");
    r.git(&["mv", "a/b/file.rs", "a/file.rs"]);
    r.write("a/file.rs", "one\nTWO\nthree\nfour\nfive\nsix\n");

    let files = changed_files(r.path(), Scope::Commit, Some("HEAD")).unwrap();
    let renamed = files.iter().find(|f| f.kind == ChangeKind::Renamed).expect("a renamed file");
    assert_eq!(renamed.path, "a/file.rs");
    assert_eq!(renamed.previous_path.as_deref(), Some("a/b/file.rs"));
    assert!(renamed.additions + renamed.deletions > 0, "the edit's stats are counted");
}

#[test]
fn untracked_paths_with_spaces_survive_verbatim() {
    let r = Repo::init();
    r.write("seed.rs", "x\n");
    r.commit_all("init");
    r.write("a file with spaces.rs", "u\n");

    let files = changed_files(r.path(), Scope::Commit, Some("HEAD")).unwrap();
    let f = by_path(&files)["a file with spaces.rs"];
    assert_eq!(f.kind, ChangeKind::Untracked);
    assert_eq!(f.additions, 1);
}

#[test]
fn untracked_files_in_a_new_directory_are_listed_individually() {
    let r = Repo::init();
    r.write("seed.rs", "x\n");
    r.commit_all("init");
    r.write("docs/new/a.md", "alpha\n");
    r.write("docs/new/b.md", "beta\n");

    let files = changed_files(r.path(), Scope::Commit, Some("HEAD")).unwrap();
    let by = by_path(&files);
    assert!(by.contains_key("docs/new/a.md"), "the file is listed, not the directory");
    assert!(by.contains_key("docs/new/b.md"));
    assert!(!by.contains_key("docs/new/"), "the bare directory is not an entry");
    assert_eq!(by["docs/new/a.md"].kind, ChangeKind::Untracked);
}

#[test]
fn a_repo_with_no_commits_lists_untracked_without_erroring() {
    let r = Repo::init();
    r.write("fresh.rs", "one\ntwo\n");
    let files = changed_files(r.path(), Scope::Commit, None).unwrap();
    assert!(by_path(&files).contains_key("fresh.rs"), "lists files in a commitless repo");
}

#[test]
fn a_binary_change_lists_with_zero_stats() {
    let r = Repo::init();
    r.write("blob.bin", "\0\0seed\0\0");
    r.commit_all("init");
    r.write("blob.bin", "\0\0changed\0\0\0");

    let files = changed_files(r.path(), Scope::Commit, Some("HEAD")).unwrap();
    let f = by_path(&files)["blob.bin"];
    assert_eq!(f.kind, ChangeKind::Modified);
    assert_eq!((f.additions, f.deletions), (0, 0));
}

#[test]
fn git_access_never_mutates_the_repo() {
    let r = Repo::init();
    r.write("a.rs", "x\n");
    r.commit_all("init");
    r.write("a.rs", "y\n");

    let head_before = r.git(&["rev-parse", "HEAD"]);
    let status_before = r.git(&["status", "--porcelain"]);

    let _ = changed_files(r.path(), Scope::Commit, Some("HEAD")).unwrap();
    let _ = file_content(r.path(), "HEAD", "a.rs");
    let _ = changed_files(r.path(), Scope::Branch, Some("main")).unwrap();

    assert_eq!(head_before, r.git(&["rev-parse", "HEAD"]), "HEAD unchanged");
    assert_eq!(status_before, r.git(&["status", "--porcelain"]), "working tree unchanged");
}

#[test]
fn changed_against_tree_shows_edits_creates_and_deletes_since_the_snapshot() {
    let r = Repo::init();
    r.write("tracked.rs", "one\ntwo\n");
    r.write("doomed.rs", "bye\n");
    r.commit_all("init");
    r.write("idle_untracked.rs", "u\n");

    let base = snapshot_worktree(r.path()).unwrap();

    r.write("tracked.rs", "one\nTWO\nthree\n");
    r.write("created.rs", "new\n");
    r.remove("doomed.rs");

    let files = changed_against_tree(r.path(), &base).unwrap();
    let files = by_path(&files);
    assert_eq!(files["tracked.rs"].kind, ChangeKind::Modified);
    assert_eq!(files["created.rs"].kind, ChangeKind::Added);
    assert_eq!(files["doomed.rs"].kind, ChangeKind::Deleted);
    assert!(
        !files.contains_key("idle_untracked.rs"),
        "an untracked file unchanged across the turn is not a phantom delete"
    );
}

#[test]
fn changed_against_tree_sees_an_untracked_only_turn() {
    let r = Repo::init();
    r.write("a.rs", "a\n");
    r.commit_all("init");
    let base = snapshot_worktree(r.path()).unwrap();
    r.write("fresh.rs", "x\n");
    let files = changed_against_tree(r.path(), &base).unwrap();
    assert_eq!(by_path(&files)["fresh.rs"].kind, ChangeKind::Added);
}

#[test]
fn snapshot_worktree_never_mutates_the_repo() {
    let r = Repo::init();
    r.write("a.rs", "x\n");
    r.commit_all("init");
    r.write("a.rs", "y\n");
    r.write("untracked.rs", "u\n");

    let git_dir = r.git(&["rev-parse", "--absolute-git-dir"]);
    let git_dir = std::path::Path::new(git_dir.trim());
    let staged_before = r.git(&["ls-files", "--stage"]);
    let status_before = r.git(&["status", "--porcelain"]);
    let head_before = r.git(&["rev-parse", "HEAD"]);
    let branches_before = r.git(&["branch", "-a"]);

    let tree = snapshot_worktree(r.path()).unwrap();
    assert_eq!(tree.len(), 40, "a tree object id");

    assert_eq!(r.git(&["ls-files", "--stage"]), staged_before, "real index entries untouched");
    assert_eq!(r.git(&["status", "--porcelain"]), status_before, "working tree status unchanged");
    assert_eq!(r.git(&["rev-parse", "HEAD"]), head_before, "HEAD unchanged");
    assert_eq!(r.git(&["branch", "-a"]), branches_before, "no branch created");
    assert!(!git_dir.join("reviewr-turn-index").exists(), "the temp index is cleaned up");
}

#[test]
fn baseline_ref_round_trips_under_the_private_namespace() {
    let r = Repo::init();
    r.write("a.rs", "a\n");
    r.commit_all("init");
    let key = worktree_key(r.path());
    assert!(read_baseline_ref(r.path(), &key).is_none(), "no baseline initially");

    let tree = snapshot_worktree(r.path()).unwrap();
    write_baseline_ref(r.path(), &key, &tree).unwrap();
    assert_eq!(read_baseline_ref(r.path(), &key).as_deref(), Some(tree.as_str()));

    assert!(!r.git(&["branch", "-a"]).contains("reviewr"), "the baseline is not a branch");
    assert!(
        r.git(&["show-ref"]).contains("refs/reviewr/turn-base/"),
        "the baseline lives under the private ref namespace"
    );
}

#[test]
fn worktree_key_is_stable_and_path_specific() {
    let a = std::path::Path::new("/repo/one");
    let b = std::path::Path::new("/repo/two");
    assert_eq!(worktree_key(a), worktree_key(a), "deterministic for one path");
    assert_ne!(worktree_key(a), worktree_key(b), "distinct per worktree path");
}

#[test]
fn all_files_lists_tracked_untracked_and_ignored_dirs_collapsed() {
    let r = Repo::init();
    r.write("src/app.rs", "fn main() {}\n");
    r.write("Cargo.toml", "[package]\n");
    r.commit_all("init");
    r.write("untracked.rs", "u\n");
    r.write(".gitignore", "target/\nbuild.log\n");
    r.write("target/build.o", "binary\n");
    r.write("target/deep/x.o", "binary\n");
    r.write("build.log", "noise\n");

    let files = all_files(r.path()).unwrap();
    let by = |p: &str| files.iter().find(|e| e.path == p);
    assert!(by("src/app.rs").is_some_and(|e| !e.ignored && !e.is_dir), "tracked file listed");
    assert!(by("untracked.rs").is_some_and(|e| !e.ignored), "untracked-not-ignored listed");
    assert!(by("target").is_some_and(|e| e.ignored && e.is_dir), "ignored dir is a placeholder");
    assert!(!files.iter().any(|e| e.path.starts_with("target/")), "ignored dir is not walked");
    assert!(by("build.log").is_some_and(|e| e.ignored && !e.is_dir), "ignored file listed, dimmed");

    let paths: Vec<&str> = files.iter().map(|e| e.path.as_str()).collect();
    let mut sorted = paths.clone();
    sorted.sort_unstable();
    assert_eq!(paths, sorted, "the listing is sorted");
}

#[test]
fn list_ignored_dir_returns_immediate_children_only() {
    use herdr_reviewr::git::list_ignored_dir;
    let r = Repo::init();
    r.write(".gitignore", "target/\n");
    r.write("target/build.o", "x\n");
    r.write("target/deep/x.o", "y\n");
    r.commit_all("init");

    let kids = list_ignored_dir(r.path(), "target");
    assert!(kids.iter().all(|e| e.ignored), "every child of an ignored dir is ignored");
    assert!(kids.iter().any(|e| e.path == "target/build.o" && !e.is_dir), "immediate file");
    assert!(kids.iter().any(|e| e.path == "target/deep" && e.is_dir), "subdir as a placeholder");
    assert!(!kids.iter().any(|e| e.path == "target/deep/x.o"), "does not recurse past one level");
}

#[test]
fn branch_scope_follows_origin_head_when_mainline_is_develop() {
    let r = Repo::init();
    r.write("base.rs", "1\n");
    r.commit_all("base");
    r.git(&["branch", "-m", "main", "develop"]);
    let develop_tip = r.git(&["rev-parse", "HEAD"]).trim().to_string();
    r.git(&["update-ref", "refs/remotes/origin/develop", "HEAD"]);
    r.git(&["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/develop"]);
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("feature.rs", "1\n");
    r.commit_all("feature work");

    assert_eq!(merge_base(r.path(), None), Some(develop_tip.clone()));
    let files = changed_files(r.path(), Scope::Branch, None).expect("changed_files");
    assert!(by_path(&files).contains_key("feature.rs"), "branch diff must see the branch commit");
    assert_eq!(merge_base(r.path(), Some("develop")), Some(develop_tip));
}

#[test]
fn ancestor_branches_lists_the_lineage_nearest_first_and_excludes_siblings() {
    let r = Repo::init();
    r.write("a.rs", "1\n");
    r.commit_all("A");
    r.git(&["checkout", "-q", "-b", "b"]);
    r.write("b.rs", "1\n");
    r.commit_all("B");
    r.git(&["checkout", "-q", "-b", "c"]);
    r.write("c.rs", "1\n");
    r.commit_all("C");
    r.git(&["checkout", "-q", "b"]);
    r.git(&["checkout", "-q", "-b", "sib"]);
    r.write("sib.rs", "1\n");
    r.commit_all("S");
    r.git(&["checkout", "-q", "c"]);
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("f.rs", "1\n");
    r.commit_all("F");
    r.git(&["update-ref", "refs/remotes/origin/c", "c"]);
    r.git(&["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/c"]);

    let a = herdr_reviewr::git::ancestor_branches(r.path());
    assert_eq!(a.local, vec!["c", "b", "main"], "local ancestors, nearest fork first");
    assert!(!a.local.contains(&"sib".to_string()), "a sibling off the lineage is excluded");
    assert!(!a.local.contains(&"feature".to_string()), "the current branch is not a choice");
    assert!(a.remote.contains(&"origin/c".to_string()), "an origin ancestor is listed");
    assert!(!a.remote.iter().any(|b| b.ends_with("/HEAD")), "origin/HEAD alias must be skipped");
    assert!(!a.remote.iter().any(|b| b == "origin"), "origin/HEAD short alias must not leak");
}

#[test]
fn recent_commits_lists_full_history_newest_first() {
    let r = Repo::init();
    r.write("a.rs", "one\n");
    r.commit_all("base commit");
    r.git(&["checkout", "-q", "-b", "feature"]);
    r.write("b.rs", "two\n");
    r.commit_all("add b");
    r.write("c.rs", "three\n");
    r.commit_all("add c");

    let commits = recent_commits(r.path(), 50);
    let titles: Vec<&str> = commits.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(titles, vec!["add c", "add b", "base commit"], "full history, newest first");
    for c in &commits {
        assert!(!c.short.is_empty(), "abbreviated hash present");
        assert!(c.sha.starts_with(&c.short), "short is a prefix of the full sha");
        assert_eq!(c.sha.len(), 40, "full sha");
        assert!(!c.author.is_empty(), "author name present");
        assert_eq!(c.date.len(), 10, "author date is YYYY-MM-DD");
    }
}

#[test]
fn recent_commits_lists_history_even_on_the_base_branch() {
    let r = Repo::init();
    r.write("a.rs", "one\n");
    r.commit_all("only commit");
    let commits = recent_commits(r.path(), 50);
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0].title, "only commit");

    assert!(recent_commits(r.path(), 0).is_empty(), "the limit is honoured");
}

#[test]
fn working_status_reports_staged_unstaged_and_untracked() {
    let r = Repo::init();
    r.write("a.rs", "one\n");
    r.commit_all("init");
    r.write("new.rs", "n\n");
    r.write("a.rs", "ONE\n");

    let st = StatusSnapshot::collect(r.path()).unwrap().file_statuses();
    assert_eq!(st.get("new.rs").map(|s| (s.marker, s.staged)), Some(('?', false)));
    assert_eq!(st.get("a.rs").map(|s| (s.marker, s.staged)), Some(('M', false)));

    r.git(&["add", "new.rs", "a.rs"]);
    let st = StatusSnapshot::collect(r.path()).unwrap().file_statuses();
    assert_eq!(st.get("new.rs").map(|s| (s.marker, s.staged)), Some(('A', true)));
    assert_eq!(st.get("a.rs").map(|s| (s.marker, s.staged)), Some(('M', true)));
}

// --- row-10: two reviewer panes on ONE repo, comment-persistence concurrency -------------
// A user running two reviewer panes on the same repo shares a SINGLE comments ref:
// `worktree_key` is a pure function of the repo's top-level path (see
// `worktree_key_is_stable_and_path_specific`), so both panes key to
// `refs/reviewr/comments/<same-key>`. The write path (`git::write_state_blob`) is an
// UNCONDITIONAL `git update-ref` — no old-value CAS — and each write is a FULL snapshot of one
// pane's in-memory store. The per-tick guard in `App::persist_comments` compares the IN-PROCESS
// `store.rev()` only, so it cannot see another process's write. Net: last-writer-wins clobber.
// See `.loop-build/LOG.md` (run 5, c2.p1) for the confirmed bug + the recommended CAS-merge fix.

/// Characterization gate (RUNS, green today): pins the current last-writer-wins behavior of the
/// shared comments ref. It asserts pane A's comment is LOST after pane B's stale-snapshot write,
/// so the moment a real CAS/merge fix lands this test turns RED — forcing the fixer to also
/// un-ignore `two_panes_on_one_repo_keep_both_comments`. That coupling is the gate's teeth.
#[test]
fn comments_ref_write_is_last_writer_wins_with_no_cas() {
    let r = Repo::init();
    r.write("a.rs", "a\n");
    r.commit_all("init");
    let key = worktree_key(r.path());

    // Pane A persists its store (contains AONE). Pane B was seeded before AONE existed, so its
    // full-snapshot write does not carry AONE — it overwrites the ref unconditionally.
    write_comments_blob(r.path(), &key, "[pane-A: AONE]").unwrap();
    write_comments_blob(r.path(), &key, "[pane-B: BONE]").unwrap();

    let persisted = read_comments_blob(r.path(), &key).expect("ref exists after the writes");
    assert!(persisted.contains("BONE"), "the last writer's comment survives");
    assert!(
        !persisted.contains("AONE"),
        "clobber: pane A's comment is gone — the shared ref has no CAS/merge (see LOG row-10)"
    );
}

/// Expected-fail reproduction (IGNORED so the suite stays green): the invariant a correct fix
/// must restore — both panes' comments survive a concurrent persist. Run with
/// `cargo test -- --ignored` to watch it fail today; un-ignore once the CAS-merge lands.
#[test]
#[ignore = "row-10 clobber bug: two panes on one repo lose each other's comments; \
            un-ignore when a CAS-merge (or per-instance key) lands. See .loop-build/LOG.md"]
fn two_panes_on_one_repo_keep_both_comments() {
    let r = Repo::init();
    r.write("a.rs", "a\n");
    r.commit_all("init");
    let key = worktree_key(r.path());

    write_comments_blob(r.path(), &key, "[pane-A: AONE]").unwrap(); // pane A
    write_comments_blob(r.path(), &key, "[pane-B: BONE]").unwrap(); // pane B, stale seed

    let persisted = read_comments_blob(r.path(), &key).expect("ref exists after the writes");
    assert!(persisted.contains("AONE"), "pane A's comment must survive pane B's write");
    assert!(persisted.contains("BONE"), "pane B's comment must survive pane A's write");
}

// --- untracked-additions cache: correct counts without re-reading unchanged files ---------

#[test]
fn untracked_addition_counts_stay_correct_across_cached_rescans() {
    let r = Repo::init();
    r.write("base.rs", "x\n");
    r.commit_all("init");
    r.write("u.txt", "one\ntwo\n"); // untracked, 2 lines

    let mut cache = herdr_reviewr::git::AdditionsCache::default();
    let count = |cache: &mut herdr_reviewr::git::AdditionsCache| {
        let snap = StatusSnapshot::collect(r.path()).unwrap();
        let files =
            herdr_reviewr::git::changed_files_cached(r.path(), Scope::Commit, None, &snap, cache)
                .unwrap();
        by_path(&files).get("u.txt").map(|f| f.additions)
    };

    assert_eq!(count(&mut cache), Some(2), "first scan counts the lines");
    assert_eq!(count(&mut cache), Some(2), "a cached rescan keeps the count");

    // An append (size change) must refresh the count, not serve the cached one.
    r.write("u.txt", "one\ntwo\nthree\n");
    assert_eq!(count(&mut cache), Some(3), "an appended line shows on the next scan");

    // A same-size rewrite must refresh via mtime: 6 bytes / 3 lines -> 6 bytes / 1 line.
    r.write("u.txt", "a\nb\nc\n");
    assert_eq!(count(&mut cache), Some(3), "3 one-char lines");
    r.write("u.txt", "abcde\n");
    assert_eq!(count(&mut cache), Some(1), "a same-size rewrite still refreshes the count");

    // A deleted untracked file drops out entirely (and its cache entry with it).
    std::fs::remove_file(r.path().join("u.txt")).unwrap();
    assert_eq!(count(&mut cache), None, "a removed untracked file leaves the changeset");
}
