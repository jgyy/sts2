"""The Queen and her Torch Head Amalgam (an act 3 boss) as a model fight, played by the bot's own planner.

    python3 scripts/bosses/queenfight.py [FIGHTS ARM ...]   arms: "on" (as the bot plays), NAME=VALUE (a constant of
                                                            battle/combat/triggers), K.name=VALUE (a setting of
                                                            learn.CHOICES), joined by "+" for several
    python3 scripts/bosses/queenfight.py trace [SEED] [DECK] [ARM]   one fight, play by play
    python3 scripts/bosses/queenfight.py flips DECK ARM [ARM]        the fights one arm wins and the other loses

The fight is the game data's and the logs' (Ascension 0-7 numbers): round 1 Puppet Strings (Chains of Binding 3: the
first 3 cards drawn each turn are Bound, one Bound card a turn), round 2 You Are Mine (99 Weak, Frail and Vulnerable on
us), then Burn Bright for Me (the Amalgam +1 Strength, the Queen 20 Block) while the Amalgam lives; from the round after
it dies, Off With Your Head 3x5, Execution 15, Enrage +2 Strength. The Amalgam (a minion: it leaves when she dies) hits
26, 18, then 8x3, 14, 14 over and over. Our Strikes read 25% less from round 3 (Weak) and Defends 25% less (Frail).
"""

import logging
import os
import random
import sys
from unittest import mock

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from modelfight import attack, flips, game_state, knob_setting, mean, patched, piles, played, shuffled, status, table, through, trace_args, turn
from sts2bot.fight import battle
from tests.fake_game import card

QUEEN_HP, AMALGAM_HP, QUEEN_BLOCK = 400, 199, 20
UNPLAYABLE = {"can_play": False, "unplayable_reason": "HasUnplayableKeyword"}
BOUND_OUT = {"can_play": False, "unplayable_reason": "BlockedByHook"}

CARDS = {  # kind: (id, name, type, cost, text, target); d, b, v are what a Strike, a Defend and Bash read now (Weak, Frail in)
    "A": lambda d, b, v: ("STRIKE_R", "Strike", "Attack", 1, f"Deal {d} damage.", "AnyEnemy"),
    "H": lambda d, b, v: ("HEAVY", "Heavy Blow", "Attack", 2, f"Deal {int(2.2 * d)} damage.", "AnyEnemy"),
    "V": lambda d, b, v: ("BASH", "Bash", "Attack", 2, f"Deal {v} damage. Apply 2 Vulnerable.", "AnyEnemy"),
    "W": lambda d, b, v: ("SUCKER_PUNCH", "Sucker Punch", "Attack", 1, f"Deal {d // 2} damage. Apply 2 Weak.", "AnyEnemy"),
    "B": lambda d, b, v: ("DEFEND_R", "Defend", "Skill", 1, f"Gain {b} Block.", "Self"),
    "D": lambda d, b, v: ("BACKFLIP", "Backflip", "Skill", 1, f"Gain {b * 2 // 3} Block. Draw 2 cards.", "Self"),
    "F": lambda d, b, v: ("CLUMSY", "Clumsy", "Curse", -1, "Unplayable.", "None"),
}
KINDS = "ABHVWDF"


