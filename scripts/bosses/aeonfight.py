"""Aeonglass (the act 3 boss with the Withers) as a model fight, played by the bot's own planner.

    python3 scripts/bosses/aeonfight.py [FIGHTS ARM ...]   arms: "on" (as the bot plays), NAME=VALUE (a constant of
                                                           battle/combat/triggers), WB=X (the planner's Wither bill
                                                           times X), K.name=VALUE (a setting of learn.CHOICES),
                                                           joined by "+" for several
    python3 scripts/bosses/aeonfight.py trace [SEED] [DECK] [ARM]   one fight, play by play
    python3 scripts/bosses/aeonfight.py flips DECK ARM [ARM]        the fights one arm wins and the other loses

The fight is the Ascension 0 logs' (`intent`): Artifact 3; Withering Presence puts a Wither in our hand every 6 cards
played; a Wither left in the hand takes 3 damage a level (Block first), a level for each time the boss has buffed.
"""

import logging
import os
import random
import sys
from unittest import mock

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from modelfight import attack, deal, flips, game_state, hand_cards, hit, knob_setting, mean, patched, piles, played, shuffled, status, table, through, trace_args, turn
from sts2bot.fight import battle, combat

BOSS_HP, BOSS_BLOCK, ARTIFACT, EVERY = 512, 33, 3, 6


def intent(k, st):
    """(hits, damage a hit before Weak, Block gained, kind) for the k-th round of the cycle (k from 0)."""
    return [(1, 22 + st, BOSS_BLOCK, "Attack+Defend"), (2, 11 + st, 0, "Attack"), (0, 0, 0, "StatusCard+Buff")][k % 3]


CARDS = {  # kind: (id, name, type, cost, text, target)
    "A": lambda d: ("STRIKE_R", "Strike", "Attack", 1, f"Deal {d} damage.", "AnyEnemy"),
    "H": lambda d: ("HEAVY", "Heavy Blow", "Attack", 2, f"Deal {int(2.2 * d)} damage.", "AnyEnemy"),
    "V": lambda d: ("BASH", "Bash", "Attack", 2, "Deal 8 damage. Apply 2 Vulnerable.", "AnyEnemy"),
    "W": lambda d: ("SUCKER_PUNCH", "Sucker Punch", "Attack", 1, f"Deal {d // 2} damage. Apply 2 Weak.", "AnyEnemy"),
    "B": lambda b: ("DEFEND_R", "Defend", "Skill", 1, f"Gain {b} Block.", "Self"),
    "F": lambda _: ("CLUMSY", "Clumsy", "Curse", -1, "Unplayable.", "None"),
}


def wither(level):
    return ("WITHER", "Wither" if level == 0 else f"Wither+{level}", "Status", 0, f"Unplayable. At the end of your turn, if this is in your Hand, take {3 * (level + 1)} damage.", "None")


