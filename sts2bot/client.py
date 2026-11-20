"""Thin HTTP client for the STS2MCP bridge mod (localhost REST API)."""

from __future__ import annotations

import json
import urllib.error
import urllib.request
from typing import Any
from urllib.parse import quote


class GameUnavailable(Exception):
    """The game (or the bridge mod inside it) is not answering."""


class Client:
    def __init__(self, host: str = "127.0.0.1", port: int = 15526, timeout: float = 15.0):
        self.base = f"http://{host}:{port}"
        self.url = f"{self.base}/api/v1/singleplayer"
        self.timeout = timeout
        # Never route localhost traffic through a system proxy.
        self._opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))

    def _request(self, url: str, body: dict | None = None) -> dict[str, Any]:
        data = None if body is None else json.dumps(body).encode()
        req = urllib.request.Request(url, data=data, headers={} if body is None else {"Content-Type": "application/json"})
        try:
            with self._opener.open(req, timeout=self.timeout) as resp:
                raw = resp.read()
        except urllib.error.HTTPError as e:
            # The mod reports action failures as 4xx/5xx with a JSON body.
            raw = e.read()
            try:
                parsed = json.loads(raw)
            except ValueError:
                parsed = {}
            if not isinstance(parsed, dict):
                parsed = {}
            parsed.setdefault("status", "error")
            parsed.setdefault("error", f"HTTP {e.code}")
            return parsed
        except (urllib.error.URLError, OSError, TimeoutError) as e:
            raise GameUnavailable(str(e)) from e
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
