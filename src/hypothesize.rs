use crate::action::{InferenceAction, TemperatureState};
use crate::classify::{classify, Classification, TaskClass};

pub struct Recommendation {
    pub class: TaskClass,
    pub matched_keywords: Vec<&'static str>,
    pub low: f32,
    pub ideal: f32,
    pub high: f32,
    /// Overall confidence in the recommendation, folding in classification
    /// confidence and any output-constraint corroboration.
    pub confidence: f32,
    pub reasons: Vec<String>,
}

impl Recommendation {
    /// Signed deviation of the action's *current* explicit temperature
    /// from the estimated ideal. `None` for implicit/inapplicable actions,
    /// which have no current value to compare against.
    pub fn verdict(&self, action: &InferenceAction) -> Option<f32> {
        action.temperature.value().map(|v| v - self.ideal)
    }

    pub fn is_material_deviation(&self, action: &InferenceAction) -> bool {
        match self.verdict(action) {
            Some(d) => d.abs() > 0.15 || !self.in_band(action),
            None => false,
        }
    }

    fn in_band(&self, action: &InferenceAction) -> bool {
        match action.temperature.value() {
            Some(v) => v >= self.low && v <= self.high,
            None => true,
        }
    }
}

/// Structural signals in the surrounding context that push the ideal
/// temperature down (tighter output contracts) or up (more open-ended),
/// independent of the keyword-based task class.
struct ConstraintSignal {
    needle: &'static str,
    ideal_shift: f32,
    reason: &'static str,
}

const CONSTRAINTS: &[ConstraintSignal] = &[
    ConstraintSignal { needle: "json_schema", ideal_shift: -0.10, reason: "output is constrained by a JSON schema" },
    ConstraintSignal { needle: "response_format", ideal_shift: -0.08, reason: "a structured response_format is enforced" },
    ConstraintSignal { needle: "tool_choice", ideal_shift: -0.10, reason: "tool_choice pins the decision space" },
    ConstraintSignal { needle: "seed", ideal_shift: -0.05, reason: "a fixed seed suggests reproducibility is required" },
    ConstraintSignal { needle: "stream", ideal_shift: 0.02, reason: "streaming conversational output tolerates more variance" },
];

pub fn hypothesize(action: &InferenceAction) -> Recommendation {
    let Classification { class, confidence, matched_keywords } = classify(action);
    let (low, ideal_base, high) = class.prior_band();

    let mut reasons = Vec::new();
    if matched_keywords.is_empty() {
        reasons.push(format!(
            "no strong task keywords found near '{}'; falling back to a wide unclassified band",
            action.name
        ));
    } else {
        reasons.push(format!(
            "classified as {} on keywords: {}",
            class.label(),
            matched_keywords.join(", ")
        ));
    }

    let haystack = action.context.to_lowercase();
    let mut ideal = ideal_base;
    let mut confidence_bonus = 0.0f32;
    for c in CONSTRAINTS {
        if haystack.contains(c.needle) {
            ideal += c.ideal_shift;
            confidence_bonus += 0.03;
            reasons.push(c.reason.to_string());
        }
    }
    // Constraint shifts can push `ideal` outside the class's prior band
    // (e.g. several tightening constraints stacking on a deterministic
    // action could otherwise drive it negative); keep it inside [low, high].
    ideal = ideal.clamp(low, high);

    match action.temperature {
        TemperatureState::Implicit => {
            reasons.push("no explicit temperature set; provider default is in effect".to_string());
        }
        TemperatureState::Inapplicable => {
            reasons.push("call site does not expose a meaningful temperature parameter".to_string());
        }
        TemperatureState::Explicit(_) => {}
    }

    let confidence = (confidence + confidence_bonus).clamp(0.05, 0.97);

    Recommendation {
        class,
        matched_keywords,
        low,
        ideal,
        high,
        confidence,
        reasons,
    }
}
