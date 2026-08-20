mod action;
mod benchmark;
mod cli;
mod classify;
mod discover;
mod export;
mod fix;
mod hypothesize;
mod report;
mod run;
mod watch;

use anyhow::{bail, Result};
use clap::Parser;
use cli::{Cli, Command, ReportArgs, ReportFormat};
use run::{build_entries, run_audit, RunOptions};

fn main() -> Result<()> {
    let cli = Cli::parse();

    if let Some(Command::Report(args)) = cli.command {
        return run_report(args);
    }

    if !cli.path.exists() {
        bail!("path does not exist: {}", cli.path.display());
    }
    let root = std::fs::canonicalize(&cli.path)?;

    let opts = RunOptions {
        explain: cli.explain,
        benchmark: cli.benchmark,
        json: cli.report,
    };

    if cli.watch {
        return watch::watch(&root, &opts);
    }

    if cli.fix {
        let entries = build_entries(&root)?;
        let plans = fix::plan_fixes(&entries, cli.fix_confidence);
        fix::print_plan(&plans);
        if plans.is_empty() {
            return Ok(());
        }
        if cli.yes {
            let skipped = fix::apply_fixes(&root, &plans)?;
            let applied = plans.len() - skipped.len();
            println!("\nApplied {applied} of {} change(s).", plans.len());
            if !skipped.is_empty() {
                println!("Skipped (no recognized temperature pattern on that line):");
                for p in skipped {
                    println!("  {}:{}  {}", p.file.display(), p.line, p.name);
                }
            }
        } else {
            println!("\nDry run only — pass --fix --yes to apply.");
        }
        return Ok(());
    }

    run_audit(&root, &opts)
}

fn run_report(args: ReportArgs) -> Result<()> {
    if !args.path.exists() {
        bail!("path does not exist: {}", args.path.display());
    }
    let root = std::fs::canonicalize(&args.path)?;
    let entries = build_entries(&root)?;

    std::fs::create_dir_all(&args.out)?;

    let mut written = Vec::new();

    if matches!(args.format, ReportFormat::Md | ReportFormat::Both) {
        let md = export::render_markdown(&root, &entries, args.title.as_deref());
        let path = args.out.join("report.md");
        std::fs::write(&path, md)?;
        written.push(path);
    }

    if matches!(args.format, ReportFormat::Html | ReportFormat::Both) {
        let html = export::render_html(&root, &entries, args.title.as_deref());
        let path = args.out.join("report.html");
        std::fs::write(&path, html)?;
        written.push(path);
    }

    println!("TempCheq report written:");
    for p in &written {
        println!("  {}", p.display());
    }
    println!(
        "\n{} inference action(s) audited, {} flagged for review.",
        entries.len(),
        entries
            .iter()
            .filter(|e| e.rec.is_material_deviation(&e.action))
            .count()
    );

    Ok(())
}
