use crate::hypothesize::Recommendation;
use crate::report::Entry;
use owo_colors::OwoColorize;

const CANDIDATE_OFFSETS: &[f32] = &[-0.5, -0.3, -0.1, 0.0, 0.1, 0.3];

/// Deterministic pseudo-random unit value in [0, 1) from a string + salt,
/// used only to give the simulated benchmark curve non-identical noise
/// across runs of the same action — not a source of real signal.
fn hash_unit(seed: &str, salt: u32) -> f32 {
    let mut h: u64 = 1469598103934665603 ^ salt as u64;
    for b in seed.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(1099511628211);
    }
    (h % 10_000) as f32 / 10_000.0
}

/// A simulated score curve peaking near the hypothesized ideal. This is
/// NOT a live model call — TempCheq does not have API keys or a scoring
/// harness wired in by default. It exists to make the shape of the
/// "perturb and benchmark" workflow visible now, and as the seam a real
/// eval hook (a project-supplied command that scores a transcript at a
/// given temperature) would plug into later.
fn simulated_score(rec: &Recommendation, candidate: f32) -> f32 {
    let spread = ((rec.high - rec.low).max(0.05)) * 1.3;
    let dist = ((candidate - rec.ideal) / spread).abs();
    let base = (1.0 - dist.min(1.0).powf(1.6)) * 0.85 + 0.10;
    let noise = (hash_unit(&format!("{:.2}", candidate), rec.matched_keywords.len() as u32) - 0.5) * 0.03;
    (base + noise).clamp(0.0, 1.0)
}

pub fn print_benchmarks(entries: &[Entry]) {
    println!(
        "{}",
        "Benchmark mode is SIMULATED: scores below are generated from the heuristic \
         classification prior, not measured from live model calls."
            .yellow()
    );
    println!(
        "To ground this empirically, wire a project eval command into TempCheq's benchmark hook."
    );
    println!();

    for e in entries {
        let current = e.action.temperature.value();
        println!(
            "{} ({}:{})",
            e.action.name.bold(),
            e.action.file.display(),
            e.action.line
        );
        if let Some(c) = current {
            println!("Current:       T={:.2}", c);
        } else {
            println!("Current:       {}", e.action.temperature.as_display());
        }
        println!();
        println!("Hypotheses:");

        let mut best_t = e.rec.ideal;
        let mut best_score = f32::MIN;
        let mut candidates: Vec<f32> = CANDIDATE_OFFSETS
            .iter()
            .map(|off| (e.rec.ideal + off).clamp(0.0, 2.0))
            .collect();
        candidates.dedup_by(|a, b| (*a - *b).abs() < 0.01);
        candidates.sort_by(|a, b| a.partial_cmp(b).unwrap());

        for t in &candidates {
            let score = simulated_score(&e.rec, *t);
            if score > best_score {
                best_score = score;
                best_t = *t;
            }
            println!("T={:<5.2} score {:.3}", t, score);
        }

        println!();
        println!("Estimated optimum: T≈{:.2}", best_t);
        if let Some(c) = current {
            println!("Current deviation: {:+.2}", c - best_t);
        }
        println!();
    }
}
