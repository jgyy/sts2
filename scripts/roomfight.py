"""An ordinary act 2 fight as a model: the bot's own planner against the play that keeps the most HP.

    python3 scripts/roomfight.py [FIGHTS A CELL]      HP lost and deaths by the planner, next to the best play
    python3 scripts/roomfight.py trace [SEED] [FOE] [DECK] [HP]
                                                      one fight, turn by turn, with the best play beside ours
    ... NAME=VALUE                                    with a constant of combat.py or closing.py, or a setting
                                                      of learn.py, set to that (caution=1.35, HP_BASE=2.0)

demonfight.py asks how often the planner wins a boss fight; an ordinary fight is nearly always won, and
what counts is the HP it leaves for the rest of the act. So here the best play is the one with the most
HP expected at the end, a death counted as DEATH HP below nothing, and the planner is measured by the HP
it gives away a fight. The enemies are three of act 2 as the logs show them on Ascension 7 (HP, and what
they hit for round by round); the decks and the drawing are demonfight's (every hand afresh from the whole
deck), so a position is the round, our HP and the enemy's. Read a trace before believing a table.
"""

import logging
import multiprocessing
import os
import random
import sys

import numpy as np

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import demonfight as demon  # noqa: E402
from demonfight import Deck, hands, plays  # noqa: E402
from sts2bot.fight import closing, combat  # noqa: E402
from sts2bot import learn  # noqa: E402

ROUNDS = 16
DEATH = 60.0  # what a death counts for, in HP below ending on none: the rest of the run

# name -> (HP, what it hits for in rounds 1, 2, 3 ..., the hits that is made of; the last three rounds go round again)
FOES = {
    "toad": (117, [0, 23, 17], [0, 1, 1]),  # Spiny Toad: Buff, 23, 17
    "hunter": (121, [0, 17, 21, 21], [0, 1, 3, 3]),  # Hunter Killer: Debuff, then 17 and 7x3
    "louse": (134, [9, 0, 19, 14, 0, 24, 19, 0, 29, 24, 0, 34, 29, 0, 39, 34], [1] * 16),  # Louse Progenitor: 5 Strength more each time round
}


