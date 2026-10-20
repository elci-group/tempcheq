"""Provider adapters: deterministic mock (dry-run) and OpenAI-compatible (live).

The mock provider is circular by design: it renders fixture-authored answers
with a quality draw driven by |temperature - ideal|, so the full pipeline
(prompt -> call -> checker -> stats -> report) can be validated without
provider cost. Mock results are pipeline evidence, never outcome evidence.
"""

import json
import time
import urllib.error
import urllib.request


class CallResult:
    def __init__(self, text: str, meta: dict):
        self.text = text
        self.meta = meta


class MockProvider:
    name = "mock"

    def __init__(self, quality_model: dict):
        self.quality_model = quality_model  # per-class params from config

    def effective_temperature(self, temperature: float) -> float:
        return float(temperature)  # the mock accepts the full design range

    def complete(self, messages, temperature: float, seed: int, ctx: dict) -> CallResult:
        cls = ctx["task_class"]
        params = self.quality_model[cls]
        ideal = ctx["ideal"]
        # Seeded per (task, condition, rep) so mock runs are reproducible.
        import random
        rng = random.Random(seed)
        delta = abs(float(temperature) - ideal)
        q = params["base"] - params["sensitivity"] * delta + rng.uniform(
            -params["noise"], params["noise"])
        q = max(0.0, min(1.0, q * ctx.get("quality_factor", 1.0)))
        text = ctx["sample_answer"](q, rng)
        return CallResult(text, {"provider": "mock", "quality_draw": round(q, 4),
                                 "temperature": temperature})


class OpenAICompatProvider:
    """Any OpenAI-compatible /chat/completions endpoint (OpenAI, Groq, vLLM,
    Zai, Ollama-with-openai-compat). Stdlib urllib; bearer auth."""

    name = "openai-compat"

    def __init__(self, base_url: str, api_key: str, model: str,
                 max_tokens: int = 512, timeout_s: int = 60, retries: int = 3,
                 send_seed: bool = True, extra_body: dict | None = None,
                 temp_range: tuple | None = None):
        self.base_url = base_url.rstrip("/")
        self.api_key = api_key
        self.model = model
        self.max_tokens = max_tokens
        self.timeout_s = timeout_s
        self.retries = retries
        self.send_seed = send_seed
        self.extra_body = extra_body or {}
        self.temp_range = temp_range

    def effective_temperature(self, temperature: float) -> float:
        """Endpoint-legal temperature (design temp is preserved upstream and
        both values are logged, so saturation is measurable, not silent)."""
        if self.temp_range:
            return max(self.temp_range[0], min(self.temp_range[1], float(temperature)))
        return float(temperature)

    def complete(self, messages, temperature: float, seed: int, ctx: dict) -> CallResult:
        effective = self.effective_temperature(temperature)
        payload = {
            "model": self.model,
            "messages": messages,
            "temperature": effective,
            "max_tokens": self.max_tokens,
        }
        if self.send_seed:
            payload["seed"] = int(seed) % (2 ** 31 - 1)
        payload.update(self.extra_body)
        last_err = None
        for attempt in range(self.retries):
            try:
                req = urllib.request.Request(
                    self.base_url + "/chat/completions",
                    data=json.dumps(payload).encode("utf-8"),
                    headers={
                        "Content-Type": "application/json",
                        "Authorization": f"Bearer {self.api_key}",
                    },
                    method="POST",
                )
                with urllib.request.urlopen(req, timeout=self.timeout_s) as resp:
                    body = json.loads(resp.read().decode("utf-8"))
                text = body["choices"][0]["message"]["content"] or ""
                meta = {
                    "provider": self.name,
                    "model": self.model,
                    "temperature": effective,
                    "requested_temperature": float(temperature),
                    "usage": body.get("usage", {}),
                    "attempts": attempt + 1,
                }
                return CallResult(text, meta)
            except urllib.error.HTTPError as err:
                last_err = err
                if err.code == 429 and attempt < self.retries - 1:
                    retry_after = err.headers.get("retry-after") if err.headers else None
                    delay = float(retry_after) if retry_after else 2.0 ** attempt
                    time.sleep(min(delay, 60))
                    continue
                if attempt < self.retries - 1:
                    time.sleep(2 ** attempt)
            except (urllib.error.URLError, TimeoutError, KeyError,
                    json.JSONDecodeError) as err:
                last_err = err
                if attempt < self.retries - 1:
                    time.sleep(2 ** attempt)
        raise RuntimeError(f"provider call failed after {self.retries} attempts: {last_err}")


def provider_from_env(cfg: dict, env: dict) -> OpenAICompatProvider:
    key = env.get("TEMPCHEQ_BENCH_API_KEY", "")
    if not key:
        raise SystemExit("TEMPCHEQ_BENCH_API_KEY is not set (live mode)")
    base = env.get("TEMPCHEQ_BENCH_BASE_URL", cfg.get("base_url", "https://api.openai.com/v1"))
    model = env.get("TEMPCHEQ_BENCH_MODEL", cfg.get("model", "gpt-4o-mini"))
    extra = {}
    raw_extra = env.get("TEMPCHEQ_BENCH_EXTRA_JSON", "")
    if raw_extra:
        extra = json.loads(raw_extra)
    retries = int(env.get("TEMPCHEQ_BENCH_RETRIES", cfg["retries"]))
    temp_range = None
    raw_range = env.get("TEMPCHEQ_BENCH_TEMP_RANGE", "")
    if raw_range:
        lo, hi = raw_range.split(",")
        temp_range = (float(lo), float(hi))
    return OpenAICompatProvider(base, key, model,
                                max_tokens=cfg["max_tokens"],
                                timeout_s=cfg["request_timeout_s"],
                                retries=retries,
                                extra_body=extra,
                                temp_range=temp_range)
