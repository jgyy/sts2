"""The Kin (act 1 boss: the Kin Priest and two Kin Followers) as a model fight, played by the bot's own planner.

    python3 scripts/kinfight.py [FIGHTS A DECK] [ARMS]   each arm on the same shuffles (default arms: off on)
    python3 scripts/kinfight.py trace [SEED] [DECK] [ARM]   one fight, play by play (DECK: its place in DECKS)
    python3 scripts/kinfight.py flips [DECK] [ARM ARM]     the fights one arm wins and the other loses

Arms: "on" as the bot plays (battle.MAIN_MINIONS: the Followers count as the main threat, so the planner's
lowest-health-first order takes them first); "off" without that (the Priest first, as before 2026-11-22
13:26); NAME=VALUE with a constant of battle/combat/triggers set to that.

The game side is the fight as the logs show it on Ascension 8 (2026-11-21/22, 19 fights, all the same):
the Priest 199 HP, the Followers 63 and 62, both Minions (they die with the Priest). Follower 0: Buff
(2 Strength), 5, 2x2, round and round; Follower 1 the same a turn later (5, 2x2, Buff). Priest: 8 and
Frail, 8 and Weak, 3x3, Buff (2 Strength); Strength adds to every hit. Frail takes a quarter off the
Block of our cards for a turn, Weak a quarter off our attacks. The deck is Strikes, Defends, two-cost
attacks, attacks that hit ALL enemies and dead cards on three energy. Like crabfight.py: trace a fight
and read it before believing a table.
"""

import logging
import multiprocessing
import os
import random
import sys
from unittest import mock

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from sts2bot import battle, combat, triggers  # noqa: E402
from tests.fake_game import card  # noqa: E402

HP = {"KIN_FOLLOWER_0": 63, "KIN_FOLLOWER_1": 62, "KIN_PRIEST_0": 199}


def intent(eid, r, st):
    """(damage a hit, hits, Strength it gains, debuff it puts on us, kind) on round `r`."""
    if eid == "KIN_PRIEST_0":
        return [(8 + st, 1, 0, "FRAIL", "Attack"), (8 + st, 1, 0, "WEAK", "Attack"), (3 + st, 3, 0, "", "Attack"), (0, 0, 2, "", "Buff")][(r - 1) % 4]
    cycle = [(0, 0, 2, "", "Buff"), (5 + st, 1, 0, "", "Attack"), (2 + st, 2, 0, "", "Attack")]
    return cycle[(r - 1 + (1 if eid == "KIN_FOLLOWER_1" else 0)) % 3]


CARDS = {  # kind: (id, name, type, cost, text, target) from the number on the card (damage or Block)
    "A": lambda n: ("STRIKE_R", "Strike", "Attack", 1, f"Deal {n} damage.", "AnyEnemy"),
    "H": lambda n: ("HEAVY", "Heavy Blow", "Attack", 2, f"Deal {n} damage.", "AnyEnemy"),
    "S": lambda n: ("SWEEP", "Sweeping Blow", "Attack", 1, f"Deal {n} damage to ALL enemies.", "AllEnemies"),
    "B": lambda n: ("DEFEND_R", "Defend", "Skill", 1, f"Gain {n} Block.", "Self"),
    "F": lambda n: ("CLUMSY", "Clumsy", "Curse", -1, "Unplayable.", "None"),
}
COST = {"A": 1, "H": 2, "S": 1, "B": 1, "F": -1}


def dealt(c, da, weak):
    d = {"A": da, "H": int(2.4 * da), "S": max(1, int(0.75 * da))}[c]
    return int(d * 0.75) if weak else d


def fight(spec, rng, maxhp=70, trace=False):
    da, db, natk, nblk, nheavy, nsweep, nfill = spec
    deck = ["A"] * natk + ["B"] * nblk + ["H"] * nheavy + ["S"] * nsweep + ["F"] * nfill
    rng.shuffle(deck)
    draw, disc = deck[:], []
    hp = maxhp
    E = {k: dict(hp=v, st=0) for k, v in HP.items()}
    order = []
    frail = weak = False
    for r in range(1, 31):
        hand = []
        for _ in range(5):
            if not draw:
                draw, disc = disc, []
                rng.shuffle(draw)
            if draw:
                hand.append(draw.pop())
        en, block = 3, 0
        b_blk = int(db * 0.75) if frail else db
        shown = lambda c, w=None: b_blk if c == "B" else 0 if c == "F" else dealt(c, da, weak if w is None else w)

        def state():
            enemies = []
            for eid, e in E.items():
                if e["hp"] <= 0:
                    continue
                d, h, _, debuff, kind = intent(eid, r, e["st"])
                intents = [{"type": "Attack", "label": f"{d}x{h}" if h > 1 else str(d)}] if d else [{"type": kind, "label": ""}]
                if debuff:
                    intents.append({"type": "Debuff", "label": ""})
                st = [{"id": "STRENGTH_POWER", "amount": e["st"]}] if e["st"] else []
                if eid != "KIN_PRIEST_0":
                    st.append({"id": "MINION_POWER", "amount": 1})
                enemies.append({"entity_id": eid, "hp": e["hp"], "max_hp": HP[eid], "block": 0, "status": st, "intents": intents})
            cards = []
            for i, c in enumerate(hand):
                cid, name, ctype, cost, text, target = CARDS[c](shown(c))  # (the numbers as the game shows them: Weak and Frail in)
                extra = {"can_play": False, "unplayable_reason": "HasUnplayableKeyword"} if c == "F" else {}
                cards.append(card(i, cid, name, ctype, cost, text, target, **extra))
            pile = lambda cs: [{"name": CARDS[c](0)[1], "cost": str(COST[c]), "description": CARDS[c](db if c == "B" else shown(c, False))[4]} for c in cs]
            status = ([{"id": "FRAIL_POWER", "amount": 1}] if frail else []) + ([{"id": "WEAK_POWER", "amount": 1}] if weak else [])
            return {
                "state_type": "boss", "run": {"act": 1, "floor": 17, "ascension": 8},
                "battle": {"round": r, "turn": "player", "is_play_phase": True, "enemies": enemies},
                "player": {"hp": hp, "max_hp": maxhp, "block": block, "energy": en, "hand": cards, "potions": [], "status": status,
                           "draw_pile": pile(draw), "discard_pile": pile(disc), "draw_pile_count": len(draw), "discard_pile_count": len(disc), "exhaust_pile_count": 0},
            }

        for _ in range(12):
            if E["KIN_PRIEST_0"]["hp"] <= 0:
                break
            act = combat.choose_card(state())
            if act is None:
                break
            i = act[1]["card_index"]
            c = hand[i]
            cost = COST[c]
            if c == "S":
                for eid, e in E.items():
                    if e["hp"] > 0:
                        e["hp"] = max(0, e["hp"] - dealt(c, da, weak))
                        if e["hp"] == 0:
                            order.append(eid[4])
            elif c in "AH":
                t = act[1].get("target")
                if t is None or E[t]["hp"] <= 0:
                    t = next(x for x in E if E[x]["hp"] > 0)
                E[t]["hp"] = max(0, E[t]["hp"] - dealt(c, da, weak))
                if E[t]["hp"] == 0:
                    order.append(t[4])
            elif c == "B":
                block += b_blk
            en -= cost
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


