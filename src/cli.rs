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
    /// Structured investigation mode using a small DSL.
    Smart(SmartArgs),
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

    /// Include hidden files.
    #[arg(long)]
    pub hidden: bool,

    /// Ignore .gitignore and related ignore files.
    #[arg(long = "no-ignore")]
    pub no_ignore: bool,

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

    /// Max files to return.
    #[arg(long, default_value_t = 10)]
    pub max_files: usize,

    /// Include hidden files.
    #[arg(long)]
    pub hidden: bool,

    /// Ignore .gitignore and related ignore files.
    #[arg(long = "no-ignore")]
    pub no_ignore: bool,

    /// Optional root path to search instead of the current directory.
    #[arg(long)]
    pub path: Option<String>,

    /// Restrict candidate files by glob.
    #[arg(long)]
    pub glob: Option<String>,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FullRegionMode {
    Auto,
    Always,
    Never,
}
