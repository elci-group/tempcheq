"""Deterministic closed-world fixture generation.

Every task carries: the prompt messages actually sent to the provider, the
gold reference used ONLY by checkers (never sent), and a mock renderer that
produces answers scoring ~quality under the checker (dry-run only).

All randomness flows through random.Random(base_seed) so the same suite is
byte-identical on every machine.
"""

import json
import random

from harness import CLASSES

SKUS = ["AX-110", "BX-220", "CX-330", "DX-440"]
CUSTOMERS = ["Mero Street Cafe", "Bluefin Logistics", "Kite & Co", "Vantar Group"]
DOCKS = ["A", "B", "C", "D", "E", "F"]
TOOLS = {
    "search_docs": "Search the internal knowledge base.",
    "create_ticket": "Open a support ticket.",
    "list_orders": "List a customer's recent orders.",
    "refund_order": "Refund a specific order id.",
    "replace_item": "Ship a no-cost replacement for an order.",
    "send_email": "Send an email to a customer.",
    "read_file": "Read a file from the workspace.",
    "run_sql": "Run a read-only SQL query.",
    "schedule_meeting": "Schedule a calendar meeting.",
    "page_oncall": "Page the on-call engineer.",
    "export_csv": "Export a report as CSV.",
    "translate_text": "Translate text between languages.",
}


def _extraction_task(i: int, rng: random.Random) -> dict:
    order_id = f"ORD-{7000 + i * 37}"
    customer = CUSTOMERS[i % len(CUSTOMERS)]
    sku = SKUS[i % len(SKUS)]
    quantity = 1 + (i * 13) % 9
    dock = DOCKS[i % len(DOCKS)]
    duty = bool(i % 2)
    unanswerable = (i % 4 == 3)
    doc_lines = [
        f"Received shift {i % 3 + 1}; seal number {4000 + i * 7} intact.",
        f"Courier scan: OK. Pallet wraps: {2 + i % 5}. Weigh-in: {120 + i} kg.",
        f"Reference ticket: T-{900 + i}. Customs form CF-{300 + i} attached.",
        "Customer label on pallet was smudged; billing copy shows the account name."
        if unanswerable else f"Billing account: {customer}.",
        f"Duty {'prepaid' if duty else 'on delivery'}; surcharge code S{i % 4}.",
        f"Dock assignment changed twice; final assignment {dock}.",
    ]
    rng.shuffle(doc_lines)
    doc = "\n".join(["WAREHOUSE INTAKE SLIP v2", f"Order reference: {order_id}",
                     f"SKU: {sku}", f"Units per manifest: {quantity}", *doc_lines])
    gold = {"order_id": order_id, "customer": None if unanswerable else customer,
            "sku": sku, "quantity": quantity, "dock": dock, "duty_prepaid": duty}
    schema_hint = ('{"order_id": str, "customer": str or null, "sku": str, '
                   '"quantity": int, "dock": str, "duty_prepaid": bool}. '
                   "Use null when the slip does not state a value; the smudged-label "
                   "note does not count as stating it.")
    messages = [
        {"role": "system", "content": "You extract fields verbatim. Reply with only minified JSON."},
        {"role": "user", "content": f"Extract these fields per schema: {schema_hint}\n\nSLIP:\n{doc}"},
    ]

    def sample(q, rng2):
        wrong = rng2.random() > q
        out = dict(gold)
        if wrong:
            field = rng2.choice(list(gold))
            if field == "customer" and gold["customer"] is None:
                out["customer"] = rng2.choice(CUSTOMERS)  # fabrication
            elif field == "quantity":
                out["quantity"] = gold["quantity"] + rng2.choice([-1, 1, 2])
            elif field == "duty_prepaid":
                out["duty_prepaid"] = not gold["duty_prepaid"]
            elif field == "dock":
                out["dock"] = rng2.choice([d for d in DOCKS if d != gold["dock"]])
            else:
                out[field] = "XX-" + str(rng2.randint(100, 999))
        return json.dumps(out, separators=(",", ":"))

    return {"id": f"extraction-{i:02d}", "class": "deterministic-extraction",
            "families": ["hallucination", "artifact-quality"], "unanswerable": unanswerable,
            "messages": messages, "gold": gold, "sample_answer": sample}


