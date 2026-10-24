use super::*;
use crate::control::{SearchControl, SearchLimits, with_search_control};
use std::io::Write;
use std::path::Path;
use std::time::Instant;

// A portable subprocess fixture, invoked only via this test executable.
// No shell, external utility, global environment mutation, or PATH races.
#[test]
#[ignore]
fn child_fixture() {
    let Ok(mode) = std::env::var("AGENTGREP_CAPTURE_FIXTURE") else {
        return;
    };
    if let Ok(path) = std::env::var("AGENTGREP_CAPTURE_PID") {
        std::fs::write(path, std::process::id().to_string()).unwrap();
    }
    let chunk = if mode == "line" {
        vec![b'x'; 8192]
    } else {
        b"x\n".repeat(4096)
    };
    match mode.as_str() {
        "ordinary" => {
            std::io::stdout().write_all(b"hello\n").unwrap();
            std::io::stderr().write_all(b"warning\n").unwrap();
            std::process::exit(0);
        }
        "idle" => loop {
            thread::sleep(Duration::from_millis(10));
        },
        "stderr" => loop {
            if std::io::stderr().write_all(&chunk).is_err() {
                std::process::exit(0);
            }
        },
        _ => loop {
            if std::io::stdout().write_all(&chunk).is_err() {
                std::process::exit(0);
            }
        },
    }
}

fn command(mode: &str, pid: &Path) -> Command {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args([
        "--exact",
        "capture::tests::child_fixture",
        "--ignored",
        "--nocapture",
    ])
    .env("AGENTGREP_CAPTURE_FIXTURE", mode)
    .env("AGENTGREP_CAPTURE_PID", pid);
    cmd
}

fn run(cmd: &mut Command, token: Arc<AtomicBool>, limits: SearchLimits) -> Result<Output, String> {
    with_search_control(SearchControl::new(token, limits), || output(cmd))?
        .map_err(|e| e.to_string())
}

#[cfg(unix)]
fn assert_reaped(path: &Path) {
    let pid: i32 = std::fs::read_to_string(path).unwrap().parse().unwrap();
    let mut status = 0;
    // SAFETY: WNOHANG never blocks and the PID belongs to our completed child.
    assert_eq!(
        unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
    assert_eq!(unsafe { libc::kill(pid, 0) }, -1, "child still exists");
    assert_eq!(io::Error::last_os_error().raw_os_error(), Some(libc::ESRCH));
}
#[cfg(not(unix))]
fn assert_reaped(_path: &Path) {}

#[test]
fn bounded_stdout_stderr_and_single_line_kill_and_reap() {
    for (mode, expected) in [
        ("stdout", "stdout bytes"),
        ("stderr", "stderr bytes"),
        ("line", "output line bytes"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let pid = dir.path().join("pid");
        let limits = SearchLimits {
            max_output_bytes: 64 * 1024,
            max_stderr_bytes: 32 * 1024,
            max_line_bytes: 16 * 1024,
            ..SearchLimits::default()
        };
        let error = run(
            &mut command(mode, &pid),
            Arc::new(AtomicBool::new(false)),
            limits,
        )
        .unwrap_err();
        assert!(error.contains(expected), "{mode}: {error}");
        assert_reaped(&pid);
    }
}

#[test]
fn cancellation_interrupts_idle_child_and_reaps_it() {
    let dir = tempfile::tempdir().unwrap();
    let pid = dir.path().join("pid");
    let ready = pid.clone();
    let token = Arc::new(AtomicBool::new(false));
    let cancel = token.clone();
    let trigger = thread::spawn(move || {
        let start = Instant::now();
        while !ready.exists() {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "child did not start"
            );
            thread::sleep(Duration::from_millis(2));
        }
        cancel.store(true, Ordering::Relaxed);
    });
    let error = run(&mut command("idle", &pid), token, SearchLimits::default()).unwrap_err();
    trigger.join().unwrap();
    assert!(error.contains("cancelled"), "{error}");
    assert_reaped(&pid);
}

#[test]
fn deadline_interrupts_idle_child_and_reaps_it() {
    let dir = tempfile::tempdir().unwrap();
    let pid = dir.path().join("pid");
    let limits = SearchLimits {
        timeout: Duration::from_secs(1),
        ..SearchLimits::default()
    };
    let error = run(
        &mut command("idle", &pid),
        Arc::new(AtomicBool::new(false)),
        limits,
    )
    .unwrap_err();
    assert!(error.contains("deadline"), "{error}");
    assert_reaped(&pid);
}

#[test]
fn ordinary_capture_preserves_both_streams() {
    let dir = tempfile::tempdir().unwrap();
    let output = run(
        &mut command("ordinary", &dir.path().join("pid")),
        Arc::new(AtomicBool::new(false)),
        SearchLimits::default(),
    )
    .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("hello\n"));
    assert_eq!(output.stderr, b"warning\n");
}
