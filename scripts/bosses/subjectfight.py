"""The Test Subject (the act 3 boss in three forms) as a model fight, played by the bot's own planner.

    python3 scripts/bosses/subjectfight.py [FIGHTS ARM ...]   arms: "on" (as the bot plays), NAME=VALUE (a constant of
                                                              battle/combat/triggers), K.name=VALUE (a setting of
                                                              learn.CHOICES), joined by "+" for several
    python3 scripts/bosses/subjectfight.py trace [SEED] [DECK] [ARM]   one fight, play by play
    python3 scripts/bosses/subjectfight.py flips DECK ARM [ARM]        the fights one arm wins and the other loses

100 fights a deck (600 an arm) take about 11 s an arm. What it said on 2026-11-22 (600 fights an arm, baseline 19.3%,
nearly every loss in form 3 after forms 1-2 cost about 30 HP): the planner is at its best on every Test Subject
constant: battle.STAB_SCAR 0 -> 14.3%, 4 16.5%, 12-40 19.3-21.0%; combat.FORM_WORTH 0 or 100 and combat.ENRAGE_PRICE
0.5 or 4 within a point; counting form 2's extra hit a round as growth (`grows`, so damage on it is worth up to 2.5x)
9.5-10.8%, so racing form 2 harder loses. `caution` 0.75 14.7%, 1.5 and 2.0 19.8-20.3%. What a deck change is worth
there (three decks of 27-33%, 150 fights each): one energy a turn +36 points, one card a hand +23, a dead card out
+12, Weak card +8, Defend +6, two-cost attack +5, Bash +1, one more Strike -4: the deck is the limit, as at the Demon.

The game side is the boss as the logs show it on Ascension 9 (2026-11-22, nine fights; Ascension 8 in brackets):
- form 1, 111 HP, Enrage 3 [2] (Strength for every Skill we play): Attack 22 [20], then Attack 16 [14] and
  Vulnerable on us for the next round (so the 22 lands as 33 + 1.5 x Strength), round and round;
- form 2, 212 HP, Painful Stabs (a Wound into our discard pile for each hit that gets through): 11x3 [10x3]
  and one hit more every round;
- form 3, 313 HP, Nemesis (Intangible on its 1st, 3rd, 5th ... round: every hit does 1): 11x3, 45, five [three]
  Burns into the discard pile and 3 [2] Strength, round and round.
A form killed leaves for the rest of the turn (its attack does not come); the next form starts the round after,
with its own Strength and debuffs at nothing. The deck is Strikes, Defends, two-cost attacks, a Bash (8 and 2
Vulnerable), Weak cards (half a Strike and 2 Weak) and dead cards, with energy and hand size set per deck (act 3
decks hit far harder than a Strike; see DECKS); each fight has its own seed, so every arm plays the same shuffles. Trace a fight and read it before believing a table (crabfight.py's lesson).
"""

import logging
import multiprocessing
import os
import random
import sys
from unittest import mock

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))

from sts2bot import learn
from sts2bot.fight import battle, combat, triggers
from tests.fake_game import card

FORM_HP = (111, 212, 313)
ENRAGE, HIT, BURNS, BUFF = 3, 11, 5, 3  # Ascension 9
BURN_TEXT = "Unplayable. At the end of your turn, if this is in your Hand, take 2 damage."


def intent(form, k, st):
    """(damage a hit before Weak/Vulnerable, hits, Burns, Strength gained, kind) on the k-th round of a form (k from 0)."""
    if form == 1:
        return (22 + st, 1, 0, 0, "Attack") if k % 2 == 0 else (16 + st, 1, 0, 0, "Attack+Debuff")
    if form == 2:
        return (HIT, 3 + k, 0, 0, "Attack")
    return [(HIT + st, 3, 0, 0, "Attack"), (45 + st, 1, 0, 0, "Attack"), (0, 0, BURNS, BUFF, "StatusCard+Buff")][k % 3]


