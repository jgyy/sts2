"""What the model fights share: drawing, the combat state the planner reads, its turn, NAME=VALUE patches, the arms table."""

import collections
import logging
import multiprocessing
import os
import random
import sys
from contextlib import ExitStack
from unittest import mock

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from sts2bot import learn
from sts2bot.fight import battle, combat, triggers
from tests.fake_game import card

UNPLAYABLE = {"can_play": False, "unplayable_reason": "HasUnplayableKeyword"}


def shuffled(rng, kinds: str, counts) -> list:
    """A deck of counts[i] cards of kinds[i], shuffled."""
    deck = [k for k, n in zip(kinds, counts) for _ in range(n)]
    rng.shuffle(deck)
    return deck


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


def piles(face):
    """`pile(kinds)` for game_state, from `face` as in hand_cards."""
    return lambda cs: [{"name": f[1], "cost": str(f[3]), "description": f[4]} for f in map(face, cs)]


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


def status(*powers) -> list:
    """Powers as the game lists them, from (id, amount) pairs; those at 0 are left out."""
    return [{"id": p, "amount": n} for p, n in powers if n]


def attack(d: int, hits: int = 1) -> dict:
    """An attack intent, labelled as the game does."""
    return {"type": "Attack", "label": f"{d}x{hits}" if hits > 1 else str(d)}


def turn(state, hand: list, done=lambda: False, most: int = 12):
    """The planner's plays of a turn, (index, card kind, action) each, until it ends the turn or `done()`."""
    for _ in range(most):
        if done() or (act := combat.choose_card(state())) is None:
            return
        yield act[1]["card_index"], hand[act[1]["card_index"]], act


def hit(e: dict, d: int, block: str = "block") -> None:
    """d damage to an enemy, through its Block (`e[block]`) first."""
    soak = min(e[block], d)
    e[block] -= soak
    e["hp"] = max(0, e["hp"] - (d - soak))


def through(block: int, d: int) -> tuple:
    """(our Block left, damage through it) when d damage meets it."""
    soak = min(block, d)
    return block - soak, d - soak


def target(act: tuple, foes: dict) -> str:
    """The enemy a planned card is aimed at, or the first one alive when it named none (or a dead one)."""
    t = act[1].get("target")
    return t if t is not None and foes[t]["hp"] > 0 else next(x for x in foes if foes[x]["hp"] > 0)


def setting(name: str, value: float):
    """Patch one constant of battle.py, combat.py or triggers.py (NAME=VALUE on the command line)."""
    mod = next(m for m in (battle, combat, triggers) if hasattr(m, name))
    return mock.patch.object(mod, name, value)


def knob_setting(name: str, value: float):
    """A constant as `setting`, or a setting of learn.KNOB (K.name=VALUE)."""
    return mock.patch.dict(learn.KNOB, {name[2:]: value}) if name.startswith("K.") else setting(name, value)


def patched(arm: str, setting=knob_setting) -> ExitStack:
    """Every NAME=VALUE of an arm ("+" between them) entered, on debuff_worth 2.0 (the live sampler plays 2.0 and 3.0)."""
    stack = ExitStack()
    for p in [mock.patch.dict(learn.KNOB, {"debuff_worth": 2.0})] + [setting(p.split("=")[0], float(p.split("=")[1])) for p in arm.split("+") if "=" in p]:
        stack.enter_context(p)
    return stack


def played(fight, spec, n: int, seed: int, **kw) -> list:
    """n fights of a deck, a seed for each: every arm plays the same shuffles."""
    logging.disable(logging.CRITICAL)
    return [fight(spec, random.Random(seed * 10007 + j), **kw) for j in range(n)]


def mean(rows, i: int) -> float:
    return sum(r[i] for r in rows) / len(rows)


def firsts(rows, i: int) -> dict:
    """The rows' tallies of what died first (column i), added up and sorted."""
    return dict(sorted(sum((collections.Counter(o[i]) for o in rows), collections.Counter()).items()))


def on_all_cores(job, jobs: list, most: int = 12) -> list:
    with multiprocessing.Pool(min(most, os.cpu_count() or 2)) as pool:
        return pool.map(job, jobs)


def table(run, decks: list, arms: list):
    """`[FIGHTS] [ARM ...]`: run(arm, deck, FIGHTS, 900 + its place) for every arm and deck: (the results, [(arm, its rows)])."""
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 100
    arms = sys.argv[2:] or arms
    out = on_all_cores(run, [(arm, dk, n, 900 + i) for arm in arms for i, dk in enumerate(decks)])
    return out, [(arm, [o for o in out if o[0] == arm]) for arm in arms]


def trace_args(decks: list) -> tuple:
    """`trace [SEED] [DECK] [ARM]`: (seed, deck, arm), by default 1, decks[1] and "on"."""
    a = sys.argv[2:]
    return int(a[0]) if a else 1, decks[int(a[1])] if len(a) > 1 else decks[1], a[2] if len(a) > 2 else "on"


def flips(fight, deck, i: int, arms, patch, keep=None, kw=lambda arm: {}):
    """(seed, {arm: fight(...)[:keep]}) for the 200 fights of deck i, every arm under patch(arm) on the same shuffles."""
    for j in range(200):
        seed, res = (900 + i) * 10007 + j, {}
        for arm in arms:
            with patch(arm):
                res[arm] = fight(deck, random.Random(seed), **kw(arm))[:keep]
        yield seed, res
