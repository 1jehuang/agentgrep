//! Cross-mode limits through the public API, without mutating process globals.
use agentgrep::cli::{Cli, Command};
use agentgrep::control::{SearchControl, SearchLimits, with_search_control};
use clap::Parser;
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

fn search(
    root: &Path,
    args: &[&str],
    limits: SearchLimits,
    token: Arc<AtomicBool>,
) -> Result<(), String> {
    let cli =
        Cli::try_parse_from(std::iter::once("agentgrep").chain(args.iter().copied())).unwrap();
    with_search_control(SearchControl::new(token, limits), || match cli.command {
        Command::Grep(args) => agentgrep::search::run_grep(root, &args).map(|_| ()),
        Command::Find(args) => agentgrep::find::run_find(root, &args).map(|_| ()),
        Command::Outline(args) => agentgrep::outline::run_outline(root, &args).map(|_| ()),
        Command::Trace(args) => {
            let query =
                agentgrep::smart_dsl::parse_smart_query(&args.terms).map_err(|e| e.to_string())?;
            agentgrep::smart_engine::run_smart(root, &query, &args).map(|_| ())
        }
    })?
}
fn token() -> Arc<AtomicBool> {
    Arc::new(AtomicBool::new(false))
}

#[test]
fn cancelled_scope_rejects_every_search_mode_and_restores_thread_state() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("needle.rs"), "fn needle() {}\n").unwrap();
    for args in [
        vec!["grep", "needle"],
        vec!["find", "needle"],
        vec!["outline", "needle.rs"],
        vec!["trace", "subject:needle", "relation:defined"],
    ] {
        let cancel = token();
        cancel.store(true, Ordering::Relaxed);
        let error = search(dir.path(), &args, SearchLimits::default(), cancel).unwrap_err();
        assert!(error.contains("cancelled"), "{error}");
        search(dir.path(), &args, SearchLimits::default(), token()).unwrap();
    }
}

#[test]
fn native_modes_fail_clearly_on_file_line_read_and_traversal_limits() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("needle.rs"),
        "fn needle() {}\nfn needle2() {}\n",
    )
    .unwrap();
    let modes = [
        vec!["find", "needle"],
        vec!["outline", "needle.rs"],
        vec!["trace", "subject:needle", "relation:defined"],
    ];
    for args in &modes {
        for (limits, expected) in [
            (
                SearchLimits {
                    max_file_bytes: 4,
                    ..SearchLimits::default()
                },
                "file bytes",
            ),
            (
                SearchLimits {
                    max_line_bytes: 4,
                    ..SearchLimits::default()
                },
                "line bytes",
            ),
            (
                SearchLimits {
                    max_total_read_bytes: 4,
                    ..SearchLimits::default()
                },
                "read bytes",
            ),
            (
                SearchLimits {
                    max_structure_items: 1,
                    ..SearchLimits::default()
                },
                "structure items",
            ),
            (
                SearchLimits {
                    max_file_lines: 1,
                    ..SearchLimits::default()
                },
                "file lines",
            ),
        ] {
            let error = search(dir.path(), args, limits, token()).unwrap_err();
            assert!(error.contains(expected), "{args:?}: {error}");
        }
    }
    for args in [&modes[0], &modes[2]] {
        let error = search(
            dir.path(),
            args,
            SearchLimits {
                max_entries: 0,
                ..SearchLimits::default()
            },
            token(),
        )
        .unwrap_err();
        assert!(error.contains("entries"), "{error}");
    }
}

#[test]
fn scopes_restore_on_unwind_and_are_thread_local() {
    let cancel = token();
    cancel.store(true, Ordering::Relaxed);
    let result = std::thread::spawn(move || {
        search(
            Path::new("."),
            &["find", "needle"],
            SearchLimits::default(),
            cancel,
        )
    })
    .join()
    .unwrap();
    assert!(result.unwrap_err().contains("cancelled"));
    let _ = std::panic::catch_unwind(|| {
        let _ = with_search_control(SearchControl::new(token(), SearchLimits::default()), || {
            panic!("test unwind")
        });
    });
    let dir = tempfile::tempdir().unwrap();
    search(
        dir.path(),
        &["find", "needle"],
        SearchLimits::default(),
        token(),
    )
    .unwrap();
}

#[cfg(windows)]
#[test]
fn junction_traversal_is_bounded_and_no_follow_skips_external_tree() {
    use std::process::{Command as ProcessCommand, Stdio};
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("needle_local.rs"),
        "fn needle_local() {}\n",
    )
    .unwrap();
    for index in 0..32 {
        std::fs::write(
            outside.path().join(format!("needle_{index}.rs")),
            "fn needle() {}\n",
        )
        .unwrap();
    }
    let junction = root.path().join("linked");
    // Directory junctions need no Developer Mode or symlink privilege.
    let status = ProcessCommand::new("cmd")
        .args(["/D", "/C", "mklink", "/J"])
        .arg(&junction)
        .arg(outside.path())
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "mklink /J failed: {status}");

    let cli = Cli::try_parse_from([
        "agentgrep",
        "find",
        "needle",
        "--no-ignore",
        "--max-files",
        "100",
    ])
    .unwrap();
    let Command::Find(mut args) = cli.command else {
        unreachable!()
    };
    let limits = SearchLimits {
        max_entries: 8,
        ..SearchLimits::default()
    };
    let followed = with_search_control(SearchControl::new(token(), limits.clone()), || {
        agentgrep::find::run_find(root.path(), &args)
    });
    assert!(followed.unwrap_err().contains("entries"));

    args.no_follow = true;
    let local = with_search_control(SearchControl::new(token(), limits), || {
        agentgrep::find::run_find(root.path(), &args)
    })
    .unwrap()
    .unwrap();
    assert_eq!(local.files.len(), 1);
    assert_eq!(local.files[0].path, "needle_local.rs");

    args.no_follow = false;
    let followed = agentgrep::find::run_find(root.path(), &args).unwrap();
    assert_eq!(
        followed.files.len(),
        33,
        "ordinary junction following regressed"
    );
    // Explicitly unlink the junction before TempDir cleanup, never its target.
    std::fs::remove_dir(&junction).unwrap();
}
