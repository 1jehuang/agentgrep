use crate::cli::GrepArgs;
use crate::structure::{StructureItem, extract_file_structure};
use crate::workspace::{SearchScope, collect_file_entries, read_text_file};
use regex::Regex;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

const OTHER_SYMBOLS_LIMIT: usize = 4;
const DENSE_MATCH_LIMITED_GROUPING_THRESHOLD: usize = 12;
const DENSE_MATCH_SKIP_STRUCTURE_THRESHOLD: usize = 24;
const DENSE_GROUPS_LIMIT: usize = 8;
const DENSE_OTHER_SYMBOLS_LIMIT: usize = 2;
const MAX_MATCH_LINE_CHARS: usize = 240;
const MATCH_LINE_PREFIX_CONTEXT_CHARS: usize = 80;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrepResult {
    pub query: String,
    pub regex: bool,
    pub root: String,
    pub files: Vec<FileMatches>,
    pub total_files: usize,
    pub total_matches: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchGroup {
    pub kind: String,
    pub label: String,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
    match_indices: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GrepResultJson {
    pub query: String,
    pub regex: bool,
    pub root: String,
    pub files: Vec<FileMatchesJson>,
    pub total_files: usize,
    pub total_matches: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FileMatchesJson {
    pub path: String,
    pub language: String,
    pub role: String,
    pub matches: Vec<LineMatch>,
    pub groups: Vec<MatchGroupJson>,
    pub total_symbols: usize,
    pub matched_symbol_count: usize,
    pub other_symbols: Vec<StructureItem>,
    pub other_symbols_omitted_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MatchGroupJson {
    pub kind: String,
    pub label: String,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
    pub matches: Vec<LineMatch>,
}

impl GrepResult {
    pub fn to_json(&self) -> GrepResultJson {
        GrepResultJson {
            query: self.query.clone(),
            regex: self.regex,
            root: self.root.clone(),
            files: self.files.iter().map(FileMatches::to_json).collect(),
            total_files: self.total_files,
            total_matches: self.total_matches,
        }
    }
}

impl FileMatches {
    fn to_json(&self) -> FileMatchesJson {
        FileMatchesJson {
            path: self.path.clone(),
            language: self.language.clone(),
            role: self.role.clone(),
            matches: self.matches.clone(),
            groups: self
                .groups
                .iter()
                .map(|group| group.to_json(&self.matches))
                .collect(),
            total_symbols: self.total_symbols,
            matched_symbol_count: self.matched_symbol_count,
            other_symbols: self.other_symbols.clone(),
            other_symbols_omitted_count: self.other_symbols_omitted_count,
        }
    }
}

impl MatchGroup {
    pub fn resolved_matches<'a>(
        &'a self,
        matches: &'a [LineMatch],
    ) -> impl Iterator<Item = &'a LineMatch> + 'a {
        self.match_indices.iter().map(move |&idx| &matches[idx])
    }

    fn to_json(&self, matches: &[LineMatch]) -> MatchGroupJson {
        MatchGroupJson {
            kind: self.kind.clone(),
            label: self.label.clone(),
            start_line: self.start_line,
            end_line: self.end_line,
            matches: self.resolved_matches(matches).cloned().collect(),
        }
    }

    #[cfg(test)]
    fn match_count(&self) -> usize {
        self.match_indices.len()
    }
}

pub fn run_grep(root: &Path, args: &GrepArgs) -> Result<GrepResult, String> {
    if let Some(result) = run_grep_with_rg(root, args)? {
        return Ok(result);
    }

    run_grep_native(root, args)
}

fn run_grep_native(root: &Path, args: &GrepArgs) -> Result<GrepResult, String> {
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

fn run_grep_with_rg(root: &Path, args: &GrepArgs) -> Result<Option<GrepResult>, String> {
    if args.paths_only {
        let Some(paths) = run_rg_paths_only(root, args)? else {
            return Ok(None);
        };
        let files = paths
            .into_iter()
            .map(|path| FileMatches {
                path,
                language: String::new(),
                role: String::new(),
                matches: Vec::new(),
                groups: Vec::new(),
                total_symbols: 0,
                matched_symbol_count: 0,
                other_symbols: Vec::new(),
                other_symbols_omitted_count: 0,
            })
            .collect::<Vec<_>>();

        return Ok(Some(GrepResult {
            query: args.query.clone(),
            regex: args.regex,
            root: root.display().to_string(),
            total_files: files.len(),
            total_matches: 0,
            files,
        }));
    }

    let Some(match_map) = run_rg_match_map(root, args)? else {
        return Ok(None);
    };
    let matched_files = match_map.into_iter().collect::<Vec<_>>();
    let worker_count = thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(1)
        .min(matched_files.len().max(1));

    let mut results = Vec::with_capacity(matched_files.len());
    let mut total_matches = 0;
    if worker_count <= 1 || matched_files.len() <= 8 {
        for (path, matches) in matched_files {
            if let Some(file_matches) = process_rg_match_file(root, path, matches) {
                total_matches += file_matches.matches.len();
                results.push(file_matches);
            }
        }
    } else {
        let chunk_size = matched_files.len().div_ceil(worker_count);
        let partials = thread::scope(|scope| {
            let mut handles = Vec::new();
            for chunk in matched_files.chunks(chunk_size) {
                handles.push(scope.spawn(move || {
                    let mut partial = Vec::new();
                    let mut partial_total = 0;
                    for (path, matches) in chunk.iter().cloned() {
                        if let Some(file_matches) = process_rg_match_file(root, path, matches) {
                            partial_total += file_matches.matches.len();
                            partial.push(file_matches);
                        }
                    }
                    (partial, partial_total)
                }));
            }

            handles
                .into_iter()
                .map(|handle| handle.join().expect("rg grep worker panicked"))
                .collect::<Vec<_>>()
        });

        for (mut partial, partial_total) in partials {
            total_matches += partial_total;
            results.append(&mut partial);
        }
    }

    results.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(Some(GrepResult {
        query: args.query.clone(),
        regex: args.regex,
        root: root.display().to_string(),
        total_files: results.len(),
        total_matches,
        files: results,
    }))
}

fn run_rg_paths_only(root: &Path, args: &GrepArgs) -> Result<Option<Vec<String>>, String> {
    let mut command = build_rg_command(root, args);
    command.arg("--files-with-matches");
    command.arg("--null");
    command.arg(".");

    let output = match command.output() {
        Ok(output) => output,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("failed to execute rg: {err}")),
    };

    match output.status.code() {
        // Exit 2 with results on stdout means rg hit per-file errors (for
        // example broken symlinks with --follow) but still searched the rest;
        // treat the partial output as authoritative. Exit 2 with no output is
        // ambiguous (could be a fatal error), so fall back to the native path.
        Some(0) | Some(1) => {}
        Some(2) if !output.stdout.is_empty() => {}
        Some(2) => return Ok(None),
        Some(code) => {
            return Err(format!(
                "rg failed with exit code {code}: {}",
                String::from_utf8_lossy(&output.stderr)
            ));
        }
        None => return Err("rg terminated by signal".to_string()),
    }

    let mut paths = output
        .stdout
        .split(|byte| *byte == b'\0')
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8_lossy(path).into_owned())
        .map(|path| normalize_rg_path(&path))
        .collect::<Vec<_>>();
    paths.sort();
    Ok(Some(paths))
}

fn run_rg_match_map(
    root: &Path,
    args: &GrepArgs,
) -> Result<Option<BTreeMap<RgPathKey, Vec<LineMatch>>>, String> {
    let matcher = Matcher::new(&args.query, args.regex)?;
    #[cfg(windows)]
    {
        let Some(output) = run_rg_json_search(root, args)? else {
            return Ok(None);
        };
        return parse_rg_json(&output.stdout, &matcher).map(Some);
    }

    #[cfg(not(windows))]
    {
        if let Some(output) = run_rg_plain_search(root, args)? {
            match parse_rg_plain(&output.stdout, &matcher) {
                Ok(match_map) => return Ok(Some(match_map)),
                Err(_) => {
                    let Some(json_output) = run_rg_json_search(root, args)? else {
                        return Ok(None);
                    };
                    return parse_rg_json(&json_output.stdout, &matcher).map(Some);
                }
            }
        }
        Ok(None)
    }
}

fn run_rg_plain_search(
    root: &Path,
    args: &GrepArgs,
) -> Result<Option<std::process::Output>, String> {
    let mut command = build_rg_command(root, args);
    command.arg("--line-number");
    command.arg("--column");
    command.arg("--with-filename");
    command.arg("--color").arg("never");
    command.arg("--no-heading");
    command.arg(".");
    run_rg_output(command)
}

fn run_rg_json_search(
    root: &Path,
    args: &GrepArgs,
) -> Result<Option<std::process::Output>, String> {
    let mut command = build_rg_command(root, args);
    command.arg("--json");
    command.arg("--line-number");
    command.arg("--with-filename");
    command.arg("--color").arg("never");
    command.arg("--no-heading");
    command.arg(".");
    run_rg_output(command)
}

fn run_rg_output(mut command: Command) -> Result<Option<std::process::Output>, String> {
    let output = match command.output() {
        Ok(output) => output,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("failed to execute rg: {err}")),
    };

    match output.status.code() {
        Some(0) | Some(1) => Ok(Some(output)),
        // Exit 2 with results on stdout means rg hit per-file errors (for
        // example broken symlinks with --follow) but still searched the rest.
        // Exit 2 with no output is ambiguous (could be a fatal error such as a
        // bad pattern), so return None to fall back to the native path.
        Some(2) if !output.stdout.is_empty() => Ok(Some(output)),
        Some(2) => Ok(None),
        Some(code) => Err(format!(
            "rg failed with exit code {code}: {}",
            String::from_utf8_lossy(&output.stderr)
        )),
        None => Err("rg terminated by signal".to_string()),
    }
}

fn build_rg_command(root: &Path, args: &GrepArgs) -> Command {
    let mut command = Command::new("rg");
    command.current_dir(root);
    // Isolate agentgrep from the user's ripgrep configuration so the rg fast
    // path and the native fallback return the same results. `--no-config`
    // (supported since rg 0.8) makes rg skip config files entirely; removing
    // RIPGREP_CONFIG_PATH is belt-and-suspenders for the same goal.
    command.arg("--no-config");
    command.env_remove("RIPGREP_CONFIG_PATH");

    if args.regex {
        command.arg("-e").arg(&args.query);
    } else {
        command.arg("--fixed-strings");
        // Pass the pattern via -e so queries starting with '-' (for example
        // "--features") are not parsed as rg flags.
        command.arg("-e").arg(&args.query);
    }

    // Follow symlinks so symlinked files/directories are searched, matching the
    // native walker (workspace::collect_file_entries sets follow_links(true)).
    // --no-messages keeps broken-symlink noise off stderr; per-file errors are
    // tolerated via the exit-code handling in run_rg_paths_only/run_rg_output.
    command.arg("--follow");
    command.arg("--no-messages");

    if args.hidden {
        command.arg("--hidden");
    }
    if args.no_ignore {
        command.arg("--no-ignore");
    }
    if let Some(glob) = &args.glob {
        command.arg("-g").arg(glob);
    }
    if let Some(file_type) = args.file_type.as_deref() {
        let ext = crate::workspace::normalize_file_type(file_type);
        command.arg("-g").arg(format!("*.{ext}"));
    }

    command
}

fn parse_rg_plain(
    stdout: &[u8],
    matcher: &Matcher,
) -> Result<BTreeMap<RgPathKey, Vec<LineMatch>>, String> {
    let text = std::str::from_utf8(stdout)
        .map_err(|err| format!("failed to decode rg plain output as UTF-8: {err}"))?;
    let mut matches_by_path: BTreeMap<RgPathKey, Vec<LineMatch>> = BTreeMap::new();

    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let mut parts = line.splitn(4, ':');
        let Some(path) = parts.next() else {
            continue;
        };
        let Some(line_number) = parts.next() else {
            return Err("rg plain output did not include line numbers".to_string());
        };
        let Some(_column) = parts.next() else {
            return Err("rg plain output did not include columns".to_string());
        };
        let Some(line_text) = parts.next() else {
            return Err("rg plain output did not include line text".to_string());
        };
        let line_number = line_number
            .parse::<usize>()
            .map_err(|err| format!("failed to parse rg line number: {err}"))?;
        matches_by_path
            .entry(RgPathKey::from_bytes(path.as_bytes()))
            .or_default()
            .push(LineMatch {
                line_number,
                line_text: matcher.display_line_text(line_text),
            });
    }

    Ok(matches_by_path)
}

