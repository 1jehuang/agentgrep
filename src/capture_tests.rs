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
        "hold" => {
            let release = std::env::var_os("AGENTGREP_CAPTURE_RELEASE").unwrap();
            let start = Instant::now();
            while !Path::new(&release).exists() && start.elapsed() < Duration::from_secs(10) {
                thread::sleep(Duration::from_millis(2));
            }
            std::fs::write(Path::new(&release).with_extension("done"), b"done").unwrap();
            std::process::exit(0);
        }
        "descendant" => {
            let pid = std::env::var_os("AGENTGREP_CAPTURE_DESCENDANT_PID").unwrap();
            let mut descendant = command("hold", Path::new(&pid))
                .stdin(Stdio::null())
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .spawn()
                .unwrap();
            let _ = descendant.wait();
            std::process::exit(0);
        }
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

fn pid_ready(path: &Path) -> bool {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| text.parse::<u32>().ok())
        .is_some()
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
#[cfg(windows)]
struct WindowsProcess(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl WindowsProcess {
    fn open(path: &Path) -> io::Result<Self> {
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
        };
        let pid = std::fs::read_to_string(path)?.parse::<u32>().unwrap();
        // SAFETY: only query/synchronize access is requested for our child PID.
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                pid,
            )
        };
        if handle.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(handle))
        }
    }
}

#[cfg(windows)]
impl Drop for WindowsProcess {
    fn drop(&mut self) {
        // SAFETY: this wrapper uniquely owns a successful OpenProcess handle.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
fn assert_reaped(path: &Path) {
    use windows_sys::Win32::Foundation::{ERROR_INVALID_PARAMETER, STILL_ACTIVE, WAIT_OBJECT_0};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    let process = match WindowsProcess::open(path) {
        Ok(process) => process,
        Err(error) => {
            // Windows has already destroyed the process object when no handle
            // remains. Any other error is not evidence that the child exited.
            assert_eq!(error.raw_os_error(), Some(ERROR_INVALID_PARAMETER as i32));
            return;
        }
    };
    // SAFETY: the query/synchronize handle is live until this wrapper drops.
    assert_eq!(
        unsafe { WaitForSingleObject(process.0, 0) },
        WAIT_OBJECT_0,
        "child is still running"
    );
    let mut code = 0;
    assert_ne!(unsafe { GetExitCodeProcess(process.0, &mut code) }, 0);
    assert_ne!(code, STILL_ACTIVE as u32, "child has no exit status");
}

#[cfg(not(any(unix, windows)))]
fn assert_reaped(_path: &Path) {}

#[cfg(unix)]
fn process_is_running(path: &Path) -> bool {
    let pid: i32 = std::fs::read_to_string(path).unwrap().parse().unwrap();
    // SAFETY: signal zero only queries the existence of this fixture process.
    unsafe { libc::kill(pid, 0) == 0 }
}

#[cfg(windows)]
fn process_is_running(path: &Path) -> bool {
    use windows_sys::Win32::Foundation::{ERROR_INVALID_PARAMETER, WAIT_TIMEOUT};
    let process = match WindowsProcess::open(path) {
        Ok(process) => process,
        Err(error) => {
            assert_eq!(error.raw_os_error(), Some(ERROR_INVALID_PARAMETER as i32));
            return false;
        }
    };
    // SAFETY: the synchronize handle is owned and live.
    unsafe {
        windows_sys::Win32::System::Threading::WaitForSingleObject(process.0, 0) == WAIT_TIMEOUT
    }
}

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
        while !pid_ready(&ready) {
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

#[cfg(any(unix, windows))]
#[test]
fn cancellation_does_not_wait_for_inherited_pipe_writers() {
    let dir = tempfile::tempdir().unwrap();
    let pid = dir.path().join("parent-pid");
    let descendant = dir.path().join("descendant-pid");
    let release = dir.path().join("release");
    let ready = descendant.clone();
    let release_from_trigger = release.clone();
    let token = Arc::new(AtomicBool::new(false));
    let cancel = token.clone();
    let (started_tx, started_rx) = mpsc::channel();
    let (cleanup_tx, cleanup_rx) = mpsc::channel();
    let trigger = thread::spawn(move || {
        let start = Instant::now();
        while !pid_ready(&ready) {
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "descendant did not start"
            );
            thread::sleep(Duration::from_millis(2));
        }
        started_tx.send(Instant::now()).unwrap();
        cancel.store(true, Ordering::Relaxed);
        // Release even a broken implementation after three seconds, so this
        // regression fails instead of hanging indefinitely in a reader join.
        let _ = cleanup_rx.recv_timeout(Duration::from_secs(3));
        std::fs::write(&release_from_trigger, b"release").unwrap();
        let start = Instant::now();
        while !release_from_trigger.with_extension("done").exists() {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "descendant did not release its pipes"
            );
            thread::sleep(Duration::from_millis(2));
        }
    });
    let result = run(
        command("descendant", &pid)
            .env("AGENTGREP_CAPTURE_DESCENDANT_PID", &descendant)
            .env("AGENTGREP_CAPTURE_RELEASE", &release),
        token,
        SearchLimits::default(),
    );
    let elapsed = started_rx.recv().unwrap().elapsed();
    let still_running = process_is_running(&descendant);
    let _ = cleanup_tx.send(());
    trigger.join().unwrap();
    assert!(result.unwrap_err().contains("cancelled"));
    assert!(
        elapsed < Duration::from_secs(2),
        "reader waited for descendant: {elapsed:?}"
    );
    assert!(still_running, "test did not retain inherited pipe writers");
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
