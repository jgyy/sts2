"""Thin HTTP client for the STS2MCP bridge mod (localhost REST API).

It speaks plain HTTP/1.1 over a socket: urllib.request would bring in SSL, e-mail parsing and proxy
handling (about 6 MB of a process that otherwise needs some 20) for a server on this machine.
"""

from __future__ import annotations

import json
import socket
from typing import Any
from urllib.parse import quote, urlsplit


class GameUnavailable(Exception):
    """The game (or the bridge mod inside it) is not answering."""


def _dechunk(body: bytes) -> bytes:
    """The body of a response sent with "Transfer-Encoding: chunked"."""
    out, pos = bytearray(), 0
    while True:
        eol = body.index(b"\r\n", pos)
        size = int(body[pos:eol].split(b";")[0], 16)
        if size == 0:
            return bytes(out)
        out += body[eol + 2 : eol + 2 + size]
        pos = eol + 2 + size + 2


class Client:
    def __init__(self, host: str = "127.0.0.1", port: int = 15526, timeout: float = 15.0):
        self.base = f"http://{host}:{port}"
        self.url = f"{self.base}/api/v1/singleplayer"
        self.timeout = timeout
        self._address = (host, port)  # (a socket of our own: localhost traffic never goes through a system proxy)

    def _exchange(self, url: str, data: bytes | None) -> tuple[int, bytes]:
        """Send one request and read the whole answer: (status code, body)."""
        parts = urlsplit(url)
        path = (parts.path or "/") + (f"?{parts.query}" if parts.query else "")
        head = f"{'GET' if data is None else 'POST'} {path} HTTP/1.1\r\nHost: {parts.netloc}\r\nConnection: close\r\nAccept: application/json\r\n"
        if data is not None:
            head += f"Content-Type: application/json\r\nContent-Length: {len(data)}\r\n"
        with socket.create_connection(self._address, timeout=self.timeout) as sock:
            sock.sendall(head.encode("latin-1") + b"\r\n" + (data or b""))
            chunks = []
            while chunk := sock.recv(65536):
                chunks.append(chunk)
        raw = b"".join(chunks)
        header, sep, body = raw.partition(b"\r\n\r\n")
        lines = header.decode("latin-1").split("\r\n")
        status = lines[0].split(" ", 2)
        if not sep or len(status) < 2 or not status[0].startswith("HTTP/"):
            raise GameUnavailable("not an HTTP answer from the game")
        fields = {k.strip().lower(): v.strip() for k, _, v in (line.partition(":") for line in lines[1:])}
        if "chunked" in fields.get("transfer-encoding", "").lower():
            body = _dechunk(body)
        elif fields.get("content-length", "").isdigit():
            body = body[: int(fields["content-length"])]
        return int(status[1]), body

    def _request(self, url: str, body: dict | None = None) -> dict[str, Any]:
        data = None if body is None else json.dumps(body).encode()
        try:
            code, raw = self._exchange(url, data)
        except (OSError, ValueError) as e:  # (ValueError: a status or chunk size that is not a number)
            raise GameUnavailable(str(e)) from e
        if code >= 400:
            # The mod reports action failures as 4xx/5xx with a JSON body.
            try:
                parsed = json.loads(raw)
            except ValueError:
                parsed = {}
            if not isinstance(parsed, dict):
                parsed = {}
            parsed.setdefault("status", "error")
            parsed.setdefault("error", f"HTTP {code}")
            return parsed
        try:
            parsed = json.loads(raw)
        except ValueError as e:
            raise GameUnavailable(f"bad JSON from game: {e}") from e
        return parsed if isinstance(parsed, dict) else {"value": parsed}

    def ping(self) -> bool:
        try:
            self._request(self.base + "/")
            return True
        except GameUnavailable:
            return False

    def profile(self) -> dict[str, Any]:
        """Profile statistics, including each character's highest unlocked ascension."""
        return self._request(f"{self.base}/api/v1/profile")

    def card(self, card_id: str) -> dict[str, Any] | None:
        """The game's own entry for a card we have met: its text as it is and as it is upgraded."""
        found = self._request(f"{self.base}/api/v1/wiki?type=card&limit=3&q={quote(card_id)}")
        for entry in found.get("results") or []:
            if str(entry.get("id")) == card_id:
                return entry
        return None

    def state(self) -> dict[str, Any]:
        return self._request(self.url)

    def act(self, action: str, **params: Any) -> dict[str, Any]:
        return self._request(self.url, {"action": action, **params})