fn parse_rg_json(
    stdout: &[u8],
    matcher: &Matcher,
) -> Result<BTreeMap<RgPathKey, Vec<LineMatch>>, String> {
    let mut matches_by_path: BTreeMap<RgPathKey, Vec<LineMatch>> = BTreeMap::new();

    for line in stdout.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            continue;
        }
        let event: RgEvent = serde_json::from_slice(line)
            .map_err(|err| format!("failed to parse rg json output: {err}"))?;
        let Some(data) = event.data else {
            continue;
        };
        if event.kind != "match" {
            continue;
        }
        // rg --json emits {"text": ...} for UTF-8 fields and {"bytes": base64}
        // for fields containing invalid UTF-8. Accept both so a single binary
        // match line (or a non-UTF-8 filename) cannot fail the whole grep.
        let Some(path_bytes) = data.path.and_then(RgTextField::into_bytes) else {
            return Err("rg json output did not include a match path".to_string());
        };
        let Some(line_number) = data.line_number else {
            return Err("rg json output did not include line numbers".to_string());
        };
        let Some(line_bytes) = data.lines.and_then(RgTextField::into_bytes) else {
            return Err("rg json output did not include line text".to_string());
        };
        let line_text = String::from_utf8_lossy(&line_bytes);
        matches_by_path
            .entry(RgPathKey::from_bytes(&path_bytes))
            .or_default()
            .push(LineMatch {
                line_number,
                line_text: matcher.display_line_text(line_text.trim_end_matches(['\n', '\r'])),
            });
    }

    Ok(matches_by_path)
}

