"""Statistics: exact McNemar, Wilcoxon signed-rank (normal approx),
paired bootstrap CIs, Holm correction. Stdlib only, deterministic seeds.
"""

import math
import random


def risk_difference(a1: int, n1: int, a0: int, n0: int) -> float:
    """Treatment (C1) minus baseline (C0) proportion, in [−1, 1]."""
    if n1 == 0 or n0 == 0:
        return 0.0
    return a1 / n1 - a0 / n0


def mcnemar_exact(n01: int, n10: int) -> float:
    """Exact two-sided binomial McNemar p-value. n01 = baseline-passed-but-
    treatment-failed count, n10 = the converse."""
    k, n = min(n01, n10), n01 + n10
    if n == 0:
        return 1.0
    tail = sum(math.comb(n, i) for i in range(0, k + 1))
    p = min(1.0, 2 * tail / 2 ** n)
    return p


def wilcoxon_signed_rank(deltas: list) -> tuple:
    """Two-sided Wilcoxon signed-rank test, normal approximation with
    continuity correction. Returns (z, p). Zeros are dropped (Wilcoxon
    pratt-lite); |d| < 1e-12 counts as zero."""
    d = [x for x in deltas if abs(x) > 1e-12]
    n = len(d)
    if n == 0:
        return 0.0, 1.0
    ranked = sorted((abs(x), i) for i, x in enumerate(d))
    ranks = [0.0] * n
    i = 0
    while i < n:
        j = i
        while j + 1 < n and abs(ranked[j + 1][0] - ranked[i][0]) < 1e-12:
            j += 1
        mid = (i + j) / 2 + 1
        for k in range(i, j + 1):
            ranks[ranked[k][1]] = mid
        i = j + 1
    w_plus = sum(r for r, x in zip(ranks, d) if x > 0)
    mean = n * (n + 1) / 4
    var = n * (n + 1) * (2 * n + 1) / 24
    if var == 0:
        return 0.0, 1.0
    z = (w_plus - mean) / math.sqrt(var)
    p = 2 * (1 - _norm_cdf(abs(z)))
    return z, max(0.0, min(1.0, p))


def _norm_cdf(x: float) -> float:
    return 0.5 * (1 + math.erf(x / math.sqrt(2)))


def paired_bootstrap_ci(deltas: list, draws: int = 2000, seed: int = 0,
                        level: float = 0.95) -> tuple:
    """Percentile bootstrap CI for the mean of paired deltas."""
    if not deltas:
        return 0.0, 0.0
    rng = random.Random(seed)
    n = len(deltas)
    means = []
    for _ in range(draws):
        sample = [deltas[rng.randrange(n)] for _ in range(n)]
        means.append(sum(sample) / n)
    means.sort()
    lo_idx = int((1 - level) / 2 * draws)
    hi_idx = int((1 + level) / 2 * draws) - 1
    return round(means[lo_idx], 6), round(means[min(hi_idx, draws - 1)], 6)


def holm(pvals: dict) -> dict:
    """Holm–Bonferroni adjusted p-values keyed like the input."""
    items = sorted(pvals.items(), key=lambda kv: kv[1])
    m = len(items)
    adjusted, running = {}, 0.0
    for idx, (key, p) in enumerate(items):
        running = max(running, min(1.0, (m - idx) * p))
        adjusted[key] = round(running, 6)
    return adjusted


def spearman(xs: list, ys: list) -> float:
    """Spearman rank correlation (ties get midranks)."""
    def ranks(v):
        order = sorted(range(len(v)), key=lambda i: v[i])
        out = [0.0] * len(v)
        i = 0
        while i < len(v):
            j = i
            while j + 1 < len(v) and v[order[j + 1]] == v[order[i]]:
                j += 1
            mid = (i + j) / 2 + 1
            for k in range(i, j + 1):
                out[order[k]] = mid
            i = j + 1
        return out
    rx, ry = ranks(xs), ranks(ys)
    n = len(xs)
    if n < 2:
        return 0.0
    mx, my = sum(rx) / n, sum(ry) / n
    num = sum((a - mx) * (b - my) for a, b in zip(rx, ry))
    den = math.sqrt(sum((a - mx) ** 2 for a in rx) * sum((b - my) ** 2 for b in ry))
    return num / den if den else 0.0
