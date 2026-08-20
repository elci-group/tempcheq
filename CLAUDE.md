# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

TempCheq is a Rust CLI that audits a workspace for LLM inference "temperature" settings and recommends
what each one *should* be, based on the kind of task the call site is performing. It is not a temperature
setter — it's a static-analysis tool: scan a repo, find every place a sampling temperature is set (or
silently defaulted), classify what that call is for, and flag values that don't match the task.

## Commands

```bash
cargo build                 # debug build -> target/debug/tempcheq
cargo build --release       # release build -> target/release/tempcheq
cargo run -- <path> [flags] # e.g. cargo run -- . --explain
cargo clippy --all-targets -- -W clippy::all
cargo audit                 # checks Cargo.lock against the RustSec advisory DB
cargo test                  # no tests exist yet (see Known gaps below)
```

CLI flags (see `src/cli.rs`): `--explain`, `--benchmark`, `--fix` (+ `--yes`, `--fix-confidence`),
`--watch`, `--report` (JSON to stdout). Path defaults to `.`.

`tempcheq report [path] [--out DIR] [--format md|html|both] [--title STR]` is a separate subcommand
(not a flag) — it writes `report.md`/`report.html` files (default `--out tempcheq-report/`) rendered by
`src/export.rs`. Distinct from `--report`: that flag dumps JSON to stdout for piping; the subcommand
produces polished deliverable files. `Cli` has both `#[command(subcommand)] command: Option<Command>` and
the legacy top-level fields side by side — clap resolves `report` as the subcommand rather than the
positional `path` because it matches a known subcommand name first.

## Architecture

The pipeline is Discover → Classify → Hypothesise → Report/Benchmark/Fix, wired together in `src/run.rs`:

1. **`discover.rs`** — walks the workspace with `walkdir`, regex-scans each file for temperature-related
   patterns, and produces `Vec<InferenceAction>` (defined in `action.rs`). Scanning is line-based, not
   AST-based, and runs in four passes over each file's lines:
   - Pass 1: explicit values — `temperature: 0.7` / `.temperature(0.7)` / `TEMPERATURE=0.7` style
     assignments (`EXPLICIT_TEMP`, `BUILDER_TEMP`, `ENV_TEMP_ASSIGN`).
   - Pass 2: embedding-style calls (`.embeddings.create(`, etc.) → `TemperatureState::Inapplicable`.
   - Pass 3: known inference call markers (`CALL_MARKERS`: OpenAI/Anthropic/Ollama/LangChain/curl/etc.)
     with no explicit temperature within a ±10-line window → `TemperatureState::Implicit`.
   - Pass 4: env-var *references* with no inline default (`os.environ.get("TEMPERATURE")`) → `Implicit`.
   - A line claimed by an earlier pass is tracked in `explained_lines` so later passes skip it — this
     matters if you add a new pass; forgetting to record `explained_lines` there will double-count call
     sites that also happen to match a later pass's pattern.
   - `nearest_name()` derives a human-readable action name by scanning backward for the nearest enclosing
     `fn`/`def`/`function`/`class`/etc. (preferred), then falling back to a nearby JSON/YAML key.

2. **`classify.rs`** — scores an action's name + context line + file path against `RULES`, a static table
   of `(TaskClass, weight, keywords)`. `TaskClass` (deterministic-extraction, tool-selection, code-gen,
   summarization, conversational, reflection, synthesis, creative, eval-judge) is the axis the whole tool
   reasons about. Keyword matching uses `keyword_present()`, a hand-rolled "letter-boundary" check (not
   the `regex` crate's `\b`, which treats `_` as a word char and would reject `eval` inside `run_eval`) —
   it matches a keyword unless it's flanked by another ASCII letter, so `eval` matches in `run_eval.sh`
   but not in `retrieval.py`. The `regex` crate has no lookaround, so this boundary logic is done by hand
   over byte offsets in `discover.rs` and `classify.rs` alike; keep that constraint in mind before reaching
   for a "smarter" regex.

3. **`hypothesize.rs`** — combines a `TaskClass`'s prior `(low, ideal, high)` band with `CONSTRAINTS`
   (structural signals like `json_schema`/`tool_choice` in the context line that shift `ideal` down, or
   `stream` that shifts it up), clamped back into `[low, high]`. Produces a `Recommendation` with a
   confidence score and a `verdict()` (signed deviation of the action's current value from `ideal`).
   **Caveat:** `context` on an `InferenceAction` is only the single matched line — a multi-line call where
   `tool_choice` sits several lines away from the `temperature=` line won't trigger that constraint.

4. **`report.rs` / `benchmark.rs` / `fix.rs` / `watch.rs`** — presentation and side effects over
   `Vec<Entry>` (an `InferenceAction` + its `Recommendation`, from `run.rs`):
   - `report.rs`: the table, `--explain` reasoning, `--report` JSON.
   - `benchmark.rs`: `--benchmark` prints a perturb-and-score curve, but it's a **deterministic
     simulation** seeded from the classification prior (`hash_unit`), not a live model call — this is
     called out in the printed output itself. There's no real eval hook wired in yet.
   - `fix.rs`: `--fix` only rewrites lines matching one of the three explicit-value regexes
     (`EXPLICIT_TEMP`, `BUILDER_TEMP`, `ENV_TEMP_ASSIGN`, all `pub(crate)` in `discover.rs`). If a new
     assignment style is added to `discover.rs`'s Pass 1, `apply_fixes()` needs a matching branch or it
     will silently skip that action (it does report skips explicitly, via the returned skip list — don't
     regress that into a silent no-op). Dry-run by default; `--yes` required to write.

5. **`export.rs`** — renders `Vec<Entry>` into the `report` subcommand's Markdown and HTML. The whole
   point is byte-for-byte determinism across runs against the same code, so this file has hard rules:
   no wall-clock timestamps anywhere in the output; never iterate a `HashMap` (Rust randomizes its hash
   seed per process, so iteration order isn't stable run-to-run) — group by `TaskClass` using
   `classify::ALL_CLASSES` (fixed declaration order), and sort every other listing explicitly with a
   tie-break down to `(file, line)`. Markdown uses Unicode block characters for bar charts instead of
   inline SVG/images, since GitHub's Markdown renderer strips raw `<svg>`; HTML uses real inline SVG
   (self-contained, no CDN/external fonts — the file has to stand alone). If you add a new field to the
   report, keep it derived only from `Entry`/`root`, not from anything time- or environment-dependent.

## Known gaps

- No test suite yet — all validation so far has been manual repro against scratch fixtures.
- `discover.rs`'s `CALL_MARKERS` includes a broad `"model":` marker for arbitrary config files, which can
  false-positive on any JSON/YAML/TOML file with an unrelated `model` key (e.g. a data-model schema).
- `--benchmark` is a labeled simulation, not a real provider-grounded measurement.
