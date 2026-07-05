use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SearchScope<'a> {
    pub root: &'a Path,
    pub file_type: Option<&'a str>,
    pub glob: Option<&'a str>,
    pub hidden: bool,
    pub no_ignore: bool,
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    /// Lossy relative path used for matching, scoring, and role inference.
    pub relative_path: String,
    /// Raw bytes of the relative path when it is not valid UTF-8 (Unix only).
    /// `None` for ordinary UTF-8 paths.
    pub relative_raw: Option<Vec<u8>>,
}

impl FileEntry {
    /// Display path that is unique per file: the lossy relative path, plus a
    /// stable suffix derived from the native bytes when the name is not
    /// valid UTF-8 (see [`disambiguate_display_path`]).
    pub fn display_path(&self) -> String {
        disambiguate_display_path(&self.relative_path, self.relative_raw.as_deref())
    }

    /// Full hex encoding of the raw relative path bytes for JSON consumers,
    /// present only when the path is not valid UTF-8.
    pub fn path_bytes_hex(&self) -> Option<String> {
        self.relative_raw.as_deref().map(path_bytes_hex)
    }
}

#[derive(Debug, Clone)]
pub struct TextFile {
    pub path: PathBuf,
    /// Lossy relative path used for matching, scoring, and role inference.
    pub relative_path: String,
    /// Raw bytes of the relative path when it is not valid UTF-8 (Unix only).
    pub relative_raw: Option<Vec<u8>>,
    pub text: String,
}

impl TextFile {
    /// See [`FileEntry::display_path`].
    pub fn display_path(&self) -> String {
        disambiguate_display_path(&self.relative_path, self.relative_raw.as_deref())
    }

    /// See [`FileEntry::path_bytes_hex`].
    pub fn path_bytes_hex(&self) -> Option<String> {
        self.relative_raw.as_deref().map(path_bytes_hex)
    }
}

pub fn collect_file_entries(scope: &SearchScope<'_>) -> Vec<FileEntry> {
    let mut builder = WalkBuilder::new(scope.root);
    builder.hidden(!scope.hidden);
    // Follow symlinks so directly-symlinked files and symlinked directories are
    // searched, matching the rg fast path (which passes --follow). The ignore
    // crate detects symlink loops when following, and broken symlinks simply
    // fail the `is_file` check below and are skipped. Tradeoff: content outside
    // the root becomes searchable through links, and hardlink-style duplicates
    // can appear under both their real and linked paths.
    builder.follow_links(true);
    if scope.no_ignore {
        builder.git_ignore(false);
        builder.git_global(false);
        builder.git_exclude(false);
        builder.ignore(false);
    } else {
        // ripgrep honors `.rgignore` files by default, but the ignore crate
        // only knows about `.ignore`/`.gitignore`. Register `.rgignore` as a
        // custom ignore file so the native fallback matches the rg fast path.
        builder.add_custom_ignore_filename(".rgignore");
    }

    let file_type = scope.file_type.map(normalize_file_type);
    let glob = scope.glob.and_then(build_glob);
    let mut files = Vec::new();

    for entry in builder.build() {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if let Some(expected_ext) = file_type.as_deref()
            && path.extension().and_then(|s| s.to_str()) != Some(expected_ext)
        {
            continue;
        }
        // Match globs against the raw relative path (not the lossy display
        // string) so non-UTF-8 names filter identically to the rg fast path,
        // which also matches raw bytes via globset. On Unix, `Path` preserves
        // the raw bytes, so e.g. `a?.txt` matches b"a\xff.txt" while a glob
        // containing a literal U+FFFD replacement character does not.
        if let Some(glob) = &glob
            && !glob.is_match(path.strip_prefix(scope.root).unwrap_or(path))
        {
            continue;
        }
        let relative_path = normalize_display_path(scope.root, path);

        let relative_raw = relative_raw_bytes(scope.root, path);
        files.push(FileEntry {
            path: path.to_path_buf(),
            relative_path,
            relative_raw,
        });
    }

    // Sort by native path bytes so result ordering (including tie-breaks
    // between names whose lossy display strings collide) is deterministic and
    // portable instead of leaking filesystem readdir order.
    files.sort_by(|a, b| a.path.as_os_str().cmp(b.path.as_os_str()));

    files
}

pub fn read_text_file(path: &Path) -> Option<String> {
    let Ok(bytes) = fs::read(path) else {
        return None;
    };
    if bytes.contains(&0) {
        return None;
    }

    match String::from_utf8(bytes) {
        Ok(text) => Some(text),
        Err(err) => Some(String::from_utf8_lossy(err.as_bytes()).into_owned()),
    }
}

