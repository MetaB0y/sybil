"""Arena-owned read boundary for decision and equity analytics.

The decision database is an implementation detail of the Python Arena process.
Other services consume these typed JSON responses over the private Compose
network instead of mounting the SQLite volume or depending on its schema.
"""

from __future__ import annotations

import json
import logging
import math
import secrets
import sqlite3
import threading
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Any
from urllib.parse import parse_qs, urlparse

log = logging.getLogger(__name__)

DEFAULT_DECISION_LIMIT = 50
MAX_DECISION_LIMIT = 500
DEFAULT_EQUITY_LIMIT = 200
MAX_EQUITY_LIMIT = 1_000


def _clean(value: str | None) -> str | None:
    if value is None:
        return None
    value = value.strip()
    return value or None


def _bounded_int(value: str | None, default: int, maximum: int) -> int:
    try:
        parsed = int(value) if value is not None else default
    except ValueError:
        parsed = default
    return max(1, min(parsed, maximum))


def _open_read_only(db_path: str) -> sqlite3.Connection:
    uri = Path(db_path).resolve().as_uri() + "?mode=ro"
    conn = sqlite3.connect(uri, uri=True, timeout=0.75)
    conn.row_factory = sqlite3.Row
    return conn


def _table_exists(conn: sqlite3.Connection, table: str) -> bool:
    row = conn.execute(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?)",
        (table,),
    ).fetchone()
    return bool(row and row[0])


def _columns(conn: sqlite3.Connection, table: str) -> set[str]:
    if not _table_exists(conn, table):
        return set()
    return {str(row[1]) for row in conn.execute(f"PRAGMA table_info({table})").fetchall()}


def _count_rows(conn: sqlite3.Connection, table: str) -> int:
    if not _table_exists(conn, table):
        return 0
    return int(conn.execute(f"SELECT COUNT(*) FROM {table}").fetchone()[0])


def _edge(fair_value: float | None, market_price: float | None) -> float | None:
    if fair_value is None or market_price is None:
        return None
    return abs(fair_value - market_price)


def _json_column(value: str | None) -> Any:
    if value is None:
        return []
    try:
        return json.loads(value)
    except (TypeError, ValueError):
        return []


def _summary(trader_name: str) -> dict[str, Any]:
    return {
        "trader_name": trader_name,
        "account_id": None,
        "active": False,
        "role": None,
        "scored": False,
        "decision_count": 0,
        "avg_edge": None,
        "latest_timestamp": None,
        "latest_market_id": None,
        "latest_market_name": None,
        "latest_fair_value": None,
        "latest_market_price": None,
        "latest_edge": None,
        "latest_balance": None,
        "portfolio_value": None,
        "pnl": None,
        "total_fills": None,
        "total_orders": None,
        "snapshot_timestamp": None,
    }


def _apply_latest_decisions(
    conn: sqlite3.Connection,
    summaries: dict[str, dict[str, Any]],
    run_id: str | None,
) -> None:
    where = "WHERE run_id = ?" if run_id is not None else ""
    params: tuple[Any, ...] = (run_id,) if run_id is not None else ()
    for row in conn.execute(
        "SELECT trader_name, COUNT(*) AS decision_count, "
        "AVG(ABS(fair_value - market_price)) AS avg_edge "
        f"FROM decisions {where} GROUP BY trader_name",
        params,
    ):
        current = summaries.setdefault(str(row[0]), _summary(str(row[0])))
        current["decision_count"] = int(row[1])
        current["avg_edge"] = row[2]

    for row in conn.execute(
        "SELECT d.trader_name, d.market_id, d.market_name, d.timestamp, "
        "d.fair_value, d.market_price, d.balance FROM decisions d "
        "JOIN (SELECT trader_name, MAX(id) AS id FROM decisions "
        f"{where} GROUP BY trader_name) latest "
        "ON d.trader_name = latest.trader_name AND d.id = latest.id",
        params,
    ):
        current = summaries.setdefault(str(row[0]), _summary(str(row[0])))
        current.update(
            latest_market_id=row[1],
            latest_market_name=row[2],
            latest_timestamp=row[3],
            latest_fair_value=row[4],
            latest_market_price=row[5],
            latest_balance=row[6],
            latest_edge=_edge(row[4], row[5]),
        )


