#![cfg(feature = "watch")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const PATIENCE: Duration = Duration::from_secs(15);

struct Watcher(Child);

impl Drop for Watcher {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
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

fn wait_for(path: &Path, expected: &[u8], watcher: &mut Watcher) {
    let started = Instant::now();
    while fs::read(path).ok().as_deref() != Some(expected) {
        if let Some(status) = watcher.0.try_wait().unwrap() {
            panic!("tgw --watch exited early with {status}");
        }
        assert!(
            started.elapsed() < PATIENCE,
            "{} never matched the one-off render",
            path.display()
        );
        thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn watch_rewrites_the_output_on_every_good_save() {
    let dir = scratch("watch");
    let input = dir.join("diagram.tgw");
    let output = dir.join("diagram.svg");
    let first = "clk: p...\n";
    let second = "clk:  p.......\ndata: x.=.x...\n";
    let third = "clk: n...\nreq: 0.1.\n";
    let first_svg = one_off(&dir, first);
    let second_svg = one_off(&dir, second);
    let third_svg = one_off(&dir, third);
    fs::write(&input, first).unwrap();

    let mut watcher = Watcher(
        tgw()
            .arg(&input)
            .args(["--watch", "--indent", "2", "-o"])
            .arg(&output)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    wait_for(&output, &first_svg, &mut watcher);

    fs::write(&input, second).unwrap();
    wait_for(&output, &second_svg, &mut watcher);

    fs::write(&input, "clk: p...\nbroken\n").unwrap();
    thread::sleep(Duration::from_millis(900));
    assert_eq!(
        fs::read(&output).unwrap(),
        second_svg,
        "a bad save kept the last output"
    );
    assert!(
        watcher.0.try_wait().unwrap().is_none(),
        "a bad save stopped tgw"
    );

    fs::write(&input, third).unwrap();
    wait_for(&output, &third_svg, &mut watcher);

    drop(watcher);
    let leftovers: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .filter(|name| name.to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn live_mode_rejects_what_it_cannot_keep_current() {
    let dir = scratch("reject");
    let input = dir.join("diagram.tgw");
    fs::write(&input, "clk: p\n").unwrap();
    for args in [
        vec!["--watch".to_string()],
        vec!["--watch".into(), "-o".into(), input.display().to_string()],
        vec![
            "--watch".into(),
            "--convert".into(),
            "-o".into(),
            "x.tgw".into(),
        ],
    ] {
        let run = tgw().arg(&input).args(&args).output().unwrap();
        assert!(!run.status.success(), "{args:?}");
        assert!(
            String::from_utf8_lossy(&run.stderr).starts_with("tgw: "),
            "{args:?}"
        );
    }
    assert_eq!(fs::read_to_string(&input).unwrap(), "clk: p\n");
    let _ = fs::remove_dir_all(&dir);
}
