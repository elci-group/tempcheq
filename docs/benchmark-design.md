# TempCheq Impact Benchmark — Design

Status: design / pre-registration. Harness v1 implemented (see
`harness/README.md`): deterministic fixtures for all nine classes, two
multi-step pipelines, programmatic checkers, mock dry-run mode, and a live
OpenAI-compatible adapter. v1 renders `results.json` + deterministic
`report.md`; the HTML report and the `temps` harvest over the full ~50-repo
corpus remain follow-ups. Target gap: `--benchmark` is currently a labeled
simulation seeded from the classification prior (`src/benchmark.rs`); CLAUDE.md
records that "there's no real eval hook wired in yet." This document specifies
that hook as a controlled experiment that measures whether TempCheq's
temperature recommendations actually change downstream outcomes — and by how
much.

The benchmark is external to the Rust crate: a Python harness that consumes
`tempcheq --report` JSON and drives real provider calls. TempCheq itself stays
a static-analysis tool; the harness is the measurement instrument.

---

## 1. Hypotheses

- **H1 — Uplift.** Call sites whose current temperature deviates from
  TempCheq's recommendation produce measurably better outputs when set to the
  recommended ideal. Primary claim.
- **H2 — Regret.** TempCheq's `band_ideal` is close to the empirically
  optimal temperature per task class: score(oracle temp) − score(recommended
  temp) is small. Measures recommendation quality, not just direction.
- **H3 — Sensitivity (instrument validation).** Scores degrade as
  temperature moves away from the recommendation, monotonically per task
  class. If H3 fails for a task family, that family is temperature-insensitive
  and is excluded from H1/H2 scoring (a suite that can't detect any
  temperature effect cannot certify impact).
- **H4 — Compounding.** Aligning temperature across every step of a
  multi-step process improves end-to-end completion more than the sum of
  single-step effects predicts (temperature error compounds across steps).

All hypotheses are paired, per-task, same model, same prompts, same seeds;
only temperature varies.

## 2. Conditions (independent variable)

Every task records a *call-site profile*: `(TaskClass, as_found_temperature,
tempcheq_ideal)`. As-found temperatures are harvested from real repositories
(§4.1), including implicit provider defaults, so the baseline reflects the
wild rather than a synthetic strawman.

| Cond | Name | Temperature per task | Purpose |
|------|------|----------------------|---------|
| C0 | wild | as-found value (or provider default when implicit) | baseline |
| C1 | tempcheq | `band_ideal` from `--report` JSON (constraint-adjusted, clamped to `[band_low, band_high]`) | treatment |
| C2 | adverse | band edge farthest from ideal, +0.2 further away, clamped to [0.0, 2.0] | negative control; must score worst or H3 fails |
| C3 | oracle | grid sweep {0.0, 0.1, …, 1.3} on a task subset | empirical optimum → H2 regret |

Expected ordering: **C2 < C0 ≤ C1**, with C1 ≈ C3.

The C1 value is taken verbatim from TempCheq output. No per-task manual
tuning is permitted — the harness must not overfit the suite it runs on.

## 3. Dimensions → operational metrics

Each dimension has one pre-registered primary endpoint and secondary
diagnostics. All verifiable metrics use programmatic checkers or gold
references; the LLM judge is used only where no checker exists, at
temperature 0, blind to condition, with position-swapped pairwise
presentation and a 5% human-audit sample to validate agreement.

