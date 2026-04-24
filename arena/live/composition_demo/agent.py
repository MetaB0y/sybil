"""Agent logic for composition discovery, trade proposals, and draft creation."""

from __future__ import annotations

import json
import os
from typing import Any

try:
    from openai import OpenAI
except ImportError:  # pragma: no cover - optional unless OPENROUTER_API_KEY is set.
    OpenAI = None  # type: ignore[assignment]

from .registry import formula_conditions, formula_to_text, search_instruments, validate_formula
from .store import NANOS_PER_DOLLAR

MODEL = "deepseek/deepseek-v4-flash"

DISCOVERY_MODES = {"hedge", "news", "interview", "alpha", "create", "search"}


def discover(query: str, state: dict[str, Any], mode: str = "") -> dict[str, Any]:
    mode = normalize_discovery_mode(mode, query)
    if mode == "hedge":
        return discover_hedge(query, state)
    if mode == "news":
        return discover_news(query, state)
    if mode == "interview":
        return discover_interview(query, state)
    if mode == "alpha":
        return discover_alpha(query, state)
    if mode == "create":
        draft = deterministic_draft(query, state)
        ranked = [find_instrument(state, item) for item in formula_conditions(draft["formula"])]
        return discovery_payload(
            mode=mode,
            query=query,
            ranked=ranked,
            answer=(
                "I would start by drafting a new market definition, then validate duplicate and implication warnings "
                "before publishing. The ranked conditions below are candidate leaves for the formula."
            ),
            actions=[
                "Open the draft in the market creation wizard.",
                "Check every condition's measurement, window, and source.",
                "Publish only after duplicate checks show no equivalent definition.",
            ],
            thesis=draft["description"],
            creation_prompt=query,
        )
    return discover_search(query, state, mode="search")


def discover_search(query: str, state: dict[str, Any], mode: str = "search") -> dict[str, Any]:
    result = search_instruments(state["instruments"], query=query, limit=12)
    ranked = result["items"]
    recommendation = ranked[0] if ranked else None

    visible = ranked[:8]

    return {
        "mode": mode,
        "answer": build_discovery_answer(query, recommendation, ranked[:5]),
        "recommendation_id": recommendation["id"] if recommendation else None,
        "ranked_ids": [item["id"] for item in visible],
        "actions": [
            "Inspect the measurement and source behind the best match.",
            "Draft a proposition from the top conditions if no existing market matches.",
            "Check source and resolver primitive before approving a created market.",
        ],
        "questions": [],
        "thesis": "",
        "proxy_markets": [],
        "hedge_markets": [],
        "creation_prompt": "",
    }


def discover_hedge(query: str, state: dict[str, Any]) -> dict[str, Any]:
    lower = query.lower()
    domain = infer_domain(query)
    queries = [query]
    if domain == "crypto" or any(word in lower for word in ["long eth", "ethereum", "btc", "bitcoin", "sol", "defi"]):
        queries.extend(["crypto downside shock ETH below BTC below VIX recession", "ETH < 2000 BTC < 70k Crypto shock"])
    elif domain == "macro" or any(word in lower for word in ["stocks", "equity", "portfolio", "duration", "rates"]):
        queries.extend(["VIX recession hard landing drawdown unemployment", "VIX > 40 SPX drawdown Hard landing"])
    elif domain == "politics":
        queries.extend(["party control sweep election hedge president senate house", "Dem sweep Republican president"])
    else:
        queries.extend(["VIX hard landing recession drawdown", "crypto shock macro stress"])
    ranked = ranked_for_queries(state, queries, kinds=["proposition", "condition"], limit=10)
    return discovery_payload(
        mode="hedge",
        query=query,
        ranked=ranked,
        answer=(
            "For a hedge, prefer markets that pay in the bad state you are worried about, not just markets about the same topic. "
            "I ranked direct downside contracts first, then broader proxy markets that may respond if the risk spills into macro or risk assets."
        ),
        actions=[
            "Separate direct hedges from proxy hedges before trading.",
            "Check whether the payoff date matches the exposure you are hedging.",
            "Use smaller size on proxy markets because basis risk can dominate.",
        ],
        questions=[
            "What position are you hedging, and is the risk mostly price, timing, or event-driven?",
            "What loss scenario should the hedge pay in?",
            "When do you need the hedge to work?",
        ],
        thesis="Find markets that pay when the user's existing exposure loses value.",
        hedge_markets=[item["id"] for item in ranked[:6]],
    )


