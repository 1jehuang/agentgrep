use crate::cli::GrepArgs;
use crate::structure::{StructureItem, extract_file_structure};
use crate::workspace::{SearchScope, collect_file_entries, read_text_file};
use regex::Regex;
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::thread;

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
    let scope = SearchScope {
        root,
        file_type: args.file_type.as_deref(),
        glob: args.glob.as_deref(),
        hidden: args.hidden,
        no_ignore: args.no_ignore,
    };
    let files = collect_file_entries(&scope);
    let worker_count = thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(1)
        .min(files.len().max(1));

    let mut results = Vec::new();
    let mut total_matches = 0;

    if worker_count <= 1 || files.len() <= 8 {
        for entry in files {
            if let Some(file_matches) = process_file(entry, &matcher) {
                total_matches += file_matches.matches.len();
                results.push(file_matches);
            }
        }
    } else {
        let chunk_size = files.len().div_ceil(worker_count);
        let partials = thread::scope(|scope| {
            let mut handles = Vec::new();
            for chunk in files.chunks(chunk_size) {
                let matcher = matcher.clone();
                handles.push(scope.spawn(move || {
                    let mut partial = Vec::new();
                    let mut partial_total = 0;
                    for entry in chunk.iter().cloned() {
                        if let Some(file_matches) = process_file(entry, &matcher) {
                            partial_total += file_matches.matches.len();
                            partial.push(file_matches);
                        }
                    }
                    (partial, partial_total)
                }));
            }

            handles
                .into_iter()
                .map(|handle| handle.join().expect("grep worker panicked"))
                .collect::<Vec<_>>()
        });

        for (mut partial, partial_total) in partials {
            total_matches += partial_total;
            results.append(&mut partial);
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

fn process_file(entry: crate::workspace::FileEntry, matcher: &Matcher) -> Option<FileMatches> {
    let text = read_text_file(&entry.path)?;

    let mut matches = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        if matcher.is_match(line) {
            matches.push(LineMatch {
                line_number: idx + 1,
                line_text: line.to_string(),
            });
        }
    }

    if matches.is_empty() {
        return None;
    }

    let structure = extract_file_structure(&entry.path, &entry.relative_path, &text);
    let grouping = group_matches(&structure.items, &matches);

    Some(FileMatches {
        path: entry.relative_path,
        language: structure.language,
        role: structure.role,
        matches,
        groups: grouping.groups,
        total_symbols: structure.items.len(),
        matched_symbol_count: grouping.matched_symbol_count,
        other_symbols: grouping.other_symbols,
        other_symbols_omitted_count: grouping.other_symbols_omitted_count,
    })
}

#[derive(Clone)]
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

#[derive(Clone)]
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
    use std::fs;
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