def _tool_selection_task(i: int, rng: random.Random) -> dict:
    registry = "\n".join(f"- {k}: {v}" for k, v in TOOLS.items())
    scenarios = [
        (["list_orders", "read_file", "replace_item"],
         "A customer's goods arrived damaged and verified by the warehouse; pull their "
         "recent orders, check the damage photo policy file in the workspace, then ship "
         "the no-cost fix."),
        (["run_sql", "export_csv", "send_email"],
         "Finance needs last quarter's refund rows queried from the warehouse database, "
         "exported for audit, and mailed to the controller."),
        (["search_docs", "create_ticket", "page_oncall"],
         "Production inference latency is spiking; check the runbook for known causes, "
         "log the incident for the morning review, and wake the responder now."),
    ]
    need, scenario = scenarios[i % len(scenarios)]
    decoys = [t for t in TOOLS if t not in need]
    messages = [
        {"role": "system", "content": "Select tools. Reply with only a minified JSON array of tool names."},
        {"role": "user", "content": f"Tools:\n{registry}\n\nScenario: {scenario}"},
    ]
    gold = sorted(need)

    def sample(q, rng2):
        picked = list(gold)
        if rng2.random() > q:
            swaps = rng2.sample(decoys, k=min(2, len(decoys)))
            for slot in rng2.sample(range(len(picked)), k=len(swaps)):
                picked[slot] = swaps.pop()
        return json.dumps(sorted(picked), separators=(",", ":"))

    return {"id": f"toolsel-{i:02d}", "class": "tool-selection",
            "families": ["problem-solving"], "messages": messages,
            "gold": gold, "sample_answer": sample}


JUDGE_CASES = [
    # (candidate, gold verdict) — verdicts hinge on one detail.
    ("The refund was processed on day 30, and policy allows refunds within 30 days inclusive.",
     "pass"),
    ("The refund was processed on day 31; the agent noted the policy allows 30 days but "
     "approved it as a goodwill exception with manager sign-off.",
     "pass"),
    ("The outage credit was issued at the standard 5% even though the SLA table requires "
     "10% for outages exceeding the 4-hour threshold; the outage lasted 6 hours.",
     "fail"),
    ("The ticket was closed after the second contact attempt, which matches the "
     "two-attempt escalation policy.",
     "pass"),
    ("The customer data export was sent to the address on file, verified this morning.",
     "pass"),
    ("The escalation skipped the on-call rotation, but the SEV-2 policy permits skipping "
     "during documented maintENance windows.",
     "fail"),
]


def _eval_judge_task(i: int, rng: random.Random) -> dict:
    candidate, gold_verdict = JUDGE_CASES[i % len(JUDGE_CASES)]
    messages = [
        {"role": "system", "content": 'Grade strictly. Reply with only: {"verdict": "pass"|"fail"}'},
        {"role": "user", "content": f"Grading rubric: pass only if every policy claim is true AND "
                                    f"every deviation is explicitly authorized. Trivial-looking "
                                    f"numeric or day-count errors fail.\n\nCANDIDATE:\n{candidate}"},
    ]
    gold = {"verdict": gold_verdict}

    def sample(q, rng2):
        v = gold["verdict"] if rng2.random() <= q else ("fail" if gold_verdict == "pass" else "pass")
        return json.dumps({"verdict": v}, separators=(",", ":"))

    return {"id": f"judge-{i:02d}", "class": "eval-judge",
            "families": ["response-quality"], "messages": messages,
            "gold": gold, "sample_answer": sample}


