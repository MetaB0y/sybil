"""SQLite connection helpers for live arena state."""

from __future__ import annotations

import sqlite3
from pathlib import Path
from urllib.parse import quote

BUSY_TIMEOUT_MS = 30_000


def connect_writer(db_path: str, *, check_same_thread: bool = False) -> sqlite3.Connection:
    """Open the arena writer connection with WAL enabled.

    WAL keeps dashboard/status readers from blocking the live runner's writes
    under normal local-volume deployment.
    """
    conn = sqlite3.connect(
        db_path,
        check_same_thread=check_same_thread,
        timeout=BUSY_TIMEOUT_MS / 1000,
    )
    conn.row_factory = sqlite3.Row
    conn.execute(f"PRAGMA busy_timeout={BUSY_TIMEOUT_MS}")
    conn.execute("PRAGMA journal_mode=WAL")
    conn.execute("PRAGMA synchronous=NORMAL")
    return conn


def connect_reader(db_path: str, *, check_same_thread: bool = True) -> sqlite3.Connection:
    """Open a read-only arena connection with a generous busy timeout."""
    path = quote(str(Path(db_path).resolve()), safe="/")
    conn = sqlite3.connect(
        f"file:{path}?mode=ro",
        uri=True,
        check_same_thread=check_same_thread,
        timeout=BUSY_TIMEOUT_MS / 1000,
    )
    conn.row_factory = sqlite3.Row
    conn.execute(f"PRAGMA busy_timeout={BUSY_TIMEOUT_MS}")
    conn.execute("PRAGMA query_only=ON")
    return conn
