"""Deterministic Markdown rendering of results. No timestamps, fixed class
order, sorted keys — two runs from identical inputs diff as empty (except
run_id-scoped content)."""

from harness import CLASSES, config


def _fmt(x, suffix=""):
    if x is None:
        return "n/a"
    if isinstance(x, float):
        return f"{x:+.2f}{suffix}" if suffix == "pp" else f"{x:.2f}{suffix}"
    return f"{x}{suffix}"


def render_markdown(r: dict) -> str:
    lines = []
    lines.append(f"# TempCheq Impact Benchmark — run `{r['run_id']}`")
    lines.append("")
    lines.append(f"- Profile: `{r['profile']}` | Mode: `{r['mode']}` | "
                 f"Provider: `{r['provider']}`"
                 + (f" | Model: `{r['model']}`" if r.get("model") else ""))
    lines.append(f"- Seed: {r['seed']} | Calls recorded: {r['calls_recorded']} "
                 f"(estimate {r['estimate']['total']})")
    if r.get("disclaimer"):
        lines.append(f"- **Disclaimer:** {r['disclaimer']}")
    if r.get("temp_clamp", {}).get("applied"):
        lines.append(f"- **Endpoint temperature clamp applied on "
                     f"{r['temp_clamp']['calls']} calls** (requested > endpoint max); "
                     f"saturation reduces contrast for those conditions — see "
                     f"effective_temperature in calls.jsonl")
    lines.append("")
    lines.append("## Hypotheses (design doc section 9 thresholds)")
    lines.append("")
    h = r["hypotheses"]
    for key, label in (("H1_uplift", "H1 uplift (C1 > C0)"),
                       ("H2_regret", "H2 oracle regret"),
                       ("H3_sensitivity", "H3 temperature sensitivity"),
                       ("H4_compounding", "H4 multi-step compounding")):
        entry = h[key]
        verdict = "PASS" if entry["pass"] else "FAIL"
        lines.append(f"- **{verdict}** — {label}: {_summarize(key, entry)}")
    extra = []
    if h.get("hallucination_direction_ok") is not None:
        extra.append(f"hallucination direction {'ok' if h['hallucination_direction_ok'] else 'REGRESSED'}")
    if h.get("code_direction_ok") is not None:
        extra.append(f"code-correctness direction {'ok' if h['code_direction_ok'] else 'REGRESSED'}")
    if extra:
        lines.append(f"- Guards: {'; '.join(extra)}")
    lines.append("")
    lines.append("## Per-class analysis (C1 vs C0 uplift)")
    lines.append("")
    lines.append("| Class | n | C0 mean | C1 mean | C2 mean | Uplift (pp) | 95% CI (pp) | Wilcoxon p | Sensitive |")
    lines.append("|-------|---|---------|---------|---------|-------------|-------------|------------|-----------|")
    for cls in CLASSES:
        a = r["class_analyses"][cls]
        c1 = a.get("C1_vs_C0")
        # Condition means come from the aggregated record set.
        means = r.get("condition_means", {}).get(cls, {})
        if not c1:
            lines.append(f"| {cls} | 0 | n/a | n/a | n/a | n/a | n/a | n/a | {a['temperature_sensitive']} |")
            continue
        ci = c1["ci95_pp"]
        lines.append(
            f"| {cls} | {means.get('n', 'n/a')} | {means.get('C0', 'n/a')} "
            f"| {means.get('C1', 'n/a')} | {means.get('C2', 'n/a')} "
            f"| {c1['mean_delta_pp']:+.2f} | [{ci[0]:+.2f}, {ci[1]:+.2f}] "
            f"| {c1['wilcoxon_p']:.4f} | {'yes' if a['temperature_sensitive'] else 'no'} |")
    lines.append("")
    lines.append("## Oracle grid (C3): regret vs TempCheq ideal")
    lines.append("")
    lines.append("| Class | Best temp | Best mean | Regret (pp) | Temp regret | Monotone (Spearman) |")
    lines.append("|-------|-----------|-----------|-------------|-------------|---------------------|")
    for cls in CLASSES:
        o = r["oracle"][cls]
        if not o:
            lines.append(f"| {cls} | n/a | n/a | n/a | n/a | n/a |")
            continue
        lines.append(
            f"| {cls} | {o['best_temp']:.2f} | {o['best_mean']:.3f} "
            f"| {_fmt(o['regret_pp'])} | {o['temp_regret']:.2f} "
            f"| {o['monotone_spearman']:+.2f} ({'yes' if o['monotone'] else 'no'}) |")
    lines.append("")
    fab = r["fabrication"]
    if any(v is not None for v in fab.values()):
        lines.append("## Hallucination (fabrication rate)")
        lines.append("")
        lines.append("| Condition | Eligible | Fabrication rate |")
        lines.append("|-----------|----------|------------------|")
        for cond in ("C0", "C1", "C2"):
            v = fab.get(cond)
            lines.append(f"| {cond} | {v['eligible'] if v else 'n/a'} "
                         f"| {v['rate'] if v else 'n/a'} |")
        lines.append("")
    if any(v is not None for v in r["code_pass"].values()):
        lines.append("## Code correctness (pass@1)")
        lines.append("")
        lines.append("| Condition | Pass rate |")
        lines.append("|-----------|-----------|")
        for cond in ("C0", "C1", "C2"):
            v = r["code_pass"].get(cond)
            lines.append(f"| {cond} | {v if v is not None else 'n/a'} |")
        lines.append("")
    lines.append("## Multi-step pipelines")
    lines.append("")
    for pipe_id, meta in r["pipelines"].items():
        comp = meta["completion"]
        comp_s = ", ".join(f"{c}: {comp[c]:.2f}" for c in ("C0", "C1", "C2") if c in comp)
        lines.append(f"- `{pipe_id}` completion — {comp_s or 'n/a'}")
        ratio = r["compound"].get(pipe_id)
        if ratio:
            lines.append(f"  - uplift {ratio['pipeline_uplift_pp']:+.2f} pp vs summed "
                         f"steps {ratio['summed_step_uplift_pp']:+.2f} pp "
                         f"(ratio {ratio['ratio']})")
    if r.get("judge"):
        j = r["judge"]
        lines.append("")
        lines.append(f"## Blind judge (position-swapped): wins {j['wins']}, "
                     f"losses {j['losses']}, ties {j['ties']}, "
                     f"win-rate {j['win_rate']}")
    lines.append("")
    acc = config.ACCEPTANCE
    all_pass = all(r["hypotheses"][k]["pass"]
                   for k in ("H1_uplift", "H2_regret", "H3_sensitivity",
                             "H4_compounding"))
    underpowered = r["profile"] in ("smoke",) or r["mode"] == "dry-run"
    lines.append(f"Verdict: {'IMPACT SUPPORTED' if all_pass else 'IMPACT NOT SUPPORTED'} "
                 f"by this run (thresholds: ≥{acc['min_uplift_classes']} sensitive classes "
                 f"at p<{acc['alpha']}, median uplift ≥{acc['median_uplift_pp']}pp, "
                 f"regret ≤{acc['median_regret_pp']}pp, monotone ≥"
                 f"{acc['min_monotone_classes']}/9, compound ratio ≥"
                 f"{acc['pipeline_compound_ratio']}).")
    if underpowered:
        lines.append("Note: this run is underpowered by design (smoke/dry-run) — "
                     "outcome claims require a live pilot or full profile.")
    return "\n".join(lines)


def _summarize(key, entry):
    if key == "H1_uplift":
        return (f"{entry['significant_classes']} sensitive classes significant, "
                f"median uplift {entry['median_uplift_pp']} pp")
    if key == "H2_regret":
        return (f"median regret {entry['median_regret_pp']} pp, "
                f"median temp regret {entry['median_temp_regret']}")
    if key == "H3_sensitivity":
        return (f"{entry['monotone_classes']}/9 monotone, "
                f"{entry['sensitive_classes']}/9 sensitivity-gated in")
    if key == "H4_compounding":
        ratios = [v['ratio'] for v in entry['ratios'].values() if v['ratio']]
        return f"ratios {[round(x, 2) for x in ratios] if ratios else 'n/a'}"
    return ""
