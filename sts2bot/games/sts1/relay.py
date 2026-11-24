#!/usr/bin/env python3
"""The first game's end of the bridge. CommunicationMod starts it and talks to it over stdin/stdout (a JSON state per line
when the game is ready, commands a line at a time); the bot talks to it over localhost:

    GET  /         {"status": "ok", "connected": true once the game has sent a state}
    GET  /state    {"seq": n, "message": the last state the game sent}
    POST /command  {"command": "play 1 0"} -> {"status": "ok"|"error", "message": the next state, ...}

Standard library only, no imports from the bot. Stdout is the game's command channel: logs go to stderr."""

from __future__ import annotations

import argparse
import json
import os
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

DEFAULT_PORT = 15527
ANSWER_TIMEOUT = 20.0  # seconds to wait for the game to answer a command (an enemy turn can take a while)


def _log(message: str) -> None:
    print(f"{time.strftime('%H:%M:%S')} relay: {message}", file=sys.stderr, flush=True)


class Relay:
    """Holds the game's latest message and passes commands to it, one at a time."""

    def __init__(self, out=None, answer_timeout: float = ANSWER_TIMEOUT):
        self.out, self.answer_timeout = out if out is not None else sys.stdout, answer_timeout
        self.cond, self.send_lock = threading.Condition(), threading.Lock()
        self.seq = 0  # messages received from the game
        self.latest: dict | None = None  # the last game state (errors are kept apart)
        self.last_error: tuple[int, str] | None = None  # (seq, text) of the last error message
        self.waiting = False  # a command was sent and the game has not answered yet

    def feed(self, line: str) -> None:
        """One line from the game."""
        line = line.strip()
        if not line:
            return
        try:
            msg = json.loads(line)
        except ValueError:
            _log(f"not JSON from the game: {line[:200]}")
            return
        if not isinstance(msg, dict):
            return
        with self.cond:
            self.seq += 1
            if msg.get("error") is not None:
                self.last_error = (self.seq, str(msg["error"]))
            else:
                self.latest = msg
            self.waiting = False
            self.cond.notify_all()

    def snapshot(self) -> dict:
        with self.cond:
            return {"seq": self.seq, "message": self.latest}

    def send(self, command: str, timeout: float | None = None) -> dict:
        """Pass one command to the game and wait for its answer."""
        command = " ".join(str(command).split())  # one line, whatever was sent
        if not command:
            return {"status": "error", "error": "empty command"}
        timeout = self.answer_timeout if timeout is None else float(timeout)
        with self.send_lock:
            with self.cond:
                # The game takes one command at a time: one sent before it answered the last is lost.
                self.cond.wait_for(lambda: not self.waiting, timeout)
                before = self.seq
                self.waiting = True
                self.out.write(command + "\n")
                self.out.flush()
                if not self.cond.wait_for(lambda: self.seq > before, timeout):
                    return {"status": "ok", "message": None, "pending": True, "seq": self.seq}
                if self.last_error and self.last_error[0] > before:
                    return {"status": "error", "error": self.last_error[1], "seq": self.seq}
                return {"status": "ok", "message": self.latest, "seq": self.seq}


def make_handler(relay: Relay):
    class Handler(BaseHTTPRequestHandler):
        def _send(self, code: int, body: dict) -> None:
            raw = json.dumps(body).encode()
            self.send_response(code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(raw)))
            self.end_headers()
            self.wfile.write(raw)

        def do_GET(self):
            self._send(200, relay.snapshot() if self.path.startswith("/state") else {"status": "ok", "game": "sts1", "connected": relay.seq > 0})

        def do_POST(self):
            try:
                body = json.loads(self.rfile.read(int(self.headers.get("Content-Length") or 0)) or b"{}")
            except ValueError:
                self._send(400, {"status": "error", "error": "bad JSON"})
                return
            if not self.path.startswith("/command") or not isinstance(body, dict):
                self._send(404, {"status": "error", "error": "unknown request"})
                return
            result = relay.send(str(body.get("command") or ""), body.get("timeout"))
            self._send(200 if result["status"] == "ok" else 400, result)

        def log_message(self, *args):
            pass

    return Handler


def serve(port: int, relay: Relay, tries: int = 30) -> ThreadingHTTPServer:
    """Listen on localhost. A relay of an earlier game session may still be closing: wait for its port."""
    for attempt in range(tries):
        try:
            server = ThreadingHTTPServer(("127.0.0.1", port), make_handler(relay))
            server.daemon_threads = True
            return server
        except OSError as e:
            if attempt == tries - 1:
                raise
            _log(f"port {port} busy ({e}); retrying")
            time.sleep(1.0)
    raise OSError("unreachable")


def _ask_until_heard(relay: Relay, every: float = 5.0) -> None:
    """The game sends its state when it has something to say; at start-up that can be never. Ask until it does."""
    while True:
        time.sleep(every)
        if relay.seq:
            return
        relay.send("state", timeout=every)


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="Relay between CommunicationMod (stdin/stdout) and the bot (localhost HTTP).")
    ap.add_argument("--port", type=int, default=DEFAULT_PORT)
    args = ap.parse_args(argv)
    out = sys.stdout
    sys.stdout = sys.stderr  # nothing but commands may reach the game's command channel
    relay = Relay(out)
    server = serve(args.port, relay)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    _log(f"listening on 127.0.0.1:{args.port}")
    out.write("ready\n")  # CommunicationMod waits for this before it sends anything
    out.flush()
    threading.Thread(target=_ask_until_heard, args=(relay,), daemon=True).start()
    for line in sys.stdin:
        relay.feed(line)
    _log("the game closed the connection; exiting")
    server.shutdown()
    os._exit(0)  # (request threads may still be waiting on the game)


if __name__ == "__main__":
    raise SystemExit(main())
