use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StructureItem {
    pub kind: String,
    pub label: String,
    pub start_line: usize,
    pub end_line: usize,
    pub line_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FileStructure {
    pub language: String,
    pub role: String,
    pub items: Vec<StructureItem>,
}

pub fn extract_file_structure(path: &Path, relative_path: &str, text: &str) -> FileStructure {
    let language = detect_language(path);
    let mut items = match language {
        "rust" => extract_rust(text),
        "typescript" | "javascript" => extract_ts_js(text),
        "python" => extract_python(text),
        "markdown" => extract_markdown(text),
        _ => extract_generic(text),
    };

    finalize_ranges(text, &mut items);

    FileStructure {
        language: language.to_string(),
        role: infer_role(relative_path),
        items,
    }
}

pub fn enclosing_item(items: &[StructureItem], line_number: usize) -> Option<&StructureItem> {
    items
        .iter()
        .find(|item| item.start_line <= line_number && line_number <= item.end_line)
}

fn detect_language(path: &Path) -> &'static str {
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
}

pub(crate) fn infer_role(relative_path: &str) -> String {
    let path = relative_path.to_ascii_lowercase();
    if path.contains("/tests/") || path.contains("_test") || path.contains("test_") {
        "test".to_string()
    } else if path.contains("/docs/") || path.ends_with(".md") {
        "docs".to_string()
    } else if path.contains("/ui/") || path.contains("/tui/") || path.contains("view") {
        "ui".to_string()
    } else if path.contains("auth") {
        "auth".to_string()
    } else if path.contains("provider") {
        "provider".to_string()
    } else if path.contains("config") {
        "config".to_string()
    } else if path.contains("handler") || path.contains("router") {
        "handler".to_string()
    } else if path.contains("src/") {
        "implementation".to_string()
    } else {
        "generic".to_string()
    }
}

fn extract_rust(text: &str) -> Vec<StructureItem> {
    let mut items = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let line_number = idx + 1;
        let trimmed = line.trim_start();

        if let Some(label) = parse_rust_keyword_item(trimmed, "fn") {
            items.push(structure_item("function", label, line_number));
            continue;
        }
        if let Some(label) = parse_rust_keyword_item(trimmed, "struct") {
            items.push(structure_item("struct", label, line_number));
            continue;
        }
        if let Some(label) = parse_rust_keyword_item(trimmed, "enum") {
            items.push(structure_item("enum", label, line_number));
            continue;
        }
        if let Some(label) = parse_rust_keyword_item(trimmed, "trait") {
            items.push(structure_item("trait", label, line_number));
            continue;
        }
        if let Some(label) = parse_rust_impl_item(trimmed) {
            items.push(structure_item("impl", label, line_number));
        }
    }
    items
}

fn extract_ts_js(text: &str) -> Vec<StructureItem> {
    let mut items = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let line_number = idx + 1;
        let mut trimmed = line.trim_start();
        trimmed = strip_keyword_prefix(trimmed, "export")
            .unwrap_or(trimmed)
            .trim_start();

        if let Some(label) = parse_keyword_identifier(trimmed, "function") {
            items.push(structure_item("function", label, line_number));
            continue;
        }
        if let Some(label) = parse_keyword_identifier(trimmed, "class") {
            items.push(structure_item("class", label, line_number));
            continue;
        }
        if let Some(label) = parse_keyword_identifier(trimmed, "interface") {
            items.push(structure_item("interface", label, line_number));
            continue;
        }
        if let Some(label) = parse_ts_arrow_item(trimmed) {
            items.push(structure_item("function", label, line_number));
        }
    }
    items
}

fn extract_python(text: &str) -> Vec<StructureItem> {
    let mut items = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let line_number = idx + 1;
        let trimmed = line.trim_start();
        if let Some(label) = parse_keyword_identifier(trimmed, "def") {
            items.push(structure_item("function", label, line_number));
            continue;
        }
        if let Some(label) = parse_keyword_identifier(trimmed, "class") {
            items.push(structure_item("class", label, line_number));
        }
    }
    items
}

fn extract_markdown(text: &str) -> Vec<StructureItem> {
    let mut items = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let bytes = line.as_bytes();
        let level = bytes.iter().take_while(|&&byte| byte == b'#').count();
        if level == 0 || bytes.get(level).copied() != Some(b' ') {
            continue;
        }
        let label = line[level + 1..].trim();
        if label.is_empty() {
            continue;
        }
        items.push(StructureItem {
            kind: format!("heading{level}"),
            label: label.to_string(),
            start_line: idx + 1,
            end_line: idx + 1,
            line_count: 1,
        });
    }
    items
}

fn extract_generic(text: &str) -> Vec<StructureItem> {
    let mut items = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if is_generic_section(trimmed) {
            items.push(structure_item("section", trimmed, idx + 1));
        }
    }
    items
}

fn parse_rust_keyword_item<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let mut rest = strip_rust_visibility(line).trim_start();
    if keyword == "fn" {
        rest = strip_keyword_prefix(rest, "async")
            .unwrap_or(rest)
            .trim_start();
    }
    parse_keyword_identifier(rest, keyword)
}

fn parse_rust_impl_item(line: &str) -> Option<&str> {
    let mut rest = line.trim_start().strip_prefix("impl")?;
    if !rest.is_empty() {
        let next = rest.chars().next()?;
        if !next.is_whitespace() && next != '<' {
            return None;
        }
    }
    rest = rest.trim_start();
    if rest.starts_with('<') {
        rest = skip_balanced(rest, '<', '>')?.trim_start();
    }
    take_identifier_like(rest)
}

