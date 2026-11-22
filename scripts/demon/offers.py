"""The Knowledge Demon's afflictions, priced by perfect play across a grid of decks (what fixed afflictions.SLOTH_MARGIN).

    python3 scripts/demon/offers.py [2|3]

Offer 2 (default) compares Sloth with the burn after Mind Rot; offer 3 compares Waste Away with the burn after
Mind Rot and Sloth. Each deck is solved exactly by scripts/demonfight.py's `solve` (about a second a solve), for
energy 3 to 7, hands 1 to 3 cards bigger than the model's five (the Necrobinder's Souls and draws), four mixes of
cards and six damage levels; decks that perfect play wins under 4% or over 97% either way are left out. Next to
each is the statistic the bot itself measures, `afflictions.lost_to_sloth`: cards a turn beyond three, here worked
out for a hand of Mind Rot's size by the play that gives most damage + Block with no limit.

Result (2026-11-22, offer 2, 207 decks): Sloth is better up to 1.75 (48 decks of 49 from 1.25, 7 points a deck), they
tie to 2.0, the burn is better from 2.0 (+2 points, +10 from 2.25), at any energy. Offer 3: Waste Away is better in 129 of 134 decks.
"""

import itertools
import logging
import os
import sys
from multiprocessing import Pool

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
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
        if max(a, b) < 0.04 or min(a, b) > 0.97:
            continue
        rows.append((lost(deck, 4 + bonus), mix, e, bonus, da, a, b))
    for l, mix, e, bonus, da, a, b in sorted(rows):
        print(f"{l:5.2f}  {''.join(map(str, mix))} e{e} +{bonus} da{da:2d} | MSW {a:.3f}  {'MbW' if OFFER == 2 else 'MSb'} {b:.3f}")