def _apply_latest_snapshots(
    conn: sqlite3.Connection,
    summaries: dict[str, dict[str, Any]],
    run_id: str | None,
) -> None:
    columns = _columns(conn, "portfolio_snapshots")
    if not columns:
        return
    optional = {
        name: name if name in columns else f"NULL AS {name}"
        for name in ("account_id", "total_fills", "total_orders")
    }
    if run_id is not None and "run_id" not in columns:
        return
    where = "WHERE run_id = ?" if run_id is not None else ""
    params: tuple[Any, ...] = (run_id,) if run_id is not None else ()
    sql = (
        "SELECT p.trader_name, "
        f"p.{optional['account_id']}, p.balance, p.portfolio_value, p.pnl, "
        f"p.{optional['total_fills']}, p.{optional['total_orders']}, p.timestamp "
        "FROM portfolio_snapshots p JOIN ("
        "SELECT trader_name, MAX(id) AS id FROM portfolio_snapshots "
        f"{where} GROUP BY trader_name) latest "
        "ON p.trader_name = latest.trader_name AND p.id = latest.id"
    )
    # The expressions above need no `p.` prefix when they are synthetic NULLs.
    sql = sql.replace("p.NULL AS ", "NULL AS ")
    for row in conn.execute(sql, params):
        current = summaries.setdefault(str(row[0]), _summary(str(row[0])))
        if current["latest_balance"] is None:
            current["latest_balance"] = row[2]
        current.update(
            account_id=row[1],
            portfolio_value=row[3],
            pnl=row[4],
            total_fills=row[5],
            total_orders=row[6],
            snapshot_timestamp=row[7],
        )


def _load_summaries(conn: sqlite3.Connection) -> list[dict[str, Any]]:
    summaries: dict[str, dict[str, Any]] = {}
    _apply_latest_decisions(conn, summaries, None)
    _apply_latest_snapshots(conn, summaries, None)

    active_run_id: str | None = None
    if _table_exists(conn, "arena_runs") and _table_exists(conn, "arena_run_participants"):
        participants = conn.execute(
            "SELECT p.run_id, p.trader_name, p.role, p.scored "
            "FROM arena_run_participants p JOIN arena_runs r ON r.run_id = p.run_id "
            "WHERE r.run_id = (SELECT run_id FROM arena_runs "
            "WHERE stopped_at_utc IS NULL "
            "AND julianday(heartbeat_at_utc) >= julianday('now', '-15 minutes') "
            "ORDER BY started_at_utc DESC LIMIT 1)"
        ).fetchall()
        for row in participants:
            active_run_id = str(row[0])
            current = summaries.setdefault(str(row[1]), _summary(str(row[1])))
            current.update(active=True, role=str(row[2]), scored=bool(row[3]) and row[2] == "competitor")

    if active_run_id is not None:
        for current in summaries.values():
            if current["active"]:
                preserved = {
                    "trader_name": current["trader_name"],
                    "account_id": current["account_id"],
                    "active": True,
                    "role": current["role"],
                    "scored": current["scored"],
                }
                current.clear()
                current.update(_summary(preserved["trader_name"]), **preserved)
        _apply_latest_decisions(conn, summaries, active_run_id)
        _apply_latest_snapshots(conn, summaries, active_run_id)

    rows = list(summaries.values())
    rows.sort(key=lambda row: row["trader_name"])
    rows.sort(key=lambda row: row["decision_count"], reverse=True)
    rows.sort(key=lambda row: row["latest_timestamp"] or "", reverse=True)
    rows.sort(key=lambda row: row["scored"], reverse=True)
    rows.sort(key=lambda row: row["active"], reverse=True)
    return rows


def _decision_filters(
    trader: str | None, market_id: int | None, since: str | None
) -> tuple[str, list[Any]]:
    clauses: list[str] = []
    params: list[Any] = []
    for clause, value in (
        ("trader_name = ?", trader),
        ("market_id = ?", market_id),
        ("timestamp >= ?", since),
    ):
        if value is not None:
            clauses.append(clause)
            params.append(value)
    return ("WHERE " + " AND ".join(clauses) if clauses else "", params)


