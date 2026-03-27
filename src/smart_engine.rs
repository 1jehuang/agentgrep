use crate::cli::{FullRegionMode, SmartArgs};
use crate::context::{Familiarity, HarnessContext};
use crate::smart_dsl::{Relation, SmartQuery};
use crate::structure::{StructureItem, extract_file_structure, infer_role};
use crate::workspace::{SearchScope, TextFile, collect_file_entries, read_text_file};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct SmartResult {
    pub query: SmartQuery,
    pub root: String,
    pub summary: SmartSummary,
    pub files: Vec<SmartFile>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SmartSummary {
    pub total_files: usize,
    pub total_regions: usize,
    pub best_file: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SmartFile {
    pub path: String,
    pub role: String,
    pub language: String,
    pub score: i32,
    pub why: Vec<String>,
    pub structure: SmartStructure,
    pub regions: Vec<SmartRegion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_applied: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SmartStructure {
    pub items: Vec<StructureItem>,
    pub omitted_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SmartRegion {
    pub kind: String,
    pub label: String,
    pub start_line: usize,
    pub end_line: usize,
    pub line_count: usize,
    pub score: i32,
    pub body: String,
    pub full_region: bool,
    pub why: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_applied: Option<String>,
}

pub fn run_smart(root: &Path, query: &SmartQuery, args: &SmartArgs) -> Result<SmartResult, String> {
    let scope = SearchScope {
        root,
        file_type: args.file_type.as_deref(),
        glob: args.glob.as_deref(),
        hidden: args.hidden,
        no_ignore: args.no_ignore,
    };

    let relation_terms = relation_terms(&query.relation);
    let subject_lower = query.subject.to_ascii_lowercase();
    let subject_tokens = tokenize_subject(&query.subject);
    let support_terms = query
        .support
        .iter()
        .map(|s| s.to_ascii_lowercase())
        .collect::<Vec<_>>();
    let path_hint = query.path_hint.as_ref().map(|s| s.to_ascii_lowercase());
    let context = HarnessContext::load(args.context_json.as_deref())?;

    let mut files = Vec::new();
    for entry in collect_file_entries(&scope) {
        let relative_lower = entry.relative_path.to_ascii_lowercase();
        if let Some(path_hint) = &path_hint
            && !relative_lower.contains(path_hint)
        {
            continue;
        }

        let inferred_role = infer_role(&entry.relative_path);
        if should_filter_kind(query.kind.as_deref(), &inferred_role) {
            continue;
        }

        let Some(text) = read_text_file(&entry.path) else {
            continue;
        };
        let file = TextFile {
            path: entry.path,
            relative_path: entry.relative_path,
            text,
        };
        let text_lower = file.text.to_ascii_lowercase();
        let structure = extract_file_structure(&file.path, &file.relative_path, &file.text);

        let subject_mentions = find_lines(&file.text, &subject_lower);
        if subject_mentions.is_empty()
            && !relative_lower.contains(&subject_lower)
            && !structure
                .items
            .iter()
            .any(|item| item.label.to_ascii_lowercase().contains(&subject_lower))
        {
            continue;
        }

        let relation_hits = relation_terms
            .iter()
            .filter(|term| {
                relative_lower.contains(term.as_str())
                    || structure
                        .items
                        .iter()
                        .any(|item| item.label.to_ascii_lowercase().contains(term.as_str()))
                    || text_lower.contains(term.as_str())
            })
            .count();

        let support_hits = support_terms
            .iter()
            .filter(|term| text_lower.contains(term.as_str()))
            .count();

        let mut file_score = 0;
        let mut why = vec!["exact subject match or symbol hit".to_string()];
        file_score += (subject_mentions.len() as i32) * 5;
        if exact_subject_path_match(&relative_lower, &subject_lower) {
            file_score += match query.relation {
                Relation::Defined | Relation::Implementation => 140,
                _ => 60,
            };
            why.push("path matches subject variant".to_string());
        }
        if relation_hits > 0 {
            file_score += (relation_hits as i32) * 20;
            why.push(format!("relation-context hits: {relation_hits}"));
        }
        if support_hits > 0 {
            file_score += (support_hits as i32) * 10;
            why.push(format!("support-term hits: {support_hits}"));
        }
        if role_aligns(&structure.role, &query.relation) {
            file_score += 20;
            why.push(format!("role aligned: {}", structure.role));
        }
        match structure.role.as_str() {
            "implementation" | "auth" | "provider" | "ui" | "handler" => {
                file_score += 25;
                why.push(format!("code role boost: {}", structure.role));
            }
            "docs" => {
                file_score -= 50;
                why.push("docs penalty".to_string());
            }
            "test" => {
                file_score -= 20;
                why.push("test penalty".to_string());
            }
            _ => {}
        }
        if let Some(path_hint) = &path_hint {
            file_score += 30;
            why.push(format!("path hint matched: {path_hint}"));
        }

        let mut regions = build_regions(
            &file,
            &structure.items,
            &subject_lower,
            &subject_tokens,
            &query.relation,
            args,
            context.as_ref(),
        );
        if regions.is_empty() {
            continue;
        }
        regions.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| a.start_line.cmp(&b.start_line))
        });
        let best_region_score = regions.first().map(|r| r.score).unwrap_or(0);
        if let Some(best_region) = regions.first() {
            if best_region
                .why
                .iter()
                .any(|reason| reason == "test/example penalty")
            {
                file_score -= 40;
                why.push("best region is test/example-like".to_string());
            }
            if best_region
                .why
                .iter()
                .any(|reason| reason == "cli/example penalty")
            {
                file_score -= 50;
                why.push("best region is cli/example-like".to_string());
            }
        }
        file_score += best_region_score / 2;
        why.push(format!("best region score: {best_region_score}"));
        regions.truncate(args.max_regions);

        let file_familiarity = context
            .as_ref()
            .map(|ctx| ctx.file_familiarity(&file.relative_path))
            .unwrap_or_default();
        let structure_budget = structure_budget_for_file(file_familiarity);
        let shown_items = select_structure_items(&structure.items, &regions, structure_budget);
        let omitted_count = structure.items.len().saturating_sub(shown_items.len());
        let context_applied = context_note_for_file(file_familiarity);

        files.push(SmartFile {
            path: file.relative_path,
            role: structure.role.clone(),
            language: structure.language.clone(),
            score: file_score,
            why,
            structure: SmartStructure {
                items: shown_items,
                omitted_count,
            },
            regions,
            context_applied,
        });
    }

    files.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.path.cmp(&b.path)));
    files.truncate(args.max_files);

    let total_regions = files.iter().map(|f| f.regions.len()).sum();
    let best_file = files.first().map(|f| f.path.clone());

    Ok(SmartResult {
        query: query.clone(),
        root: root.display().to_string(),
        summary: SmartSummary {
            total_files: files.len(),
            total_regions,
            best_file,
        },
        files,
    })
}