CODE_TASKS = [
    ("slugify", 'Return the slug: lowercase, non-alphanumerics collapsed to single hyphens, stripped.',
     'def slugify(s):\n    out = ""\n    for ch in s.lower():\n        out += ch if ch.isalnum() else "-"\n    while "--" in out:\n        out = out.replace("--", "-")\n    return out.strip("-")',
     ['assert slugify("Hello, World!") == "hello-world"',
      'assert slugify("  A  B  ") == "a-b"',
      'assert slugify("Price: $5.99?!") == "price-5-99"',
      'assert slugify("--x--") == "x"',
      'assert slugify("Ünïcode_Äx") == "n-code-x"']),
    ("roman_to_int", "Convert a Roman numeral string to an integer.",
     'def roman_to_int(s):\n    vals = {"I": 1, "V": 5, "X": 10, "L": 50, "C": 100, "D": 500, "M": 1000}\n    total = 0\n    for i, ch in enumerate(s):\n        v = vals[ch]\n        if i + 1 < len(s) and vals[s[i + 1]] > v:\n            total -= v\n        else:\n            total += v\n    return total',
     ['assert roman_to_int("III") == 3', 'assert roman_to_int("IX") == 9',
      'assert roman_to_int("MCMXCIV") == 1994', 'assert roman_to_int("LVIII") == 58',
      'assert roman_to_int("CDXLIV") == 444']),
    ("flatten_dict", "Flatten nested dicts, joining keys with dots. Lists are values, not flattened.",
     'def flatten_dict(d):\n    out = {}\n    def walk(prefix, obj):\n        for k, v in obj.items():\n            key = prefix + "." + k if prefix else k\n            if isinstance(v, dict):\n                walk(key, v)\n            else:\n                out[key] = v\n    walk("", d)\n    return out',
     ['assert flatten_dict({"a": 1}) == {"a": 1}',
      'assert flatten_dict({"a": {"b": 2}}) == {"a.b": 2}',
      'assert flatten_dict({"a": {"b": {"c": 3}}}) == {"a.b.c": 3}',
      'assert flatten_dict({"a": [1, 2]}) == {"a": [1, 2]}',
      'assert flatten_dict({}) == {}']),
    ("balanced_brackets", "Return True if every bracket in the string closes correctly. Three types: (), [], {}.",
     'def balanced_brackets(s):\n    pairs = {")": "(", "]": "[", "}": "{"}\n    stack = []\n    for ch in s:\n        if ch in "([{":\n            stack.append(ch)\n        elif ch in pairs:\n            if not stack or stack[-1] != pairs[ch]:\n                return False\n            stack.pop()\n    return not stack',
     ['assert balanced_brackets("") is True',
      'assert balanced_brackets("([]{})") is True',
      'assert balanced_brackets("([)]") is False',
      'assert balanced_brackets("(((") is False',
      'assert balanced_brackets("no brackets here") is True',
      'assert balanced_brackets("{[()]}") is True']),
    ("top_words", "Return the k most frequent words (lowercased, split on whitespace) as a list, ties broken alphabetically.",
     'def top_words(text, k):\n    counts = {}\n    for w in text.lower().split():\n        counts[w] = counts.get(w, 0) + 1\n    return [w for w, _ in sorted(counts.items(), key=lambda kv: (-kv[1], kv[0]))[:k]]',
     ['assert top_words("b a b a c", 2) == ["a", "b"]',
      'assert top_words("B b c", 2) == ["b", "c"]',
      'assert top_words("one", 5) == ["one"]',
      'assert top_words("z y x w", 2) == ["w", "x"]',
      'assert top_words("a a a b b c", 3) == ["a", "b", "c"]']),
]


def _code_gen_task(i: int, rng: random.Random) -> dict:
    name, desc, impl, tests = CODE_TASKS[i % len(CODE_TASKS)]
    messages = [
        {"role": "system", "content": "Write a Python function. Reply with only the function, no fences."},
        {"role": "user", "content": f"Implement {name}(s). {desc}"},
    ]
    mutations = {
        "roman_to_int": ("total -= v", "total += v"),
        "slugify": ('.strip("-")', ""),
        "flatten_dict": ("if isinstance(v, dict):", "if False:"),
        "balanced_brackets": ("return not stack", "return True"),
        "top_words": ("(-kv[1], kv[0])", "(kv[1], kv[0])"),
    }
    old, new = mutations[name]
    broken = impl.replace(old, new)
    gold = {"name": name, "tests": tests}

    def sample(q, rng2):
        return impl if rng2.random() <= q else broken

    return {"id": f"codegen-{i:02d}", "class": "code-generation",
            "families": ["code-correctness"], "messages": messages,
            "gold": gold, "gold_impl": impl, "sample_answer": sample}


SUMMARY_DOCS = [
    ("Freight delays hit the north hub for a third week, and two carriers suspended "
     "pickups pending a safety review that concludes Friday. Retail accounts in the "
     "north should expect two extra days of lead time through month end. The south hub "
     "is unaffected, running at 60% capacity, and is absorbing overflow for one region "
     "only. Pricing team confirmed no tariff changes this cycle."),
    ("Usage grew 14% quarter over quarter, led by API customers, while churn held flat "
     "at 2%. Support tickets per account fell from 3.1 to 2.4 after the onboarding "
     "revamp. The databoard launch drove most new signups, and the mobile segment "
     "declined 3% for the third consecutive quarter. Finance re-forecast year-end "
     "revenue upward by 4%."),
    ("Security audit closed with no high findings; two medium findings concern stale "
     "service credentials, and rotation lands this sprint. The penetration-test "
     "follow-ups are tracked in the risk register with owners assigned. Access reviews "
     "will move to quarterly from biannual. No customer data was implicated in any "
     "finding."),
]


