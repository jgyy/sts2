"""The Knowledge Demon's afflictions, priced by perfect play across a grid of decks (what fixed afflictions.SLOTH_MARGIN).

    python3 scripts/demon/offers.py [2|3]

Offer 2 (default) sets Sloth against the burn after Mind Rot; offer 3 Waste Away against the burn after Mind Rot and
Sloth. Decks perfect play wins under 4% or over 97% either way are left out; beside each is the bot's own measure,
`afflictions.lost_to_sloth`: cards a turn beyond three the best play with no limit would use.
"""

import itertools
import logging
import os
import sys
from multiprocessing import Pool

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
logging.disable(logging.CRITICAL)
import demonfight as d  # noqa: E402

OFFER = int(sys.argv[1]) if len(sys.argv) > 1 else 2
BONUS = 0  # cards drawn beyond the model's five, set in each worker
_rules = d.rules


def rules(r, burns):
    out = _rules(r, burns)
    out["draw"] += BONUS
    return out


d.rules = rules


def lost(deck, draw):
    """Cards a turn beyond three the unlimited best play would use (the bot's `lost_to_sloth`, at most 3 a turn)."""
    total = 0.0
    for hand, p in d.hands(deck, draw, 0):
        a, h, z, b, _f, _pw = hand
        best = None
        for i, j, k in itertools.product(range(a + 1), range(h + 1), range(b + 1)):
            if i + 2 * j + k > deck.energy:
                continue
            n = i + j + k + z
            key = (i * deck.hit("A", 0) + j * deck.hit("H", 0) + z * deck.hit("Z", 0) + k * deck.db, -n)
            if best is None or key > best[0]:
                best = (key, n)
        total += p * min(3, max(0, best[1] - 3))
    return total


def job(args):
    global BONUS
    i, bonus, deck, burns = args
    BONUS = bonus
    return i, burns, float(d.solve(deck, burns)[1][0][deck.maxhp, d.BOSS])


if __name__ == "__main__":
    mixes = [(4, 3, 5, 6, 3, 0), (3, 3, 7, 5, 3, 0), (5, 2, 4, 7, 3, 0), (3, 2, 9, 4, 3, 0)]
    meta = [(mix, e, bonus, da, d.Deck(mix, da=da, db=int(da * 0.8), energy=e, maxhp=66))
            for mix in mixes for e in (3, 4, 5, 6, 7) for bonus in (0, 1, 2, 3) for da in (8, 10, 12, 14, 17, 20)]
    base = (False, False, False)  # Mind Rot, Sloth, Waste Away
    other = (False, True, False) if OFFER == 2 else (False, False, True)
    with Pool(os.cpu_count() or 4) as pool:
        out = pool.map(job, [(i, m[2], m[4], b) for i, m in enumerate(meta) for b in (base, other)], chunksize=4)
    res = {}
    for i, b, v in out:
        res.setdefault(i, {})[b] = v
    print(f"offer {OFFER}: MSW (Mind Rot, Sloth, Waste Away) against {'MbW (the burn at offer 2)' if OFFER == 2 else 'MSb (the burn at offer 3)'}")
    rows = []
    for i, (mix, e, bonus, da, deck) in enumerate(meta):
        a, b = res[i][base], res[i][other]
        if not (max(a, b) < 0.04 or min(a, b) > 0.97):
            rows.append((lost(deck, 4 + bonus), mix, e, bonus, da, a, b))
    for l, mix, e, bonus, da, a, b in sorted(rows):
        print(f"{l:5.2f}  {''.join(map(str, mix))} e{e} +{bonus} da{da:2d} | MSW {a:.3f}  {'MbW' if OFFER == 2 else 'MSb'} {b:.3f}")
