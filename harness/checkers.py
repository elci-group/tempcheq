"""Programmatic verifiers. No LLM judgment here — deterministic only.

Every checker returns (passed: bool, score: float in [0,1], details: dict).
Gold references come from fixture metadata and are never sent to providers.
"""

import json
import re
import subprocess
import tempfile
import os


def _parse_json_loose(text: str):
    text = text.strip()
    if text.startswith("```"):
        text = re.sub(r"^```[a-zA-Z]*\n?", "", text)
        text = re.sub(r"\n?```$", "", text.strip())
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        match = re.search(r"[\[{].*[\]}]", text, re.DOTALL)
        if match:
            try:
                return json.loads(match.group(0))
            except json.JSONDecodeError:
                return None
        return None


def extraction_checker(answer: str, gold: dict) -> tuple:
    """artifact-quality + hallucination. A non-null answer for a field whose
    gold is null counts as fabrication."""
    parsed = _parse_json_loose(answer)
    if not isinstance(parsed, dict):
        return False, 0.0, {"error": "unparseable"}
    fields = [k for k in gold]
    ok = sum(1 for k in fields if parsed.get(k) == gold[k])
    fabricated = sum(1 for k in fields if gold[k] is None and parsed.get(k) is not None)
    score = ok / len(fields)
    score = max(0.0, score - 0.25 * fabricated)
    return (score >= 0.999), round(score, 4), {
        "field_ok": ok, "fields": len(fields), "fabricated": fabricated}


def exact_set_checker(answer: str, gold: list) -> tuple:
    parsed = _parse_json_loose(answer)
    if not isinstance(parsed, list):
        return False, 0.0, {"error": "not-a-list"}
    have = sorted(str(x) for x in parsed)
    want = sorted(gold)
    if have == want:
        return True, 1.0, {"match": True}
    common = len(set(have) & set(want))
    precision = common / len(have) if have else 0.0
    recall = common / len(want) if want else 0.0
    f1 = 2 * precision * recall / (precision + recall) if precision + recall else 0.0
    return False, round(f1, 4), {"have": have, "want": want}


def verdict_checker(answer: str, gold: dict) -> tuple:
    parsed = _parse_json_loose(answer)
    if not isinstance(parsed, dict) or "verdict" not in parsed:
        return False, 0.0, {"error": "no-verdict"}
    passed = parsed["verdict"] == gold["verdict"]
    return passed, 1.0 if passed else 0.0, {"verdict": parsed["verdict"]}


def rouge_l_f1(answer: str, gold: str) -> float:
    a = [w.strip(".,?!").lower() for w in answer.split() if w.strip(".,?!")]
    g = [w.strip(".,?!").lower() for w in gold.split() if w.strip(".,?!")]
    if not a or not g:
        return 0.0
    # LCS length via dynamic programming.
    prev = [0] * (len(g) + 1)
    for wa in a:
        cur = [0] * (len(g) + 1)
        for j, wg in enumerate(g, 1):
            cur[j] = prev[j - 1] + 1 if wa == wg else max(prev[j], cur[j - 1])
        prev = cur
    lcs = prev[-1]
    if lcs == 0:
        return 0.0
    p, r = lcs / len(a), lcs / len(g)
    return 2 * p * r / (p + r)


def summarization_checker(answer: str, gold: dict) -> tuple:
    """response-quality: ROUGE-L vs gold plus required-term coverage."""
    text = answer.lower()
    terms = gold["required_terms"]
    covered = sum(1 for t in terms if t.lower() in text)
    rouge = rouge_l_f1(answer, gold["summary"])
    score = 0.6 * rouge + 0.4 * (covered / len(terms) if terms else 1.0)
    return (score >= 0.75), round(score, 4), {"rouge_l": round(rouge, 4),
                                              "terms": f"{covered}/{len(terms)}"}


def conversational_checker(answer: str, gold: dict) -> tuple:
    """Format compliance (exactly N bullets) + banned-content guard +
    persona-keyword requirement."""
    lines = [ln for ln in answer.splitlines() if ln.strip()]
    bullets = [ln for ln in lines if ln.lstrip().startswith("-")]
    low = answer.lower()
    banned_hits = [b for b in gold["banned"] if b in low]
    bullet_ok = len(bullets) == gold["bullets"]
    persona_ok = gold.get("must_contain", "").lower() in low if \
        gold.get("must_contain") else True
    score = (1.0 if bullet_ok else 0.0) * (0.0 if banned_hits else 1.0) \
        * (1.0 if persona_ok else 0.0)
    return (score >= 0.999), round(score, 4), {"bullets": len(bullets),
                                               "banned_hits": banned_hits,
                                               "persona_ok": persona_ok}


