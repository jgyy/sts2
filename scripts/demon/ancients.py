"""The Ancients' every-turn relics at the Knowledge Demon, by perfect play (demonfight.py's exact model).

    python3 scripts/demon/ancients.py      eight decks (six on 4 energy or more, two on 3) under each relic (about 10 s)

Each relic is a change to the fight's rules (RELICS); the Demon's afflictions are all taken. Behind relicworth.energy_lean.
"""
import logging
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
logging.disable(logging.CRITICAL)
import demonfight as D  # noqa: E402

_rules = D.rules
ROUND = {"r": 1}  # the round being solved: `solve` asks for each round's rules before its plays
RELIC = {}  # the relic in force, as changes to the rules (set in each worker)


def rules(r, burns):
    out = dict(_rules(r, burns))
    ROUND["r"] = r
    out["draw"] += RELIC.get("draw", 0)
    out["drain"] -= RELIC.get("energy", 0) - (RELIC.get("energy", 0) if RELIC.get("from3") and r < 3 else 0) + (RELIC.get("cocoa", 0) if r == 1 else 0)
    if RELIC.get("enemy_str") and out["hits"]:
        out["inc"] += RELIC["enemy_str"] * out["hits"]
        out["per"] = out["inc"] // out["hits"]
    return out


D.rules = rules


class Deck(D.Deck):
    def hit(self, kind, s):
        return super().hit(kind, s) + RELIC.get("str_per_round", 0) * ROUND["r"]


BASES = [
    ((6, 4, 2, 6, 2, 0), dict(da=15, db=11, energy=4)),
    ((6, 3, 3, 6, 2, 0), dict(da=19, db=12)),
    ((6, 4, 2, 6, 2, 2), dict(da=13, db=10, energy=4)),
    ((6, 3, 3, 6, 2, 2), dict(da=17, db=12)),
    ((5, 4, 3, 6, 2, 3), dict(da=15, db=11, maxhp=90)),
    ((4, 3, 5, 7, 3, 3), dict(da=14, db=11, energy=4, maxhp=66)),
    ((6, 4, 2, 6, 2, 0), dict(da=15, db=11)),
    ((7, 3, 2, 7, 3, 0), dict(da=17, db=12, maxhp=66)),
]
RELICS = [  # (name, rule changes, dead cards added)
    ("none", {}, 0),
    ("+1 energy (Velvet Choker, Prismatic Gem, Sozu)", {"energy": 1}, 0),
    ("+1 card a turn (Pael's Blood)", {"draw": 1}, 0),
    ("+2 cards a turn (Snecko Eye unconfused, Fiddle)", {"draw": 2}, 0),
    ("+1 energy from turn 3 (Pael's Flesh)", {"energy": 1, "from3": 1}, 0),
    ("+4 energy on turn 1 (Very Hot Cocoa)", {"cocoa": 4}, 0),
    ("+1 energy, Demon +1 Strength (Philosopher's Stone)", {"energy": 1, "enemy_str": 1}, 0),
    ("+1 energy, a dead card (Blessed Antler, Blood-Soaked Rose)", {"energy": 1}, 1),
    ("+1 energy, two dead cards", {"energy": 1}, 2),
    ("+1 Strength a round (Toasty Mittens, no card lost)", {"str_per_round": 1}, 0),
    ("+1 Strength a round, a card less (Toasty Mittens)", {"str_per_round": 1, "draw": -1}, 0),
]
TAKEN = (False, False, False)  # every affliction taken, no burn


def job(args):
    b, v = args
    counts, kw = BASES[b]
    _name, changes, dead = RELICS[v]
    RELIC.clear()
    RELIC.update(changes)
    deck = Deck(tuple(n + (dead if k == "F" else 0) for n, k in zip(counts, D.KINDS)), **kw)
    return b, v, float(D.solve(deck, TAKEN)[1][0][deck.maxhp, D.BOSS])


if __name__ == "__main__":
    res = {(b, v): p for b, v, p in D.on_all_cores(job, [(b, v) for b in range(len(BASES)) for v in range(len(RELICS))], most=8)}
    on3 = [b for b, (_c, kw) in enumerate(BASES) if kw.get("energy", 3) == 3]
    on4 = [b for b in range(len(BASES)) if b not in on3]
    gain = lambda v, bs: 100 * sum(res[(b, v)] - res[(b, 0)] for b in bs) / len(bs)
    print("perfect play's chance of beating the Demon from full HP, by deck; then the mean gain in points on 3 and on 4+ energy")
    for v, (name, _c, _d) in enumerate(RELICS):
        print(f"{name:58s}" + "".join(f" {res[(b, v)]:.3f}" for b in range(len(BASES))) + f"   3E {gain(v, on3):+5.1f}  4E+ {gain(v, on4):+5.1f}")