def fight(spec, rng, maxhp=75, trace=False):
    da, db, energy, size = spec[0], spec[1], spec[8], spec[9]
    draw, disc, hp = shuffled(rng, "ABHVWF", spec[2:]), [], maxhp
    boss = dict(hp=BOSS_HP, block=0, st=0, vuln=0, weak=0, art=ARTIFACT)
    level = buffs = 0  # Wither level (the same for all of them); Strength gained so far goes by `buffs`
    count = EVERY  # cards left before the next Wither
    face = lambda c: wither(level) if c == "X" else CARDS[c](db if c == "B" else da)
    for r in range(1, 31):
        k = (r - 1) % 3
        hand, en, block = deal(draw, disc, rng, size), energy, 0
        if trace:
            print(f"r{r} hand {''.join(hand)} hp {hp} boss {boss['hp']}/{boss['block']}blk st {boss['st']} intent {intent(k, boss['st'])[:3]} withers {(draw + disc + hand).count('X')} level {level} count {count}")

        def label():
            hits, d, blk, kind = intent(k, boss["st"])
            if not hits:
                return [{"type": "StatusCard", "label": "1"}, {"type": "Buff", "label": ""}]
            return [attack(int(d * (0.75 if boss["weak"] else 1.0)), hits)] + ([{"type": "Defend", "label": ""}] if blk else [])

        def state():
            st = [{"id": "WITHERING_PRESENCE_POWER", "amount": count}]
            st += status(("ARTIFACT_POWER", boss["art"]), ("STRENGTH_POWER", boss["st"]), ("VULNERABLE_POWER", boss["vuln"]), ("WEAK_POWER", boss["weak"]))
            enemies = [{"entity_id": "AEONGLASS_0", "hp": boss["hp"], "max_hp": BOSS_HP, "block": boss["block"], "status": st, "intents": label()}]
            return game_state("boss", {"act": 3, "floor": 48, "ascension": 0}, r, enemies, hand_cards(hand, face, dead="FX"), piles(face), draw, disc, hp=hp, maxhp=maxhp,
                              block=block, energy=en, status=[])

        for i, c, act in turn(state, hand, most=20):
            en -= CARDS[c](da)[3]
            if c in "AHVW":
                d = {"A": da, "H": int(2.2 * da), "V": 8, "W": da // 2}[c]
                hit(boss, int(d * 1.5) if boss["vuln"] else d)
                if c in "VW":  # Artifact swallows the debuff (and is used up)
                    if boss["art"]:
                        boss["art"] -= 1
                    else:
                        boss["vuln" if c == "V" else "weak"] += 2
            elif c == "B":
                block += db
            disc.append(hand.pop(i))
            count -= 1
            if count == 0:  # Withering Presence: a Wither lands in the hand
                hand.append("X")
                count = EVERY
            if trace:
                print(f"   play {c}  boss {boss['hp']}hp/{boss['block']}blk en {en} blk {block} count {count}")
            if boss["hp"] <= 0:
                return True, hp, r, buffs
        # End of our turn: each Wither still in the hand burns (Block takes it first); then the boss moves.
        burn = 3 * (level + 1) * hand.count("X")
        block, lost = through(block, burn)
        hp -= lost
        disc += hand
        before = hp
        boss["block"] = 0  # (its Block from last turn is gone when it acts)
        hits, d, blk, kind = intent(k, boss["st"])
        d = int(d * (0.75 if boss["weak"] else 1.0))
        for _ in range(hits):
            block, lost = through(block, d)
            hp -= lost
        if blk:
            boss["block"] = blk
        if kind == "StatusCard+Buff":
            boss["st"] += 3 + buffs
            buffs += 1
            level += 1
            disc.append("X")
        boss["vuln"], boss["weak"] = max(0, boss["vuln"] - 1), max(0, boss["weak"] - 1)
        if trace:
            print(f"r{r} burn {burn} took {before - hp} hp {hp} | boss {boss['hp']} st {boss['st']}")
        if hp <= 0:
            return False, boss["hp"], r, buffs
    return False, boss["hp"], 30, buffs


def setting(name: str, value: float):
    """NAME=VALUE and K.name=VALUE as modelfight.knob_setting takes them, or the Wither bill times X (WB=X)."""
    if name != "WB":
        return knob_setting(name, value)
    bill = battle._wither_bill
    return mock.patch.object(combat, "_wither_bill", lambda *a: bill(*a) * value)


def run(args):
    arm, spec, n, seed = args
    with patched(arm, setting):
        res = played(fight, spec, n, seed)
    return arm, spec, mean(res, 0), mean(res, 1), mean(res, 2)


DECKS = [  # damage, Block, Strikes, Defends, heavy, Bash, Weak, dead, energy, hand size (tuned until the planner wins 5-55%)
    (24, 14, 6, 6, 2, 1, 1, 1, 4, 6), (26, 14, 6, 6, 2, 1, 1, 1, 4, 5), (28, 14, 6, 6, 2, 1, 1, 1, 4, 5),
    (26, 14, 6, 6, 2, 1, 1, 1, 4, 6), (34, 14, 6, 6, 2, 1, 1, 1, 3, 5), (34, 14, 6, 6, 2, 1, 1, 1, 3, 6),
]

if __name__ == "__main__":
    logging.disable(logging.CRITICAL)
    if sys.argv[1:2] == ["trace"]:
        seed, dk, arm = trace_args(DECKS)
        with patched(arm, setting):
            print(fight(dk, random.Random(seed), trace=True))
    elif sys.argv[1:2] == ["flips"]:
        i, arms = int(sys.argv[2]), (sys.argv[3:] + ["on", "on"])[:2]
        for seed, res in flips(fight, DECKS[i], i, arms, lambda arm: patched(arm, setting), keep=3):
            if len({v[0] for v in res.values()}) > 1:
                print(seed, res)
    else:
        out, arms = table(run, DECKS, ["on"])
        for arm, rows in arms:
            print(f"{arm:28s}: won {mean(rows, 2):.3f}  boss HP left {mean(rows, 3):6.1f}  rounds {mean(rows, 4):4.1f}")
        for dk in DECKS:
            print(dk, "  ".join(f"{o[0]}: won {o[2]:.2f}" for o in out if o[1] == dk))