pub fn collect_text_files(scope: &SearchScope<'_>) -> Vec<TextFile> {
    let mut files = Vec::new();

    for entry in collect_file_entries(scope) {
        let Some(text) = read_text_file(&entry.path) else {
            continue;
        };

        files.push(TextFile {
            path: entry.path,
            relative_path: entry.relative_path,
            relative_raw: entry.relative_raw,
            text,
        });
    }

    files
}

fn build_glob(glob: &str) -> Option<GlobSet> {
    let mut builder = GlobSetBuilder::new();
    builder.add(Glob::new(glob).ok()?);
    builder.build().ok()
}

pub fn normalize_file_type(file_type: &str) -> String {
    match file_type {
        "rust" => "rs".to_string(),
        "javascript" => "js".to_string(),
        "typescript" => "ts".to_string(),
        other => other.trim_start_matches('.').to_string(),
    }
}

pub fn normalize_display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Raw bytes of the root-relative path when it is not valid UTF-8.
///
/// Returns `None` for ordinary UTF-8 paths (the common case) and on
/// non-Unix targets, where raw path bytes are not exposed.
pub fn relative_raw_bytes(root: &Path, path: &Path) -> Option<Vec<u8>> {
    let relative = path.strip_prefix(root).unwrap_or(path);
    if relative.to_str().is_some() {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        Some(relative.as_os_str().as_bytes().to_vec())
    }
    #[cfg(not(unix))]
    {
        None
    }
}

