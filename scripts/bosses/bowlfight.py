"""Bowlbug Rock and Bowlbug Nectar (an act 2 hallway fight) as a model, played by the bot's own planner.

    python3 scripts/bosses/bowlfight.py [FIGHTS A DECK] [ARMS]   each arm on the same shuffles (default arms: off on)
    python3 scripts/bosses/bowlfight.py trace [SEED] [DECK] [ARM]   one fight, play by play (DECK: its place in DECKS)
    python3 scripts/bosses/bowlfight.py flips [DECK] [ARM ARM]     the fights where the arms differ most in HP lost

Arms: "on" as the bot plays (battle.KNOWN_THREAT as it stands); "off" without BOWLBUG_NECTAR in it (as before
2026-11-22 21:05); "egg" adds a Bowlbug Egg (the second most common pairing); NAME=VALUE sets a constant of
battle/combat/triggers; "nectar=N" puts the Nectar in KNOWN_THREAT at N. Arms join with "+".

The game side is the fight as 30 logged fights show it (Ascension 9, 2026-11-22 17:35-20:58): the Rock 46-49 HP,
Imbalanced (an attack of his that our Block stops whole leaves him Stunned his next turn), 16 every turn he is not
Stunned, and he moves first; the Nectar 36-39 HP: 3, then Buff (16 Strength for good), then 19 every turn. The Egg
24 HP: 8 and 8 Block on itself every turn. An ordinary fight, so the planner gets the bot's per-fight threat
average (`_threat`: what each enemy has aimed at us a round so far, this round included). The measure is HP lost.
"""

import logging
import multiprocessing
import os
import random
import sys
from contextlib import ExitStack
from unittest import mock

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))

from sts2bot.fight import battle, combat, triggers
from tests.fake_game import card

CARDS = {  # kind: (id, name, type, cost, text, target) from the number on the card (damage or Block)
    "A": lambda n: ("STRIKE_R", "Strike", "Attack", 1, f"Deal {n} damage.", "AnyEnemy"),
    "H": lambda n: ("HEAVY", "Heavy Blow", "Attack", 2, f"Deal {n} damage.", "AnyEnemy"),
    "S": lambda n: ("SWEEP", "Sweeping Blow", "Attack", 1, f"Deal {n} damage to ALL enemies.", "AllEnemies"),
    "B": lambda n: ("DEFEND_R", "Defend", "Skill", 1, f"Gain {n} Block.", "Self"),
    "F": lambda n: ("CLUMSY", "Clumsy", "Curse", -1, "Unplayable.", "None"),
}
COST = {"A": 1, "H": 2, "S": 1, "B": 1, "F": -1}


def dealt(c, da):
    return {"A": da, "H": int(2.4 * da), "S": max(1, int(0.75 * da))}[c]


def intent(eid, e, r):
    """(damage, Strength gained, Block gained, kind) for this enemy on round `r`."""
    if eid.startswith("BOWLBUG_ROCK"):
        return (0, 0, 0, "Stun") if e["stunned"] else (16, 0, 0, "Attack")
    if eid.startswith("BOWLBUG_NECTAR"):
        return (3 + e["st"], 0, 0, "Attack") if r != 2 else (0, 16, 0, "Buff")
    return (8, 0, 8, "Attack")  # the Egg: 8 and 8 Block


def fight(spec, rng, egg=False, maxhp=75, start=60, trace=False):
    da, db, natk, nblk, nheavy, nsweep, nfill = spec
    deck = ["A"] * natk + ["B"] * nblk + ["H"] * nheavy + ["S"] * nsweep + ["F"] * nfill
    rng.shuffle(deck)
    draw, disc = deck[:], []
    hp = start
    E = {"BOWLBUG_ROCK_0": dict(hp=rng.randint(46, 49), mx=49, st=0, blk=0, stunned=False),
         "BOWLBUG_NECTAR_0": dict(hp=rng.randint(36, 39), mx=39, st=0, blk=0, stunned=False)}
    if egg:
        E["BOWLBUG_EGG_0"] = dict(hp=rng.randint(23, 24), mx=24, st=0, blk=0, stunned=False)
    seen = {k: [0, 0] for k in E}
    order = []
    for r in range(1, 21):
        hand = []
        for _ in range(5):
            if not draw:
                draw, disc = disc, []
                rng.shuffle(draw)
            if draw:
                hand.append(draw.pop())
        en, block = 3, 0
        for eid, e in E.items():
            if e["hp"] > 0:
                seen[eid][0] += intent(eid, e, r)[0]
                seen[eid][1] += 1
        shown = lambda c: db if c == "B" else 0 if c == "F" else dealt(c, da)

        def state():
            enemies = []
            for eid, e in E.items():
                if e["hp"] <= 0:
                    continue
                d, _, _, kind = intent(eid, e, r)
                intents = [{"type": "Attack", "label": str(d)}] if d else [{"type": kind, "label": ""}]
                if eid.startswith("BOWLBUG_EGG"):
                    intents.append({"type": "Defend", "label": ""})
                st = [{"id": "STRENGTH_POWER", "amount": e["st"]}] if e["st"] else []
                if eid.startswith("BOWLBUG_ROCK"):
                    st.append({"id": "IMBALANCED_POWER", "amount": 1})
                enemies.append({"entity_id": eid, "hp": e["hp"], "max_hp": e["mx"], "block": e["blk"], "status": st, "intents": intents})
            cards = []
            for i, c in enumerate(hand):
                cid, name, ctype, cost, text, target = CARDS[c](shown(c))
                extra = {"can_play": False, "unplayable_reason": "HasUnplayableKeyword"} if c == "F" else {}
                cards.append(card(i, cid, name, ctype, cost, text, target, **extra))
            pile = lambda cs: [{"name": CARDS[c](0)[1], "cost": str(COST[c]), "description": CARDS[c](shown(c))[4]} for c in cs]
            return {
                "state_type": "monster", "run": {"act": 2, "floor": 25, "ascension": 9},
                "battle": {"round": r, "turn": "player", "is_play_phase": True, "enemies": enemies},
                "player": {"hp": hp, "max_hp": maxhp, "block": block, "energy": en, "hand": cards, "potions": [], "status": [],
                           "draw_pile": pile(draw), "discard_pile": pile(disc), "draw_pile_count": len(draw), "discard_pile_count": len(disc), "exhaust_pile_count": 0},
                "_threat": {k: t / max(1, n) for k, (t, n) in seen.items()},
            }

        def hit(eid, amount):
            e = E[eid]
            through = max(0, amount - e["blk"])
            e["blk"] = max(0, e["blk"] - amount)
            e["hp"] = max(0, e["hp"] - through)
            if e["hp"] == 0 and eid[8] not in order:
                order.append(eid[8])

        for _ in range(12):
            if all(e["hp"] <= 0 for e in E.values()):
                break
            act = combat.choose_card(state())
            if act is None:
                break
            i = act[1]["card_index"]
            c = hand[i]
            if c == "S":
                for eid, e in E.items():
                    if e["hp"] > 0:
                        hit(eid, dealt(c, da))
            elif c in "AH":
                t = act[1].get("target")
                if t is None or E[t]["hp"] <= 0:
                    t = next(x for x in E if E[x]["hp"] > 0)
                hit(t, dealt(c, da))
            elif c == "B":
                block += db
            en -= COST[c]
            disc.append(hand.pop(i))
            if trace:
                print("   play", c, act[1].get("target", "")[8:], {k[8:12]: v["hp"] for k, v in E.items()})
        if all(e["hp"] <= 0 for e in E.values()):
            return start - hp, r, "".join(order)
        lost = 0
        for eid, e in E.items():  # (in order: the Rock first)
            if e["hp"] <= 0:
                continue
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