fn build_regions(
    file: &TextFile,
    items: &[StructureItem],
    subject_lower: &str,
    subject_tokens: &[String],
    relation: &Relation,
    args: &SmartArgs,
    context: Option<&HarnessContext>,
) -> Vec<SmartRegion> {
    let relation_terms = relation_terms(relation);
    let lines = file.text.lines().collect::<Vec<_>>();

    let mut regions = Vec::new();
    for item in items {
        let start_idx = item.start_line.saturating_sub(1);
        let end_idx = item.end_line.min(lines.len());
        if start_idx >= end_idx {
            continue;
        }

        let region_lines = &lines[start_idx..end_idx];
        let region_lower = region_lines
            .iter()
            .map(|line| line.to_ascii_lowercase())
            .collect::<Vec<_>>();

        let subject_line_hits = region_lower
            .iter()
            .enumerate()
            .filter_map(|(idx, line)| line.contains(subject_lower).then_some(idx))
            .collect::<Vec<_>>();
        let item_label_lower = item.label.to_ascii_lowercase();
        let exact_label_match = exact_subject_label_match(&item.label, subject_lower);
        let token_label_match = subject_tokens_match_label(&item.label, subject_tokens);
        if subject_line_hits.is_empty() && !exact_label_match && !token_label_match {
            continue;
        }

        let mut score = 80 + (subject_line_hits.len() as i32 * 10);
        let mut why = Vec::new();
        if !subject_line_hits.is_empty() {
            why.push("exact subject match".to_string());
        }
        let relation_hit = relation_terms.iter().any(|term| {
            item_label_lower.contains(term.as_str())
                || region_lower.iter().any(|line| line.contains(term.as_str()))
        });
        if relation_hit {
            score += 30;
            why.push("relation-context aligned".to_string());
        }

        let owner_match = exact_label_match || token_label_match;
        let kind = classify_region(item, relation, owner_match);
        if exact_label_match {
            score += match relation {
                Relation::Defined | Relation::Implementation => 120,
                _ => 50,
            };
            why.push("exact subject label match".to_string());
        } else if token_label_match {
            score += match relation {
                Relation::Defined | Relation::Implementation => 90,
                _ => 35,
            };
            why.push("subject tokens match label".to_string());
        } else if matches!(relation, Relation::Defined | Relation::Implementation) {
            score -= 50;
            why.push("non-owner penalty".to_string());
        }
        let region_text = region_lines.join("\n");
        if owner_match && matches!(relation, Relation::Defined | Relation::Implementation) {
            if region_text.trim_start().starts_with("pub ") {
                score += 20;
                why.push("public owner bonus".to_string());
            }
            let normalized_label = normalize_match_text(&item.label);
            if normalized_label.ends_with("tool") {
                score += 15;
                why.push("tool suffix bonus".to_string());
            }
            if normalized_label.ends_with("input") || normalized_label.ends_with("output") {
                score -= 10;
                why.push("auxiliary suffix penalty".to_string());
            }
        }
        match kind.as_str() {
            "render-site" | "definition" | "handler" | "assignment" => score += 20,
            _ => {}
        }

        if is_test_like(item, &region_text) {
            score -= 60;
            why.push("test/example penalty".to_string());
        }
        let representative_line = if let Some(first_match_idx) = subject_line_hits.first() {
            region_lines[*first_match_idx]
        } else {
            region_lines[0]
        };
        if looks_like_string_fixture(representative_line) {
            score -= 25;
            why.push("string-literal penalty".to_string());
        }
        if looks_like_cli_or_example_line(representative_line) {
            score -= 60;
            why.push("cli/example penalty".to_string());
        }

        let match_line_number = if let Some(first_match_idx) = subject_line_hits.first() {
            item.start_line + *first_match_idx
        } else {
            item.start_line
        };
        let familiarity = context
            .map(|ctx| {
                ctx.region_familiarity(
                    &file.relative_path,
                    &item.label,
                    item.start_line,
                    item.end_line,
                )
            })
            .unwrap_or_default();
        let full_region = should_include_full_region(item, args.full_region)
            && !should_prune_region(familiarity);
        let mut context_applied = None;
        let body = if full_region {
            extract_region(lines.as_slice(), item.start_line, item.end_line)
        } else if should_prune_region(familiarity) {
            context_applied = Some("compressed repeated region from harness context".to_string());
            region_lines[0].to_string()
        } else {
            lines[match_line_number - 1].to_string()
        };

        regions.push(SmartRegion {
            kind,
            label: item.label.clone(),
            start_line: item.start_line,
            end_line: item.end_line,
            line_count: item.line_count,
            score,
            body,
            full_region,
            why,
            context_applied,
        });
    }

    regions
}

