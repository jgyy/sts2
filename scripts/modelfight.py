"""What the model fights share (crabfight.py, kinfight.py, knightfight.py, bosses/*.py).

Each of them plays one fight of the game as the logs show it, with the bot's own planner choosing the
cards. The fight itself is each script's own; drawing a hand, writing the combat state the planner reads,
patching a constant from the command line and running the fights on every core are the same everywhere.
"""

import multiprocessing
import os
import sys
from unittest import mock

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from sts2bot.fight import battle, combat, triggers
from tests.fake_game import card

UNPLAYABLE = {"can_play": False, "unplayable_reason": "HasUnplayableKeyword"}


def deal(draw: list, disc: list, rng, size: int = 5) -> list:
    """Draw a hand, shuffling the discard pile into the (emptied) draw pile when it runs out."""
    hand = []
    for _ in range(size):
        if not draw:
            draw[:] = disc
            disc.clear()
            rng.shuffle(draw)
        if draw:
            hand.append(draw.pop())
    return hand


def hand_cards(hand: list, face, dead: str = "F", **extra) -> list:
    """The hand as the game lists it; `face(kind)` is (id, name, type, cost, text, target) for a card kind."""
    return [card(i, *face(c), **(UNPLAYABLE if c in dead else {}), **extra) for i, c in enumerate(hand)]


def game_state(kind: str, run: dict, r: int, enemies: list, cards: list, pile, draw: list, disc: list, *,
               hp: int, maxhp: int, block: int, energy: int, status: list, **extra) -> dict:
    """The combat state the bot reads; `pile(kinds)` lists a pile's cards."""
    return {
        "state_type": kind, "run": run,
        "battle": {"round": r, "turn": "player", "is_play_phase": True, "enemies": enemies},
        "player": {"hp": hp, "max_hp": maxhp, "block": block, "energy": energy, "hand": cards, "potions": [], "status": status,
                   "draw_pile": pile(draw), "discard_pile": pile(disc), "draw_pile_count": len(draw), "discard_pile_count": len(disc), "exhaust_pile_count": 0},
        **extra,
    }


def target(act: tuple, foes: dict) -> str:
    """The enemy a planned card is aimed at, or the first one alive when it named none (or a dead one)."""
    t = act[1].get("target")
    return t if t is not None and foes[t]["hp"] > 0 else next(x for x in foes if foes[x]["hp"] > 0)


def setting(name: str, value: float):
    """Patch one constant of battle.py, combat.py or triggers.py (NAME=VALUE on the command line)."""
    mod = next(m for m in (battle, combat, triggers) if hasattr(m, name))
    return mock.patch.object(mod, name, value)


def count(tally: dict, key) -> None:
    tally[key] = tally.get(key, 0) + 1


def merged(dicts) -> dict:
    """Counts added up across several tallies."""
    out: dict = {}
    for d in dicts:
        for k, v in d.items():
            out[k] = out.get(k, 0) + v
    return out


def on_all_cores(job, jobs: list, most: int = 12) -> list:
    with multiprocessing.Pool(min(most, os.cpu_count() or 2)) as pool:
        return pool.map(job, jobs)
