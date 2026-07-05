"""LLM cost accounting for the arena (SYB-64).

Two cost sources, in priority order:

1. **Provider-reported cost (0% error).** OpenRouter returns the actual billed
   USD cost in the usage object when the request opts in via
   ``extra_body={"usage": {"include": True}}`` (see
   ``PersonaAnalyst._call_llm``). When that field is present we use it verbatim,
   so the recorded cost matches OpenRouter's billing exactly.

2. **Price-table fallback.** When the response carries no cost (an older/edge
   response, or a fixture without a cost field) we recompute from the raw token
   counts using the maintained table below. We always persist the raw token
   counts too, so a cost can be recomputed later if the table is corrected.

Prices below are USD per 1,000,000 tokens, **as of 2026-07-05**, taken from the
openrouter.ai model pages for the models the live arena calls. Update
``MODEL_PRICES`` (and this date) whenever a model or its price changes. Values
are only used for the fallback path; the provider-reported cost is authoritative
when present.
"""

from __future__ import annotations

import logging
from typing import Any

log = logging.getLogger(__name__)

# USD per 1,000,000 tokens: model -> (input_price, output_price).
# Prices as of 2026-07-05 (openrouter.ai). Fallback only; see module docstring.
MODEL_PRICES: dict[str, tuple[float, float]] = {
    "deepseek/deepseek-v4-flash": (0.10, 0.30),
    "google/gemma-4-31b-it": (0.05, 0.10),
}

# Used when a called model is absent from MODEL_PRICES. Deliberately on the high
# side so an un-priced model over-estimates spend (pauses early) rather than
# silently running the budget negative.
DEFAULT_PRICE_PER_M: tuple[float, float] = (1.0, 3.0)

_PER_MILLION = 1_000_000.0


def price_from_table(model: str, prompt_tokens: int, completion_tokens: int) -> float:
    """USD cost for a call, computed from the per-model price table."""
    in_price, out_price = MODEL_PRICES.get(model, DEFAULT_PRICE_PER_M)
    return (
        prompt_tokens * in_price + completion_tokens * out_price
    ) / _PER_MILLION


def _reported_cost(usage: Any) -> float | None:
    """Extract OpenRouter's billed USD cost from a usage object, if present.

    OpenRouter attaches ``cost`` (and sometimes ``cost_details``) to the usage
    object when usage accounting is requested. Returns a positive float or None.
    """
    if usage is None:
        return None
    cost = getattr(usage, "cost", None)
    if cost is None and isinstance(usage, dict):
        cost = usage.get("cost")
    if cost is None:
        return None
    try:
        cost = float(cost)
    except (TypeError, ValueError):
        return None
    return cost if cost > 0 else None


def cost_of_call(
    usage: Any,
    model: str,
    prompt_tokens: int,
    completion_tokens: int,
) -> tuple[float, str]:
    """Return ``(usd_cost, source)`` for one LLM call.

    ``source`` is ``"response"`` when the provider reported the cost (0% error)
    or ``"price_table"`` when it was recomputed from tokens. Never raises: any
    failure falls back to the price table so accounting cannot crash a caller.
    """
    try:
        reported = _reported_cost(usage)
        if reported is not None:
            return reported, "response"
        return price_from_table(model, prompt_tokens, completion_tokens), "price_table"
    except Exception:  # pragma: no cover - defensive; accounting must never crash
        log.debug("cost_of_call failed; charging $0", exc_info=True)
        return 0.0, "price_table"
