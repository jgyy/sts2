"""One log line per boss fight, written when the fight is over.

Working out which boss fights were won from the floor log alone is
error-prone: a win leaves no "run over" line, so every fight looked lost.
This watches the states as they come in and says, when a boss fight ends,
what it was, how it went and how the boss and the player were doing.
"""

from __future__ import annotations

import re

from . import combat


# Screens the game opens in the middle of a fight, like the card a Survivor
# makes us discard. They do not end the fight (one did, and the watch logged
# a win at 87 of the boss's 240 HP).
_IN_FIGHT_SCREENS = {"hand_select", "card_select", "bundle_select", "overlay"}


def _base_name(entity_id: str) -> str:
    return re.sub(r"_\d+$", "", entity_id)


class BossWatch:
    def __init__(self) -> None:
        self.fight: dict | None = None
        self.brought: str | None = None  # a log line for the fight that has just begun: what the bot holds going in

    def update(self, state: dict, kind: str | None) -> str | None:
        """Feed every state the bot reads. Returns a log line when a boss fight has just ended."""
        if kind == "boss" and state.get("battle"):
            self._seen(state)
            return None
        if self.fight is None or kind in _IN_FIGHT_SCREENS:
            return None
        fight, self.fight = self.fight, None
        return self._summary(fight, lost=kind == "game_over")

    def _seen(self, state: dict) -> None:
        battle = state.get("battle") or {}
        player = state.get("player") or {}
        enemies = combat.read_enemies(state)
        if not enemies and self.fight is None:
            return  # nothing alive yet: not a fight we can report on
        boss_hp = sum(e.hp for e in enemies)
        run = state.get("run") or {}
        if self.fight is None:
            self.fight = {
                "where": (run.get("act"), run.get("floor")),
                "names": sorted({_base_name(e.entity_id) for e in enemies}),
                "boss_hp0": boss_hp,
                "hp0": int(player.get("hp") or 0),
                "max_hp": int(player.get("max_hp") or 0),
                "rounds": set(),
                # What the bot brought to the fight (to tell later what wins them).
                "relics": len(player.get("relics") or []),
                "potions": len(player.get("potions") or []),
                "deck": sum(len(player.get(pile) or []) for pile in ("hand", "draw_pile", "discard_pile")),
            }
            # ... and by name, since a run that wins leaves no record of its deck at this boss: the run history
            # only has the deck a run ended with, so decks that beat a boss could never be set against ones that lost.
            names = lambda key, field: [str(x.get(field) or x.get("name") or "?") for x in player.get(key) or []]
            cards = sorted(n for pile in ("hand", "draw_pile", "discard_pile") for n in names(pile, "name"))
            self.brought = (
                f"boss fight begins: act {run.get('act')} floor {run.get('floor')} {'+'.join(self.fight['names']) or '?'}; "
                f"relics {' '.join(names('relics', 'id'))}; potions {', '.join(names('potions', 'name'))}; cards {', '.join(cards)}"
            )
        f = self.fight
        f["boss_hp"] = boss_hp
        f["hp"] = int(player.get("hp") or 0)
        f["names"] = sorted(set(f["names"]) | {_base_name(e.entity_id) for e in enemies})
        if battle.get("round") is not None:
            f["rounds"].add(int(battle.get("round")))

    @staticmethod
    def _summary(f: dict, lost: bool) -> str:
        act, floor = f["where"]
        return (
            f"boss fight over: act {act} floor {floor} {'+'.join(f['names']) or '?'} "
            f"{'lost' if lost else 'won'}; boss hp {f['boss_hp0']} -> {f.get('boss_hp', f['boss_hp0'])}; "
            f"player hp {f['hp0']} -> {f.get('hp', f['hp0'])}/{f['max_hp']}; rounds {len(f['rounds'])}; "
            f"brought relics {f.get('relics', 0)}, potions {f.get('potions', 0)}, deck {f.get('deck', 0)}"
        )
