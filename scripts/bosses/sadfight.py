"""The Fool's Sadness in a boss fight, as a model played by the bot's own planner.

    python3 scripts/bosses/sadfight.py [FIGHTS ARM ...]   arms: "on" (as the bot plays), NAME=VALUE (a constant of
                                                          battle/combat/triggers), S.NAME=VALUE (a constant of
                                                          fight/fool/sadness.py), K.name=VALUE (a setting), joined by "+"
    python3 scripts/bosses/sadfight.py trace [SEED] [DECK] [ARM]   one fight, play by play
    python3 scripts/bosses/sadfight.py flips DECK ARM [ARM]        the fights one arm wins and the other loses

A Fool deck with Sorrowful Strike (13 damage, 2 Sadness), Mourning Blow (0 cost, 10 damage, then a Sadness itself) and
Ennui (a Sadness and 2 more cards every turn) against an act 1 boss on a fixed cycle (12, 6x2, 16, +2 Strength, 10) that
goes Intangible every fifth round, as Soul Fysh does. Sadness is Unplayable and Retained, and a hand holds 10: from
2026-11-25 19:03, the Fool lost to Soul Fysh on 25 HP with ten Sadness in the hand from round 10.
"""

import logging
import os
import random
import sys
from unittest import mock

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from modelfight import attack, flips, game_state, knob_setting, mean, patched, piles, played, shuffled, status, table, through, trace_args, turn
from sts2bot.fight.fool import sadness
from tests.fake_game import card

UNPLAYABLE = {"can_play": False, "unplayable_reason": "HasUnplayableKeyword"}
HAND_CAP = 10

CARDS = {  # kind: (id, name, type, cost, text, target)
    "A": ("STRIKE_FOOL", "Strike", "Attack", 1, "Deal {d} damage.", "AnyEnemy"),
    "H": ("HEAVY", "Heavy Blow", "Attack", 2, "Deal {h} damage.", "AnyEnemy"),
    "B": ("DEFEND_FOOL", "Defend", "Skill", 1, "Gain {b} Block.", "Self"),
    "S": ("SORROWFUL_STRIKE", "Sorrowful Strike", "Attack", 1, "Deal {s} damage. Add 2 Sadness into your Hand.", "AnyEnemy"),
    "M": ("MOURNING_BLOW", "Mourning Blow", "Attack", 0, "Deal {m} damage. Transform this into Sadness and return it to your Hand.", "AnyEnemy"),
    "E": ("ENNUI", "Ennui", "Power", 1, "At the start of your turn, add 1 Sadness to your Hand and draw 2 additional cards.", "Self"),
    "X": ("SADNESS", "Sadness", "Status", 0, "Unplayable. Retain.", "None"),
    "F": ("ASCENDERS_BANE", "Ascender's Bane", "Curse", -1, "Unplayable. Ethereal.", "None"),
}
DAMAGE = {"A": 6, "H": 14, "S": 13, "M": 10}
KINDS = "AHBSMEF"


def intent(r, st):
    """(damage a hit, hits, kind) on round r (from 1), Strength in."""
    return [(12 + st, 1, "Attack"), (6 + st, 2, "Attack"), (16 + st, 1, "Attack"), (0, 0, "Buff"), (10 + st, 1, "Attack")][(r - 1) % 5]