def _summarization_task(i: int, rng: random.Random) -> dict:
    doc = SUMMARY_DOCS[i % len(SUMMARY_DOCS)]
    required = sorted({w.strip(".,+%.") for w in doc.split() if w.isalpha() and len(w) > 5})[:9]
    gold = {"summary": doc, "required_terms": required}
    messages = [
        {"role": "system", "content": "Summarize in exactly two sentences, preserving all named specifics, numbers, and time frames."},
        {"role": "user", "content": doc},
    ]

    def sample(q, rng2):
        sentences = [s.strip() for s in doc.split(". ") if s.strip()]
        if rng2.random() > q:
            sentences = sentences[:max(1, int(len(sentences) * q))]
        answer = ". ".join(sentences)
        if not answer.endswith("."):
            answer += "."
        return answer

    return {"id": f"summarize-{i:02d}", "class": "summarization",
            "families": ["response-quality"], "messages": messages,
            "gold": gold, "sample_answer": sample}


def _conversational_task(i: int, rng: random.Random) -> dict:
    topics = ["where is my package?", "can I change the delivery address?",
              "my box arrived crushed", "can I redirect it to a pickup point?"]
    q1 = topics[i % len(topics)]
    q2 = topics[(i + 1) % len(topics)]
    system = ("You are ORBIT, a shipping support bot. Every reply must: contain exactly "
              "4 bullet points; start the first bullet with your name ORBIT; never "
              "mention prices, discounts, refunds, or supervisors.")
    messages = [
        {"role": "system", "content": system},
        {"role": "user", "content": q1},
        {"role": "assistant", "content": "- ORBIT here.\n- Track link sent.\n- Carrier updated today.\n- Reply here anytime."},
        {"role": "user", "content": q2},
    ]
    gold = {"bullets": 4, "banned": ["price", "discount", "refund", "$", "supervisor"],
            "must_contain": "orbit"}

    def sample(q, rng2):
        n = 4 if rng2.random() <= q else rng2.choice([3, 5])
        lines = [f"- Point {k} about your request." for k in range(1, n + 1)]
        if rng2.random() > q:
            target = rng2.randrange(len(lines))
            lines[target] = rng2.choice([
                "- We can also discuss a refund of the $12 fee.",
                "- Let me get my supervisor for a discount code.",
            ])
        if rng2.random() <= q:
            lines[0] = "- ORBIT checking on that now."
        return "\n".join(lines)

    return {"id": f"conv-{i:02d}", "class": "conversational",
            "families": ["response-quality"], "messages": messages,
            "gold": gold, "sample_answer": sample}


REFLECTION_DOC = (
    "Quarterly ops memo, v3. DEF-1: the invoice totals column double-counts freight "
    "on multi-pallet rows. Paragraph two reconciles cleanly. DEF-2: dates switch "
    "between ISO and US formats starting at the appendix tables. DEF-3: the customer "
    "Merro Street is misspelled as Merro Streent in the appendix. The duty column is "
    "fine. DEF-4: the summary block claims 98% on-time rate, but the underlying log "
    "computes 89%. Footnote formatting is acceptable. DEF-5: the region totals row is "
    "hardcoded from last quarter and does not recompute.")


def _reflection_task(i: int, rng: random.Random) -> dict:
    gold_ids = ["DEF-1", "DEF-2", "DEF-3", "DEF-4", "DEF-5"]
    messages = [
        {"role": "system", "content": 'List every defect id present in the text. Reply with only a minified JSON array.'},
        {"role": "user", "content": REFLECTION_DOC},
    ]

    def sample(q, rng2):
        found = [d for d in gold_ids if rng2.random() <= q]
        if rng2.random() > q:
            found.append("DEF-9")  # false positive
        return json.dumps(sorted(set(found)), separators=(",", ":"))

    return {"id": f"reflect-{i:02d}", "class": "reflection",
            "families": ["problem-solving"], "messages": messages,
            "gold": {"defects": gold_ids}, "sample_answer": sample}


