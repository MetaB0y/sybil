"""Phase 1 relevance filter: headline-only YES/NO classification via LLM,
then fetch full article text for accepted articles.

Usage:
    cd arena && uv run python -m iran.phase1_filter --bot american_believer --date 2026-01-02
    cd arena && uv run python -m iran.phase1_filter --bot israeli_trader --all-days
    cd arena && uv run python -m iran.phase1_filter --bot israeli_trader --all-days --model moonshotai/kimi-k2
"""

import argparse
import asyncio
import json
import os
import time
from datetime import date
from pathlib import Path

import httpx
import openai
import trafilatura
from dotenv import load_dotenv

load_dotenv()

from .news_explorer import BOT_PERSONAS, DATASETS_DIR, PHASE1_DIR

DATA_PATH = DATASETS_DIR / "iran_news_raw.json"

PHASE1_PROMPT = """\
You are screening news headlines for a prediction market on US-Iran military conflict.

Market: "Will the US carry out a military strike against Iran before March 31, 2026?"

Headline: "{headline}" -- {source}

Is this headline related to any of the following?
- US-Iran tensions, threats, or diplomatic signals
- Military moves, troop deployments, or defense posture involving US or Iran
- Iran nuclear program or sanctions
- Regional conflicts involving Iran (proxies, Israel, Gulf states)
- Iran domestic unrest that could trigger US intervention
- Market/economic reactions to US-Iran tensions (oil, safe havens, defense stocks)

Say YES if the headline touches any of these topics, even indirectly.
Say NO only if the headline is clearly unrelated to US-Iran dynamics.
When in doubt, say YES -- it is cheap to filter later, expensive to miss relevant news.

Answer only YES or NO."""

DEFAULT_MODEL = "moonshotai/kimi-k2"


def load_articles_for_date(target_date: date, sources: list[str]) -> list[dict]:
    """Load English articles from the given sources on the given date."""
    with open(DATA_PATH) as f:
        raw = json.load(f)

    articles = []
    date_str = target_date.strftime("%Y%m%d")
    for chunk in raw["chunks"]:
        for art in chunk["articles"]:
            # Filter: date match, source match, English only
            if not art["timestamp"].startswith(date_str):
                continue
            if art["source"] not in sources:
                continue
            articles.append(art)

    articles.sort(key=lambda a: a["timestamp"])
    return articles


def get_all_dates(sources: list[str]) -> list[date]:
    """Return sorted list of all dates that have articles for the given sources."""
    with open(DATA_PATH) as f:
        raw = json.load(f)

    dates = set()
    for chunk in raw["chunks"]:
        for art in chunk["articles"]:
            if art["source"] not in sources:
                continue
            # Parse YYYYMMDD from timestamp like "20260102T080000Z"
            dates.add(date(int(art["timestamp"][:4]), int(art["timestamp"][4:6]), int(art["timestamp"][6:8])))

    return sorted(dates)


def output_path(bot_key: str, target_date: date) -> Path:
    # Use phase1_bot for filename so siblings (e.g. american_believer + american_skeptic)
    # share one result file, matching what _resolve_phase1_path() expects.
    phase1_key = BOT_PERSONAS.get(bot_key, {}).get("phase1_bot", bot_key)
    return PHASE1_DIR / f"{phase1_key}_{target_date.strftime('%Y%m%d')}_phase1_results.json"


async def classify_headline(
    client: openai.AsyncOpenAI, model: str, headline: str, source: str,
) -> str:
    """Call LLM to classify a headline as YES or NO."""
    prompt = PHASE1_PROMPT.format(headline=headline, source=source)
    try:
        resp = await client.chat.completions.create(
            model=model,
            messages=[{"role": "user", "content": prompt}],
            temperature=0.2,
            max_tokens=10,
        )
        text = (resp.choices[0].message.content or "").strip().upper()
        if "YES" in text:
            return "YES"
        if "NO" in text:
            return "NO"
        return "NO"
    except Exception as e:
        print(f"    LLM error: {e}")
        return "NO"


