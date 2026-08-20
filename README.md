# TempCheq

TempCheq is a Rust CLI that audits a codebase for LLM inference "temperature" settings and
recommends what each one *should* be, based on the kind of task the call site is performing.

It is not a temperature setter. It's a static-analysis tool: scan a repo, find every place a
sampling temperature is set (or silently defaulted), classify what that call is for, and flag
values that don't match the task. A JSON-extraction call and a brainstorming call may hit the same
model, but they have very different sane temperature ranges — TempCheq is built around that
distinction rather than a single "right" number.

## Install

```bash
git clone https://github.com/elci-group/tempcheq.git
cd tempcheq
cargo build --release
# binary at target/release/tempcheq
```

Requires a recent stable Rust toolchain. No other runtime dependencies.

## Usage

```bash
tempcheq                 # audit the current directory
tempcheq <path>           # audit a specific workspace
tempcheq --explain        # show the reasoning behind each recommendation
tempcheq --benchmark       # perturb candidate temperatures, print a (simulated) score curve
tempcheq --fix             # preview rewriting high-confidence deviations (dry run)
tempcheq --fix --yes       # actually apply those rewrites
tempcheq --watch           # re-audit automatically whenever a file changes
tempcheq --report          # machine-readable JSON to stdout
tempcheq report <path>     # generate deterministic report.md / report.html files
```

### Example

```
TEMPCHEQ — inference temperature audit
Workspace: .

Found 7 inference actions
Temperature-controlled: 5
Implicit/default: 1
Temperature-inapplicable: 1

┌─────────────────────┬────────────────┬─────────┬─────────┬────────────┬─────────┐
│ Action               │ Kind           │ Temp    │ Optimal │ Confidence │ Verdict │
├─────────────────────┼────────────────┼─────────┼─────────┼────────────┼─────────┤
│ route_intent         │ sdk-invocation │ 0.20    │ 0.15    │ 0.70       │ +0.05   │
│ run_eval             │ shell-wrapper  │ 0.90    │ 0.10    │ 0.55       │ +0.80   │
└─────────────────────┴────────────────┴─────────┴─────────┴────────────┴─────────┘
```

`Optimal` is a heuristic prior from task classification, not a measured value — it's a starting
point for review, not a verdict. `--benchmark` perturbs candidates and prints a score curve, but
that curve is clearly labeled as a simulation seeded from the same classification prior; it is not
a live model call.

### `tempcheq report`

For a deliverable rather than a terminal table:

```bash
tempcheq report .                          # writes tempcheq-report/{report.md,report.html}
tempcheq report . --out audit --format html --title "Q3 Inference Audit"
```

Both files are generated deterministically — the same codebase produces byte-identical output on
every run, so two reports from the same commit diff as empty. The HTML is a single self-contained
file (inline CSS and SVG bar charts, no external requests); the Markdown uses Unicode block-bar
charts since GitHub strips raw `<svg>` from rendered Markdown.

## How it works

Discover → Classify → Hypothesise → Report/Benchmark/Fix:

1. **Discover** — regex-scans every file in the workspace for temperature assignments, known
   inference-SDK call sites, and env-var references, in four passes (see `CLAUDE.md` for the exact
   pass ordering and known false-positive sources).
2. **Classify** — scores each action's name, source line, and file path against a keyword table to
   assign a task class: deterministic-extraction, tool-selection, code-generation, summarization,
   conversational, reflection, synthesis, creative, or eval-judge.
3. **Hypothesise** — maps that class to a `(low, ideal, high)` temperature band, adjusted for
   structural signals like `json_schema` or `tool_choice` in the surrounding code, and compares it
   against whatever the code currently has set.
4. **Report / Benchmark / Fix / Watch** — presents the result as a table, JSON, a deterministic
   Markdown/HTML report, a (simulated) perturbation curve, or an automated rewrite of high-confidence
   deviations.

Full architecture notes — module boundaries, the specific regexes, and the gotchas that come with
line-based rather than AST-based scanning — live in `CLAUDE.md`.

## Known limitations

- Detection is regex/line-based, not AST-based. A `"model":` key anywhere in a JSON/YAML/TOML file
  is treated as a possible inference call site, which can false-positive on unrelated config or log
  data (a `data-model` schema, an archived transcript file, etc.).
- `--benchmark` is a clearly-labeled simulation seeded from the same heuristic prior used for the
  recommendation, not a real provider-grounded measurement.
- No automated test suite yet; correctness has been validated through targeted manual and scripted
  repro against fixture codebases.

## License

MIT — see [LICENSE](LICENSE).