def setting(name: str, value: float):
    """Patch one constant of battle.py, combat.py or triggers.py (NAME=VALUE on the command line)."""
    mod = next(m for m in (battle, combat, triggers) if hasattr(m, name))
    return mock.patch.object(mod, name, value)


def arm_patch(arm):
    """(patches to enter, whether the Egg is in the fight) for an arm like "off", "on+egg" or "nectar=12+egg"."""
    stack, egg = ExitStack(), False
    known = {k: v for k, v in battle.KNOWN_THREAT.items()}
    for part in arm.split("+"):
        if part == "egg":
            egg = True
        elif part == "off":
            known.pop("BOWLBUG_NECTAR", None)
        elif part.startswith("nectar="):
            known["BOWLBUG_NECTAR"] = float(part.split("=")[1])
        elif "=" in part:
            name, value = part.split("=")
            stack.enter_context(setting(name, float(value)))
    stack.enter_context(mock.patch.object(battle, "KNOWN_THREAT", known))
    return stack, egg


def run(args):
    arm, spec, n, seed = args
    logging.disable(logging.CRITICAL)
    lost = rounds = deaths = 0
    firsts = {}
    stack, egg = arm_patch(arm)
    with stack:
        for j in range(n):
            loss, r, order = fight(spec, random.Random(seed * 10007 + j), egg=egg)  # (a seed per fight: both arms play the same shuffles)
            lost += loss
            rounds += r
            deaths += order.endswith("x")
            firsts[order[:1] or "-"] = firsts.get(order[:1] or "-", 0) + 1
    return arm, spec, lost / n, rounds / n, deaths / n, firsts


DECKS = [  # damage, Block, Strikes, Defends, two-cost attacks, ALL-enemy attacks, dead cards (an act 2 deck: 20 to 30 a round)
    (8, 7, 5, 5, 2, 0, 3), (9, 8, 5, 5, 2, 0, 2), (9, 8, 5, 6, 3, 0, 2), (10, 8, 5, 5, 2, 1, 3), (9, 9, 4, 5, 2, 2, 2), (11, 9, 5, 5, 2, 1, 2),
]

if __name__ == "__main__":
    logging.disable(logging.CRITICAL)
    if sys.argv[1:2] == ["trace"]:
        seed = int(sys.argv[2]) if len(sys.argv) > 2 else 1
        dk = DECKS[int(sys.argv[3])] if len(sys.argv) > 3 else DECKS[1]
        stack, egg = arm_patch(sys.argv[4] if len(sys.argv) > 4 else "on")
        with stack:
            print(fight(dk, random.Random(seed), egg=egg, trace=True))
        sys.exit()
    if sys.argv[1:2] == ["flips"]:
        i = int(sys.argv[2]) if len(sys.argv) > 2 else 1
        a, b = sys.argv[3:5] if len(sys.argv) > 4 else ("off", "on")
        rows = []
        for j in range(200):
            seed = (900 + i) * 10007 + j
            res = {}
            for arm in (a, b):
                stack, egg = arm_patch(arm)
                with stack:
                    res[arm] = fight(DECKS[i], random.Random(seed), egg=egg)
            rows.append((res[a][0] - res[b][0], seed, res))
        for d, seed, res in sorted(rows, key=lambda x: -abs(x[0]))[:12]:
            print(seed, d, res)
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
        print(f"{arm:20s}: hp lost {sum(o[2] for o in rows) / len(rows):5.2f}  rounds {sum(o[3] for o in rows) / len(rows):4.1f}  died {sum(o[4] for o in rows) / len(rows):.3f}  first to die {dict(sorted(f.items()))}")
    for dk in DECKS:
        print(dk, "  ".join(f"{o[0]}: lost {o[2]:5.2f}" for o in out if o[1] == dk))
