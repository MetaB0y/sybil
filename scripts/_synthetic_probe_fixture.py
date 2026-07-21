#!/usr/bin/env python3
"""Local HTTP fixture for the scheduled synthetic probe's public web path."""

from __future__ import annotations

import argparse
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlsplit


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--mode",
        choices=("ok", "app-http", "app-shell", "app-asset"),
        required=True,
    )
    parser.add_argument("--port-file", type=Path, required=True)
    args = parser.parse_args()

    class Handler(BaseHTTPRequestHandler):
        block_height = 40

        def log_message(self, _format: str, *_args: object) -> None:
            return

        def send_body(
            self,
            status: int,
            body: bytes,
            content_type: str,
            headers: dict[str, str] | None = None,
        ) -> None:
            self.send_response(status)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(body)))
            for name, value in (headers or {}).items():
                self.send_header(name, value)
            self.end_headers()
            self.wfile.write(body)

        def send_json(self, value: object, status: int = 200) -> None:
            self.send_body(
                status,
                json.dumps(value, separators=(",", ":")).encode(),
                "application/json",
            )

        def do_GET(self) -> None:  # noqa: N802 - stdlib handler contract
            parsed = urlsplit(self.path)
            if parsed.path == "/api/v1/health":
                self.send_json(
                    {
                        "status": "ok",
                        "height": self.block_height,
                        "genesis_hash": "ab" * 32,
                    }
                )
            elif parsed.path == "/api/v1/blocks/latest":
                type(self).block_height += 1
                self.send_json({"height": self.block_height})
            elif parsed.path == "/api/v1/markets":
                self.send_json([{"market_id": 1}])
            elif parsed.path == "/api/v1/markets/search":
                catalog = parse_qs(parsed.query).get("tags", [""])[0]
                market: dict[str, object] = {"liquidity_avg10_nanos": 1}
                if catalog == "polymarket":
                    market.update(
                        reference_price_nanos=500_000_000,
                        reference_price_expires_at_ms=9_999_999_999_999,
                    )
                self.send_json([market])
            elif parsed.path == "/app/":
                if args.mode == "app-http":
                    self.send_body(503, b"unavailable", "text/plain")
                elif args.mode == "app-shell":
                    self.send_body(200, b"<html><title>Other</title></html>", "text/html")
                else:
                    self.send_body(
                        200,
                        (
                            b"<!doctype html><html><head><title>Sybil</title></head>"
                            b'<body><script src="/app/_next/static/chunk.js"></script>'
                            b"</body></html>"
                        ),
                        "text/html",
                    )
            elif parsed.path == "/app/_next/static/chunk.js":
                if args.mode == "app-asset":
                    self.send_body(503, b"unavailable", "text/plain")
                else:
                    self.send_body(200, b"self.__sybil_fixture=true;", "application/javascript")
            else:
                self.send_json({"error": "not found"}, 404)

        def do_OPTIONS(self) -> None:  # noqa: N802 - stdlib handler contract
            if urlsplit(self.path).path != "/api/v1/onboarding/accounts":
                self.send_json({"error": "not found"}, 404)
                return
            self.send_body(
                204,
                b"",
                "text/plain",
                {
                    "Access-Control-Allow-Origin": self.headers.get("Origin", ""),
                    "Access-Control-Allow-Methods": "POST, OPTIONS",
                    "Access-Control-Allow-Headers": "content-type",
                },
            )

        def do_POST(self) -> None:  # noqa: N802 - stdlib handler contract
            if urlsplit(self.path).path == "/vm/api/v1/import/prometheus":
                length = int(self.headers.get("Content-Length", "0"))
                self.rfile.read(length)
                self.send_body(204, b"", "text/plain")
            else:
                self.send_json({"error": "not found"}, 404)

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    args.port_file.write_text(str(server.server_port), encoding="utf-8")
    server.serve_forever()


if __name__ == "__main__":
    main()
