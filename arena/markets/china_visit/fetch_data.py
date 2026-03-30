"""
Fetch news articles from GDELT API for the China visit prediction market.

Usage:
    cd arena && uv run python -m markets.china_visit.fetch_data
    cd arena && uv run python -m markets.china_visit.fetch_data --test   # 1-day test run

Fetches in configurable time windows to stay under GDELT's 250-result cap.
Sequential requests with delay to respect GDELT rate limits (1 req per 5s).

Output files (in datasets/):
    {name}_raw.json      — raw chunks, no dedup, no filtering
"""

import argparse
import asyncio
import json
import time
from datetime import datetime, timedelta, timezone
from pathlib import Path
from urllib.parse import urlencode

import httpx

# ── Queries ───────────────────────────────────────────────────────────────────
# Two queries: direct visit signals + broader diplomatic/trade context

QUERIES = {
    "china_visit_direct": (
        "trump (china OR beijing OR xi) "
        "(visit OR trip OR travel OR summit OR meeting OR invite OR invitation)"
    ),
    "china_visit_diplomacy": (
        "trump (china OR beijing OR chinese) "
        "(trade OR tariff OR negotiations OR diplomacy OR relations "
        "OR talks OR agreement OR deal OR sanctions OR cooperation)"
    ),
}

# ── Time period ───────────────────────────────────────────────────────────────
START = datetime(2026, 2, 10, tzinfo=timezone.utc)
END = datetime(2026, 3, 18, tzinfo=timezone.utc)  # exclusive (covers through March 17)
WINDOW_HOURS = 2

# ── GDELT settings ────────────────────────────────────────────────────────────
MAX_RECORDS = 250
REQUEST_DELAY = 10  # generous base delay between requests
MAX_RETRIES = 8     # more retries with longer backoff
GDELT_URL = "https://api.gdeltproject.org/api/v2/doc/doc"


def make_gdelt_url(query: str, start: datetime, end: datetime) -> str:
    params = {
        "query": query,
        "mode": "artlist",
        "maxrecords": MAX_RECORDS,
        "format": "json",
        "startdatetime": start.strftime("%Y%m%d%H%M%S"),
        "enddatetime": end.strftime("%Y%m%d%H%M%S"),
        "sort": "datedesc",
    }
    return f"{GDELT_URL}?{urlencode(params)}"


def parse_gdelt_article(art: dict) -> dict:
    return {
        "timestamp": datetime.strptime(art["seendate"], "%Y%m%dT%H%M%SZ").strftime(
            "%Y%m%dT%H%M%SZ"
        ),
        "title": art.get("title", ""),
        "source": art.get("domain", ""),
        "url": art.get("url", ""),
        "language": art.get("language", ""),
        "sourcecountry": art.get("sourcecountry", ""),
    }


class Logger:
    def __init__(self, log_path: Path):
        self.log_file = open(log_path, "w")

    def log(self, msg: str = ""):
        print(msg, flush=True)
        self.log_file.write(msg + "\n")
        self.log_file.flush()

    def close(self):
        self.log_file.close()


async def fetch_window(
    client: httpx.AsyncClient,
    query: str,
    win_start: datetime,
    win_end: datetime,
) -> dict:
    url = make_gdelt_url(query, win_start, win_end)
    label = win_start.strftime("%Y-%m-%d %H:00")

    last_error = "unknown"
    for attempt in range(MAX_RETRIES):
        try:
            resp = await client.get(url)

            if resp.status_code == 429:
                last_error = "429 rate limited"
                await asyncio.sleep(15)
                continue

            resp.raise_for_status()
            text = resp.text.strip()

            # GDELT sometimes returns rate-limit message as 200 with text body
            if "limit requests" in text.lower():
                last_error = "rate limited (200 body)"
                await asyncio.sleep(15)
                continue

            if not text or text == "{}":
                return {"window": label, "count": 0, "capped": False, "articles": []}
            if not text.startswith('{"articles"'):
                return {"window": label, "count": 0, "capped": False, "articles": []}

            data = json.loads(text)
            articles = [parse_gdelt_article(a) for a in data.get("articles", [])]
            capped = len(articles) >= MAX_RECORDS
            return {
                "window": label,
                "count": len(articles),
                "capped": capped,
                "articles": articles,
            }
        except Exception as e:
            last_error = str(e)
            if attempt < MAX_RETRIES - 1:
                await asyncio.sleep(10 * (attempt + 1))
            else:
                return {
                    "window": label,
                    "count": 0,
                    "capped": False,
                    "error": str(e),
                    "articles": [],
                }

    return {
        "window": label,
        "count": 0,
        "capped": False,
        "error": last_error,
        "articles": [],
    }


