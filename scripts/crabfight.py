"""Crusher and Rocket as a model fight, played by the bot's own planner (combat.choose_card).

    python3 scripts/crabfight.py [FIGHTS] [ARM,ARM,...]   ARM: a RAGE_COST, NAME=VALUE of a learn.py knob
                                                          (caution=1.5: the HP price; 2026-11-22 sweep below), or
                                                          threat=ROCKET/20/CRUSHER/12 (battle.KNOWN_THREAT: kill order)
    python3 scripts/crabfight.py trace [SEED]

The game side is a few rules taken from the logs: the two cycles of intents
(Crusher 12, 4, 6x2 + Weak and Frail, Buff, 12 + 15 Block; Rocket 3, 18,
Buff, 31, Sleep; each Buff 2 Strength), half again from whoever is behind us,
we face whoever we aimed at last, and a crab whose ally dies gains 6 Strength
and 99 Block until its next turn. The deck is Strikes, Defends, a few
two-cost attacks and dead cards. Everything else is the real planner, so a
change to targeting or to what a kill is worth can be tried on a few hundred
fights in two minutes before the live game sees it.

A model is only as good as its rules: the first version let a dead crab go
on attacking and said that killing one early lost fights. Trace a fight and
read it before believing a table.
"""

import logging
import multiprocessing
import os
import random
import sys
from unittest import mock

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from sts2bot import battle, combat, learn  # noqa: E402
from tests.fake_game import card  # noqa: E402

C_CYCLE = [(12, 1, False, False), (4, 1, False, False), (6, 2, False, True), (0, 0, False, False), (12, 1, True, False)]
R_CYCLE = [(3, 1), (18, 1), (0, 0), (31, 1), (0, 0)]
KIND_C = ["Attack", "Attack", "Attack", "Buff", "Attack"]
KIND_R = ["Attack", "Attack", "Buff", "Attack", "Sleep"]