fn should_prune_region(familiarity: Familiarity) -> bool {
    familiarity.prune_confidence >= 0.7
        && familiarity.body_confidence >= 0.7
        && familiarity.current_version_confidence >= 0.6
}

fn structure_budget_for_file(familiarity: Familiarity) -> usize {
    if familiarity.focused
        && familiarity.structure_confidence >= 0.8
        && familiarity.prune_confidence >= 0.7
    {
        4
    } else if familiarity.structure_confidence >= 0.8
        && familiarity.current_version_confidence >= 0.6
        && familiarity.prune_confidence >= 0.7
    {
        6
    } else {
        10
    }
}

fn context_note_for_file(familiarity: Familiarity) -> Option<String> {
    if familiarity.focused
        && familiarity.structure_confidence >= 0.8
        && familiarity.prune_confidence >= 0.7
    {
        Some("compressed file structure from harness context".to_string())
    } else if familiarity.structure_confidence >= 0.8
        && familiarity.current_version_confidence >= 0.6
        && familiarity.prune_confidence >= 0.7
    {
        Some("reduced repeated structure from harness context".to_string())
    } else {
        None
    }
}

fn classify_region(item: &StructureItem, relation: &Relation, exact_label_match: bool) -> String {
    match relation {
        Relation::Rendered => "render-site".to_string(),
        Relation::Handled => "handler".to_string(),
        Relation::Populated => "assignment".to_string(),
        Relation::CalledFrom => "callsite".to_string(),
        Relation::Defined | Relation::Implementation => {
            if exact_label_match {
                "definition".to_string()
            } else {
                "reference".to_string()
            }
        }
        _ if item.kind == "function" => "reference".to_string(),
        _ => item.kind.clone(),
    }
}

