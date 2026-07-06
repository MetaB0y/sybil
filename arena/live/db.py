"""SQLite decision and article logging for the live trading dashboard."""

import json
import sqlite3
import threading
from datetime import datetime, timezone

from .sqlite_utils import connect_writer


class DecisionDB:
    """Thread-safe SQLite wrapper for logging bot decisions and articles."""

    def __init__(self, db_path: str = "live/decisions.db"):
        self.conn = connect_writer(db_path, check_same_thread=False)
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
                # SYB-64: per-call USD cost + its source (provider vs price table).
                ("token_usage", "usd_cost", "REAL DEFAULT 0"),
                ("token_usage", "cost_source", "TEXT"),
                # SYB-114: sizer-side calibration/freshness metadata.
                ("decisions", "raw_fair_value", "REAL"),
                ("decisions", "effective_fair_value", "REAL"),
                ("decisions", "fair_value_age_s", "REAL"),
                ("decisions", "confidence", "REAL"),
                ("decisions", "countercase", "TEXT"),
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
                    no_pos INTEGER,
                    raw_fair_value REAL,
                    effective_fair_value REAL,
                    fair_value_age_s REAL,
                    confidence REAL,
                    countercase TEXT
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

                CREATE TABLE IF NOT EXISTS bot_accounts (
                    persona TEXT NOT NULL,
                    strategy TEXT NOT NULL,
                    account_id INTEGER NOT NULL,
                    created_at TEXT,
                    PRIMARY KEY (persona, strategy)
                );

                CREATE TABLE IF NOT EXISTS token_usage (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    trader_name TEXT,
                    timestamp TEXT,
                    prompt_tokens INTEGER,
                    completion_tokens INTEGER,
                    model TEXT,
                    duration_s REAL,
                    usd_cost REAL DEFAULT 0,
                    cost_source TEXT
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
        raw_fair_value: float | None = None,
        effective_fair_value: float | None = None,
        fair_value_age_s: float | None = None,
        confidence: float | None = None,
        countercase: str = "",
    ) -> int:
        with self._lock:
            cur = self.conn.execute(
                """INSERT INTO decisions
                   (trader_name, market_id, market_name, timestamp, article_ids,
                    article_urls, analysis, fair_value, market_price, orders,
                    motivation, raw_llm_response, llm_duration_s, balance,
                    yes_pos, no_pos, raw_fair_value, effective_fair_value,
                    fair_value_age_s, confidence, countercase)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
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
                    raw_fair_value,
                    effective_fair_value,
                    fair_value_age_s,
                    confidence,
                    countercase,
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
        usd_cost: float = 0.0,
        cost_source: str = "",
    ):
        with self._lock:
            self.conn.execute(
                """INSERT INTO token_usage
                   (trader_name, timestamp, prompt_tokens, completion_tokens,
                    model, duration_s, usd_cost, cost_source)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?)""",
                (
                    trader_name,
                    datetime.now(timezone.utc).isoformat(),
                    prompt_tokens,
                    completion_tokens,
                    model,
                    duration_s,
                    usd_cost,
                    cost_source,
                ),
            )
            self.conn.commit()

    def get_total_llm_cost(self, trader_name: str) -> float:
        """Sum of persisted USD LLM cost for a trader (SYB-64).

        Lets a restarting analyst reconstruct its cumulative spend — and thus
        its remaining budget — from the persisted token_usage rows, so the
        pause-at-zero accounting survives an arena restart.
        """
        with self._lock:
            row = self.conn.execute(
                "SELECT COALESCE(SUM(usd_cost), 0) AS total FROM token_usage "
                "WHERE trader_name = ?",
                (trader_name,),
            ).fetchone()
        return float(row["total"]) if row is not None else 0.0

    def get_bot_account_id(self, persona: str, strategy: str) -> int | None:
        """Return the persisted account id for a (persona, strategy) bot, if any.

        Lets the runner reattach a restarting bot to its existing portfolio
        instead of minting a fresh account and abandoning the old one (AR-3).
        """
        with self._lock:
            row = self.conn.execute(
                "SELECT account_id FROM bot_accounts WHERE persona = ? AND strategy = ?",
                (persona, strategy),
            ).fetchone()
        return int(row["account_id"]) if row is not None else None

    def save_bot_account_id(self, persona: str, strategy: str, account_id: int) -> None:
        """Persist the account id owning a (persona, strategy) bot's portfolio."""
        with self._lock:
            self.conn.execute(
                """INSERT INTO bot_accounts (persona, strategy, account_id, created_at)
                   VALUES (?, ?, ?, ?)
                   ON CONFLICT(persona, strategy)
                   DO UPDATE SET account_id = excluded.account_id,
                                 created_at = excluded.created_at""",
                (
                    persona,
                    strategy,
                    account_id,
                    datetime.now(timezone.utc).isoformat(),
                ),
            )
            self.conn.commit()

    def close(self):
        self.conn.close()
