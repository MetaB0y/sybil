"""
Merge multiple GDELT raw JSON files into a single deduplicated dataset.

Usage:
    cd arena && uv run python -m markets.iran.merge_data

Reads all *_raw.json files in the datasets directory, merges articles,
deduplicates by URL, and writes the combined result back to iran_news_raw.json.
"""

import json
from pathlib import Path

OUT_DIR = Path(__file__).parent / "datasets"
MERGED_NAME = "iran_news_raw.json"

# Files to merge (order doesn't matter — we dedup by URL)
INPUT_FILES = [
    "iran_news_raw.json",
    "iran_news_diplomacy_raw.json",
]


def main():
    all_articles: list[dict] = []
    file_stats: list[tuple[str, int]] = []

    for fname in INPUT_FILES:
        path = OUT_DIR / fname
        if not path.exists():
            print(f"  SKIP {fname} (not found)")
            continue
        with open(path) as f:
            data = json.load(f)
        count = 0
        for chunk in data["chunks"]:
            count += len(chunk["articles"])
            all_articles.extend(chunk["articles"])
        file_stats.append((fname, count))
        print(f"  Loaded {fname}: {count:,} articles")

    total_raw = len(all_articles)
    print(f"\nTotal raw (with dupes): {total_raw:,}")

    # Dedup by URL, keeping first occurrence
    seen_urls: set[str] = set()
    unique: list[dict] = []
    for art in all_articles:
        url = art.get("url", "")
        if url and url not in seen_urls:
            seen_urls.add(url)
            unique.append(art)

    dupes = total_raw - len(unique)
    print(f"Duplicates removed:     {dupes:,}")
    print(f"Unique articles:        {len(unique):,}")

    # Sort by timestamp
    unique.sort(key=lambda a: a.get("timestamp", ""))

    # Re-chunk by hour (matching original format)
    from collections import defaultdict

    hourly: dict[str, list[dict]] = defaultdict(list)
    for art in unique:
        ts = art.get("timestamp", "")
        if len(ts) >= 11:
            hour_key = ts[:11] + "0000Z"  # e.g., 20260102T080000Z -> 20260102T08:00
            hour_label = f"{ts[0:4]}-{ts[4:6]}-{ts[6:8]} {ts[9:11]}:00"
        else:
            hour_label = "unknown"
        hourly[hour_label].append(art)

    chunks = []
    for hour_label in sorted(hourly.keys()):
        arts = hourly[hour_label]
        chunks.append({
            "hour": hour_label,
            "count": len(arts),
            "capped": False,
            "articles": arts,
        })

    # Build merged metadata
    merged = {
        "query": "MERGED: military + diplomacy keywords",
        "period": "2026-01-01 to 2026-02-19",
        "fetched_at": "merged",
        "total_raw": len(unique),
        "merge_sources": {fname: count for fname, count in file_stats},
        "duplicates_removed": dupes,
        "capped_hours": [],
        "failed_hours": [],
        "chunks": chunks,
    }

    # Back up original before overwriting
    orig = OUT_DIR / MERGED_NAME
    if orig.exists():
        backup = OUT_DIR / "iran_news_raw.json.bak"
        import shutil
        shutil.copy2(orig, backup)
        print(f"\nBacked up original to {backup.name}")

    with open(orig, "w") as f:
        json.dump(merged, f, indent=2)
    mb = orig.stat().st_size / 1024 / 1024
    print(f"Saved merged: {MERGED_NAME} ({mb:.1f} MB, {len(unique):,} articles)")


if __name__ == "__main__":
    main()
