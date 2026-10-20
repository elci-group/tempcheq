# Impact Benchmark Harness

Implementation of `docs/benchmark-design.md` (M1–M2 milestones). Stdlib-only
Python 3.12; no third-party packages.

## Layout

```
harness/
  run.py           entry point: dry-run / live orchestration
  config.py        profiles (smoke | pilot | full), acceptance thresholds
  conditions.py    C0 wild / C1 tempcheq / C2 adverse / C3 oracle resolution
  providers.py     MockProvider (dry-run) + OpenAI-compatible adapter (live)
  fixtures_gen.py  deterministic closed-world task suites + 2 pipelines
  checkers.py      programmatic verifiers (no LLM judgment)
  judge.py         blind position-swapped pairwise judge (live, optional)
  stats.py         McNemar, Wilcoxon, bootstrap CI, Holm, Spearman
  analyze.py       per-class uplift, sensitivity gate, regret, hypotheses
  report.py        deterministic results.json + report.md rendering
  tests/           unittest suite (19 tests)
corpus/
  harvest.py       probes tempcheq for bands; harvests as-found temps
  bands.json       C1 ground truth (verbatim from the installed binary)
  profiles.sample.json  labeled synthetic wild-temperature distribution
results/           run artifacts (results.json, report.md, calls.jsonl)
```

## Usage

Dry run (free, mock provider, validates the full pipeline):

```bash
python3 harness/run.py --profile smoke --mode dry-run
python3 harness/run.py --profile pilot --mode dry-run
```

Live run (spends provider calls; `--yes` required):

```bash
export TEMPCHEQ_BENCH_API_KEY=...        # required
export TEMPCHEQ_BENCH_BASE_URL=https://api.openai.com/v1   # or any OpenAI-compatible endpoint
export TEMPCHEQ_BENCH_MODEL=gpt-4o-mini  # or your endpoint's model id
python3 harness/run.py --profile pilot --mode live --yes
# add --judge for the blind pairwise quality endpoint (2 calls per pair)
```

Call estimates per profile are printed before execution and recorded in
`results.json` (`estimate`). Design-doc cost table corresponds to `full`
(~14k calls/model). The harness never sends gold references, temperatures,
or condition ids to the provider; every call is logged to `calls.jsonl`
with `(task, condition, rep, temperature, seed, answer, check)` for replay.

## Live code-generation checker

Executing model-written code is opt-in in live mode (`--allow-code-exec`)
and runs in a stripped-env subprocess with a 5 s timeout. Run inside a
container for untrusted providers. Dry-run executes only fixture-authored
code and enables the checker implicitly.

## Corpus

`corpus/harvest.py bands` regenerates `bands.json` from the installed
tempcheq (C1 is taken verbatim from the tool's output — no manual tuning).
`corpus/harvest.py temps <repo...>` builds `corpus/profiles.json` from real
repositories; until then the committed `profiles.sample.json` (labeled
synthetic) drives C0. C0 wild temps are drawn per task, seeded.

## v1 scope notes

- Reports: `results.json` + deterministic `report.md` (no timestamps; fixed
  class order). HTML rendering is a follow-up.
- Judge: implemented, live-mode only; mock mode relies on checker-backed
  endpoints by design.
- Mock provider is circular by construction (quality = f(|temp − ideal|)):
  it validates the analysis plumbing and the sensitivity gate, never outcome
  claims. Dry-run reports carry the disclaimer and an underpowered note.
- Wilcoxon uses a normal approximation; full-protocol claims should use the
  `full` profile where n satisfies the approximation's assumptions.
