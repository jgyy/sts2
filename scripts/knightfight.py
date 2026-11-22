"""The act 3 knights (Flail, Spectral and Magi Knight) as a model fight, played by the bot's own planner.

    python3 scripts/knightfight.py [FIGHTS A DECK]      the planner with battle.KNOWN_THREAT and without it
    python3 scripts/knightfight.py trace [SEED] [DECK] [off]   one fight, play by play (DECK: its place in DECKS)
    python3 scripts/knightfight.py flips [DECK]         the fights one arm wins and the other loses

The game side is the elite as the logs show it on Ascension 8 (2026-11-21/22, nine fights): 106, 95 and 87 HP.
Flail Knight 15, 9x2, 15, Buff (3 Strength), round and round. Spectral Knight Debuff on round 1 ("ALL your
cards are Ethereal" while it lives: whatever is left in the hand is Exhausted when the turn ends), then 15,
3x3, 15. Magi Knight 6 and 6 Block, Debuff (every card Downgraded while it lives: nothing here is upgraded,
so nothing changes), then 10, 6 Block, 35. The deck is Strikes, Defends, two-cost attacks, a Bash (8 and 2
Vulnerable) and dead cards on three energy; both arms play the same shuffles. Like crabfight.py: trace a
fight and read it before believing a table.
"""

import logging
import multiprocessing
import os
import random
import sys
from unittest import mock

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from sts2bot.fight import battle, combat, triggers  # noqa: E402
from tests.fake_game import card  # noqa: E402

HP = {"FLAIL_KNIGHT_0": 106, "SPECTRAL_KNIGHT_0": 95, "MAGI_KNIGHT_0": 87}


def intent(eid, r, st):
    """(damage a hit, hits, Block it gains, Strength it gains, kind) on round `r`."""
    if eid == "FLAIL_KNIGHT_0":
        return [(15 + st, 1, 0, 0, "Attack"), (9 + st, 2, 0, 0, "Attack"), (15 + st, 1, 0, 0, "Attack"), (0, 0, 0, 3, "Buff")][(r - 1) % 4]
    if eid == "SPECTRAL_KNIGHT_0":
        return (0, 0, 0, 0, "Debuff") if r == 1 else [(15, 1, 0, 0, "Attack"), (3, 3, 0, 0, "Attack"), (15, 1, 0, 0, "Attack")][(r - 2) % 3]
    if r == 1:
        return (6, 1, 6, 0, "Attack")
    if r == 2:
        return (0, 0, 0, 0, "Debuff")
    return [(10, 1, 0, 0, "Attack"), (0, 0, 6, 0, "Defend"), (35, 1, 0, 0, "Attack")][(r - 3) % 3]


CARDS = {  # kind: (id, name, type, cost, text, target)
    "A": lambda d: ("STRIKE_R", "Strike", "Attack", 1, f"Deal {d} damage.", "AnyEnemy"),
    "H": lambda d: ("HEAVY", "Heavy Blow", "Attack", 2, f"Deal {int(2.2 * d)} damage.", "AnyEnemy"),
    "V": lambda d: ("BASH", "Bash", "Attack", 2, "Deal 8 damage. Apply 2 Vulnerable.", "AnyEnemy"),
    "B": lambda b: ("DEFEND_R", "Defend", "Skill", 1, f"Gain {b} Block.", "Self"),
    "F": lambda _: ("CLUMSY", "Clumsy", "Curse", -1, "Unplayable.", "None"),
}