def fight(spec, rng, maxhp=77, trace=False):
    boss_hp, db, energy = spec[0], spec[1], spec[-1]
    draw, disc, hp = shuffled(rng, KINDS, spec[2:-1]), [], maxhp
    e = dict(hp=boss_hp, st=0)
    hand, ennui, peak = [], 0, 0
    face = lambda c: tuple(str(x).format(d=DAMAGE["A"], h=DAMAGE["H"], s=DAMAGE["S"], m=DAMAGE["M"], b=db) for x in CARDS[c])
    for r in range(1, 31):
        hand = [c for c in hand if c == "X"]  # (only Sadness is Retained; the Bane is Ethereal and the rest discarded)
        if ennui and len(hand) < HAND_CAP:
            hand.append("X")
        for _ in range(5 + 2 * ennui):
            if len(hand) >= HAND_CAP:
                break
            if not draw:
                draw[:] = disc
                disc.clear()
                rng.shuffle(draw)
            if not draw:
                break
            hand.append(draw.pop())
        peak = max(peak, hand.count("X"))
        en, block = energy, 0
        intangible = r % 5 == 0
        if trace:
            print(f"r{r} hand {''.join(hand)} hp {hp} boss {e['hp']} st {e['st']} ennui {ennui}{' INTANGIBLE' if intangible else ''}")

        def state():
            d, h, kind = intent(r, e["st"])
            labels = [attack(d, h)] if h else [{"type": "Buff", "label": ""}]
            foes = [{"entity_id": "SOUL_FYSH_0", "hp": e["hp"], "max_hp": boss_hp, "block": 0,
                     "status": status(("STRENGTH_POWER", e["st"]), ("INTANGIBLE_POWER", int(intangible))), "intents": labels}]
            cards = [card(i, *face(c), **(UNPLAYABLE if c in "XF" else {})) for i, c in enumerate(hand)]
            return game_state("boss", {"act": 1, "floor": 17, "ascension": 5}, r, foes, cards, piles(face), draw, disc, hp=hp, maxhp=maxhp,
                              block=block, energy=en, status=status(("ENNUI_POWER", 2 * ennui)))

        for i, c, act in turn(state, hand, most=15):
            en -= CARDS[c][3]
            hand.pop(i)
            if c in DAMAGE:
                e["hp"] = max(0, e["hp"] - (1 if intangible else DAMAGE[c]))
            if c == "B":
                block += db
            if c == "S":
                hand += ["X"] * min(2, HAND_CAP - len(hand))
            if c == "M":
                if len(hand) < HAND_CAP:
                    hand.append("X")
            elif c == "E":
                ennui = 1
            else:
                disc.append(c)
            if trace:
                print(f"   play {c}  boss {e['hp']} en {en} blk {block} sadness {hand.count('X')}")
            if e["hp"] <= 0:
                return True, hp, r, peak
        disc += [c for c in hand if c not in "XF"]
        before = hp
        d, h, kind = intent(r, e["st"])
        for _ in range(h):
            block, lost = through(block, d)
            hp -= lost
        if kind == "Buff":
            e["st"] += 2
        if trace:
            print(f"r{r} took {before - hp} hp {hp}")
        if hp <= 0:
            return False, 0, r, peak
    return False, 0, 30, peak


def setting(name: str, value: float):
    """S.NAME=VALUE: a constant of fight/fool/sadness.py; anything else as modelfight.knob_setting takes it."""
    return mock.patch.object(sadness, name[2:], value) if name.startswith("S.") else knob_setting(name, value)


def run(args):
    arm, spec, n, seed = args
    with patched(arm, setting):
        res = played(fight, spec, n, seed)
    return arm, spec, mean(res, 0), mean(res, 1), mean(res, 2), mean(res, 3)


DECKS = [  # boss HP, Block, Strikes, Heavy, Defends, Sorrowful, Mourning, Ennui, Bane, energy (tuned until the planner wins 15-75%)
    (150, 6, 4, 1, 4, 2, 1, 1, 1, 3), (165, 6, 4, 1, 4, 2, 1, 1, 1, 3), (180, 6, 4, 1, 4, 2, 1, 1, 1, 3),
    (180, 6, 4, 1, 4, 2, 1, 0, 1, 3), (180, 7, 3, 2, 4, 3, 1, 1, 1, 3), (200, 7, 3, 2, 4, 3, 1, 1, 1, 3),
]

if __name__ == "__main__":
    logging.disable(logging.CRITICAL)
    if sys.argv[1:2] == ["trace"]:
        seed, dk, arm = trace_args(DECKS)
        with patched(arm, setting):
            print(fight(dk, random.Random(seed), trace=True))
    elif sys.argv[1:2] == ["flips"]:
        i, arms = int(sys.argv[2]), (sys.argv[3:] + ["on", "on"])[:2]
        for seed, res in flips(fight, DECKS[i], i, arms, lambda arm: patched(arm, setting), keep=4):
            if len({v[0] for v in res.values()}) > 1:
                print(seed, res)
    else:
        out, arms = table(run, DECKS, ["on"])
        for arm, rows in arms:
            print(f"{arm:28s}: won {mean(rows, 2):.3f}  hp left {mean(rows, 3):5.1f}  rounds {mean(rows, 4):4.1f}  peak Sadness {mean(rows, 5):4.1f}")
        for dk in DECKS:
            print(dk, "  ".join(f"{o[0]}: won {o[2]:.2f} sad {o[5]:3.1f}" for o in out if o[1] == dk))
