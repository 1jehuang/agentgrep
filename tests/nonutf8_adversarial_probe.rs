//! Adversarial injectivity and resolution tests for the `#b=` display-path
//! disambiguation scheme (see `workspace::disambiguate_display_path`).
//!
//! Attack classes covered:
//! 1. Run-boundary / literal-U+FFFD ambiguity: distinct raw names whose
//!    lossy displays are identical because a literal U+FFFD character in one
//!    name lines up with a lossy-replaced invalid run in another.
//! 2. `#b=` decoys: real UTF-8 files literally NAMED like a disambiguated
//!    display (e.g. `a\u{FFFD}.txt#b=ff`) coexisting with the non-UTF-8 file
//!    whose disambiguated display that is.
//! 3. A deterministic fuzz/property test: random byte names created in a
//!    tempdir must produce pairwise-distinct displays that each round-trip
//!    through outline back to exactly their own file.
#![cfg(unix)]

use agentgrep::cli::{FindArgs, GrepArgs, OutlineArgs};
use agentgrep::find::run_find;
use agentgrep::outline::run_outline;
use agentgrep::search::run_grep;
use agentgrep::workspace::disambiguate_display_path;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use tempfile::tempdir;

fn display_for(raw: &[u8]) -> String {
    let lossy = String::from_utf8_lossy(raw).into_owned();
    let non_utf8 = std::str::from_utf8(raw).is_err();
    disambiguate_display_path(&lossy, non_utf8.then_some(raw))
}

fn outline_args(file: &str) -> OutlineArgs {
    OutlineArgs {
        file: file.to_string(),
        json: false,
        max_items: None,
        path: None,
        context_json: None,
    }
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
        max_files: 50,
        hidden: false,
        no_ignore: true,
        path: None,
        glob: None,
    }
}

/// Class 1: literal U+FFFD vs lossy-replaced byte in the same slot position.
/// Both names lossy-decode to `a\u{FFFD}\u{FFFD}.txt`; the per-slot token
/// scheme must keep their displays distinct.
#[test]
fn literal_fffd_and_invalid_byte_interleavings_stay_distinct() {
    // a, 0xff, literal U+FFFD (ef bf bd), .txt
    let raw1: &[u8] = b"a\xff\xef\xbf\xbd.txt";
    // a, literal U+FFFD, 0xff, .txt
    let raw2: &[u8] = b"a\xef\xbf\xbd\xff.txt";
    let lossy1 = String::from_utf8_lossy(raw1).into_owned();
    let lossy2 = String::from_utf8_lossy(raw2).into_owned();
    assert_eq!(lossy1, lossy2, "precondition: lossy displays collide");

    let d1 = display_for(raw1);
    let d2 = display_for(raw2);
    assert_eq!(d1, "a\u{FFFD}\u{FFFD}.txt#b=ff.-");
    assert_eq!(d2, "a\u{FFFD}\u{FFFD}.txt#b=-.ff");
    assert_ne!(d1, d2);
}

/// Class 1b: a pure literal-U+FFFD UTF-8 name vs the non-UTF-8 name that
/// lossy-decodes to the same string.
#[test]
fn literal_fffd_name_and_lossy_collider_stay_distinct() {
    let literal: &[u8] = "a\u{FFFD}.txt".as_bytes();
    let collider: &[u8] = b"a\xff.txt";
    assert_eq!(
        String::from_utf8_lossy(literal),
        String::from_utf8_lossy(collider)
    );
    assert_eq!(display_for(literal), "a\u{FFFD}.txt#b=-");
    assert_eq!(display_for(collider), "a\u{FFFD}.txt#b=ff");
}

/// Class 2: a real UTF-8 file literally named like a disambiguated display
/// must not share a display with the non-UTF-8 file it mimics.
#[test]
fn hash_b_decoy_display_does_not_collide() {
    let collider: &[u8] = b"a\xff.txt";
    let decoy: &[u8] = "a\u{FFFD}.txt#b=ff".as_bytes();
    let collider_display = display_for(collider);
    let decoy_display = display_for(decoy);
    assert_eq!(collider_display, "a\u{FFFD}.txt#b=ff");
    // The decoy's own literal U+FFFD earns it a `-` token, so its display is
    // suffixed again and cannot impersonate the collider.
    assert_eq!(decoy_display, "a\u{FFFD}.txt#b=ff#b=-");
    assert_ne!(collider_display, decoy_display);
}