def discover_news(query: str, state: dict[str, Any]) -> dict[str, Any]:
    lower = query.lower()
    queries = [query]
    if any(word in lower for word in ["iran", "strike", "troop", "aumf", "war", "nuclear"]):
        queries.extend(["Iran escalation troops strikes AUMF occupation", "Iran hawkish Iran mainstream Iran legal"])
    elif any(word in lower for word in ["cpi", "inflation", "fed", "rates", "jobs", "unemployment"]):
        queries.extend(["CPI Fed funds unemployment hard landing VIX", "Inflation scare hard landing"])
    elif any(word in lower for word in ["injury", "nba", "tatum", "knicks", "celtics"]):
        queries.extend(["Tatum injury Celtics same-game parlay Knicks Celtics", "Tatum points Celtics win NBA"])
    elif any(word in lower for word in ["ai", "model", "benchmark", "openai", "anthropic"]):
        queries.extend(["AI benchmark FrontierMath ARC AGI SWE-bench", "frontier AI benchmark"])
    else:
        queries.extend(["proxy market related market underappreciated"])
    ranked = ranked_for_queries(state, queries, kinds=["proposition", "condition"], limit=10)
    return discovery_payload(
        mode="news",
        query=query,
        ranked=ranked,
        answer=(
            "I treated the news as a catalyst and looked for proxy markets whose probability should move if the news is genuinely important. "
            "The main risk is false precision: a clean proxy can still miss the mechanism in the story."
        ),
        actions=[
            "Identify the causal channel from the news to the market.",
            "Prefer a narrow condition if the news maps directly to one measurement.",
            "Use a broader definition only when the direct market is missing or too noisy.",
        ],
        questions=[
            "What exactly changed in the news versus prior expectations?",
            "Which market should move first if your interpretation is right?",
            "Is the edge about probability, timing, or market attention?",
        ],
        thesis="Translate news into direct or proxy prediction markets.",
        proxy_markets=[item["id"] for item in ranked[:6]],
    )


def discover_interview(query: str, state: dict[str, Any]) -> dict[str, Any]:
    domain = infer_domain(query)
    ranked = ranked_for_queries(
        state,
        [query, f"{domain} broad basket" if domain else "crypto macro politics geopolitics sports"],
        domains=[domain] if domain else [],
        kinds=["proposition", "condition"],
        limit=8,
    )
    return discovery_payload(
        mode="interview",
        query=query,
        ranked=ranked,
        answer=(
            "I would interview you into a small portfolio of claims, then create missing markets only after the existing graph fails to express the thesis. "
            "The goal is to turn vague conviction into measurable conditions with dates and sources."
        ),
        actions=[
            "Answer the follow-up questions, then rerun discovery with the strongest claim.",
            "Inspect the ranked definitions for the closest existing expression.",
            "Use the wizard for any claim that needs a custom formula.",
        ],
        questions=[
            "What do you believe that the current market is underpricing?",
            "What observable event would prove you right within 3 to 18 months?",
            "Which related outcome would make your thesis wrong?",
        ],
        thesis="Interview the user until opinions become measurable, tradable claims.",
    )


def discover_alpha(query: str, state: dict[str, Any]) -> dict[str, Any]:
    ranked = ranked_for_queries(
        state,
        [query, expand_prompt(query), "direct condition proxy market composition"],
        kinds=["condition", "proposition"],
        limit=10,
    )
    return discovery_payload(
        mode="alpha",
        query=query,
        ranked=ranked,
        answer=(
            "For alpha, I would first look for the tightest direct market, then a liquid proxy, then a custom definition. "
            "The hard part is not finding a contract; it is avoiding a proxy whose resolution path does not capture the information advantage."
        ),
        actions=[
            "Choose the narrowest market that the alpha actually predicts.",
            "Check live market creation and quote status before sizing.",
            "Create a custom definition only if existing markets add too much basis risk.",
        ],
        questions=[
            "What is the private or underweighted information?",
            "Which existing condition should reprice if the alpha is real?",
            "How quickly do you expect the market to notice?",
        ],
        thesis="Monetize a concrete informational edge through the closest tradable condition or proxy.",
        proxy_markets=[item["id"] for item in ranked[:6]],
        creation_prompt=query,
    )


