"""Bowlbug Rock and Bowlbug Nectar (an act 2 hallway fight) as a model, played by the bot's own planner.

    python3 scripts/bosses/bowlfight.py [FIGHTS A DECK] [ARMS]       each arm on the same shuffles (default arms: off on)
    python3 scripts/bosses/bowlfight.py trace [SEED] [DECK] [ARM]    one fight, play by play (DECK: its place in DECKS)
    python3 scripts/bosses/bowlfight.py flips [DECK] [ARM ARM]       the fights where the arms differ most in HP lost

Arms ("+" joins them): "on" as the bot plays (battle.KNOWN_THREAT as it stands), "off" without BOWLBUG_NECTAR in it,
"egg" adds a Bowlbug Egg, "nectar=N" puts the Nectar in KNOWN_THREAT at N, NAME=VALUE a constant of battle/combat/triggers.
The fight is the Ascension 9 logs' (`intent`); the planner gets the bot's threat average (`_threat`). The measure is HP lost.
"""

import collections
import logging
import os
import random
import sys
from contextlib import ExitStack
from unittest import mock

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from modelfight import attack, deal, firsts, flips, game_state, hand_cards, hit, mean, piles, played, setting, shuffled, status, table, target, trace_args, turn
from sts2bot.fight import battle

CARDS = {  # kind: (id, name, type, cost, text, target) from the number on the card (damage or Block)
    "A": lambda n: ("STRIKE_R", "Strike", "Attack", 1, f"Deal {n} damage.", "AnyEnemy"),
    "H": lambda n: ("HEAVY", "Heavy Blow", "Attack", 2, f"Deal {n} damage.", "AnyEnemy"),
    "S": lambda n: ("SWEEP", "Sweeping Blow", "Attack", 1, f"Deal {n} damage to ALL enemies.", "AllEnemies"),
    "B": lambda n: ("DEFEND_R", "Defend", "Skill", 1, f"Gain {n} Block.", "Self"),
    "F": lambda n: ("CLUMSY", "Clumsy", "Curse", -1, "Unplayable.", "None"),
}


def dealt(c, da):
    return {"A": da, "H": int(2.4 * da), "S": max(1, int(0.75 * da))}[c]


def intent(eid, e, r):
    """(damage, Strength gained, Block gained, kind) for this enemy on round `r`."""
    if eid.startswith("BOWLBUG_ROCK"):  # Imbalanced: Stunned for a turn when our Block stops his attack whole
        return (0, 0, 0, "Stun") if e["stunned"] else (16, 0, 0, "Attack")
    if eid.startswith("BOWLBUG_NECTAR"):
        return (3 + e["st"], 0, 0, "Attack") if r != 2 else (0, 16, 0, "Buff")
    return (8, 0, 8, "Attack")  # the Egg: 8 and 8 Block


def fight(spec, rng, egg=False, maxhp=75, start=60, trace=False):
    da, db = spec[:2]
    draw, disc, hp, order = shuffled(rng, "ABHSF", spec[2:]), [], start, []
    E = {"BOWLBUG_ROCK_0": dict(hp=rng.randint(46, 49), mx=49, st=0, blk=0, stunned=False),
         "BOWLBUG_NECTAR_0": dict(hp=rng.randint(36, 39), mx=39, st=0, blk=0, stunned=False)}
    if egg:
        E["BOWLBUG_EGG_0"] = dict(hp=rng.randint(23, 24), mx=24, st=0, blk=0, stunned=False)
    seen = {k: [0, 0] for k in E}
    face = lambda c: CARDS[c](db if c == "B" else 0 if c == "F" else dealt(c, da))
    for r in range(1, 21):
        hand, en, block = deal(draw, disc, rng), 3, 0
        for eid, e in E.items():
            if e["hp"] > 0:
                seen[eid][0] += intent(eid, e, r)[0]
                seen[eid][1] += 1

        def state():
            enemies = []
            for eid, e in E.items():
                if e["hp"] > 0:
                    d, _, _, kind = intent(eid, e, r)
                    intents = [attack(d) if d else {"type": kind, "label": ""}] + ([{"type": "Defend", "label": ""}] if eid.startswith("BOWLBUG_EGG") else [])
                    st = status(("STRENGTH_POWER", e["st"]), ("IMBALANCED_POWER", int(eid.startswith("BOWLBUG_ROCK"))))
                    enemies.append({"entity_id": eid, "hp": e["hp"], "max_hp": e["mx"], "block": e["blk"], "status": st, "intents": intents})
            return game_state("monster", {"act": 2, "floor": 25, "ascension": 9}, r, enemies, hand_cards(hand, face), piles(face), draw, disc, hp=hp, maxhp=maxhp,
                              block=block, energy=en, status=[], _threat={k: t / max(1, n) for k, (t, n) in seen.items()})

        for i, c, act in turn(state, hand, lambda: all(e["hp"] <= 0 for e in E.values())):
            for t in [x for x in E if E[x]["hp"] > 0] if c == "S" else [target(act, E)] if c in "AH" else []:
                hit(E[t], dealt(c, da), "blk")
                if E[t]["hp"] == 0 and t[8] not in order:
                    order.append(t[8])
            block += db if c == "B" else 0
            en -= CARDS[c](0)[3]
            disc.append(hand.pop(i))
            if trace:
                print("   play", c, act[1].get("target", "")[8:], {k[8:12]: v["hp"] for k, v in E.items()})
        if all(e["hp"] <= 0 for e in E.values()):
            return start - hp, r, "".join(order)
        lost = 0
        for eid, e in E.items():  # (in order: the Rock first)
            if e["hp"] > 0:
                e["blk"] = 0
                d, gain, blk, kind = intent(eid, e, r)
                soaked = min(block, d)
                block -= soaked
                lost += d - soaked
                if eid.startswith("BOWLBUG_ROCK"):
                    e["stunned"] = kind == "Attack" and d > 0 and soaked >= d
                e["st"] += gain
                e["blk"] += blk
        hp -= lost
        disc += hand
        if trace:
            print(f"r{r} took {lost} hp {hp} | " + " ".join(f"{k[8:12]} {v['hp']}" for k, v in E.items()))
        if hp <= 0:
            return start, r, "".join(order) + "x"
    return start - hp, 20, "".join(order)