/// Class 2 end-to-end: collider + `#b=` decoy + literal-U+FFFD decoy all in
/// one directory. grep and find must show three distinct paths, and outline
/// must resolve each display to exactly its own file.
#[test]
fn combined_decoys_are_separately_addressable_in_all_modes() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let collider: &[u8] = b"a\xff.rs";
    let literal_decoy = "a\u{FFFD}.rs";
    let hashb_decoy = "a\u{FFFD}.rs#b=ff";
    // Distinct line counts double as a language-independent identity check
    // (the hashb decoy's extension is `rs#b=ff`, so outline detects no
    // structure items for it).
    fs::write(
        root.join(OsStr::from_bytes(collider)),
        "fn probe_collider() { let _ = \"needle_adv\"; }\n",
    )
    .unwrap();
    fs::write(
        root.join(literal_decoy),
        "fn probe_literal_decoy() { let _ = \"needle_adv\"; }\n//\n",
    )
    .unwrap();
    fs::write(
        root.join(hashb_decoy),
        "fn probe_hashb_decoy() { let _ = \"needle_adv\"; }\n//\n//\n",
    )
    .unwrap();

    // display -> (grep marker, outline line count identity)
    let expected: HashMap<&str, (&str, usize)> = HashMap::from([
        ("a\u{FFFD}.rs#b=ff", ("probe_collider", 1)),
        ("a\u{FFFD}.rs#b=-", ("probe_literal_decoy", 2)),
        ("a\u{FFFD}.rs#b=ff#b=-", ("probe_hashb_decoy", 3)),
    ]);

    // grep: three distinct display paths, none merged or dropped, each with
    // its own file's content (proves per-display addressability of content).
    let grep = run_grep(root, &grep_args("needle_adv")).unwrap();
    let grep_paths: HashSet<String> = grep.files.iter().map(|f| f.path.clone()).collect();
    assert_eq!(grep.files.len(), 3, "no dedup-drop: {grep_paths:?}");
    for (path, (marker, _)) in &expected {
        let file = grep
            .files
            .iter()
            .find(|f| f.path == *path)
            .unwrap_or_else(|| panic!("grep missing {path:?}: {grep_paths:?}"));
        assert!(
            file.matches[0].line_text.contains(marker),
            "{path:?} shows the wrong file's content: {:?}",
            file.matches[0].line_text
        );
    }

    // find: same three distinct paths.
    let find = run_find(root, &find_args(&["a"]));
    let find_paths: HashSet<String> = find.files.iter().map(|f| f.path.clone()).collect();
    for path in expected.keys() {
        assert!(
            find_paths.contains(*path),
            "find missing {path:?}: {find_paths:?}"
        );
    }
    assert_eq!(
        find_paths.len(),
        find.files.len(),
        "find paths must be unique"
    );

    // outline: every display resolves to exactly its own file (identified
    // by line count), and no request is hijacked by a decoy.
    for (display, (_, lines)) in &expected {
        let result = run_outline(root, &outline_args(display)).unwrap();
        assert_eq!(&result.path, display, "outline must echo the display path");
        assert_eq!(
            result.total_lines, *lines,
            "{display:?} resolved to the wrong file"
        );
    }

    // The verbatim literal-U+FFFD name still resolves to the literal decoy
    // (no other file emits that display), and its emitted path is the
    // disambiguated form.
    let verbatim = run_outline(root, &outline_args(literal_decoy)).unwrap();
    assert!(
        verbatim
            .structure
            .items
            .iter()
            .any(|i| i.label == "probe_literal_decoy")
    );
    assert_eq!(verbatim.path, "a\u{FFFD}.rs#b=-");
    // The request "a\u{FFFD}.rs#b=ff" is BOTH the collider's emitted display
    // and the hashb decoy's verbatim name. Emitted displays are the
    // addressing contract, so the collider wins; the decoy remains reachable
    // only through its own emitted display (asserted above).
    let contested = run_outline(root, &outline_args(hashb_decoy)).unwrap();
    assert!(
        contested
            .structure
            .items
            .iter()
            .any(|i| i.label == "probe_collider"),
        "emitted display must outrank a decoy's verbatim name: {:?}",
        contested.structure.items
    );
}

/// Deterministic PRNG so the fuzz test is reproducible without new deps.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        // Numerical Recipes LCG constants.
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[(self.next() as usize) % items.len()]
    }
}

