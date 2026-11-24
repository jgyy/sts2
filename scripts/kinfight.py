"""The Kin (act 1 boss: the Kin Priest and two Kin Followers) as a model fight, played by the bot's own planner.

    python3 scripts/kinfight.py [FIGHTS A DECK] [ARMS]        each arm on the same shuffles (default arms: off on)
    python3 scripts/kinfight.py trace [SEED] [DECK] [ARM]     one fight, play by play (DECK: its place in DECKS)
    python3 scripts/kinfight.py flips [DECK] [ARM ARM]        the fights one arm wins and the other loses

Arms: "on" as the bot plays (battle.MAIN_MINIONS: the Followers first), "off" without it (the Priest first), NAME=VALUE
a constant of battle/combat/triggers. The fight is the Ascension 8 logs' (HP, `intent`); Frail and Weak take a quarter off.
"""

import collections
import logging
import os
import random
import sys
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from modelfight import attack, deal, firsts, flips, game_state, hand_cards, mean, piles, played, setting, shuffled, status, table, target, trace_args, turn
from sts2bot.fight import battle

HP = {"KIN_FOLLOWER_0": 63, "KIN_FOLLOWER_1": 62, "KIN_PRIEST_0": 199}


def intent(eid, r, st):
    """(damage a hit, hits, Strength it gains, debuff it puts on us, kind) on round `r`."""
    if eid == "KIN_PRIEST_0":
        return [(8 + st, 1, 0, "FRAIL", "Attack"), (8 + st, 1, 0, "WEAK", "Attack"), (3 + st, 3, 0, "", "Attack"), (0, 0, 2, "", "Buff")][(r - 1) % 4]
    return [(0, 0, 2, "", "Buff"), (5 + st, 1, 0, "", "Attack"), (2 + st, 2, 0, "", "Attack")][(r - 1 + (eid == "KIN_FOLLOWER_1")) % 3]


CARDS = {  # kind: (id, name, type, cost, text, target) from the number on the card (damage or Block)
    "A": lambda n: ("STRIKE_R", "Strike", "Attack", 1, f"Deal {n} damage.", "AnyEnemy"),
    "H": lambda n: ("HEAVY", "Heavy Blow", "Attack", 2, f"Deal {n} damage.", "AnyEnemy"),
    "S": lambda n: ("SWEEP", "Sweeping Blow", "Attack", 1, f"Deal {n} damage to ALL enemies.", "AllEnemies"),
    "B": lambda n: ("DEFEND_R", "Defend", "Skill", 1, f"Gain {n} Block.", "Self"),
    "F": lambda n: ("CLUMSY", "Clumsy", "Curse", -1, "Unplayable.", "None"),
}


def dealt(c, da, weak):
    d = {"A": da, "H": int(2.4 * da), "S": max(1, int(0.75 * da))}[c]
    return int(d * 0.75) if weak else d


