"""Bot personas for the China visit market.

Phase 1 uses two source pools (us_pool, cn_pool).
Six trader personas (3 per pool) will be added after phase 1 completes.
"""

from .sources import US_SOURCES, CN_SOURCES

BOT_PERSONAS = {
    "us_pool": {
        "name": "US Pool",
        "description": "US/Western media source pool for phase 1 filtering",
        "model": None,
        "sources": US_SOURCES,
        "phase1_bot": "us_pool",
        "enabled": True,
        "persona": {
            "identity": "placeholder — phase 1 only",
            "read_style": [],
            "trade_style": [],
        },
    },
    "cn_pool": {
        "name": "CN Pool",
        "description": "Chinese/Asian media source pool for phase 1 filtering",
        "model": None,
        "sources": CN_SOURCES,
        "phase1_bot": "cn_pool",
        "enabled": True,
        "persona": {
            "identity": "placeholder — phase 1 only",
            "read_style": [],
            "trade_style": [],
        },
    },
}
