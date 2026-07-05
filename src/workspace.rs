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
    /// Follow symlinks during the walk. Defaults to true at call sites to
    /// match the rg fast path (--follow); set false via --no-follow as a
    /// guardrail when a symlink points at a huge external tree.
    pub follow: bool,
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: PathBuf,
    pub relative_path: String,
}

#[derive(Debug, Clone)]
pub struct TextFile {
    pub path: PathBuf,
    pub relative_path: String,
    pub text: String,
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
    builder.follow_links(scope.follow);
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
        // Without --follow, rg skips symlinked files entirely (the walker
        // yields the symlink entry, but `is_file()` stats through it). Skip
        // them here too so the native path stays in parity.
        if !scope.follow && entry.path_is_symlink() {
            continue;
        }
        if !path.is_file() {
            continue;
        }
        if let Some(expected_ext) = file_type.as_deref()
            && path.extension().and_then(|s| s.to_str()) != Some(expected_ext)
        {
            continue;
        }
        let relative_path = normalize_display_path(scope.root, path);
        if let Some(glob) = &glob
            && !glob.is_match(&relative_path)
        {
            continue;
        }

        files.push(FileEntry {
            path: path.to_path_buf(),
            relative_path,
        });
    }

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
