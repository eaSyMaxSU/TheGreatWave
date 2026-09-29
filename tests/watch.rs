#![cfg(feature = "watch")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// Generous enough for a first window on a CI machine with software rendering.
const PATIENCE: Duration = Duration::from_secs(30);
/// Longer than the debounce, the settle time, and one poll interval together.
const QUIET: Duration = Duration::from_millis(900);

/// A running `tgw` in live mode; killed when dropped.
struct Live {
    child: Child,
    log: PathBuf,
}

impl Live {
    fn start(input: &Path, output: &Path, mode: &str) -> Self {
        let log = input.with_extension("stderr.log");
        let child = tgw()
            .arg(input)
            .args([mode, "--indent", "2", "-o"])
            .arg(output)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(fs::File::create(&log).unwrap())
            .spawn()
            .unwrap();
        Self { child, log }
    }

    fn stderr(&self) -> String {
        fs::read_to_string(&self.log).unwrap_or_default()
    }

    fn assert_running(&mut self, context: &str) {
        if let Some(status) = self.child.try_wait().unwrap() {
            panic!(
                "tgw exited with {status} {context}; stderr:\n{}",
                self.stderr()
            );
        }
    }

    fn wait_for(&mut self, path: &Path, expected: &[u8], context: &str) {
        let started = Instant::now();
        while fs::read(path).ok().as_deref() != Some(expected) {
            self.assert_running(context);
            if started.elapsed() > PATIENCE {
                panic!(
                    "{} never matched the one-off render {context}; stderr:\n{}",
                    path.display(),
                    self.stderr()
                );
            }
            thread::sleep(Duration::from_millis(25));
        }
    }

    fn assert_kept(&mut self, path: &Path, expected: &[u8], context: &str) {
        thread::sleep(QUIET);
        self.assert_running(context);
        assert!(
            fs::read(path).ok().as_deref() == Some(expected),
            "{} changed {context}; stderr:\n{}",
            path.display(),
            self.stderr()
        );
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn tgw() -> Command {
    Command::new(env!("CARGO_BIN_EXE_tgw"))
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tgw-e2e-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// What a one-off `tgw INPUT -o OUTPUT -t 2` writes for `source`.
fn one_off(dir: &Path, source: &str) -> Vec<u8> {
    let input = dir.join("expected.tgw");
    let output = dir.join("expected.svg");
    fs::write(&input, source).unwrap();
    let status = tgw()
        .arg(&input)
        .args(["-t", "2", "-o"])
        .arg(&output)
        .status()
        .unwrap();
    assert!(status.success());
    fs::read(output).unwrap()
}

/// Saves the way editors with "atomic save" do: write a sibling, rename it over.
fn save_by_rename(path: &Path, text: &str) {
    let staged = path.with_extension("tgw~");
    fs::write(&staged, text).unwrap();
    let started = Instant::now();
    // Windows refuses the rename while another process briefly holds the file.
    while let Err(error) = fs::rename(&staged, path) {
        assert!(started.elapsed() < PATIENCE, "rename: {error}");
        thread::sleep(Duration::from_millis(10));
    }
}

/// Drives one live session through good, broken, deleted, and replaced sources.
fn follow_saves(name: &str, mode: &str) {
    let dir = scratch(name);
    let input = dir.join("diagram.tgw");
    let output = dir.join("diagram.svg");
    let first = "clk: p...\n";
    let second = "clk:  p.......\ndata: x.=.x...\n";
    let third = "clk: n...\nreq: 0.1.\n";
    let first_svg = one_off(&dir, first);
    let second_svg = one_off(&dir, second);
    let third_svg = one_off(&dir, third);
    fs::write(&input, first).unwrap();

    let mut live = Live::start(&input, &output, mode);
    live.wait_for(&output, &first_svg, "after starting");

    fs::write(&input, second).unwrap();
    live.wait_for(&output, &second_svg, "after an in-place save");

    fs::write(&input, "clk: p...\nbroken\n").unwrap();
    live.assert_kept(&output, &second_svg, "after a broken save");

    fs::remove_file(&input).unwrap();
    live.assert_kept(&output, &second_svg, "after the diagram was deleted");

    save_by_rename(&input, third);
    live.wait_for(&output, &third_svg, "after a save by rename");

    let log = live.stderr();
    drop(live);
    let leftovers: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name.to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}; stderr:\n{log}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn watch_rewrites_the_output_on_every_good_save() {
    follow_saves("watch", "--watch");
}

#[cfg(feature = "view")]
#[test]
#[ignore = "opens a window; run with --ignored where a display is available"]
fn view_rewrites_the_output_on_every_good_save() {
    follow_saves("view", "--view");
}

#[cfg(all(feature = "view", any(target_os = "linux", target_os = "freebsd")))]
#[test]
fn view_without_a_display_exits_with_advice() {
    let dir = scratch("headless");
    let input = dir.join("diagram.tgw");
    fs::write(&input, "clk: p\n").unwrap();
    let mut child = tgw()
        .arg(&input)
        .arg("--view")
        .env_remove("DISPLAY")
        .env_remove("WAYLAND_DISPLAY")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if started.elapsed() > PATIENCE {
            let _ = child.kill();
            panic!("tgw --view without a display did not exit");
        }
        thread::sleep(Duration::from_millis(25));
    };
    let stderr = std::io::read_to_string(child.stderr.take().unwrap()).unwrap();
    assert!(!status.success());
    assert!(
        stderr.contains("DISPLAY") && stderr.contains("--watch"),
        "{stderr}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn live_mode_rejects_what_it_cannot_keep_current() {
    let dir = scratch("reject");
    let input = dir.join("diagram.tgw");
    fs::write(&input, "clk: p\n").unwrap();
    let path = |p: &Path| p.display().to_string();
    let output = path(&dir.join("diagram.svg"));
    let missing = path(&dir.join("typo.tgw"));
    let cases: [(&[&str], &str); 5] = [
        (&[&path(&input), "--watch"], "needs -o"),
        (
            &[&path(&input), "--watch", "-o", &path(&input)],
            "overwrite",
        ),
        (&[&path(&input), "--watch", "-o", &path(&dir)], "directory"),
        (&[&missing, "--watch", "-o", &output], "typo.tgw"),
        (
            &[&path(&input), "--watch", "--convert", "-o", "x.tgw"],
            "--convert",
        ),
    ];
    for (args, reason) in cases {
        let started = Instant::now();
        let run = tgw().args(args).output().unwrap();
        let stderr = String::from_utf8_lossy(&run.stderr);
        assert!(!run.status.success(), "{args:?}");
        assert!(
            stderr.starts_with("tgw: ") && stderr.contains(reason),
            "{args:?}: {stderr}"
        );
        assert!(started.elapsed() < PATIENCE, "{args:?} did not fail fast");
    }
    assert_eq!(fs::read_to_string(&input).unwrap(), "clk: p\n");
    assert!(!dir.join("diagram.svg").exists());
    let _ = fs::remove_dir_all(&dir);
}
