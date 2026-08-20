use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

/// TempCheq — inference-temperature auditor and optimiser.
///
/// Walks a workspace, finds every place a sampling temperature is set
/// (or should be), and checks it against what the surrounding inference
/// action actually calls for.
#[derive(Parser, Debug)]
#[command(name = "tempcheq", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Workspace root to audit. Ignored when a subcommand is given.
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Show the reasoning behind each recommendation.
    #[arg(long)]
    pub explain: bool,

    /// Perturb candidate temperatures and show a simulated score curve
    /// per action (see the printed disclaimer: this is not a live call).
    #[arg(long)]
    pub benchmark: bool,

    /// Rewrite source files for high-confidence deviations. Prints the
    /// plan and exits unless --yes is also passed.
    #[arg(long)]
    pub fix: bool,

    /// Actually apply --fix changes instead of dry-running them.
    #[arg(long)]
    pub yes: bool,

    /// Minimum recommendation confidence required for --fix to touch a
    /// line.
    #[arg(long, default_value_t = 0.75)]
    pub fix_confidence: f32,

    /// Re-run the audit whenever a file under `path` changes.
    #[arg(long)]
    pub watch: bool,

    /// Emit a machine-readable JSON report to stdout instead of the table.
    #[arg(long)]
    pub report: bool,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Generate deterministic Markdown/HTML report files for a workspace.
    Report(ReportArgs),
}

#[derive(Args, Debug)]
pub struct ReportArgs {
    /// Workspace root to audit.
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Directory to write the report into (created if missing).
    #[arg(long, default_value = "tempcheq-report")]
    pub out: PathBuf,

    /// Which file(s) to generate.
    #[arg(long, value_enum, default_value_t = ReportFormat::Both)]
    pub format: ReportFormat,

    /// Report title. Defaults to the workspace path.
    #[arg(long)]
    pub title: Option<String>,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReportFormat {
    Md,
    Html,
    Both,
}
