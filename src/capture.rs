//! Bounded concurrent pipe draining. Never wait for a child with undrained pipes.
use crate::control;
use std::io::{self, Read};
use std::process::{Child, Command, Output, Stdio};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread;
use std::time::Duration;

#[cfg(test)]
#[path = "capture_tests.rs"]
mod tests;

struct Reap(Child);
impl Drop for Reap {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[cfg(unix)]
fn nonblocking(pipe: &impl std::os::fd::AsRawFd) -> io::Result<()> {
    let fd = pipe.as_raw_fd();
    // SAFETY: fcntl only reads/updates flags on the live, owned pipe descriptor.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub(crate) fn output(command: &mut Command) -> io::Result<Output> {
    control::checkpoint().map_err(io::Error::other)?;
    let mut child = Reap(
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?,
    );
    let stdout = child.0.stdout.take().unwrap();
    let stderr = child.0.stderr.take().unwrap();
    #[cfg(unix)]
    {
        nonblocking(&stdout)?;
        nonblocking(&stderr)?;
    }
    let stop = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::sync_channel(2);
    let pump = |mut pipe: Box<dyn Read + Send>,
                is_stderr,
                tx: mpsc::SyncSender<_>,
                stop: Arc<AtomicBool>| {
        thread::spawn(move || {
            let mut buf = [0; 8192];
            while !stop.load(Ordering::Relaxed) {
                match pipe.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if tx.send((is_stderr, Ok(buf[..n].to_vec()))).is_err() {
                            break;
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(2))
                    }
                    Err(e) => {
                        let _ = tx.send((is_stderr, Err(e)));
                        break;
                    }
                }
            }
        })
    };
    let out_reader = pump(Box::new(stdout), false, tx.clone(), stop.clone());
    let err_reader = pump(Box::new(stderr), true, tx, stop.clone());
    let limits = control::limits();
    let mut out = Vec::new();
    let mut err = Vec::new();
    let mut line_bytes = 0usize;
    let result = (|| {
        loop {
            control::checkpoint().map_err(io::Error::other)?;
            match rx.recv_timeout(Duration::from_millis(5)) {
                Ok((is_stderr, bytes)) => {
                    let bytes = bytes?;
                    let (target, limit) = if is_stderr {
                        (&mut err, limits.max_stderr_bytes)
                    } else {
                        (&mut out, limits.max_output_bytes)
                    };
                    if bytes.len() > limit.saturating_sub(target.len()) {
                        return Err(io::Error::other(control::fail(format!(
                            "search resource limit exceeded: {} bytes (limit {limit})",
                            if is_stderr { "stderr" } else { "stdout" }
                        ))));
                    }
                    if !is_stderr {
                        for byte in &bytes {
                            if *byte == b'\n' || *byte == 0 {
                                line_bytes = 0;
                            } else {
                                line_bytes += 1;
                            }
                            if line_bytes > limits.max_line_bytes {
                                return Err(io::Error::other(control::fail(format!(
                                    "search resource limit exceeded: output line bytes (limit {})",
                                    limits.max_line_bytes,
                                ))));
                            }
                        }
                    }
                    target.extend_from_slice(&bytes);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        // Pipes can close before the process exits. Poll rather than blocking
        // in wait so deadline/cancellation still kill a child that stays alive.
        let status = loop {
            control::checkpoint().map_err(io::Error::other)?;
            if let Some(status) = child.0.try_wait()? {
                break status;
            }
            thread::sleep(Duration::from_millis(5));
        };
        Ok(Output {
            status,
            stdout: out,
            stderr: err,
        })
    })();
    stop.store(true, Ordering::Relaxed);
    if result.is_err() {
        let _ = child.0.kill();
    }
    let _ = child.0.wait();
    drop(rx); // Release senders blocked on the bounded channel before joining.
    let _ = out_reader.join();
    let _ = err_reader.join();
    result
}
