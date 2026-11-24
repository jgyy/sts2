"""The Knowledge Demon as a model fight: the bot's own planner against the best play there is.

    python3 scripts/demonfight.py [FIGHTS A DECK]      won by the planner, next to what perfect play wins
    python3 scripts/demonfight.py regret [FIGHTS]      the turns where the planner gave most away, by kind
    python3 scripts/demonfight.py trace [SEED] [DECK]  one fight, turn by turn, with the best play beside ours
    python3 scripts/demonfight.py energy [FIGHTS]      the same decks on 3 and on 4 energy a turn (what an energy relic is worth here)
    python3 scripts/demonfight.py cards [FIGHTS]       one more card of each sort, an energy-a-turn Power among them (decks.ENERGY_POWER)
    ... NAME=VALUE                                     with a constant of combat.py or closing.py set to that (CLOSE_REACH=0)

The Demon is the Ascension 7 logs' (`rules`; its three afflictions are settings of the fight). Every hand is drawn afresh
from the whole deck, so a position is the round, our HP and the Demon's: `solve` works out perfect play exactly, and each
of the planner's turns is priced by the chance of winning it gave away.
"""

import itertools
import logging
import math
import random
import sys

import numpy as np

from modelfight import game_state, hand_cards, on_all_cores, piles, status, turn
from sts2bot.fight import closing, combat

BOSS = 379
ROUNDS = 24
KINDS = "AHZBFP"  # Strike, two-cost attack, free attack, Defend, dead card, Power (1 energy: 3 Strength for the rest of the fight)
GAIN = 3  # Strength a Power gives: that much more on every attack card


class Deck:
    def __init__(self, counts, da=9, db=8, dz=7, maxhp=75, energy=3, pe=0, ps=GAIN):
        self.counts = dict(zip(KINDS, counts))
        self.da, self.db, self.dz, self.dh = da, db, dz, int(2.2 * da)
        self.maxhp, self.energy = maxhp, energy
        self.powers = self.counts["P"]
        self.pe, self.ps = pe, ps  # a Power in play: `ps` Strength on every attack card (Inflame: 3), `pe` energy a turn (Demesne: 1)

    def turn_energy(self, s, drain=0):
        """Energy a turn starts with, `s` Powers in play."""
        return self.energy - drain + self.pe * s

    def weights(self, s):
        """The chance of each kind in a draw, with `s` Powers already in play (they have left the deck)."""
        left = [n - (s if k == "P" else 0) for k, n in self.counts.items()]
        return [n / sum(left) for n in left]

    def hit(self, kind, s):
        return max(0, {"A": self.da, "H": self.dh, "Z": self.dz}[kind] + self.ps * s)

    def __repr__(self):
        power = f" P=+{self.pe}E{self.ps:+d}S" if self.pe else ""
        return f"{''.join(f'{k}{n}' for k, n in self.counts.items())} hit {self.da} block {self.db} hp {self.maxhp} energy {self.energy}{power}"


