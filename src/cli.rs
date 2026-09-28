use clap::{ArgAction, Parser, Subcommand, ValueEnum};

#[derive(Debug, Clone, Parser)]
#[command(
    name = "agentgrep",
    version,
    about = "CLI-first code search and retrieval for agents"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    /// Exact lexical search.
    Grep(GrepArgs),
    /// Ranked file/path discovery.
    Find(FindArgs),
    /// File structure outline for a known file.
    Outline(OutlineArgs),
    /// Structured trace mode using a small relation-aware DSL.
    #[command(name = "trace", visible_alias = "smart")]
    Trace(SmartArgs),
}

#[derive(Debug, Clone, Parser)]
pub struct GrepArgs {
    /// Exact query to search for.
    pub query: String,

    /// Treat the query as a regular expression.
    #[arg(long)]
    pub regex: bool,

    /// Restrict to a known file type.
    #[arg(long = "type")]
    pub file_type: Option<String>,

    /// Emit JSON output.
    #[arg(long)]
    pub json: bool,

    /// Print only matching file paths.
    #[arg(long)]
    pub paths_only: bool,

    /// Include hidden files.
    #[arg(long)]
    pub hidden: bool,

    /// Ignore .gitignore and related ignore files.
    #[arg(long = "no-ignore")]
    pub no_ignore: bool,

    /// Do not follow symlinks while searching (guardrail for repos that
    /// symlink huge external trees; disables the rg --follow flag and the
    /// native walker's follow_links so both paths stay in parity).
    #[arg(long = "no-follow")]
    pub no_follow: bool,

    /// Stop after this many matches per file.
    ///
    /// Consumers that render only the first N regions per file otherwise pay
    /// for a full unbounded scan: `rg` reports every match and agentgrep parses
    /// all of them before truncation happens at render time. Setting this
    /// pushes the bound into `rg --max-count` (and an equivalent early exit on
    /// the native path), which is where the work actually is.
    ///
    /// This is a per-file cap, so `total_matches` becomes a lower bound when it
    /// is set. Leave it unset for exact counts.
    #[arg(long = "max-count")]
    pub max_matches_per_file: Option<usize>,

    /// Extract per-file structure (symbols, grouping) for at most this many
    /// files, in result order.
    ///
    /// Structure extraction re-reads and parses each matched file, costing
    /// ~26us/file. Consumers that render only a bounded prefix of results pay
    /// that for every match: a query hitting 625 files does 625 extractions
    /// even when the renderer stops after a fraction of them. Files past the
    /// bound still appear with their matches, just without symbol grouping,
    /// exactly like files over the dense-match threshold.
    ///
    /// Results are ordered by display path before this is applied, so the
    /// gated prefix is deterministic and matches what a path-ordered renderer
    /// shows first. Leave unset to extract structure for every matched file.
    #[arg(long = "max-structure-files")]
    pub max_structure_files: Option<usize>,

    /// Optional root path to search instead of the current directory.
    #[arg(long)]
    pub path: Option<String>,

    /// Restrict candidate files by glob.
    #[arg(long)]
    pub glob: Option<String>,
}

#[derive(Debug, Clone, Parser)]
pub struct FindArgs {
    /// File/path-oriented query terms.
    #[arg(required = true)]
    pub query_parts: Vec<String>,

    /// Restrict to a known file type.
    #[arg(long = "type")]
    pub file_type: Option<String>,

    /// Emit JSON output.
    #[arg(long)]
    pub json: bool,

    /// Print only matching file paths.
    #[arg(long)]
    pub paths_only: bool,

    /// Print score information in human-readable output.
    #[arg(long = "debug-score", action = ArgAction::SetTrue)]
    pub debug_score: bool,

    /// Max files to return.
    #[arg(long, default_value_t = 10)]
    pub max_files: usize,

    /// Include hidden files.
    #[arg(long)]
    pub hidden: bool,

    /// Ignore .gitignore and related ignore files.
    #[arg(long = "no-ignore")]
    pub no_ignore: bool,

    /// Do not follow symlinks while searching (guardrail for repos that
    /// symlink huge external trees; disables the rg --follow flag and the
    /// native walker's follow_links so both paths stay in parity).
    #[arg(long = "no-follow")]
    pub no_follow: bool,

    /// Optional root path to search instead of the current directory.
    #[arg(long)]
    pub path: Option<String>,

    /// Restrict candidate files by glob.
    #[arg(long)]
    pub glob: Option<String>,
}

#[derive(Debug, Clone, Parser)]
pub struct OutlineArgs {
    /// File path to outline.
    pub file: String,

    /// Emit JSON output.
    #[arg(long)]
    pub json: bool,

    /// Maximum structure items to print. Defaults to all detected items.
    #[arg(long)]
    pub max_items: Option<usize>,

    /// Optional root path to resolve relative file paths against.
    #[arg(long)]
    pub path: Option<String>,

    /// Optional harness context JSON file.
    #[arg(long = "context-json")]
    pub context_json: Option<String>,
}

#[derive(Debug, Clone, Parser)]
pub struct SmartArgs {
    /// Structured smart query DSL terms, e.g. subject:auth_status relation:rendered.
    #[arg(required = true)]
    pub terms: Vec<String>,

    /// Emit JSON output.
    #[arg(long)]
    pub json: bool,

    /// Max files to return.
    #[arg(long, default_value_t = 5)]
    pub max_files: usize,

    /// Max regions to return per query.
    #[arg(long, default_value_t = 6)]
    pub max_regions: usize,

    /// Preferred region expansion mode.
    #[arg(long, value_enum, default_value_t = FullRegionMode::Auto)]
    pub full_region: FullRegionMode,

    /// Print parser/planner details.
    #[arg(long = "debug-plan", action = ArgAction::SetTrue)]
    pub debug_plan: bool,

    /// Print score information in human-readable output.
    #[arg(long = "debug-score", action = ArgAction::SetTrue)]
    pub debug_score: bool,

    /// Print only matching file paths.
    #[arg(long)]
    pub paths_only: bool,

    /// Optional root path to search instead of the current directory.
    #[arg(long)]
    pub path: Option<String>,

    /// Restrict to a known file type.
    #[arg(long = "type")]
    pub file_type: Option<String>,

    /// Restrict candidate files by glob.
    #[arg(long)]
    pub glob: Option<String>,

    /// Include hidden files.
    #[arg(long)]
    pub hidden: bool,

    /// Ignore .gitignore and related ignore files.
    #[arg(long = "no-ignore")]
    pub no_ignore: bool,

    /// Optional harness context JSON file.
    #[arg(long = "context-json")]
    pub context_json: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FullRegionMode {
    Auto,
    Always,
    Never,
}