| # | Dimension | Primary endpoint | Secondary diagnostics |
|---|-----------|------------------|------------------------|
| 1 | Response quality | blind pairwise judge win-rate, C1 vs C0 (position-swapped) | reference overlap (ROUGE-L / BERTScore) where gold exists; instruction-compliance checklist pass rate |
| 2 | Comparable multi-step processes | end-to-end completion rate on matched pipelines (identical step graph, prompts, seeds across conditions) | per-step pass matrix (localizes error propagation); conditional P(final fail \| step i fail) = amplification coefficient; cross-rep outcome variance |
| 3 | Artifact quality | machine-validated artifact rate: parses + schema-valid + required fields present (JSON Schema, HTML tidy, Markdown link check) | broken-reference rate; format rubric (judge); byte-determinism probe at temp 0 (mirrors TempCheq's own determinism guarantee) |
| 4 | Code correctness | pass@1 on held-out unit tests, sandboxed execution (no network, per-task timeout) | pass@5; compile rate; lint-clean rate; regression safety (fixture's existing tests still pass) |
| 5 | Hallucination rate | closed-world fabrication rate: any API symbol, identifier, or citation in the answer that is absent from the supplied context | abstention correctness on unanswerable probes (fabrication = inventing instead of abstaining); atomic-claim support ratio vs gold context |
| 6 | Problem solving | verifier-checked solve rate (programmatic checkers: math exact-match, constraint satisfaction, planning validation) | move-efficiency vs known optimal; first-attempt vs after-hint delta |

Hallucination tasks use **closed-world fixtures**: the context enumerates a
finite set of valid APIs/IDs/citations, generated fresh per batch with unique
synthetic names, so fabrication is decidable by set membership, not by
judgment. Unanswerable variants ask about entities deliberately absent from
the context; the only correct behavior is abstention.

## 4. Suite design

### 4.1 Corpus harvest (grounds C0 and stratification)

1. Run `tempcheq --report` over a curated corpus of ~50 public repos that
   make LLM calls (agents, RAG, extraction pipelines, creative tools).
2. Harvest every action's `(class, as_found_temperature | implicit,
   band_ideal, material_deviation)` into the condition table.
3. Measure the natural class distribution; stratify the suite by it (min 20
   tasks per class, including rare classes, so per-class numbers exist).
   Classes: deterministic-extraction, tool-selection, eval-judge,
   code-generation, summarization, conversational, reflection, synthesis,
   creative (fixed order from `classify::ALL_CLASSES`).

### 4.2 Task families (per class)

- **deterministic-extraction** — contract-grounded extraction against a JSON
  Schema over noisy documents; exact-match scoring.
- **tool-selection** — choose tools from a fixed registry to satisfy a goal;
  exact-match + invalid-tool rate.
- **eval-judge** — grade sample answers with known gold grades; grading
  agreement rate.
- **code-generation** — repo-style tasks (function + tests, bugfix + hidden
  tests); pass@1/pass@5 in sandbox.
- **summarization** — gold-reference summaries; faithfulness checkers.
- **conversational** — multi-turn persona/instruction maintenance; checklist
  compliance per turn.
- **reflection / synthesis** — multi-document synthesis with verifiable
  claim coverage; critique precision/recall against seeded defects.
- **creative** — constrained novelty tasks (unique naming within a banned
  list, constrained-format poetry); constraint pass + novelty metrics.

### 4.3 Multi-step pipelines (dimension 2)

Two canonical pipelines, fixed step graph, every step a real inference call:

1. `extract → summarize → synthesize → report` (3 classes composed)
2. `tool-select → execute → reflect → retry?` (agent loop, ≤ 3 iterations)

Each pipeline runs on 20 scenario instances. In C1 every step runs at its
class ideal; in C0 every step runs at its as-found wild temperature (the
realistic mixed setting); in C2 every step at adverse. Because step graphs,
prompts, and seeds are identical across conditions, differences are
attributable to temperature alone. Per-step telemetry (prompt/response hash,
temp, pass/fail) is recorded so H4's compounding can be decomposed.

## 5. Harness architecture

```
corpus/            # harvested (class, temp) profiles; fixture sources
fixtures/          # closed-world generators (deterministic, seeded)
harness/
  runner.py        # condition × task × rep executor; provider adapters
  checkers/        # programmatic verifiers per dimension
  judge.py         # frozen judge (temp 0, position-swap, condition-blind)
  stats.py         # paired tests, bootstrap CIs, Holm correction
  report.py        # results.json + deterministic report.md / report.html
results/           # append-only run logs; one JSON per call (hash, seed, temp)
```

- Input: `tempcheq <corpus> --report` JSON (`actions[].class`,
  `temperature`, `temperature_state`, `band_low/ideal/high`,
  `material_deviation`) — exactly the fields the tool already emits.
- Model: one primary model + one secondary model for generalizability;
  snapshot IDs pinned; provider `seed` used where supported; reps absorb
  nondeterminism elsewhere.
- Every call is logged with (condition, task, rep, temp, seed, response
  hash, latency, cost) enabling exact replay of the analysis.
- `report.py` output is deterministic (same rules as `src/export.rs`: no
  wall-clock, no HashMap iteration order) so result reports diff cleanly —
  consistent with TempCheq's own report philosophy.

## 6. Statistical plan (pre-registered)

- Unit of analysis: the task; paired observations across conditions; k = 5
  reps per (task, condition) for continuous metrics, k = 10 for binary rate
  metrics on small suites.
- Binary endpoints: McNemar (paired) + risk difference with bootstrap 95% CI.
  Continuous: paired Wilcoxon + bootstrap CI. Effect sizes reported, not just
  p-values.
- H3 gates H1/H2: a task family is scored only if C2 < C1 with CI excluding
  0 (temperature-sensitive). Insensitive families are reported, not hidden.
- Multiple comparisons: Holm–Bonferroni across the 6 dimension families
  within each hypothesis.
- Power: 20 tasks × 10 reps at p0 = 0.5 detects a ~13 pp risk difference at
  α = .05, power = .80 (two-sided). The pilot run re-estimates variance and
  may raise k before the full run; no other protocol changes after pilot.
- Oracle regret (H2) uses the best C3 grid point per task family; report
  both score-space regret (pp) and temperature-space |argmax − band_ideal|.

## 7. Confound controls and threats

- Only temperature varies; prompts, system messages, max_tokens, stop
  sequences are frozen and versioned in the repo.
- Judge blindness: transcripts stripped of condition labels; judge never sees
  temperatures; execution order across conditions is seeded-shuffled to
  decorrelate provider drift over time.
- Contamination: closed-world fixtures regenerate unique identifiers per
  batch; code tasks use private hidden tests.
- Recommendation overfit: C1 comes from TempCheq's general keyword heuristics
  over the *harvested corpus*, not from the benchmark fixtures — the fixture
  repos must not be in the harvest corpus.
- Known limitation carried over from the tool: classification confidence and
  the ±10-line context window mean C1 for a misclassified call site is
  intentionally mis-set. That is in scope — the benchmark measures the tool
  as it behaves, and per-class breakdowns expose where misclassification
  hurts (e.g. code-generation bands applied to creative tasks).
- LLM-judge residual bias: mitigated by position swap, temp 0, human audit
  sample; checker-verifiable dimensions are primary for that reason.

## 8. Cost estimate (primary model, single run)

| Block | Calls |
|-------|-------|
| Single-step: 9 classes × 20 tasks × {C0, C1, C2} × 10 reps | 5,400 |
| Oracle grid: 9 classes × 10 tasks × 14 temps × 3 reps | 3,780 |
| Pipelines: 2 × 20 instances × 4 conds × 5 reps × ~3.5 calls | 2,800 |
| Judge + audit overhead | ~2,000 |
| **Total** | **~14,000** (~×2 for the secondary model) |

## 9. Acceptance criteria (impact claim)

TempCheq demonstrably improves outcomes if, after correction:

1. **H1:** significant paired uplift of C1 over C0 (aggregate), median
   per-class risk difference ≥ +5 pp on ≥ 6 of 9 temperature-sensitive
   classes.
2. **H2:** median oracle regret ≤ 3 pp of score, and median
   |argmax_temp − band_ideal| ≤ 0.10.
3. **H3:** monotone degradation with |Δtemp| from ideal (negative
   Spearman correlation) in ≥ 7 of 9 classes.
4. **H4:** pipeline completion uplift of C1 over C0 ≥ 1.0× the summed
   single-step uplifts, or per-step telemetry shows propagating error
   reduction.

Hallucination and code-correctness dimensions must each show the same
direction of improvement; a significant regression on any dimension is a
fail and must be reported (e.g. conversational/creative classes may prefer
higher temperatures than the prior band — that finding feeds back into
`prior_band()` in `src/classify.rs`).

## 10. Implementation phases

1. **M0 — Corpus harvest.** Curate repos, run `--report`, build condition
   table + class distribution. Output: `corpus/profiles.json`.
2. **M1 — Fixtures.** Closed-world generators + task families with
   checkers; each checker ships with unit tests (this also seeds the crate's
   missing test discipline, one level up).
3. **M2 — Harness.** Runner, provider adapters, logging, judge; dry-run
   mode (fixtures + mock provider) with golden outputs.
4. **M3 — Pilot.** 2 classes × 10 tasks; re-estimate variance/k; freeze
   protocol.
5. **M4 — Full run.** Both models; results.json + deterministic report.
6. **M5 — Analysis.** Pre-registered stats; write findings back into
   `prior_band()` tuning and the README's limitations section.

Later integration: `harness/` becomes the live backend for
`tempcheq --benchmark --eval`, replacing the simulated perturbation curve
with measured curves when an API key is present.
