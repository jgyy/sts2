"""What the model runs (sim/) do with each card, set against its text: every card with a drawback or a pile clause played in a
model fight, and what changed (our HP, powers and piles, the enemy's powers, Osty, orb slots); then the cards whose Strength or
Dexterity differ from what the planner's parse says. A clause that changes nothing is one the engine leaves out, and the card
table (`sim prior`) then learns the model's bug (2026-11-26: Grand Finale playable with a full draw pile, Bone Shards' Osty
never dying, Fight Me's Strength for the enemy, Fight Through's Wounds; the first game's Reprogram counted twice).

    python3 scripts/audit/simcards.py [sts2|sts1] [CARD ...]

Clauses that need a pile to act on (a card from the discard pile, next turn) are played with a few cards in each pile.
"""

from __future__ import annotations

import importlib
import logging
import re
import sys
from collections import Counter
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from sts2bot import games, learn
from sts2bot.cards import parse

CLAUSE = re.compile(r"enem(?:y|ies) gains? \d+ strength|osty dies|he dies|can only be played|gain \d+ (?:vulnerable|weak|frail)|lose \d+ (?:strength|dexterity|focus)|"
                    r"orb slot|(?:shuffle|add|put|place)\b[^.]*\binto your|copy of this card|lose \d+ hp|end your turn|this turn", re.I)
FOE = {"sts2": ("NIBBIT", 0), "sts1": ("JAW_WORM", "monster")}


def fight(mod, game: str, character: str):
    from sim.engine import Fight
    from sim.run import Run
    run = Run(mod, character, 1, {}, 0)
    run.act_id = "OVERGROWTH"
    run.note_deck()
    key, kind = FOE[game]
    f = Fight(run, [mod._new(key, 0, run, kind)], "monster")
    f.start()
    f.player_turn_start()
    f.energy = 6
    if f.osty >= 0:
        f.osty = 5
    starter = [k for k in mod.CHARACTERS[character]["deck"]]
    f.discard.extend(mod.make(k) for k in starter[:3])
    return f


def owner(mod, d) -> str:
    chars = list(mod.CHARACTERS)
    return next((c for c in chars if str(d.color).upper() in (c, c.replace("THE_", ""))), chars[0])


def played(mod, game: str, key: str) -> str:
    from sim.cards import Card
    d = mod.CARDS[key]
    f = fight(mod, game, owner(mod, d))
    snap = lambda: (f.run.hp, dict(f.pw), dict(f.temp), dict(f.foes[0].powers), f.osty, f.orb_slots, Counter(c.key for c in f.hand + f.draw + f.discard),
                    len(f.hand), len(f.discard), len(f.draw))
    before = snap()
    c = Card(d)
    f.hand.append(c)
    if not f.playable(c):
        return "not playable"
    f.play(len(f.hand) - 1, f.foes[0].eid)
    after = snap()
    out = []
    names = ("hp", "powers", "this turn", "enemy", "osty", "orb slots")
    for name, a, b in zip(names, before, after):
        if isinstance(a, dict):
            out += [f"{name}.{k} {a.get(k)}->{b.get(k)}" for k in sorted(set(a) | set(b)) if a.get(k) != b.get(k) and k != "DRAWN_TURN"]
        elif a != b:
            out.append(f"{name} {a}->{b}")
    made = after[6] - before[6]
    made[key] -= 1
    if +made:
        out.append(f"cards made {dict(+made)}")
    out.append(f"hand {before[7]}->{after[7]} discard {before[8]}->{after[8]} draw {before[9]}->{after[9]}")
    return "; ".join(out)


def doubled(mod, game: str):
    """Cards whose Strength or Dexterity in the model is not what the parse reads."""
    from sim.cards import Card
    for key, d in sorted(mod.CARDS.items()):
        if d.type in ("Status", "Curse", "Power") or not re.search(r"\b(strength|dexterity)\b", d.text, re.I):
            continue
        f = fight(mod, game, owner(mod, d))
        c = Card(d)
        i = parse(c.view(0))
        s0, d0 = f.strength(), f.dexterity()
        f.hand.append(c)
        if f.playable(c):
            f.play(len(f.hand) - 1, f.foes[0].eid)
            ds, dd = f.strength() - s0, f.dexterity() - d0
            if (i.strength and ds != i.strength) or (i.dexterity and dd != i.dexterity):
                yield key, i.strength, i.dexterity, ds, dd, d.text


def main(argv: list[str]) -> int:
    game = argv[0] if argv and argv[0] in FOE else "sts2"
    keys = [a.upper() for a in argv[1:] if a not in FOE] or None
    logging.disable(logging.CRITICAL)
    learn.KNOB.update(learn.DEFAULTS)
    games.select(game)
    mod = importlib.import_module(f"sim.content.{game}")
    pools = set(mod.CHARACTERS) | {c.replace("THE_", "") for c in mod.CHARACTERS} | {"RED", "GREEN", "BLUE", "PURPLE", "COLORLESS"}
    for key in keys or sorted(k for k, d in mod.CARDS.items() if CLAUSE.search(d.text) and (str(d.color).upper() in pools or game == "sts1")):
        d = mod.CARDS[key]
        print(f"{key:22s} {played(mod, game, key)}\n    {d.text[:150]}")
    if not keys:
        print("\nStrength or Dexterity unlike the parse's:")
        for key, ps, pd, ds, dd, text in doubled(mod, game):
            print(f"  {key:22s} parse {ps:+d}/{pd:+d}, model {ds:+d}/{dd:+d} | {text[:100]}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
