use crate::cli::FindArgs;
use crate::structure::{FileStructure, StructureItem, extract_file_structure};
use crate::workspace::{SearchScope, collect_text_files};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct FindResult {
    pub query: String,
    pub root: String,
    pub files: Vec<FindFile>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FindFile {
    pub path: String,
    pub role: String,
    pub language: String,
    pub score: i32,
    pub why: Vec<String>,
    pub structure: FindStructure,
}

#[derive(Debug, Clone, Serialize)]
pub struct FindStructure {
    pub items: Vec<StructureItem>,
    pub omitted_count: usize,
}

pub fn run_find(root: &Path, args: &FindArgs) -> FindResult {
    let query = args.query_parts.join(" ");
    let query_lower = query.to_ascii_lowercase();
    let query_tokens = tokenize_query(&query);

    let scope = SearchScope {
        root,
        file_type: args.file_type.as_deref(),
        hidden: args.hidden,
        no_ignore: args.no_ignore,
    };

    let mut files = Vec::new();
    for file in collect_text_files(&scope) {
        let relative_lower = file.relative_path.to_ascii_lowercase();
        let structure = extract_file_structure(&file.path, &file.relative_path, &file.text);
        let (score, why) = score_file(
            &query_lower,
            &query_tokens,
            &relative_lower,
            &structure,
            &file.text,
        );
        if score <= 0 {
            continue;
        }

        let shown_items = structure.items.iter().take(8).cloned().collect::<Vec<_>>();
        let omitted_count = structure.items.len().saturating_sub(shown_items.len());

        files.push(FindFile {
            path: file.relative_path,
            role: structure.role.clone(),
            language: structure.language.clone(),
            score,
            why,
            structure: FindStructure {
                items: shown_items,
                omitted_count,
            },
        });
    }

    files.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
    files.truncate(args.max_files);

    FindResult {
        query,
        root: root.display().to_string(),
        files,
    }
}

fn score_file(
    query_lower: &str,
    query_tokens: &[String],
    relative_lower: &str,
    structure: &FileStructure,
    text: &str,
) -> (i32, Vec<String>) {
    let mut score = 0;
    let mut why = Vec::new();
    let mut evidence_hits = 0;

    if relative_lower.contains(query_lower) {
        score += 120;
        why.push("path contains full query".to_string());
        evidence_hits += 1;
    }

    let matched_tokens = query_tokens
        .iter()
        .filter(|token| relative_lower.contains(token.as_str()))
        .count();
    if matched_tokens > 0 {
        score += (matched_tokens as i32) * 25;
        why.push(format!("path token matches: {matched_tokens}"));
        evidence_hits += matched_tokens;
    }

    let mut structure_hits = 0;
    for item in &structure.items {
        let label_lower = item.label.to_ascii_lowercase();
        if query_tokens.iter().any(|token| label_lower.contains(token)) {
            structure_hits += 1;
        }
    }
    if structure_hits > 0 {
        score += (structure_hits as i32) * 15;
        why.push(format!("symbol/outline hits: {structure_hits}"));
        evidence_hits += structure_hits;
    }

    let text_lower = text.to_ascii_lowercase();
    let text_hits = query_tokens
        .iter()
        .filter(|token| text_lower.contains(token.as_str()))
        .count();
    if text_hits > 0 {
        score += (text_hits as i32) * 8;
        why.push(format!("text hits: {text_hits}"));
        evidence_hits += text_hits;
    }

    if query_tokens
        .iter()
        .any(|token| structure.role.contains(token))
    {
        score += 20;
        why.push(format!("role matched: {}", structure.role));
    }

    match structure.role.as_str() {
        "implementation" | "auth" | "provider" | "ui" | "handler" => {
            score += 20;
            why.push(format!("code role boost: {}", structure.role));
        }
        "docs" => {
            score -= 25;
            why.push("docs penalty".to_string());
        }
        "test" => {
            score -= 15;
            why.push("test penalty".to_string());
        }
        _ => {}
    }

    if evidence_hits == 0 {
        return (0, Vec::new());
    }

    (score, why)
}

fn tokenize_query(query: &str) -> Vec<String> {
    query
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|part| !part.is_empty())
        .map(|part| part.to_ascii_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::FindArgs;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn ranked_find_prefers_matching_paths_and_symbols() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/auth")).unwrap();
        fs::create_dir_all(dir.path().join("src/tui")).unwrap();
        fs::write(
            dir.path().join("src/auth/mod.rs"),
            "pub struct AuthStatus {}\npub fn auth_status() {}\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("src/tui/app.rs"),
            "fn render_status_bar() {}\nfn draw_header() {}\n",
        )
        .unwrap();

        let args = FindArgs {
            query_parts: vec!["auth".to_string(), "status".to_string()],
            file_type: Some("rs".to_string()),
            json: false,
            max_files: 5,
            hidden: false,
            no_ignore: true,
            path: None,
        };

        let result = run_find(dir.path(), &args);
        assert!(!result.files.is_empty());
        assert_eq!(result.files[0].path, "src/auth/mod.rs");
    }
}
