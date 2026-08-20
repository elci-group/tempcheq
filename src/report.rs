use crate::action::{InferenceAction, TemperatureState};
use crate::hypothesize::Recommendation;
use owo_colors::OwoColorize;
use serde::Serialize;
use std::path::Path;

pub struct Entry {
    pub action: InferenceAction,
    pub rec: Recommendation,
}

pub fn print_header(root: &Path, entries: &[Entry]) {
    let total = entries.len();
    let controlled = entries
        .iter()
        .filter(|e| matches!(e.action.temperature, TemperatureState::Explicit(_)))
        .count();
    let implicit = entries
        .iter()
        .filter(|e| matches!(e.action.temperature, TemperatureState::Implicit))
        .count();
    let inapplicable = entries
        .iter()
        .filter(|e| matches!(e.action.temperature, TemperatureState::Inapplicable))
        .count();

    println!("{}", "TEMPCHEQ — inference temperature audit".bold());
    println!("Workspace: {}", root.display());
    println!();
    println!("Found {total} inference actions");
    println!("Temperature-controlled: {controlled}");
    println!("Implicit/default: {implicit}");
    println!("Temperature-inapplicable: {inapplicable}");
    println!();
}

pub fn print_table(entries: &[Entry]) {
    if entries.is_empty() {
        println!("(nothing to show)");
        return;
    }

    let headers = ["Action", "Kind", "Temp", "Optimal", "Confidence", "Verdict"];
    let mut rows: Vec<[String; 6]> = Vec::new();

    for e in entries {
        let optimal = format!("{:.2}", e.rec.ideal);
        let confidence = format!("{:.2}", e.rec.confidence);
        let verdict = match e.rec.verdict(&e.action) {
            Some(v) => format!("{:+.2}", v),
            None => "—".to_string(),
        };
        rows.push([
            truncate(&e.action.name, 24),
            e.action.source_kind.label().to_string(),
            e.action.temperature.as_display(),
            optimal,
            confidence,
            verdict,
        ]);
    }

    let mut widths = [0usize; 6];
    for (i, h) in headers.iter().enumerate() {
        widths[i] = h.len();
    }
    for row in &rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(cell.chars().count());
        }
    }

    print_rule(&widths, '┌', '┬', '┐');
    print_row(&headers.map(|h| h.to_string()), &widths, None);
    print_rule(&widths, '├', '┼', '┤');
    for (row, e) in rows.iter().zip(entries.iter()) {
        let color = row_color(e);
        print_row(row, &widths, color);
    }
    print_rule(&widths, '└', '┴', '┘');
    println!();
    println!(
        "Note: 'Optimal' is a heuristic prior from task classification, not a measured value. \
         Run --benchmark to empirically ground it against real outcomes."
    );
}

#[derive(Clone, Copy)]
enum RowColor {
    Green,
    Red,
    Yellow,
}

fn row_color(e: &Entry) -> Option<RowColor> {
    if matches!(e.action.temperature, TemperatureState::Explicit(_)) {
        if e.rec.is_material_deviation(&e.action) {
            Some(RowColor::Red)
        } else {
            Some(RowColor::Green)
        }
    } else if matches!(e.action.temperature, TemperatureState::Implicit) {
        Some(RowColor::Yellow)
    } else {
        None
    }
}

fn print_rule(widths: &[usize], left: char, mid: char, right: char) {
    let mut s = String::new();
    s.push(left);
    for (i, w) in widths.iter().enumerate() {
        s.push_str(&"─".repeat(w + 2));
        s.push(if i + 1 == widths.len() { right } else { mid });
    }
    println!("{s}");
}

fn print_row(cells: &[String], widths: &[usize], color: Option<RowColor>) {
    let mut s = String::from("│");
    for (cell, w) in cells.iter().zip(widths.iter()) {
        s.push_str(&format!(" {:<width$} │", cell, width = w));
    }
    match color {
        Some(RowColor::Green) => println!("{}", s.green()),
        Some(RowColor::Red) => println!("{}", s.red()),
        Some(RowColor::Yellow) => println!("{}", s.yellow()),
        None => println!("{s}"),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    }
}

