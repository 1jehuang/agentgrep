use crate::cli::OutlineArgs;
use crate::context::HarnessContext;
use crate::structure::{StructureItem, extract_file_structure};
use crate::workspace::{SearchScope, collect_file_entries, normalize_display_path, read_text_file};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct OutlineResult {
    pub root: String,
    pub path: String,
    pub language: String,
    pub role: String,
    pub total_lines: usize,
    pub structure: OutlineStructure,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_applied: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutlineStructure {
    pub items: Vec<StructureItem>,
    pub omitted_count: usize,
}

pub fn run_outline(root: &Path, args: &OutlineArgs) -> Result<OutlineResult, String> {
    let file_path = resolve_outline_path(root, &args.file);
    if !file_path.exists() {
        return Err(file_not_found_error(root, &args.file, &file_path));
    }
    if !file_path.is_file() {
        return Err(format!("not a file: {}", file_path.display()));
    }

    let text = read_text_file(&file_path)
        .ok_or_else(|| format!("file is binary or unreadable: {}", file_path.display()))?;
    let display_path = normalize_display_path(root, &file_path);
    let structure = extract_file_structure(&file_path, &display_path, &text);
    let total_lines = text.lines().count().max(1);
    let context = HarnessContext::load(args.context_json.as_deref())?;

    let (max_items, context_applied) = if let Some(max_items) = args.max_items {
        (max_items, None)
    } else if let Some(context) = &context {
        let familiarity = context.file_familiarity(&display_path);
        if familiarity.structure_confidence >= 0.8
            && familiarity.current_version_confidence >= 0.6
            && familiarity.prune_confidence >= 0.7
        {
            (
                8,
                Some("compressed repeated outline from harness context".to_string()),
            )
        } else {
            (usize::MAX, None)
        }
    } else {
        (usize::MAX, None)
    };
    let shown_items = structure
        .items
        .iter()
        .take(max_items)
        .cloned()
        .collect::<Vec<_>>();
    let omitted_count = structure.items.len().saturating_sub(shown_items.len());

    Ok(OutlineResult {
        root: root.display().to_string(),
        path: display_path,
        language: structure.language,
        role: structure.role,
        total_lines,
        structure: OutlineStructure {
            items: shown_items,
            omitted_count,
        },
        context_applied,
    })
}

fn resolve_outline_path(root: &Path, file: &str) -> PathBuf {
    let path = Path::new(file);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    }
}

fn file_not_found_error(root: &Path, requested: &str, resolved: &PathBuf) -> String {
    let mut message = format!("file not found: {}", resolved.display());
    let suggestions = suggest_similar_files(root, requested);
    if !suggestions.is_empty() {
        message.push_str("\n\ndid you mean:");
        for suggestion in suggestions {
            message.push_str("\n  ");
            message.push_str(&suggestion);
        }
    }
    message
}

/// Find workspace files whose name matches the requested file name so a
/// mistyped or moved path yields actionable suggestions instead of a dead end.
fn suggest_similar_files(root: &Path, requested: &str) -> Vec<String> {
    const MAX_SUGGESTIONS: usize = 5;

    let requested_name = Path::new(requested)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    let Some(requested_name) = requested_name else {
        return Vec::new();
    };
    let requested_stem = Path::new(&requested_name)
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_ascii_lowercase());

    let scope = SearchScope {
        root,
        file_type: None,
        glob: None,
        hidden: false,
        no_ignore: false,
        follow: true,
    };

    let mut exact = Vec::new();
    let mut stem_matches = Vec::new();
    for entry in collect_file_entries(&scope) {
        let Some(name) = entry.path.file_name().map(|n| n.to_string_lossy()) else {
            continue;
        };
        if name == requested_name.as_str() {
            exact.push(entry.relative_path.clone());
        } else if let Some(requested_stem) = requested_stem.as_deref() {
            let stem = Path::new(name.as_ref())
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_ascii_lowercase());
            if stem.as_deref() == Some(requested_stem) {
                stem_matches.push(entry.relative_path.clone());
            }
        }
    }

    // Sort before truncation so suggestions are deterministic regardless of
    // filesystem directory-entry (readdir) order.
    exact.sort();
    stem_matches.sort();
    exact.extend(stem_matches);
    exact.truncate(MAX_SUGGESTIONS);
    exact
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::OutlineArgs;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn outline_returns_file_structure() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/app.rs"),
            "pub struct App {}\n\nimpl App {}\n\npub fn render_status_bar() {}\n",
        )
        .unwrap();

        let args = OutlineArgs {
            file: "src/app.rs".to_string(),
            json: false,
            max_items: None,
            path: None,
            context_json: None,
        };

        let result = run_outline(dir.path(), &args).unwrap();
        assert_eq!(result.path, "src/app.rs");
        assert_eq!(result.language, "rust");
        assert_eq!(result.role, "implementation");
        assert!(
            result
                .structure
                .items
                .iter()
                .any(|item| item.label == "App")
        );
        assert!(
            result
                .structure
                .items
                .iter()
                .any(|item| item.label == "render_status_bar")
        );
    }

    #[test]
    fn outline_missing_file_suggests_similar_paths() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("crates/core/src")).unwrap();
        fs::write(dir.path().join("crates/core/src/dag.rs"), "fn one() {}\n").unwrap();

        let args = OutlineArgs {
            file: "crates/plan/src/dag.rs".to_string(),
            json: false,
            max_items: None,
            path: None,
            context_json: None,
        };

        let err = run_outline(dir.path(), &args).unwrap_err();
        assert!(err.contains("file not found"), "unexpected error: {err}");
        assert!(err.contains("did you mean"), "missing suggestions: {err}");
        assert!(
            err.contains("crates/core/src/dag.rs"),
            "missing suggested path: {err}"
        );
    }

    #[test]
    fn outline_can_truncate_items() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/app.rs"),
            "fn one() {}\nfn two() {}\nfn three() {}\n",
        )
        .unwrap();

        let args = OutlineArgs {
            file: "src/app.rs".to_string(),
            json: false,
            max_items: Some(2),
            path: None,
            context_json: None,
        };

        let result = run_outline(dir.path(), &args).unwrap();
        assert_eq!(result.structure.items.len(), 2);
        assert_eq!(result.structure.omitted_count, 1);
    }

    #[test]
    fn outline_uses_context_to_compress_structure() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/app.rs"),
            "fn one() {}\nfn two() {}\nfn three() {}\nfn four() {}\nfn five() {}\nfn six() {}\nfn seven() {}\nfn eight() {}\nfn nine() {}\n",
        )
        .unwrap();
        let context_path = dir.path().join("context.json");
        fs::write(
            &context_path,
            r#"{
  "known_files": [
    {
      "path": "src/app.rs",
      "structure_confidence": 0.95,
      "current_version_confidence": 0.9,
      "prune_confidence": 0.85
    }
  ]
}"#,
        )
        .unwrap();

        let args = OutlineArgs {
            file: "src/app.rs".to_string(),
            json: false,
            max_items: None,
            path: None,
            context_json: Some(context_path.display().to_string()),
        };

        let result = run_outline(dir.path(), &args).unwrap();
        assert_eq!(result.structure.items.len(), 8);
        assert_eq!(result.structure.omitted_count, 1);
        assert!(result.context_applied.is_some());
    }
}
