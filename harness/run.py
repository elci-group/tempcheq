"""Impact benchmark runner.

Modes:
  dry-run  mock provider; validates the full pipeline; zero cost.
  live     OpenAI-compatible endpoint via TEMPCHEQ_BENCH_API_KEY /
           TEMPCHEQ_BENCH_BASE_URL / TEMPCHEQ_BENCH_MODEL.

Usage:
  python3 harness/run.py --profile smoke --mode dry-run
  python3 harness/run.py --profile pilot --mode live --yes
"""

import argparse
import hashlib
import json
import os
import random
import sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))

from harness import CLASSES, config
from harness import analyze, checkers, fixtures_gen, judge, report
from harness.conditions import condition_temps, oracle_grid
from harness.providers import MockProvider, provider_from_env

CONDS = ("C0", "C1", "C2")


def load_json(path: str, fallback: str, label: str) -> dict:
    for candidate in (path, fallback):
        if candidate and os.path.exists(candidate):
            with open(candidate, "r", encoding="utf-8") as fh:
                data = json.load(fh)
            print(f"[corpus] {label}: {candidate}")
            return data
    raise SystemExit(f"missing {label} (looked at {path}, {fallback})")


def assign_wild_temps(tasks: list, corpus: dict, seed: int) -> None:
    """Draw each task's as-found temperature from its class's harvested
    distribution (seeded; design section 4.1)."""
    rng = random.Random(f"{seed}:wild")
    for task in tasks:
        dist = corpus.get(task["class"], {}).get("found_temps", [])
        task["as_found"] = rng.choice(dist) if dist else None


def estimate_calls(cfg: dict) -> dict:
    single = len(CLASSES) * cfg["tasks_per_class"] * 3 * cfg["reps"]
    oracle = (len(CLASSES) * cfg["oracle_tasks_per_class"]
              * len(cfg["oracle_temps"]) * cfg["oracle_reps"])
    pipelines = len(config.PIPELINES) * cfg["pipeline_instances"] * 3 \
        * cfg["pipeline_reps"] * 3  # three conditions, three steps
    judge = 2 * cfg["judge_pairs"]
    return {"single_step": single, "oracle": oracle,
            "pipelines": pipelines, "judge": judge,
            "total": single + oracle + pipelines + judge}


def one_call(provider, cfg, cls, messages, gold, tag, cond, rep, temperature,
             seed, allow_exec, sample_answer, record_class, pipeline=None,
             is_final=False, quality_factor=1.0, step_index=None):
    seed_i = int(hashlib.sha256(f"{tag}:{cond}:{rep}:{seed}".encode()).hexdigest()[:8], 16)
    ctx = {"task_class": cls if not record_class.startswith("pipeline:") else cls,
           "ideal": cfg["_bands"][cls]["ideal"],
           "sample_answer": sample_answer,
           "quality_factor": quality_factor}
    result = provider.complete(messages, temperature, seed_i, ctx)
    passed, score, details = checkers.check(cls, result.text, gold, allow_exec)
    return {
        "task_id": tag,
        "class": record_class,
        "pipeline": pipeline,
        "is_final": bool(is_final),
        "step_index": step_index,
        "condition": cond,
        "rep": rep,
        "temperature": temperature,
        "effective_temperature": result.meta.get("temperature", temperature),
        "seed": seed_i,
        "answer": result.text[:2000],
        "check": {"passed": bool(passed), "score": score, "details": details},
        "provider": result.meta.get("provider"),
        "quality_draw": result.meta.get("quality_draw"),
    }


