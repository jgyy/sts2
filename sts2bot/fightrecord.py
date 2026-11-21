"""One record per fight, kept for good (fights.jsonl next to the logs).

The log holds about four hours of play before it rotates away, and the run
history only has the deck a run ended with. So what a fight cost could be set
against the deck that fought it for 130 runs at a time and no more: enough to
see that a Block card held at the act 1 boss makes every act 2 fight 2.6% of
Max HP cheaper, not enough to say which cards and relics do it. Each line here
is a fight as it began (deck, relics, potions, HP) and as it ended.

    python3 scripts/fightlog.py decks      reads it
"""

from __future__ import annotations

import json
import logging
import re
import time
from pathlib import Path

log = logging.getLogger("sts2bot")

COMBAT = ("monster", "elite", "boss")
# Screens the game opens in the middle of a fight (see bossfight.py): they do not end it.
_IN_FIGHT_SCREENS = {"hand_select", "card_select", "bundle_select", "overlay"}
LIMIT = 64 << 20  # bytes before the file is set aside as fights.jsonl.1 (about ten days of play)
_PILES = ("hand", "draw_pile", "discard_pile", "exhaust_pile")


def _names(player: dict, key: str, field: str = "name") -> list[str]:
    return [str(x.get(field) or x.get("name") or "?") for x in player.get(key) or []]


class FightWatch:
    def __init__(self, path: Path | None = None) -> None:
        self.path = path
        self.fight: dict | None = None

    def update(self, state: dict, kind: str | None) -> dict | None:
        """Feed every state the bot reads. Returns the record of a fight that has just ended."""
        if kind in COMBAT and state.get("battle"):
            run = state.get("run") or {}
            where = (run.get("act"), run.get("floor"))
            done = None
            if self.fight is not None and self.fight["where"] != where:
                done = self._close(state, lost=False)  # (one fight straight into the next: it was won)
            self._seen(state, str(kind), where)
            return done
        if self.fight is None or kind in _IN_FIGHT_SCREENS:
            return None
        return self._close(state, lost=kind == "game_over")

    def _seen(self, state: dict, kind: str, where: tuple) -> None:
        battle, player, run = state.get("battle") or {}, state.get("player") or {}, state.get("run") or {}
        foes = sorted({re.sub(r"_\d+$", "", str(e.get("entity_id") or e.get("name") or "?")) for e in battle.get("enemies") or []})
        rnd = int(battle.get("round") or 0)
        if self.fight is None:
            if not foes:
                return  # nothing on the field yet
            self.fight = {
                "where": where,
                "time": time.strftime("%Y-%m-%dT%H:%M:%S"),
                "character": player.get("character"),
                "ascension": run.get("ascension"),
                "act": where[0],
                "floor": where[1],
                "kind": kind,
                "enemies": foes,
                "count": len(battle.get("enemies") or []),
                # False for a fight the bot came into half way (a restart): its HP and piles are not the fight's own.
                "whole": rnd <= 1 and not player.get("discard_pile") and not player.get("exhaust_pile"),
                "hp_in": int(player.get("hp") or 0),
                "max_hp": int(player.get("max_hp") or 0),
                "relics": _names(player, "relics", "id"),
                "potions": _names(player, "potions"),
                "deck": sorted(n for pile in _PILES for n in _names(player, pile)),
                "rounds": 0,
            }
        f = self.fight
        f["enemies"] = sorted(set(f["enemies"]) | set(foes))
        f["rounds"] = max(f["rounds"], rnd)
        f["hp"] = int(player.get("hp") or 0)
        f["potions_left"] = len(player.get("potions") or [])

    def _close(self, state: dict, lost: bool) -> dict:
        f, self.fight = self.fight, None
        player = state.get("player") or {}
        del f["where"]
        f["died"] = lost
        # After a fight won the next screen has the HP it left us on, what the fight's end gave back included.
        f["hp_out"] = 0 if lost else int(player.get("hp") if player.get("hp") is not None else f.pop("hp", 0))
        f.pop("hp", None)
        self._write(f)
        return f

    def _write(self, record: dict) -> None:
        if self.path is None:
            return
        try:
            self.path.parent.mkdir(parents=True, exist_ok=True)
            if self.path.exists() and self.path.stat().st_size > LIMIT:
                self.path.replace(self.path.with_name(self.path.name + ".1"))
            with open(self.path, "a") as out:
                out.write(json.dumps(record, separators=(",", ":")) + "\n")
        except OSError as e:  # a full disk must not stop the bot playing
            log.warning("could not write %s: %s", self.path, e)
