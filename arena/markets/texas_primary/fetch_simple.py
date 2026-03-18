"""Simple synchronous GDELT fetcher for Texas primary market.

Usage:
    cd arena && PYTHONUNBUFFERED=1 uv run python -m markets.texas_primary.fetch_simple
"""

import json
import time
from datetime import datetime, timedelta, timezone
from pathlib import Path
from urllib.parse import urlencode

import httpx

QUERIES = {
    "texas_primary_candidates": (
        "(cornyn OR paxton) (texas OR senate) "
        "(primary OR election OR race OR campaign OR poll)"
    ),
    "texas_primary_race": (
        "texas senate (republican OR gop OR primary OR runoff) "
        "(2026 OR campaign OR poll OR endorsement OR fundraising)"
    ),
    "texas_primary_trump": (
        "trump (cornyn OR paxton OR texas senate) "
        "(endorse OR support OR back OR primary)"
    ),
}

START = datetime(2026, 2, 10, tzinfo=timezone.utc)
END = datetime(2026, 3, 18, tzinfo=timezone.utc)
WINDOW_HOURS = 6
GDELT_URL = "https://api.gdeltproject.org/api/v2/doc/doc"
OUT_DIR = Path(__file__).parent / "datasets"


def fetch_one(client: httpx.Client, query: str, ws: datetime, we: datetime) -> dict:
    """Fetch one time window. Returns chunk dict. Retries on rate limit."""
    params = urlencode({
        "query": query, "mode": "artlist", "maxrecords": 250,
        "format": "json",
        "startdatetime": ws.strftime("%Y%m%d%H%M%S"),
        "enddatetime": we.strftime("%Y%m%d%H%M%S"),
        "sort": "datedesc",
    })
    url = f"{GDELT_URL}?{params}"
    label = ws.strftime("%Y-%m-%d %H:00")

    for attempt in range(10):
        try:
            r = client.get(url, timeout=30)
            text = r.text.strip()

            if r.status_code == 429 or "limit requests" in text.lower():
                time.sleep(20)
                continue

            if not text or text == "{}" or not text.startswith('{"articles"'):
                return {"window": label, "count": 0, "capped": False, "articles": []}

            data = json.loads(text)
            arts = [{
                "timestamp": a["seendate"],
                "title": a.get("title", ""),
                "source": a.get("domain", ""),
                "url": a.get("url", ""),
                "language": a.get("language", ""),
                "sourcecountry": a.get("sourcecountry", ""),
            } for a in data.get("articles", [])]

            return {"window": label, "count": len(arts), "capped": len(arts) >= 250, "articles": arts}

        except Exception as e:
            if attempt < 9:
                time.sleep(10)
            else:
                return {"window": label, "count": 0, "capped": False, "error": str(e), "articles": []}

    return {"window": label, "count": 0, "capped": False, "error": "max retries", "articles": []}


def save(path: Path, query: str, chunks: list, total: int, capped: list, failed: list):
    data = {
        "query": query,
        "period": f"{START.date()} to {END.date()}",
        "fetched_at": datetime.now(timezone.utc).isoformat(),
        "total_raw": total,
        "window_hours": WINDOW_HOURS,
        "capped_windows": capped,
        "failed_windows": failed,
        "chunks": chunks,
    }
    with open(path, "w") as f:
        json.dump(data, f)


def run_query(name: str, query: str):
    raw_path = OUT_DIR / f"{name}_raw.json"
    print(f"\n=== {name} ===")
    print(f"Query: {query[:80]}...")
    print(f"Period: {START.date()} to {(END - timedelta(days=1)).date()}")

    windows = []
    t = START
    while t < END:
        windows.append(t)
        t += timedelta(hours=WINDOW_HOURS)

    total = 0
    capped = []
    failed = []
    all_chunks = []
    client = httpx.Client()

    for i, ws in enumerate(windows):
        we = ws + timedelta(hours=WINDOW_HOURS)
        chunk = fetch_one(client, query, ws, we)
        all_chunks.append(chunk)
        total += chunk["count"]

        if chunk.get("capped"):
            capped.append(chunk["window"])
        if chunk.get("error"):
            failed.append(chunk["window"])

        wins_per_day = 24 // WINDOW_HOURS
        if (i + 1) % wins_per_day == 0:
            day = ws.strftime("%Y-%m-%d")
            day_arts = sum(c["count"] for c in all_chunks[-wins_per_day:])
            n_days = len(windows) // wins_per_day
            print(f"  [{(i+1)//wins_per_day}/{n_days}] {day}: {day_arts} articles (total: {total:,})", flush=True)
            save(raw_path, query, all_chunks, total, capped, failed)

        time.sleep(15)

    # Final save
    save(raw_path, query, all_chunks, total, capped, failed)
    mb = raw_path.stat().st_size / 1024 / 1024
    print(f"Done: {total:,} articles, {mb:.1f} MB")
    if capped:
        print(f"  CAPPED windows: {len(capped)}")
    if failed:
        print(f"  FAILED windows: {len(failed)}")

    client.close()


if __name__ == "__main__":
    OUT_DIR.mkdir(exist_ok=True)
    for name, query in QUERIES.items():
        run_query(name, query)
    print("\nAll done! Run merge_data.py next.")
