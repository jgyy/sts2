"""The act 3 knights (Flail, Spectral and Magi Knight) as a model fight, played by the bot's own planner.

    python3 scripts/knightfight.py [FIGHTS A DECK] [ARMS]          arms: "off" (without battle.KNOWN_THREAT), "on" (as the
                                                                  bot plays), NAME=VALUE ("on" with that constant); default off on
    python3 scripts/knightfight.py trace [SEED] [DECK] [off]       one fight, play by play (DECK: its place in DECKS)
    python3 scripts/knightfight.py flips [DECK]                    the fights one arm wins and the other loses

The fight is the Ascension 8 logs' (HP, `intent`): the Spectral Knight's Hex makes every card Ethereal while it lives.
"""

import collections
import logging
import os
import random
import sys
from contextlib import ExitStack
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from modelfight import attack, deal, firsts, flips, game_state, hand_cards, hit, mean, piles, played, setting, shuffled, status, table, target, turn
from sts2bot.fight import battle

HP = {"FLAIL_KNIGHT_0": 106, "SPECTRAL_KNIGHT_0": 95, "MAGI_KNIGHT_0": 87}


def intent(eid, r, st):
    """(damage a hit, hits, Block it gains, Strength it gains, kind) on round `r`."""
    if eid == "FLAIL_KNIGHT_0":
        return [(15 + st, 1, 0, 0, "Attack"), (9 + st, 2, 0, 0, "Attack"), (15 + st, 1, 0, 0, "Attack"), (0, 0, 0, 3, "Buff")][(r - 1) % 4]
    if eid == "SPECTRAL_KNIGHT_0":
        return (0, 0, 0, 0, "Debuff") if r == 1 else [(15, 1, 0, 0, "Attack"), (3, 3, 0, 0, "Attack"), (15, 1, 0, 0, "Attack")][(r - 2) % 3]
    return (6, 1, 6, 0, "Attack") if r == 1 else (0, 0, 0, 0, "Debuff") if r == 2 else [(10, 1, 0, 0, "Attack"), (0, 0, 6, 0, "Defend"), (35, 1, 0, 0, "Attack")][(r - 3) % 3]


CARDS = {  # kind: (id, name, type, cost, text, target)
    "A": lambda d: ("STRIKE_R", "Strike", "Attack", 1, f"Deal {d} damage.", "AnyEnemy"),
    "H": lambda d: ("HEAVY", "Heavy Blow", "Attack", 2, f"Deal {int(2.2 * d)} damage.", "AnyEnemy"),
    "V": lambda d: ("BASH", "Bash", "Attack", 2, "Deal 8 damage. Apply 2 Vulnerable.", "AnyEnemy"),
    "B": lambda b: ("DEFEND_R", "Defend", "Skill", 1, f"Gain {b} Block.", "Self"),
    "F": lambda _: ("CLUMSY", "Clumsy", "Curse", -1, "Unplayable.", "None"),
}