def reflection_checker(answer: str, gold: dict) -> tuple:
    parsed = _parse_json_loose(answer)
    have = set(parsed) if isinstance(parsed, list) else set()
    want = set(gold["defects"])
    fp = len(have - want)
    tp = len(have & want)
    precision = tp / len(have) if have else 0.0
    recall = tp / len(want) if want else 0.0
    f1 = 2 * precision * recall / (precision + recall) if precision + recall else 0.0
    exact = have == want
    return exact, round(f1, 4), {"true_positives": tp, "false_positives": fp,
                                 "missed": sorted(want - have)}


def synthesis_checker(answer: str, gold: dict) -> tuple:
    """problem-solving + hallucination: required claims present, forbidden
    claims absent."""
    low = answer.lower()
    req = gold["required_claims"]
    covered = sum(1 for c in req if c.lower() in low)
    forbidden_hits = [c for c in gold.get("forbidden", []) if c.lower() in low]
    score = (covered / len(req) if req else 1.0) * (0.0 if forbidden_hits else 1.0)
    return (score >= 0.999 and not forbidden_hits), round(score, 4), {
        "claims": f"{covered}/{len(req)}", "forbidden_hits": forbidden_hits}


def creative_checker(answer: str, gold: dict) -> tuple:
    name = answer.strip()
    parts = name.split("-")
    low = name.lower()
    banned_hits = [b for b in gold["banned"] if b in low]
    want_words = gold.get("words", 2)
    fmt_ok = (len(parts) == want_words
              and all(p.islower() for p in parts)
              and all(p.isalpha() for p in parts)
              and len(set(parts)) == len(parts))
    score = (1.0 if fmt_ok else 0.0) * (0.0 if banned_hits else 1.0)
    return (score >= 0.999), round(score, 4), {"name": name, "format_ok": fmt_ok,
                                               "banned_hits": banned_hits}


def run_code_tests(code: str, tests: list, timeout_s: float = 5.0) -> tuple:
    """Execute fixture-defined tests against the answer in a subprocess.

    Guarded: callers must set allow_code_exec=True (config) and should run
    inside a container. Environment is stripped; no network in tests.
    """
    harness_code = (
        code + "\n\n" + "\n".join(tests) + "\nprint('ALL_TESTS_PASSED')\n")
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "candidate.py")
        with open(path, "w", encoding="utf-8") as fh:
            fh.write(harness_code)
        try:
            proc = subprocess.run(
                ["python3", path], capture_output=True, text=True,
                timeout=timeout_s, cwd=tmp,
                env={"PATH": os.environ.get("PATH", "/usr/bin:/bin")})
        except subprocess.TimeoutExpired:
            return False, 0.0, {"error": "timeout"}
        if proc.returncode != 0 or "ALL_TESTS_PASSED" not in proc.stdout:
            return False, 0.0, {"error": (proc.stderr or "tests failed").strip()[-300:]}
        return True, 1.0, {"tests": len(tests)}


def code_gen_checker(answer: str, gold: dict, allow_exec: bool) -> tuple:
    if not allow_exec:
        return False, 0.0, {"skipped": "code execution disabled"}
    code = answer.strip()
    if code.startswith("```"):
        code = re.sub(r"^```[a-zA-Z]*\n?", "", code)
        code = re.sub(r"\n?```$", "", code.strip())
    return run_code_tests(code, gold["tests"])


CHECKERS = {
    "deterministic-extraction": extraction_checker,
    "tool-selection": exact_set_checker,
    "eval-judge": verdict_checker,
    "code-generation": lambda a, g: code_gen_checker(a, g, allow_exec=True),
    "summarization": summarization_checker,
    "conversational": conversational_checker,
    "reflection": reflection_checker,
    "synthesis": synthesis_checker,
    "creative": creative_checker,
}


def check(task_class: str, answer: str, gold, allow_exec: bool = False) -> tuple:
    if task_class == "code-generation":
        return code_gen_checker(answer, gold, allow_exec)
    return CHECKERS[task_class](answer, gold)