CARDS = {  # kind: (id, name, type, cost, text, target)
    "A": lambda d: ("STRIKE_R", "Strike", "Attack", 1, f"Deal {d} damage.", "AnyEnemy"),
    "H": lambda d: ("HEAVY", "Heavy Blow", "Attack", 2, f"Deal {int(2.2 * d)} damage.", "AnyEnemy"),
    "V": lambda d: ("BASH", "Bash", "Attack", 2, "Deal 8 damage. Apply 2 Vulnerable.", "AnyEnemy"),
    "W": lambda d: ("SUCKER_PUNCH", "Sucker Punch", "Attack", 1, f"Deal {d // 2} damage. Apply 2 Weak.", "AnyEnemy"),
    "B": lambda b: ("DEFEND_R", "Defend", "Skill", 1, f"Gain {b} Block.", "Self"),
    "F": lambda _: ("CLUMSY", "Clumsy", "Curse", -1, "Unplayable.", "None"),
    "U": lambda _: ("WOUND", "Wound", "Status", 0, "Unplayable.", "None"),
    "R": lambda _: ("BURN", "Burn", "Status", 0, BURN_TEXT, "Self"),
}
DEAD = "FUR"  # cards that cannot be played


def fight(spec, rng, maxhp=75, trace=False):
    da, db, natk, nblk, nheavy, nbash, nweak, nfill, energy, size = spec
    deck = ["A"] * natk + ["B"] * nblk + ["H"] * nheavy + ["V"] * nbash + ["W"] * nweak + ["F"] * nfill
    rng.shuffle(deck)
    draw, disc = deck[:], []
    hp = maxhp
    form, k = 1, 0  # form, and rounds it has had
    e = dict(hp=FORM_HP[0], st=0, vuln=0, weak=0)
    our_vuln = 0  # rounds of Vulnerable on us
    lost_in = {1: 0, 2: 0, 3: 0}
    for r in range(1, 40):
        hand = []
        for _ in range(size):
            if not draw:
                draw, disc = disc, []
                rng.shuffle(draw)
            if draw:
                hand.append(draw.pop())
        en, block = energy, 0
        intangible = form == 3 and k % 2 == 0
        if trace:
            print(f"r{r} F{form} hand {''.join(hand)} hp {hp} boss {e['hp']} intent {intent(form, k, e['st'])[:2]}{' INTANGIBLE' if intangible else ''}")

        def label():
            d, h, burns, _, kind = intent(form, k, e["st"])
            if not d:
                return [{"type": "StatusCard", "label": str(burns)}, {"type": "Buff", "label": ""}]
            d = int(d * (0.75 if e["weak"] else 1.0) * (1.5 if our_vuln else 1.0))
            out = [{"type": "Attack", "label": f"{d}x{h}" if h > 1 else str(d)}]
            return out + ([{"type": "Debuff", "label": ""}] if "Debuff" in kind else [])

        def state():
            st = [{"id": "ADAPTABLE_POWER", "amount": 1}] if form < 3 else []
            st += [{"id": ("ENRAGE_POWER", "PAINFUL_STABS_POWER", "NEMESIS_POWER")[form - 1], "amount": ENRAGE if form == 1 else 1}]
            st += [{"id": "INTANGIBLE_POWER", "amount": 1}] if intangible else []
            st += [{"id": "STRENGTH_POWER", "amount": e["st"]}] if e["st"] else []
            st += [{"id": "VULNERABLE_POWER", "amount": e["vuln"]}] if e["vuln"] else []
            st += [{"id": "WEAK_POWER", "amount": e["weak"]}] if e["weak"] else []
            enemies = [{"entity_id": "TEST_SUBJECT_0", "hp": e["hp"], "max_hp": FORM_HP[form - 1], "block": 0, "status": st, "intents": label()}]
            cards = []
            for i, c in enumerate(hand):
                cid, name, ctype, cost, text, target = CARDS[c](db if c == "B" else da)
                extra = {"can_play": False, "unplayable_reason": "HasUnplayableKeyword"} if c in DEAD else {}
                cards.append(card(i, cid, name, ctype, cost, text, target, **extra))
            pile = lambda cs: [{"name": CARDS[c](da)[1], "cost": str(CARDS[c](da)[3]), "description": CARDS[c](db if c == "B" else da)[4]} for c in cs]
            return {
                "state_type": "boss", "run": {"act": 3, "floor": 48, "ascension": 9},
                "battle": {"round": r, "turn": "player", "is_play_phase": True, "enemies": enemies},
                "player": {"hp": hp, "max_hp": maxhp, "block": block, "energy": en, "hand": cards, "potions": [],
                           "status": [{"id": "VULNERABLE_POWER", "amount": our_vuln}] if our_vuln else [],
                           "draw_pile": pile(draw), "discard_pile": pile(disc), "draw_pile_count": len(draw), "discard_pile_count": len(disc), "exhaust_pile_count": 0},
            }

        killed = False
        for _ in range(15):
            act = combat.choose_card(state())
            if act is None:
                break
            i = act[1]["card_index"]
            c = hand[i]
            en -= CARDS[c](da)[3]
            if c in "AHVW":
                d = {"A": da, "H": int(2.2 * da), "V": 8, "W": da // 2}[c]
                d = int(d * 1.5) if e["vuln"] else d
                e["hp"] = max(0, e["hp"] - (1 if intangible else d))
                if c == "V":
                    e["vuln"] += 2
                if c == "W":
                    e["weak"] += 2
            elif c == "B":
                block += db
                if form == 1:
                    e["st"] += ENRAGE
            disc.append(hand.pop(i))
            if trace:
                print(f"   play {c}  F{form} {e['hp']}hp st {e['st']} en {en} blk {block}")
            if e["hp"] <= 0:
                killed = True
                break
        if killed and form == 3:
            return True, hp, r, 3, lost_in
        disc += hand
        before = hp
        if killed:
            form, k = form + 1, 0
            e = dict(hp=FORM_HP[form - 1], st=0, vuln=0, weak=0)
            our_vuln = max(0, our_vuln - 1)
            if trace:
                print(f"r{r} form {form - 1} killed: no attack | hp {hp}")
            continue
        # End of our turn: each Burn in the hand burns (Block takes it first), then the boss acts.
        burnt = 2 * sum(1 for c in hand if c == "R")
        soak = min(block, burnt)
        block -= soak
        hp -= burnt - soak
        d, h, burns, gain, kind = intent(form, k, e["st"])
        d = int(d * (0.75 if e["weak"] else 1.0) * (1.5 if our_vuln else 1.0))
        for _ in range(h):
            soak = min(block, d)
            block -= soak
            hp -= d - soak
            if form == 2 and d - soak > 0:
                disc.append("U")
        disc += ["R"] * burns
        e["st"] += gain
        lost_in[form] += before - hp
        if trace:
            print(f"r{r} F{form} took {before - hp} hp {hp} | boss {e['hp']} st {e['st']} | deck {len(draw) + len(disc)} wounds {(draw + disc).count('U')} burns {(draw + disc).count('R')}")
        if hp <= 0:
            return False, 0, r, form, lost_in
        our_vuln = max(0, our_vuln - 1)
        if "Debuff" in kind:
            our_vuln = 1
        e["vuln"], e["weak"] = max(0, e["vuln"] - 1), max(0, e["weak"] - 1)
        k += 1
    return False, 0, 40, form, lost_in


KNOBS = {"debuff_worth": 2.0}  # the live sampler's arms are 2.0 and 3.0 (learn.CHOICES); the default 1.0 is never played


def setting(name: str, value: float):
    """Patch one constant of battle.py, combat.py or triggers.py (NAME=VALUE), or a setting (K.name=VALUE)."""
    if name.startswith("K."):
        return mock.patch.dict(learn.KNOB, {name[2:]: value})
    mod = next(m for m in (battle, combat, triggers) if hasattr(m, name))
    return mock.patch.object(mod, name, value)


def patches(arm: str) -> list:
    return [mock.patch.dict(learn.KNOB, KNOBS)] + [setting(p.split("=")[0], float(p.split("=")[1])) for p in arm.split("+") if "=" in p]


def run(args):
    arm, spec, n, seed = args
    logging.disable(logging.CRITICAL)
    ps = patches(arm)
    for p in ps:
        p.start()
    try:
        wins = hpl = rounds = 0
        reached = {1: 0, 2: 0, 3: 0}
        lost = {1: 0, 2: 0, 3: 0}
        for j in range(n):
            ok, hp, r, form, lost_in = fight(spec, random.Random(seed * 10007 + j))  # (a seed per fight: every arm plays the same shuffles)
            wins += ok
            hpl += hp
            rounds += r
            reached[form] += not ok
            for f in lost:
                lost[f] += lost_in[f]
    finally:
        for p in reversed(ps):
            p.stop()
    return arm, spec, wins / n, hpl / n, rounds / n, reached, {f: v / n for f, v in lost.items()}


DECKS = [  # damage, Block, Strikes, Defends, heavy, Bash, Weak, dead, energy, hand size (tuned until the planner wins
    # 0-60%: the weaker ones die in form 2 as most live fights do, the stronger in form 3)
    (24, 15, 6, 5, 2, 1, 1, 2, 4, 6), (20, 14, 6, 6, 2, 1, 2, 2, 5, 6), (28, 18, 6, 6, 2, 1, 2, 2, 4, 6),
    (30, 18, 6, 6, 2, 1, 1, 2, 4, 6), (24, 16, 6, 6, 2, 1, 2, 2, 5, 6), (28, 16, 6, 6, 2, 1, 2, 2, 5, 5),
]

if __name__ == "__main__":
    logging.disable(logging.CRITICAL)
    if sys.argv[1:2] == ["trace"]:
        seed = int(sys.argv[2]) if len(sys.argv) > 2 else 1
        dk = DECKS[int(sys.argv[3])] if len(sys.argv) > 3 else DECKS[1]
        arm = sys.argv[4] if len(sys.argv) > 4 else "on"
        for p in patches(arm):
            p.start()
        print(fight(dk, random.Random(seed), trace=True))
        sys.exit()
    if sys.argv[1:2] == ["flips"]:
        i, arms = int(sys.argv[2]), (sys.argv[3:] + ["on", "on"])[:2]
        for j in range(200):
            seed = (900 + i) * 10007 + j
            res = {}
            for arm in arms:
                ps = patches(arm)
                for p in ps:
                    p.start()
                res[arm] = fight(DECKS[i], random.Random(seed))[:4]
                for p in reversed(ps):
                    p.stop()
            if len({v[0] for v in res.values()}) > 1:
                print(seed, res)
        sys.exit()
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 100
    arms = sys.argv[2:] or ["on"]
    jobs = [(arm, dk, n, 900 + i) for arm in arms for i, dk in enumerate(DECKS)]
    with multiprocessing.Pool(min(12, os.cpu_count() or 2)) as pool:
        out = pool.map(run, jobs)
    for arm in arms:
        rows = [o for o in out if o[0] == arm]
        died = {f: sum(o[5][f] for o in rows) for f in (1, 2, 3)}
        lost = {f: sum(o[6][f] for o in rows) / len(rows) for f in (1, 2, 3)}
        print(f"{arm:28s}: won {sum(o[2] for o in rows) / len(rows):.3f}  hp left {sum(o[3] for o in rows) / len(rows):5.1f}  rounds {sum(o[4] for o in rows) / len(rows):4.1f}"
              f"  died in form {died}  hp lost by form {' '.join(f'{v:.0f}' for v in lost.values())}")
    for dk in DECKS:
        print(dk, "  ".join(f"{o[0]}: won {o[2]:.2f} hp {o[3]:4.1f}" for o in out if o[1] == dk))