def setting(name: str, value: float):
    """Patch one constant of battle.py, combat.py or triggers.py (NAME=VALUE on the command line)."""
    mod = next(m for m in (battle, combat, triggers) if hasattr(m, name))
    return mock.patch.object(mod, name, value)


def arm_patch(arm):
    if "=" in arm:
        name, value = arm.split("=")
        return setting(name, float(value))
    if arm == "off":
        return mock.patch.object(battle, "MAIN_MINIONS", set())
    return mock.patch.object(battle, "MAIN_MINIONS", battle.MAIN_MINIONS)  # "on": as the bot plays


def run(args):
    arm, spec, n, seed = args
    logging.disable(logging.CRITICAL)
    wins = hpl = rounds = 0
    firsts = {}
    with arm_patch(arm):
        for j in range(n):
            ok, hp, r, order = fight(spec, random.Random(seed * 10007 + j))  # (a seed per fight: both arms play the same shuffles)
            wins += ok
            hpl += hp
            rounds += r
            firsts[order[:1] or "-"] = firsts.get(order[:1] or "-", 0) + 1
    return arm, spec, wins / n, hpl / n, rounds / n, firsts


DECKS = [  # damage, Block, Strikes, Defends, two-cost attacks, ALL-enemy attacks, dead cards (an act 1 boss deck: 20 to 30 a round)
    (11, 9, 5, 5, 2, 0, 2), (12, 10, 5, 5, 2, 0, 2), (11, 10, 5, 6, 3, 0, 1), (12, 9, 5, 5, 2, 1, 2), (11, 10, 4, 5, 2, 2, 2), (13, 10, 5, 5, 2, 1, 2),
]

if __name__ == "__main__":
    logging.disable(logging.CRITICAL)
    if sys.argv[1:2] == ["trace"]:
        seed = int(sys.argv[2]) if len(sys.argv) > 2 else 1
        dk = DECKS[int(sys.argv[3])] if len(sys.argv) > 3 else DECKS[1]
        with arm_patch(sys.argv[4] if len(sys.argv) > 4 else "on"):
            print(fight(dk, random.Random(seed), trace=True))
        sys.exit()
    if sys.argv[1:2] == ["flips"]:
        i = int(sys.argv[2]) if len(sys.argv) > 2 else 1
        a, b = sys.argv[3:5] if len(sys.argv) > 4 else ("off", "on")
        for j in range(200):
            seed = (900 + i) * 10007 + j
            res = {}
            for arm in (a, b):
                with arm_patch(arm):
                    res[arm] = fight(DECKS[i], random.Random(seed))
            if res[a][0] != res[b][0]:
                print(seed, res)
        sys.exit()
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 100
    arms = sys.argv[2:] or ["off", "on"]
    jobs = [(arm, dk, n, 900 + i) for arm in arms for i, dk in enumerate(DECKS)]
    with multiprocessing.Pool(min(12, os.cpu_count() or 2)) as pool:
        out = pool.map(run, jobs)
    for arm in arms:
        rows = [o for o in out if o[0] == arm]
        f = {}
        for o in rows:
            for k, v in o[5].items():
                f[k] = f.get(k, 0) + v
        print(f"{arm:20s}: won {sum(o[2] for o in rows) / len(rows):.3f}  hp left {sum(o[3] for o in rows) / len(rows):5.1f}  rounds {sum(o[4] for o in rows) / len(rows):4.1f}  first to die {dict(sorted(f.items()))}")
    for dk in DECKS:
        print(dk, "  ".join(f"{o[0]}: won {o[2]:.2f} hp {o[3]:4.1f}" for o in out if o[1] == dk))
