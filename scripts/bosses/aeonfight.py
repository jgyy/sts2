"""Aeonglass (the act 3 boss with the Withers) as a model fight, played by the bot's own planner.

    python3 scripts/bosses/aeonfight.py [FIGHTS ARM ...]   arms: "on" (as the bot plays), NAME=VALUE (a constant of
                                                           battle/combat/triggers), WB=X (the planner's Wither bill
                                                           times X), K.name=VALUE (a setting of learn.CHOICES),
                                                           joined by "+" for several
    python3 scripts/bosses/aeonfight.py trace [SEED] [DECK] [ARM]   one fight, play by play
    python3 scripts/bosses/aeonfight.py flips DECK ARM [ARM]        the fights one arm wins and the other loses

The game side is the boss as the logs show it on Ascension 0 (2026-11-23, seven fights, one won):
- 512 HP, Artifact 3 (the first debuffs on it are swallowed), Withering Presence: every 6 cards we play a Wither
  is added to our hand (the count carries over from turn to turn);
- a Wither is unplayable and takes 3 damage for each level (Wither 3, Wither+1 6, ...) when the turn ends with it
  in the hand, Block first; the level is the number of times Aeonglass has buffed, for every one of them;
- three rounds, round and round: Attack 22 and Defend (33 Block until its next turn), Attack 11 twice, then a
  Status card and a Buff (a Wither into our discard pile, Strength +3, then +4, +5 ...); Strength adds to every hit.
The deck is Strikes, Defends, two-cost attacks, a Bash (8 and 2 Vulnerable), Weak cards and dead cards, energy and
hand size set per deck (see DECKS); each fight has its own seed, so every arm plays the same shuffles. Trace a
fight and read it before believing a table (crabfight.py's lesson).
"""

import logging
import os
import random
import sys
from contextlib import ExitStack
from unittest import mock

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

import modelfight
from modelfight import deal, game_state, hand_cards, on_all_cores
from sts2bot import learn
from sts2bot.fight import battle, combat

BOSS_HP, BOSS_BLOCK, ARTIFACT, EVERY = 512, 33, 3, 6


def intent(k, st):
    """(hits, damage a hit before Weak, Block gained, kind) for the k-th round of the cycle (k from 0)."""
    return [(1, 22 + st, BOSS_BLOCK, "Attack+Defend"), (2, 11 + st, 0, "Attack"), (0, 0, 0, "StatusCard+Buff")][k % 3]


def wither_name(level):
    return "Wither" if level == 0 else f"Wither+{level}"


CARDS = {  # kind: (id, name, type, cost, text, target)
    "A": lambda d: ("STRIKE_R", "Strike", "Attack", 1, f"Deal {d} damage.", "AnyEnemy"),
    "H": lambda d: ("HEAVY", "Heavy Blow", "Attack", 2, f"Deal {int(2.2 * d)} damage.", "AnyEnemy"),
    "V": lambda d: ("BASH", "Bash", "Attack", 2, "Deal 8 damage. Apply 2 Vulnerable.", "AnyEnemy"),
    "W": lambda d: ("SUCKER_PUNCH", "Sucker Punch", "Attack", 1, f"Deal {d // 2} damage. Apply 2 Weak.", "AnyEnemy"),
    "B": lambda b: ("DEFEND_R", "Defend", "Skill", 1, f"Gain {b} Block.", "Self"),
    "F": lambda _: ("CLUMSY", "Clumsy", "Curse", -1, "Unplayable.", "None"),
}
DEAD = "FX"  # cards that cannot be played ("X": a Wither)


def wither_text(level):
    return f"Unplayable. At the end of your turn, if this is in your Hand, take {3 * (level + 1)} damage."


