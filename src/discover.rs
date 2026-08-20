use crate::action::{InferenceAction, SourceKind, TemperatureState};
use anyhow::Result;
use once_cell::sync::Lazy;
use regex::Regex;
use std::path::Path;
use walkdir::WalkDir;

/// Directories we never descend into: build output, VCS internals, deps.
const SKIP_DIRS: &[&str] = &[
    ".git", "target", "node_modules", "dist", "build", ".venv", "venv",
    "__pycache__", ".mypy_cache", ".pytest_cache", ".next", ".cache",
];

/// File extensions worth scanning at all.
const SCAN_EXTS: &[&str] = &[
    "rs", "py", "js", "ts", "tsx", "jsx", "sh", "bash", "zsh", "yml",
    "yaml", "json", "toml", "env", "cfg", "ini", "rb", "go",
];

pub(crate) static EXPLICIT_TEMP: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)\btemperature\b"?\s*[:=]\s*"?(-?[0-9]*\.?[0-9]+)"?"#).unwrap()
});

pub(crate) static BUILDER_TEMP: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)\.temperature\(\s*(-?[0-9]*\.?[0-9]+)\s*\)"#).unwrap()
});

// Note: the identifier group starts with `[A-Z0-9_]*` (star, not a
// mandatory leading char class) so that a bare `TEMPERATURE` — with no
// prefix like `OPENAI_` — still matches. A mandatory leading char class
// here would make the *shortest* possible match require a prefix char
// before the literal "TEMPERATURE", which a same-length bare var name
// can never supply.
pub(crate) static ENV_TEMP_ASSIGN: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)\b([A-Z0-9_]*TEMPERATURE[A-Z0-9_]*)\s*[:=]\s*"?(-?[0-9]*\.?[0-9]+)"?"#).unwrap()
});

static ENV_TEMP_REF: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)(?:os\.environ(?:\.get)?|process\.env|std::env::var)\(?["']?([A-Z0-9_]*TEMPERATURE[A-Z0-9_]*)["']?\)?"#).unwrap()
});

static NAME_HINT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)^\s*(?:export\s+)?(?:pub\s+)?(?:async\s+)?(?:fn|def|function|class|struct|const|let|impl)\s+([A-Za-z_][A-Za-z0-9_]*)"#).unwrap()
});

static JSON_KEY_HINT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"^\s*"([A-Za-z_][A-Za-z0-9_\- ]*)"\s*:"#).unwrap()
});

static YAML_KEY_HINT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"^\s*([A-Za-z_][A-Za-z0-9_\-]*)\s*:\s*\S"#).unwrap()
});