def load_decision_feed(
    db_path: str,
    *,
    limit: int = DEFAULT_DECISION_LIMIT,
    trader: str | None = None,
    market_id: int | None = None,
    since: str | None = None,
) -> dict[str, Any]:
    """Return the stable bot-decision API document."""
    limit = max(1, min(limit, MAX_DECISION_LIMIT))
    if not db_path.strip():
        return _unavailable_decisions(None, "Arena decision database is not configured")
    if not Path(db_path).exists():
        return _unavailable_decisions(db_path, "arena decision database not found")
    try:
        with _open_read_only(db_path) as conn:
            if not _table_exists(conn, "decisions"):
                return _unavailable_decisions(db_path, "decisions table is missing")
            summaries = _load_summaries(conn)
            where, params = _decision_filters(_clean(trader), market_id, _clean(since))
            rows = conn.execute(
                "SELECT id, trader_name, market_id, market_name, timestamp, analysis, "
                "fair_value, market_price, orders, motivation, llm_duration_s, balance, "
                f"yes_pos, no_pos, article_urls FROM decisions {where} "
                "ORDER BY id DESC LIMIT ?",
                (*params, limit),
            ).fetchall()
            decisions = [
                {
                    "id": row[0],
                    "trader_name": row[1],
                    "market_id": row[2],
                    "market_name": row[3],
                    "timestamp": row[4],
                    "analysis": row[5],
                    "motivation": row[9],
                    "fair_value": row[6],
                    "market_price": row[7],
                    "edge": _edge(row[6], row[7]),
                    "orders": _json_column(row[8]),
                    "article_urls": _json_column(row[14]),
                    "llm_duration_s": row[10],
                    "balance": row[11],
                    "yes_pos": row[12],
                    "no_pos": row[13],
                }
                for row in rows
            ]
            token_usage = []
            if _table_exists(conn, "token_usage"):
                token_usage = [
                    {
                        "trader_name": row[0],
                        "calls": row[1],
                        "prompt_tokens": row[2],
                        "completion_tokens": row[3],
                        "avg_latency_s": row[4],
                        "latest_model": row[5],
                    }
                    for row in conn.execute(
                        "SELECT trader_name, COUNT(*), COALESCE(SUM(prompt_tokens), 0), "
                        "COALESCE(SUM(completion_tokens), 0), AVG(duration_s), MAX(model) "
                        "FROM token_usage GROUP BY trader_name ORDER BY COUNT(*) DESC"
                    )
                ]
            latest = conn.execute(
                "SELECT timestamp FROM decisions ORDER BY id DESC LIMIT 1"
            ).fetchone()
            return {
                "db_available": True,
                "db_path": db_path,
                "error": None,
                "stats": {
                    "decisions": _count_rows(conn, "decisions"),
                    "articles": _count_rows(conn, "articles"),
                    "snapshots": _count_rows(conn, "portfolio_snapshots"),
                    "token_usage": _count_rows(conn, "token_usage"),
                    "traders": sum(1 for summary in summaries if summary["active"]),
                    "latest_decision_timestamp": latest[0] if latest else None,
                },
                "summaries": summaries,
                "decisions": decisions,
                "token_usage": token_usage,
            }
    except sqlite3.Error as error:
        return _unavailable_decisions(db_path, f"failed to query arena decisions: {error}")


def _unavailable_decisions(db_path: str | None, error: str) -> dict[str, Any]:
    return {
        "db_available": False,
        "db_path": db_path,
        "error": error,
        "stats": {
            "decisions": 0,
            "articles": 0,
            "snapshots": 0,
            "token_usage": 0,
            "traders": 0,
            "latest_decision_timestamp": None,
        },
        "summaries": [],
        "decisions": [],
        "token_usage": [],
    }


def load_equity_series(
    db_path: str,
    *,
    limit: int = DEFAULT_EQUITY_LIMIT,
    trader: str | None = None,
    since: str | None = None,
) -> dict[str, Any]:
    """Return the stable, bounded bot-equity API document."""
    limit = max(1, min(limit, MAX_EQUITY_LIMIT))
    trader = _clean(trader)
    since = _clean(since)
    if not db_path.strip():
        return _unavailable_equity(None, trader, since, limit, "Arena database is not configured")
    if not Path(db_path).exists():
        return _unavailable_equity(db_path, trader, since, limit, "arena decision database not found")
    try:
        with _open_read_only(db_path) as conn:
            columns = _columns(conn, "portfolio_snapshots")
            if not columns:
                return _unavailable_equity(
                    db_path, trader, since, limit, "portfolio_snapshots table is missing"
                )
            where, params = _decision_filters(trader, None, since)
            source_rows = int(
                conn.execute(
                    f"SELECT COUNT(*) FROM portfolio_snapshots {where}", params
                ).fetchone()[0]
            )
            stride = math.ceil(source_rows / limit) if source_rows > limit else 1
            totals = (
                "total_fills, total_orders"
                if {"total_fills", "total_orders"}.issubset(columns)
                else "NULL AS total_fills, NULL AS total_orders"
            )
            rows = conn.execute(
                "SELECT id, trader_name, timestamp, balance, portfolio_value, pnl, "
                "total_fills, total_orders FROM ("
                "SELECT id, trader_name, timestamp, balance, portfolio_value, pnl, "
                f"{totals}, ROW_NUMBER() OVER (ORDER BY id ASC) AS rn, "
                f"COUNT(*) OVER () AS total_rows FROM portfolio_snapshots {where}) sampled "
                "WHERE ? <= 1 OR ((rn - 1) % ?) = 0 OR rn = total_rows "
                "ORDER BY id ASC LIMIT ?",
                (*params, stride, stride, limit + 1),
            ).fetchall()
            if len(rows) > limit:
                rows = [*rows[: limit - 1], rows[-1]]
            points = [
                {
                    "id": row[0],
                    "trader_name": row[1],
                    "timestamp": row[2],
                    "balance": row[3],
                    "portfolio_value": row[4],
                    "pnl": row[5],
                    "total_fills": row[6],
                    "total_orders": row[7],
                }
                for row in rows
            ]
            return {
                "db_available": True,
                "db_path": db_path,
                "error": None,
                "trader": trader,
                "since": since,
                "limit": limit,
                "server_cap": MAX_EQUITY_LIMIT,
                "source_rows": source_rows,
                "returned_rows": len(points),
                "downsampled": source_rows > len(points),
                "stride": stride,
                "points": points,
            }
    except sqlite3.Error as error:
        return _unavailable_equity(
            db_path, trader, since, limit, f"failed to query arena equity: {error}"
        )


