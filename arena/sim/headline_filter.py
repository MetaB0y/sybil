"""Phase 1 relevance filter: headline-only YES/NO classification via LLM,
then fetch full article text for accepted articles.

Usage:
    cd arena && uv run python -m sim.headline_filter --market iran --bot american_believer --date 2026-01-02
    cd arena && uv run python -m sim.headline_filter --market iran --bot israeli_trader --all-days
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

DEFAULT_MODEL = "google/gemini-3.1-flash-lite-preview"


def _load_market_config(market_name: str):
    """Load a MarketConfig by name."""
    import importlib
    mod = importlib.import_module(f"markets.{market_name}")
    return mod.get_config()


def load_articles_for_date(data_path: Path, target_date: date, sources: list[str]) -> list[dict]:
    """Load articles from the given sources on the given date."""
    with open(data_path) as f:
        raw = json.load(f)

    articles = []
    date_str = target_date.strftime("%Y%m%d")
    for chunk in raw["chunks"]:
        for art in chunk["articles"]:
            if not art["timestamp"].startswith(date_str):
                continue
            if art["source"] not in sources:
                continue
            articles.append(art)

    articles.sort(key=lambda a: a["timestamp"])
    return articles


def get_all_dates(data_path: Path, sources: list[str]) -> list[date]:
    """Return sorted list of all dates that have articles for the given sources."""
    with open(data_path) as f:
        raw = json.load(f)

    dates = set()
    for chunk in raw["chunks"]:
        for art in chunk["articles"]:
            if art["source"] not in sources:
                continue
            dates.add(date(int(art["timestamp"][:4]), int(art["timestamp"][4:6]), int(art["timestamp"][6:8])))

    return sorted(dates)


def output_path(phase1_dir: Path, bot_key: str, personas: dict, target_date: date) -> Path:
    phase1_key = personas.get(bot_key, {}).get("phase1_bot", bot_key)
    return phase1_dir / f"{phase1_key}_{target_date.strftime('%Y%m%d')}_phase1_results.json"


async def classify_headline(
    client: openai.AsyncOpenAI, model: str, prompt_template: str,
    headline: str, source: str,
) -> str:
    """Call LLM to classify a headline as YES or NO."""
    prompt = prompt_template.format(headline=headline, source=source)
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
    data_path: Path, prompt_template: str,
) -> dict:
    """Process all articles for a single date. Returns the results dict."""
    articles = load_articles_for_date(data_path, target_date, sources)
    results = []

    for art in articles:
        verdict = await classify_headline(client, model, prompt_template, art["title"], art["source"])
        results.append({
            "timestamp": art["timestamp"],
            "title": art["title"],
            "source": art["source"],
            "url": art["url"],
            "phase1": verdict,
            "full_text": None,
        })
        await asyncio.sleep(0.3)

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
    parser.add_argument("--market", required=True, help="Market name (e.g. iran)")
    parser.add_argument("--bot", required=True, help="Bot key from market personas")
    parser.add_argument("--date", type=str, default=None, help="Single date YYYY-MM-DD")
    parser.add_argument("--all-days", action="store_true", help="Process all available days")
    parser.add_argument("--model", default=DEFAULT_MODEL, help=f"LLM model (default: {DEFAULT_MODEL})")
    args = parser.parse_args()

    config = _load_market_config(args.market)

    if args.bot not in config.personas:
        print(f"Unknown bot: {args.bot}. Available: {', '.join(config.personas.keys())}")
        return

    if not args.date and not args.all_days:
        print("Specify --date YYYY-MM-DD or --all-days")
        return

    api_key = os.environ.get("OPENROUTER_API_KEY")
    if not api_key:
        print("Set OPENROUTER_API_KEY environment variable")
        return

    persona = config.personas[args.bot]
    sources = persona["sources"]

    # Find dataset file — use largest raw file (the merged one)
    data_paths = sorted(config.datasets_dir.glob("*_raw.json"))
    if not data_paths:
        print(f"No dataset files found in {config.datasets_dir}")
        return
    data_path = max(data_paths, key=lambda p: p.stat().st_size)

    client = openai.AsyncOpenAI(
        base_url="https://openrouter.ai/api/v1",
        api_key=api_key,
    )

    config.phase1_dir.mkdir(parents=True, exist_ok=True)

    if args.all_days:
        dates = get_all_dates(data_path, sources)
    else:
        dates = [date.fromisoformat(args.date)]

    pending = []
    for d in dates:
        if output_path(config.phase1_dir, args.bot, config.personas, d).exists():
            print(f"  Skipping {d} (already processed)")
        else:
            pending.append(d)

    if not pending:
        print("All dates already processed.")
        return

    print(f"Processing {len(pending)} day(s) for {persona['name']} ({args.model})\n")

    for i, d in enumerate(pending, 1):
        t0 = time.time()
        data = await process_date(
            args.bot, d, sources, client, args.model,
            data_path, config.phase1_prompt_template,
        )
        elapsed = time.time() - t0

        out = output_path(config.phase1_dir, args.bot, config.personas, d)
        out.write_text(json.dumps(data, indent=2))

        skip = data.get('skip', 0)
        skip_str = f", {skip} SKIP" if skip else ""
        print(f"[{i}/{len(pending)}] {d}: {data['total']} articles → {data['yes']} YES, {data['no']} NO{skip_str} ({elapsed:.0f}s)")

    print("\nDone.")


if __name__ == "__main__":
    asyncio.run(main())
