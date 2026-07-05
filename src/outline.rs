use crate::cli::OutlineArgs;
use crate::context::HarnessContext;
use crate::structure::{StructureItem, extract_file_structure};
use crate::workspace::{
    SearchScope, collect_file_entries, disambiguate_display_path, normalize_display_path,
    read_text_file, relative_raw_bytes,
};
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
    let file_path = if file_path.exists() {
        file_path
    } else if let Some(native) = resolve_display_disambiguated_path(root, &args.file) {
        // The request is a disambiguated display path (`a\u{FFFD}.txt#b=ff`)
        // emitted by grep/find/trace for a non-UTF-8 filename; map it back to
        // the native path so those outputs round-trip into outline.
        native
    } else {
        return Err(file_not_found_error(root, &args.file, &file_path));
    };
    if !file_path.is_file() {
        return Err(format!("not a file: {}", file_path.display()));
    }

    let text = read_text_file(&file_path)
        .ok_or_else(|| format!("file is binary or unreadable: {}", file_path.display()))?;
    let display_path = normalize_display_path(root, &file_path);
    let structure = extract_file_structure(&file_path, &display_path, &text);
    // Emit the disambiguated form so non-UTF-8 outline paths stay unique and
    // round-trippable, matching grep/find/trace output.
    let output_path = disambiguate_display_path(
        &display_path,
        relative_raw_bytes(root, &file_path).as_deref(),
    );
    let total_lines = text.lines().count().max(1);
    let context = HarnessContext::load(args.context_json.as_deref())?;

    let (max_items, context_applied) = if let Some(max_items) = args.max_items {
        (max_items, None)
    } else if let Some(context) = &context {
        let familiarity = context.file_familiarity(&output_path);
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
        path: output_path,
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

/// Resolve a display-disambiguated path (one containing U+FFFD, optionally
/// with the `#b=` byte suffix appended by `disambiguate_display_path`) back
/// to the native path of the file it names. Returns None when the request
/// does not look like a disambiguated path or no workspace file matches.
fn resolve_display_disambiguated_path(root: &Path, requested: &str) -> Option<PathBuf> {
    if !requested.contains('\u{FFFD}') {
        return None;
    }
    let scope = SearchScope {
        root,
        file_type: None,
        glob: None,
        hidden: true,
        no_ignore: false,
    };
    let mut matched = None;
    for entry in collect_file_entries(&scope) {
        // Accept both the full disambiguated form and the bare lossy form
        // (the latter only when it is unambiguous).
        if entry.display_path() == requested || entry.relative_path == requested {
            if matched.is_some() {
                // Ambiguous bare lossy path: refuse to guess.
                return None;
            }
            matched = Some(entry.path);
        }
    }
    matched
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
    };

    let mut exact = Vec::new();
    let mut stem_matches = Vec::new();
    for entry in collect_file_entries(&scope) {
        let Some(name) = entry.path.file_name().map(|n| n.to_string_lossy()) else {
            continue;
        };
        if name == requested_name.as_str() {
            exact.push(entry.display_path());
        } else if let Some(requested_stem) = requested_stem.as_deref() {
            let stem = Path::new(name.as_ref())
                .file_stem()
                .map(|stem| stem.to_string_lossy().to_ascii_lowercase());
            if stem.as_deref() == Some(requested_stem) {
                stem_matches.push(entry.display_path());
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

    /// Create `files` (relative paths) under `root` in the given order so we
    /// can build corpora whose on-disk creation order differs.
    fn create_files_in_order(root: &Path, files: &[&str]) {
        for file in files {
            let path = root.join(file);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(path, "fn placeholder() {}\n").unwrap();
        }
    }

    fn assert_lexicographically_sorted(suggestions: &[String]) {
        let mut sorted = suggestions.to_vec();
        sorted.sort();
        assert_eq!(
            suggestions,
            &sorted[..],
            "suggestions are not lexicographically sorted: {suggestions:?}"
        );
    }

    #[test]
    fn suggestions_deterministic_across_directory_creation_orders() {
        // More exact matches (7) than MAX_SUGGESTIONS (5) so truncation is
        // exercised: a nondeterministic readdir order would surface a
        // different subset without the pre-truncation sort.
        let files = [
            "alpha/config.rs",
            "bravo/config.rs",
            "charlie/config.rs",
            "delta/config.rs",
            "echo/config.rs",
            "foxtrot/config.rs",
            "golf/config.rs",
        ];

        // Forward creation order.
        let forward = tempdir().unwrap();
        create_files_in_order(forward.path(), &files);

        // Opposite creation order.
        let reversed: Vec<&str> = files.iter().rev().copied().collect();
        let backward = tempdir().unwrap();
        create_files_in_order(backward.path(), &reversed);

        // Shuffled creation order.
        let shuffled = [
            "delta/config.rs",
            "alpha/config.rs",
            "golf/config.rs",
            "bravo/config.rs",
            "foxtrot/config.rs",
            "charlie/config.rs",
            "echo/config.rs",
        ];
        let mixed = tempdir().unwrap();
        create_files_in_order(mixed.path(), &shuffled);

        let from_forward = suggest_similar_files(forward.path(), "missing/config.rs");
        let from_backward = suggest_similar_files(backward.path(), "missing/config.rs");
        let from_mixed = suggest_similar_files(mixed.path(), "missing/config.rs");

        let expected = vec![
            "alpha/config.rs".to_string(),
            "bravo/config.rs".to_string(),
            "charlie/config.rs".to_string(),
            "delta/config.rs".to_string(),
            "echo/config.rs".to_string(),
        ];
        assert_eq!(from_forward, expected, "content/order mismatch (forward)");
        assert_eq!(
            from_forward, from_backward,
            "suggestions differ between opposite creation orders"
        );
        assert_eq!(
            from_forward, from_mixed,
            "suggestions differ between shuffled creation orders"
        );
        assert_lexicographically_sorted(&from_forward);
    }

    #[test]
    fn suggestions_interleave_exact_before_stem_and_truncate() {
        // Exact-name matches must all rank before stem-only matches, and the
        // combined list must truncate to MAX_SUGGESTIONS (5) with each group
        // lexicographically sorted. 3 exact + 3 stem = 6 candidates, so the
        // lexicographically last stem match (util.ts) is dropped.
        let files = [
            "web/util.js",
            "api/util.js",
            "cli/util.js",
            "util.go",
            "util.ts",
            "util.py",
        ];

        let forward = tempdir().unwrap();
        create_files_in_order(forward.path(), &files);

        let reversed: Vec<&str> = files.iter().rev().copied().collect();
        let backward = tempdir().unwrap();
        create_files_in_order(backward.path(), &reversed);

        let from_forward = suggest_similar_files(forward.path(), "lib/util.js");
        let from_backward = suggest_similar_files(backward.path(), "lib/util.js");

        let expected = vec![
            // Exact matches first, sorted.
            "api/util.js".to_string(),
            "cli/util.js".to_string(),
            "web/util.js".to_string(),
            // Then stem matches, sorted, truncated at 5 total.
            "util.go".to_string(),
            "util.py".to_string(),
        ];
        assert_eq!(from_forward, expected, "content/order mismatch (forward)");
        assert_eq!(
            from_forward, from_backward,
            "suggestions differ between opposite creation orders"
        );
        assert!(
            !from_forward.contains(&"util.ts".to_string()),
            "truncation should drop the lexicographically last stem match"
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
