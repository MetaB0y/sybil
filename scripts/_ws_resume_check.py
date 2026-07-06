#!/usr/bin/env python3
"""Minimal stdlib WebSocket client for the post-deploy smoke test (SYB-223).

Connects to `/v1/blocks/ws?from_block=N` and asserts the first-party replay
contract: at least one *replayed* block frame (height <= head-at-connect) is
delivered, followed by at least one *live* block frame (height > head).

No third-party deps: implements the RFC 6455 client handshake and frame
parsing over a raw (optionally TLS) socket. `websocat` is preferred by the
shell script when present; this is the stdlib fallback.

Usage:  _ws_resume_check.py <ws_url> <head_height> [timeout_secs]
Exit:   0 on success, 1 on failure. Diagnostics go to stderr; a final
        `ws_resume=pass|fail` line goes to stdout.
"""

import base64
import json
import os
import socket
import ssl
import struct
import sys
import time
from urllib.parse import urlparse


def log(msg):
    print(f"    ws: {msg}", file=sys.stderr)


class Frames:
    """Buffered reader that yields WebSocket frames from a socket."""

    def __init__(self, sock, initial=b""):
        self.sock = sock
        self.buf = bytearray(initial)

    def _recvn(self, n, deadline):
        while len(self.buf) < n:
            remaining = deadline - time.time()
            if remaining <= 0:
                raise TimeoutError()
            self.sock.settimeout(remaining)
            chunk = self.sock.recv(4096)
            if not chunk:
                raise EOFError()
            self.buf += chunk
        out = bytes(self.buf[:n])
        del self.buf[:n]
        return out

    def next(self, deadline):
        header = self._recvn(2, deadline)
        opcode = header[0] & 0x0F
        masked = header[1] & 0x80
        length = header[1] & 0x7F
        if length == 126:
            length = struct.unpack(">H", self._recvn(2, deadline))[0]
        elif length == 127:
            length = struct.unpack(">Q", self._recvn(8, deadline))[0]
        mask = self._recvn(4, deadline) if masked else b""
        payload = self._recvn(length, deadline) if length else b""
        if masked:
            payload = bytes(b ^ mask[i % 4] for i, b in enumerate(payload))
        return opcode, payload


def connect(url, timeout):
    u = urlparse(url)
    secure = u.scheme == "wss"
    host = u.hostname
    port = u.port or (443 if secure else 80)
    path = u.path + (("?" + u.query) if u.query else "")

    sock = socket.create_connection((host, port), timeout=timeout)
    if secure:
        ctx = ssl.create_default_context()
        sock = ctx.wrap_socket(sock, server_hostname=host)

    key = base64.b64encode(os.urandom(16)).decode()
    request = (
        f"GET {path} HTTP/1.1\r\n"
        f"Host: {host}:{port}\r\n"
        "Upgrade: websocket\r\n"
        "Connection: Upgrade\r\n"
        f"Sec-WebSocket-Key: {key}\r\n"
        "Sec-WebSocket-Version: 13\r\n\r\n"
    )
    sock.sendall(request.encode())

    buf = b""
    sock.settimeout(timeout)
    while b"\r\n\r\n" not in buf:
        chunk = sock.recv(4096)
        if not chunk:
            raise RuntimeError("connection closed during handshake")
        buf += chunk
    header, _, rest = buf.partition(b"\r\n\r\n")
    status_line = header.split(b"\r\n", 1)[0]
    if b" 101" not in status_line:
        raise RuntimeError(f"expected 101 Switching Protocols, got: {status_line.decode(errors='replace')}")
    return Frames(sock, rest)


def main():
    if len(sys.argv) < 3:
        log("usage: _ws_resume_check.py <ws_url> <head_height> [timeout_secs]")
        print("ws_resume=fail")
        return 1
    url = sys.argv[1]
    head = int(sys.argv[2])
    timeout = float(sys.argv[3]) if len(sys.argv) > 3 else 20.0
    deadline = time.time() + timeout

    try:
        frames = connect(url, timeout)
    except Exception as exc:  # noqa: BLE001 - report any connect failure
        log(f"connect failed: {exc}")
        print("ws_resume=fail")
        return 1

    replay_seen = False
    live_seen = False
    replay_complete_seen = False
    try:
        while time.time() < deadline and not (replay_seen and live_seen):
            opcode, payload = frames.next(deadline)
            if opcode == 0x8:  # close
                log("server closed the stream")
                break
            if opcode in (0x9, 0xA):  # ping/pong - ignore
                continue
            if opcode != 0x1:  # only text envelopes carry JSON
                continue
            try:
                env = json.loads(payload.decode("utf-8"))
            except (ValueError, UnicodeDecodeError):
                continue
            kind = env.get("type")
            if kind == "retention_gap":
                log(f"retention_gap: {env}")
                print("ws_resume=fail")
                return 1
            if kind == "lagged":
                log(f"lagged: {env}")
                continue
            if kind == "replay_complete":
                replay_complete_seen = True
                log(f"replay_complete up_to_height={env.get('up_to_height')}")
                continue
            if kind == "block":
                height = env.get("data", {}).get("height")
                if height is None:
                    continue
                if height <= head:
                    replay_seen = True
                    log(f"replay block height={height}")
                else:
                    live_seen = True
                    log(f"live block height={height}")
    except TimeoutError:
        log("timed out waiting for frames")
    except (EOFError, OSError) as exc:
        log(f"read error: {exc}")

    log(
        f"replay_seen={replay_seen} replay_complete_seen={replay_complete_seen} "
        f"live_seen={live_seen}"
    )
    if replay_seen and live_seen:
        print("ws_resume=pass")
        return 0
    print("ws_resume=fail")
    return 1


if __name__ == "__main__":
    sys.exit(main())
