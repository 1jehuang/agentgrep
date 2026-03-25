use regex::Regex;
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
    let mut items = match language.as_str() {
        "rust" => extract_rust(text),
        "typescript" | "javascript" => extract_ts_js(text),
        "python" => extract_python(text),
        "markdown" => extract_markdown(text),
        _ => extract_generic(text),
    };

    finalize_ranges(text, &mut items);

    FileStructure {
        language,
        role: infer_role(relative_path),
        items,
    }
}

pub fn enclosing_item(items: &[StructureItem], line_number: usize) -> Option<&StructureItem> {
    items
        .iter()
        .find(|item| item.start_line <= line_number && line_number <= item.end_line)
}

fn detect_language(path: &Path) -> String {
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

fn infer_role(relative_path: &str) -> String {
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
    let fn_re = Regex::new(r"^\s*(?:pub\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    let struct_re = Regex::new(r"^\s*(?:pub\s+)?struct\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    let enum_re = Regex::new(r"^\s*(?:pub\s+)?enum\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    let trait_re = Regex::new(r"^\s*(?:pub\s+)?trait\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    let impl_re = Regex::new(r"^\s*impl(?:\s*<[^>]+>)?\s+([^\s{]+)").unwrap();

    collect_by_regexes(
        text,
        &[
            ("function", &fn_re),
            ("struct", &struct_re),
            ("enum", &enum_re),
            ("trait", &trait_re),
            ("impl", &impl_re),
        ],
    )
}

fn extract_ts_js(text: &str) -> Vec<StructureItem> {
    let fn_re = Regex::new(r"^\s*(?:export\s+)?function\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    let class_re = Regex::new(r"^\s*(?:export\s+)?class\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    let interface_re =
        Regex::new(r"^\s*(?:export\s+)?interface\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    let arrow_re =
        Regex::new(r"^\s*(?:export\s+)?(?:const|let|var)\s+([A-Za-z_][A-Za-z0-9_]*)\s*=.*=>")
            .unwrap();
    collect_by_regexes(
        text,
        &[
            ("function", &fn_re),
            ("class", &class_re),
            ("interface", &interface_re),
            ("function", &arrow_re),
        ],
    )
}

fn extract_python(text: &str) -> Vec<StructureItem> {
    let def_re = Regex::new(r"^\s*def\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    let class_re = Regex::new(r"^\s*class\s+([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    collect_by_regexes(text, &[("function", &def_re), ("class", &class_re)])
}

fn extract_markdown(text: &str) -> Vec<StructureItem> {
    let heading_re = Regex::new(r"^(#+)\s+(.+)$").unwrap();
    let mut items = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        if let Some(caps) = heading_re.captures(line) {
            let level = caps.get(1).map(|m| m.as_str().len()).unwrap_or(1);
            let label = caps.get(2).map(|m| m.as_str().trim()).unwrap_or("");
            items.push(StructureItem {
                kind: format!("heading{level}"),
                label: label.to_string(),
                start_line: idx + 1,
                end_line: idx + 1,
                line_count: 1,
            });
        }
    }
    items
}

fn extract_generic(text: &str) -> Vec<StructureItem> {
    let section_re = Regex::new(r"^\s*([A-Z][A-Z0-9_\- ]{3,})\s*$").unwrap();
    collect_by_regexes(text, &[("section", &section_re)])
}

fn collect_by_regexes(text: &str, regexes: &[(&str, &Regex)]) -> Vec<StructureItem> {
    let mut items = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        for (kind, regex) in regexes {
            if let Some(caps) = regex.captures(line) {
                let label = caps.get(1).map(|m| m.as_str()).unwrap_or(line.trim());
                items.push(StructureItem {
                    kind: (*kind).to_string(),
                    label: label.to_string(),
                    start_line: idx + 1,
                    end_line: idx + 1,
                    line_count: 1,
                });
                break;
            }
        }
    }
    items
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
}