async def fetch_article_text(http: httpx.AsyncClient, url: str) -> str | None:
    """Fetch and extract article text from a URL using trafilatura."""
    try:
        resp = await http.get(url, follow_redirects=True)
        if resp.status_code != 200:
            return None
        text = trafilatura.extract(resp.text)
        return text or None
    except Exception:
        return None


async def process_date(
    bot_key: str, target_date: date, sources: list[str],
    client: openai.AsyncOpenAI, model: str,
) -> dict:
    """Process all articles for a single date. Returns the results dict."""
    articles = load_articles_for_date(target_date, sources)
    results = []

    # Phase 1: classify headlines
    for art in articles:
        verdict = await classify_headline(client, model, art["title"], art["source"])
        results.append({
            "timestamp": art["timestamp"],
            "title": art["title"],
            "source": art["source"],
            "url": art["url"],
            "phase1": verdict,
            "full_text": None,
        })
        await asyncio.sleep(0.3)

    # Fetch full text for YES articles
    yes_results = [r for r in results if r["phase1"] == "YES"]
    if yes_results:
        async with httpx.AsyncClient(timeout=15.0, headers={
            "User-Agent": "Mozilla/5.0 (compatible; NewsBot/1.0)",
        }) as http:
            for r in yes_results:
                text = await fetch_article_text(http, r["url"])
                if text:
                    r["full_text"] = text
                else:
                    r["phase1"] = "SKIP"
                await asyncio.sleep(0.3)

    yes_count = sum(1 for r in results if r["phase1"] == "YES")
    skip_count = sum(1 for r in results if r["phase1"] == "SKIP")
    no_count = sum(1 for r in results if r["phase1"] == "NO")

    return {
        "bot": bot_key,
        "window": target_date.isoformat(),
        "model": model,
        "total": len(results),
        "yes": yes_count,
        "skip": skip_count,
        "no": no_count,
        "results": results,
    }


async def main():
    parser = argparse.ArgumentParser(description="Phase 1 headline relevance filter")
    parser.add_argument("--bot", required=True, help="Bot key from BOT_PERSONAS")
    parser.add_argument("--date", type=str, default=None, help="Single date YYYY-MM-DD")
    parser.add_argument("--all-days", action="store_true", help="Process all available days")
    parser.add_argument("--model", default=DEFAULT_MODEL, help=f"LLM model (default: {DEFAULT_MODEL})")
    args = parser.parse_args()

    if args.bot not in BOT_PERSONAS:
        print(f"Unknown bot: {args.bot}. Available: {', '.join(BOT_PERSONAS.keys())}")
        return

    if not args.date and not args.all_days:
        print("Specify --date YYYY-MM-DD or --all-days")
        return

    api_key = os.environ.get("OPENROUTER_API_KEY")
    if not api_key:
        print("Set OPENROUTER_API_KEY environment variable")
        return

    persona = BOT_PERSONAS[args.bot]
    sources = persona["sources"]

    client = openai.AsyncOpenAI(
        base_url="https://openrouter.ai/api/v1",
        api_key=api_key,
    )

    PHASE1_DIR.mkdir(parents=True, exist_ok=True)

    # Determine dates to process
    if args.all_days:
        dates = get_all_dates(sources)
    else:
        dates = [date.fromisoformat(args.date)]

    # Filter out already-processed dates
    pending = []
    for d in dates:
        if output_path(args.bot, d).exists():
            print(f"  Skipping {d} (already processed)")
        else:
            pending.append(d)

    if not pending:
        print("All dates already processed.")
        return

    print(f"Processing {len(pending)} day(s) for {persona['name']} ({args.model})\n")

    for i, d in enumerate(pending, 1):
        t0 = time.time()
        data = await process_date(args.bot, d, sources, client, args.model)
        elapsed = time.time() - t0

        # Save
        out = output_path(args.bot, d)
        out.write_text(json.dumps(data, indent=2))

        skip = data.get('skip', 0)
        skip_str = f", {skip} SKIP" if skip else ""
        print(f"[{i}/{len(pending)}] {d}: {data['total']} articles → {data['yes']} YES, {data['no']} NO{skip_str} ({elapsed:.0f}s)")

    print("\nDone.")


if __name__ == "__main__":
    asyncio.run(main())
