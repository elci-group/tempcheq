use crate::discover::{BUILDER_TEMP, ENV_TEMP_ASSIGN, EXPLICIT_TEMP};
use crate::report::Entry;
use anyhow::{Context, Result};
use owo_colors::OwoColorize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub struct FixPlan {
    pub file: PathBuf,
    pub line: usize,
    pub name: String,
    pub old: f32,
    pub new: f32,
}

/// Only actions with an explicit temperature, a material deviation, and
/// classification confidence at or above `threshold` are eligible — this
/// is deliberately conservative since --fix rewrites source files.
pub fn plan_fixes(entries: &[Entry], threshold: f32) -> Vec<FixPlan> {
    entries
        .iter()
        .filter_map(|e| {
            let old = e.action.temperature.value()?;
            if !e.rec.is_material_deviation(&e.action) || e.rec.confidence < threshold {
                return None;
            }
            Some(FixPlan {
                file: e.action.file.clone(),
                line: e.action.line,
                name: e.action.name.clone(),
                old,
                new: e.rec.ideal,
            })
        })
        .collect()
}

pub fn print_plan(plans: &[FixPlan]) {
    if plans.is_empty() {
        println!("No high-confidence fixes to apply.");
        return;
    }
    println!("{}", "Planned changes:".bold());
    for p in plans {
        println!(
            "  {}:{}  {}  {:.2} -> {:.2}",
            p.file.display(),
            p.line,
            p.name,
            p.old,
            p.new
        );
    }
}

/// Rewrite each planned line in place, replacing only the numeric
/// temperature value while leaving the rest of the line untouched.
/// Returns the number of plans actually applied — always compare this
/// against `plans.len()` rather than assuming every plan lands, since a
/// plan whose line matches none of the known temperature patterns (e.g.
/// discover.rs learns of new patterns before this list is updated to
/// match) is skipped rather than guessed at.
pub fn apply_fixes<'a>(root: &Path, plans: &'a [FixPlan]) -> Result<Vec<&'a FixPlan>> {
    let mut by_file: BTreeMap<PathBuf, Vec<&FixPlan>> = BTreeMap::new();
    for p in plans {
        by_file.entry(p.file.clone()).or_default().push(p);
    }

    let mut skipped = Vec::new();

    for (rel, file_plans) in by_file {
        let abs = root.join(&rel);
        let content = std::fs::read_to_string(&abs)
            .with_context(|| format!("reading {}", abs.display()))?;
        let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();

        for p in file_plans {
            let idx = p.line - 1;
            let Some(line) = lines.get(idx) else {
                skipped.push(p);
                continue;
            };
            let range = EXPLICIT_TEMP
                .captures(line)
                .or_else(|| BUILDER_TEMP.captures(line))
                .map(|m| m.get(1).unwrap().range())
                .or_else(|| ENV_TEMP_ASSIGN.captures(line).map(|m| m.get(2).unwrap().range()));
            let Some(range) = range else {
                skipped.push(p);
                continue;
            };
            let mut s = line.clone();
            s.replace_range(range, &format!("{:.2}", p.new));
            lines[idx] = s;
        }

        let mut new_content = lines.join("\n");
        if content.ends_with('\n') {
            new_content.push('\n');
        }
        std::fs::write(&abs, new_content)
            .with_context(|| format!("writing {}", abs.display()))?;
    }

    Ok(skipped)
}