def fight(spec, rng, maxhp=75, trace=False):
    da, db, natk, nblk, nheavy, nbash, nfill = spec
    deck = ["A"] * natk + ["B"] * nblk + ["H"] * nheavy + ["V"] * nbash + ["F"] * nfill
    rng.shuffle(deck)
    draw, disc = deck[:], []
    hp = maxhp
    E = {k: dict(hp=v, block=0, st=0, vuln=0) for k, v in HP.items()}
    order = []
    hexed = False
    for r in range(1, 25):
        hand = []
        for _ in range(5):
            if not draw:
                draw, disc = disc, []
                rng.shuffle(draw)
            if draw:
                hand.append(draw.pop())
        en, block = 3, 0

        def state():
            enemies = []
            for eid, e in E.items():
                if e["hp"] <= 0:
                    continue
                d, h, _, _, kind = intent(eid, r, e["st"])
                intents = [{"type": "Attack", "label": f"{d}x{h}" if h > 1 else str(d)}] if d else [{"type": kind, "label": ""}]
                if eid == "MAGI_KNIGHT_0" and r == 1:
                    intents.append({"type": "Defend", "label": ""})
                st = [{"id": "STRENGTH_POWER", "amount": e["st"]}] if e["st"] else []
                st += [{"id": "VULNERABLE_POWER", "amount": e["vuln"]}] if e["vuln"] else []
                enemies.append({"entity_id": eid, "hp": e["hp"], "max_hp": HP[eid], "block": e["block"], "status": st, "intents": intents})
            cards = []
            for i, c in enumerate(hand):
                cid, name, ctype, cost, text, target = CARDS[c](db if c == "B" else da)
                extra = {"can_play": False, "unplayable_reason": "HasUnplayableKeyword"} if c == "F" else {}
                if hexed:
                    extra["keywords"] = [{"name": "Hexed", "description": "Add Ethereal to this card."}]
                cards.append(card(i, cid, name, ctype, cost, text, target, **extra))
            pile = lambda cs: [{"name": CARDS[c](da)[1], "cost": str(CARDS[c](da)[3]), "description": CARDS[c](db if c == "B" else da)[4]} for c in cs]
            return {
                "state_type": "elite", "run": {"act": 3, "floor": 44, "ascension": 8},
                "battle": {"round": r, "turn": "player", "is_play_phase": True, "enemies": enemies},
                "player": {"hp": hp, "max_hp": maxhp, "block": block, "energy": en, "hand": cards, "potions": [],
                           "status": [{"id": "HEX_POWER", "amount": 2}] if hexed else [],
                           "draw_pile": pile(draw), "discard_pile": pile(disc), "draw_pile_count": len(draw), "discard_pile_count": len(disc), "exhaust_pile_count": 0},
            }

        for _ in range(12):
            if all(e["hp"] <= 0 for e in E.values()):
                break
            act = combat.choose_card(state())
            if act is None:
                break
            i = act[1]["card_index"]
            c = hand[i]
            cost = CARDS[c](da)[3]
            if c in "AHV":
                t = act[1].get("target")
                if t is None or E[t]["hp"] <= 0:
                    t = next(x for x in E if E[x]["hp"] > 0)
                e = E[t]
                d = {"A": da, "H": int(2.2 * da), "V": 8}[c]
                d = int(d * 1.5) if e["vuln"] else d
                soak = min(e["block"], d)
                e["block"] -= soak
                e["hp"] = max(0, e["hp"] - (d - soak))
                if c == "V" and e["hp"] > 0:
                    e["vuln"] += 2
                if e["hp"] == 0:
                    order.append(t[:1])
            elif c == "B":
                block += db
            en -= cost
            disc.append(hand.pop(i))
            if trace:
                print("   play", c, act[1].get("target"), {k[:1]: v["hp"] for k, v in E.items()})
        if all(e["hp"] <= 0 for e in E.values()):
            return True, hp, r, "".join(order)
        inc = 0
        for eid, e in E.items():
            if e["hp"] > 0:
                d, h, _, _, _ = intent(eid, r, e["st"])
                inc += d * h
        lost = max(0, inc - block)
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


def setting(name: str, value: float):
    """Patch one constant of battle.py, combat.py or triggers.py (NAME=VALUE on the command line)."""
    mod = next(m for m in (battle, combat, triggers) if hasattr(m, name))
    return mock.patch.object(mod, name, value)


def run(args):
    arm, spec, n, seed = args
    logging.disable(logging.CRITICAL)
    if "=" in arm:
        name, value = arm.split("=")
        with setting(name, float(value)):
            return (arm,) + run(("on", spec, n, seed))[1:]
    table = {} if arm == "off" else dict(battle.KNOWN_THREAT)
    wins = hpl = rounds = 0
    firsts = {}
    with mock.patch.dict(battle.KNOWN_THREAT, table, clear=True):
        for j in range(n):
            ok, hp, r, order = fight(spec, random.Random(seed * 10007 + j))  # (a seed per fight: both arms play the same shuffles)
            wins += ok
            hpl += hp
            rounds += r
            firsts[order[:1] or "-"] = firsts.get(order[:1] or "-", 0) + 1
    return arm, spec, wins / n, hpl / n, rounds / n, firsts


DECKS = [  # damage, Block, Strikes, Defends, heavy, Bash, dead (an act 3 deck: 35 to 55 a round against these three)
    (16, 12, 6, 5, 2, 1, 1), (18, 12, 6, 5, 2, 1, 1), (16, 15, 5, 6, 2, 1, 1), (20, 13, 6, 5, 2, 1, 2), (18, 15, 5, 5, 3, 1, 1), (22, 12, 6, 5, 2, 1, 1),
]

if __name__ == "__main__":
    logging.disable(logging.CRITICAL)
    if sys.argv[1:2] == ["trace"]:
        seed = int(sys.argv[2]) if len(sys.argv) > 2 else 1
        dk = DECKS[int(sys.argv[3])] if len(sys.argv) > 3 and sys.argv[3].isdigit() else DECKS[1]
        with mock.patch.dict(battle.KNOWN_THREAT, {} if "off" in sys.argv[3:] else dict(battle.KNOWN_THREAT), clear=True):
            print(fight(dk, random.Random(seed), trace=True))
        sys.exit()
    if sys.argv[1:2] == ["flips"]:
        i = int(sys.argv[2]) if len(sys.argv) > 2 else 5
        for j in range(200):
            seed = (900 + i) * 10007 + j
            res = {}
            for arm in ("off", "on"):
                with mock.patch.dict(battle.KNOWN_THREAT, {} if arm == "off" else dict(battle.KNOWN_THREAT), clear=True):
                    res[arm] = fight(DECKS[i], random.Random(seed))
            if res["off"][0] != res["on"][0]:
                print(seed, res)
        sys.exit()
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 100
    arms = sys.argv[2:] or ["off", "on"]  # "off": without KNOWN_THREAT; "on": as the bot plays; NAME=VALUE: "on" with that constant
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
