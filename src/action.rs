use std::path::PathBuf;

/// A single place in the workspace where an inference call's sampling
/// temperature is set, defaulted, or structurally inapplicable.
#[derive(Debug, Clone)]
pub struct InferenceAction {
    pub name: String,
    pub file: PathBuf,
    pub line: usize,
    pub source_kind: SourceKind,
    pub temperature: TemperatureState,
    pub context: String,
}

/// What kind of artifact this action was discovered in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    ApiCall,
    SdkInvocation,
    AgentConfig,
    ShellWrapper,
    EnvVar,
    ConfigFile,
    WorkflowDef,
    PromptRunner,
    BatchJob,
    EvalScript,
}

impl SourceKind {
    pub fn label(&self) -> &'static str {
        match self {
            SourceKind::ApiCall => "api-call",
            SourceKind::SdkInvocation => "sdk-invocation",
            SourceKind::AgentConfig => "agent-config",
            SourceKind::ShellWrapper => "shell-wrapper",
            SourceKind::EnvVar => "env-var",
            SourceKind::ConfigFile => "config-file",
            SourceKind::WorkflowDef => "workflow-def",
            SourceKind::PromptRunner => "prompt-runner",
            SourceKind::BatchJob => "batch-job",
            SourceKind::EvalScript => "eval-script",
        }
    }
}

#[derive(Debug, Clone)]
pub enum TemperatureState {
    /// An explicit numeric value was found.
    Explicit(f32),
    /// The call site is a recognized inference action but no temperature
    /// was set, so the provider default applies.
    Implicit,
    /// A temperature parameter is structurally meaningless here (e.g. a
    /// tool-only/deterministic-decoding endpoint, embeddings call, or a
    /// call already pinned to temperature=0 by an unconfigurable path).
    Inapplicable,
}

impl TemperatureState {
    pub fn as_display(&self) -> String {
        match self {
            TemperatureState::Explicit(v) => format!("{:.2}", v),
            TemperatureState::Implicit => "default".to_string(),
            TemperatureState::Inapplicable => "n/a".to_string(),
        }
    }

    pub fn value(&self) -> Option<f32> {
        match self {
            TemperatureState::Explicit(v) => Some(*v),
            _ => None,
        }
    }
}
