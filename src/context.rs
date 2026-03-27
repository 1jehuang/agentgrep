use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HarnessContext {
    #[serde(default)]
    pub version: Option<u32>,
    #[serde(default)]
    pub known_regions: Vec<KnownRegion>,
    #[serde(default)]
    pub known_files: Vec<KnownFile>,
    #[serde(default)]
    pub known_symbols: Vec<KnownSymbol>,
    #[serde(default)]
    pub focus_files: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KnownFile {
    pub path: String,
    #[serde(default)]
    pub structure_confidence: Option<f32>,
    #[serde(default)]
    pub body_confidence: Option<f32>,
    #[serde(default)]
    pub current_version_confidence: Option<f32>,
    #[serde(default)]
    pub prune_confidence: Option<f32>,
    #[serde(default)]
    pub source_strength: Option<String>,
    #[serde(default)]
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KnownRegion {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    #[serde(default)]
    pub structure_confidence: Option<f32>,
    #[serde(default)]
    pub body_confidence: Option<f32>,
    #[serde(default)]
    pub current_version_confidence: Option<f32>,
    #[serde(default)]
    pub prune_confidence: Option<f32>,
    #[serde(default)]
    pub source_strength: Option<String>,
    #[serde(default)]
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KnownSymbol {
    pub path: String,
    pub symbol: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub structure_confidence: Option<f32>,
    #[serde(default)]
    pub body_confidence: Option<f32>,
    #[serde(default)]
    pub current_version_confidence: Option<f32>,
    #[serde(default)]
    pub prune_confidence: Option<f32>,
    #[serde(default)]
    pub source_strength: Option<String>,
    #[serde(default)]
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Familiarity {
    pub structure_confidence: f32,
    pub body_confidence: f32,
    pub current_version_confidence: f32,
    pub prune_confidence: f32,
    pub focused: bool,
}

impl HarnessContext {
    pub fn load(path: Option<&str>) -> Result<Option<Self>, String> {
        let Some(path) = path else {
            return Ok(None);
        };
        let data = std::fs::read_to_string(path)
            .map_err(|err| format!("failed to read context file {}: {}", path, err))?;
        let context = serde_json::from_str(&data)
            .map_err(|err| format!("failed to parse context file {}: {}", path, err))?;
        Ok(Some(context))
    }

    pub fn file_familiarity(&self, path: &str) -> Familiarity {
        let mut familiarity = Familiarity::default();
        for known in &self.known_files {
            if same_path(&known.path, path) {
                merge_file_into(&mut familiarity, known);
            }
        }
        familiarity.focused = self.focus_files.iter().any(|focus| same_path(focus, path));
        familiarity
    }

    pub fn symbol_familiarity(&self, path: &str, symbol: &str) -> Familiarity {
        let mut familiarity = self.file_familiarity(path);
        for known in &self.known_symbols {
            if same_path(&known.path, path) && known.symbol == symbol {
                merge_symbol_into(&mut familiarity, known);
            }
        }
        familiarity
    }

    pub fn region_familiarity(
        &self,
        path: &str,
        symbol: &str,
        start_line: usize,
        end_line: usize,
    ) -> Familiarity {
        let mut familiarity = self.symbol_familiarity(path, symbol);
        for known in &self.known_regions {
            if same_path(&known.path, path)
                && ranges_overlap(start_line, end_line, known.start_line, known.end_line)
            {
                merge_region_into(&mut familiarity, known);
            }
        }
        familiarity
    }
}

fn same_path(left: &str, right: &str) -> bool {
    normalize_path(left) == normalize_path(right)
}

fn normalize_path(path: &str) -> String {
    Path::new(path)
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn ranges_overlap(a_start: usize, a_end: usize, b_start: usize, b_end: usize) -> bool {
    a_start <= b_end && b_start <= a_end
}

fn clamp_confidence(value: Option<f32>) -> f32 {
    value.unwrap_or(0.0).clamp(0.0, 1.0)
}

fn merge_file_into(target: &mut Familiarity, known: &KnownFile) {
    target.structure_confidence = target
        .structure_confidence
        .max(clamp_confidence(known.structure_confidence));
    target.body_confidence = target.body_confidence.max(clamp_confidence(known.body_confidence));
    target.current_version_confidence = target
        .current_version_confidence
        .max(clamp_confidence(known.current_version_confidence));
    target.prune_confidence = target
        .prune_confidence
        .max(clamp_confidence(known.prune_confidence));
}

fn merge_region_into(target: &mut Familiarity, known: &KnownRegion) {
    target.structure_confidence = target
        .structure_confidence
        .max(clamp_confidence(known.structure_confidence));
    target.body_confidence = target.body_confidence.max(clamp_confidence(known.body_confidence));
    target.current_version_confidence = target
        .current_version_confidence
        .max(clamp_confidence(known.current_version_confidence));
    target.prune_confidence = target
        .prune_confidence
        .max(clamp_confidence(known.prune_confidence));
}

fn merge_symbol_into(target: &mut Familiarity, known: &KnownSymbol) {
    target.structure_confidence = target
        .structure_confidence
        .max(clamp_confidence(known.structure_confidence));
    target.body_confidence = target.body_confidence.max(clamp_confidence(known.body_confidence));
    target.current_version_confidence = target
        .current_version_confidence
        .max(clamp_confidence(known.current_version_confidence));
    target.prune_confidence = target
        .prune_confidence
        .max(clamp_confidence(known.prune_confidence));
}