MEMOS = [
    "MEMO A: The north hub paused outbound shipments pending the safety review that "
    "concludes Friday.",
    "MEMO B: South hub capacity is at 60% and can absorb overflow from one region only.",
    "MEMO C: Verified-damage orders qualify for no-cost replacement under policy 7.2; "
    "unverified damage is refund-eligible only with photo evidence.",
    "MEMO D: The damage photos for order INT-441 are on file and verified.",
]


def _synthesis_task(i: int, rng: random.Random) -> dict:
    required = ["north hub", "safety review", "verified damage"]
    forbidden = ["south hub paused", "photos were lost", "replacement is prohibited"]
    docs = "\n\n".join(MEMOS)
    messages = [
        {"role": "system", "content": "Synthesize across memos. Reply with one short paragraph; never invent facts."},
        {"role": "user", "content": "Question: which region must pause shipments, why, and what is the correct "
                                    "resolution path for order INT-441?\n\n" + docs},
    ]
    gold = {"required_claims": required, "forbidden": forbidden}

    def sample(q, rng2):
        parts = ["Shipments are paused at the north hub pending the safety review."]
        if rng2.random() <= q:
            parts.append("Order INT-441 has verified damage, so policy 7.2 applies.")
        else:
            parts.append(rng2.choice(forbidden[:1] + ["Order INT-441 photos were lost."]))
        if rng2.random() <= q:
            parts.append("The south hub can take the overflow once the review closes.")
        return " ".join(parts)

    return {"id": f"synth-{i:02d}", "class": "synthesis",
            "families": ["problem-solving", "hallucination"], "messages": messages,
            "gold": gold, "sample_answer": sample}


BANNED_TOKENS = ["alpha", "beta", "nova", "pulse", "orbit", "zenith", "falcon",
                 "comet", "vortex", "ember", "quartz", "nimbus", "onyx", "raven"]


def _creative_task(i: int, rng: random.Random) -> dict:
    banned = sorted(rng.sample(BANNED_TOKENS, 8) + [f"codename{x}" for x in range(8)])
    words = ["cedar", "lantern", "marble", "harbor", "thistle", "quarry", "willow", "copper"]
    good = "-".join([words[i % len(words)], words[(i + 3) % len(words)],
                     words[(i + 5) % len(words)]])
    messages = [
        {"role": "system", "content": "Reply with only the codename: kebab-case, exactly three lowercase words, no repeated word."},
        {"role": "user", "content": f"Invent a product codename. Banned tokens (none may appear, even as a substring of a word): {', '.join(banned)}."},
    ]
    gold = {"banned": banned, "words": 3}

    def sample(q, rng2):
        if rng2.random() <= q:
            return good
        return rng2.choice([
            banned[0],
            "Mixed Case-Name",
            f"{words[i % len(words)]}-{banned[1]}-tail",
            f"{words[i % len(words)]}-{words[(i + 3) % len(words)]}",  # only two words
        ])

    return {"id": f"creative-{i:02d}", "class": "creative",
            "families": ["response-quality"], "messages": messages,
            "gold": gold, "sample_answer": sample}


GENERATORS = {
    "deterministic-extraction": _extraction_task,
    "tool-selection": _tool_selection_task,
    "eval-judge": _eval_judge_task,
    "code-generation": _code_gen_task,
    "summarization": _summarization_task,
    "conversational": _conversational_task,
    "reflection": _reflection_task,
    "synthesis": _synthesis_task,
    "creative": _creative_task,
}


def build_suite(tasks_per_class: int, base_seed: int) -> list:
    tasks = []
    for cls in CLASSES:
        gen = GENERATORS[cls]
        for i in range(tasks_per_class):
            rng = random.Random(f"{base_seed}:{cls}:{i}")
            tasks.append(gen(i, rng))
    return tasks


# ------------------------- multi-step pipelines -------------------------

