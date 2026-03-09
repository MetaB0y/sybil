# Moved to sim/news_trader.py
from sim.news_trader import (  # noqa: F401
    Article,
    NewsTrader,
    NewsTrader as IranNewsTrader,  # backward compat alias
    PriceSnapshot,
    TradeRecord,
    load_articles,
    _describe_order,
    _format_fills,
    _order_side,
)
