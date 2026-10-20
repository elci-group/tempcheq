# TD-001 — False-Positive Suppression in Inference-Action Discovery

| Field | Value |
|-------|-------|
| Directive ID | TD-001 |
| Status | Accepted — implementation in this change |
| Component | `src/discover.rs` (Discover stage), `src/cli.rs`, `src/run.rs`, `src/main.rs` |
| Author | Kilo (automated engineering session) |
| Date | 2026-10-07 |
| Related | `docs/benchmark-design.md` (§7 confound controls), README "Known limitations" |
| Priority | High — blocks benchmark M0 corpus harvest (benchmark validity depends on flag precision) |

## 1. Background

TempCheq's Discover stage is line-based, not AST-based (CLAUDE.md, architecture §1).
A self-audit of this repository (`tempcheq . --explain`, 2026-10-07) reported
**10 inference actions, 1 flagged as a material deviation**. Manual review showed
that **all 10 detections were false positives**: this codebase makes no LLM
inference calls. Every flag originated in one of three line-content classes the
scanner cannot currently distinguish from call sites:

| Class | Evidence (self-audit) | Mechanism |
|-------|----------------------|-----------|
| F-1 Doc comments | `src/action.rs:60` (confidence 0.85 — highest in the audit), `src/discover.rs:251` | `///` prose containing `temperature=0` / `os.environ.get("TEMPERATURE")` matches Pass 1 and Pass 4 |
| F-2 Pattern definitions | `src/discover.rs:68,69,77` | Lines defining the scanner's own `Regex::new(...)` literals contain endpoint substrings (`chat/completions`, `/v1/messages`) matching Pass 3 markers and the multiline HTTP pairing |
| F-3 Test fixtures | `src/discover.rs:397,419,435,464,479` | `#[test]` functions embed synthetic request snippets (`"model":"demo"`, `temperature:0.2`, `curl`) that match Passes 1–3; the single "material deviation" (+0.15) was a fixture literal |

Secondary finding: the summary metric "Average |current − optimal| deviation" is
averaged over materially-deviating actions only (`src/report.rs:144-147`), so a
single false positive produced the headline "0.15". Metric semantics are out of
scope here (see §9) but recorded for the record.

Consequence if unaddressed: (a) user trust erosion — every early adopter running
the tool on its own source sees self-flags; (b) benchmark contamination — the M0
corpus harvest (benchmark design §4.1) would harvest fixture temperatures as
"wild" baselines, and the H1 uplift claim would rest on unvalidated flags.

## 2. Goals

- G-1: A self-scan of this repository returns **zero** inference actions.
- G-2: Suppression must be **conservative**: no true positive detectable today
  (outside suppressed regions) may be lost. Golden detection fixtures must pass
  unchanged.
- G-3: Suppression must be **deterministic and reproducible** — same input
  bytes, same mask, on every run and platform.
- G-4: Suppression must be **layered and auditable**: each layer independently
  justified, individually testable, and attributable in code review.
- G-5: An operator escape hatch (`--include-tests`) must exist for audits where
  test/fixture code is legitimately in scope (e.g. eval-harness repos).
- G-6: The false-positive class reduction must be regression-locked by unit
  tests that fail on the pre-fix scanner.

## 3. Non-goals

- N-1: AST-based parsing (tracked as future work; line-based constraint stands).
- N-2: Fixing the `"model":` false-positive class on arbitrary *data* files
  (JSON/YAML schemas, transcripts) — README already documents this; TD-002
  candidate (per-path config).
- N-3: Inline trailing-comment stripping (`let x = 1; // temperature=0.9`) —
  deferred: risky for one-line call sites; documented as residual limitation.
- N-4: Changing classification priors, bands, or the report summary semantics.

## 4. Requirements

- R-1: Introduce a per-file **suppression mask** (`Vec<bool>`, one flag per
  line) computed *before* all four discovery passes; every pass must consult it.
- R-2: Layer S1 — comment suppression. Lines that are comment-only, and block
  comments (open through close), are suppressed, per language family:
  C-family (`//`, `/* */`): `rs, js, ts, tsx, jsx, go`; hash-family (`#`):
  `py, sh, bash, zsh, yml, yaml, toml, cfg, ini, env, rb`. Unknown extensions:
  suppress nothing (conservative default).
- R-3: Layer S2 — pattern-definition suppression. Any line containing
  `Regex::new(`, `re.compile(`, or `new RegExp(` is a pattern literal under
  construction, not a call site, and is suppressed in all extensions.
- R-4: Layer S3 — test-region suppression (Rust only in this iteration).
  Regions opened by `#[test]`, `#[tokio::test]`, `#[async_std::test]`, or
  `#[cfg(test)]` are suppressed until brace depth returns to the attribute's
  depth *and* the closing line is a bare `}`. Governed by `--include-tests`.