def _save(raw_path: Path, query: str, chunks: list, total: int, capped: list, failed: list):
    raw_data = {
        "query": query,
        "period": f"{START.date()} to {END.date()}",
        "fetched_at": datetime.now(timezone.utc).isoformat(),
        "total_raw": total,
        "window_hours": WINDOW_HOURS,
        "capped_windows": capped,
        "failed_windows": failed,
        "chunks": chunks,
    }
    with open(raw_path, "w") as f:
        json.dump(raw_data, f)


async def fetch_query(query_name: str, query: str, start: datetime, end: datetime):
    out_dir = Path(__file__).parent / "datasets"
    out_dir.mkdir(exist_ok=True)
    raw_path = out_dir / f"{query_name}_raw.json"
    log_path = out_dir / f"{query_name}.log"
    log = Logger(log_path)

    windows = []
    t = start
    while t < end:
        windows.append(t)
        t += timedelta(hours=WINDOW_HOURS)

    days: dict[str, list[datetime]] = {}
    for w in windows:
        day_key = w.strftime("%Y-%m-%d")
        days.setdefault(day_key, []).append(w)

    total_windows = len(windows)
    log.log(f"Fetching {len(days)} days ({total_windows} windows, {WINDOW_HOURS}h each)")
    log.log(f"Query: {query[:80]}...")
    log.log(f"Period: {start.date()} to {(end - timedelta(days=1)).date()}")
    log.log(f"Delay: {REQUEST_DELAY}s between requests")
    log.log()

    all_chunks: list[dict] = []
    total_articles = 0
    capped_windows: list[str] = []
    failed_windows: list[str] = []
    t0 = time.time()

    timeout = httpx.Timeout(20.0, connect=10.0)
    async with httpx.AsyncClient(timeout=timeout) as client:
        day_keys = sorted(days.keys())
        for i, day_key in enumerate(day_keys):
            day_t0 = time.time()
            day_count = 0
            day_capped = []
            day_failed = []

            for win_start in days[day_key]:
                win_end = win_start + timedelta(hours=WINDOW_HOURS)
                chunk = await fetch_window(client, query, win_start, win_end)
                all_chunks.append(chunk)
                day_count += chunk["count"]
                if chunk.get("capped"):
                    day_capped.append(chunk["window"])
                if chunk.get("error"):
                    day_failed.append(chunk["window"])
                await asyncio.sleep(REQUEST_DELAY)

            total_articles += day_count
            capped_windows.extend(day_capped)
            failed_windows.extend(day_failed)

            elapsed = time.time() - day_t0
            eta_min = (len(day_keys) - i - 1) * elapsed / 60
            cap_warn = f"  !! CAPPED: {day_capped}" if day_capped else ""
            fail_warn = f"  !! FAILED: {day_failed}" if day_failed else ""
            log.log(
                f"  [{i+1}/{len(day_keys)}] {day_key}: "
                f"{day_count:>5} articles  ({elapsed:.0f}s, "
                f"ETA {eta_min:.0f}m, {total_articles:,} total)"
                f"{cap_warn}{fail_warn}"
            )

            _save(raw_path, query, all_chunks, total_articles, capped_windows, failed_windows)

    total_time = time.time() - t0
    log.log()
    log.log(f"Done in {total_time:.0f}s — {total_articles:,} raw articles")

    if capped_windows:
        log.log(f"WARNING: {len(capped_windows)} windows hit 250 cap: {capped_windows}")
    if failed_windows:
        log.log(f"WARNING: {len(failed_windows)} windows failed: {failed_windows}")

    _save(raw_path, query, all_chunks, total_articles, capped_windows, failed_windows)
    raw_mb = raw_path.stat().st_size / 1024 / 1024
    log.log(f"Saved raw: {raw_path.name} ({raw_mb:.1f} MB)")
    log.close()


async def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--test", action="store_true", help="Fetch only 1 day for testing")
    args = parser.parse_args()

    if args.test:
        # Test mode: fetch just 1 day (Feb 15) for both queries
        test_start = START
        test_end = START + timedelta(days=1)
        print(f"=== TEST MODE: fetching {test_start.date()} only ===\n")
        for name, query in QUERIES.items():
            print(f"\n--- {name} ---")
            await fetch_query(name, query, test_start, test_end)
    else:
        for name, query in QUERIES.items():
            print(f"\n--- {name} ---")
            await fetch_query(name, query, START, END)


if __name__ == "__main__":
    asyncio.run(main())