def _unavailable_equity(
    db_path: str | None,
    trader: str | None,
    since: str | None,
    limit: int,
    error: str,
) -> dict[str, Any]:
    return {
        "db_available": False,
        "db_path": db_path,
        "error": error,
        "trader": trader,
        "since": since,
        "limit": limit,
        "server_cap": MAX_EQUITY_LIMIT,
        "source_rows": 0,
        "returned_rows": 0,
        "downsampled": False,
        "stride": 1,
        "points": [],
    }


class _ArenaReadHandler(BaseHTTPRequestHandler):
    server: "ArenaReadHttpServer"

    def do_GET(self) -> None:  # noqa: N802 -- BaseHTTPRequestHandler API
        parsed = urlparse(self.path)
        if parsed.path == "/healthz":
            self._json(HTTPStatus.OK, {"status": "ok"})
            return
        expected = self.server.auth_token
        supplied = self.headers.get("Authorization", "").removeprefix("Bearer ")
        if not expected or not secrets.compare_digest(supplied, expected):
            self._json(HTTPStatus.UNAUTHORIZED, {"error": "unauthorized"})
            return
        params = parse_qs(parsed.query, keep_blank_values=True)

        def one(name: str) -> str | None:
            return params.get(name, [None])[0]

        if parsed.path == "/v1/decisions":
            raw_market_id = one("market_id")
            try:
                market_id = int(raw_market_id) if raw_market_id is not None else None
            except ValueError:
                market_id = None
            body = load_decision_feed(
                self.server.db_path,
                limit=_bounded_int(one("limit"), DEFAULT_DECISION_LIMIT, MAX_DECISION_LIMIT),
                trader=one("trader"),
                market_id=market_id,
                since=one("since"),
            )
            self._json(HTTPStatus.OK, body)
        elif parsed.path == "/v1/equity-series":
            body = load_equity_series(
                self.server.db_path,
                limit=_bounded_int(one("limit"), DEFAULT_EQUITY_LIMIT, MAX_EQUITY_LIMIT),
                trader=one("trader"),
                since=one("since"),
            )
            self._json(HTTPStatus.OK, body)
        else:
            self._json(HTTPStatus.NOT_FOUND, {"error": "not found"})

    def _json(self, status: HTTPStatus, body: dict[str, Any]) -> None:
        payload = json.dumps(body, allow_nan=False, separators=(",", ":")).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, format: str, *args: Any) -> None:
        log.debug("Arena read API: " + format, *args)


class ArenaReadHttpServer(ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, address: tuple[str, int], db_path: str, auth_token: str):
        self.db_path = db_path
        self.auth_token = auth_token
        super().__init__(address, _ArenaReadHandler)


def start_read_server(db_path: str, host: str, port: int, auth_token: str):
    """Start the private read API in a daemon thread, or disable it with port <= 0."""
    if port <= 0:
        return None
    if not auth_token:
        raise ValueError("Arena read API requires a nonempty auth token")
    server = ArenaReadHttpServer((host, port), db_path, auth_token)
    thread = threading.Thread(target=server.serve_forever, name="arena-read-api", daemon=True)
    thread.start()
    return server, thread
