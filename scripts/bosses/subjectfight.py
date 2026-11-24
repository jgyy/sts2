"""The Test Subject (the act 3 boss in three forms) as a model fight, played by the bot's own planner.

    python3 scripts/bosses/subjectfight.py [FIGHTS ARM ...]   arms: "on" (as the bot plays), NAME=VALUE (a constant of
                                                              battle/combat/triggers), K.name=VALUE (a setting of
                                                              learn.CHOICES), joined by "+" for several
    python3 scripts/bosses/subjectfight.py trace [SEED] [DECK] [ARM]   one fight, play by play
    python3 scripts/bosses/subjectfight.py flips DECK ARM [ARM]        the fights one arm wins and the other loses

The fight is the Ascension 9 logs' (`intent`): form 1 (111 HP) Enrage, form 2 (212) Painful Stabs (a Wound for each hit
that gets through), form 3 (313) Nemesis (Intangible every other round) and Burns. A form killed does not attack; the
next starts the round after, with its Strength and debuffs at nothing.
"""

import logging
import os
import random
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from modelfight import attack, deal, flips, game_state, hand_cards, mean, patched, piles, played, shuffled, status, table, through, trace_args, turn

FORM_HP = (111, 212, 313)
ENRAGE, HIT, BURNS, BUFF = 3, 11, 5, 3  # Ascension 9


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
    "R": lambda _: ("BURN", "Burn", "Status", 0, "Unplayable. At the end of your turn, if this is in your Hand, take 2 damage.", "Self"),
}


