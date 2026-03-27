use crate::cli::GrepArgs;
use crate::structure::{StructureItem, extract_file_structure};
use ignore::WalkBuilder;
use regex::Regex;
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

const OTHER_SYMBOLS_LIMIT: usize = 4;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GrepResult {
    pub query: String,
    pub regex: bool,
    pub root: String,
    pub files: Vec<FileMatches>,
    pub total_files: usize,
    pub total_matches: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FileMatches {
    pub path: String,
    pub language: String,
    pub role: String,
    pub matches: Vec<LineMatch>,
    pub groups: Vec<MatchGroup>,
    pub total_symbols: usize,
    pub matched_symbol_count: usize,
    pub other_symbols: Vec<StructureItem>,
    pub other_symbols_omitted_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LineMatch {
    pub line_number: usize,
    pub line_text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MatchGroup {
    pub kind: String,
    pub label: String,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
    pub matches: Vec<LineMatch>,
}

pub fn run_grep(root: &Path, args: &GrepArgs) -> Result<GrepResult, String> {
    let matcher = Matcher::new(&args.query, args.regex)?;
    let files = collect_files(root, args);

    let mut results = Vec::new();
    let mut total_matches = 0;

    for path in files {
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        if bytes.contains(&0) {
            continue;
        }

        let text = String::from_utf8_lossy(&bytes);
        let mut matches = Vec::new();
        for (idx, line) in text.lines().enumerate() {
            if matcher.is_match(line) {
                matches.push(LineMatch {
                    line_number: idx + 1,
                    line_text: line.to_string(),
                });
            }
        }

        if !matches.is_empty() {
            let relative_path = normalize_display_path(root, &path);
            let structure = extract_file_structure(&path, &relative_path, &text);
            let grouping = group_matches(&structure.items, &matches);
            total_matches += matches.len();
            results.push(FileMatches {
                path: relative_path,
                language: structure.language,
                role: structure.role,
                matches,
                groups: grouping.groups,
                total_symbols: structure.items.len(),
                matched_symbol_count: grouping.matched_symbol_count,
                other_symbols: grouping.other_symbols,
                other_symbols_omitted_count: grouping.other_symbols_omitted_count,
            });
        }
    }

    results.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(GrepResult {
        query: args.query.clone(),
        regex: args.regex,
        root: root.display().to_string(),
        total_files: results.len(),
        total_matches,
        files: results,
    })
}

fn collect_files(root: &Path, args: &GrepArgs) -> Vec<PathBuf> {
    let mut builder = WalkBuilder::new(root);
    builder.hidden(!args.hidden);
    if args.no_ignore {
        builder.git_ignore(false);
        builder.git_global(false);
        builder.git_exclude(false);
        builder.ignore(false);
    }

    let file_type = args.file_type.as_deref().map(normalize_file_type);
    let glob = args.glob.as_deref().and_then(build_glob);
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
        let relative_path = normalize_display_path(root, path);
        if let Some(glob) = &glob
            && !glob.is_match(&relative_path)
        {
            continue;
        }
        files.push(path.to_path_buf());
    }

    files
}

fn build_glob(glob: &str) -> Option<globset::GlobSet> {
    let mut builder = globset::GlobSetBuilder::new();
    builder.add(globset::Glob::new(glob).ok()?);
    builder.build().ok()
}

fn normalize_file_type(file_type: &str) -> String {
    match file_type {
        "rust" => "rs".to_string(),
        "javascript" => "js".to_string(),
        "typescript" => "ts".to_string(),
        other => other.trim_start_matches('.').to_string(),
    }
}

fn normalize_display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

struct Matcher {
    kind: MatcherKind,
}

impl Matcher {
    fn new(query: &str, regex: bool) -> Result<Self, String> {
        let kind = if regex {
            MatcherKind::Regex(Regex::new(query).map_err(|err| format!("invalid regex: {err}"))?)
        } else {
            MatcherKind::Literal(query.to_string())
        };
        Ok(Self { kind })
    }

    fn is_match(&self, line: &str) -> bool {
        match &self.kind {
            MatcherKind::Literal(literal) => line.contains(literal),
            MatcherKind::Regex(regex) => regex.is_match(line),
        }
    }
}

enum MatcherKind {
    Literal(String),
    Regex(Regex),
}

struct GroupingResult {
    groups: Vec<MatchGroup>,
    matched_symbol_count: usize,
    other_symbols: Vec<StructureItem>,
    other_symbols_omitted_count: usize,
}

fn group_matches(items: &[StructureItem], matches: &[LineMatch]) -> GroupingResult {
    let mut grouped_matches: BTreeMap<usize, Vec<LineMatch>> = BTreeMap::new();
    let mut matched_indices = HashSet::new();
    let mut file_scope_matches = Vec::new();

    for line_match in matches {
        if let Some((idx, _item)) = items.iter().enumerate().find(|(_, item)| {
            item.start_line <= line_match.line_number && line_match.line_number <= item.end_line
        }) {
            matched_indices.insert(idx);
            grouped_matches
                .entry(idx)
                .or_default()
                .push(line_match.clone());
        } else {
            file_scope_matches.push(line_match.clone());
        }
    }

    let mut groups = Vec::new();
    if !file_scope_matches.is_empty() {
        groups.push(MatchGroup {
            kind: "file-scope".to_string(),
            label: "<file scope>".to_string(),
            start_line: None,
            end_line: None,
            matches: file_scope_matches,
        });
    }

    for (idx, item) in items.iter().enumerate() {
        let Some(matches) = grouped_matches.remove(&idx) else {
            continue;
        };
        groups.push(MatchGroup {
            kind: item.kind.clone(),
            label: item.label.clone(),
            start_line: Some(item.start_line),
            end_line: Some(item.end_line),
            matches,
        });
    }

    let mut other_symbols = Vec::new();
    let mut other_symbols_omitted_count = 0;
    for (idx, item) in items.iter().enumerate() {
        if matched_indices.contains(&idx) {
            continue;
        }
        if other_symbols.len() < OTHER_SYMBOLS_LIMIT {
            other_symbols.push(item.clone());
        } else {
            other_symbols_omitted_count += 1;
        }
    }

    GroupingResult {
        groups,
        matched_symbol_count: matched_indices.len(),
        other_symbols,
        other_symbols_omitted_count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::GrepArgs;
    use tempfile::tempdir;

    fn grep_args(query: &str) -> GrepArgs {
        GrepArgs {
            query: query.to_string(),
            regex: false,
            file_type: None,
            json: false,
            paths_only: false,
            hidden: false,
            no_ignore: false,
            path: None,
            glob: None,
        }
    }

    #[test]
    fn literal_grep_finds_matches_grouped_by_file() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("a.rs"),
            "fn auth_status() {}\nlet x = auth_status();\n",
        )
        .unwrap();
        fs::write(dir.path().join("b.rs"), "no match here\n").unwrap();

        let result = run_grep(dir.path(), &grep_args("auth_status")).unwrap();
        assert_eq!(result.total_files, 1);
        assert_eq!(result.total_matches, 2);
        assert_eq!(result.files[0].path, "a.rs");
        assert_eq!(result.files[0].matches[0].line_number, 1);
        assert_eq!(result.files[0].matches[1].line_number, 2);
        assert_eq!(result.files[0].groups.len(), 1);
        assert_eq!(result.files[0].groups[0].label, "auth_status");
    }

    #[test]
    fn regex_grep_works() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.rs"), "auth_status\nauth_message\n").unwrap();

        let mut args = grep_args("auth_.*");
        args.regex = true;

        let result = run_grep(dir.path(), &args).unwrap();
        assert_eq!(result.total_matches, 2);
    }

    #[test]
    fn file_type_filter_works() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.rs"), "auth_status\n").unwrap();
        fs::write(dir.path().join("a.txt"), "auth_status\n").unwrap();

        let mut args = grep_args("auth_status");
        args.file_type = Some("rs".to_string());

        let result = run_grep(dir.path(), &args).unwrap();
        assert_eq!(result.total_files, 1);
        assert_eq!(result.files[0].path, "a.rs");
    }

    #[test]
    fn glob_filter_works() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/tool")).unwrap();
        fs::create_dir_all(dir.path().join("src/other")).unwrap();
        fs::write(dir.path().join("src/tool/a.rs"), "auth_status\n").unwrap();
        fs::write(dir.path().join("src/other/b.rs"), "auth_status\n").unwrap();

        let mut args = grep_args("auth_status");
        args.glob = Some("src/tool/*".to_string());

        let result = run_grep(dir.path(), &args).unwrap();
        assert_eq!(result.total_files, 1);
        assert_eq!(result.files[0].path, "src/tool/a.rs");
    }

    #[test]
    fn grep_groups_matches_by_enclosing_symbol_and_keeps_other_structure_summary() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("a.rs"),
            concat!(
                "fn render_status_bar() {\n",
                "    let status = auth_status();\n",
                "    ui.label(auth_status().to_string());\n",
                "}\n\n",
                "fn draw_header() {}\n\n",
                "fn auth_status() -> AuthStatus {\n",
                "    auth_status_impl()\n",
                "}\n",
            ),
        )
        .unwrap();

        let result = run_grep(dir.path(), &grep_args("auth_status")).unwrap();
        let file = &result.files[0];
        assert_eq!(file.total_symbols, 3);
        assert_eq!(file.matched_symbol_count, 2);
        assert_eq!(file.groups.len(), 2);
        assert_eq!(file.groups[0].label, "render_status_bar");
        assert_eq!(file.groups[0].matches.len(), 2);
        assert_eq!(file.groups[1].label, "auth_status");
        assert_eq!(file.groups[1].matches.len(), 2);
        assert_eq!(file.other_symbols.len(), 1);
        assert_eq!(file.other_symbols[0].label, "draw_header");
    }

    #[test]
    fn grep_uses_file_scope_group_when_no_symbol_encloses_match() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("a.txt"),
            "AUTH_STATUS=ok\nnot interesting\n",
        )
        .unwrap();

        let result = run_grep(dir.path(), &grep_args("AUTH_STATUS")).unwrap();
        let file = &result.files[0];
        assert_eq!(file.groups.len(), 1);
        assert_eq!(file.groups[0].label, "<file scope>");
        assert_eq!(file.groups[0].matches[0].line_number, 1);
    }
}
