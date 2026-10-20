"""Condition resolution: C0 wild / C1 tempcheq / C2 adverse / C3 oracle.

C1 is taken verbatim from TempCheq's recommendation output (bands.json,
harvested from `tempcheq --report`). No per-task manual tuning.
"""

from harness import clamp


def adverse_temperature(band: dict, extra: float = 0.2, clamp_lo: float = 0.0,
                        clamp_hi: float = 2.0) -> float:
    """Band edge farthest from ideal, pushed `extra` further away, clamped."""
    low, ideal, high = band["low"], band["ideal"], band["high"]
    edge = high if (high - ideal) >= (ideal - low) else low
    direction = 1.0 if edge == high else -1.0
    return clamp(edge + direction * extra, clamp_lo, clamp_hi)


def condition_temps(task: dict, band: dict, cfg: dict) -> dict:
    """Resolve C0/C1/C2 temperatures for one task call site profile."""
    found = task.get("as_found")
    c0 = cfg["provider_default_temperature"] if found is None else found
    return {
        "C0": round(clamp(float(c0), cfg["temp_clamp"][0], cfg["temp_clamp"][1]), 4),
        "C1": round(float(band["ideal"]), 4),
        "C2": round(adverse_temperature(band, cfg["adverse_extra"],
                                        cfg["temp_clamp"][0], cfg["temp_clamp"][1]), 4),
    }


def oracle_grid(oracle_temps: list) -> list:
    return [round(float(t), 4) for t in oracle_temps]
