//! Regression tests for non-UTF-8 filenames whose lossy display strings
//! collide (e.g. b"a\xff.txt" and b"a\xfe.txt" both lossy-decode to
//! "a\u{FFFD}.txt").
//!
//! Requirements verified here:
//! - every mode (grep, grep --paths-only, find, trace, outline) emits a
//!   UNIQUE display path per file, so consumers that dedup on the displayed
//!   path cannot silently drop a result;
//! - JSON output carries a `path_bytes` hex field so consumers can address
//!   the real file;
//! - result ordering is deterministic (native byte order), independent of
//!   filesystem readdir order;
//! - outline round-trips the disambiguated display paths emitted by the
//!   other modes.
#![cfg(unix)]

use agentgrep::cli::{FindArgs, FullRegionMode, GrepArgs, OutlineArgs, SmartArgs};
use agentgrep::find::run_find;
use agentgrep::outline::run_outline;
use agentgrep::search::run_grep;
use agentgrep::smart_dsl::{Relation, SmartQuery};
use agentgrep::smart_engine::run_smart;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use tempfile::{TempDir, tempdir};

const FF_NAME: &[u8] = b"collide\xff.rs";
const FE_NAME: &[u8] = b"collide\xfe.rs";
const FF_DISPLAY: &str = "collide\u{FFFD}.rs#b=ff";
const FE_DISPLAY: &str = "collide\u{FFFD}.rs#b=fe";

/// Build a collision corpus, creating the two collider files in the given
/// order so tests can prove independence from creation/readdir order.
fn collision_corpus(ff_first: bool) -> TempDir {
    let dir = tempdir().unwrap();
    let write_ff = || {
        fs::write(
            dir.path().join(OsStr::from_bytes(FF_NAME)),
            "fn collide_fn() { let marker = \"needle_xyz MARKER_FF\"; }\n",
        )
        .unwrap();
    };
    let write_fe = || {
        fs::write(
            dir.path().join(OsStr::from_bytes(FE_NAME)),
            "fn collide_fn() { let marker = \"needle_xyz MARKER_FE\"; }\n",
        )
        .unwrap();
    };
    if ff_first {
        write_ff();
        write_fe();
    } else {
        write_fe();
        write_ff();
    }
    fs::write(
        dir.path().join("clean.rs"),
        "fn collide_fn_helper() { let _ = \"needle_xyz clean\"; }\n",
    )
    .unwrap();
    dir
}

fn grep_args(query: &str) -> GrepArgs {
    GrepArgs {
        query: query.to_string(),
        regex: false,
        file_type: None,
        json: false,
        paths_only: false,
        hidden: false,
        no_ignore: true,
        path: None,
        glob: None,
    }
}

fn find_args(parts: &[&str]) -> FindArgs {
    FindArgs {
        query_parts: parts.iter().map(|s| s.to_string()).collect(),
        file_type: None,
        json: false,
        paths_only: false,
        debug_score: false,
        max_files: 10,
        hidden: false,
        no_ignore: true,
        path: None,
        glob: None,
    }
}

fn smart_args() -> SmartArgs {
    SmartArgs {
        terms: vec![],
        json: false,
        max_files: 10,
        max_regions: 6,
        full_region: FullRegionMode::Auto,
        debug_plan: false,
        debug_score: false,
        paths_only: false,
        path: None,
        file_type: None,
        glob: None,
        hidden: false,
        no_ignore: true,
        context_json: None,
    }
}

fn assert_unique<'a>(paths: impl Iterator<Item = &'a str>, mode: &str) {
    let paths: Vec<&str> = paths.collect();
    let unique: HashSet<&str> = paths.iter().copied().collect();
    assert_eq!(
        unique.len(),
        paths.len(),
        "{mode}: displayed paths must be unique, got {paths:?}"
    );
}

#[test]
fn grep_emits_unique_display_paths_and_path_bytes_for_colliders() {
    let dir = collision_corpus(true);
    let result = run_grep(dir.path(), &grep_args("needle_xyz")).unwrap();

    assert_eq!(result.total_files, 3);
    let paths: Vec<&str> = result.files.iter().map(|f| f.path.as_str()).collect();
    assert_unique(paths.iter().copied(), "grep");
    assert!(
        paths.contains(&FF_DISPLAY),
        "missing {FF_DISPLAY}: {paths:?}"
    );
    assert!(
        paths.contains(&FE_DISPLAY),
        "missing {FE_DISPLAY}: {paths:?}"
    );
    assert!(paths.contains(&"clean.rs"));

    // JSON view exposes path_bytes hex for non-UTF-8 names only.
    let json = result.to_json();
    for file in &json.files {
        if file.path == FF_DISPLAY {
            assert_eq!(file.path_bytes.as_deref(), Some("636f6c6c696465ff2e7273"));
        } else if file.path == FE_DISPLAY {
            assert_eq!(file.path_bytes.as_deref(), Some("636f6c6c696465fe2e7273"));
        } else {
            assert_eq!(file.path_bytes, None, "utf-8 path must omit path_bytes");
        }
    }

    // The two colliders keep their own distinct content.
    let ff = result.files.iter().find(|f| f.path == FF_DISPLAY).unwrap();
    assert!(ff.matches[0].line_text.contains("MARKER_FF"));
    let fe = result.files.iter().find(|f| f.path == FE_DISPLAY).unwrap();
    assert!(fe.matches[0].line_text.contains("MARKER_FE"));
}