def fight(spec, rng, maxhp=75, trace=False):
    da, db, energy, size = spec[0], spec[1], spec[8], spec[9]
    draw, disc, hp = shuffled(rng, "ABHVWF", spec[2:]), [], maxhp
    form, k = 1, 0  # form, and rounds it has had
    e = dict(hp=FORM_HP[0], st=0, vuln=0, weak=0)
    our_vuln = 0  # rounds of Vulnerable on us
    lost_in = {1: 0, 2: 0, 3: 0}
    face = lambda c: CARDS[c](db if c == "B" else da)
    for r in range(1, 40):
        hand, en, block = deal(draw, disc, rng, size), energy, 0
        intangible = form == 3 and k % 2 == 0
        if trace:
            print(f"r{r} F{form} hand {''.join(hand)} hp {hp} boss {e['hp']} intent {intent(form, k, e['st'])[:2]}{' INTANGIBLE' if intangible else ''}")

        def label():
            d, h, burns, _, kind = intent(form, k, e["st"])
            if not d:
                return [{"type": "StatusCard", "label": str(burns)}, {"type": "Buff", "label": ""}]
            return [attack(int(d * (0.75 if e["weak"] else 1.0) * (1.5 if our_vuln else 1.0)), h)] + ([{"type": "Debuff", "label": ""}] if "Debuff" in kind else [])

        def state():
            st = status(("ADAPTABLE_POWER", int(form < 3)), (("ENRAGE_POWER", "PAINFUL_STABS_POWER", "NEMESIS_POWER")[form - 1], ENRAGE if form == 1 else 1),
                        ("INTANGIBLE_POWER", int(intangible)), ("STRENGTH_POWER", e["st"]), ("VULNERABLE_POWER", e["vuln"]), ("WEAK_POWER", e["weak"]))
            enemies = [{"entity_id": "TEST_SUBJECT_0", "hp": e["hp"], "max_hp": FORM_HP[form - 1], "block": 0, "status": st, "intents": label()}]
            return game_state("boss", {"act": 3, "floor": 48, "ascension": 9}, r, enemies, hand_cards(hand, face, dead="FUR"), piles(face), draw, disc, hp=hp, maxhp=maxhp,
                              block=block, energy=en, status=status(("VULNERABLE_POWER", our_vuln)))

        for i, c, act in turn(state, hand, most=15):
            en -= CARDS[c](da)[3]
            if c in "AHVW":
                d = {"A": da, "H": int(2.2 * da), "V": 8, "W": da // 2}[c]
                e["hp"] = max(0, e["hp"] - (1 if intangible else int(d * 1.5) if e["vuln"] else d))
                if c in "VW":
                    e["vuln" if c == "V" else "weak"] += 2
            elif c == "B":
                block += db
                e["st"] += ENRAGE if form == 1 else 0
            disc.append(hand.pop(i))
            if trace:
                print(f"   play {c}  F{form} {e['hp']}hp st {e['st']} en {en} blk {block}")
            if e["hp"] <= 0:
                break
        killed = e["hp"] <= 0
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
        block, burnt = through(block, 2 * hand.count("R"))
        hp -= burnt
        d, h, burns, gain, kind = intent(form, k, e["st"])
        d = int(d * (0.75 if e["weak"] else 1.0) * (1.5 if our_vuln else 1.0))
        for _ in range(h):
            block, lost = through(block, d)
            hp -= lost
            if form == 2 and lost > 0:
                disc.append("U")
        disc += ["R"] * burns
        e["st"] += gain
        lost_in[form] += before - hp
        if trace:
            print(f"r{r} F{form} took {before - hp} hp {hp} | boss {e['hp']} st {e['st']} | deck {len(draw) + len(disc)} wounds {(draw + disc).count('U')} burns {(draw + disc).count('R')}")
        if hp <= 0:
            return False, 0, r, form, lost_in
        our_vuln = 1 if "Debuff" in kind else max(0, our_vuln - 1)
        e["vuln"], e["weak"] = max(0, e["vuln"] - 1), max(0, e["weak"] - 1)
        k += 1
    return False, 0, 40, form, lost_in


def run(args):
    arm, spec, n, seed = args
    with patched(arm):
        res = played(fight, spec, n, seed)
    return (arm, spec, mean(res, 0), mean(res, 1), mean(res, 2), {f: sum(not o[0] and o[3] == f for o in res) for f in (1, 2, 3)},
            {f: sum(o[4][f] for o in res) / n for f in (1, 2, 3)})


DECKS = [  # damage, Block, Strikes, Defends, heavy, Bash, Weak, dead, energy, hand size (tuned until the planner wins 0-60%)
    (24, 15, 6, 5, 2, 1, 1, 2, 4, 6), (20, 14, 6, 6, 2, 1, 2, 2, 5, 6), (28, 18, 6, 6, 2, 1, 2, 2, 4, 6),
    (30, 18, 6, 6, 2, 1, 1, 2, 4, 6), (24, 16, 6, 6, 2, 1, 2, 2, 5, 6), (28, 16, 6, 6, 2, 1, 2, 2, 5, 5),
]

if __name__ == "__main__":
    logging.disable(logging.CRITICAL)
    if sys.argv[1:2] == ["trace"]:
        seed, dk, arm = trace_args(DECKS)
        with patched(arm):
            print(fight(dk, random.Random(seed), trace=True))
    elif sys.argv[1:2] == ["flips"]:
        i, arms = int(sys.argv[2]), (sys.argv[3:] + ["on", "on"])[:2]
        for seed, res in flips(fight, DECKS[i], i, arms, patched, keep=4):
            if len({v[0] for v in res.values()}) > 1:
                print(seed, res)
    else:
        out, arms = table(run, DECKS, ["on"])
        for arm, rows in arms:
            died = {f: sum(o[5][f] for o in rows) for f in (1, 2, 3)}
            lost = ' '.join(f'{mean([o[6] for o in rows], f):.0f}' for f in (1, 2, 3))
            print(f"{arm:28s}: won {mean(rows, 2):.3f}  hp left {mean(rows, 3):5.1f}  rounds {mean(rows, 4):4.1f}  died in form {died}  hp lost by form {lost}")
        for dk in DECKS:
            print(dk, "  ".join(f"{o[0]}: won {o[2]:.2f} hp {o[3]:4.1f}" for o in out if o[1] == dk))