fn parse_ts_arrow_item(line: &str) -> Option<&str> {
    let (keyword, rest) = ["const", "let", "var"]
        .iter()
        .find_map(|keyword| strip_keyword_prefix(line, keyword).map(|rest| (*keyword, rest)))?;
    let _ = keyword;
    let (name, rest) = take_identifier(rest.trim_start())?;
    let rest = rest.trim_start();
    if !rest.starts_with('=') {
        return None;
    }
    if !rest.contains("=>") {
        return None;
    }
    Some(name)
}

fn parse_keyword_identifier<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = strip_keyword_prefix(line, keyword)?;
    let (identifier, _) = take_identifier(rest.trim_start())?;
    Some(identifier)
}

fn strip_rust_visibility(line: &str) -> &str {
    let Some(rest) = line.strip_prefix("pub") else {
        return line;
    };
    if !rest.is_empty() {
        let Some(next) = rest.chars().next() else {
            return rest;
        };
        if !next.is_whitespace() && next != '(' {
            return line;
        }
    }
    let rest = rest.trim_start();
    if rest.starts_with('(') {
        skip_balanced(rest, '(', ')').unwrap_or(rest)
    } else {
        rest
    }
}

fn strip_keyword_prefix<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(keyword)?;
    if rest.is_empty() {
        return Some(rest);
    }
    let next = rest.chars().next()?;
    if next.is_whitespace() {
        Some(rest)
    } else {
        None
    }
}

fn take_identifier(input: &str) -> Option<(&str, &str)> {
    let mut end = 0;
    for (idx, ch) in input.char_indices() {
        if idx == 0 {
            if !is_identifier_start(ch) {
                return None;
            }
        } else if !is_identifier_continue(ch) {
            end = idx;
            break;
        }
    }
    if end == 0 {
        end = input.len();
    }
    Some((&input[..end], &input[end..]))
}

fn take_identifier_like(input: &str) -> Option<&str> {
    let mut end = 0;
    for (idx, ch) in input.char_indices() {
        if ch.is_whitespace() || ch == '{' {
            end = idx;
            break;
        }
    }
    if end == 0 {
        end = input.len();
    }
    let token = input[..end].trim_end_matches(':').trim_end_matches(',');
    if token.is_empty() { None } else { Some(token) }
}

fn skip_balanced(input: &str, open: char, close: char) -> Option<&str> {
    let mut depth = 0usize;
    for (idx, ch) in input.char_indices() {
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth = depth.checked_sub(1)?;
            if depth == 0 {
                let next_idx = idx + ch.len_utf8();
                return Some(&input[next_idx..]);
            }
        }
    }
    None
}

fn is_identifier_start(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphabetic()
}

fn is_identifier_continue(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn is_generic_section(line: &str) -> bool {
    if line.len() < 4 {
        return false;
    }
    let mut chars = line.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_uppercase() {
        return false;
    }
    line.chars()
        .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || matches!(ch, '_' | '-' | ' '))
}

fn structure_item(kind: &str, label: &str, line_number: usize) -> StructureItem {
    StructureItem {
        kind: kind.to_string(),
        label: label.to_string(),
        start_line: line_number,
        end_line: line_number,
        line_count: 1,
    }
}

fn finalize_ranges(text: &str, items: &mut [StructureItem]) {
    let total_lines = text.lines().count().max(1);
    for idx in 0..items.len() {
        let end = if idx + 1 < items.len() {
            items[idx + 1]
                .start_line
                .saturating_sub(1)
                .max(items[idx].start_line)
        } else {
            total_lines
        };
        items[idx].end_line = end;
        items[idx].line_count = end.saturating_sub(items[idx].start_line) + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn extracts_rust_functions_and_structs() {
        let text =
            "pub struct AuthStatus {}\n\npub fn auth_status() {}\nfn render_status_bar() {}\n";
        let structure =
            extract_file_structure(Path::new("src/auth/mod.rs"), "src/auth/mod.rs", text);
        assert_eq!(structure.language, "rust");
        assert_eq!(structure.role, "auth");
        assert!(
            structure
                .items
                .iter()
                .any(|item| item.label == "AuthStatus")
        );
        assert!(
            structure
                .items
                .iter()
                .any(|item| item.label == "auth_status")
        );
        assert!(
            structure
                .items
                .iter()
                .any(|item| item.label == "render_status_bar")
        );
    }

    #[test]
    fn extracts_rust_pub_crate_async_and_impl_items() {
        let text = concat!(
            "pub(crate) async fn render_status_bar() {}\n",
            "impl<T> StatusView<T> {}\n",
            "pub trait Renderable {}\n",
        );
        let structure = extract_file_structure(Path::new("src/ui/view.rs"), "src/ui/view.rs", text);
        assert!(
            structure
                .items
                .iter()
                .any(|item| item.label == "render_status_bar")
        );
        assert!(
            structure
                .items
                .iter()
                .any(|item| item.label == "StatusView<T>")
        );
        assert!(
            structure
                .items
                .iter()
                .any(|item| item.label == "Renderable")
        );
    }

    #[test]
    fn extracts_markdown_headings_without_regex() {
        let text = "# Title\ntext\n## Subtitle\n";
        let structure = extract_file_structure(Path::new("docs/test.md"), "docs/test.md", text);
        assert_eq!(structure.language, "markdown");
        assert!(structure.items.iter().any(|item| item.label == "Title"));
        assert!(structure.items.iter().any(|item| item.label == "Subtitle"));
    }
}
