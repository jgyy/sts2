"""Main loop: read state, decide, act, repeat. One Bot plays run after run."""

from __future__ import annotations

import hashlib
import json
import logging
import time
from collections import defaultdict
from pathlib import Path

from . import combat, strategy
from .client import Client
from .strategy import Memory

log = logging.getLogger("sts2bot")

COMBAT_STATES = ("monster", "elite", "boss")
MAX_STRIKES = 2  # failed attempts before an action is shelved for the current state
MAX_PLAYS_PER_TURN = 40
_VOLATILE = {"is_hovered", "is_highlighted"}


class Stuck(Exception):
    """Nothing the bot tries changes the game any more."""


def _scrub(obj):
    if isinstance(obj, dict):
        return {k: _scrub(v) for k, v in obj.items() if k not in _VOLATILE}
    if isinstance(obj, list):
        return [_scrub(v) for v in obj]
    return obj


def fingerprint(state: dict) -> str:
    raw = json.dumps(_scrub(state), sort_keys=True, default=str)
    return hashlib.sha1(raw.encode()).hexdigest()


class Bot:
    def __init__(
        self,
        client: Client,
        *,
        character: str = "IRONCLAD",
        log_dir: Path | None = None,
        poll: float = 0.4,
        settle: float = 3.0,
        stuck_timeout: float = 300.0,
        clock=time.monotonic,
        sleep=time.sleep,
    ):
        self.client = client
        self.character = character
        self.log_dir = log_dir
        self.poll = poll
        self.settle = settle
        self.travel_settle = 8.0
        self.stuck_timeout = stuck_timeout
        self.clock = clock
        self.sleep = sleep

        self.mem = Memory()
        self.max_runs: int | None = None
        self.abandon_next = False  # set by the supervisor to drop a run that keeps hanging
        self.runs_finished = 0
        self.wins = 0
        self._strikes: dict = defaultdict(int)
        self._last_fp = ""
        self._last_change = clock()
        self._last_retry = clock()
        self._over_logged = False
        self._turn_key: tuple = ()
        self._turn_plays = 0
        self._last_where: tuple = ()

    # ------------------------------------------------------------ deciding

    def progress(self) -> tuple:
        """Changes whenever the bot gets somewhere new (used to spot repeat hangs)."""
        return (self.runs_finished, *self._last_where)

    def candidates(self, state: dict) -> list[tuple[str, dict]]:
        kind = state.get("state_type")
        mem = self.mem
        if kind == "menu":
            if self.abandon_next and (state.get("menu_screen") or "main") == "main":
                names = [o if isinstance(o, str) else o.get("name") for o in state.get("options") or []]
                if "abandon_run" in names:
                    return [("menu_select", {"option": "abandon_run"})]
                self.abandon_next = False
            return strategy.menu(state, mem, self.character)
        if kind in COMBAT_STATES:
            return self._combat(state)
        handler = {
            "hand_select": strategy.hand_select,
            "rewards": strategy.rewards,
            "card_reward": strategy.card_reward,
            "map": strategy.map_screen,
            "event": strategy.event,
            "rest_site": strategy.rest_site,
            "shop": strategy.shop,
            "treasure": strategy.treasure,
            "card_select": strategy.card_select,
            "bundle_select": strategy.bundle_select,
            "relic_select": strategy.relic_select,
            "crystal_sphere": strategy.crystal_sphere,
            "game_over": strategy.game_over,
            "overlay": strategy.overlay,
        }.get(kind)
        if kind == "fake_merchant":
            return strategy.shop(state, mem, key="fake_merchant")
        return handler(state, mem) if handler else []

    def _combat(self, state: dict) -> list[tuple[str, dict]]:
        battle = state.get("battle")
        player = state.get("player") or {}
        if not battle or "hand" not in player:
            return []  # combat is starting or just ended
        if battle.get("turn") != "player" or not battle.get("is_play_phase"):
            return []
        self.mem.deck_size = (
            len(player.get("hand") or [])
            + int(player.get("draw_pile_count") or 0)
            + int(player.get("discard_pile_count") or 0)
            + int(player.get("exhaust_pile_count") or 0)
        )
        key = (*self._where(state), battle.get("round"))
        if key != self._turn_key:
            self._turn_key, self._turn_plays = key, 0
        if self._turn_plays >= MAX_PLAYS_PER_TURN:
            return [("end_turn", {})]
        return combat.actions(state)

    @staticmethod
    def _where(state: dict) -> tuple:
        run = state.get("run") or {}
        return (run.get("act"), run.get("floor"))

    # -------------------------------------------------------------- acting

    def _after_success(self, state: dict, action: str, params: dict) -> None:
        kind = state.get("state_type")
        if action == "select_card":
            self.mem.selected.add(params.get("index"))
        elif action == "skip_card_reward":
            where = self._where(state)
            self.mem.card_skips[where] = self.mem.card_skips.get(where, 0) + 1
        elif action in ("play_card", "use_potion"):
            self._turn_plays += 1
        elif action == "menu_select" and kind == "menu":
            option = str(params.get("option"))
            if state.get("menu_screen") == "character_select" and option not in ("confirm", "embark"):
                self.mem.character_selected = True
            elif option == "abandon_run":
                log.warning("abandoning the current run")
            elif option in ("yes", "confirm") and self.abandon_next and state.get("menu_screen") == "popup":
                self.abandon_next = False
                self.mem.reset_run()

    def _note_state(self, state: dict) -> None:
        kind = state.get("state_type")
        if state.get("run"):
            where = self._where(state)
            if where != self._last_where:
                self._last_where = where
                p = state.get("player") or {}
                log.info("act %s floor %s  %s  hp %s/%s  gold %s", *where, kind, p.get("hp"), p.get("max_hp"), p.get("gold"))
        if kind == "game_over":
            if not self._over_logged:
                self._over_logged = True
                self._record_run(state)
        elif self._over_logged:
            self._over_logged = False
            self.mem.reset_run()
            self.mem.runs_finished = self.runs_finished

    def _record_run(self, state: dict) -> None:
        p = state.get("player") or {}
        run = state.get("run") or {}
        won = int(p.get("hp") or 0) > 0
        self.runs_finished += 1
        self.wins += won
        entry = {
            "time": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
            "character": p.get("character"),
            "result": "win" if won else "loss",
            "act": run.get("act"),
            "floor": run.get("floor"),
            "ascension": run.get("ascension"),
            "hp": p.get("hp"),
            "max_hp": p.get("max_hp"),
            "gold": p.get("gold"),
            "relics": [r.get("id") for r in p.get("relics") or []],
            "deck_size": self.mem.deck_size,
        }
        log.info("run %d over: %s on act %s floor %s (%d wins so far)", self.runs_finished, entry["result"], entry["act"], entry["floor"], self.wins)
        if self.log_dir:
            self.log_dir.mkdir(parents=True, exist_ok=True)
            with open(self.log_dir / "runs.jsonl", "a") as f:
                f.write(json.dumps(entry) + "\n")

    def _wait_for_change(self, fp: str, leave: str | None = None) -> None:
        """Wait for the action to show. With `leave`, wait until that screen is gone.

        The map keeps answering with fresh options while the travel animation
        plays; acting on those would click a second node.
        """
        deadline = self.clock() + (self.travel_settle if leave else self.settle)
        while self.clock() < deadline:
            self.sleep(min(0.15, self.poll))
            state = self.client.state()
            if leave:
                if state.get("state_type") != leave:
                    return
            elif fingerprint(state) != fp:
                self.sleep(min(0.25, self.poll))  # let the animation finish resolving
                return

    def step(self) -> bool:
        """One read-decide-act cycle. Returns True if an action was sent."""
        state = self.client.state()
        fp = fingerprint(state)
        now = self.clock()
        if fp != self._last_fp:
            self._last_fp, self._last_change, self._last_retry = fp, now, now
            self._strikes.clear()
        elif now - self._last_change > self.stuck_timeout:
            raise Stuck(f"no change for {self.stuck_timeout:.0f}s in state {state.get('state_type')!r}")
        elif now - self._last_retry > 8:  # clicks sent mid-animation are ignored; try again soon
            self._last_retry = now
            self._strikes.clear()  # give shelved actions another go

        self._note_state(state)
        if self._done():
            return False
        for action, params in self.candidates(state):
            key = (action, json.dumps(params, sort_keys=True))
            if self._strikes[key] >= MAX_STRIKES:
                continue
            result = self.client.act(action, **params)
            if result.get("status") == "error" or "error" in result:
                self._strikes[key] += 1
                log.debug("%s %s rejected: %s", action, params, result.get("error"))
                self.sleep(self.poll)
                return True
            log.debug("%s %s -> %s", action, params, result.get("message"))
            self._after_success(state, action, params)
            self._wait_for_change(fp, leave="map" if action == "choose_map_node" else None)
            if fingerprint(self.client.state()) == fp:
                self._strikes[key] += 1  # accepted, but nothing happened
            return True
        self.sleep(self.poll)
        return False

    def _done(self) -> bool:
        """True once the requested number of runs is over and its end screen is dismissed."""
        return self.max_runs is not None and self.runs_finished >= self.max_runs and not self._over_logged

    def run(self, max_runs: int | None = None) -> None:
        """Play until `max_runs` runs have finished (forever if None)."""
        self.max_runs = max_runs
        self._last_change = self.clock()
        while not self._done():
            self.step()