def build_pipeline(instance_id: int, pipeline: dict, base_seed: int) -> dict:
    """A fixed step graph: every step has its own class, prompt, gold, and
    checker; upstream answers feed downstream prompts; the final step's
    pass/fail is the pipeline completion endpoint (design section 4.3)."""
    order_id = f"ORD-{9000 + instance_id}"
    doc = (f"Intake slip. Order: {order_id}. Customer: Vantar Group. SKU: AX-110. "
           f"Quantity: 6. Note: customer reported damaged goods; photos on file?")
    review = ("Review notes: damage confirmed by warehouse team. Customer asked for a "
              "replacement, not a refund. Policy allows a no-cost replacement for "
              "verified damage.")
    tool_registry = "\n".join(f"- {k}: {v}" for k, v in TOOLS.items())
    tool_result = ("Order ORD found and eligible. Photo evidence: NOT ON FILE. "
                   "Policy: refunds without photo evidence are prohibited.")

    if pipeline["id"] == "order-decision":
        steps = [
            {"class": "deterministic-extraction",
             "messages": [
                 {"role": "system", "content": "Extract fields verbatim. Reply with only minified JSON."},
                 {"role": "user", "content": 'Fields: {"order_id": str, "customer": str, "issue": str}\n\nSLIP:\n' + doc}],
             "gold": {"order_id": order_id, "customer": "Vantar Group", "issue": "damaged goods"}},
            {"class": "summarization",
             "messages": [
                 {"role": "system", "content": "Summarize in one sentence, preserving the customer's ask."},
                 {"role": "user", "content": review}],
             "gold": {"summary": review, "required_terms": ["replacement", "refund", "damage", "verified"]}},
            {"class": "synthesis",
             "messages": [
                 {"role": "system", "content": "Synthesize a decision paragraph; never invent facts."},
                 {"role": "user", "content": "Given the intake slip, review notes, and that the north hub is "
                                             "paused while the south hub can absorb one region's overflow: "
                                             "what is the decision and which hub handles it? Our policy map: "
                                             "verified damage -> replacement; unverified -> refund."}],
             "gold": {"required_claims": ["replacement", "south"], "forbidden": ["north hub handles"]}},
        ]
    else:  # refund-loop
        steps = [
            {"class": "tool-selection",
             "messages": [
                 {"role": "system", "content": "Select tools. Reply with only a minified JSON array of tool names."},
                 {"role": "user", "content": f"Tools:\n{tool_registry}\n\nScenario: A customer wants a refund "
                                             f"for damaged goods; first check their orders, then act on the order."}],
             "gold": ["list_orders", "refund_order"]},
            {"class": "reflection",
             "messages": [
                 {"role": "system", "content": 'List every unmet requirement id present. Reply with only a minified JSON array.'},
                 {"role": "user", "content": "Requirement codes: PHOTO-MISSING = photo evidence not on file; "
                                             "ORDER-ELIGIBLE = order refund-eligible.\n\nTOOL RESULT:\n" + tool_result}],
             "gold": {"defects": ["PHOTO-MISSING"]}},
            {"class": "tool-selection",
             "messages": [
                 {"role": "system", "content": "Select the single next action. Reply with only a minified JSON array of one tool name."},
                 {"role": "user", "content": f"Tools:\n{tool_registry}\n\nReflection found PHOTO-MISSING: refunds "
                                             f"without photo evidence are prohibited.\n\nScenario: finalize the "
                                             f"correct next action for this request."}],
             "gold": ["create_ticket"]},
        ]

    sample_steps = pipeline.get("sample_steps")
    return {"id": f"pipe-{pipeline['id']}-{instance_id:02d}", "pipeline": pipeline["id"],
            "steps": steps, "sample_answer": _pipeline_step_sample}



def _pipeline_step_sample(quality: float, rng: random.Random, step: dict) -> str:
    cls = step["class"]
    correct = rng.random() <= quality
    if cls == "deterministic-extraction":
        g = dict(step["gold"])
        if not correct:
            g["customer"] = "Wrong Co"
        return json.dumps(g, separators=(",", ":"))
    if cls == "summarization":
        gold = step["gold"]["summary"]
        return gold if correct else gold.replace("replacement", "refund")
    if cls == "tool-selection":
        want = list(step["gold"])
        if not correct:
            want[0] = "send_email"
        return json.dumps(sorted(want), separators=(",", ":"))
    if cls == "reflection":
        want = list(step["gold"]["defects"])
        if not correct:
            want = ["ORDER-ELIGIBLE"]  # misses the real gap
        return json.dumps(sorted(set(want)), separators=(",", ":"))
    # synthesis
    base = ("Decision: ship a replacement from the south hub, since the north hub is "
            "paused and the damage is verified.")
    return base if correct else base.replace("replacement", "refund").replace("south", "north")