#[test]
fn grep_paths_only_emits_unique_display_paths() {
    let dir = collision_corpus(true);
    let mut args = grep_args("needle_xyz");
    args.paths_only = true;

    let result = run_grep(dir.path(), &args).unwrap();
    assert_eq!(result.total_files, 3);
    let paths: Vec<&str> = result.files.iter().map(|f| f.path.as_str()).collect();
    assert_unique(paths.iter().copied(), "grep --paths-only");
    assert!(paths.contains(&FF_DISPLAY), "{paths:?}");
    assert!(paths.contains(&FE_DISPLAY), "{paths:?}");
}

#[test]
fn find_emits_unique_display_paths_for_colliders() {
    let dir = collision_corpus(true);
    let result = run_find(dir.path(), &find_args(&["collide"]));

    let paths: Vec<&str> = result.files.iter().map(|f| f.path.as_str()).collect();
    assert_unique(paths.iter().copied(), "find");
    assert!(paths.contains(&FF_DISPLAY), "{paths:?}");
    assert!(paths.contains(&FE_DISPLAY), "{paths:?}");

    for file in &result.files {
        if file.path == FF_DISPLAY || file.path == FE_DISPLAY {
            assert!(file.path_bytes.is_some(), "collider must carry path_bytes");
        } else {
            assert!(file.path_bytes.is_none());
        }
    }
}

#[test]
fn trace_emits_unique_display_paths_for_colliders() {
    let dir = collision_corpus(true);
    let query = SmartQuery {
        subject: "collide_fn".to_string(),
        relation: Relation::Defined,
        support: vec![],
        kind: None,
        path_hint: None,
    };
    let result = run_smart(dir.path(), &query, &smart_args()).unwrap();

    let paths: Vec<&str> = result.files.iter().map(|f| f.path.as_str()).collect();
    assert_unique(paths.iter().copied(), "trace");
    assert!(paths.contains(&FF_DISPLAY), "{paths:?}");
    assert!(paths.contains(&FE_DISPLAY), "{paths:?}");
    for file in &result.files {
        if file.path == FF_DISPLAY || file.path == FE_DISPLAY {
            assert!(file.path_bytes.is_some());
        }
    }
}

/// Tie-break stability: identical corpora created in opposite orders must
/// produce identical result orderings in every mode, with the colliding
/// display names in byte order (fe before ff).
#[test]
fn result_order_is_stable_across_creation_orders() {
    let forward = collision_corpus(true);
    let backward = collision_corpus(false);

    // find
    let find_paths = |root: &Path| {
        run_find(root, &find_args(&["collide"]))
            .files
            .into_iter()
            .map(|f| f.path)
            .collect::<Vec<_>>()
    };
    let forward_find = find_paths(forward.path());
    assert_eq!(
        forward_find,
        find_paths(backward.path()),
        "find order must not depend on creation/readdir order"
    );
    let fe_idx = forward_find.iter().position(|p| p == FE_DISPLAY).unwrap();
    let ff_idx = forward_find.iter().position(|p| p == FF_DISPLAY).unwrap();
    assert!(fe_idx < ff_idx, "equal-score colliders must sort byte-wise");

    // trace
    let trace_paths = |root: &Path| {
        let query = SmartQuery {
            subject: "collide_fn".to_string(),
            relation: Relation::Defined,
            support: vec![],
            kind: None,
            path_hint: None,
        };
        run_smart(root, &query, &smart_args())
            .unwrap()
            .files
            .into_iter()
            .map(|f| f.path)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        trace_paths(forward.path()),
        trace_paths(backward.path()),
        "trace order must not depend on creation/readdir order"
    );

    // grep (whichever engine is available) and grep --paths-only
    let grep_paths = |root: &Path, paths_only: bool| {
        let mut args = grep_args("needle_xyz");
        args.paths_only = paths_only;
        run_grep(root, &args)
            .unwrap()
            .files
            .into_iter()
            .map(|f| f.path)
            .collect::<Vec<_>>()
    };
    for paths_only in [false, true] {
        let fwd = grep_paths(forward.path(), paths_only);
        let bwd = grep_paths(backward.path(), paths_only);
        assert_eq!(
            fwd, bwd,
            "grep(paths_only={paths_only}) order must not depend on creation order"
        );
        let fe_idx = fwd.iter().position(|p| p == FE_DISPLAY).unwrap();
        let ff_idx = fwd.iter().position(|p| p == FF_DISPLAY).unwrap();
        assert!(fe_idx < ff_idx, "colliders must sort byte-wise: {fwd:?}");
    }
}

