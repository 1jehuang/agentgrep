use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
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
    #[serde(skip)]
    file_index: HashMap<String, Familiarity>,
    #[serde(skip)]
    symbol_index: HashMap<(String, String), Familiarity>,
    #[serde(skip)]
    region_index: HashMap<String, Vec<IndexedRegion>>,
    #[serde(skip)]
    focus_index: HashSet<String>,
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

#[derive(Debug, Clone)]
struct IndexedRegion {
    start_line: usize,
    end_line: usize,
    familiarity: Familiarity,
}

impl HarnessContext {
    pub fn load(path: Option<&str>) -> Result<Option<Self>, String> {
        let Some(path) = path else {
            return Ok(None);
        };
        let data = std::fs::read_to_string(path)
            .map_err(|err| format!("failed to read context file {}: {}", path, err))?;
        let mut context: Self = serde_json::from_str(&data)
            .map_err(|err| format!("failed to parse context file {}: {}", path, err))?;
        context.rebuild_indexes();
        Ok(Some(context))
    }

    pub fn file_familiarity(&self, path: &str) -> Familiarity {
        let normalized_path = normalize_path(path);
        let mut familiarity = self
            .file_index
            .get(&normalized_path)
            .copied()
            .unwrap_or_default();
        familiarity.focused = self.focus_index.contains(&normalized_path);
        familiarity
    }

    pub fn symbol_familiarity(&self, path: &str, symbol: &str) -> Familiarity {
        let normalized_path = normalize_path(path);
        let mut familiarity = self
            .file_index
            .get(&normalized_path)
            .copied()
            .unwrap_or_default();
        familiarity.focused = self.focus_index.contains(&normalized_path);
        if let Some(symbol_familiarity) = self
            .symbol_index
            .get(&(normalized_path, symbol.to_string()))
            .copied()
        {
            merge_familiarity(&mut familiarity, symbol_familiarity);
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
        let normalized_path = normalize_path(path);
        let mut familiarity = self.symbol_familiarity(&normalized_path, symbol);
        if let Some(regions) = self.region_index.get(&normalized_path) {
            for known in regions {
                if ranges_overlap(start_line, end_line, known.start_line, known.end_line) {
                    merge_familiarity(&mut familiarity, known.familiarity);
                }
            }
        }
        familiarity
    }

    fn rebuild_indexes(&mut self) {
        self.file_index.clear();
        self.symbol_index.clear();
        self.region_index.clear();
        self.focus_index.clear();

        for focus in &self.focus_files {
            self.focus_index.insert(normalize_path(focus));
        }

        for known in &self.known_files {
            let normalized_path = normalize_path(&known.path);
            let entry = self.file_index.entry(normalized_path).or_default();
            merge_file_into(entry, known);
        }

        for known in &self.known_symbols {
            let key = (normalize_path(&known.path), known.symbol.clone());
            let entry = self.symbol_index.entry(key).or_default();
            merge_symbol_into(entry, known);
        }

        for known in &self.known_regions {
            self.region_index
                .entry(normalize_path(&known.path))
                .or_default()
                .push(IndexedRegion {
                    start_line: known.start_line,
                    end_line: known.end_line,
                    familiarity: region_familiarity_from(known),
                });
        }
    }
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

fn merge_familiarity(target: &mut Familiarity, other: Familiarity) {
    target.structure_confidence = target.structure_confidence.max(other.structure_confidence);
    target.body_confidence = target.body_confidence.max(other.body_confidence);
    target.current_version_confidence = target
        .current_version_confidence
        .max(other.current_version_confidence);
    target.prune_confidence = target.prune_confidence.max(other.prune_confidence);
    target.focused |= other.focused;
}

fn region_familiarity_from(known: &KnownRegion) -> Familiarity {
    let mut familiarity = Familiarity::default();
    merge_region_into(&mut familiarity, known);
    familiarity
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
