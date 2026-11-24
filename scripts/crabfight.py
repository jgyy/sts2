"""Crusher and Rocket as a model fight, played by the bot's own planner (combat.choose_card).

    python3 scripts/crabfight.py [FIGHTS] [ARM,ARM,...]   ARM: a RAGE_COST, NAME=VALUE (a constant of battle.py or
                                                          combat.py, else a learn.py knob: caution=1.5), or
                                                          threat=ROCKET/20/CRUSHER/12 (battle.KNOWN_THREAT); '+' joins
    python3 scripts/crabfight.py FIGHTS ARMS debuff       the decks with a Bash and a Clothesline (Vulnerable, Weak)
    python3 scripts/crabfight.py trace [SEED]

The rules are the logs' (C_CYCLE, R_CYCLE): a crab hits half again from behind us, we face the one we hit last, and a
crab whose ally dies gains 6 Strength and 99 Block. Trace a fight and read it before believing a table.
"""

import collections
import contextlib
import logging
import os
import random
import sys
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from modelfight import attack, deal, game_state, hand_cards, hit, mean, on_all_cores, shuffled, status, target, turn
from sts2bot.fight import battle, combat
from sts2bot import learn

C_CYCLE = [(12, 1, False, False), (4, 1, False, False), (6, 2, False, True), (0, 0, False, False), (12, 1, True, False)]  # hit, hits, 15 Block, Weak+Frail
R_CYCLE = [(3, 1), (18, 1), (0, 0), (31, 1), (0, 0)]
KIND_C = ["Attack", "Attack", "Attack", "Buff", "Attack"]
KIND_R = ["Attack", "Attack", "Buff", "Attack", "Sleep"]
FACES = {  # kind: (id, name, type, cost, text, target) from the number on the card
    'A': lambda n: ("STRIKE_R", "Strike", "Attack", 1, f"Deal {n} damage.", "AnyEnemy"),
    'H': lambda n: ("HEAVY", "Heavy Blow", "Attack", 2, f"Deal {n} damage.", "AnyEnemy"),
    'V': lambda n: ("BASH_R", "Bash", "Attack", 1, f"Deal {n} damage. Apply 2 Vulnerable.", "AnyEnemy"),
    'W': lambda n: ("CLOTHESLINE_R", "Clothesline", "Attack", 1, f"Deal {n} damage. Apply 2 Weak.", "AnyEnemy"),
    'B': lambda n: ("DEFEND_R", "Defend", "Skill", 1, f"Gain {n} Block.", "Self"),
    'F': lambda n: ("CLUMSY", "Clumsy", "Curse", -1, "Unplayable.", "None"),
}