pub fn print_optimization_summary(entries: &[Entry]) {
    let deviating: Vec<&Entry> = entries
        .iter()
        .filter(|e| {
            matches!(e.action.temperature, TemperatureState::Explicit(_))
                && e.rec.is_material_deviation(&e.action)
        })
        .collect();

    if deviating.is_empty() {
        println!("No actions materially outside their estimated optimum.");
        return;
    }

    let avg_abs_dev: f32 = deviating
        .iter()
        .filter_map(|e| e.rec.verdict(&e.action))
        .map(|v| v.abs())
        .sum::<f32>()
        / deviating.len() as f32;

    println!("Potential optimisation:");
    println!(
        "  {} action{} materially outside estimated optimum",
        deviating.len(),
        if deviating.len() == 1 { "" } else { "s" }
    );
    println!(
        "  Average |current - optimal| deviation: {:.2}",
        avg_abs_dev
    );
    println!(
        "  (heuristic estimate — run --benchmark for an empirically grounded figure)"
    );
}

pub fn print_explain(entries: &[Entry]) {
    println!("{}", "Reasoning".bold());
    println!();
    for e in entries {
        println!(
            "{} ({}:{})",
            e.action.name.bold(),
            e.action.file.display(),
            e.action.line
        );
        println!(
            "  class: {}  |  band: [{:.2}, {:.2}]  ideal: {:.2}  confidence: {:.2}",
            e.rec.class.label(),
            e.rec.low,
            e.rec.high,
            e.rec.ideal,
            e.rec.confidence
        );
        for r in &e.rec.reasons {
            println!("  - {r}");
        }
        if let Some(v) = e.rec.verdict(&e.action) {
            if e.rec.is_material_deviation(&e.action) {
                println!(
                    "  {} current={:.2} deviates {:+.2} from ideal",
                    "verdict:".red(),
                    e.action.temperature.value().unwrap_or_default(),
                    v
                );
            } else {
                println!(
                    "  {} current={:.2} is within the estimated band",
                    "verdict:".green(),
                    e.action.temperature.value().unwrap_or_default()
                );
            }
        }
        println!();
    }
}

#[derive(Serialize)]
struct ReportAction {
    name: String,
    file: String,
    line: usize,
    source_kind: String,
    temperature: Option<f32>,
    temperature_state: String,
    class: String,
    band_low: f32,
    band_ideal: f32,
    band_high: f32,
    confidence: f32,
    verdict: Option<f32>,
    material_deviation: bool,
    reasons: Vec<String>,
}

#[derive(Serialize)]
struct Report {
    workspace: String,
    total_actions: usize,
    temperature_controlled: usize,
    implicit_default: usize,
    inapplicable: usize,
    material_deviations: usize,
    actions: Vec<ReportAction>,
}

pub fn build_json_report(root: &Path, entries: &[Entry]) -> serde_json::Value {
    let actions: Vec<ReportAction> = entries
        .iter()
        .map(|e| ReportAction {
            name: e.action.name.clone(),
            file: e.action.file.display().to_string(),
            line: e.action.line,
            source_kind: e.action.source_kind.label().to_string(),
            temperature: e.action.temperature.value(),
            temperature_state: match e.action.temperature {
                TemperatureState::Explicit(_) => "explicit".to_string(),
                TemperatureState::Implicit => "implicit".to_string(),
                TemperatureState::Inapplicable => "inapplicable".to_string(),
            },
            class: e.rec.class.label().to_string(),
            band_low: e.rec.low,
            band_ideal: e.rec.ideal,
            band_high: e.rec.high,
            confidence: e.rec.confidence,
            verdict: e.rec.verdict(&e.action),
            material_deviation: e.rec.is_material_deviation(&e.action),
            reasons: e.rec.reasons.clone(),
        })
        .collect();

    let report = Report {
        workspace: root.display().to_string(),
        total_actions: entries.len(),
        temperature_controlled: entries
            .iter()
            .filter(|e| matches!(e.action.temperature, TemperatureState::Explicit(_)))
            .count(),
        implicit_default: entries
            .iter()
            .filter(|e| matches!(e.action.temperature, TemperatureState::Implicit))
            .count(),
        inapplicable: entries
            .iter()
            .filter(|e| matches!(e.action.temperature, TemperatureState::Inapplicable))
            .count(),
        material_deviations: entries
            .iter()
            .filter(|e| e.rec.is_material_deviation(&e.action))
            .count(),
        actions,
    };

    serde_json::to_value(report).unwrap()
}