def execute(provider, cfg, suite, bands, seed, allow_exec, records, log):
    def emit(rec):
        records.append(rec)
        log.write(json.dumps(rec, sort_keys=True) + "\n")

    by_class = {}
    for task in suite:
        by_class.setdefault(task["class"], []).append(task)

    # Single-step tasks: C0/C1/C2 full grid, C3 oracle on a per-class subset.
    for cls in CLASSES:
        band = bands[cls]
        for task in by_class.get(cls, []):
            temps = condition_temps(task, band, cfg)
            for cond in CONDS:
                for rep in range(cfg["reps"]):
                    emit(one_call(provider, cfg, cls, task["messages"],
                                  task["gold"], task["id"], cond, rep, temps[cond],
                                  seed, allow_exec, task["sample_answer"], cls))
        for task in by_class.get(cls, [])[:cfg["oracle_tasks_per_class"]]:
            seen = set()
            for temp in oracle_grid(cfg["oracle_temps"]):
                effective = provider.effective_temperature(temp)
                if effective in seen:  # endpoint-clamped duplicates waste calls
                    continue
                seen.add(effective)
                for rep in range(cfg["oracle_reps"]):
                    emit(one_call(provider, cfg, cls, task["messages"],
                                  task["gold"], task["id"], "C3", rep, temp,
                                  seed, allow_exec, task["sample_answer"], cls))

    # Multi-step pipelines: per-step class temps, propagation via the mock
    # quality factor, completion judged on the final step.
    pipe_meta = {}
    rng_pipe = random.Random(f"{seed}:pipe")
    for pipeline in config.PIPELINES:
        prototype = fixtures_gen.build_pipeline(0, pipeline, seed)
        steps_classes = [s["class"] for s in prototype["steps"]]
        pipe_meta[pipeline["id"]] = {"steps": steps_classes}
        for instance in range(cfg["pipeline_instances"]):
            pipe = fixtures_gen.build_pipeline(instance, pipeline, seed)
            wild = {}
            for cls in dict.fromkeys(steps_classes):
                dist = cfg["_corpus"].get(cls, {}).get("found_temps", [])
                wild[cls] = rng_pipe.choice(dist) if dist else None
            for cond in CONDS:
                for rep in range(cfg["pipeline_reps"]):
                    failed = False
                    for step_index, step in enumerate(pipe["steps"]):
                        step_cls = step["class"]
                        task_view = {"id": f"{pipe['id']}-s{step_index}",
                                     "as_found": wild.get(step_cls)}
                        temps = condition_temps(task_view, bands[step_cls], cfg)
                        rec = one_call(
                            provider, cfg, step_cls, step["messages"], step["gold"],
                            f"{pipe['id']}-s{step_index}", cond, rep, temps[cond],
                            seed, allow_exec,
                            (lambda q, r, s=step: pipe["sample_answer"](q, r, s)),
                            f"pipeline:{pipeline['id']}",
                            pipeline=pipeline["id"],
                            is_final=(step_index == len(pipe["steps"]) - 1),
                            quality_factor=(0.6 if failed else 1.0),
                            step_index=step_index)
                        failed = failed or not rec["check"]["passed"]
                        emit(rec)
    return pipe_meta


def collect_judge_pairs(records, suite, limit):
    """(request, C1 answer, C0 answer) for checker-light quality families."""
    by_key = {}
    for r in records:
        if r["class"] in ("summarization", "creative") and r["rep"] == 0 \
                and r["condition"] in ("C0", "C1"):
            by_key.setdefault((r["class"], r["task_id"]), {})[r["condition"]] = r
    suite_by_id = {t["id"]: t for t in suite}
    pairs = []
    for (cls, task_id), both in sorted(by_key.items()):
        if "C0" in both and "C1" in both:
            task = suite_by_id.get(task_id)
            if not task:
                continue
            request = task["messages"][-1]["content"]
            pairs.append((request, both["C1"]["answer"], both["C0"]["answer"]))
        if len(pairs) >= limit:
            break
    return pairs


