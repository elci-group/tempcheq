//! Stable thermal-instrument domain model for HTML visualisation.
//!
//! This module is the adapter boundary between TempCheq's audit records and
//! presentation. It owns validation, state classification, scale generation,
//! and display precision; the HTML renderer only consumes `ThermalProcess`.

use crate::action::TemperatureState;
use crate::report::Entry;
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThermalState {
    Cold,
    Optimal,
    High,
    Critical,
    Unknown,
}

impl ThermalState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cold => "COLD",
            Self::Optimal => "OPTIMAL",
            Self::High => "HIGH",
            Self::Critical => "CRITICAL",
            Self::Unknown => "UNKNOWN",
        }
    }

    pub fn css_class(self) -> &'static str {
        match self {
            Self::Cold => "cold",
            Self::Optimal => "optimal",
            Self::High => "high",
            Self::Critical => "critical",
            Self::Unknown => "unknown",
        }
    }

    pub fn rank(self) -> u8 {
        match self {
            Self::Critical => 4,
            Self::High => 3,
            Self::Cold => 2,
            Self::Unknown => 1,
            Self::Optimal => 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScaleTick {
    pub value: f32,
    pub percent: f32,
    pub major: bool,
}

#[derive(Debug, Clone)]
pub struct ThermalScale {
    /// Presentation bounds only; never represented as safety limits.
    pub display_min: f32,
    pub display_max: f32,
    pub major_step: f32,
    pub ticks: Vec<ScaleTick>,
}

impl ThermalScale {
    pub fn percent(&self, value: f32) -> f32 {
        if !value.is_finite() || self.display_max <= self.display_min {
            return 0.0;
        }
        (((value - self.display_min) / (self.display_max - self.display_min)) * 100.0)
            .clamp(0.0, 100.0)
    }
}

#[derive(Debug, Clone)]
pub struct ThermalProcess {
    pub id: String,
    pub name: String,
    pub location: String,
    pub source_kind: String,
    pub task_class: String,
    pub actual: Option<f32>,
    pub ideal: Option<f32>,
    /// TempCheq's heuristic class band, not an engineering safety limit.
    pub operating_min: Option<f32>,
    pub operating_max: Option<f32>,
    /// Reserved for a future authoritative threshold supplied by telemetry.
    pub critical_high: Option<f32>,
    pub deviation: Option<f32>,
    pub state: ThermalState,
    pub unit: &'static str,
    pub precision: usize,
    pub confidence: f32,
    pub freshness: Option<String>,
    pub scale: ThermalScale,
    pub reasons: Vec<String>,
}

impl ThermalProcess {
    pub fn actual_text(&self) -> String {
        self.actual
            .map(|value| format_value(value, self.precision))
            .unwrap_or_else(|| "—".to_string())
    }

    pub fn ideal_text(&self) -> String {
        self.ideal
            .map(|value| format_value(value, self.precision.max(2)))
            .unwrap_or_else(|| "—".to_string())
    }

    pub fn deviation_text(&self) -> String {
        self.deviation
            .map(|value| format!("{:+.*}", self.precision.max(2), normalized_zero(value)))
            .unwrap_or_else(|| "—".to_string())
    }

    pub fn actual_percent(&self) -> f32 {
        self.actual.map(|v| self.scale.percent(v)).unwrap_or(0.0)
    }

    pub fn ideal_percent(&self) -> Option<f32> {
        self.ideal.map(|v| self.scale.percent(v))
    }

    pub fn range_percent(&self) -> Option<(f32, f32)> {
        Some((
            self.scale.percent(self.operating_min?),
            self.scale.percent(self.operating_max?),
        ))
    }
}

pub fn normalize_entries(entries: &[Entry]) -> Vec<ThermalProcess> {
    let mut processes: Vec<ThermalProcess> = entries
        .iter()
        .map(|entry| {
            let actual = finite(entry.action.temperature.value());
            let applicable = !matches!(entry.action.temperature, TemperatureState::Inapplicable);
            let ideal = applicable.then_some(entry.rec.ideal).and_then(finite_value);
            let operating_min = applicable.then_some(entry.rec.low).and_then(finite_value);
            let operating_max = applicable.then_some(entry.rec.high).and_then(finite_value);
            // TempCheq currently has no authoritative critical threshold. Do
            // not infer one from the heuristic band and mislabel it as safety.
            let critical_high = None;
            let deviation = actual.zip(ideal).map(|(a, i)| a - i).and_then(finite_value);
            let state = classify_state(actual, operating_min, operating_max, critical_high);
            let scale = calculate_scale(actual, ideal, operating_min, operating_max, critical_high);
            let precision = entry.action.temperature_precision.unwrap_or(2).min(6);

            ThermalProcess {
                id: format!("{}:{}", entry.action.file.display(), entry.action.line),
                name: entry.action.name.clone(),
                location: format!("{}:{}", entry.action.file.display(), entry.action.line),
                source_kind: entry.action.source_kind.label().to_string(),
                task_class: entry.rec.class.label().to_string(),
                actual,
                ideal,
                operating_min,
                operating_max,
                critical_high,
                deviation,
                state,
                unit: "unitless",
                precision,
                confidence: entry.rec.confidence,
                freshness: None,
                scale,
                reasons: entry.rec.reasons.clone(),
            }
        })
        .collect();

    processes.sort_by(|a, b| {
        b.state
            .rank()
            .cmp(&a.state.rank())
            .then_with(|| {
                b.deviation
                    .map(f32::abs)
                    .partial_cmp(&a.deviation.map(f32::abs))
                    .unwrap_or(Ordering::Equal)
            })
            .then_with(|| a.id.cmp(&b.id))
    });
    processes
}

fn finite(value: Option<f32>) -> Option<f32> {
    value.and_then(finite_value)
}

fn finite_value(value: f32) -> Option<f32> {
    value.is_finite().then_some(value)
}

fn classify_state(
    actual: Option<f32>,
    operating_min: Option<f32>,
    operating_max: Option<f32>,
    critical_high: Option<f32>,
) -> ThermalState {
    let Some(actual) = finite(actual) else {
        return ThermalState::Unknown;
    };
    if critical_high.is_some_and(|threshold| threshold.is_finite() && actual >= threshold) {
        return ThermalState::Critical;
    }
    if operating_min.is_some_and(|minimum| minimum.is_finite() && actual < minimum) {
        return ThermalState::Cold;
    }
    if operating_max.is_some_and(|maximum| maximum.is_finite() && actual > maximum) {
        return ThermalState::High;
    }
    if operating_min.is_some() || operating_max.is_some() {
        ThermalState::Optimal
    } else {
        ThermalState::Unknown
    }
}

pub fn calculate_scale(
    actual: Option<f32>,
    ideal: Option<f32>,
    operating_min: Option<f32>,
    operating_max: Option<f32>,
    critical_high: Option<f32>,
) -> ThermalScale {
    let mut points: Vec<f32> = [actual, ideal, operating_min, operating_max, critical_high]
        .into_iter()
        .flatten()
        .filter(|value| value.is_finite())
        .collect();
    if points.is_empty() {
        points.extend([0.0, 1.0]);
    }

    let mut low = points.iter().copied().fold(f32::INFINITY, f32::min);
    let mut high = points.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let non_negative_domain = low >= 0.0;
    let mut span = high - low;
    if span <= f32::EPSILON {
        span = (high.abs() * 0.4).max(0.2);
        low -= span / 2.0;
        high += span / 2.0;
    }

    let margin = (span * 0.16).max(0.02);
    let step = nice_step((span + margin * 2.0) / 5.0);
    let mut display_min = ((low - margin) / step).floor() * step;
    let display_max = ((high + margin) / step).ceil() * step;
    if non_negative_domain && display_min < 0.0 {
        display_min = 0.0;
    }

    let display_max = if display_max <= display_min {
        display_min + step
    } else {
        display_max
    };
    let mut ticks = Vec::new();
    let intervals = (((display_max - display_min) / step).round() as usize).clamp(1, 12);
    for major_index in 0..=intervals {
        let major_value = display_min + major_index as f32 * step;
        ticks.push(ScaleTick {
            value: normalized_zero(major_value),
            percent: (major_index as f32 / intervals as f32) * 100.0,
            major: true,
        });
        if major_index < intervals {
            for minor_index in 1..4 {
                ticks.push(ScaleTick {
                    value: major_value + step * minor_index as f32 / 4.0,
                    percent: ((major_index as f32 + minor_index as f32 / 4.0) / intervals as f32)
                        * 100.0,
                    major: false,
                });
            }
        }
    }

    ThermalScale {
        display_min: normalized_zero(display_min),
        display_max: normalized_zero(display_max),
        major_step: step,
        ticks,
    }
}

fn nice_step(raw: f32) -> f32 {
    if !raw.is_finite() || raw <= 0.0 {
        return 0.2;
    }
    let exponent = raw.log10().floor();
    let magnitude = 10_f32.powf(exponent);
    let fraction = raw / magnitude;
    let nice_fraction = if fraction <= 1.0 {
        1.0
    } else if fraction <= 2.0 {
        2.0
    } else if fraction <= 2.5 {
        2.5
    } else if fraction <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice_fraction * magnitude
}

pub fn precision_for_step(step: f32) -> usize {
    if !step.is_finite() || step <= 0.0 || step >= 1.0 {
        return 0;
    }
    (-step.log10().floor() as i32).clamp(0, 4) as usize
}

pub fn format_value(value: f32, precision: usize) -> String {
    if !value.is_finite() {
        return "—".to_string();
    }
    format!("{:.*}", precision.min(6), normalized_zero(value))
}

fn normalized_zero(value: f32) -> f32 {
    if value.abs() < 0.000_000_5 {
        0.0
    } else {
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_contains_actual_ideal_band_and_negative_values() {
        let scale = calculate_scale(Some(-0.4), Some(0.2), Some(-0.1), Some(0.5), None);
        assert!(scale.display_min <= -0.4);
        assert!(scale.display_max >= 0.5);
        assert!((0.0..=100.0).contains(&scale.percent(0.2)));
        assert!(scale.ticks.iter().any(|tick| !tick.major));
    }

    #[test]
    fn scale_is_stable_for_small_changes_inside_the_same_band() {
        let first = calculate_scale(Some(0.50), Some(0.45), Some(0.30), Some(0.60), None);
        let second = calculate_scale(Some(0.51), Some(0.45), Some(0.30), Some(0.60), None);
        assert_eq!(first.display_min, second.display_min);
        assert_eq!(first.display_max, second.display_max);
        assert_eq!(first.major_step, second.major_step);
    }

    #[test]
    fn identical_values_still_produce_a_meaningful_scale() {
        let scale = calculate_scale(Some(0.7), Some(0.7), Some(0.7), Some(0.7), None);
        assert!(scale.display_max > scale.display_min);
        assert!(scale.display_min <= 0.7 && scale.display_max >= 0.7);
    }

    #[test]
    fn states_do_not_invent_critical_thresholds() {
        assert_eq!(
            classify_state(Some(1.2), Some(0.2), Some(0.8), None),
            ThermalState::High
        );
        assert_eq!(
            classify_state(Some(1.2), Some(0.2), Some(0.8), Some(1.1)),
            ThermalState::Critical
        );
        assert_eq!(
            classify_state(Some(f32::NAN), Some(0.2), Some(0.8), None),
            ThermalState::Unknown
        );
    }

    #[test]
    fn numeric_formatting_preserves_requested_precision() {
        assert_eq!(format_value(72.4, 1), "72.4");
        assert_eq!(format_value(0.20, 2), "0.20");
        assert_eq!(format_value(f32::INFINITY, 2), "—");
    }
}
