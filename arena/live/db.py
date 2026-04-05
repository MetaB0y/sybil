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
                    positions TEXT
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
    ) -> int:
        with self._lock:
            cur = self.conn.execute(
                """INSERT INTO decisions
                   (trader_name, market_id, market_name, timestamp, article_ids,
                    analysis, fair_value, market_price, orders, motivation,
                    raw_llm_response, llm_duration_s, balance, yes_pos, no_pos)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (
                    trader_name,
                    market_id,
                    market_name,
                    datetime.now(timezone.utc).isoformat(),
                    json.dumps(article_ids or []),
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
    ) -> int:
        with self._lock:
            cur = self.conn.execute(
                """INSERT INTO portfolio_snapshots
                   (trader_name, timestamp, balance, portfolio_value, pnl, positions)
                   VALUES (?, ?, ?, ?, ?, ?)""",
                (
                    trader_name,
                    datetime.now(timezone.utc).isoformat(),
                    balance,
                    portfolio_value,
                    pnl,
                    json.dumps(positions),
                ),
            )
            self.conn.commit()
            return cur.lastrowid

    def close(self):
        self.conn.close()
