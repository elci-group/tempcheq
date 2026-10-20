"""Aggregation and hypothesis evaluation over run records."""

import statistics

from harness import CLASSES, clamp01
from harness.stats import (mcnemar_exact, paired_bootstrap_ci, risk_difference,
                           spearman, wilcoxon_signed_rank)


def _scores(records, cls, cond):
    return [r["check"]["score"] for r in records
            if r["class"] == cls and r["condition"] == cond]


def _paired(records, cls, cond_a, cond_b):
    """Deltas (a − b) matched by (task, rep); only tasks present in both."""
    index = {}
    for r in records:
        if r["class"] == cls and r["condition"] in (cond_a, cond_b):
            index.setdefault((r["task_id"], r["rep"]), {})[r["condition"]] = r
    d_scores, passes_a, passes_b = [], [], []
    n01 = n10 = 0  # b pass & a fail / a pass & b fail
    for key in sorted(index):
        pair = index[key]
        if cond_a not in pair or cond_b not in pair:
            continue
        ra, rb = pair[cond_a], pair[cond_b]
        d_scores.append(ra["check"]["score"] - rb["check"]["score"])
        pa, pb = ra["check"]["passed"], rb["check"]["passed"]
        passes_a.append(1 if pa else 0)
        passes_b.append(1 if pb else 0)
        if pb and not pa:
            n01 += 1
        if pa and not pb:
            n10 += 1
    return d_scores, (passes_a, passes_b), n01, n10


def class_condition_means(records):
    """Mean score per class per condition, for the report table."""
    out = {}
    for cls in CLASSES:
        entry, total = {}, 0
        for cond in ("C0", "C1", "C2"):
            scores = _scores(records, cls, cond)
            total = max(total, len(scores))
            entry[cond] = round(statistics.fmean(scores), 3) if scores else None
        entry["n"] = total
        out[cls] = entry
    return out


def condition_summary(records, cls, cond):
    scores = _scores(records, cls, cond)
    if not scores:
        return None
    passes = [r["check"]["passed"] for r in records
              if r["class"] == cls and r["condition"] == cond]
    return {"n": len(scores),
            "mean_score": round(statistics.fmean(scores), 4),
            "pass_rate": round(sum(1 for p in passes if p) / len(passes), 4)}


def uplift_analysis(records, cls, cfg, seed):
    """C1 vs C0 (treatment uplift) and C2 vs C1 (sensitivity gate)."""
    out = {"C1_vs_C0": None, "C2_vs_C1": None}
    d, passes, n01, n10 = _paired(records, cls, "C1", "C0")
    if d:
        lo, hi = paired_bootstrap_ci(d, cfg["bootstrap_draws"], seed)
        _, p = wilcoxon_signed_rank(d)
        pa, pb = passes
        out["C1_vs_C0"] = {
            "mean_delta_pp": round(statistics.fmean(d) * 100, 2),
            "ci95_pp": [round(lo * 100, 2), round(hi * 100, 2)],
            "wilcoxon_p": p,
            "mcnemar_p": mcnemar_exact(n01, n10),
            "risk_difference": round(risk_difference(sum(pa), len(pa),
                                                     sum(pb), len(pb)), 4),
        }
    d2, _, n01b, n10b = _paired(records, cls, "C2", "C1")
    if d2:
        lo2, hi2 = paired_bootstrap_ci(d2, cfg["bootstrap_draws"], seed + 1)
        _, p2 = wilcoxon_signed_rank(d2)
        out["C2_vs_C1"] = {
            "mean_delta_pp": round(statistics.fmean(d2) * 100, 2),
            "ci95_pp": [round(lo2 * 100, 2), round(hi2 * 100, 2)],
            "wilcoxon_p": p2,
            "mcnemar_p": mcnemar_exact(n01b, n10b),
        }
        out["temperature_sensitive"] = bool(hi2 < 0)
    else:
        out["temperature_sensitive"] = False
    return out


def oracle_analysis(records, cls, band, c1_mean=None):
    """C3 grid: empirical optimum, regret vs C1 (single-step mean at the
    recommended ideal), monotonicity (H3/H2)."""
    by_temp = {}
    for r in records:
        if r["class"] == cls and r["condition"] == "C3":
            key = r.get("effective_temperature", r["temperature"])
            by_temp.setdefault(key, []).append(r["check"]["score"])
    if not by_temp:
        return None
    curve = [{"temp": t, "mean": round(statistics.fmean(v), 4)}
             for t, v in sorted(by_temp.items())]
    best = max(curve, key=lambda c: c["mean"])
    c1_temp = band["ideal"]
    deltas, means = [], []
    for c in curve:
        deltas.append(abs(c["temp"] - c1_temp))
        means.append(c["mean"])
    rho = spearman(deltas, means)
    return {"curve": curve, "best_temp": best["temp"], "best_mean": best["mean"],
            "regret_pp": (round((best["mean"] - c1_mean) * 100, 2)
                          if c1_mean is not None else None),
            "temp_regret": round(abs(best["temp"] - c1_temp), 4),
            "monotone_spearman": round(rho, 4),
            "monotone": bool(rho < 0)}