def discovery_payload(
    *,
    mode: str,
    query: str,
    ranked: list[dict[str, Any]],
    answer: str,
    actions: list[str],
    questions: list[str] | None = None,
    thesis: str = "",
    proxy_markets: list[str] | None = None,
    hedge_markets: list[str] | None = None,
    creation_prompt: str = "",
) -> dict[str, Any]:
    visible = ranked[:8]
    recommendation = visible[0] if visible else None
    if not visible:
        answer = f"{answer} I did not find a close existing market, so the next step is a wizard draft."
    return {
        "mode": mode,
        "answer": answer,
        "recommendation_id": recommendation["id"] if recommendation else None,
        "ranked_ids": [item["id"] for item in visible],
        "actions": actions,
        "questions": questions or [],
        "thesis": thesis,
        "proxy_markets": proxy_markets or [],
        "hedge_markets": hedge_markets or [],
        "creation_prompt": creation_prompt or query,
    }


def normalize_discovery_mode(mode: str, query: str) -> str:
    value = str(mode or "").strip().lower().replace("_", "-")
    aliases = {
        "hedging": "hedge",
        "risk": "hedge",
        "news-proxy": "news",
        "proxy": "news",
        "opinion": "interview",
        "opinions": "interview",
        "find-bets": "interview",
        "monetize": "alpha",
        "trade-alpha": "alpha",
        "draft": "create",
        "wizard": "create",
    }
    value = aliases.get(value, value)
    if value in DISCOVERY_MODES:
        return value
    lower = query.lower()
    if any(word in lower for word in ["hedge", "worried", "protect", "risk", "exposure"]):
        return "hedge"
    if any(word in lower for word in ["news", "headline", "underappreciated", "catalyst"]):
        return "news"
    if any(word in lower for word in ["interview", "opinions", "find bets"]):
        return "interview"
    if any(word in lower for word in ["alpha", "make money", "edge"]):
        return "alpha"
    return "search"


def ranked_for_queries(
    state: dict[str, Any],
    queries: list[str],
    *,
    domains: list[str] | None = None,
    kinds: list[str] | None = None,
    limit: int = 8,
) -> list[dict[str, Any]]:
    rows: dict[str, dict[str, Any]] = {}
    domain_values = domains if domains else [""]
    kind_values = kinds if kinds else [""]
    for query_index, query in enumerate(queries):
        for domain in domain_values:
            for kind in kind_values:
                for item in search_instruments(state["instruments"], query=query, domain=domain, kind=kind, limit=limit * 2)[
                    "items"
                ]:
                    existing = rows.get(item["id"])
                    base_score = float(item.get("search_score", 0)) + (len(queries) - query_index) * 0.25
                    if item.get("object_kind") == "proposition":
                        base_score += 0.2
                    if item.get("market_id") is not None:
                        base_score += 0.1
                    candidate = dict(item)
                    candidate["_agent_score"] = base_score
                    if existing is None or base_score > existing.get("_agent_score", 0):
                        rows[item["id"]] = candidate
    ranked = sorted(rows.values(), key=lambda item: item.get("_agent_score", 0), reverse=True)
    return [strip_private_score(item) for item in ranked[:limit]]


def strip_private_score(item: dict[str, Any]) -> dict[str, Any]:
    row = dict(item)
    row.pop("_agent_score", None)
    return row


def build_discovery_answer(query: str, recommendation: dict[str, Any] | None, ranked: list[dict[str, Any]]) -> str:
    if not recommendation:
        return "I could not find a matching composition. Draft a new one from the creation panel."
    names = ", ".join(item["short_name"] for item in ranked)
    return (
        f"For '{query}', I would start with {recommendation['short_name']} "
        f"({recommendation.get('domain', 'unknown')}/{recommendation.get('object_kind', recommendation['kind'])}). "
        f"Nearby candidates: {names}. Use these as leaves for a new formula if no single proposition matches."
    )


def propose_trade(payload: dict[str, Any], state: dict[str, Any]) -> dict[str, Any]:
    instrument = find_instrument(state, payload.get("instrument_id"))
    side_hint = str(payload.get("side") or payload.get("intent") or "").lower()
    side = "BUY_NO" if any(word in side_hint for word in ["no", "against", "short"]) else "BUY_YES"
    market = instrument.get("market") or {}
    yes_price = (market.get("yes_price_nanos") or int(instrument.get("fair_value", 0.5) * NANOS_PER_DOLLAR)) / NANOS_PER_DOLLAR
    limit = 1.0 - yes_price if side == "BUY_NO" else yes_price
    limit = min(0.99, max(0.01, limit + 0.02))
    quantity = int(payload.get("quantity") or 25)
    return {
        "instrument_id": instrument["id"],
        "market_id": instrument.get("market_id"),
        "side": side,
        "limit_price": round(limit, 4),
        "quantity": quantity,
        "notional": round(quantity * limit, 2),
        "rationale": (
            f"{instrument['short_name']} is the cleanest target for this thesis. "
            "This is a proposal only; confirm in the trade ticket to submit."
        ),
    }