def run(args) -> int:
    cfg = config.config_for(args.profile)
    cfg["seed"] = args.seed
    live = args.mode == "live"
    if live:
        cfg["base_url"] = args.base_url
        cfg["model"] = args.model
    if args.mode == "dry-run":
        cfg["allow_code_exec"] = True  # fixture-authored code only
    else:
        cfg["allow_code_exec"] = args.allow_code_exec
    provider = provider_from_env(cfg, os.environ) if live else MockProvider(config.MOCK_QUALITY)

    bands = load_json(args.bands, "corpus/bands.json", "temperature bands")
    corpus = load_json(args.corpus, "corpus/profiles.sample.json", "corpus profiles")
    missing = [c for c in CLASSES if c not in bands]
    if missing:
        raise SystemExit(f"bands missing classes: {missing}; run corpus/harvest.py bands")
    cfg["_bands"] = bands
    cfg["_corpus"] = corpus

    suite = fixtures_gen.build_suite(cfg["tasks_per_class"], cfg["seed"])
    assign_wild_temps(suite, corpus, cfg["seed"])

    estimate = estimate_calls(cfg)
    print(f"[plan] profile={args.profile} mode={args.mode} "
          f"provider={provider.name} calls~{estimate['total']} {estimate}")
    if live and not args.yes:
        print("refusing to spend provider calls without --yes")
        return 2

    run_id = hashlib.sha256(json.dumps(
        {"profile": args.profile, "seed": cfg["seed"], "bands": bands,
         "corpus": corpus, "provider": provider.name, "model": cfg.get("model")},
        sort_keys=True).encode()).hexdigest()[:12]
    out_dir = os.path.join(args.out, run_id)
    os.makedirs(out_dir, exist_ok=True)

    records = []
    with open(os.path.join(out_dir, "calls.jsonl"), "w", encoding="utf-8") as log:
        pipe_meta = execute(provider, cfg, suite, bands, cfg["seed"],
                            cfg["allow_code_exec"], records, log)

    judge_stats = None
    if live and args.judge and cfg["judge_pairs"] > 0:
        pairs = collect_judge_pairs(records, suite, cfg["judge_pairs"])
        if pairs:
            try:
                judge_stats = judge.judge_win_rate(provider, pairs, cfg["seed"])
            except RuntimeError as err:
                print(f"[judge] skipped: {err}")

    class_analyses = {cls: analyze.uplift_analysis(records, cls, cfg, cfg["seed"])
                      for cls in CLASSES}
    condition_means = analyze.class_condition_means(records)
    clamp_calls = sum(1 for r in records
                      if abs(r.get("effective_temperature", r["temperature"])
                             - r["temperature"]) > 1e-9)
    oracle = {cls: analyze.oracle_analysis(records, cls, bands[cls],
                                           condition_means[cls]["C1"])
              for cls in CLASSES}
    fabrication = {cond: analyze.fabrication_rate(records, cond) for cond in CONDS}
    code_pass = {cond: analyze.pass_rate(records, "code-generation", cond) for cond in CONDS}
    pipeline_completion = analyze.pipeline_summary(records, pipe_meta)
    compound = analyze.compound_ratio(pipeline_completion, class_analyses, pipe_meta)
    hypotheses = analyze.hypotheses(class_analyses, oracle, fabrication, code_pass,
                                    compound, config.ACCEPTANCE)

    results = {
        "run_id": run_id,
        "profile": args.profile,
        "mode": args.mode,
        "provider": provider.name,
        "model": cfg.get("model") if live else None,
        "seed": cfg["seed"],
        "config": {k: v for k, v in cfg.items() if not k.startswith("_")},
        "estimate": estimate,
        "calls_recorded": len(records),
        "temp_clamp": {"applied": clamp_calls > 0, "calls": clamp_calls},
        "class_analyses": class_analyses,
        "condition_means": condition_means,
        "oracle": oracle,
        "fabrication": fabrication,
        "code_pass": code_pass,
        "pipelines": pipeline_completion,
        "compound": compound,
        "judge": judge_stats,
        "hypotheses": hypotheses,
        "disclaimer": ("mock provider: pipeline validation only, not outcome "
                       "evidence" if not live else None),
    }
    with open(os.path.join(out_dir, "results.json"), "w", encoding="utf-8") as fh:
        json.dump(results, fh, indent=2, sort_keys=True)
    md = report.render_markdown(results)
    with open(os.path.join(out_dir, "report.md"), "w", encoding="utf-8") as fh:
        fh.write(md)
    print(f"[done] {out_dir}/results.json, report.md, calls.jsonl")
    verdict = [ln for ln in md.splitlines() if ln.startswith("Verdict")]
    print(verdict[0] if verdict else "")
    return 0


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description="TempCheq impact benchmark")
    parser.add_argument("--profile", choices=list(config.PROFILES), default="smoke")
    parser.add_argument("--mode", choices=("dry-run", "live"), default="dry-run")
    parser.add_argument("--out", default="results")
    parser.add_argument("--bands", default="corpus/bands.json")
    parser.add_argument("--corpus", default="corpus/profiles.json",
                        help="harvested profiles (falls back to the committed "
                             "synthetic sample when absent)")
    parser.add_argument("--seed", type=int, default=config.DEFAULTS["seed"])
    parser.add_argument("--yes", action="store_true",
                        help="required for live runs (spends provider calls)")
    parser.add_argument("--allow-code-exec", action="store_true",
                        help="live-mode code-generation executor (sandbox "
                             "recommended; implied in dry-run)")
    parser.add_argument("--judge", dest="judge", action="store_true", default=False)
    parser.add_argument("--base-url", default=None)
    parser.add_argument("--model", default=None)
    args = parser.parse_args(argv)
    return run(args)


if __name__ == "__main__":
    sys.exit(main())
