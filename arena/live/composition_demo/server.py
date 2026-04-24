"""Local HTTP gateway for the agentic composition demo.

Run:
    cd arena && uv run python -m live.composition_demo.server
"""

from __future__ import annotations

import argparse
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any
from urllib.parse import parse_qs, urlparse

from . import agent
from .store import (
    DEFAULT_SYBIL_URL,
    add_instrument,
    enrich_state,
    explorer_search,
    import_sources,
    load_state,
    quote_once,
    seed_markets,
    submit_order,
    trigger_event,
    validate_formula_payload,
)


class Handler(BaseHTTPRequestHandler):
    sybil_url = DEFAULT_SYBIL_URL

    def do_OPTIONS(self) -> None:
        self.respond({}, status=204)

    def do_GET(self) -> None:
        parsed = urlparse(self.path)
        qs = parse_qs(parsed.query)
        sybil_url = qs.get("sybil_url", [self.sybil_url])[0]
        if parsed.path == "/health":
            self.respond({"status": "ok", "sybil_url": sybil_url})
            return
        if parsed.path == "/state":
            self.respond(enrich_state(load_state(), sybil_url))
            return
        self.respond({"error": "not found"}, status=404)

    def do_POST(self) -> None:
        parsed = urlparse(self.path)
        body = self.read_json()
        sybil_url = body.get("sybil_url") or self.sybil_url
        try:
            if parsed.path == "/seed":
                self.respond(seed_markets(sybil_url))
            elif parsed.path == "/sources/import":
                self.respond(import_sources(force=bool(body.get("force")), max_atoms=int(body.get("max_atoms", 300))))
            elif parsed.path == "/explorer/search":
                self.respond(explorer_search(body, sybil_url))
            elif parsed.path == "/formula/validate":
                self.respond(validate_formula_payload(body))
            elif parsed.path == "/quote":
                self.respond(quote_once(sybil_url))
            elif parsed.path == "/event":
                self.respond(trigger_event(body.get("event", "helicopter"), sybil_url))
            elif parsed.path == "/agent/discover":
                self.respond(agent.discover(body.get("query", ""), enrich_state(load_state(), sybil_url)))
            elif parsed.path == "/agent/draft-composition":
                self.respond(agent.draft_composition(body.get("prompt", ""), enrich_state(load_state(), sybil_url)))
            elif parsed.path == "/agent/build-from-conditions":
                self.respond(agent.draft_composition(body.get("prompt", ""), enrich_state(load_state(), sybil_url)))
            elif parsed.path == "/agent/explain-instrument":
                self.respond(agent.explain_instrument(body.get("instrument_id", ""), enrich_state(load_state(), sybil_url)))
            elif parsed.path == "/agent/propose-trade":
                self.respond(agent.propose_trade(body, enrich_state(load_state(), sybil_url)))
            elif parsed.path == "/markets/create-draft":
                self.respond(add_instrument(body["draft"], sybil_url))
            elif parsed.path == "/orders/submit":
                self.respond(
                    submit_order(
                        sybil_url=sybil_url,
                        account_id=int(body["account_id"]),
                        market_id=int(body["market_id"]),
                        side=body["side"],
                        price=float(body["price"]),
                        quantity=int(body["quantity"]),
                    )
                )
            else:
                self.respond({"error": "not found"}, status=404)
        except Exception as e:
            self.respond({"error": str(e)}, status=500)

    def read_json(self) -> dict[str, Any]:
        length = int(self.headers.get("Content-Length", "0"))
        if length == 0:
            return {}
        return json.loads(self.rfile.read(length).decode("utf-8"))

    def respond(self, payload: Any, status: int = 200) -> None:
        body = b"" if status == 204 else json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET,POST,OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if body:
            self.wfile.write(body)

    def log_message(self, fmt: str, *args: Any) -> None:
        print(f"{self.address_string()} - {fmt % args}")


def main() -> None:
    parser = argparse.ArgumentParser(description="Composition demo agent gateway")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8787)
    parser.add_argument("--sybil-url", default=DEFAULT_SYBIL_URL)
    args = parser.parse_args()
    Handler.sybil_url = args.sybil_url
    server = ThreadingHTTPServer((args.host, args.port), Handler)
    print(f"composition demo gateway listening on http://{args.host}:{args.port}")
    print(f"sybil-api: {args.sybil_url}")
    server.serve_forever()


if __name__ == "__main__":
    main()
