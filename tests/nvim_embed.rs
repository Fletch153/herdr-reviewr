//! Integration tests for the embedded-nvim engine against a real `nvim --embed --clean`.
//! Skipped cleanly when nvim is not on the PATH. All waits are deadline-polled — no bare
//! sleeps — and time out with a full grid dump so failures are diagnosable.

use std::path::Path;
use std::time::{Duration, Instant};

use herdr_reviewr::nvim::{Nvim, RpcFailure, StartOpts, nvim_present};

const DEADLINE: Duration = Duration::from_secs(5);

fn start(repo: &Path, cols: u16, rows: u16) -> Nvim {
    Nvim::start(repo, cols, rows, &StartOpts { clean: true, rtp: None }).expect("nvim starts")
}

/// Poll `pred` every 10ms up to the deadline; on expiry, panic with the grid contents.
fn wait_for(nv: &mut Nvim, what: &str, mut pred: impl FnMut(&mut Nvim) -> bool) {
    let deadline = Instant::now() + DEADLINE;
    while Instant::now() < deadline {
        if pred(nv) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let dump: Vec<String> = {
        let grid = nv.grid();
        (0..grid.rows).map(|r| grid.row_text(r)).collect()
    };
    panic!("timed out waiting for {what}; grid:\n{}", dump.join("\n"));
}

fn grid_contains(nv: &mut Nvim, needle: &str) -> bool {
    let grid = nv.grid();
    (0..grid.rows).any(|r| grid.row_text(r).contains(needle))
}

#[test]
fn attach_type_and_read_back() {
    if !nvim_present() {
        eprintln!("skipping: nvim not on PATH");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut nv = start(dir.path(), 60, 18);
    wait_for(&mut nv, "first paint", |nv| nv.needs_redraw() || grid_contains(nv, "~"));
    nv.input("ihello embedded<Esc>").unwrap();
    wait_for(&mut nv, "typed text to paint", |nv| grid_contains(nv, "hello embedded"));
    // The request round-trip doubles as a liveness check.
    let line = nv.eval("getline('.')").unwrap();
    assert_eq!(line.as_str(), Some("hello embedded"));
}

#[test]
fn open_file_renders_and_modified_flag_tracks() {
    if !nvim_present() {
        eprintln!("skipping: nvim not on PATH");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    // Space + single quote in the name exercises both escaping layers of open_file.
    let file = dir.path().join("a b'c.txt");
    std::fs::write(&file, "sentinel-content\n").unwrap();
    let mut nv = start(dir.path(), 60, 18);
    // Open from insert mode: the payload's `stopinsert` prefix must normalize it first.
    nv.input("i").unwrap();
    wait_for(&mut nv, "insert mode", |nv| nv.grid().mode.starts_with("insert"));
    nv.open_file(&file, false).unwrap();
    wait_for(&mut nv, "file contents", |nv| grid_contains(nv, "sentinel-content"));
    wait_for(&mut nv, "unmodified flag", |nv| {
        nv.eval("&modified").ok().and_then(|v| v.as_i64()) == Some(0)
    });
    nv.input("A!<Esc>").unwrap();
    wait_for(&mut nv, "modified flag", |nv| {
        nv.eval("&modified").ok().and_then(|v| v.as_i64()) == Some(1)
    });
}

#[test]
fn command_surfaces_nvim_errors() {
    if !nvim_present() {
        eprintln!("skipping: nvim not on PATH");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut nv = start(dir.path(), 40, 10);
    match nv.command("NotARealCommand") {
        Err(RpcFailure::Nvim(msg)) => assert!(msg.contains("E492"), "unexpected: {msg}"),
        other => panic!("expected an nvim error, got {other:?}"),
    }
}

#[test]
fn resize_propagates_to_the_grid() {
    if !nvim_present() {
        eprintln!("skipping: nvim not on PATH");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut nv = start(dir.path(), 40, 10);
    nv.resize(100, 30).unwrap();
    wait_for(&mut nv, "grid to resize", |nv| {
        let g = nv.grid();
        (g.cols, g.rows) == (100, 30)
    });
}

#[test]
fn death_is_detected_not_panicked() {
    if !nvim_present() {
        eprintln!("skipping: nvim not on PATH");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut nv = start(dir.path(), 40, 10);
    nv.command_fire("qa!").unwrap();
    wait_for(&mut nv, "death detection", |nv| !nv.is_running());
    assert!(nv.died().is_some());
    assert!(nv.input("j").is_err(), "input into a dead engine must error, not panic");
}

#[test]
fn shutdown_force_reaps_cleanly() {
    if !nvim_present() {
        eprintln!("skipping: nvim not on PATH");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut nv = start(dir.path(), 40, 10);
    nv.shutdown(true).unwrap();
    assert!(!nv.is_running());
    drop(nv); // completing under the harness timeout is the no-hang assertion
}