def rules(foe, r):
    _hp, hit, hits = FOES[foe]
    i = r - 1 if r <= len(hit) else len(hit) - 3 + (r - 1 - len(hit)) % 3
    return dict(inc=hit[i], hits=max(1, hits[i]), per=hit[i] // max(1, hits[i]), heal=0, burn=0, strength=0, limit=99, draw=5, kind=0)


def after(V, dmg, lost):
    """HP expected at the end once a turn has dealt `dmg` and cost us `lost`, for every (our HP, the enemy's HP) before it."""
    hp, b = V.shape
    out = np.full_like(V, -DEATH)
    cols = np.arange(b)
    alive = cols > dmg
    src = np.clip(cols - dmg, 0, b - 1)
    if lost < hp:
        out[lost:, :] = np.where(alive[None, :], V[: hp - lost][:, src], 0.0)
    out[:, ~alive] = np.arange(hp)[:, None]  # it died first: we keep what we have
    out[0, :] = -DEATH
    return out


def solve(deck, foe):
    """V[r][our HP, the enemy's HP]: the HP the best play expects to end on from the start of round r (a death: -DEATH)."""
    top = FOES[foe][0]
    V = [None] * (ROUNDS + 2)
    V[ROUNDS + 1] = np.full((deck.maxhp + 1, top + 1), -DEATH)
    for r in range(ROUNDS, 0, -1):
        rl, cache = rules(foe, r), {}
        total = np.zeros_like(V[r + 1])
        for hand, p in hands(deck, rl["draw"]):
            best = None
            for dmg, blk, _n in plays(deck, hand, deck.energy, rl["limit"]):
                key = (dmg, max(0, rl["inc"] - blk))
                if key not in cache:
                    cache[key] = after(V[r + 1], *key)
                best = cache[key] if best is None else np.maximum(best, cache[key])
            total += p * best
        total[0, :] = -DEATH
        V[r] = total
    return V


def value(V, r, rl, hp, foe_hp, dmg, blk):
    if foe_hp - dmg <= 0:
        return float(hp)
    left = hp - max(0, rl["inc"] - blk)
    if left <= 0 or r + 1 > ROUNDS:
        return -DEATH
    return float(V[r + 1][left, foe_hp - dmg])


def state(deck, foe, r, rl, hp, foe_hp, hand, energy, block, played):
    st = demon.state(deck, r, rl, hp, foe_hp, hand, energy, block, played)
    st["state_type"] = "monster"
    st["run"]["floor"] = 24
    enemy = st["battle"]["enemies"][0]
    enemy["entity_id"], enemy["max_hp"] = "MODEL_FOE_0", FOES[foe][0]
    return st


def fight(deck, foe, hp, V, rng, trace=False):
    """One fight by the planner: (HP left or None for a death, rounds, HP given away turn by turn)."""
    foe_hp, given = FOES[foe][0], []
    for r in range(1, ROUNDS + 1):
        rl = rules(foe, r)
        hand = rng.choices(demon.KINDS, weights=deck.weights(0), k=rl["draw"])
        counts = tuple(hand.count(k) for k in demon.KINDS)
        energy, block, dealt, played = deck.energy, 0, 0, 0
        shown = "".join(sorted(hand))
        for _ in range(12):
            if foe_hp - dealt <= 0:
                break
            act = combat.choose_card(state(deck, foe, r, rl, hp, foe_hp - dealt, hand, energy, block, played))
            if act is None:
                break
            c = hand.pop(act[1]["card_index"])
            played += 1
            if c == "B":
                block += deck.db
                energy -= 1
            else:
                dealt += deck.hit(c, 0)
                energy -= {"A": 1, "H": 2, "Z": 0}[c]
        ours = value(V, r, rl, hp, foe_hp, dealt, block)
        best = max((value(V, r, rl, hp, foe_hp, d, b), d, b) for d, b, _n in plays(deck, counts, deck.energy, rl["limit"]))
        given.append((r, max(0.0, best[0] - ours), dealt - best[1], block - best[2], rl["inc"], hp / deck.maxhp))
        if trace:
            print(f"r{r:2} hp {hp:3} foe {foe_hp:3} coming {rl['inc']:2} hand {shown:5} | ours {dealt:2} dmg {block:2} blk ({ours:6.2f})  best {best[1]:2} dmg {best[2]:2} blk ({best[0]:6.2f})")
        foe_hp -= dealt
        if foe_hp <= 0:
            return hp, r, given
        hp -= max(0, rl["inc"] - block)
        if hp <= 0:
            return None, r, given
    return None, ROUNDS, given


DECKS = [  # act 2 decks as the bot brings them: about 26 enemy HP a round, an ordinary fight of four or five rounds
    Deck((6, 3, 2, 6, 3, 0), da=9, db=8, maxhp=75),
    Deck((6, 3, 2, 6, 3, 0), da=11, db=9, maxhp=75),
    Deck((7, 4, 2, 4, 3, 0), da=10, db=8, maxhp=75),
    Deck((5, 2, 2, 8, 3, 0), da=10, db=9, maxhp=75),
]
STARTS = [1.0, 0.6, 0.35]  # the share of Max HP the fight is entered on
PATCH = {}


def patch(values):
    for k, v in values.items():
        if k in learn.DEFAULTS:
            learn.KNOB[k] = v
        else:
            setattr(closing if hasattr(closing, k) else combat, k, v)


def job(args):
    d, foe, n, seed, values = args
    patch(values)
    deck = DECKS[d]
    V = solve(deck, foe)
    out = {}
    for share in STARTS:
        hp = max(1, round(share * deck.maxhp))
        rng = random.Random(seed)
        out[share] = (float(V[1][hp, FOES[foe][0]]), hp, [fight(deck, foe, hp, V, rng) for _ in range(n)])
    return d, foe, out


def run(n, values=None):
    jobs = [(d, foe, n, 500 + 31 * d + i, values if values is not None else PATCH) for d in range(len(DECKS)) for i, foe in enumerate(FOES)]
    with multiprocessing.Pool(min(8, os.cpu_count() or 2)) as pool:
        return pool.map(job, jobs)


def table(out):
    print(f"each cell: HP the planner lost a fight / the best play's, and deaths in a hundred (a death counts as all the HP and {DEATH:.0f} more)")
    tot = {s: [0.0, 0.0, 0, 0, 0.0] for s in STARTS}
    for share in STARTS:
        print(f"entered on {share:.0%} of Max HP")
        for d, deck in enumerate(DECKS):
            cells = []
            for dd, foe, res in out:
                if dd != d:
                    continue
                best, hp, fights = res[share]
                lost = sum(hp - (f[0] if f[0] is not None else -DEATH) for f in fights) / len(fights)
                dead = sum(f[0] is None for f in fights)
                cells.append(f"{foe} {lost:5.1f}/{hp - best:5.1f} †{100 * dead / len(fights):3.0f}")
                t = tot[share]
                t[0] += lost * len(fights)
                t[1] += (hp - best) * len(fights)
                t[2] += dead
                t[3] += len(fights)
                t[4] += sum(f[1] for f in fights)
            print(f"  {str(deck):44} " + "   ".join(cells))
    for share, (lost, best, dead, n, rounds) in tot.items():
        print(f"all, entered on {share:.0%}: planner lost {lost / n:.2f} a fight, the best play {best / n:.2f}; deaths {100 * dead / n:.1f}%; rounds {rounds / n:.2f}")


if __name__ == "__main__":
    logging.disable(logging.CRITICAL)
    for arg in [a for a in sys.argv[1:] if "=" in a]:
        PATCH[arg.split("=")[0]] = float(arg.split("=")[1])
        sys.argv.remove(arg)
    if sys.argv[1:2] == ["trace"]:
        patch(PATCH)
        seed, foe, d, share = (sys.argv[2:] + [1, "toad", 0, 1.0][len(sys.argv) - 2 :])[:4]
        deck = DECKS[int(d)]
        hp = max(1, round(float(share) * deck.maxhp))
        V = solve(deck, foe)
        print(deck, foe, "the best play ends on", round(float(V[1][hp, FOES[foe][0]]), 2), "of", hp)
        print(fight(deck, foe, hp, V, random.Random(int(seed)), trace=True)[:2])
    else:
        table(run(int(sys.argv[1]) if len(sys.argv) > 1 else 100))