def fight(spec, rng, maxhp=75, trace=False):
    da, db = spec[:2]
    draw, disc, hp, order = shuffled(rng, "ABHVF", spec[2:]), [], maxhp, []
    E = {k: dict(hp=v, block=0, st=0, vuln=0) for k, v in HP.items()}
    face = lambda c: CARDS[c](db if c == "B" else da)
    hexed = False
    for r in range(1, 25):
        hand, en, block = deal(draw, disc, rng), 3, 0

        def state():
            enemies = []
            for eid, e in E.items():
                if e["hp"] > 0:
                    d, h, _, _, kind = intent(eid, r, e["st"])
                    intents = [attack(d, h) if d else {"type": kind, "label": ""}] + ([{"type": "Defend", "label": ""}] if eid == "MAGI_KNIGHT_0" and r == 1 else [])
                    st = status(("STRENGTH_POWER", e["st"]), ("VULNERABLE_POWER", e["vuln"]))
                    enemies.append({"entity_id": eid, "hp": e["hp"], "max_hp": HP[eid], "block": e["block"], "status": st, "intents": intents})
            hex_mark = {"keywords": [{"name": "Hexed", "description": "Add Ethereal to this card."}]} if hexed else {}
            return game_state("elite", {"act": 3, "floor": 44, "ascension": 8}, r, enemies, hand_cards(hand, face, **hex_mark), piles(face), draw, disc,
                              hp=hp, maxhp=maxhp, block=block, energy=en, status=[{"id": "HEX_POWER", "amount": 2}] if hexed else [])

        for i, c, act in turn(state, hand, lambda: all(e["hp"] <= 0 for e in E.values())):
            if c in "AHV":
                e = E[t := target(act, E)]
                d = {"A": da, "H": int(2.2 * da), "V": 8}[c]
                hit(e, int(d * 1.5) if e["vuln"] else d)
                if c == "V" and e["hp"] > 0:
                    e["vuln"] += 2
                if e["hp"] == 0:
                    order.append(t[:1])
            block += db if c == "B" else 0
            en -= CARDS[c](da)[3]
            disc.append(hand.pop(i))
            if trace:
                print("   play", c, act[1].get("target"), {k[:1]: v["hp"] for k, v in E.items()})
        if all(e["hp"] <= 0 for e in E.values()):
            return True, hp, r, "".join(order)
        lost = max(0, sum(d * h for d, h, *_ in (intent(eid, r, e["st"]) for eid, e in E.items() if e["hp"] > 0)) - block)
        hp -= lost
        if trace:
            print(f"r{r} took {lost} hp {hp} | " + " ".join(f"{k[:1]} {v['hp']}" for k, v in E.items()))
        disc += [] if hexed else hand  # Ethereal: what is left in the hand is Exhausted
        if hp <= 0:
            return False, 0, r, "".join(order)
        for eid, e in E.items():
            e["block"] = 0
            if e["hp"] > 0:
                _, _, gain, st, _ = intent(eid, r, e["st"])
                e["block"] += gain
                e["st"] += st
                e["vuln"] = max(0, e["vuln"] - 1)
        hexed = E["SPECTRAL_KNIGHT_0"]["hp"] > 0
    return False, 0, 24, "".join(order)


def arm_patch(arm):
    stack = ExitStack()
    if "=" in arm:
        name, value = arm.split("=")
        stack.enter_context(setting(name, float(value)))
    stack.enter_context(mock.patch.dict(battle.KNOWN_THREAT, {} if arm == "off" else dict(battle.KNOWN_THREAT), clear=True))
    return stack


def run(args):
    arm, spec, n, seed = args
    with arm_patch(arm):
        res = played(fight, spec, n, seed)
    return arm, spec, mean(res, 0), mean(res, 1), mean(res, 2), collections.Counter(o[3][:1] or "-" for o in res)


DECKS = [  # damage, Block, Strikes, Defends, heavy, Bash, dead (an act 3 deck: 35 to 55 a round against these three)
    (16, 12, 6, 5, 2, 1, 1), (18, 12, 6, 5, 2, 1, 1), (16, 15, 5, 6, 2, 1, 1), (20, 13, 6, 5, 2, 1, 2), (18, 15, 5, 5, 3, 1, 1), (22, 12, 6, 5, 2, 1, 1),
]

if __name__ == "__main__":
    logging.disable(logging.CRITICAL)
    if sys.argv[1:2] == ["trace"]:
        seed = int(sys.argv[2]) if len(sys.argv) > 2 else 1
        dk = DECKS[int(sys.argv[3])] if len(sys.argv) > 3 and sys.argv[3].isdigit() else DECKS[1]
        with arm_patch("off" if "off" in sys.argv[3:] else "on"):
            print(fight(dk, random.Random(seed), trace=True))
    elif sys.argv[1:2] == ["flips"]:
        i = int(sys.argv[2]) if len(sys.argv) > 2 else 5
        for seed, res in flips(fight, DECKS[i], i, ("off", "on"), arm_patch):
            if res["off"][0] != res["on"][0]:
                print(seed, res)
    else:
        out, arms = table(run, DECKS, ["off", "on"])
        for arm, rows in arms:
            print(f"{arm:20s}: won {mean(rows, 2):.3f}  hp left {mean(rows, 3):5.1f}  rounds {mean(rows, 4):4.1f}  first to die {firsts(rows, 5)}")
        for dk in DECKS:
            print(dk, "  ".join(f"{o[0]}: won {o[2]:.2f} hp {o[3]:4.1f}" for o in out if o[1] == dk))
