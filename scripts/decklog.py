#!/usr/bin/env python3
"""What the decks that fought cheaply held: fights.jsonl (each fight's deck, relics and HP in and out) against wiki.json
(what each card is; `fightlog.py wiki` writes it, and a card it lacks counts in the deck's size only).

    python3 scripts/decklog.py [ACT] ['YYYY-MM-DD HH:MM']          ordinary fights: what one costs (a share of Max HP) against how
                                                                   many cards of each sort the deck held, the character taken out;
                                                                   then the cards and relics themselves
    python3 scripts/decklog.py bosses [ACT] ['YYYY-MM-DD HH:MM']   the same for boss fights, won or lost

A death costs the HP the fight began on and a tenth of Max HP more; fights begun under half HP or joined half way are left
out. t is the slope over its standard error (with twenty sorts of card on the table, one passes 2 by chance).
"""

from __future__ import annotations

import collections
import json
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from sts2bot import cards
from sts2bot.run import decks

LOGS = os.path.expanduser(os.environ.get("STS2BOT_STATE", "~/.local/state/sts2bot/"))


def _wiki() -> dict:
    with open(os.path.join(LOGS, "wiki.json")) as f:
        wiki = json.load(f)
    return {v["name"]: dict(id=cid, name=v["name"], type=v["type"], cost=v["cost"], description=v["description"], rarity=v.get("rarity"))
            for cid, entry in wiki["cards"].items() for v in (entry.get("base"), entry.get("upgraded")) if v}


def load(since: str = "") -> list[dict]:
    fights = []
    for path in (os.path.join(LOGS, "fights.jsonl.1"), os.path.join(LOGS, "fights.jsonl")):
        if os.path.exists(path):
            with open(path) as f:
                fights += [r for r in map(json.loads, f) if r.get("whole") and r["time"].replace("T", " ") >= since]
    return fights


def features(fight: dict, wiki: dict) -> dict:
    names = fight["deck"]
    known = [wiki[n] for n in names if n in wiki]
    infos = [cards.parse({**c, "index": 0}) for c in known]
    own = [(c, i) for c, i in zip(known, infos) if c["rarity"] != "Basic" and c["type"] not in ("Curse", "Status")]
    return {
        "Block cards": sum(1 for c, _ in own if decks._blocks(c)),
        "cards for damage": sum(1 for c, _ in own if decks._punch(c) >= 8.0),
        "other attacks": sum(1 for c, _ in own if c["type"] == "Attack" and decks._punch(c) < 8.0),
        "hit ALL": sum(1 for _, i in own if i.aoe and i.damage),
        "Powers": sum(1 for c, _ in own if c["type"] == "Power"),
        "other Skills": sum(1 for c, _ in own if c["type"] == "Skill" and not decks._blocks(c)),
        "Weak": sum(1 for _, i in own if i.weak),
        "Vulnerable": sum(1 for _, i in own if i.vulnerable),
        "cards drawn": sum(i.draw for _, i in own),
        "cost 0": sum(1 for _, i in own if i.cost == 0),
        "cost 2+": sum(1 for _, i in own if i.cost >= 2),
        "upgraded": sum(1 for n in names if n.endswith("+")),
        "Strikes": sum(1 for c in known if c["id"].startswith("STRIKE")),
        "Defends": sum(1 for c in known if c["id"].startswith("DEFEND")),
        "Curses": sum(1 for c in known if c["type"] in ("Curse", "Status")),
        "deck size": len(names),
        "relics": len(fight["relics"]),
        "potions": len(fight["potions"]),
    }


def _slope(rows: list[tuple]) -> tuple[float, float]:
    """(slope, its standard error) of y on x with a constant for every group: rows are (group, x, y)."""
    by = collections.defaultdict(list)
    for g, x, y in rows:
        by[g].append((x, y))
    centred = []
    for pts in by.values():
        mx, my = sum(x for x, _ in pts) / len(pts), sum(y for _, y in pts) / len(pts)
        centred += [(x - mx, y - my) for x, y in pts]
    sxx, sxy = sum(x * x for x, _ in centred), sum(x * y for x, y in centred)
    if sxx <= 0 or len(centred) - len(by) - 1 <= 0:
        return 0.0, 0.0
    b = sxy / sxx
    return b, (sum((y - b * x) ** 2 for x, y in centred) / (len(centred) - len(by) - 1) / sxx) ** 0.5


def report(fights: list[dict], bosses: bool) -> None:
    wiki = _wiki()
    for act in sorted({f["act"] for f in fights}):
        if bosses:
            sub = [f for f in fights if f["act"] == act and f["kind"] == "boss"]
            outcome = lambda f: 100.0 * (not f["died"])
            what, unit = "won", "points of win rate"
        else:
            sub = [f for f in fights if f["act"] == act and f["kind"] == "monster" and f["max_hp"] and f["hp_in"] >= 0.5 * f["max_hp"]]
            outcome = lambda f: 100.0 * ((f["hp_in"] / f["max_hp"] + 0.1) if f["died"] else (f["hp_in"] - f["hp_out"]) / f["max_hp"])
            what, unit = "cost, % of Max HP", "% of Max HP"
        if len(sub) < 30:
            print(f"act {act}: {len(sub)} fights, too few")
            continue
        feats, ys = [features(f, wiki) for f in sub], [outcome(f) for f in sub]
        print(f"act {act}: {len(sub)} {'boss' if bosses else 'ordinary'} fights; {what} {sum(ys) / len(ys):.1f} on average")
        for name in feats[0]:
            xs = [ft[name] for ft in feats]
            b, se = _slope([(f["character"], x, y) for f, x, y in zip(sub, xs, ys)])
            dose = collections.defaultdict(list)
            for x, y in zip(xs, ys):
                dose[min(x, 3) if name not in ("deck size", "relics", "upgraded", "cards drawn") else "-"].append(y)
            table = "  ".join(f"{k}{'+' if k == 3 else ''}: {sum(v) / len(v):4.1f} ({len(v)})" for k, v in sorted(dose.items(), key=str) if k != "-")
            print(f"  {name:17} held {sum(xs) / len(xs):4.1f}   one more: {b:+5.2f} {unit} (t {b / se if se else 0:+.1f})   {table}")
        par = collections.defaultdict(list)
        for f, y in zip(sub, ys):
            par[f["character"]].append(y)
        par = {k: sum(v) / len(v) for k, v in par.items()}
        for label, key in (("cards", "deck"), ("relics", "relics")):
            seen = collections.defaultdict(list)
            for f, y in zip(sub, ys):
                for item in {n.rstrip("+") for n in f[key]}:
                    seen[item].append(y - par[f["character"]])
            ranked = sorted((sum(v) / len(v), len(v), k) for k, v in seen.items() if len(v) >= 25)
            ends = ranked[:10] + ranked[-10:] if len(ranked) > 20 else ranked
            print(f"  {label} against the character's usual ({unit}; 25+ fights; the ten at each end): " + ", ".join(f"{k} {m:+.1f} ({n})" for m, n, k in ends))


if __name__ == "__main__":
    args = sys.argv[1:]
    bosses = args[:1] == ["bosses"]
    args = args[1:] if bosses else args
    act = int(args.pop(0)) if args and args[0].isdigit() else None
    fights = [f for f in load(args[0] if args else "") if act is None or f["act"] == act]
    if fights:
        report(fights, bosses)
    else:
        print(__doc__ + "\n(no fights on record yet)")