fn normalize_rg_path(path: &str) -> String {
    path.strip_prefix("./").unwrap_or(path).to_string()
}

/// Map key for rg match results.
///
/// `display` mirrors the lossy relative path the native walker produces via
/// `normalize_display_path`, while `raw` keeps the exact bytes rg reported.
/// Keying on both prevents two distinct non-UTF-8 filenames whose lossy
/// display strings collide from being silently merged into one entry, and
/// `raw` lets us open the real file on Unix.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RgPathKey {
    display: String,
    raw: Vec<u8>,
}

impl RgPathKey {
    fn from_bytes(raw: &[u8]) -> Self {
        let raw = raw.strip_prefix(b"./".as_slice()).unwrap_or(raw);
        Self {
            display: normalize_rg_path(&String::from_utf8_lossy(raw)),
            raw: raw.to_vec(),
        }
    }

    fn to_absolute(&self, root: &Path) -> PathBuf {
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            root.join(std::ffi::OsStr::from_bytes(&self.raw))
        }
        #[cfg(not(unix))]
        {
            root.join(&self.display)
        }
    }
}

/// Minimal standard-alphabet base64 decoder for rg's `{"bytes": ...}` JSON
/// fields. Returns None on any non-base64 input.
fn decode_base64(input: &str) -> Option<Vec<u8>> {
    fn sextet(byte: u8) -> Option<u32> {
        match byte {
            b'A'..=b'Z' => Some(u32::from(byte - b'A')),
            b'a'..=b'z' => Some(u32::from(byte - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(byte - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }

    let data: Vec<u8> = input
        .bytes()
        .filter(|byte| *byte != b'=' && !byte.is_ascii_whitespace())
        .collect();
    let mut out = Vec::with_capacity(data.len() * 3 / 4 + 3);
    for chunk in data.chunks(4) {
        let mut acc: u32 = 0;
        for &byte in chunk {
            acc = (acc << 6) | sextet(byte)?;
        }
        match chunk.len() {
            4 => {
                out.push((acc >> 16) as u8);
                out.push((acc >> 8) as u8);
                out.push(acc as u8);
            }
            3 => {
                acc <<= 6;
                out.push((acc >> 16) as u8);
                out.push((acc >> 8) as u8);
            }
            2 => {
                acc <<= 12;
                out.push((acc >> 16) as u8);
            }
            _ => return None,
        }
    }
    Some(out)
}

fn process_rg_match_file(
    root: &Path,
    path: RgPathKey,
    matches: Vec<LineMatch>,
) -> Option<FileMatches> {
    let absolute_path = path.to_absolute(root);
    let path = path.display;
    if matches.len() >= DENSE_MATCH_SKIP_STRUCTURE_THRESHOLD {
        return Some(build_dense_file_matches(path, &absolute_path, matches));
    }

    let text = read_text_file(&absolute_path)?;
    let structure = extract_file_structure(&absolute_path, &path, &text);
    let grouping = group_matches(&structure.items, &matches);
    Some(FileMatches {
        path,
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

#[derive(Debug, serde::Deserialize)]
struct RgEvent {
    #[serde(rename = "type")]
    kind: String,
    data: Option<RgEventData>,
}

#[derive(Debug, serde::Deserialize)]
struct RgEventData {
    path: Option<RgTextField>,
    lines: Option<RgTextField>,
    line_number: Option<usize>,
}

#[derive(Debug, serde::Deserialize)]
struct RgTextField {
    text: Option<String>,
    bytes: Option<String>,
}

impl RgTextField {
    /// Raw bytes of the field: UTF-8 `text` as-is, or base64-decoded `bytes`.
    fn into_bytes(self) -> Option<Vec<u8>> {
        if let Some(text) = self.text {
            return Some(text.into_bytes());
        }
        self.bytes.as_deref().and_then(decode_base64)
    }
}

fn process_file(entry: crate::workspace::FileEntry, matcher: &Matcher) -> Option<FileMatches> {
    let text = read_text_file(&entry.path)?;

    let mut matches = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        if matcher.is_match(line) {
            matches.push(LineMatch {
                line_number: idx + 1,
                line_text: matcher.display_line_text(line),
            });
        }
    }

    if matches.is_empty() {
        return None;
    }

    if matches.len() >= DENSE_MATCH_SKIP_STRUCTURE_THRESHOLD {
        return Some(build_dense_file_matches(
            entry.relative_path,
            &entry.path,
            matches,
        ));
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

fn build_dense_file_matches(
    path: String,
    absolute_path: &Path,
    matches: Vec<LineMatch>,
) -> FileMatches {
    FileMatches {
        language: infer_language(absolute_path),
        role: crate::structure::infer_role(&path),
        path,
        groups: vec![file_scope_group((0..matches.len()).collect())],
        matches,
        total_symbols: 0,
        matched_symbol_count: 0,
        other_symbols: Vec::new(),
        other_symbols_omitted_count: 0,
    }
}

fn infer_language(path: &Path) -> String {
    match path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
    {
        "rs" => "rust",
        "ts" | "tsx" => "typescript",
        "js" | "jsx" => "javascript",
        "py" => "python",
        "md" => "markdown",
        "json" => "json",
        "yaml" | "yml" => "yaml",
        _ => "text",
    }
    .to_string()
}

#[derive(Clone)]
struct Matcher {
    kind: MatcherKind,
}

impl Matcher {
    fn new(query: &str, regex: bool) -> Result<Self, String> {
        let kind = if regex {
            MatcherKind::Regex(Regex::new(query).map_err(|err| {
                format!(
                    "invalid regex: {err}\n\nhint: omit regex=true (or escape special characters) to search for this text literally"
                )
            })?)
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

    fn display_line_text(&self, line: &str) -> String {
        compact_match_line(line, self.first_match_span(line))
    }

    fn first_match_span(&self, line: &str) -> Option<(usize, usize)> {
        match &self.kind {
            MatcherKind::Literal(literal) => line
                .find(literal)
                .map(|start| (start, start.saturating_add(literal.len()))),
            MatcherKind::Regex(regex) => regex.find(line).map(|found| (found.start(), found.end())),
        }
    }
}

fn compact_match_line(line: &str, match_span: Option<(usize, usize)>) -> String {
    let char_count = line.chars().count();
    if char_count <= MAX_MATCH_LINE_CHARS {
        return line.to_string();
    }

    let (match_start, match_end) = match_span.unwrap_or((0, 0));
    let match_start_char = line[..match_start.min(line.len())].chars().count();
    let match_end_char = line[..match_end.min(line.len())].chars().count();
    let match_len_chars = match_end_char.saturating_sub(match_start_char).max(1);

    let start_char = match_start_char.saturating_sub(MATCH_LINE_PREFIX_CONTEXT_CHARS);
    let mut end_char = start_char
        .saturating_add(MAX_MATCH_LINE_CHARS)
        .max(match_start_char.saturating_add(match_len_chars));
    if end_char > char_count {
        end_char = char_count;
    }
    let start_char = end_char
        .saturating_sub(MAX_MATCH_LINE_CHARS)
        .min(start_char);

    let omitted_prefix = start_char;
    let omitted_suffix = char_count.saturating_sub(end_char);
    let snippet: String = line
        .chars()
        .skip(start_char)
        .take(end_char.saturating_sub(start_char))
        .collect();

    match (omitted_prefix > 0, omitted_suffix > 0) {
        (true, true) => format!(
            "…{} … [truncated: {} chars before, {} chars after]",
            snippet, omitted_prefix, omitted_suffix
        ),
        (true, false) => format!("…{} [truncated: {} chars before]", snippet, omitted_prefix),
        (false, true) => format!("{} … [truncated: {} chars after]", snippet, omitted_suffix),
        (false, false) => snippet,
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
    let options = if matches.len() >= DENSE_MATCH_LIMITED_GROUPING_THRESHOLD {
        GroupingOptions {
            max_groups: DENSE_GROUPS_LIMIT,
            other_symbols_limit: DENSE_OTHER_SYMBOLS_LIMIT,
        }
    } else {
        GroupingOptions {
            max_groups: usize::MAX,
            other_symbols_limit: OTHER_SYMBOLS_LIMIT,
        }
    };

    group_matches_with_options(items, matches, options)
}

struct GroupingOptions {
    max_groups: usize,
    other_symbols_limit: usize,
}

fn group_matches_with_options(
    items: &[StructureItem],
    matches: &[LineMatch],
    options: GroupingOptions,
) -> GroupingResult {
    let mut symbol_groups = Vec::new();
    let mut matched_indices = Vec::new();
    let mut file_scope_matches = Vec::new();
    let mut item_idx = 0usize;
    let mut last_grouped_item_idx = None;

    for (match_idx, line_match) in matches.iter().enumerate() {
        while item_idx < items.len() && items[item_idx].end_line < line_match.line_number {
            item_idx += 1;
        }

        if let Some(item) = items.get(item_idx)
            && item.start_line <= line_match.line_number
            && line_match.line_number <= item.end_line
        {
            if matched_indices.last().copied() != Some(item_idx) {
                matched_indices.push(item_idx);
                if symbol_groups.len() < options.max_groups {
                    symbol_groups.push(MatchGroup {
                        kind: item.kind.clone(),
                        label: item.label.clone(),
                        start_line: Some(item.start_line),
                        end_line: Some(item.end_line),
                        match_indices: vec![match_idx],
                    });
                    last_grouped_item_idx = Some(item_idx);
                } else {
                    file_scope_matches.push(match_idx);
                    last_grouped_item_idx = None;
                }
            } else if last_grouped_item_idx == Some(item_idx) {
                let group = symbol_groups
                    .last_mut()
                    .expect("group exists for grouped symbol");
                group.match_indices.push(match_idx);
            } else {
                file_scope_matches.push(match_idx);
            }
        } else {
            file_scope_matches.push(match_idx);
        }
    }

    let mut groups =
        Vec::with_capacity(symbol_groups.len() + usize::from(!file_scope_matches.is_empty()));
    if !file_scope_matches.is_empty() {
        groups.push(MatchGroup {
            kind: "file-scope".to_string(),
            label: "<file scope>".to_string(),
            start_line: None,
            end_line: None,
            match_indices: file_scope_matches,
        });
    }
    groups.extend(symbol_groups);

    let matched_symbol_count = matched_indices.len();
    let mut other_symbols = Vec::new();
    let mut other_symbols_omitted_count = 0;
    let mut matched_iter = matched_indices.into_iter().peekable();
    for (idx, item) in items.iter().enumerate() {
        if matched_iter.peek().copied() == Some(idx) {
            matched_iter.next();
            continue;
        }
        if other_symbols.len() < options.other_symbols_limit {
            other_symbols.push(item.clone());
        } else {
            other_symbols_omitted_count += 1;
        }
    }

    GroupingResult {
        matched_symbol_count,
        groups,
        other_symbols,
        other_symbols_omitted_count,
    }
}

fn file_scope_group(match_indices: Vec<usize>) -> MatchGroup {
    MatchGroup {
        kind: "file-scope".to_string(),
        label: "<file scope>".to_string(),
        start_line: None,
        end_line: None,
        match_indices,
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

    fn by_display<'a>(
        map: &'a BTreeMap<RgPathKey, Vec<LineMatch>>,
        name: &str,
    ) -> &'a Vec<LineMatch> {
        map.iter()
            .find(|(key, _)| key.display == name)
            .map(|(_, matches)| matches)
            .unwrap_or_else(|| panic!("no entry with display path {name:?}"))
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
    fn user_ripgrep_config_does_not_change_rg_path_results() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("keep.txt"), "needle_zzz\n").unwrap();
        fs::write(dir.path().join(".hidden_needle"), "needle_zzz\n").unwrap();

        // A user config that would change semantics if rg were allowed to
        // read it (it would surface hidden files).
        let config = dir.path().join("rgconfig");
        fs::write(&config, "--hidden\n--no-ignore\n").unwrap();

        let original = std::env::var_os("RIPGREP_CONFIG_PATH");
        unsafe { std::env::set_var("RIPGREP_CONFIG_PATH", &config) };
        let mut args = grep_args("needle_zzz");
        args.paths_only = true;
        let rg_result = run_grep_with_rg(dir.path(), &args);
        match original {
            Some(value) => unsafe { std::env::set_var("RIPGREP_CONFIG_PATH", value) },
            None => unsafe { std::env::remove_var("RIPGREP_CONFIG_PATH") },
        }

        // Skip silently when rg is not installed; this test targets the rg
        // fast path only.
        let Some(rg_result) = rg_result.unwrap() else {
            return;
        };
        let paths = rg_result
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec!["keep.txt"],
            "user ripgrep config leaked into agentgrep results"
        );
    }

    #[test]
    fn rgignore_is_honored_by_both_rg_and_native_paths() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("keep.txt"), "needle_zzz\n").unwrap();
        fs::write(dir.path().join("skipme.txt"), "needle_zzz\n").unwrap();
        fs::write(dir.path().join(".rgignore"), "skipme.txt\n").unwrap();

        let mut args = grep_args("needle_zzz");
        args.paths_only = true;

        let native = run_grep_native(dir.path(), &args).unwrap();
        let native_paths = native
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            native_paths,
            vec!["keep.txt"],
            "native walker must honor .rgignore like ripgrep does"
        );

        if let Some(rg_result) = run_grep_with_rg(dir.path(), &args).unwrap() {
            let rg_paths = rg_result
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>();
            assert_eq!(rg_paths, vec!["keep.txt"]);
        }
    }

    #[test]
    fn no_ignore_flag_overrides_rgignore_in_both_paths() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("keep.txt"), "needle_zzz\n").unwrap();
        fs::write(dir.path().join("skipme.txt"), "needle_zzz\n").unwrap();
        fs::write(dir.path().join(".rgignore"), "skipme.txt\n").unwrap();

        let mut args = grep_args("needle_zzz");
        args.paths_only = true;
        args.no_ignore = true;

        let native = run_grep_native(dir.path(), &args).unwrap();
        let native_paths = native
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(native_paths, vec!["keep.txt", "skipme.txt"]);

        if let Some(rg_result) = run_grep_with_rg(dir.path(), &args).unwrap() {
            let rg_paths = rg_result
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>();
            assert_eq!(rg_paths, vec!["keep.txt", "skipme.txt"]);
        }
    }

    #[test]
    fn grep_falls_back_to_native_when_rg_missing() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("a.rs"),
            "fn auth_status() {}\nlet x = auth_status();\n",
        )
        .unwrap();

        // Simulate an environment where rg is not on PATH.
        let original_path = std::env::var_os("PATH");
        unsafe { std::env::set_var("PATH", "") };
        let result = run_grep(dir.path(), &grep_args("auth_status"));
        match original_path {
            Some(path) => unsafe { std::env::set_var("PATH", path) },
            None => unsafe { std::env::remove_var("PATH") },
        }

        let result = result.unwrap();
        assert_eq!(result.total_files, 1);
        assert_eq!(result.total_matches, 2);
        assert_eq!(result.files[0].path, "a.rs");
    }

    #[cfg(unix)]
    fn native_grep_paths(root: &Path, args: &GrepArgs) -> Vec<String> {
        run_grep_native(root, args)
            .unwrap()
            .files
            .into_iter()
            .map(|file| file.path)
            .collect()
    }

    /// Symlinked files, symlinked directories, and broken symlinks must yield
    /// the same result set on the rg fast path and the native fallback.
    #[test]
    #[cfg(unix)]
    fn grep_includes_symlinked_files_and_dirs_and_tolerates_broken_links_on_both_paths() {
        use std::os::unix::fs::symlink;

        let outside = tempdir().unwrap();
        fs::write(outside.path().join("target.txt"), "sym_needle in target\n").unwrap();
        fs::create_dir(outside.path().join("dir")).unwrap();
        fs::write(
            outside.path().join("dir").join("inner.txt"),
            "sym_needle in dir\n",
        )
        .unwrap();

        let dir = tempdir().unwrap();
        fs::write(dir.path().join("regular.txt"), "sym_needle in regular\n").unwrap();
        symlink(
            outside.path().join("target.txt"),
            dir.path().join("linkfile.txt"),
        )
        .unwrap();
        symlink(outside.path().join("dir"), dir.path().join("linkdir")).unwrap();
        symlink(
            outside.path().join("missing.txt"),
            dir.path().join("broken.txt"),
        )
        .unwrap();

        let args = grep_args("sym_needle");
        let expected = vec![
            "linkdir/inner.txt".to_string(),
            "linkfile.txt".to_string(),
            "regular.txt".to_string(),
        ];

        assert_eq!(native_grep_paths(dir.path(), &args), expected);

        // Only meaningful when rg is installed; run_grep prefers the rg path.
        if which_rg() {
            let rg_result = run_grep(dir.path(), &args).unwrap();
            let rg_paths = rg_result
                .files
                .into_iter()
                .map(|file| file.path)
                .collect::<Vec<_>>();
            assert_eq!(rg_paths, expected);

            let mut paths_only_args = grep_args("sym_needle");
            paths_only_args.paths_only = true;
            let paths = run_rg_paths_only(dir.path(), &paths_only_args)
                .unwrap()
                .expect("rg should be available");
            assert_eq!(paths, expected);

            // Broken symlink plus zero matches: rg exits 2 with empty stdout,
            // which must fall back (None) rather than hard-error.
            let no_match_args = grep_args("zz_absent_needle");
            assert!(
                run_rg_paths_only(dir.path(), &no_match_args)
                    .unwrap()
                    .is_none()
            );
            let full = run_grep(dir.path(), &no_match_args).unwrap();
            assert_eq!(full.total_files, 0);
        }
    }

    /// A symlinked scope root must behave like the real directory on both paths.
    #[test]
    #[cfg(unix)]
    fn grep_supports_symlinked_root_on_both_paths() {
        use std::os::unix::fs::symlink;

        let real = tempdir().unwrap();
        fs::write(real.path().join("a.txt"), "root_needle here\n").unwrap();
        let holder = tempdir().unwrap();
        let root_link = holder.path().join("rootlink");
        symlink(real.path(), &root_link).unwrap();

        let args = grep_args("root_needle");
        assert_eq!(
            native_grep_paths(&root_link, &args),
            vec!["a.txt".to_string()]
        );
        let result = run_grep(&root_link, &args).unwrap();
        assert_eq!(result.total_files, 1);
        assert_eq!(result.files[0].path, "a.txt");
    }

    #[cfg(unix)]
    fn which_rg() -> bool {
        std::process::Command::new("rg")
            .arg("--version")
            .output()
            .is_ok()
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
    fn literal_grep_query_starting_with_dash_is_not_treated_as_flag() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "cargo build --features full\nplain line\n",
        )
        .unwrap();

        let result = run_grep(dir.path(), &grep_args("--features")).unwrap();
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.files[0].path, "Cargo.toml");
    }

    #[test]
    fn invalid_regex_error_suggests_literal_search() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.rs"), "todo_progress(\n").unwrap();

        let mut args = grep_args("todo_progress(");
        args.regex = true;

        let err = run_grep(dir.path(), &args).unwrap_err();
        assert!(err.contains("invalid regex"), "unexpected error: {err}");
        assert!(err.contains("literally"), "missing hint: {err}");
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
        assert_eq!(file.groups[0].match_count(), 2);
        assert_eq!(file.groups[1].label, "auth_status");
        assert_eq!(file.groups[1].match_count(), 2);
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
        assert_eq!(
            file.groups[0]
                .resolved_matches(&file.matches)
                .next()
                .unwrap()
                .line_number,
            1
        );
    }

    #[test]
    fn dense_grep_skips_structure_extraction_and_uses_file_scope_group() {
        let dir = tempdir().unwrap();
        let mut text = String::new();
        for idx in 0..DENSE_MATCH_SKIP_STRUCTURE_THRESHOLD {
            text.push_str(&format!("pub fn item_{idx}() {{}}\n"));
        }
        fs::write(dir.path().join("dense.rs"), text).unwrap();

        let result = run_grep(dir.path(), &grep_args("pub")).unwrap();
        let file = &result.files[0];
        assert_eq!(file.language, "rust");
        assert_eq!(file.total_symbols, 0);
        assert_eq!(file.matched_symbol_count, 0);
        assert_eq!(file.groups.len(), 1);
        assert_eq!(file.groups[0].label, "<file scope>");
        assert_eq!(
            file.groups[0].match_count(),
            DENSE_MATCH_SKIP_STRUCTURE_THRESHOLD
        );
    }

    #[test]
    fn dense_grep_caps_group_count_for_moderately_dense_files() {
        let dir = tempdir().unwrap();
        let mut text = String::new();
        for idx in 0..(DENSE_MATCH_LIMITED_GROUPING_THRESHOLD + 4) {
            text.push_str(&format!("fn auth_match_{idx}() {{ auth_status(); }}\n"));
        }
        fs::write(dir.path().join("moderate.rs"), text).unwrap();

        let result = run_grep(dir.path(), &grep_args("auth_status")).unwrap();
        let file = &result.files[0];
        assert!(file.total_symbols >= DENSE_MATCH_LIMITED_GROUPING_THRESHOLD);
        assert!(file.groups.len() <= DENSE_GROUPS_LIMIT + 1);
        assert_eq!(file.groups[0].label, "<file scope>");
    }

    #[test]
    fn parse_rg_plain_groups_matches_by_file() {
        let stdout = b"src/main.rs:12:3:spin_lock();\nsrc/main.rs:14:7:mutex_lock();\nREADME.md:2:1:spin_lock overview\n";
        let matcher = Matcher::new("lock", false).unwrap();
        let parsed = parse_rg_plain(stdout, &matcher).unwrap();
        assert_eq!(parsed.len(), 2);
        let main_rs = by_display(&parsed, "src/main.rs");
        assert_eq!(main_rs.len(), 2);
        assert_eq!(main_rs[0].line_number, 12);
        assert_eq!(main_rs[1].line_text, "mutex_lock();");
        assert_eq!(by_display(&parsed, "README.md")[0].line_number, 2);
    }

    #[test]
    fn grep_compacts_huge_json_or_transcript_lines_around_the_match() {
        let dir = tempdir().unwrap();
        let huge_line = format!(
            "{{\"event\":\"tool_done\",\"output\":\"{}set_status_notice{}\"}}",
            "a".repeat(900),
            "b".repeat(900)
        );
        fs::write(dir.path().join("timeline.json"), huge_line).unwrap();

        let result = run_grep(dir.path(), &grep_args("set_status_notice")).unwrap();
        let line_text = &result.files[0].matches[0].line_text;

        assert!(line_text.contains("set_status_notice"));
        assert!(line_text.contains("[truncated:"), "{line_text}");
        assert!(
            line_text.chars().count() < 340,
            "line excerpt should stay compact, got {} chars: {line_text}",
            line_text.chars().count()
        );
    }

    #[test]
    fn rg_plain_parser_compacts_huge_lines_too() {
        let matcher = Matcher::new("status_notice", false).unwrap();
        let stdout = format!(
            "assets/demo.json:1:100:{}status_notice{}\n",
            "x".repeat(700),
            "y".repeat(700)
        );

        let parsed = parse_rg_plain(stdout.as_bytes(), &matcher).unwrap();
        let line_text = &by_display(&parsed, "assets/demo.json")[0].line_text;

        assert!(line_text.contains("status_notice"));
        assert!(line_text.contains("[truncated:"), "{line_text}");
        assert!(line_text.chars().count() < 340);
    }

    #[test]
    fn decode_base64_roundtrips() {
        assert_eq!(
            decode_base64("aGVsbG8=").as_deref(),
            Some(b"hello".as_slice())
        );
        assert_eq!(decode_base64("aGk=").as_deref(), Some(b"hi".as_slice()));
        assert_eq!(decode_base64("aA==").as_deref(), Some(b"h".as_slice()));
        assert_eq!(decode_base64("").as_deref(), Some(b"".as_slice()));
        assert_eq!(
            decode_base64("bmVlZGxlX3p6eiD//iBtb3JlCg==").as_deref(),
            Some(b"needle_zzz \xff\xfe more\n".as_slice())
        );
        assert_eq!(decode_base64("!!!!"), None);
    }

    #[test]
    fn parse_rg_json_accepts_bytes_encoded_line_text() {
        // rg --json emits {"bytes": base64} instead of {"text": ...} when the
        // matched line contains invalid UTF-8. This must not fail the grep.
        let matcher = Matcher::new("needle_zzz", false).unwrap();
        let stdout = concat!(
            r#"{"type":"begin","data":{"path":{"text":"./badcontent.txt"}}}"#,
            "\n",
            r#"{"type":"match","data":{"path":{"text":"./badcontent.txt"},"lines":{"bytes":"bmVlZGxlX3p6eiD//iBtb3JlCg=="},"line_number":1,"absolute_offset":0,"submatches":[]}}"#,
            "\n",
            r#"{"type":"end","data":{"path":{"text":"./badcontent.txt"}}}"#,
            "\n",
        );

        let parsed = parse_rg_json(stdout.as_bytes(), &matcher).unwrap();
        assert_eq!(parsed.len(), 1);
        let matches = by_display(&parsed, "badcontent.txt");
        assert_eq!(matches[0].line_number, 1);
        assert_eq!(matches[0].line_text, "needle_zzz \u{FFFD}\u{FFFD} more");
    }

    #[test]
    fn parse_rg_json_accepts_bytes_encoded_paths() {
        // Non-UTF-8 filenames are also emitted as {"bytes": base64}.
        let matcher = Matcher::new("needle", false).unwrap();
        let stdout = concat!(
            r#"{"type":"match","data":{"path":{"bytes":"Li9h/y50eHQ="},"lines":{"text":"needle here\n"},"line_number":3,"absolute_offset":0,"submatches":[]}}"#,
            "\n",
        );

        let parsed = parse_rg_json(stdout.as_bytes(), &matcher).unwrap();
        assert_eq!(parsed.len(), 1);
        let (key, matches) = parsed.iter().next().unwrap();
        assert_eq!(key.display, "a\u{FFFD}.txt");
        assert_eq!(key.raw, b"a\xff.txt");
        assert_eq!(matches[0].line_number, 3);
        assert_eq!(matches[0].line_text, "needle here");
    }

    #[test]
    fn parse_rg_json_keeps_lossy_colliding_paths_separate() {
        // b"a\xff.txt" and b"a\xfe.txt" both lossy-decode to "a\u{FFFD}.txt".
        // They must remain distinct map entries instead of silently merging.
        let matcher = Matcher::new("needle", false).unwrap();
        let stdout = concat!(
            r#"{"type":"match","data":{"path":{"bytes":"Li9h/y50eHQ="},"lines":{"text":"needle one\n"},"line_number":1,"absolute_offset":0,"submatches":[]}}"#,
            "\n",
            r#"{"type":"match","data":{"path":{"bytes":"Li9h/i50eHQ="},"lines":{"text":"needle two\n"},"line_number":1,"absolute_offset":0,"submatches":[]}}"#,
            "\n",
        );

        let parsed = parse_rg_json(stdout.as_bytes(), &matcher).unwrap();
        assert_eq!(parsed.len(), 2, "distinct raw paths must not merge");
        let raws: Vec<&[u8]> = parsed.keys().map(|key| key.raw.as_slice()).collect();
        assert!(raws.contains(&b"a\xfe.txt".as_slice()));
        assert!(raws.contains(&b"a\xff.txt".as_slice()));
        for matches in parsed.values() {
            assert_eq!(matches.len(), 1);
        }
    }

    #[test]
    fn grep_survives_non_utf8_line_content_in_utf8_named_file() {
        // Regression: this used to exit with "rg json output did not include
        // UTF-8 line text" on the rg fast path while succeeding natively.
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("badcontent.txt"),
            b"needle_zzz \xff\xfe more\n",
        )
        .unwrap();
        fs::write(dir.path().join("good.txt"), "needle_zzz clean\n").unwrap();

        let result = run_grep(dir.path(), &grep_args("needle_zzz")).unwrap();
        assert_eq!(result.total_files, 2);
        assert_eq!(result.total_matches, 2);
        assert_eq!(result.files[0].path, "badcontent.txt");
        assert!(result.files[0].matches[0].line_text.contains("needle_zzz"));
        assert_eq!(result.files[1].path, "good.txt");
    }

    #[cfg(unix)]
    #[test]
    fn grep_rg_and_native_agree_on_non_utf8_corpus() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        // Corpus: non-UTF-8 content lines, non-UTF-8 filenames, and both,
        // including two filenames whose lossy display strings collide.
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("clean.txt"), "needle_qqq plain\n").unwrap();
        fs::write(
            dir.path().join("badcontent.txt"),
            b"needle_qqq \xff\xfe binaryish\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(OsStr::from_bytes(b"bad\xffname.txt")),
            "needle_qqq in bad name\n",
        )
        .unwrap();
        fs::write(
            dir.path().join(OsStr::from_bytes(b"bad\xfename.txt")),
            b"needle_qqq \xff both bad\n",
        )
        .unwrap();

        let rg_result = run_grep_with_rg(dir.path(), &grep_args("needle_qqq")).unwrap();
        let native_result = run_grep_native(dir.path(), &grep_args("needle_qqq")).unwrap();

        let Some(rg_result) = rg_result else {
            // rg not installed; nothing to compare.
            return;
        };

        assert_eq!(rg_result.total_matches, 4);
        assert_eq!(rg_result.total_files, 4, "colliding names must not merge");
        assert_eq!(rg_result.total_files, native_result.total_files);
        assert_eq!(rg_result.total_matches, native_result.total_matches);

        let summarize = |result: &GrepResult| {
            let mut summary = result
                .files
                .iter()
                .map(|file| {
                    (
                        file.path.clone(),
                        file.matches
                            .iter()
                            .map(|line| (line.line_number, line.line_text.clone()))
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>();
            summary.sort();
            summary
        };
        assert_eq!(summarize(&rg_result), summarize(&native_result));
    }
}
