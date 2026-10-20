"""TempCheq Impact Benchmark — harness package.

External Python harness specified by docs/benchmark-design.md. Consumes
`tempcheq --report` output (conditions), drives real or mock provider calls,
verifies outputs with programmatic checkers, and emits deterministic
results.json + report.md artifacts.

Standard library only.
"""

CLASSES = [
    "deterministic-extraction",
    "tool-selection",
    "eval-judge",
    "code-generation",
    "summarization",
    "conversational",
    "reflection",
    "synthesis",
    "creative",
]

DIMENSIONS = [
    "response-quality",
    "multi-step",
    "artifact-quality",
    "code-correctness",
    "hallucination",
    "problem-solving",
]

# Fixed class order everywhere output is rendered (mirrors the Rust side's
# classify::ALL_CLASSES discipline: no dict-order randomness).


def clamp01(x: float) -> float:
    return max(0.0, min(1.0, x))


def clamp(x: float, lo: float, hi: float) -> float:
    return max(lo, min(hi, x))
