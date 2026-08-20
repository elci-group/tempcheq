use crate::discover::discover;
use crate::hypothesize::hypothesize;
use crate::report::{self, Entry};
use anyhow::Result;
use std::path::Path;

#[derive(Clone)]
pub struct RunOptions {
    pub explain: bool,
    pub benchmark: bool,
    pub json: bool,
}

pub fn build_entries(root: &Path) -> Result<Vec<Entry>> {
    let actions = discover(root)?;
    Ok(actions
        .into_iter()
        .map(|action| {
            let rec = hypothesize(&action);
            Entry { action, rec }
        })
        .collect())
}

pub fn run_audit(root: &Path, opts: &RunOptions) -> Result<()> {
    let mut entries = build_entries(root)?;
    entries.sort_by(|a, b| {
        a.action
            .file
            .cmp(&b.action.file)
            .then(a.action.line.cmp(&b.action.line))
    });

    if opts.json {
        let value = report::build_json_report(root, &entries);
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }

    report::print_header(root, &entries);
    report::print_table(&entries);
    println!();
    report::print_optimization_summary(&entries);

    if opts.explain {
        println!();
        report::print_explain(&entries);
    }

    if opts.benchmark {
        println!();
        crate::benchmark::print_benchmarks(&entries);
    }

    Ok(())
}