def fight(spec, rng, maxhp=70, trace=False):
    da, db = spec[:2]
    draw, disc, hp, order = shuffled(rng, "ABHSF", spec[2:]), [], maxhp, []
    E = {k: dict(hp=v, st=0) for k, v in HP.items()}
    frail = weak = False
    for r in range(1, 31):
        hand, en, block = deal(draw, disc, rng), 3, 0
        b_blk = int(db * 0.75) if frail else db
        shown = lambda c, w=None: b_blk if c == "B" else 0 if c == "F" else dealt(c, da, weak if w is None else w)

        def state():
            enemies = []
            for eid, e in E.items():
                if e["hp"] > 0:
                    d, h, _, debuff, kind = intent(eid, r, e["st"])
                    intents = [attack(d, h) if d else {"type": kind, "label": ""}] + ([{"type": "Debuff", "label": ""}] if debuff else [])
                    st = status(("STRENGTH_POWER", e["st"]), ("MINION_POWER", int(eid != "KIN_PRIEST_0")))
                    enemies.append({"entity_id": eid, "hp": e["hp"], "max_hp": HP[eid], "block": 0, "status": st, "intents": intents})
            cards = hand_cards(hand, lambda c: CARDS[c](shown(c)))  # (the numbers as the game shows them: Weak and Frail in)
            return game_state("boss", {"act": 1, "floor": 17, "ascension": 8}, r, enemies, cards, piles(lambda c: CARDS[c](db if c == "B" else shown(c, False))), draw, disc,
                              hp=hp, maxhp=maxhp, block=block, energy=en, status=status(("FRAIL_POWER", int(frail)), ("WEAK_POWER", int(weak))))

        for i, c, act in turn(state, hand, lambda: E["KIN_PRIEST_0"]["hp"] <= 0):
            for t in [x for x in E if E[x]["hp"] > 0] if c == "S" else [target(act, E)] if c in "AH" else []:
                E[t]["hp"] = max(0, E[t]["hp"] - dealt(c, da, weak))
                if E[t]["hp"] == 0:
                    order.append(t[4])
            block += b_blk if c == "B" else 0
            en -= CARDS[c](0)[3]
            disc.append(hand.pop(i))
            if trace:
                print("   play", c, act[1].get("target"), {k[4:]: v["hp"] for k, v in E.items()})
        if E["KIN_PRIEST_0"]["hp"] <= 0:  # (the Followers are Minions: they go with him)
            return True, hp, r, "".join(order)
        inc, frail, weak = 0, False, False
        for eid, e in E.items():
            if e["hp"] > 0:
                d, h, gain, debuff, _ = intent(eid, r, e["st"])
                inc += d * h
                e["st"] += gain
                frail, weak = frail or debuff == "FRAIL", weak or debuff == "WEAK"
        lost = max(0, inc - block)
        hp -= lost
        if trace:
            print(f"r{r} took {lost} hp {hp} | " + " ".join(f"{k[4:]} {v['hp']}" for k, v in E.items()))
        disc += hand
        if hp <= 0:
            return False, 0, r, "".join(order)
    return False, 0, 30, "".join(order)


def arm_patch(arm):
    if "=" in arm:
        name, value = arm.split("=")
        return setting(name, float(value))
    return mock.patch.object(battle, "MAIN_MINIONS", set() if arm == "off" else battle.MAIN_MINIONS)  # "on": as the bot plays


def run(args):
    arm, spec, n, seed = args
    with arm_patch(arm):
        res = played(fight, spec, n, seed)
    return arm, spec, mean(res, 0), mean(res, 1), mean(res, 2), collections.Counter(o[3][:1] or "-" for o in res)


DECKS = [  # damage, Block, Strikes, Defends, two-cost attacks, ALL-enemy attacks, dead cards (an act 1 boss deck: 20 to 30 a round)
    (11, 9, 5, 5, 2, 0, 2), (12, 10, 5, 5, 2, 0, 2), (11, 10, 5, 6, 3, 0, 1), (12, 9, 5, 5, 2, 1, 2), (11, 10, 4, 5, 2, 2, 2), (13, 10, 5, 5, 2, 1, 2),
]

if __name__ == "__main__":
    logging.disable(logging.CRITICAL)
    if sys.argv[1:2] == ["trace"]:
        seed, dk, arm = trace_args(DECKS)
        with arm_patch(arm):
            print(fight(dk, random.Random(seed), trace=True))
    elif sys.argv[1:2] == ["flips"]:
        i = int(sys.argv[2]) if len(sys.argv) > 2 else 1
        a, b = sys.argv[3:5] if len(sys.argv) > 4 else ("off", "on")
        for seed, res in flips(fight, DECKS[i], i, (a, b), arm_patch):
            if res[a][0] != res[b][0]:
                print(seed, res)
    else:
        out, arms = table(run, DECKS, ["off", "on"])
        for arm, rows in arms:
            print(f"{arm:20s}: won {mean(rows, 2):.3f}  hp left {mean(rows, 3):5.1f}  rounds {mean(rows, 4):4.1f}  first to die {firsts(rows, 5)}")
        for dk in DECKS:
            print(dk, "  ".join(f"{o[0]}: won {o[2]:.2f} hp {o[3]:4.1f}" for o in out if o[1] == dk))
