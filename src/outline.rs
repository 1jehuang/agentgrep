use crate::cli::OutlineArgs;
use crate::context::HarnessContext;
use crate::structure::{StructureItem, extract_file_structure};
use crate::workspace::{normalize_display_path, read_text_file};
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
        return Err(format!("file not found: {}", file_path.display()));
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
            (8, Some("compressed repeated outline from harness context".to_string()))
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
        assert!(result.structure.items.iter().any(|item| item.label == "App"));
        assert!(result
            .structure
            .items
            .iter()
            .any(|item| item.label == "render_status_bar"));
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