def fight(deck_spec, energy, maxhp, rng, chp=206, rhp=196, trace=False):
    da, db, natk, nblk, nfill, big = deck_spec
    deck = ['A'] * natk + ['B'] * nblk + ['F'] * nfill + ['H'] * big
    rng.shuffle(deck)
    draw, disc = deck[:], []
    hp = maxhp
    E = {'CRUSHER_0': dict(hp=chp, mx=chp, block=0, st=0), 'ROCKET_0': dict(hp=rhp, mx=rhp, block=0, st=0)}
    raged = set()
    facing = None
    weak = frail = 0
    first_dead = None
    for r in range(1, 31):
        hand = []
        for _ in range(5):
            if not draw:
                draw, disc = disc, []
                rng.shuffle(draw)
            if draw: hand.append(draw.pop())
        en = energy
        block = 0
        k = (r - 1) % 5

        def labels():
            out = {}
            both = E['CRUSHER_0']['hp'] > 0 and E['ROCKET_0']['hp'] > 0
            face = facing if facing in E and E[facing]['hp'] > 0 else 'ROCKET_0'
            ch, chits, _, _ = C_CYCLE[k]
            if ch and E['CRUSHER_0']['hp'] > 0:
                x = ch + E['CRUSHER_0']['st']
                if both and face != 'CRUSHER_0': x = int(x * 1.5)
                out['CRUSHER_0'] = (x, chits)
            rh, rhits = R_CYCLE[k]
            if rh and E['ROCKET_0']['hp'] > 0:
                x = rh + E['ROCKET_0']['st']
                if both and face != 'ROCKET_0': x = int(x * 1.5)
                out['ROCKET_0'] = (x, rhits)
            return out

        def state():
            lab = labels()
            enemies = []
            for eid, kinds, side in (('CRUSHER_0', KIND_C, 'LEFT'), ('ROCKET_0', KIND_R, 'RIGHT')):
                e = E[eid]
                if e['hp'] <= 0: continue
                st = [{"id": f"BACK_ATTACK_{side}_POWER", "amount": 1}]
                if eid not in raged: st.append({"id": "CRAB_RAGE_POWER", "amount": 1})
                if e['st']: st.append({"id": "STRENGTH_POWER", "amount": e['st']})
                if eid in lab:
                    x, h = lab[eid]
                    intents = [{"type": "Attack", "label": f"{x}x{h}" if h > 1 else str(x)}]
                else:
                    intents = [{"type": kinds[k], "label": ""}]
                enemies.append({"entity_id": eid, "hp": e['hp'], "max_hp": e['mx'], "block": e['block'], "status": st, "intents": intents})
            cards = []
            for i, c in enumerate(hand):
                if c == 'A': cards.append(card(i, "STRIKE_R", "Strike", "Attack", 1, f"Deal {int(da * (0.75 if weak else 1))} damage.", "AnyEnemy"))
                elif c == 'H': cards.append(card(i, "HEAVY", "Heavy Blow", "Attack", 2, f"Deal {int(2.2 * da * (0.75 if weak else 1))} damage.", "AnyEnemy"))
                elif c == 'B': cards.append(card(i, "DEFEND_R", "Defend", "Skill", 1, f"Gain {int(db * (0.75 if frail else 1))} Block.", "Self"))
                else: cards.append(card(i, "CLUMSY", "Clumsy", "Curse", -1, "Unplayable.", "None", can_play=False, unplayable_reason="HasUnplayableKeyword"))
            pst = [{"id": "SURROUNDED_POWER", "amount": 1}]
            if weak: pst.append({"id": "WEAK_POWER", "amount": weak})
            if frail: pst.append({"id": "FRAIL_POWER", "amount": frail})
            name = {'A': "Strike", 'B': "Defend", 'F': "Clumsy", 'H': "Heavy Blow"}
            text = {'A': f"Deal {da} damage.", 'B': f"Gain {db} Block.", 'F': "Unplayable.", 'H': f"Deal {int(2.2 * da)} damage."}
            pile = lambda cs: [{"name": name[c], "cost": "2" if c == 'H' else "1", "description": text[c]} for c in cs]
            return {
                "state_type": "boss", "run": {"act": 2, "floor": 33, "ascension": 7},
                "battle": {"round": r, "turn": "player", "is_play_phase": True, "enemies": enemies},
                "player": {"hp": hp, "max_hp": maxhp, "block": block, "energy": en, "hand": cards, "potions": [], "status": pst,
                           "draw_pile": pile(draw), "discard_pile": pile(disc), "draw_pile_count": len(draw), "discard_pile_count": len(disc), "exhaust_pile_count": 0},
                "_facing": facing,
            }

        for _ in range(12):
            if all(e['hp'] <= 0 for e in E.values()): break
            act = combat.choose_card(state())
            if act is None: break
            i = act[1]["card_index"]; c = hand[i]
            if c in 'AH':
                t = act[1].get("target")
                if t is None or E[t]['hp'] <= 0: t = next(x for x in E if E[x]['hp'] > 0)
                d = int((da if c == 'A' else 2.2 * da) * (0.75 if weak else 1))
                e = E[t]
                soak = min(e['block'], d); e['block'] -= soak; e['hp'] = max(0, e['hp'] - (d - soak))
                facing = t
                if e['hp'] == 0:
                    other = E['ROCKET_0' if t == 'CRUSHER_0' else 'CRUSHER_0']
                    if first_dead is None: first_dead = (t, r, other['hp'])
                    if other['hp'] > 0:
                        o = 'ROCKET_0' if t == 'CRUSHER_0' else 'CRUSHER_0'
                        other['st'] += 6; other['block'] += 99; raged.add(o)
                en -= 1 if c == 'A' else 2
            elif c == 'B':
                block += int(db * (0.75 if frail else 1)); en -= 1
            disc.append(hand.pop(i))
            if trace: print('   play', c, act[1].get('target'), {k_: v['hp'] for k_, v in E.items()})
        if all(e['hp'] <= 0 for e in E.values()):
            return True, hp, r, first_dead
        lab = labels()
        inc = sum(x * h for x, h in lab.values())
        lost = max(0, inc - block)
        hp -= lost
        if trace: print(f"r{r} lost {lost} hp {hp} | C {E['CRUSHER_0']['hp']} R {E['ROCKET_0']['hp']} facing {facing}")
        disc += hand
        if hp <= 0: return False, 0, r, first_dead
        # the enemies' turn: Block goes, then what they did
        for e in E.values(): e['block'] = 0
        _, _, cdef, cdeb = C_CYCLE[k]
        if E['CRUSHER_0']['hp'] > 0:
            if cdef: E['CRUSHER_0']['block'] = 15
            if KIND_C[k] == "Buff": E['CRUSHER_0']['st'] += 2
        if E['ROCKET_0']['hp'] > 0 and KIND_R[k] == "Buff": E['ROCKET_0']['st'] += 2
        weak, frail = max(0, weak - 1), max(0, frail - 1)
        if E['CRUSHER_0']['hp'] > 0 and cdeb: weak = frail = 2
    return False, 0, 30, first_dead


