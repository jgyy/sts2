"""What one scaling Power is worth at the Knowledge Demon, by perfect play (demonfight.py's exact model).

    python3 scripts/demon/scaling.py      six decks holding no Power, each with one card more of each sort (about 3 min)

Behind decks.SCALE_POWER (2026-11-22).

Adds to the model's Power slot: `pd` damage (Doom) every turn per Power in play (Countdown: 6 Doom at the start of
each turn), `pc` the Power's cost, `pm` attacks multiplied by (1 + pm) once it is in play (Reaper Form: every attack
applies as much Doom as it deals, so x2 against one enemy). Doom is counted as damage: it does not heal off and the
Demon dies once it covers its HP (at the end of its turn rather than at once: left out).
"""
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
import demonfight as D


class Deck2(D.Deck):
    def __init__(self, counts, pd=0, pc=1, pm=0.0, **kw):
        super().__init__(counts, **kw)
        self.pd, self.pc, self.pm = pd, pc, pm

    def hit(self, kind, s):
        return int(super().hit(kind, s) * (1 + self.pm * s))


def plays(deck, hand, energy, limit, s=0):
    a, h, z, b, _f, pw = hand
    pc, pd = getattr(deck, "pc", 1), getattr(deck, "pd", 0)
    seen = set()
    for n in range(min(pw, deck.powers - s) + 1):
        for i in range(a + 1):
            for j in range(h + 1):
                for k in range(b + 1):
                    if n * pc + i + 2 * j + k > energy:
                        continue
                    for zz in range(z + 1):
                        if n + i + j + k + zz <= limit:
                            dmg = i * deck.hit("A", s + n) + j * deck.hit("H", s + n) + zz * deck.hit("Z", s + n) + pd * s
                            seen.add((dmg, k * deck.db, n))
    return [p for p in seen if not any(q != p and q[2] == p[2] and q[0] >= p[0] and q[1] >= p[1] for q in seen)]


D.plays = plays

BASES = [
    ((6, 4, 2, 6, 2, 0), dict(da=15, db=11)),
    ((6, 3, 3, 6, 2, 0), dict(da=19, db=12)),
    ((7, 3, 2, 7, 3, 0), dict(da=17, db=12, maxhp=66)),
    ((6, 4, 2, 6, 2, 0), dict(da=15, db=11, energy=4)),
    ((5, 4, 3, 6, 2, 0), dict(da=15, db=11, maxhp=90)),
    ((8, 2, 2, 7, 3, 0), dict(da=13, db=10, energy=4, maxhp=66)),
]
P1 = (0, 0, 0, 0, 0, 1)
EXTRA = [
    ("nothing", (0,) * 6, {}),
    ("Strike-like attack", (1, 0, 0, 0, 0, 0), {}),
    ("two-cost attack", (0, 1, 0, 0, 0, 0), {}),
    ("Inflame (+3 Str, 1)", P1, {}),
    ("energy Power", P1, dict(pe=1, ps=0)),
    ("Countdown (6/turn, 1)", P1, dict(ps=0, pd=6)),
    ("Countdown+ (9/turn?, 1)", P1, dict(ps=0, pd=9)),
    ("Lethality (~+half a hit, 1)", P1, None),
    ("Reaper Form (x2, 3)", P1, dict(ps=0, pm=1.0, pc=3)),
    ("Reaper Form (x2, 2)", P1, dict(ps=0, pm=1.0, pc=2)),
]


def cell(args):
    counts, kw, name, add, pw = args
    if pw is None:
        pw = dict(ps=0, pd=int(0.5 * kw["da"]))
    deck = Deck2(tuple(a + b for a, b in zip(counts, add)), **kw, **pw)
    V = D.solve(deck, D.OFFERS[0])
    return str(D.Deck(counts, **kw)), name, float(V[1][0][deck.maxhp, D.BOSS])


if __name__ == "__main__":
    import multiprocessing

    jobs = [(c, kw, n, a, p) for c, kw in BASES for n, a, p in EXTRA]
    with multiprocessing.Pool(min(8, os.cpu_count() or 2)) as pool:
        out = pool.map(cell, jobs)
    last = None
    for deck, name, v in out:
        if deck != last:
            print(deck)
            last = deck
        print(f"    + {name:30} perfect {v:.3f}")