def rules(r, burns):
    """Round `r` under the afflictions taken (`burns`: for each of the three offers, whether the burn was taken)."""
    s, kind = 2 * ((r - 1) // 4), (r - 1) % 4
    inc, hits = (0, 17 + s, 3 * (8 + s), 11 + s)[kind], (0, 1, 3, 1)[kind]
    burn = (6 if burns[0] and r >= 2 else 0) + (7 if burns[1] and r >= 6 else 0) + (8 if burns[2] and r >= 10 else 0)
    return dict(kind=kind, inc=inc, hits=hits, per=inc // max(1, hits), strength=s, heal=30 if kind == 3 else 0, burn=burn,
                draw=5 - (1 if not burns[0] and r >= 2 else 0), limit=3 if not burns[1] and r >= 6 else 99, drain=1 if not burns[2] and r >= 10 else 0)


def hands(deck, n, s=0):
    """Every hand of `n` cards as counts by kind, with its chance (`s` Powers in play)."""
    out, weights = [], deck.weights(s)
    for combo in itertools.product(range(n + 1), repeat=len(KINDS)):
        if sum(combo) != n:
            continue
        p = math.factorial(n)
        for c, q in zip(combo, weights):
            p *= q**c / math.factorial(c)
        if p > 0:
            out.append((combo, p))
    return out


def plays(deck, hand, energy, limit, s=0):
    """What a hand can do: the (damage, Block, Powers played) no other play with as many Powers beats on both counts.
    (A Power played counts for the attacks of later turns, not of this one: the order within a turn is left out.)"""
    a, h, z, b, _f, pw = hand
    seen = set()
    for n, i, j, k in itertools.product(range(min(pw, deck.powers - s) + 1), range(a + 1), range(h + 1), range(b + 1)):  # (a Power beyond the deck's is dead)
        if n + i + 2 * j + k <= energy:
            for zz in range(z + 1):
                if n + i + j + k + zz <= limit:
                    seen.add((i * deck.hit("A", s + n) + j * deck.hit("H", s + n) + zz * deck.hit("Z", s + n), k * deck.db, n))
    return [p for p in seen if not any(q != p and q[2] == p[2] and q[0] >= p[0] and q[1] >= p[1] for q in seen)]


def after(V, dmg, lost, heal):
    """Chance of winning once a turn has dealt `dmg` and cost us `lost`, for every (our HP, the Demon's HP) before it."""
    hp, b = V.shape
    out = np.zeros_like(V)
    cols = np.arange(b)
    src = np.minimum(BOSS, cols - dmg + heal)
    alive = cols > dmg
    if lost < hp:
        rows = V[: hp - lost] if lost else V
        out[lost:, :] = np.where(alive[None, :], rows[:, np.clip(src, 0, b - 1)], 1.0)
    out[: min(lost, hp), :] = np.where(alive[None, :], 0.0, 1.0)  # dead unless the Demon died first
    out[0, :] = 0.0
    return out


def solve(deck, burns):
    """V[r][Powers in play][our HP, the Demon's HP]: the chance that perfect play wins from the start of round r."""
    V = [None] * (ROUNDS + 2)
    V[ROUNDS + 1] = [np.zeros((deck.maxhp + 1, BOSS + 1)) for _ in range(deck.powers + 1)]
    for r in range(ROUNDS, 0, -1):
        rl, cache, V[r] = rules(r, burns), {}, []
        for s in range(deck.powers + 1):
            total = np.zeros_like(V[r + 1][0])
            for hand, p in hands(deck, rl["draw"], s):
                best = None
                for dmg, blk, n in plays(deck, hand, deck.turn_energy(s, rl["drain"]), rl["limit"], s):
                    lost = max(0, rl["inc"] + rl["burn"] - blk)
                    key = (dmg, lost, s + n)
                    if key not in cache:
                        cache[key] = after(V[r + 1][s + n], dmg, lost, rl["heal"])
                    best = cache[key] if best is None else np.maximum(best, cache[key])
                total += p * best
            total[0, :] = 0.0
            V[r].append(total)
    return V


def value(V, r, rl, hp, boss, dmg, blk, s=0):
    """The chance of winning after this turn's play (`s`: Powers in play once it is over)."""
    if boss - dmg <= 0:
        return 1.0
    left = hp - max(0, rl["inc"] + rl["burn"] - blk)
    if left <= 0 or r + 1 > ROUNDS:
        return 0.0
    return float(V[r + 1][s][left, min(BOSS, boss - dmg + rl["heal"])])


def state(deck, r, rl, hp, boss, hand, energy, block, played, s=0):
    text = {"A": ("STRIKE_R", "Strike", "Attack", 1, f"Deal {deck.hit('A', s)} damage.", "AnyEnemy"), "H": ("HEAVY", "Heavy Blow", "Attack", 2, f"Deal {deck.hit('H', s)} damage.", "AnyEnemy"),
            "Z": ("FLICK", "Flick", "Attack", 0, f"Deal {deck.hit('Z', s)} damage.", "AnyEnemy"), "B": ("DEFEND_R", "Defend", "Skill", 1, f"Gain {deck.db} Block.", "Self"),
            "P": ("INFLAME", "Inflame", "Power", 1, f"Gain {deck.ps} Strength.", "Self") if not deck.pe else
                 ("FRIENDSHIP", "Friendship", "Power", 1, (f"Lose {-deck.ps} Strength. " if deck.ps < 0 else "") + "Gain [necrobinder_energy_icon.png] at the start of each turn.", "Self"),
            "F": ("CLUMSY", "Clumsy", "Curse", -1, "Unplayable.", "None")}
    intents = [{"type": "Debuff", "label": ""}] if not rl["inc"] else [{"type": "Attack", "label": f"{rl['per']}x{rl['hits']}" if rl["hits"] > 1 else str(rl["inc"])}]
    if rl["heal"]:
        intents += [{"type": "Heal", "label": ""}, {"type": "Buff", "label": ""}]
    pst = status(("STRENGTH_POWER", deck.ps * s), ("FRIENDSHIP_POWER", s if deck.pe else 0), ("DISINTEGRATION_POWER", rl["burn"]))
    pst += [{"id": "SLOTH_POWER", "amount": played}] if rl["limit"] < 99 else []
    pile = [k for k, n in deck.counts.items() if k != "F" for _ in range(n - (s if k == "P" else 0))]
    enemies = [{"entity_id": "KNOWLEDGE_DEMON_0", "hp": boss, "max_hp": BOSS, "block": 0, "status": status(("STRENGTH_POWER", rl["strength"])), "intents": intents}]
    return game_state("boss", {"act": 2, "floor": 33, "ascension": 7}, r, enemies, hand_cards(hand, text.get), piles(text.get), pile, [], hp=hp, maxhp=deck.maxhp,
                      block=block, energy=energy, status=pst, _plays_this_turn=played)


def fight(deck, burns, V, rng, trace=False):
    """One fight by the planner: (won, rounds, [(round, kind, HP share, the Demon's HP, chance given away, damage and Block beyond the best play's)])."""
    hp, boss, s = deck.maxhp, BOSS, 0
    turns = []
    for r in range(1, ROUNDS + 1):
        rl = rules(r, burns)
        hand = rng.choices(KINDS, weights=deck.weights(s), k=rl["draw"])
        while hand.count("P") > deck.powers - s:
            hand[hand.index("P")] = "F"
        counts = tuple(hand.count(k) for k in KINDS)
        energy, block, dealt, played, s0 = deck.turn_energy(s, rl["drain"]), 0, 0, 0, s
        shown = "".join(sorted(hand))
        for i, c, _act in turn(lambda: state(deck, r, rl, hp, boss - dealt, hand, energy, block, played, s), hand, lambda: boss - dealt <= 0):
            hand.pop(i)
            played += 1
            if c == "B":
                block += deck.db
            elif c == "P":
                s += 1
            else:
                dealt += deck.hit(c, s)
            energy -= {"A": 1, "H": 2, "Z": 0, "B": 1, "P": 1}[c]
        ours = value(V, r, rl, hp, boss, dealt, block, s)
        # (The planner may play its Power before its attacks and so do a little better than any play listed: no chance given away then.)
        best = max((value(V, r, rl, hp, boss, d, b, s0 + n), d, b, n) for d, b, n in plays(deck, counts, deck.turn_energy(s0, rl["drain"]), rl["limit"], s0))
        turns.append((r, rl["kind"], hp / deck.maxhp, boss, max(0.0, best[0] - ours), dealt - best[1], block - best[2], rl["inc"] + rl["burn"], (s - s0) - best[3], counts[5]))
        if trace:
            print(f"r{r:2} hp {hp:3} demon {boss:3} coming {rl['inc'] + rl['burn']:2} hand {shown:5} | ours {dealt:2} dmg {block:2} blk {s - s0} P ({ours:.3f})  best {best[1]:2} dmg {best[2]:2} blk {best[3]} P ({best[0]:.3f})")
        boss -= dealt
        if boss <= 0:
            return True, r, turns
        hp -= max(0, rl["inc"] + rl["burn"] - block)
        if hp <= 0:
            return False, r, turns
        boss = min(BOSS, boss + rl["heal"])
    return False, ROUNDS, turns


DECKS = [  # tuned until perfect play wins between a fifth and four fifths: a deck of plain Strikes and Defends never does
    Deck((6, 4, 2, 6, 2, 0), da=15, db=11, energy=4),
    Deck((6, 3, 3, 6, 2, 0), da=19, db=12),
    Deck((6, 4, 2, 6, 2, 2), da=13, db=10, energy=4),
    Deck((6, 3, 3, 6, 2, 2), da=17, db=12),
    Deck((5, 4, 3, 6, 2, 3), da=15, db=11, maxhp=90),
    Deck((4, 3, 5, 7, 3, 3), da=14, db=11, energy=4, maxhp=66),
]
OFFERS = [(False, False, False), (False, False, True)]  # the burn taken at each offer, or not
PATCH = {}  # name -> value set on sts2bot.combat in every worker (to try a planner constant)


def take_patches(into: dict) -> None:
    """Move each NAME=VALUE of the command line into `into`."""
    for arg in [a for a in sys.argv[1:] if "=" in a]:
        into[arg.split("=")[0]] = float(arg.split("=")[1])
        sys.argv.remove(arg)


def set_constants(values: dict) -> None:
    for k, v in values.items():
        setattr(closing if hasattr(closing, k) else combat, k, v)


def job(args):
    d, burns, n, seed, patch = args
    set_constants(patch)
    deck = DECKS[d]
    V = solve(deck, burns)
    rng = random.Random(seed)
    return d, burns, float(V[1][0][deck.maxhp, BOSS]), [fight(deck, burns, V, rng) for _ in range(n)]


def run(n, offers=OFFERS, patch=None):
    return on_all_cores(job, [(d, burns, n, 900 + 17 * d + i, patch or PATCH) for d in range(len(DECKS)) for i, burns in enumerate(offers)], most=8)


def table(out, n):
    name = lambda burns: "".join("b" if x else w for x, w in zip(burns, "MSW"))
    print("afflictions: M Mind Rot, S Sloth, W Waste Away, b the burn instead; each cell: planner won / perfect play wins")
    for d, deck in enumerate(DECKS):
        print(f"{str(deck):48} " + "  ".join(f"{name(b)} {sum(f[0] for f in fights) / n:.2f}/{best:.2f}" for dd, b, best, fights in out if dd == d))
    won = sum(f[0] for _d, _b, _best, fights in out for f in fights) / sum(len(fights) for *_x, fights in out)
    given = sum(t[4] for *_x, fights in out for _w, _r, turns in fights for t in turns) / sum(len(fights) for *_x, fights in out)
    print(f"all: planner {won:.3f}, perfect play {sum(best for _d, _b, best, _f in out) / len(out):.3f}, chance given away a fight {given:.4f}")


def regret(out):
    rows = [t for _d, _b, _best, fights in out for _won, _r, turns in fights for t in turns]
    fights = sum(len(f) for *_x, f in out)
    print(f"chance of winning given away a fight: {sum(t[4] for t in rows) / fights:.3f} over {len(rows) / fights:.1f} turns")
    kind = ["Debuff (nothing coming)", "17", "8x3", "11 and the heal"]
    band = lambda t: "HP 70%+" if t[2] >= 0.7 else "HP 40-70%" if t[2] >= 0.4 else "HP under 40%"
    lean = lambda t: "held a Power the best play plays" if t[8] < 0 else "played a Power the best play holds" if t[8] > 0 else "blocked more" if t[6] > 0 else "blocked less" if t[6] < 0 else "same Block, less damage" if t[5] < 0 else "as the best"
    for title, key in (("by what the Demon was doing", lambda t: kind[t[1]]), ("by our HP", band), ("by what the planner did beside the best play", lean),
                       ("by both", lambda t: f"{band(t):13} {kind[t[1]]:24} {lean(t)}")):
        print(f"\n{title}")
        groups = {}
        for t in rows:
            groups.setdefault(key(t), []).append(t[4])
        for k, v in sorted(groups.items(), key=lambda kv: -sum(kv[1])):
            if sum(v) / fights >= 0.002:
                print(f"  {k:60} turns a fight {len(v) / fights:5.2f}  given away a fight {sum(v) / fights:.3f}  a turn {sum(v) / len(v):.3f}")


def energy_table(n):
    """The same deck on 3 and on 4 energy a turn, and what the energy is worth at the Demon's door (relicworth.py)."""
    offers = OFFERS[0]
    print("perfect play and the planner, the same deck on 3 and 4 energy a turn (relicworth.py's energy relics):")
    for counts, kw in (((6, 4, 2, 6, 2, 0), dict(da=15, db=11)), ((6, 3, 3, 6, 2, 2), dict(da=17, db=12))):
        for energy in (3, 4):
            deck = Deck(counts, energy=energy, **kw)
            V = solve(deck, offers)
            rng = random.Random(7)
            won = sum(fight(deck, offers, V, rng)[0] for _ in range(n)) / n
            print(f"  {str(deck):52} perfect {float(V[1][0][deck.maxhp, BOSS]):.3f}  planner {won:.3f}")
    deck = Deck((6, 4, 2, 6, 2, 0), da=15, db=11, energy=4)
    V = solve(deck, offers)
    print("  4 energy, perfect play by HP at the Demon's door:", {h: round(float(V[1][0][h, BOSS]), 3) for h in (50, 60, 75)})


def card_worth(n):
    """One more card of each sort at the Demon, by perfect play (and the planner on `n` fights), in decks with no Power yet."""
    offers = OFFERS[0]
    bases = [((6, 4, 2, 6, 2, 0), dict(da=15, db=11)), ((6, 3, 3, 6, 2, 0), dict(da=19, db=12)), ((7, 3, 2, 7, 3, 0), dict(da=17, db=12, maxhp=66))]
    extra = [("nothing", (0, 0, 0, 0, 0, 0), {}), ("Strike-like attack", (1, 0, 0, 0, 0, 0), {}), ("two-cost attack", (0, 1, 0, 0, 0, 0), {}),
             ("free attack", (0, 0, 1, 0, 0, 0), {}), ("Defend-like", (0, 0, 0, 1, 0, 0), {}), ("Inflame (+3 Strength)", (0, 0, 0, 0, 0, 1), {}),
             ("energy Power (Demesne, Pyre)", (0, 0, 0, 0, 0, 1), dict(pe=1, ps=0)), ("Friendship (+1 energy, -2 Strength)", (0, 0, 0, 0, 0, 1), dict(pe=1, ps=-2))]
    print("perfect play's chance of beating the Demon from full HP, the deck as it is and with one card more (planner beside it)")
    for counts, kw in bases:
        print(f"  {Deck(counts, **kw)}")
        for name, add, pw in extra:
            deck = Deck(tuple(a + b for a, b in zip(counts, add)), **kw, **pw)
            V = solve(deck, offers)
            rng = random.Random(11)
            won = sum(fight(deck, offers, V, rng)[0] for _ in range(n)) / n if n else float("nan")
            print(f"    + {name:38} perfect {float(V[1][0][deck.maxhp, BOSS]):.3f}  planner {won:.3f}")


if __name__ == "__main__":
    logging.disable(logging.CRITICAL)
    take_patches(PATCH)
    count = lambda default: int(sys.argv[2]) if len(sys.argv) > 2 else default
    if sys.argv[1:2] == ["trace"]:
        set_constants(PATCH)
        deck, burns = DECKS[int(sys.argv[3]) if len(sys.argv) > 3 else 0], OFFERS[0]
        V = solve(deck, burns)
        print(deck, "perfect play wins", round(float(V[1][0][deck.maxhp, BOSS]), 3))
        print(fight(deck, burns, V, random.Random(count(1)), trace=True)[:2])
    elif sys.argv[1:2] == ["regret"]:
        regret(run(count(100)))
    elif sys.argv[1:2] == ["cards"]:
        card_worth(count(0))
    elif sys.argv[1:2] == ["energy"]:
        energy_table(count(400))
    else:
        n = int(sys.argv[1]) if len(sys.argv) > 1 else 100
        table(run(n), n)