def arm_patch(arm: str):
    """An arm is a number (combat.RAGE_COST) or NAME=VALUE with a knob of learn.py (caution=1.5: the HP price)."""
    if arm.startswith("threat="):  # threat=ROCKET/20/CRUSHER/12: battle.KNOWN_THREAT for the fight (kill order, see battle._set_focus)
        parts = arm[len("threat="):].split("/")
        return mock.patch.dict(battle.KNOWN_THREAT, {parts[i]: float(parts[i + 1]) for i in range(0, len(parts), 2)})
    if "=" in arm:
        name, value = arm.split("=")
        return mock.patch.dict(learn.KNOB, {name: float(value)})
    return mock.patch.object(combat, "RAGE_COST", float(arm))


def run(args):
    arm, dk, energy, n, seed = args
    rng = random.Random(seed)
    wins = hpl = rounds = 0
    firsts = {'CRUSHER_0': 0, 'ROCKET_0': 0, None: 0}
    with arm_patch(arm):
        for _ in range(n):
            ok, hp, r, fd = fight(dk, energy, 80, rng)
            wins += ok
            hpl += hp
            rounds += r
            firsts[fd[0] if fd else None] += 1
    return arm, dk, energy, wins / n, hpl / n, rounds / n, firsts


DECKS = [(13, 9, 8, 7, 2, 2), (16, 9, 8, 7, 2, 2), (13, 12, 8, 7, 2, 2), (16, 12, 7, 8, 2, 2), (19, 9, 8, 7, 2, 2), (22, 12, 8, 7, 2, 2)]  # damage, Block, Strikes, Defends, dead, heavy

if __name__ == '__main__':
    logging.disable(logging.CRITICAL)
    if sys.argv[1:2] == ["trace"]:
        print(fight(DECKS[3], 4, 80, random.Random(int(sys.argv[2]) if len(sys.argv) > 2 else 1), trace=True))
        sys.exit()
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 100
    arms = sys.argv[2].split(',') if len(sys.argv) > 2 else [str(combat.RAGE_COST)]
    jobs = [(a, dk, 4, n, 500 + i) for a in arms for i, dk in enumerate(DECKS)]
    with multiprocessing.Pool(min(8, os.cpu_count() or 2)) as pool:
        out = pool.map(run, jobs)
    for a in arms:
        rows = [o for o in out if o[0] == a]
        f = {k: sum(o[6][k] for o in rows) for k in ('CRUSHER_0', 'ROCKET_0', None)}
        print(f"{a}: won {sum(o[3] for o in rows) / len(rows):.3f}  hp left {sum(o[4] for o in rows) / len(rows):5.1f}  rounds {sum(o[5] for o in rows) / len(rows):4.1f}  first to die: Crusher {f['CRUSHER_0']}, Rocket {f['ROCKET_0']}, neither {f[None]}")
    for dk in DECKS:
        print(dk, ' '.join(f"{o[0]}: {o[3]:.2f}" for o in out if o[1] == dk))