- R-5: Suppressed lines must also be excluded from **cross-line heuristics**:
  the ±10-line "nearby explicit temperature" window in Pass 3, and the
  multiline HTTP transport pairing window — a comment mentioning
  `temperature=0` must not mask a real call site beside it.
- R-6: `nearest_name()` and `context_window()` continue to see all lines
  (naming/classification context is unaffected by suppression).
- R-7: The mask computation must not allocate per-call regexes and must remain
  O(lines × constant).
- R-8: All behavior changes documented (README limitations, CLAUDE.md pass
  description) in the same change.

## 5. Design

```
scan_file(root, path, content, is_workflow, include_tests, out)
  ├─ lines = content.lines()
  ├─ mask  = build_suppression_mask(&lines, ext, include_tests)
  │    ├─ S1: comment tokens by family + C-family /* */ state machine
  │    ├─ S2: pattern-definition substrings (language-independent)
  │    └─ S3: Rust test-region state machine (attr → depth-tracked region)
  └─ Passes 1–4: unchanged logic, but `if mask[i] { continue; }`
       and unmasked-only windows for cross-line heuristics (R-5)
```

Key decisions:
- **Mask, not filtering**: lines remain available for naming/context (R-6) and
  the diff to each pass is one guard, minimizing regression surface.
- **Bare-`}` region close** (`trimmed == "}"` plus depth check): tolerates
  unbalanced braces inside fixture string literals, which plain depth counting
  would mis-close. Deterministic; documented as a heuristic, same class as the
  existing `nearest_name` backward scan.
- **`--include-tests` re-includes S3 only.** S1/S2 are unconditional: comment
  prose and regex literals are never legitimate call sites in any audit mode.
- Precedence/interplay with `explained_lines` is unchanged; the mask applies
  upstream of it.

## 6. Acceptance criteria

- AC-1: `cargo test` green, including 6 new regression tests (§7), of which at
  least 4 demonstrably fail against the pre-fix scanner (red→green evidence).
- AC-2: `cargo clippy --all-targets -- -W clippy::all` clean.
- AC-3: `tempcheq . --report` on this repository → `"actions": []`.
- AC-4: `tempcheq . --include-tests` → re-detects the fixture actions that
  existed pre-fix (escape hatch demonstrably wired end-to-end).
- AC-5: `deliver` passes (existing golden + thermal report command unaffected).
- AC-6: README + CLAUDE.md updated in the same change (R-8).

## 7. Test plan

New unit tests in `src/discover.rs` (red→green against the pre-fix scanner):

1. `doc_comment_temperature_mentions_are_suppressed` — F-1/Pass 1 + Pass 4.
2. `regex_pattern_definition_lines_are_suppressed` — F-2, near-verbatim copies
   of the self-flagged lines.
3. `rust_test_regions_are_suppressed` — F-3, fixture-bearing `#[test]` region.
4. `include_tests_flag_restores_test_region_detection` — same input, flag on.
5. `real_call_sites_outside_test_regions_survive` — over-suppression guard.
6. `hash_family_comments_are_suppressed` — Python fixture with comment-only
   `# temperature=0.9`.

Post-implementation verification: full suite, clippy, self-scan JSON (AC-3),
self-scan with flag (AC-4), `deliver` (AC-5).

## 8. Rollout & compatibility

- Default-on for the next minor release; behavior change called out in the
  README example/limitations.
- No output-schema changes; `--report` JSON identical in shape. Downstream
  consumers (benchmark harness) see fewer actions — expected and desired.
- Watch mode inherits the flag through `RunOptions` automatically.

## 9. Residual risks & mitigations

| Risk | Likelihood | Mitigation |
|------|-----------|------------|
| Over-suppression hides a real call inside a test region | Low | `--include-tests` (G-5); eval-harness docs point at the flag |
| Brace heuristic mis-closes a region on pathological string literals | Low | bare-`}` + depth rule; unit test with fixture braces; documented heuristic |
| Pattern-definition lines that also host a real call | Very low | substring requires all three markers on one line; review via S2 test |
| Inline trailing comments still false-positive (N-3) | Medium | documented residual; TD-002 candidate |
| `"model":` in data files still false-positive | Medium | pre-existing documented limitation (N-2) |
| Summary-metric semantics (avg over flagged only) misleads | Medium | out of scope; logged for TD-003 |

## 10. Success metrics (post-release)

- Self-scan of tempcheq: 0 actions (permanent CI gate via AC-3).
- FP rate on the benchmark M0 corpus of ~50 repos, human-validated sample:
  target ≥ 90% reduction in non-config FPs.
- Zero golden-fixture regressions (AC-1/AC-5).

## 11. Future work

- TD-002: per-path/extension config to suppress `"model":` data-file FPs;
  inline-comment stripping.
- AST or tree-sitter scanning to replace brace/attr heuristics.
- Wire suppression counts into `--report` JSON (`"suppressed_lines"` per
  action class) for observability, feeding the impact benchmark's sensitivity
  gating.
