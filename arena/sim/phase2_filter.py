"""Phase 2 relevance filter: score phase1-YES articles against the specific
market question, then optionally demote LOW-relevance ones to NO.

Run after phase1, before the sim. Review the output, then pass --apply to
actually update the phase1 results files.

Usage:
    cd arena && uv run python -m sim.phase2_filter --market china_visit --all-days
    cd arena && uv run python -m sim.phase2_filter --market china_visit --all-days --apply
"""

import argparse
import asyncio
import json
import os
from pathlib import Path

import openai
from dotenv import load_dotenv

load_dotenv()

DEFAULT_MODEL = "google/gemini-3.1-flash-lite-preview"

PHASE2_PROMPT = """\
Rate this article's relevance to a prediction market question.

Question: "{analysis_question}"

Headline: "{title}"
Source: {source}
Excerpt: "{excerpt}"

- HIGH: Directly discusses the specific event/outcome in the question
- MEDIUM: Provides useful context — diplomatic relations, political dynamics, negotiations, or conditions that could influence the outcome
- LOW: Completely unrelated to the question — e.g. purely domestic policy, unrelated trade statistics, technology/business news, or other regions' affairs with no diplomatic connection

Answer only: HIGH, MEDIUM, or LOW"""


def _load_market_config(market_name: str):
    import importlib
    mod = importlib.import_module(f"markets.{market_name}")
    return mod.get_config()


async def _score_one(
    client: openai.AsyncOpenAI,
    model: str,
    title: str,
    source: str,
    excerpt: str,
    analysis_question: str,
    semaphore: asyncio.Semaphore,
) -> str:
    async with semaphore:
        prompt = PHASE2_PROMPT.format(
            analysis_question=analysis_question,
            title=title,
            source=source,
            excerpt=excerpt[:300],
        )
        try:
            resp = await client.chat.completions.create(
                model=model,
                messages=[{"role": "user", "content": prompt}],
                temperature=0.2,
                max_tokens=10,
            )
            text = (resp.choices[0].message.content or "").strip().upper()
            if "HIGH" in text:
                return "HIGH"
            if "MEDIUM" in text:
                return "MEDIUM"
            return "LOW"
        except Exception as e:
            print(f"    LLM error: {e}")
            return "MEDIUM"  # fail open


async def process_file(
    path: Path,
    analysis_question: str,
    client: openai.AsyncOpenAI,
    model: str,
    apply: bool,
) -> dict:
    """Score all YES articles in a phase1 results file. Returns stats."""
    data = json.loads(path.read_text())
    yes_items = [r for r in data["results"] if r.get("phase1") == "YES"]

    if not yes_items:
        return {"total": len(data["results"]), "yes": 0, "scored": 0, "low": 0}

    semaphore = asyncio.Semaphore(10)
    tasks = []
    for item in yes_items:
        excerpt = item.get("full_text", "") or ""
        tasks.append(_score_one(
            client, model, item["title"], item["source"],
            excerpt, analysis_question, semaphore,
        ))
    ratings = await asyncio.gather(*tasks)

    low_items = []
    for item, rating in zip(yes_items, ratings):
        item["phase2"] = rating
        if rating == "LOW":
            low_items.append(item)

    # Print LOW articles for review
    if low_items:
        for item in low_items:
            print(f"    LOW: [{item['source']}] {item['title']}")

    if apply and low_items:
        # Save backup before modifying
        backup = path.with_suffix(".pre_phase2.json")
        if not backup.exists():
            backup.write_text(path.read_text())
        for item in low_items:
            item["phase1"] = "NO"
            item["phase1_original"] = "YES"
        # Recount
        yes_count = sum(1 for r in data["results"] if r["phase1"] == "YES")
        data["yes"] = yes_count
        path.write_text(json.dumps(data, indent=2))

    return {
        "total": len(data["results"]),
        "yes": len(yes_items),
        "scored": len(yes_items),
        "low": len(low_items),
    }


async def main():
    parser = argparse.ArgumentParser(description="Phase 2 relevance filter for phase1 results")
    parser.add_argument("--market", required=True, help="Market name (e.g. china_visit)")
    parser.add_argument("--bot", default=None, help="Specific bot key (default: all bots)")
    parser.add_argument("--date", default=None, help="Single date YYYYMMDD")
    parser.add_argument("--all-days", action="store_true", help="Process all phase1 files")
    parser.add_argument("--model", default=DEFAULT_MODEL)
    parser.add_argument("--apply", action="store_true",
                        help="Actually demote LOW articles to NO in phase1 files (default: dry run)")
    args = parser.parse_args()

    config = _load_market_config(args.market)

    api_key = os.environ.get("OPENROUTER_API_KEY")
    if not api_key:
        print("Set OPENROUTER_API_KEY environment variable")
        return

    client = openai.AsyncOpenAI(
        base_url="https://openrouter.ai/api/v1",
        api_key=api_key,
    )

    # Find phase1 result files to process
    phase1_dir = config.phase1_dir
    if not phase1_dir.exists():
        print(f"Phase1 dir not found: {phase1_dir}")
        return

    if args.date and args.bot:
        files = [phase1_dir / f"{args.bot}_{args.date}_phase1_results.json"]
    elif args.date:
        files = sorted(phase1_dir.glob(f"*_{args.date}_phase1_results.json"))
    elif args.all_days and args.bot:
        files = sorted(phase1_dir.glob(f"{args.bot}_*_phase1_results.json"))
    elif args.all_days:
        files = sorted(phase1_dir.glob("*_phase1_results.json"))
    else:
        print("Specify --date YYYYMMDD or --all-days")
        return

    files = [f for f in files if f.exists()]
    if not files:
        print("No phase1 result files found.")
        return

    mode = "APPLY" if args.apply else "DRY RUN"
    print(f"Phase2 filter ({mode}): {len(files)} file(s), question: \"{config.analysis_question}\"\n")

    total_yes = 0
    total_low = 0

    for path in files:
        print(f"  {path.name}:")
        stats = await process_file(path, config.analysis_question, client, args.model, args.apply)
        total_yes += stats["yes"]
        total_low += stats["low"]
        action = "demoted" if args.apply else "would drop"
        print(f"    {stats['yes']} YES articles → {stats['low']} LOW ({action})\n")

    pct = (total_low / total_yes * 100) if total_yes > 0 else 0
    print(f"Total: {total_low}/{total_yes} LOW ({pct:.0f}%)")
    if not args.apply and total_low > 0:
        print("\nRe-run with --apply to demote LOW articles to NO in phase1 files.")


if __name__ == "__main__":
    asyncio.run(main())
