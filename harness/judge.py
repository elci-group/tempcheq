"""Blind pairwise judge (live mode only).

Judge sees condition-stripped transcripts at temperature 0 with position
swap; each pair costs two calls. The judge is only consulted for dimensions
without programmatic checkers, and never sees temperatures or condition ids.
"""

import json
import random

from harness.providers import OpenAICompatProvider

JUDGE_SYSTEM = (
    "You are a strict evaluator. Two responses (A and B) answer the same "
    "request. Reply with only minified JSON: {\"better\": \"A\"|\"B\"|\"tie\"}.")


def judge_pair(provider: OpenAICompatProvider, request: str, resp_a: str,
               resp_b: str, seed: int) -> str:
    messages = [
        {"role": "system", "content": JUDGE_SYSTEM},
        {"role": "user", "content": f"REQUEST:\n{request}\n\nRESPONSE A:\n{resp_a}\n\nRESPONSE B:\n{resp_b}"},
    ]
    result = provider.complete(messages, 0.0, seed, {"task_class": "eval-judge",
                                                     "ideal": 0.0,
                                                     "sample_answer": lambda q, r: '{"better":"tie"}'})
    parsed = _parse(result.text)
    return parsed if parsed in ("A", "B", "tie") else "tie"


def judge_win_rate(provider: OpenAICompatProvider, pairs: list, seed: int) -> dict:
    """pairs: list of (request, treatment_response, baseline_response).
    Position-swapped; returns win/loss/tie counts from the treatment view."""
    wins = losses = ties = 0
    for idx, (request, treat, base) in enumerate(pairs):
        rng = random.Random(f"{seed}:{idx}")
        s1, s2 = rng.randrange(10 ** 6), rng.randrange(10 ** 6)
        verdict1 = judge_pair(provider, request, treat, base, s1)
        verdict2 = judge_pair(provider, request, base, treat, s2)
        call1 = {"A": "win", "B": "loss", "tie": "tie"}[verdict1]
        call2 = {"A": "loss", "B": "win", "tie": "tie"}[verdict2]
        if call1 == "win" and call2 == "win":
            wins += 1
        elif call1 == "loss" and call2 == "loss":
            losses += 1
        else:
            ties += 1
    n = max(1, wins + losses + ties)
    return {"wins": wins, "losses": losses, "ties": ties,
            "win_rate": round((wins + 0.5 * ties) / n, 4)}


def _parse(text: str):
    text = text.strip()
    try:
        return json.loads(text).get("better")
    except (json.JSONDecodeError, AttributeError):
        for token in ("\"A\"", "\"B\"", "\"tie\""):
            if token in text:
                return token.strip("\"")
        return None