/// (regex, source_kind) pairs identifying a call site as an inference
/// action, in rough order of specificity.
static CALL_MARKERS: Lazy<Vec<(Regex, SourceKind)>> = Lazy::new(|| {
    vec![
        (Regex::new(r"openai\.(ChatCompletion|chat\.completions)\.create").unwrap(), SourceKind::SdkInvocation),
        (Regex::new(r"client\.chat\.completions\.create").unwrap(), SourceKind::SdkInvocation),
        (Regex::new(r"client\.messages\.create|anthropic\.Anthropic\(|\.messages\.create\(").unwrap(), SourceKind::SdkInvocation),
        (Regex::new(r"ChatOpenAI\(|ChatAnthropic\(|ChatGoogleGenerativeAI\(|ChatOllama\(").unwrap(), SourceKind::AgentConfig),
        (Regex::new(r"ollama\.(chat|generate)\(").unwrap(), SourceKind::SdkInvocation),
        (Regex::new(r"genai\.GenerativeModel\(|GenerationConfig\(").unwrap(), SourceKind::SdkInvocation),
        (Regex::new(r"(fetch|axios\.post|reqwest::)\([^)]*(chat/completions|/messages|generateContent)").unwrap(), SourceKind::ApiCall),
        (Regex::new(r"curl\s+.*(chat/completions|/v1/messages|/api/generate)").unwrap(), SourceKind::ShellWrapper),
        (Regex::new(r#"(?i)"model"\s*:"#).unwrap(), SourceKind::ConfigFile),
    ]
});

static EMBEDDING_MARKER: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\.embeddings\.create\(|embed_documents\(|embed_query\(|EmbeddingModel\(").unwrap()
});

pub fn discover(root: &Path) -> Result<Vec<InferenceAction>> {
    let mut actions = Vec::new();

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| {
            if e.file_type().is_dir() {
                let name = e.file_name().to_string_lossy();
                !SKIP_DIRS.contains(&name.as_ref())
            } else {
                true
            }
        })
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let is_workflow = path
            .components()
            .any(|c| c.as_os_str() == ".github")
            && (ext == "yml" || ext == "yaml");
        if !SCAN_EXTS.contains(&ext) {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        scan_file(root, path, &content, is_workflow, &mut actions);
    }

    Ok(actions)
}

fn scan_file(
    root: &Path,
    path: &Path,
    content: &str,
    is_workflow: bool,
    out: &mut Vec<InferenceAction>,
) {
    let lines: Vec<&str> = content.lines().collect();
    let rel = path.strip_prefix(root).unwrap_or(path).to_path_buf();
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

    let mut explained_lines: Vec<usize> = Vec::new();

    // Pass 1: explicit temperature values (key:value or builder syntax).
    for (i, line) in lines.iter().enumerate() {
        if let Some(cap) = EXPLICIT_TEMP.captures(line).or_else(|| BUILDER_TEMP.captures(line)) {
            let Ok(val) = cap[1].parse::<f32>() else { continue };
            let kind = classify_source_kind(path, ext, is_workflow, line);
            let name = nearest_name(&lines, i, &rel);
            out.push(InferenceAction {
                name,
                file: rel.clone(),
                line: i + 1,
                source_kind: kind,
                temperature: TemperatureState::Explicit(val),
                context: line.trim().to_string(),
            });
            explained_lines.push(i);
        } else if let Some(cap) = ENV_TEMP_ASSIGN.captures(line) {
            let Ok(val) = cap[2].parse::<f32>() else { continue };
            out.push(InferenceAction {
                name: cap[1].to_string(),
                file: rel.clone(),
                line: i + 1,
                source_kind: SourceKind::EnvVar,
                temperature: TemperatureState::Explicit(val),
                context: line.trim().to_string(),
            });
            explained_lines.push(i);
        }
    }

    // Pass 2: embedding / deterministic-decoding call sites -> inapplicable.
    for (i, line) in lines.iter().enumerate() {
        if EMBEDDING_MARKER.is_match(line) {
            let name = nearest_name(&lines, i, &rel);
            out.push(InferenceAction {
                name,
                file: rel.clone(),
                line: i + 1,
                source_kind: SourceKind::ApiCall,
                temperature: TemperatureState::Inapplicable,
                context: line.trim().to_string(),
            });
            explained_lines.push(i);
        }
    }

    // Pass 3: recognized inference call sites with no nearby explicit temp
    // become "implicit/default" actions.
    for (i, line) in lines.iter().enumerate() {
        if explained_lines.contains(&i) {
            continue;
        }
        for (marker, kind) in CALL_MARKERS.iter() {
            if kind == &SourceKind::ConfigFile && !is_config_ext(ext) {
                continue;
            }
            if marker.is_match(line) {
                let window_start = i.saturating_sub(10);
                let window_end = (i + 10).min(lines.len());
                let nearby_has_temp = lines[window_start..window_end]
                    .iter()
                    .any(|l| EXPLICIT_TEMP.is_match(l) || BUILDER_TEMP.is_match(l));
                if nearby_has_temp {
                    break;
                }
                let name = nearest_name(&lines, i, &rel);
                out.push(InferenceAction {
                    name,
                    file: rel.clone(),
                    line: i + 1,
                    source_kind: *kind,
                    temperature: TemperatureState::Implicit,
                    context: line.trim().to_string(),
                });
                explained_lines.push(i);
                break;
            }
        }
    }

    // Pass 4: env var references without an inline default (e.g.
    // `os.environ.get("TEMPERATURE")`) become implicit actions too.
    for (i, line) in lines.iter().enumerate() {
        if explained_lines.contains(&i) {
            continue;
        }
        if let Some(cap) = ENV_TEMP_REF.captures(line) {
            out.push(InferenceAction {
                name: cap[1].to_string(),
                file: rel.clone(),
                line: i + 1,
                source_kind: SourceKind::EnvVar,
                temperature: TemperatureState::Implicit,
                context: line.trim().to_string(),
            });
        }
    }
}

fn is_config_ext(ext: &str) -> bool {
    matches!(ext, "yml" | "yaml" | "json" | "toml" | "cfg" | "ini")
}

fn classify_source_kind(path: &Path, ext: &str, is_workflow: bool, line: &str) -> SourceKind {
    if is_workflow {
        return SourceKind::WorkflowDef;
    }
    match ext {
        "sh" | "bash" | "zsh" => SourceKind::ShellWrapper,
        "env" => SourceKind::EnvVar,
        "yml" | "yaml" | "json" | "toml" | "cfg" | "ini" => {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if stem.contains("agent") || stem.contains("assistant") {
                SourceKind::AgentConfig
            } else if stem.contains("eval") || stem.contains("bench") {
                SourceKind::EvalScript
            } else if stem.contains("batch") {
                SourceKind::BatchJob
            } else {
                SourceKind::ConfigFile
            }
        }
        _ => {
            let lower_line = line.to_lowercase();
            if lower_line.contains("eval") || lower_line.contains("benchmark") {
                SourceKind::EvalScript
            } else if lower_line.contains("batch") {
                SourceKind::BatchJob
            } else if lower_line.contains("prompt_runner") || lower_line.contains("run_prompt") {
                SourceKind::PromptRunner
            } else {
                SourceKind::SdkInvocation
            }
        }
    }
}

/// Look backward from line `i` for the nearest identifier-bearing line to
/// use as a human-readable action name; fall back to `<file-stem>#<line>`.
fn nearest_name(lines: &[&str], i: usize, rel: &Path) -> String {
    let look_back = i.saturating_sub(25);

    // Prefer the nearest enclosing function/def/class/struct name — it's
    // a far more useful label than an arbitrary sibling key, and without
    // this pass a JS/JSON object field like `messages:` sitting between
    // the temperature line and its enclosing `function foo(...)` would
    // get picked up first by the key-hint pass below.
    for j in (look_back..=i).rev() {
        if let Some(cap) = NAME_HINT.captures(lines[j]) {
            return cap[1].to_string();
        }
    }

    for j in (look_back..=i).rev() {
        if let Some(cap) = JSON_KEY_HINT.captures(lines[j]) {
            let key = cap[1].trim();
            if !key.eq_ignore_ascii_case("temperature") && !key.is_empty() {
                return key.to_string();
            }
        }
        if let Some(cap) = YAML_KEY_HINT.captures(lines[j]) {
            let key = cap[1].trim();
            if key.eq_ignore_ascii_case("name") {
                let value = lines[j].split_once(':').map(|x| x.1).unwrap_or("").trim();
                let value = value.trim_matches(|c| c == '"' || c == '\'');
                if !value.is_empty() {
                    return value.to_string();
                }
            } else if !key.eq_ignore_ascii_case("temperature")
                && !key.eq_ignore_ascii_case("model")
                && !key.is_empty()
            {
                return key.to_string();
            }
        }
    }
    let stem = rel.file_stem().and_then(|s| s.to_str()).unwrap_or("action");
    format!("{stem}#{}", i + 1)
}