def arm_patch(arm):
    """The patches of an arm like "off", "on+egg" or "nectar=12+egg" (the Egg itself: `eggs`)."""
    stack, known = ExitStack(), dict(battle.KNOWN_THREAT)
    for part in arm.split("+"):
        if part == "off":
            known.pop("BOWLBUG_NECTAR", None)
        elif part.startswith("nectar="):
            known["BOWLBUG_NECTAR"] = float(part.split("=")[1])
        elif "=" in part:
            name, value = part.split("=")
            stack.enter_context(setting(name, float(value)))
    stack.enter_context(mock.patch.object(battle, "KNOWN_THREAT", known))
    return stack


def eggs(arm):
    return {"egg": "egg" in arm.split("+")}


def run(args):
    arm, spec, n, seed = args
    with arm_patch(arm):
        res = played(fight, spec, n, seed, **eggs(arm))
    return arm, spec, mean(res, 0), mean(res, 1), sum(o[2].endswith("x") for o in res) / n, collections.Counter(o[2][:1] or "-" for o in res)


DECKS = [  # damage, Block, Strikes, Defends, two-cost attacks, ALL-enemy attacks, dead cards (an act 2 deck: 20 to 30 a round)
    (8, 7, 5, 5, 2, 0, 3), (9, 8, 5, 5, 2, 0, 2), (9, 8, 5, 6, 3, 0, 2), (10, 8, 5, 5, 2, 1, 3), (9, 9, 4, 5, 2, 2, 2), (11, 9, 5, 5, 2, 1, 2),
]

if __name__ == "__main__":
    logging.disable(logging.CRITICAL)
    if sys.argv[1:2] == ["trace"]:
        seed, dk, arm = trace_args(DECKS)
        with arm_patch(arm):
            print(fight(dk, random.Random(seed), **eggs(arm), trace=True))
    elif sys.argv[1:2] == ["flips"]:
        i = int(sys.argv[2]) if len(sys.argv) > 2 else 1
        a, b = sys.argv[3:5] if len(sys.argv) > 4 else ("off", "on")
        rows = [(res[a][0] - res[b][0], seed, res) for seed, res in flips(fight, DECKS[i], i, (a, b), arm_patch, kw=eggs)]
        for d, seed, res in sorted(rows, key=lambda x: -abs(x[0]))[:12]:
            print(seed, d, res)
    else:
        out, arms = table(run, DECKS, ["off", "on"])
        for arm, rows in arms:
            print(f"{arm:20s}: hp lost {mean(rows, 2):5.2f}  rounds {mean(rows, 3):4.1f}  died {mean(rows, 4):.3f}  first to die {firsts(rows, 5)}")
        for dk in DECKS:
            print(dk, "  ".join(f"{o[0]}: lost {o[2]:5.2f}" for o in out if o[1] == dk))