/// Hex encoding of raw path bytes, e.g. `61ff2e747874` for `b"a\xff.txt"`.
pub fn path_bytes_hex(raw: &[u8]) -> String {
    let mut out = String::with_capacity(raw.len() * 2);
    for byte in raw {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// Make a lossy display path unique when the underlying name is not valid
/// UTF-8, by appending a short stable suffix derived from the native bytes:
/// `a\u{FFFD}.txt#b=ff` for `b"a\xff.txt"`. Two files whose lossy display
/// strings collide (b"a\xff.txt" vs b"a\xfe.txt") therefore render
/// differently, so consumers that dedup on the displayed path cannot silently
/// drop one of them. UTF-8 paths are returned unchanged, and the suffix is a
/// display-level annotation only: internal opens always use the native
/// `PathBuf`/raw bytes.
pub fn disambiguate_display_path(lossy: &str, raw: Option<&[u8]>) -> String {
    let Some(raw) = raw else {
        return lossy.to_string();
    };
    let mut suffix = String::new();
    let mut cursor = raw;
    // Hex-encode exactly the invalid byte runs, in order, so the suffix is
    // short, stable, and derived only from the bytes that the lossy string
    // cannot represent.
    while !cursor.is_empty() {
        match std::str::from_utf8(cursor) {
            Ok(_) => break,
            Err(err) => {
                let valid_up_to = err.valid_up_to();
                let invalid_len = err.error_len().unwrap_or(cursor.len() - valid_up_to);
                if !suffix.is_empty() {
                    suffix.push('.');
                }
                suffix.push_str(&path_bytes_hex(
                    &cursor[valid_up_to..valid_up_to + invalid_len],
                ));
                cursor = &cursor[valid_up_to + invalid_len..];
            }
        }
    }
    if suffix.is_empty() {
        return lossy.to_string();
    }
    format!("{lossy}#b={suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disambiguate_keeps_utf8_paths_unchanged() {
        assert_eq!(disambiguate_display_path("src/app.rs", None), "src/app.rs");
        // Raw bytes that are valid UTF-8 also produce no suffix.
        assert_eq!(
            disambiguate_display_path("src/app.rs", Some(b"src/app.rs")),
            "src/app.rs"
        );
    }

    #[test]
    fn disambiguate_appends_stable_hex_suffix_for_invalid_bytes() {
        assert_eq!(
            disambiguate_display_path("a\u{FFFD}.txt", Some(b"a\xff.txt")),
            "a\u{FFFD}.txt#b=ff"
        );
        assert_eq!(
            disambiguate_display_path("a\u{FFFD}.txt", Some(b"a\xfe.txt")),
            "a\u{FFFD}.txt#b=fe"
        );
        // Multiple invalid runs are joined with '.'.
        assert_eq!(
            disambiguate_display_path(
                "a\u{FFFD}b\u{FFFD}\u{FFFD}.txt",
                Some(b"a\xffb\xfe\xfd.txt")
            ),
            "a\u{FFFD}b\u{FFFD}\u{FFFD}.txt#b=ff.fe.fd"
        );
    }

    #[test]
    fn path_bytes_hex_encodes_all_bytes() {
        assert_eq!(path_bytes_hex(b"a\xff.txt"), "61ff2e747874");
    }

    #[cfg(unix)]
    #[test]
    fn relative_raw_bytes_only_set_for_non_utf8_names() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let root = Path::new("/root");
        assert_eq!(relative_raw_bytes(root, Path::new("/root/a.txt")), None);
        let bad = Path::new("/root").join(OsStr::from_bytes(b"a\xff.txt"));
        assert_eq!(
            relative_raw_bytes(root, &bad).as_deref(),
            Some(b"a\xff.txt".as_slice())
        );
    }

    #[cfg(unix)]
    #[test]
    fn collect_file_entries_sorted_by_native_bytes_regardless_of_creation_order() {
        use std::ffi::OsStr;
        use std::fs;
        use std::os::unix::ffi::OsStrExt;
        use tempfile::tempdir;

        let names: [&[u8]; 4] = [b"a\xff.rs", b"a\xfe.rs", b"zz.rs", b"aa.rs"];

        let make_corpus = |order: &[&[u8]]| {
            let dir = tempdir().unwrap();
            for name in order {
                fs::write(dir.path().join(OsStr::from_bytes(name)), "fn x() {}\n").unwrap();
            }
            dir
        };

        let forward = make_corpus(&names);
        let reversed_names: Vec<&[u8]> = names.iter().rev().copied().collect();
        let backward = make_corpus(&reversed_names);

        let display = |root: &Path| {
            let scope = SearchScope {
                root,
                file_type: None,
                glob: None,
                hidden: false,
                no_ignore: true,
            };
            collect_file_entries(&scope)
                .iter()
                .map(FileEntry::display_path)
                .collect::<Vec<_>>()
        };

        // Native byte order: 'a'(0x61) < 0xfe < 0xff, so "aa.rs" sorts before
        // both collider names.
        let expected = vec![
            "aa.rs".to_string(),
            "a\u{FFFD}.rs#b=fe".to_string(),
            "a\u{FFFD}.rs#b=ff".to_string(),
            "zz.rs".to_string(),
        ];
        assert_eq!(display(forward.path()), expected);
        assert_eq!(
            display(backward.path()),
            expected,
            "entry order must not depend on filesystem creation/readdir order"
        );
    }

    #[cfg(unix)]
    #[test]
    fn glob_and_type_filters_match_raw_bytes_not_lossy_display() {
        use std::ffi::OsStr;
        use std::fs;
        use std::os::unix::ffi::OsStrExt;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        for name in [
            b"a\xff.txt".as_slice(),
            b"a\xfe.txt",
            b"c\xff.rs",
            b"plain.txt",
            b"plain.rs",
        ] {
            fs::write(dir.path().join(OsStr::from_bytes(name)), "x\n").unwrap();
        }

        let collect = |glob: Option<&str>, file_type: Option<&str>| {
            let scope = SearchScope {
                root: dir.path(),
                file_type,
                glob,
                hidden: false,
                no_ignore: true,
            };
            collect_file_entries(&scope)
                .iter()
                .map(FileEntry::display_path)
                .collect::<Vec<_>>()
        };

        // Extension globs and --type must include non-UTF-8 names, matching
        // the raw bytes exactly as the rg fast path does.
        assert_eq!(
            collect(Some("*.txt"), None),
            vec![
                "a\u{FFFD}.txt#b=fe".to_string(),
                "a\u{FFFD}.txt#b=ff".to_string(),
                "plain.txt".to_string(),
            ]
        );
        assert_eq!(
            collect(None, Some("rs")),
            vec!["c\u{FFFD}.rs#b=ff".to_string(), "plain.rs".to_string()]
        );
        // `?` matches a single byte-run on raw paths (rg semantics), so it
        // matches the single invalid byte; a literal U+FFFD glob must NOT
        // match because the raw name contains 0xff, not the replacement char.
        assert_eq!(
            collect(Some("a?.txt"), None),
            vec![
                "a\u{FFFD}.txt#b=fe".to_string(),
                "a\u{FFFD}.txt#b=ff".to_string(),
            ]
        );
        assert_eq!(collect(Some("a\u{FFFD}.txt"), None), Vec::<String>::new());
        // The '#b=' display suffix is display-only and must never leak into
        // glob matching.
        assert_eq!(collect(Some("*ff"), None), Vec::<String>::new());
        assert_eq!(collect(Some("*#b=*"), None), Vec::<String>::new());
    }
}
