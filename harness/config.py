"""Run profiles and defaults for the impact benchmark."""

import copy

# Per-class mock quality model parameters (dry-run only).
# base: quality at the recommended ideal; sensitivity: quality loss per
# unit absolute temperature deviation; noise: uniform draw amplitude.
# These simulate temperature sensitivity so the analysis plumbing can be
# validated end-to-end without provider cost. They are NOT evidence.
MOCK_QUALITY = {
    "deterministic-extraction": {"base": 0.90, "sensitivity": 1.10, "noise": 0.05},
    "tool-selection": {"base": 0.88, "sensitivity": 1.00, "noise": 0.06},
    "eval-judge": {"base": 0.86, "sensitivity": 0.95, "noise": 0.06},
    "code-generation": {"base": 0.80, "sensitivity": 0.70, "noise": 0.07},
    "summarization": {"base": 0.84, "sensitivity": 0.60, "noise": 0.07},
    "conversational": {"base": 0.82, "sensitivity": 0.35, "noise": 0.08},
    "reflection": {"base": 0.78, "sensitivity": 0.45, "noise": 0.08},
    "synthesis": {"base": 0.76, "sensitivity": 0.40, "noise": 0.08},
    "creative": {"base": 0.80, "sensitivity": 0.30, "noise": 0.09},
}

# Multi-step pipelines: ordered steps, each mapped to a task class so the
# condition resolver can set per-step temperatures.
PIPELINES = [
    {
        "id": "order-decision",
        "steps": ["deterministic-extraction", "summarization", "synthesis"],
        "final": "artifact-quality",
    },
    {
        "id": "refund-loop",
        "steps": ["tool-selection", "reflection", "tool-selection"],
        "final": "problem-solving",
    },
]

PROFILES = {
    # Pipeline validation only; free (mock provider).
    "smoke": {
        "tasks_per_class": 2,
        "reps": 2,
        "oracle_tasks_per_class": 2,
        "oracle_temps": [0.0, 0.4, 0.8],
        "oracle_reps": 1,
        "pipeline_instances": 2,
        "pipeline_reps": 1,
        "judge_pairs": 0,
    },
    # M3 pilot in the design doc: re-estimates variance before a full run.
    "pilot": {
        "tasks_per_class": 10,
        "reps": 5,
        "oracle_tasks_per_class": 6,
        "oracle_temps": [0.0, 0.2, 0.4, 0.6, 0.8, 1.0],
        "oracle_reps": 2,
        "pipeline_instances": 6,
        "pipeline_reps": 3,
        "judge_pairs": 30,
    },
    # Design doc section 8 cost table.
    "full": {
        "tasks_per_class": 20,
        "reps": 10,
        "oracle_tasks_per_class": 10,
        "oracle_temps": [round(0.1 * i, 1) for i in range(14)],
        "oracle_reps": 3,
        "pipeline_instances": 20,
        "pipeline_reps": 5,
        "judge_pairs": 100,
    },
}

DEFAULTS = {
    "provider_default_temperature": 1.0,  # C0 fallback for implicit call sites
    "adverse_extra": 0.2,
    "temp_clamp": [0.0, 2.0],
    "max_tokens": 512,
    "seed": 20261007,
    "bootstrap_draws": 2000,
    "request_timeout_s": 60,
    "retries": 3,
    "allow_code_exec": False,  # code-gen checker requires explicit opt-in
}

ACCEPTANCE = {
    "min_uplift_classes": 6,          # of 9 (design section 9.1)
    "median_uplift_pp": 5.0,
    "median_regret_pp": 3.0,          # design section 9.2
    "median_temp_regret": 0.10,
    "min_monotone_classes": 7,        # design section 9.3
    "pipeline_compound_ratio": 1.0,   # design section 9.4
    "alpha": 0.05,
}


def profile(name: str) -> dict:
    return copy.deepcopy(PROFILES[name])


def config_for(name: str) -> dict:
    cfg = dict(DEFAULTS)
    cfg.update(profile(name))
    cfg["profile"] = name
    return cfg
