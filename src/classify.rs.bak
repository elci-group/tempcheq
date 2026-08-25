use crate::action::InferenceAction;

/// The kind of decoding task an inference action performs. This is the
/// axis TempCheq actually reasons about — not the model, not the file
/// type, but *what the sampling is being asked to do*.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskClass {
    Deterministic,
    ToolSelection,
    CodeGeneration,
    Summarization,
    Conversational,
    Reflection,
    Creative,
    EvalJudge,
    Synthesis,
    Unknown,
}

/// Every variant, in a fixed order — used anywhere findings are grouped
/// by class (e.g. the report subcommand's class-distribution chart) so
/// that output ordering depends only on this declaration, never on
/// hashing or discovery order.
pub const ALL_CLASSES: [TaskClass; 10] = [
    TaskClass::Deterministic,
    TaskClass::ToolSelection,
    TaskClass::CodeGeneration,
    TaskClass::Summarization,
    TaskClass::Conversational,
    TaskClass::Reflection,
    TaskClass::Creative,
    TaskClass::EvalJudge,
    TaskClass::Synthesis,
    TaskClass::Unknown,
];

impl TaskClass {
    pub fn label(&self) -> &'static str {
        match self {
            TaskClass::Deterministic => "deterministic-extraction",
            TaskClass::ToolSelection => "tool-selection",
            TaskClass::CodeGeneration => "code-generation",
            TaskClass::Summarization => "summarization",
            TaskClass::Conversational => "conversational",
            TaskClass::Reflection => "reflection",
            TaskClass::Creative => "creative/ideation",
            TaskClass::EvalJudge => "eval-judge",
            TaskClass::Synthesis => "synthesis",
            TaskClass::Unknown => "unclassified",
        }
    }

    /// (low, ideal, high) temperature band judged appropriate for this
    /// class of inference action. These are priors, not measurements —
    /// `--benchmark` exists to ground them in actual outcomes.
    pub fn prior_band(&self) -> (f32, f32, f32) {
        match self {
            TaskClass::Deterministic => (0.0, 0.05, 0.15),
            TaskClass::ToolSelection => (0.0, 0.15, 0.25),
            TaskClass::EvalJudge => (0.0, 0.10, 0.25),
            TaskClass::CodeGeneration => (0.30, 0.45, 0.60),
            TaskClass::Summarization => (0.20, 0.35, 0.55),
            TaskClass::Conversational => (0.50, 0.65, 0.80),
            TaskClass::Reflection => (0.55, 0.72, 0.85),
            TaskClass::Synthesis => (0.60, 0.78, 0.90),
            TaskClass::Creative => (0.75, 0.90, 1.10),
            TaskClass::Unknown => (0.20, 0.50, 0.80),
        }
    }
}

struct Rule {
    class: TaskClass,
    weight: f32,
    keywords: &'static [&'static str],
}

const RULES: &[Rule] = &[
    Rule { class: TaskClass::Deterministic, weight: 1.0, keywords: &[
        "extract", "extraction", "parse", "parsing", "json_schema", "structured_output",
        "regex", "classif", "categoriz", "categoris", "label", "ner", "entity",
        "validate", "validator", "deterministic",
    ]},
    Rule { class: TaskClass::ToolSelection, weight: 1.0, keywords: &[
        "tool_choice", "tool-selection", "tool_selection", "function_call", "router",
        "routing", "dispatch", "intent", "planner", "plan_step", "orchestrat",
    ]},
    Rule { class: TaskClass::EvalJudge, weight: 1.0, keywords: &[
        "eval", "evaluat", "benchmark", "judge", "grader", "grading", "scorer",
        "rubric", "assert", "test_case",
    ]},
    Rule { class: TaskClass::CodeGeneration, weight: 1.0, keywords: &[
        "codegen", "code_generation", "generate_code", "completion", "autocomplete",
        "refactor", "patch", "diff", "compile", "lint",
    ]},
    Rule { class: TaskClass::Summarization, weight: 1.0, keywords: &[
        "summar", "digest", "tldr", "condense", "abstract",
    ]},
    Rule { class: TaskClass::Reflection, weight: 1.0, keywords: &[
        "reflect", "reflection", "critique", "self_review", "self-review",
        "postmortem", "retrospective",
    ]},
    Rule { class: TaskClass::Synthesis, weight: 1.0, keywords: &[
        "synthesis", "synthesize", "synthesise", "merge_results", "aggregat", "compose",
    ]},
    Rule { class: TaskClass::Creative, weight: 1.0, keywords: &[
        "creative", "ideat", "brainstorm", "story", "narrative", "poem", "poetry",
        "explor", "divergent", "imagin",
    ]},
    Rule { class: TaskClass::Conversational, weight: 0.8, keywords: &[
        "chat", "conversation", "dialogue", "assistant", "reply", "respond",
    ]},
];

pub struct Classification {
    pub class: TaskClass,
    pub confidence: f32,
    pub matched_keywords: Vec<&'static str>,
}

/// True if `kw` occurs in `haystack` as its own token — not as a run of
/// letters embedded in a longer word. Delimiters like `_`/`-`/digits/
/// punctuation count as boundaries, so this still matches "eval" inside
/// "run_eval" or "eval_script", but rejects it inside "retrieval". Plain
/// substring search can't make this distinction, and the `regex` crate
/// has no lookaround to express it as one pattern, so it's done by hand
/// over byte offsets (safe even if `haystack` contains non-ASCII, since
/// this only ever inspects individual bytes, never slices on them).
fn keyword_present(haystack: &str, kw: &str) -> bool {
    let bytes = haystack.as_bytes();
    let mut start = 0;
    while let Some(pos) = haystack[start..].find(kw) {
        let idx = start + pos;
        let before_ok = idx == 0 || !bytes[idx - 1].is_ascii_alphabetic();
        let after = idx + kw.len();
        let after_ok = after >= bytes.len() || !bytes[after].is_ascii_alphabetic();
        if before_ok && after_ok {
            return true;
        }
        start = idx + 1;
        if start >= haystack.len() {
            break;
        }
    }
    false
}

/// Score an action's name, context line, and file path against the
/// keyword rules; pick the highest-scoring class.
pub fn classify(action: &InferenceAction) -> Classification {
    let haystack = format!(
        "{} {} {}",
        action.name,
        action.context,
        action.file.to_string_lossy()
    )
    .to_lowercase();

    let mut best: Option<(TaskClass, f32, Vec<&'static str>)> = None;

    for rule in RULES {
        let mut hits: Vec<&'static str> = Vec::new();
        for kw in rule.keywords {
            if keyword_present(&haystack, kw) {
                hits.push(kw);
            }
        }
        if hits.is_empty() {
            continue;
        }
        let score = rule.weight * hits.len() as f32;
        if best.as_ref().map(|(_, s, _)| score > *s).unwrap_or(true) {
            best = Some((rule.class, score, hits));
        }
    }

    match best {
        Some((class, score, hits)) => {
            // 1 keyword hit -> moderate confidence, more corroborating
            // hits push confidence toward the ceiling. The floor sits
            // below the single-hit baseline (0.55) on purpose: a rule
            // with weight < 1.0 (e.g. Conversational) is meant to score
            // a lone hit lower than a full-weight rule's lone hit, and a
            // floor of 0.55 would clip that distinction away entirely.
            let confidence = (0.55 + 0.15 * (score - 1.0)).clamp(0.40, 0.95);
            Classification { class, confidence, matched_keywords: hits }
        }
        None => Classification {
            class: TaskClass::Unknown,
            confidence: 0.35,
            matched_keywords: Vec::new(),
        },
    }
}
