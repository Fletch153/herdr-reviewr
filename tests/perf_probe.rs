// Perf bench (not a gate): times reload() phases on synthetic many-file repos and a real one.
mod common;

use std::time::Instant;

use common::Repo;
use herdr_reviewr::app::{App, Tab};
use herdr_reviewr::model::Scope;

const N: usize = 50_000;

fn git_phases(repo: &std::path::Path) {
    use herdr_reviewr::git;
    let snap = time("git::StatusSnapshot::collect", || git::StatusSnapshot::collect(repo).unwrap());
    time("git::changed_files", || {
        git::changed_files(repo, Scope::Commit, None, &snap).unwrap().len()
    });
    time("snap.file_statuses", || snap.file_statuses().len());
    time("git::all_files", || git::all_files(repo, &snap).unwrap().len());
}

fn time<R>(label: &str, mut f: impl FnMut() -> R) -> R {
    let t = Instant::now();
    let r = f();
    println!("  {label:32} {:>8.1} ms", t.elapsed().as_secs_f64() * 1000.0);
    r
}

fn frame_cost(app: &herdr_reviewr::app::App) {
    use ratatui::{Terminal, backend::TestBackend};
    let mut terminal = Terminal::new(TestBackend::new(200, 50)).unwrap();
    let t = Instant::now();
    for _ in 0..30 {
        terminal.draw(|f| herdr_reviewr::ui::render(f, app)).unwrap();
    }
    println!(
        "  {:32} {:>8.1} ms",
        "render frame (avg of 30)",
        t.elapsed().as_secs_f64() / 30.0 * 1000.0
    );
}

fn probe(label: &str, r: &Repo) {
    println!("== {label} (N={N})");
    let mut app = App::new(r.path_buf(), Scope::Commit, None);
    time("first reload (Changes)", || app.reload().unwrap());
    time("poll reload (Changes)", || app.reload().unwrap());
    println!("  rebuilds so far: {}", app.rebuild_count);
    frame_cost(&app);
    time("set_tab AllFiles", || app.set_tab(Tab::AllFiles).unwrap());
    time("poll reload (AllFiles)", || app.reload().unwrap());
    time("poll reload (AllFiles) 2", || app.reload().unwrap());
    println!("  rebuilds so far: {}", app.rebuild_count);
    frame_cost(&app);
    git_phases(&r.path_buf());
}

#[test]
#[ignore = "perf bench, run explicitly: HERDR_PROBE_DIR=<repo> cargo test --release --test perf_probe -- --ignored --nocapture"]
fn perf_probe_real_dir() {
    let Ok(dir) = std::env::var("HERDR_PROBE_DIR") else { return };
    println!("== real dir {dir}");
    let mut app = App::new(dir.clone().into(), Scope::Commit, None);
    time("first reload (Changes)", || app.reload().unwrap());
    time("poll reload (Changes)", || app.reload().unwrap());
    time("set_tab AllFiles", || app.set_tab(Tab::AllFiles).unwrap());
    time("poll reload (AllFiles)", || app.reload().unwrap());
    time("poll reload (AllFiles) 2", || app.reload().unwrap());
    git_phases(std::path::Path::new(&dir));
}

#[test]
#[ignore = "perf bench, run explicitly: cargo test --release --test perf_probe -- --ignored --nocapture"]
fn perf_probe() {
    // A: N tracked, committed, clean tree.
    let a = Repo::init();
    for i in 0..N {
        a.write(&format!("src/d{}/f{:04}.rs", i % 50, i), "fn x() {}\n// body\n");
    }
    a.commit_all("init");
    probe("tracked+clean", &a);

    // B: N untracked files (~1 KB each) on top of one committed file.
    let b = Repo::init();
    b.write("README.md", "hi\n");
    b.commit_all("init");
    let body = "line of text\n".repeat(80);
    for i in 0..N {
        b.write(&format!("data/d{}/u{:04}.txt", i % 50, i), &body);
    }
    probe("untracked", &b);

    // C: N ignored files.
    let c = Repo::init();
    c.write(".gitignore", "blob/\n");
    c.commit_all("init");
    for i in 0..N {
        c.write(&format!("blob/d{}/g{:04}.dat", i % 50, i), "x\n");
    }
    probe("ignored", &c);
}