def explain_instrument(instrument_id: str, state: dict[str, Any]) -> dict[str, Any]:
    item = find_instrument(state, instrument_id)
    leaves = [find_instrument(state, leaf) for leaf in formula_conditions(item.get("formula"))]
    return {
        "instrument_id": item["id"],
        "summary": item["description"],
        "formula_text": formula_to_text(item.get("formula")),
        "leaves": [
            {
                "id": leaf["id"],
                "short_name": leaf["short_name"],
                "oracle_path": leaf["oracle_path"],
                "fair_value": leaf.get("fair_value"),
            }
            for leaf in leaves
        ],
    }


def draft_composition(prompt: str, state: dict[str, Any]) -> dict[str, Any]:
    if os.environ.get("OPENROUTER_API_KEY"):
        drafted = draft_with_llm(prompt, state)
        if drafted:
            validation = validate_formula(drafted.get("formula"), state["instruments"])
            if validation["valid"]:
                return drafted
    return deterministic_draft(prompt, state)


def draft_with_llm(prompt: str, state: dict[str, Any]) -> dict[str, Any] | None:
    if OpenAI is None:
        return None
    domain = infer_domain(prompt)
    conditions = [
        {
            "id": item["id"],
            "short_name": item["short_name"],
            "description": item["description"],
            "domain": item.get("domain"),
            "predicate": item.get("predicate"),
            "measurement_id": item.get("measurement_id"),
            "source": item.get("source"),
        }
        for item in search_instruments(
            state["instruments"],
            query=expand_prompt(prompt),
            domain=domain,
            kind="condition",
            limit=80,
        )["items"]
    ]
    system = (
        "You draft prediction-market compositions. Return strict JSON only with keys: "
        "title, short_name, question, description, formula. Formula leaves use {'condition': id} "
        "or {'op':'AND'|'OR'|'NOT'|'K_OF_N'|'IF_THEN','args':[...]} and may only reference provided condition ids."
    )
    user = f"Available conditions:\n{json.dumps(conditions)}\n\nUser request:\n{prompt}"
    try:
        client = OpenAI(
            base_url="https://openrouter.ai/api/v1",
            api_key=os.environ["OPENROUTER_API_KEY"],
            timeout=45.0,
            max_retries=0,
        )
        resp = client.chat.completions.create(
            model=MODEL,
            messages=[{"role": "system", "content": system}, {"role": "user", "content": user}],
            temperature=0.2,
            max_tokens=1200,
            extra_body={"reasoning": {"max_tokens": 512}},
        )
        text = resp.choices[0].message.content or ""
        start = text.find("{")
        end = text.rfind("}")
        if start < 0 or end < start:
            return None
        data = json.loads(text[start : end + 1])
        return normalize_draft(data)
    except Exception:
        return None


def deterministic_draft(prompt: str, state: dict[str, Any]) -> dict[str, Any]:
    lower = prompt.lower()
    domain = infer_domain(prompt)
    conditions = search_instruments(
        state["instruments"],
        query=expand_prompt(prompt),
        domain=domain,
        kind="condition",
        limit=24,
    )["items"]
    conditions = pick_diverse_conditions(prompt, conditions)[:6]
    if len(conditions) < 2:
        conditions = [
            item
            for item in state["instruments"]
            if item["kind"] == "condition" and (not domain or item.get("domain") == domain)
        ][:6]
    if len(conditions) < 2:
        conditions = [item for item in state["instruments"] if item["kind"] == "condition"][:6]
    if not conditions:
        raise ValueError("no conditions available to draft from")

    args = [{"condition": item["id"]} for item in conditions[: min(4, len(conditions))]]
    if ("all" in lower or "and" in lower or "parlay" in lower or "strict" in lower) and len(args) >= 2:
        formula = {"op": "AND", "args": args}
        short = "All selected"
        desc = "Agent draft requiring every selected condition to resolve YES."
    elif ("at least" in lower or "k of" in lower or "basket" in lower or "recession" in lower) and len(args) >= 3:
        formula = {"op": "K_OF_N", "k": 2, "args": args}
        short = "Two-of basket"
        desc = "Agent draft requiring at least two of the selected conditions."
    elif ("if" in lower or "conditional" in lower) and len(args) >= 2:
        formula = {"op": "IF_THEN", "args": args[:2]}
        short = "Conditional"
        desc = "Agent draft expressing a conditional relationship between two selected conditions."
    else:
        formula = {"op": "OR", "args": args}
        short = "Any selected"
        desc = "Agent draft paying if any selected condition resolves YES."
    domain = domain or conditions[0].get("domain", "custom")
    return normalize_draft(
        {
            "title": f"{short} composition for {prompt[:56]}",
            "short_name": short,
            "question": f"Will the {short.lower()} formula for '{prompt[:80]}' resolve YES?",
            "description": desc,
            "formula": formula,
            "domain": domain,
            "tags": ["composition-demo", domain, "agent-draft"],
        }
    )