def fabrication_rate(records, cond):
    """Hallucination endpoint: fabricated fields (extraction) + forbidden
    claims (synthesis), as a rate over eligible calls."""
    eligible = fabrication_hits = 0
    for r in records:
        if r["condition"] != cond:
            continue
        details = r["check"].get("details", {})
        hit = ("fabricated" in details and details["fabricated"] > 0) or \
              ("forbidden_hits" in details and details["forbidden_hits"])
        if "fabricated" in details or "forbidden_hits" in details:
            eligible += 1
            fabrication_hits += 1 if hit else 0
    if not eligible:
        return None
    return {"eligible": eligible, "rate": round(fabrication_hits / eligible, 4)}


def pass_rate(records, cls, cond):
    recs = [r for r in records if r["class"] == cls and r["condition"] == cond]
    if not recs:
        return None
    return round(sum(1 for r in recs if r["check"]["passed"]) / len(recs), 4)


def pipeline_summary(records, pipelines_meta):
    """Completion rate per condition + compounding ratio (H4)."""
    out = {}
    for pipe_id in pipelines_meta:
        per_cond = {}
        for cond in ("C0", "C1", "C2"):
            finals = [r for r in records if r.get("pipeline") == pipe_id
                      and r["condition"] == cond and r.get("is_final")]
            if finals:
                per_cond[cond] = round(sum(1 for r in finals
                                           if r["check"]["passed"]) / len(finals), 4)
        out[pipe_id] = {"completion": per_cond}
    return out


def compound_ratio(pipeline_completion, single_class_uplift, pipes_meta):
    """H4: pipeline completion uplift (pp) vs summed single-step uplifts (pp)."""
    ratios = {}
    for pipe_id, meta in pipeline_completion.items():
        comp = meta["completion"]
        steps = pipes_meta.get(pipe_id, {}).get("steps", [])
        if not all(c in comp for c in ("C0", "C1")):
            continue
        pipe_uplift_pp = (comp["C1"] - comp["C0"]) * 100
        step_uplift = 0.0
        for cls in dict.fromkeys(steps):
            u = single_class_uplift.get(cls)
            if u and u["C1_vs_C0"]:
                step_uplift += max(0.0, u["C1_vs_C0"]["mean_delta_pp"])
        ratios[pipe_id] = {
            "pipeline_uplift_pp": round(pipe_uplift_pp, 2),
            "summed_step_uplift_pp": round(step_uplift, 2),
            "ratio": round(pipe_uplift_pp / step_uplift, 4) if step_uplift > 0 else None,
        }
    return ratios


def hypotheses(class_analyses, oracle, fabrication, code_pass, compound,
               acceptance):
    """Evaluate design-doc section 9 acceptance criteria."""
    sensitive = {c: a for c, a in class_analyses.items() if a["temperature_sensitive"]}
    uplifts = [a["C1_vs_C0"]["mean_delta_pp"] for a in sensitive.values()
               if a["C1_vs_C0"]]
    med_uplift = statistics.median(uplifts) if uplifts else None
    significant = sum(1 for a in sensitive.values()
                      if a["C1_vs_C0"] and a["C1_vs_C0"]["wilcoxon_p"] < acceptance["alpha"]
                      and a["C1_vs_C0"]["mean_delta_pp"] >= 0)
    h1 = (med_uplift is not None
          and significant >= acceptance["min_uplift_classes"]
          and (med_uplift or 0) >= acceptance["median_uplift_pp"])
    regrets = [o["regret_pp"] for o in oracle.values()
               if o and o["regret_pp"] is not None]
    t_regrets = [o["temp_regret"] for o in oracle.values() if o]
    med_regret = statistics.median(regrets) if regrets else None
    med_t_regret = statistics.median(t_regrets) if t_regrets else None
    h2 = (med_regret is not None and med_regret <= acceptance["median_regret_pp"]
          and med_t_regret is not None and med_t_regret <= acceptance["median_temp_regret"])
    monotone = sum(1 for o in oracle.values() if o and o["monotone"])
    h3 = monotone >= acceptance["min_monotone_classes"]
    ratios = [v["ratio"] for v in compound.values() if v["ratio"] is not None]
    h4 = bool(ratios) and all(r >= acceptance["pipeline_compound_ratio"] for r in ratios)
    fab_rates = {c: (v["rate"] if v else None) for c, v in fabrication.items()}
    return {
        "H1_uplift": {"pass": h1, "significant_classes": significant,
                      "median_uplift_pp": med_uplift},
        "H2_regret": {"pass": h2, "median_regret_pp": med_regret,
                      "median_temp_regret": med_t_regret},
        "H3_sensitivity": {"pass": h3, "monotone_classes": monotone,
                           "sensitive_classes": len(sensitive)},
        "H4_compounding": {"pass": h4, "ratios": compound},
        "hallucination_direction_ok": _direction_ok(fab_rates, lower_is_better=True),
        "code_direction_ok": _direction_ok(code_pass, lower_is_better=False),
    }


def _direction_ok(per_cond, lower_is_better):
    if not all(k in per_cond and per_cond[k] is not None for k in ("C0", "C1")):
        return None
    delta = per_cond["C1"] - per_cond["C0"]
    return (delta <= 0) if lower_is_better else (delta >= 0)