/// Class 3 property test: random byte names (drawn from an alphabet dense in
/// invalid bytes, truncated multibyte prefixes, literal U+FFFD bytes, and
/// `#b=`/`.`/`-` scheme characters) created in a tempdir must yield
/// pairwise-distinct displays, and each display must resolve via outline
/// back to exactly its own file.
#[test]
fn fuzz_random_byte_names_have_injective_resolvable_displays() {
    // Fragments rather than single bytes so truncated multibyte sequences
    // and full U+FFFD encodings appear frequently.
    let fragments: &[&[u8]] = &[
        b"a",
        b"b",
        b"#",
        b"=",
        b".",
        b"-",
        b"\xff",
        b"\xfe",
        b"\xef\xbf\xbd", // literal U+FFFD
        b"\xe0\xa0",     // truncated 3-byte sequence
        b"\xf0\x9f",     // truncated 4-byte sequence
        b"\xc3",         // truncated 2-byte sequence
        b"\xbf",         // stray continuation byte
        b"#b=",
        b"#b=ff",
    ];
    let mut rng = Lcg(0x5eed_cafe_f00d_0001);

    let dir = tempdir().unwrap();
    let root = dir.path();
    let mut raw_names: HashSet<Vec<u8>> = HashSet::new();
    for _ in 0..300 {
        let mut name = Vec::new();
        let parts = 1 + (rng.next() as usize) % 5;
        for _ in 0..parts {
            name.extend_from_slice(rng.pick(fragments));
        }
        name.extend_from_slice(b".rs");
        // Filesystem constraints: no '/', no NUL (alphabet already avoids
        // both), no leading '.' so default walkers see the file.
        if name.starts_with(b".") {
            name.insert(0, b'a');
        }
        raw_names.insert(name);
    }
    assert!(raw_names.len() > 100, "fuzz corpus too small");

    // Create each file with a unique marker function.
    let mut marker_by_display: HashMap<String, String> = HashMap::new();
    for (index, raw) in raw_names.iter().enumerate() {
        let marker = format!("fuzz_marker_{index}");
        fs::write(
            root.join(OsStr::from_bytes(raw)),
            format!("fn {marker}() {{}}\n"),
        )
        .unwrap();
        let display = display_for(raw);
        // Injectivity: no two raw names may share a display.
        if let Some(previous) = marker_by_display.insert(display.clone(), marker) {
            panic!(
                "display collision for {display:?}: {previous} vs raw {:?}",
                raw
            );
        }
    }

    // Cross-check against what the walker actually emits.
    let find = run_find(root, &find_args(&["rs"]));
    // find caps results; use grep paths_only-style uniqueness on the full
    // corpus via outline resolution below instead. Still assert find's own
    // returned paths are unique.
    let find_paths: Vec<&str> = find.files.iter().map(|f| f.path.as_str()).collect();
    let find_unique: HashSet<&str> = find_paths.iter().copied().collect();
    assert_eq!(
        find_unique.len(),
        find_paths.len(),
        "find emitted dup displays"
    );

    // Resolution: every display round-trips through outline to its own file.
    for (display, marker) in &marker_by_display {
        let result = run_outline(root, &outline_args(display))
            .unwrap_or_else(|err| panic!("outline failed to resolve {display:?}: {err}"));
        assert!(
            result.structure.items.iter().any(|i| i.label == *marker),
            "{display:?} resolved to wrong file (wanted {marker}): {:?}",
            result.structure.items
        );
        assert_eq!(&result.path, display, "outline must echo the display");
    }
}

/// The walker-facing display (FileEntry::display_path via find/grep) must
/// agree with the direct function for literal-U+FFFD UTF-8 names, so the
/// display seen in results is the one outline accepts.
#[test]
fn walker_display_matches_direct_function_for_literal_fffd_names() {
    let dir = tempdir().unwrap();
    let root = dir.path();
    let name = "solo\u{FFFD}.rs";
    fs::write(root.join(name), "fn solo_literal() {}\n").unwrap();

    let find = run_find(root, &find_args(&["solo"]));
    assert_eq!(find.files.len(), 1);
    assert_eq!(find.files[0].path, "solo\u{FFFD}.rs#b=-");
    assert_eq!(find.files[0].path, display_for(name.as_bytes()));

    let outline = run_outline(root, &outline_args("solo\u{FFFD}.rs#b=-")).unwrap();
    assert!(
        outline
            .structure
            .items
            .iter()
            .any(|i| i.label == "solo_literal")
    );
}