def dealt(c, d, v):
    """Damage a card of kind c does when Strikes read d and Bash v."""
    return {"A": d, "H": int(2.2 * d), "V": v, "W": d // 2}[c]


def amalgam_move(r, st):
    """(damage a hit, hits) of the Amalgam on round r, Strength in."""
    if r == 1:
        return 26 + st, 1
    if r == 2:
        return 18 + st, 1
    return [(8 + st, 3), (14 + st, 1), (14 + st, 1)][(r - 3) % 3]


def queen_move(k, st):
    """(damage a hit, hits, kind) of the Queen's k-th attack-cycle round (from 0)."""
    return [(3 + st, 5, "Attack"), (15 + st, 1, "Attack"), (0, 0, "Buff")][k % 3]


def fight(spec, rng, maxhp=80, trace=False):
    da, db, energy, size = spec[0], spec[1], spec[-2], spec[-1]
    draw, disc, hp = shuffled(rng, KINDS, spec[2:-2]), [], maxhp
    q = dict(hp=QUEEN_HP, block=0, st=0, vuln=0, weak=0)
    a = dict(hp=AMALGAM_HP, block=0, st=0, vuln=0, weak=0)
    chains = debuffed = False
    k = None  # the Queen's attack cycle: None until the Amalgam is gone
    for r in range(1, 41):
        weak_us = debuffed
        d_now, b_now, v_now = (int(da * 0.75), int(db * 0.75), 6) if debuffed else (da, db, 8)
        face = lambda c: CARDS[c](d_now, b_now, v_now)
        hand, bound = [], []
        for n in range(size):
            if not draw:
                draw[:] = disc
                disc.clear()
                rng.shuffle(draw)
            if draw:
                hand.append(draw.pop())
                bound.append(chains and n < 3)
        en, block, bound_used = energy, 0, False
        if trace:
            print(f"r{r} hand {''.join(c + ('*' if b else '') for c, b in zip(hand, bound))} hp {hp} | amalgam {a['hp']} st {a['st']} | queen {q['hp']}/{q['block']}blk st {q['st']} k {k}")

        def intents():
            out = []
            if a["hp"] > 0:
                d, h = amalgam_move(r, a["st"])
                out.append([attack(int(d * (0.75 if a["weak"] else 1.0) * (1.5 if debuffed else 1.0)), h)])
            if r == 1:
                qi = [{"type": "CardDebuff", "label": ""}]
            elif r == 2:
                qi = [{"type": "Debuff", "label": ""}]
            elif k is None:
                qi = [{"type": "Buff", "label": ""}, {"type": "Defend", "label": ""}]
            else:
                d, h, kind = queen_move(k, q["st"])
                qi = [attack(int(d * (0.75 if q["weak"] else 1.0) * 1.5), h)] if h else [{"type": "Buff", "label": ""}]
            out.append(qi)
            return out

        def state():
            labels = intents()
            enemies = []
            if a["hp"] > 0:
                enemies.append({"entity_id": "TORCH_HEAD_AMALGAM_0", "hp": a["hp"], "max_hp": AMALGAM_HP, "block": a["block"],
                                "status": status(("MINION_POWER", 1), ("STRENGTH_POWER", a["st"]), ("VULNERABLE_POWER", a["vuln"]), ("WEAK_POWER", a["weak"])),
                                "intents": labels[0]})
            enemies.append({"entity_id": "QUEEN_0", "hp": q["hp"], "max_hp": QUEEN_HP, "block": q["block"],
                            "status": status(("STRENGTH_POWER", q["st"]), ("VULNERABLE_POWER", q["vuln"]), ("WEAK_POWER", q["weak"])), "intents": labels[-1]})
            cards = []
            for i, (c, b) in enumerate(zip(hand, bound)):
                extra = dict(UNPLAYABLE) if c == "F" else dict(BOUND_OUT) if b and bound_used else {}
                if b:
                    extra["keywords"] = [{"name": "Bound", "description": "Only 1 Bound card can be played each turn."}]
                cards.append(card(i, *face(c), **extra))
            mine = status(("CHAINS_OF_BINDING_POWER", 3 * chains), ("FRAIL_POWER", 99 * debuffed), ("WEAK_POWER", 99 * weak_us), ("VULNERABLE_POWER", 99 * debuffed))
            return game_state("boss", {"act": 3, "floor": 48, "ascension": 6}, r, enemies, cards, piles(face), draw, disc, hp=hp, maxhp=maxhp,
                              block=block, energy=en, status=mine)

        for i, c, act in turn(state, hand, most=15):
            en -= face(c)[3]
            if bound[i]:
                bound_used = True
            tid = act[1].get("target")
            foe = a if tid == "TORCH_HEAD_AMALGAM_0" and a["hp"] > 0 else q
            if c in "AHVW":
                d = dealt(c, d_now, v_now)
                d = int(d * 1.5) if foe["vuln"] else d
                soak = min(foe["block"], d)
                foe["block"] -= soak
                foe["hp"] = max(0, foe["hp"] - (d - soak))
                if c == "V":
                    foe["vuln"] += 2
                if c == "W":
                    foe["weak"] += 2
            elif c in "BD":
                block += b_now if c == "B" else b_now * 2 // 3
            disc.append(hand.pop(i))
            bound.pop(i)
            if c == "D":
                for _ in range(2):
                    if not draw:
                        draw[:] = disc
                        disc.clear()
                        rng.shuffle(draw)
                    if draw:
                        hand.append(draw.pop())
                        bound.append(False)
            if trace:
                print(f"   play {c}{' at ' + ('A' if foe is a else 'Q') if c in 'AHVW' else ''}  amalgam {a['hp']} queen {q['hp']}/{q['block']} en {en} blk {block}")
            if q["hp"] <= 0:
                return True, hp, r, a["hp"] <= 0
        disc += hand
        before = hp
        # The enemies' turn: the Amalgam first, then the Queen (her Block from last turn goes when she acts).
        if a["hp"] > 0:
            d, h = amalgam_move(r, a["st"])
            d = int(d * (0.75 if a["weak"] else 1.0) * (1.5 if debuffed else 1.0))
            for _ in range(h):
                block, lost = through(block, d)
                hp -= lost
        q["block"] = 0
        if r == 1:
            chains = True
        elif r == 2:
            debuffed = True
        elif k is None and a["hp"] > 0:
            a["st"] += 1
            q["block"] = QUEEN_BLOCK
        else:
            k = 0 if k is None else k
            d, h, kind = queen_move(k, q["st"])
            d = int(d * (0.75 if q["weak"] else 1.0) * (1.5 if debuffed else 1.0))
            for _ in range(h):
                block, lost = through(block, d)
                hp -= lost
            if kind == "Buff":
                q["st"] += 2
            k += 1
        for e in (a, q):
            e["vuln"], e["weak"] = max(0, e["vuln"] - 1), max(0, e["weak"] - 1)
        if trace:
            print(f"r{r} took {before - hp} hp {hp}")
        if hp <= 0:
            return False, 0, r, a["hp"] <= 0
    return False, 0, 40, a["hp"] <= 0


def setting(name: str, value: float):
    """NAME=VALUE and K.name=VALUE as modelfight.knob_setting takes them, or the Amalgam's worth as a minion (MW=X; the bot
    has it at 1.0 while it is the main threat, which it is in this fight)."""
    if name != "MW":
        return knob_setting(name, value)
    return mock.patch.object(battle.Enemy, "worth", property(lambda e: value if e.minion else 1.0))


def run(args):
    arm, spec, n, seed = args
    with patched(arm, setting):
        res = played(fight, spec, n, seed)
    return arm, spec, mean(res, 0), mean(res, 1), mean(res, 2), mean(res, 3)


DECKS = [  # damage, Block, Strikes, Defends, heavy, Bash, Weak, Backflip, dead, energy, hand size (tuned until the planner wins 10-75%)
    (26, 14, 6, 6, 2, 1, 1, 1, 2, 4, 6), (26, 18, 6, 6, 2, 1, 1, 1, 2, 4, 6), (26, 18, 6, 6, 2, 1, 1, 1, 2, 5, 5),
    (32, 14, 6, 6, 2, 1, 1, 1, 2, 4, 5), (32, 18, 6, 6, 2, 1, 1, 1, 2, 4, 5), (36, 14, 6, 6, 2, 1, 1, 1, 2, 4, 5),
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
            print(f"{arm:28s}: won {mean(rows, 2):.3f}  hp left {mean(rows, 3):5.1f}  rounds {mean(rows, 4):4.1f}  amalgam dead {mean(rows, 5):.2f}")
        for dk in DECKS:
            print(dk, "  ".join(f"{o[0]}: won {o[2]:.2f} hp {o[3]:4.1f}" for o in out if o[1] == dk))