def infer_domain(prompt: str) -> str:
    lower = prompt.lower()
    rules = [
        ("macro", ["macro", "recession", "inflation", "fed", "gdp", "unemployment", "sahm", "cpi", "vix"]),
        ("politics", ["election", "president", "primary", "nomination", "senate", "house", "candidate"]),
        ("geopolitics", ["iran", "ukraine", "taiwan", "war", "strike", "invasion", "conflict"]),
        ("sports", ["nba", "sports", "team", "game", "player", "points", "rebounds", "assists", "parlay"]),
        ("technology", ["ai", "agi", "benchmark", "frontier", "model", "lab", "openai", "anthropic"]),
        ("crypto", ["crypto", "btc", "bitcoin", "eth", "ethereum", "sol", "solana", "hype"]),
        ("culture", ["movie", "album", "music", "release", "drake", "taylor"]),
    ]
    for domain, needles in rules:
        if any(needle in lower for needle in needles):
            return domain
    return ""


def pick_diverse_conditions(prompt: str, conditions: list[dict[str, Any]]) -> list[dict[str, Any]]:
    lower = prompt.lower()
    if "recession" in lower:
        priorities = ["gdp", "sahm", "unemployment", "drawdown", "vix", "fed"]
        ranked: list[dict[str, Any]] = []
        used_indicators: set[str] = set()
        for priority in priorities:
            for condition in conditions:
                indicator = str(condition.get("short_name", condition.get("id", ""))).lower()
                if priority in indicator and indicator not in used_indicators:
                    ranked.append(condition)
                    used_indicators.add(indicator)
                    break
        for condition in conditions:
            indicator = str(condition.get("short_name", condition.get("id", ""))).lower()
            if indicator not in used_indicators:
                ranked.append(condition)
                used_indicators.add(indicator)
        return ranked
    return conditions


def expand_prompt(prompt: str) -> str:
    lower = prompt.lower()
    expansions = [prompt]
    if "recession" in lower:
        expansions.append("GDP unemployment Sahm drawdown VIX")
    elif "macro" in lower:
        expansions.append("GDP unemployment Sahm Fed funds CPI drawdown VIX")
    if any(word in lower for word in ["nomination", "primary"]):
        expansions.append("presidential nomination primary candidate contest")
    if "agi" in lower:
        expansions.append("AI benchmark FrontierMath ARC AGI SWE-bench")
    if "iran" in lower or "invasion" in lower:
        expansions.append("Iran troops strikes declaration AUMF occupation")
    return " ".join(expansions)


def normalize_draft(data: dict[str, Any]) -> dict[str, Any]:
    return {
        "id": data.get("id", ""),
        "kind": "proposition",
        "object_kind": "proposition",
        "title": data["title"],
        "short_name": data.get("short_name") or data["title"][:24],
        "question": data["question"],
        "description": data["description"],
        "oracle_path": "Formula over graph conditions",
        "formula": data["formula"],
        "author": "Agent draft",
        "fair_value": 0.15,
        "trust_tier": "demo-draft",
        "tags": data.get("tags", ["composition-demo", "agent-draft"]),
        "domain": data.get("domain", "custom"),
        "atom_type": "composition",
        "subject": data.get("short_name") or data["title"][:24],
        "metric": "formula",
        "comparator": "resolves_true",
        "threshold": None,
        "unit": "",
        "time_window": "user-defined",
        "resolver_primitive": "predicate_formula",
        "source": "agent",
        "source_url": "",
        "canonical_key": data.get("id", "") or data["title"],
        "compatible_ops": ["AND", "OR", "NOT", "K_OF_N", "IF_THEN"],
        "exclusivity_group": None,
        "template_id": "proposition",
        "params": {},
        "quality": "agent_draft",
        "aliases": [],
    }


def find_instrument(state: dict[str, Any], instrument_id: str | None) -> dict[str, Any]:
    for item in state["instruments"]:
        if item["id"] == instrument_id:
            return item
    raise KeyError(f"unknown instrument: {instrument_id}")