def number(c, da, db, w=1, fw=1):
    """The damage (or Block) a card does, `w` and `fw` what Weak and Frail leave of it."""
    return {'A': int(da * w), 'H': int(2.2 * da * w), 'V': int(da // 2 * w), 'W': int(da // 2 * w), 'B': int(db * fw)}.get(c, 0)


def fight(deck_spec, energy, maxhp, rng, chp=206, rhp=196, trace=False):
    da, db = deck_spec[:2]
    draw, disc, hp = shuffled(rng, "ABFHVW", deck_spec[2:]), [], maxhp
    E = {'CRUSHER_0': dict(hp=chp, mx=chp, block=0, st=0, vul=0, wk=0), 'ROCKET_0': dict(hp=rhp, mx=rhp, block=0, st=0, vul=0, wk=0)}
    raged = set()
    facing = first_dead = None
    weak = frail = 0
    for r in range(1, 31):
        hand, en, block = deal(draw, disc, rng), energy, 0
        k = (r - 1) % 5

        def labels():
            """(damage a hit, hits) of each crab attacking this round."""
            out = {}
            both = E['CRUSHER_0']['hp'] > 0 and E['ROCKET_0']['hp'] > 0
            face = facing if facing in E and E[facing]['hp'] > 0 else 'ROCKET_0'
            for eid, (x, hits) in (('CRUSHER_0', C_CYCLE[k][:2]), ('ROCKET_0', R_CYCLE[k])):
                if x and E[eid]['hp'] > 0:
                    x += E[eid]['st']
                    x = int(x * 1.5) if both and face != eid else x
                    out[eid] = (int(x * 0.75) if E[eid]['wk'] else x, hits)
            return out

        def state():
            lab, enemies = labels(), []
            for eid, kinds, side in (('CRUSHER_0', KIND_C, 'LEFT'), ('ROCKET_0', KIND_R, 'RIGHT')):
                e = E[eid]
                if e['hp'] > 0:
                    st = status((f"BACK_ATTACK_{side}_POWER", 1), ("CRAB_RAGE_POWER", int(eid not in raged)), ("STRENGTH_POWER", e['st']), ("VULNERABLE_POWER", e['vul']),
                                ("WEAK_POWER", e['wk']))
                    intents = [attack(*lab[eid]) if eid in lab else {"type": kinds[k], "label": ""}]
                    enemies.append({"entity_id": eid, "hp": e['hp'], "max_hp": e['mx'], "block": e['block'], "status": st, "intents": intents})
            cards = hand_cards(hand, lambda c: FACES[c](number(c, da, db, 0.75 if weak else 1, 0.75 if frail else 1)))
            pile = lambda cs: [{"name": FACES[c](0)[1], "cost": "2" if c == 'H' else "1", "description": FACES[c](number(c, da, db))[4]} for c in cs]
            return game_state("boss", {"act": 2, "floor": 33, "ascension": 7}, r, enemies, cards, pile, draw, disc, hp=hp, maxhp=maxhp, block=block, energy=en,
                              status=status(("SURROUNDED_POWER", 1), ("WEAK_POWER", weak), ("FRAIL_POWER", frail)), _facing=facing)

        for i, c, act in turn(state, hand, lambda: all(e['hp'] <= 0 for e in E.values())):
            if c in 'AHVW':
                e = E[t := target(act, E)]
                d = number(c, da, db, 0.75 if weak else 1)
                hit(e, int(d * 1.5) if e['vul'] else d)
                facing = t
                o = 'ROCKET_0' if t == 'CRUSHER_0' else 'CRUSHER_0'
                if e['hp'] == 0:
                    first_dead = first_dead or (t, r, E[o]['hp'])
                    if E[o]['hp'] > 0:  # Crab Rage
                        E[o]['st'] += 6
                        E[o]['block'] += 99
                        raged.add(o)
                elif c in 'VW':
                    e['vul' if c == 'V' else 'wk'] += 2
                en -= 2 if c == 'H' else 1
            elif c == 'B':
                block += number(c, da, db, 1, 0.75 if frail else 1)
                en -= 1
            disc.append(hand.pop(i))
            if trace: print('   play', c, act[1].get('target'), {k_: v['hp'] for k_, v in E.items()})
        if all(e['hp'] <= 0 for e in E.values()):
            return True, hp, r, first_dead
        lost = max(0, sum(x * h for x, h in labels().values()) - block)
        hp -= lost
        if trace: print(f"r{r} lost {lost} hp {hp} | C {E['CRUSHER_0']['hp']} R {E['ROCKET_0']['hp']} facing {facing}")
        disc += hand
        if hp <= 0: return False, 0, r, first_dead
        for e in E.values():  # the enemies' turn: Block goes, then what they did
            e['block'], e['vul'], e['wk'] = 0, max(0, e['vul'] - 1), max(0, e['wk'] - 1)
        _, _, cdef, cdeb = C_CYCLE[k]
        if E['CRUSHER_0']['hp'] > 0:
            E['CRUSHER_0']['block'] = 15 if cdef else 0
            E['CRUSHER_0']['st'] += 2 if KIND_C[k] == "Buff" else 0
        if E['ROCKET_0']['hp'] > 0 and KIND_R[k] == "Buff": E['ROCKET_0']['st'] += 2
        weak, frail = (2, 2) if E['CRUSHER_0']['hp'] > 0 and cdeb else (max(0, weak - 1), max(0, frail - 1))
    return False, 0, 30, first_dead


def arm_patch(arm: str):
    """A number (combat.RAGE_COST), threat=ROCKET/20/CRUSHER/12 (battle.KNOWN_THREAT), NAME=VALUE (a constant of battle.py
    or combat.py, else a knob of learn.py), or several joined by '+'."""
    if "+" in arm:
        stack = contextlib.ExitStack()
        for part in arm.split("+"):
            stack.enter_context(arm_patch(part))
        return stack
    if arm.startswith("threat="):
        parts = arm[len("threat="):].split("/")
        return mock.patch.dict(battle.KNOWN_THREAT, {parts[i]: float(parts[i + 1]) for i in range(0, len(parts), 2)})
    if "=" in arm:
        name, value = arm.split("=")
        mod = next((m for m in (battle, combat) if hasattr(m, name)), None)
        return mock.patch.object(mod, name, type(getattr(mod, name))(float(value))) if mod else mock.patch.dict(learn.KNOB, {name: float(value)})
    return mock.patch.object(combat, "RAGE_COST", float(arm))


def run(args):
    arm, dk, energy, n, seed = args
    with arm_patch(arm):
        res = [fight(dk, energy, 80, random.Random(seed * 10007 + j)) for j in range(n)]  # a seed per fight: every arm plays the same shuffles
    return arm, dk, energy, mean(res, 0), mean(res, 1), mean(res, 2), collections.Counter(o[3][0] if o[3] else None for o in res)


DECKS = [(13, 9, 8, 7, 2, 2), (16, 9, 8, 7, 2, 2), (13, 12, 8, 7, 2, 2), (16, 12, 7, 8, 2, 2), (19, 9, 8, 7, 2, 2), (22, 12, 8, 7, 2, 2)]  # damage, Block, Strikes, Defends, dead, heavy
DEBUFF_DECKS = [(d, b, s - 2, k, f, h, 1, 1) for d, b, s, k, f, h in DECKS]  # two Strikes made a Bash (half a Strike, 2 Vulnerable) and a Clothesline (2 Weak)

if __name__ == '__main__':
    logging.disable(logging.CRITICAL)
    if sys.argv[1:2] == ["trace"]:
        print(fight(DECKS[3], 4, 80, random.Random(int(sys.argv[2]) if len(sys.argv) > 2 else 1), trace=True))
    else:
        n = int(sys.argv[1]) if len(sys.argv) > 1 else 100
        arms = sys.argv[2].split(',') if len(sys.argv) > 2 else [str(combat.RAGE_COST)]
        decks = DEBUFF_DECKS if sys.argv[3:4] == ["debuff"] else DECKS
        out = on_all_cores(run, [(a, dk, 4, n, 500 + i) for a in arms for i, dk in enumerate(decks)], most=8)
        for a in arms:
            rows = [o for o in out if o[0] == a]
            f = {k: sum(o[6][k] for o in rows) for k in ('CRUSHER_0', 'ROCKET_0', None)}
            print(f"{a}: won {mean(rows, 3):.3f}  hp left {mean(rows, 4):5.1f}  rounds {mean(rows, 5):4.1f}  first to die: Crusher {f['CRUSHER_0']}, Rocket {f['ROCKET_0']}, neither {f[None]}")
        for dk in decks:
            print(dk, ' '.join(f"{o[0]}: {o[3]:.2f}" for o in out if o[1] == dk))
