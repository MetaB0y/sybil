"""SQLite decision and article logging for the live trading dashboard."""

import json
import sqlite3
import threading
from datetime import datetime, timezone


class DecisionDB:
    """Thread-safe SQLite wrapper for logging bot decisions and articles."""

    def __init__(self, db_path: str = "live/decisions.db"):
        self.conn = sqlite3.connect(db_path, check_same_thread=False)
        self.conn.row_factory = sqlite3.Row
        self._lock = threading.Lock()
        self._create_tables()

    def _create_tables(self):
        with self._lock:
            # Migrate: add columns that were added after some databases were created.
            # ALTER TABLE is a no-op if the table doesn't exist yet (CREATE TABLE handles it).
            for table, column, coltype in [
                ("decisions", "article_urls", "TEXT"),
                ("portfolio_snapshots", "total_fills", "INTEGER DEFAULT 0"),
                ("portfolio_snapshots", "total_orders", "INTEGER DEFAULT 0"),
            ]:
                try:
                    self.conn.execute(f"SELECT {column} FROM {table} LIMIT 0")
                except sqlite3.OperationalError:
                    try:
                        self.conn.execute(f"ALTER TABLE {table} ADD COLUMN {column} {coltype}")
                        self.conn.commit()
                    except sqlite3.OperationalError:
                        pass

            self.conn.executescript("""
                CREATE TABLE IF NOT EXISTS articles (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    url TEXT UNIQUE,
                    title TEXT,
                    source TEXT,
                    published_at TEXT,
                    fetched_at TEXT,
                    full_text TEXT,
                    matched_market_ids TEXT
                );

                CREATE TABLE IF NOT EXISTS decisions (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    trader_name TEXT,
                    market_id INTEGER,
                    market_name TEXT,
                    timestamp TEXT,
                    article_ids TEXT,
                    article_urls TEXT,
                    analysis TEXT,
                    fair_value REAL,
                    market_price REAL,
                    orders TEXT,
                    motivation TEXT,
                    raw_llm_response TEXT,
                    llm_duration_s REAL,
                    balance REAL,
                    yes_pos INTEGER,
                    no_pos INTEGER
                );

                CREATE TABLE IF NOT EXISTS portfolio_snapshots (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    trader_name TEXT,
                    timestamp TEXT,
                    balance REAL,
                    portfolio_value REAL,
                    pnl REAL,
                    positions TEXT,
                    total_fills INTEGER DEFAULT 0,
                    total_orders INTEGER DEFAULT 0
                );

                CREATE TABLE IF NOT EXISTS token_usage (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    trader_name TEXT,
                    timestamp TEXT,
                    prompt_tokens INTEGER,
                    completion_tokens INTEGER,
                    model TEXT,
                    duration_s REAL
                );

                CREATE INDEX IF NOT EXISTS idx_decisions_trader
                    ON decisions(trader_name);
                CREATE INDEX IF NOT EXISTS idx_decisions_time
                    ON decisions(timestamp);
                CREATE INDEX IF NOT EXISTS idx_snapshots_trader
                    ON portfolio_snapshots(trader_name);
                CREATE INDEX IF NOT EXISTS idx_snapshots_time
                    ON portfolio_snapshots(timestamp);
            """)

    def log_article(self, article) -> int | None:
        """Insert an article. Returns row id or None if duplicate."""
        with self._lock:
            try:
                cur = self.conn.execute(
                    """INSERT OR IGNORE INTO articles
                       (url, title, source, published_at, fetched_at, full_text, matched_market_ids)
                       VALUES (?, ?, ?, ?, ?, ?, ?)""",
                    (
                        article.url,
                        article.title,
                        article.source,
                        article.published.isoformat(),
                        datetime.now(timezone.utc).isoformat(),
                        article.full_text,
                        json.dumps(article.matched_market_ids),
                    ),
                )
                self.conn.commit()
                return cur.lastrowid if cur.rowcount > 0 else None
            except Exception:
                return None

    def log_decision(
        self,
        trader_name: str,
        market_id: int,
        market_name: str,
        analysis: str,
        fair_value: float,
        market_price: float,
        orders: list[dict],
        motivation: str,
        raw_llm_response: str,
        llm_duration_s: float,
        balance: float,
        yes_pos: int,
        no_pos: int,
        article_ids: list[int] | None = None,
        article_urls: list[dict] | None = None,
    ) -> int:
        with self._lock:
            cur = self.conn.execute(
                """INSERT INTO decisions
                   (trader_name, market_id, market_name, timestamp, article_ids,
                    article_urls, analysis, fair_value, market_price, orders,
                    motivation, raw_llm_response, llm_duration_s, balance,
                    yes_pos, no_pos)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (
                    trader_name,
                    market_id,
                    market_name,
                    datetime.now(timezone.utc).isoformat(),
                    json.dumps(article_ids or []),
                    json.dumps(article_urls or []),
                    analysis,
                    fair_value,
                    market_price,
                    json.dumps(orders),
                    motivation,
                    raw_llm_response,
                    llm_duration_s,
                    balance,
                    yes_pos,
                    no_pos,
                ),
            )
            self.conn.commit()
            return cur.lastrowid

    def log_snapshot(
        self,
        trader_name: str,
        balance: float,
        portfolio_value: float,
        pnl: float,
        positions: dict,
        total_fills: int = 0,
        total_orders: int = 0,
    ) -> int:
        with self._lock:
            cur = self.conn.execute(
                """INSERT INTO portfolio_snapshots
                   (trader_name, timestamp, balance, portfolio_value, pnl, positions, total_fills, total_orders)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?)""",
                (
                    trader_name,
                    datetime.now(timezone.utc).isoformat(),
                    balance,
                    portfolio_value,
                    pnl,
                    json.dumps(positions),
                    total_fills,
                    total_orders,
                ),
            )
            self.conn.commit()
            return cur.lastrowid

    def log_token_usage(
        self,
        trader_name: str,
        prompt_tokens: int,
        completion_tokens: int,
        model: str,
        duration_s: float,
    ):
        with self._lock:
            self.conn.execute(
                """INSERT INTO token_usage
                   (trader_name, timestamp, prompt_tokens, completion_tokens, model, duration_s)
                   VALUES (?, ?, ?, ?, ?, ?)""",
                (
                    trader_name,
                    datetime.now(timezone.utc).isoformat(),
                    prompt_tokens,
                    completion_tokens,
                    model,
                    duration_s,
                ),
            )
            self.conn.commit()

    def close(self):
        self.conn.close()
