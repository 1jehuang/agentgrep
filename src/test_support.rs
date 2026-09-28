//! Test-only helpers shared across module test suites.
//!
//! Kept in one place so filesystem capability probes stay consistent between
//! `workspace`, `search`, and `outline` tests.

use std::path::Path;

/// Whether `dir` can hold files whose names are not valid UTF-8.
///
/// Several tests build corpora with raw byte names such as `a\xff.rs` to lock
/// in raw-byte ordering, glob matching, and display disambiguation. Those names
/// are representable on Linux and most Unix filesystems, but APFS (macOS)
/// enforces UTF-8 filenames and rejects the write with `EILSEQ`
/// ("Illegal byte sequence"). Windows filesystems reject them too.
///
/// Tests that need such a corpus should call this first and return early when
/// it is false, so the suite stays green on macOS while retaining full coverage
/// on platforms where the scenario is actually reachable.
#[cfg(unix)]
pub fn supports_non_utf8_filenames(dir: &Path) -> bool {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let probe = dir.join(OsStr::from_bytes(b"\xff.agentgrep-probe"));
    match std::fs::write(&probe, "") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

#[cfg(not(unix))]
pub fn supports_non_utf8_filenames(_dir: &Path) -> bool {
    false
}

/// Emit a skip notice so a silently-skipped test is still visible in output.
pub fn skip_non_utf8(test_name: &str) {
    eprintln!(
        "skipping {test_name}: filesystem rejects non-UTF-8 filenames \
         (expected on macOS/APFS and Windows)"
    );
}