fn exact_subject_path_match(relative_lower: &str, subject_lower: &str) -> bool {
    relative_lower.contains(&subject_lower.replace(' ', "_"))
        || relative_lower.contains(&subject_lower.replace(' ', "-"))
}

fn exact_subject_label_match(label: &str, subject_lower: &str) -> bool {
    let label_lower = label.to_ascii_lowercase();
    label_lower == subject_lower
        || label_lower == subject_lower.replace(' ', "_")
        || label_lower == subject_lower.replace(' ', "-")
}

fn tokenize_subject(subject: &str) -> Vec<String> {
    normalize_match_text(subject)
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

fn subject_tokens_match_label(label: &str, subject_tokens: &[String]) -> bool {
    if subject_tokens.is_empty() {
        return false;
    }
    let normalized_label = normalize_match_text(label);
    subject_tokens
        .iter()
        .all(|token| normalized_label.contains(token.as_str()))
}

fn normalize_match_text(text: &str) -> String {
    let mut out = String::new();
    let mut prev_is_lower = false;
    for ch in text.chars() {
        if ch.is_ascii_uppercase() && prev_is_lower {
            out.push(' ');
        }
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            prev_is_lower = ch.is_ascii_lowercase();
        } else {
            out.push(' ');
            prev_is_lower = false;
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn relation_terms(relation: &Relation) -> Vec<String> {
    match relation {
        Relation::Rendered => vec!["render", "draw", "ui", "widget", "view"],
        Relation::CalledFrom => vec!["call", "invoke", "dispatch"],
        Relation::TriggeredFrom => vec!["trigger", "dispatch", "schedule"],
        Relation::Populated => vec!["set", "assign", "insert", "push", "build"],
        Relation::ComesFrom => vec!["source", "load", "parse", "read", "fetch"],
        Relation::Handled => vec!["handle", "handler", "event", "dispatch"],
        Relation::Defined => vec!["fn", "struct", "enum", "class", "def"],
        Relation::Implementation => vec!["impl", "register", "wire", "tool"],
        Relation::Custom(value) => vec![value.as_str()],
    }
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn role_aligns(role: &str, relation: &Relation) -> bool {
    match relation {
        Relation::Rendered => role == "ui",
        Relation::Handled => role == "handler",
        Relation::ComesFrom => role == "provider" || role == "config",
        Relation::Implementation => role == "implementation" || role == "provider",
        _ => false,
    }
}

fn should_filter_kind(kind: Option<&str>, role: &str) -> bool {
    match kind {
        Some("code") => role == "docs",
        Some("docs") => role != "docs",
        Some("tests") => role != "test",
        _ => false,
    }
}

fn select_structure_items(
    items: &[StructureItem],
    regions: &[SmartRegion],
    max_items: usize,
) -> Vec<StructureItem> {
    let mut selected = Vec::new();
    for region in regions {
        if let Some(item) = items.iter().find(|item| {
            item.label == region.label
                && item.start_line == region.start_line
                && item.end_line == region.end_line
        }) && !selected.iter().any(|existing: &StructureItem| {
            existing.label == item.label
                && existing.start_line == item.start_line
                && existing.end_line == item.end_line
        }) {
            selected.push(item.clone());
        }
    }

    for item in items {
        if selected.len() >= max_items {
            break;
        }
        if !selected.iter().any(|existing| {
            existing.label == item.label
                && existing.start_line == item.start_line
                && existing.end_line == item.end_line
        }) {
            selected.push(item.clone());
        }
    }

    selected
}

fn find_lines(text: &str, needle: &str) -> Vec<usize> {
    text.lines()
        .enumerate()
        .filter_map(|(idx, line)| {
            line.to_ascii_lowercase()
                .contains(needle)
                .then_some(idx + 1)
        })
        .collect()
}

fn should_include_full_region(item: &StructureItem, mode: FullRegionMode) -> bool {
    match mode {
        FullRegionMode::Always => true,
        FullRegionMode::Never => false,
        FullRegionMode::Auto => item.line_count <= 20,
    }
}

fn extract_region(lines: &[&str], start_line: usize, end_line: usize) -> String {
    lines[start_line.saturating_sub(1)..end_line.min(lines.len())].join("\n")
}

fn is_test_like(item: &StructureItem, region_text: &str) -> bool {
    let label = item.label.to_ascii_lowercase();
    label.contains("test")
        || region_text.contains("#[test]")
        || region_text.contains("assert_eq!")
        || region_text.contains("unwrap_err()")
}

fn looks_like_string_fixture(line: &str) -> bool {
    let trimmed = line.trim();
    let quote_count = trimmed.matches('"').count();
    quote_count >= 2
        && (trimmed.contains("\\n")
            || trimmed.contains("subject:")
            || trimmed.contains("relation:"))
}

fn looks_like_cli_or_example_line(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.contains("agentgrep ")
        || trimmed.contains("cargo run --")
        || trimmed.contains("subject:")
        || trimmed.contains("relation:")
        || trimmed.contains("support:")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{FullRegionMode, SmartArgs};
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn smart_mode_returns_ranked_files_and_regions() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/tui")).unwrap();
        fs::create_dir_all(dir.path().join("src/auth")).unwrap();
        fs::create_dir_all(dir.path().join("docs")).unwrap();
        fs::write(
            dir.path().join("src/tui/app.rs"),
            "fn render_status_bar() {\n    let status = auth_status();\n    println!(\"{}\", status);\n}\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("src/auth/mod.rs"),
            "pub fn auth_status() -> &'static str {\n    \"ok\"\n}\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("docs/notes.md"),
            "# Notes\nwhere is auth_status rendered\nsubject:auth_status relation:rendered\n",
        )
        .unwrap();

        let query = SmartQuery {
            subject: "auth_status".to_string(),
            relation: Relation::Rendered,
            support: vec!["ui".to_string()],
            kind: None,
            path_hint: None,
        };
        let args = SmartArgs {
            terms: vec![],
            json: false,
            max_files: 5,
            max_regions: 5,
            full_region: FullRegionMode::Auto,
            debug_plan: false,
            debug_score: false,
            paths_only: false,
            path: None,
            file_type: None,
            glob: None,
            hidden: false,
            no_ignore: false,
            context_json: None,
        };

        let result = run_smart(dir.path(), &query, &args).unwrap();
        assert!(!result.files.is_empty());
        assert_eq!(result.files[0].path, "src/tui/app.rs");
        assert!(!result.files[0].regions.is_empty());
        assert_eq!(result.files[0].regions[0].kind, "render-site");
        assert!(
            result
                .files
                .iter()
                .all(|file| file.path != "docs/notes.md" || file.score < result.files[0].score)
        );
    }

    #[test]
    fn smart_kind_code_filters_out_docs() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/tui")).unwrap();
        fs::create_dir_all(dir.path().join("docs")).unwrap();
        fs::write(
            dir.path().join("src/tui/app.rs"),
            "fn render_status_bar() {\n    let status = auth_status();\n}\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("docs/notes.md"),
            "where is auth_status rendered\nsubject:auth_status relation:rendered\n",
        )
        .unwrap();

        let query = SmartQuery {
            subject: "auth_status".to_string(),
            relation: Relation::Rendered,
            support: vec![],
            kind: Some("code".to_string()),
            path_hint: None,
        };
        let args = SmartArgs {
            terms: vec![],
            json: false,
            max_files: 5,
            max_regions: 5,
            full_region: FullRegionMode::Auto,
            debug_plan: false,
            debug_score: false,
            paths_only: false,
            path: None,
            file_type: None,
            glob: None,
            hidden: false,
            no_ignore: false,
            context_json: None,
        };

        let result = run_smart(dir.path(), &query, &args).unwrap();
        assert!(result.files.iter().all(|f| !f.path.ends_with(".md")));
    }

    #[test]
    fn smart_path_hint_biases_subtree() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/tui")).unwrap();
        fs::create_dir_all(dir.path().join("src/other")).unwrap();
        fs::write(
            dir.path().join("src/tui/app.rs"),
            "fn render_status_bar() {\n    let status = auth_status();\n}\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("src/other/app.rs"),
            "fn render_status_bar() {\n    let status = auth_status();\n}\n",
        )
        .unwrap();

        let query = SmartQuery {
            subject: "auth_status".to_string(),
            relation: Relation::Rendered,
            support: vec![],
            kind: Some("code".to_string()),
            path_hint: Some("src/tui".to_string()),
        };
        let args = SmartArgs {
            terms: vec![],
            json: false,
            max_files: 5,
            max_regions: 5,
            full_region: FullRegionMode::Auto,
            debug_plan: false,
            debug_score: false,
            paths_only: false,
            path: None,
            file_type: None,
            glob: None,
            hidden: false,
            no_ignore: false,
            context_json: None,
        };

        let result = run_smart(dir.path(), &query, &args).unwrap();
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].path, "src/tui/app.rs");
    }

    #[test]
    fn smart_penalizes_cli_example_files() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/tui")).unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/tui/app.rs"),
            "fn render_status_bar() {\n    let status = auth_status();\n}\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("src/main.rs"),
            "fn main() {\n    eprintln!(\"agentgrep trace subject:auth_status relation:rendered support:ui\");\n}\n",
        )
        .unwrap();

        let query = SmartQuery {
            subject: "auth_status".to_string(),
            relation: Relation::Rendered,
            support: vec!["ui".to_string()],
            kind: Some("code".to_string()),
            path_hint: None,
        };
        let args = SmartArgs {
            terms: vec![],
            json: false,
            max_files: 5,
            max_regions: 5,
            full_region: FullRegionMode::Auto,
            debug_plan: false,
            debug_score: false,
            paths_only: false,
            path: None,
            file_type: None,
            glob: None,
            hidden: false,
            no_ignore: false,
            context_json: None,
        };

        let result = run_smart(dir.path(), &query, &args).unwrap();
        assert_eq!(result.files[0].path, "src/tui/app.rs");
    }

    #[test]
    fn implementation_query_prefers_owner_symbols() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/tool")).unwrap();
        fs::write(
            dir.path().join("src/tool/lsp.rs"),
            "pub struct LspTool;\nimpl LspTool {}\nfn description() -> &'static str { \"LSP\" }\n",
        )
        .unwrap();

        let query = SmartQuery {
            subject: "lsp".to_string(),
            relation: Relation::Implementation,
            support: vec![],
            kind: Some("code".to_string()),
            path_hint: Some("src/tool".to_string()),
        };
        let args = SmartArgs {
            terms: vec![],
            json: false,
            max_files: 5,
            max_regions: 5,
            full_region: FullRegionMode::Auto,
            debug_plan: false,
            debug_score: false,
            paths_only: false,
            path: None,
            file_type: None,
            glob: None,
            hidden: false,
            no_ignore: false,
            context_json: None,
        };

        let result = run_smart(dir.path(), &query, &args).unwrap();
        assert_eq!(result.files[0].path, "src/tool/lsp.rs");
        assert!(matches!(
            result.files[0].regions[0].kind.as_str(),
            "reference" | "definition"
        ));
        assert!(result.files[0].regions[0].score >= result.files[0].regions[1].score);
    }

    #[test]
    fn trace_context_can_compress_repeated_regions() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src/tool")).unwrap();
        fs::write(
            dir.path().join("src/tool/lsp.rs"),
            "pub struct LspTool;\nimpl LspTool {}\nfn execute() {\n    let lsp = true;\n    println!(\"{}\", lsp);\n}\n",
        )
        .unwrap();
        let context_path = dir.path().join("context.json");
        fs::write(
            &context_path,
            r#"{
  "known_files": [
    {
      "path": "src/tool/lsp.rs",
      "structure_confidence": 0.95,
      "current_version_confidence": 0.9,
      "prune_confidence": 0.85
    }
  ],
  "known_regions": [
    {
      "path": "src/tool/lsp.rs",
      "start_line": 3,
      "end_line": 6,
      "body_confidence": 0.95,
      "current_version_confidence": 0.9,
      "prune_confidence": 0.9
    }
  ]
}"#,
        )
        .unwrap();

        let query = SmartQuery {
            subject: "lsp".to_string(),
            relation: Relation::Implementation,
            support: vec![],
            kind: Some("code".to_string()),
            path_hint: Some("src/tool".to_string()),
        };
        let args = SmartArgs {
            terms: vec![],
            json: false,
            max_files: 5,
            max_regions: 5,
            full_region: FullRegionMode::Auto,
            debug_plan: false,
            debug_score: false,
            paths_only: false,
            path: None,
            file_type: None,
            glob: None,
            hidden: false,
            no_ignore: false,
            context_json: Some(context_path.display().to_string()),
        };

        let result = run_smart(dir.path(), &query, &args).unwrap();
        assert!(result.files[0].context_applied.is_some());
        assert!(result.files[0]
            .regions
            .iter()
            .any(|region| region.context_applied.is_some()));
    }
}