/// The disambiguated display paths emitted by grep/find/trace must round-trip
/// into outline, which previously dead-ended on non-UTF-8 names.
#[test]
fn outline_round_trips_disambiguated_display_paths() {
    let dir = collision_corpus(true);

    let outline_args = |file: &str| OutlineArgs {
        file: file.to_string(),
        json: false,
        max_items: None,
        path: None,
        context_json: None,
    };

    let ff = run_outline(dir.path(), &outline_args(FF_DISPLAY)).unwrap();
    assert_eq!(ff.path, FF_DISPLAY);
    assert!(
        ff.structure.items.iter().any(|i| i.label == "collide_fn"),
        "outline must parse the real FF file: {:?}",
        ff.structure.items
    );

    let fe = run_outline(dir.path(), &outline_args(FE_DISPLAY)).unwrap();
    assert_eq!(fe.path, FE_DISPLAY);

    // A bare lossy path without the suffix is ambiguous between the two
    // colliders: outline must refuse to guess and error with suggestions
    // that are themselves round-trippable.
    let err = run_outline(dir.path(), &outline_args("collide\u{FFFD}.rs")).unwrap_err();
    assert!(err.contains("file not found"), "unexpected error: {err}");
    assert!(
        err.contains(FF_DISPLAY) && err.contains(FE_DISPLAY),
        "suggestions must be disambiguated: {err}"
    );
}

/// A bare lossy path must still resolve when it is unambiguous (exactly one
/// matching file), so single non-UTF-8 files remain reachable.
#[test]
fn outline_resolves_unambiguous_lossy_path() {
    let dir = tempdir().unwrap();
    fs::write(
        dir.path().join(OsStr::from_bytes(b"only\xff.rs")),
        "fn lonely_fn() {}\n",
    )
    .unwrap();

    let args = OutlineArgs {
        file: "only\u{FFFD}.rs".to_string(),
        json: false,
        max_items: None,
        path: None,
        context_json: None,
    };
    let result = run_outline(dir.path(), &args).unwrap();
    assert_eq!(result.path, "only\u{FFFD}.rs#b=ff");
    assert!(
        result
            .structure
            .items
            .iter()
            .any(|i| i.label == "lonely_fn")
    );
}

/// A literal U+FFFD-named decoy must not hijack a disambiguated request for
/// a different (non-UTF-8) file.
#[test]
fn outline_decoy_with_literal_replacement_char_does_not_shadow_colliders() {
    let dir = collision_corpus(true);
    // Decoy literally named "collide\u{FFFD}.rs" (valid UTF-8).
    fs::write(dir.path().join("collide\u{FFFD}.rs"), "fn decoy_fn() {}\n").unwrap();

    // The disambiguated path still resolves to the real FF file.
    let args = OutlineArgs {
        file: FF_DISPLAY.to_string(),
        json: false,
        max_items: None,
        path: None,
        context_json: None,
    };
    let result = run_outline(dir.path(), &args).unwrap();
    assert!(
        result
            .structure
            .items
            .iter()
            .any(|i| i.label == "collide_fn"),
        "decoy must not shadow the disambiguated request: {:?}",
        result.structure.items
    );

    // The bare lossy path resolves to the decoy (it exists verbatim).
    let bare = OutlineArgs {
        file: "collide\u{FFFD}.rs".to_string(),
        json: false,
        max_items: None,
        path: None,
        context_json: None,
    };
    let decoy = run_outline(dir.path(), &bare).unwrap();
    assert!(decoy.structure.items.iter().any(|i| i.label == "decoy_fn"));
}

/// JSON serialization of grep results must produce unique `path` values and
/// hex `path_bytes` for the colliders (guards the serde field wiring).
#[test]
fn grep_json_serialization_has_unique_paths_and_path_bytes() {
    let dir = collision_corpus(true);
    let result = run_grep(dir.path(), &grep_args("needle_xyz")).unwrap();
    let value = serde_json::to_value(result.to_json()).unwrap();

    let files = value["files"].as_array().unwrap();
    let paths: Vec<&str> = files.iter().map(|f| f["path"].as_str().unwrap()).collect();
    assert_unique(paths.iter().copied(), "grep json");

    let with_bytes: Vec<&str> = files
        .iter()
        .filter(|f| f.get("path_bytes").is_some())
        .map(|f| f["path_bytes"].as_str().unwrap())
        .collect();
    assert_eq!(with_bytes.len(), 2, "both colliders carry path_bytes");
    assert!(with_bytes.contains(&"636f6c6c696465ff2e7273"));
    assert!(with_bytes.contains(&"636f6c6c696465fe2e7273"));
}