def fight(spec, rng, maxhp=75, trace=False):
    da, db, natk, nblk, nheavy, nbash, nweak, nfill, energy, size = spec
    deck = ["A"] * natk + ["B"] * nblk + ["H"] * nheavy + ["V"] * nbash + ["W"] * nweak + ["F"] * nfill
    rng.shuffle(deck)
    draw, disc = deck[:], []
    hp = maxhp
    boss = dict(hp=BOSS_HP, block=0, st=0, vuln=0, weak=0, art=ARTIFACT)
    level = buffs = 0  # Wither level (the same for all of them); Strength gained so far goes by `buffs`
    count = EVERY  # cards left before the next Wither
    for r in range(1, 31):
        k = (r - 1) % 3
        hand = deal(draw, disc, rng, size)
        en, block, played = energy, 0, 0
        if trace:
            print(f"r{r} hand {''.join(hand)} hp {hp} boss {boss['hp']}/{boss['block']}blk st {boss['st']} intent {intent(k, boss['st'])[:3]} withers {(draw + disc + hand).count('X')} level {level} count {count}")

        def label():
            hits, d, blk, kind = intent(k, boss["st"])
            if not hits:
                return [{"type": "StatusCard", "label": "1"}, {"type": "Buff", "label": ""}]
            d = int(d * (0.75 if boss["weak"] else 1.0))
            out = [{"type": "Attack", "label": f"{d}x{hits}" if hits > 1 else str(d)}]
            return out + ([{"type": "Defend", "label": ""}] if blk else [])

        def face(c):
            return ("WITHER", wither_name(level), "Status", 0, wither_text(level), "None") if c == "X" else CARDS[c](db if c == "B" else da)

        def state():
            st = [{"id": "WITHERING_PRESENCE_POWER", "amount": count}]
            st += [{"id": "ARTIFACT_POWER", "amount": boss["art"]}] if boss["art"] else []
            st += [{"id": "STRENGTH_POWER", "amount": boss["st"]}] if boss["st"] else []
            st += [{"id": "VULNERABLE_POWER", "amount": boss["vuln"]}] if boss["vuln"] else []
            st += [{"id": "WEAK_POWER", "amount": boss["weak"]}] if boss["weak"] else []
            enemies = [{"entity_id": "AEONGLASS_0", "hp": boss["hp"], "max_hp": BOSS_HP, "block": boss["block"], "status": st, "intents": label()}]
            cards = hand_cards(hand, face, dead=DEAD)
            pile = lambda cs: [{"name": face(c)[1], "cost": str(face(c)[3]), "description": face(c)[4]} for c in cs]
            return game_state("boss", {"act": 3, "floor": 48, "ascension": 0}, r, enemies, cards, pile, draw, disc, hp=hp, maxhp=maxhp, block=block, energy=en, status=[])

        for _ in range(20):
            act = combat.choose_card(state())
            if act is None:
                break
            i = act[1]["card_index"]
            c = hand[i]
            en -= CARDS[c](da)[3]
            if c in "AHVW":
                d = {"A": da, "H": int(2.2 * da), "V": 8, "W": da // 2}[c]
                d = int(d * 1.5) if boss["vuln"] else d
                soak = min(boss["block"], d)
                boss["block"] -= soak
                boss["hp"] = max(0, boss["hp"] - (d - soak))
                if c in "VW":  # Artifact swallows the debuff (and is used up)
                    if boss["art"]:
                        boss["art"] -= 1
                    elif c == "V":
                        boss["vuln"] += 2
                    else:
                        boss["weak"] += 2
            elif c == "B":
                block += db
            disc.append(hand.pop(i))
            played += 1
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
        soak = min(block, burn)
        block -= soak
        hp -= burn - soak
        disc += hand
        before = hp
        boss["block"] = 0  # (its Block from last turn is gone when it acts)
        hits, d, blk, kind = intent(k, boss["st"])
        d = int(d * (0.75 if boss["weak"] else 1.0))
        for _ in range(hits):
            soak = min(block, d)
            block -= soak
            hp -= d - soak
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


KNOBS = {"debuff_worth": 2.0}  # the live sampler's arms are 2.0 and 3.0 (learn.CHOICES); the default 1.0 is never played


def setting(name: str, value: float):
    """Patch one constant of battle.py, combat.py or triggers.py (NAME=VALUE), the Wither bill (WB=X), or a setting (K.name=VALUE)."""
    if name == "WB":
        bill = battle._wither_bill
        return mock.patch.object(combat, "_wither_bill", lambda *a: bill(*a) * value)
    return mock.patch.dict(learn.KNOB, {name[2:]: value}) if name.startswith("K.") else modelfight.setting(name, value)


def patched(arm: str) -> ExitStack:
    """Every patch an arm asks for, entered (leave the stack to undo them)."""
    stack = ExitStack()
    for p in [mock.patch.dict(learn.KNOB, KNOBS)] + [setting(p.split("=")[0], float(p.split("=")[1])) for p in arm.split("+") if "=" in p]:
        stack.enter_context(p)
    return stack


def run(args):
    arm, spec, n, seed = args
    logging.disable(logging.CRITICAL)
    with patched(arm):
        wins = left = rounds = 0
        for j in range(n):
            ok, rest, r, _ = fight(spec, random.Random(seed * 10007 + j))  # (a seed per fight: every arm plays the same shuffles)
            wins += ok
            left += rest
            rounds += r
    return arm, spec, wins / n, left / n, rounds / n


DECKS = [  # damage, Block, Strikes, Defends, heavy, Bash, Weak, dead, energy, hand size (tuned until the planner wins 5-55%:
    # 3 energy needs 34+ damage a card, 4 energy 24-28)
    (24, 14, 6, 6, 2, 1, 1, 1, 4, 6), (26, 14, 6, 6, 2, 1, 1, 1, 4, 5), (28, 14, 6, 6, 2, 1, 1, 1, 4, 5),
    (26, 14, 6, 6, 2, 1, 1, 1, 4, 6), (34, 14, 6, 6, 2, 1, 1, 1, 3, 5), (34, 14, 6, 6, 2, 1, 1, 1, 3, 6),
]

if __name__ == "__main__":
    logging.disable(logging.CRITICAL)
    if sys.argv[1:2] == ["trace"]:
        seed = int(sys.argv[2]) if len(sys.argv) > 2 else 1
        dk = DECKS[int(sys.argv[3])] if len(sys.argv) > 3 else DECKS[1]
        arm = sys.argv[4] if len(sys.argv) > 4 else "on"
        with patched(arm):
            print(fight(dk, random.Random(seed), trace=True))
        sys.exit()
    if sys.argv[1:2] == ["flips"]:
        i, arms = int(sys.argv[2]), (sys.argv[3:] + ["on", "on"])[:2]
        for j in range(200):
            seed = (900 + i) * 10007 + j
            res = {}
            for arm in arms:
                with patched(arm):
                    res[arm] = fight(DECKS[i], random.Random(seed))[:3]
            if len({v[0] for v in res.values()}) > 1:
                print(seed, res)
        sys.exit()
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 100
    arms = sys.argv[2:] or ["on"]
    jobs = [(arm, dk, n, 900 + i) for arm in arms for i, dk in enumerate(DECKS)]
    out = on_all_cores(run, jobs)
    for arm in arms:
        rows = [o for o in out if o[0] == arm]
        print(f"{arm:28s}: won {sum(o[2] for o in rows) / len(rows):.3f}  boss HP left {sum(o[3] for o in rows) / len(rows):6.1f}  rounds {sum(o[4] for o in rows) / len(rows):4.1f}")
    for dk in DECKS:
        print(dk, "  ".join(f"{o[0]}: won {o[2]:.2f}" for o in out if o[1] == dk))
