"""Every card, power, relic and monster of both games, and how the bot handles each: a rule of its own (its id named in the
planner, deck or relic code: rating tables do not count), read from its text by the generic parser, or not handled at all.

    python3 scripts/audit/coverage.py [sts2|sts1] [cards|powers|relics|monsters|all] [N]   (N: the unhandled to list, by use)

The game's own lists: ~/.local/state/sts2bot/loc/<game>/ (STS2's localization/eng from SlayTheSpire2.pck, extracted with
loc/pck.py; the first game's from desktop-1.0.jar). How often each is met: fights.jsonl decks and relics, bot.log power lines.
"""

from __future__ import annotations

import ast
import collections
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT))

STATE = Path.home() / ".local/state/sts2bot"
# Where a named id is behaviour (not a rating): the planner, deck and relic code, events, the first game's screens and stances.
BEHAVIOUR = ["sts2bot/fight", "sts2bot/run/decks.py", "sts2bot/run/strategy.py", "sts2bot/run/potions.py", "sts2bot/run/relicworth.py", "sts2bot/run/events",
             "sts2bot/games/sts1/screens.py", "sts2bot/games/sts1/stance.py", "sts2bot/games/sts1/keys.py", "sts2bot/cards.py", "sts2bot/bot.py"]
RATING_TABLES = {"_TIER", "_AFFLICTION", "_RARITY", "RATING", "FOOL_RATING", "_ANCIENT", "BOSS_WORTH", "_WORTH"}
PARTIAL = re.compile(r"\bwhenever\b|\bif\b|\bnext turn\b|\bfor each\b|\bthis turn\b|\bat the (?:start|end) of\b|\bretain\b|\bwhen\b|\bexhaust (?:all|your|\d)", re.I)


def named_ids() -> set[str]:
    """Upper-case string literals in the behaviour code, the keys of rating tables left out."""
    out: set[str] = set()
    files = []
    for p in BEHAVIOUR:
        path = ROOT / p
        files += sorted(path.rglob("*.py")) if path.is_dir() else [path]
    for f in files:
        tree = ast.parse(f.read_text())
        skip = set()
        for node in ast.walk(tree):
            if isinstance(node, (ast.Assign, ast.AnnAssign)):
                targets = node.targets if isinstance(node, ast.Assign) else [node.target]
                if any(isinstance(t, ast.Name) and t.id in RATING_TABLES for t in targets) and isinstance(node.value, ast.Dict):
                    skip.update(id(k) for k in node.value.keys)
        for node in ast.walk(tree):
            if isinstance(node, ast.Constant) and isinstance(node.value, str) and id(node) not in skip:
                for word in re.findall(r"\b[A-Z][A-Z0-9_]{2,}\b", node.value):
                    out.add(word)
    return out


def loc(game: str, name: str) -> dict:
    return json.load(open(STATE / "loc" / game / f"{name}.json"))


def deck_counts(game: str) -> collections.Counter:
    path = STATE / ("fights.jsonl" if game == "sts2" else "sts1/fights.jsonl")
    c: collections.Counter = collections.Counter()
    relics: collections.Counter = collections.Counter()
    for line in open(path):
        f = json.loads(line)
        for name in set(f.get("deck") or []):
            c[re.sub(r"\+\d*$", "", name)] += 1
        for r in set(f.get("relics") or []):
            relics[r] += 1
    return c, relics


def power_mentions(game: str) -> collections.Counter:
    log = STATE / ("bot.log" if game == "sts2" else "sts1/bot.log")
    c: collections.Counter = collections.Counter()
    for m in re.finditer(r"\[([A-Z0-9_:,\-]+)\]", log.read_text(errors="replace")):
        for part in m.group(1).split(","):
            c[part.split(":")[0]] += 1
    return c


def sts2_cards(named: set[str]):
    from sts2bot.cards import parse
    names = loc("sts2", "cards")
    wiki = json.load(open(STATE / "wiki.json"))["cards"]
    decks, _ = deck_counts("sts2")
    rows = []
    for cid in sorted({k.split(".")[0] for k in names if k.endswith(".title")}):
        title = names.get(cid + ".title", cid)
        entry = wiki.get(cid)
        text = (entry or {}).get("base", {}).get("description") or re.sub(r"\{[^}]*\}", "N", names.get(cid + ".description", ""))
        if cid in named:
            status = "rule"
        elif entry is None:
            status = "unseen"
        else:
            info = parse({"id": cid, "name": title, "type": entry["type"], "cost": entry["base"]["cost"], "description": text, "target_type": "AnyEnemy"})
            status = ("partial" if PARTIAL.search(text) else "read") if info.known else ("power" if entry["type"] == "Power" else "unread")
        rows.append((cid, title, status, decks.get(title, 0), text))
    return rows


def sts1_cards(named: set[str]):
    from sts2bot.cards import parse
    decks, _ = deck_counts("sts1")
    tables = {}
    for mod in ("ironclad", "silent", "defect", "watcher", "colorless"):
        tables.update(getattr(__import__(f"sts2bot.games.sts1.text.{mod}", fromlist=["CARDS"]), "CARDS", {}))
    names = {k.upper().replace(" ", "_"): v.get("NAME", k) for k, v in loc("sts1", "cards").items()}
    rows = []
    for cid, text in sorted(tables.items()):
        plain = re.sub(r"\(\d+\)", "", text)
        info = parse({"id": cid, "name": names.get(cid, cid), "type": "Skill", "cost": "1", "description": plain, "target_type": "AnyEnemy"})
        status = "rule" if cid in named else ("partial" if PARTIAL.search(plain) else "read") if info.known else "unread"
        rows.append((cid, names.get(cid, cid), status, decks.get(names.get(cid, cid), 0), plain))
    return rows


def powers(game: str, named: set[str]):
    seen = power_mentions(game)
    data = loc(game, "powers")
    rows = []
    if game == "sts2":
        for pid in sorted({k.split(".")[0] for k in data}):
            short = pid.removesuffix("_POWER")
            text = re.sub(r"\[/?\w+\]", "", data.get(pid + ".description", "") or data.get(pid + ".smartDescription", ""))
            status = "rule" if (short in named or pid in named) else "unnamed"
            rows.append((short, data.get(pid + ".title", short), status, seen.get(short, 0), text))
    else:
        from sts2bot.games.sts1.screens import POWER_ID
        for pid, v in sorted(data.items()):
            key = re.sub(r"[^A-Z0-9]+", "_", pid.upper()).strip("_")
            mapped = POWER_ID.get(key.replace("_", ""), key)
            status = "rule" if (mapped in named or key in named) else "unnamed"
            rows.append((mapped, v.get("NAME", pid), status, seen.get(mapped, 0) + seen.get(key, 0), " ".join(v.get("DESCRIPTIONS") or [])[:160]))
    return rows


def relics(game: str, named: set[str]):
    from sts2bot.run import relicworth
    from sts2bot.run.events import _ANCIENT
    _, held = deck_counts(game)
    rated = set(getattr(relicworth, "_WORTH", {})) | set(_ANCIENT) | set(getattr(relicworth, "_DECK", {}))
    rows = []
    if game == "sts2":
        data = loc("sts2", "relics")
        for rid in sorted({k.split(".")[0] for k in data if k.endswith(".title")}):
            status = "rule" if rid in named else "rated" if rid in rated else "text"
            rows.append((rid, data.get(rid + ".title", rid), status, held.get(rid, 0), re.sub(r"\[/?\w+\]", "", data.get(rid + ".description", ""))))
    else:
        for rid, v in sorted(loc("sts1", "relics").items()):
            key = re.sub(r"[^A-Z0-9]+", "_", rid.upper()).strip("_")
            status = "rule" if key in named else "rated" if key in rated else "text"
            rows.append((key, v.get("NAME", rid), status, held.get(key, 0) + held.get(rid, 0), " ".join(v.get("DESCRIPTIONS") or [])[:160]))
    return rows


def monsters(game: str, named: set[str]):
    rows = []
    if game == "sts2":
        data = loc("sts2", "monsters")
        for mid in sorted({k.split(".")[0] for k in data if k.endswith(".name")}):
            rows.append((mid, data[mid + ".name"], "rule" if mid in named else "intents", 0, ", ".join(sorted({v for k, v in data.items() if k.startswith(mid + ".moves.")}))[:160]))
    else:
        for mid, v in sorted(loc("sts1", "monsters").items()):
            key = re.sub(r"[^A-Z0-9]+", "", mid.upper())
            rows.append((key, v.get("NAME", mid), "rule" if key in named else "intents", 0, ""))
    return rows


def report(title: str, rows: list, n: int, todo: tuple) -> None:
    c = collections.Counter(r[2] for r in rows)
    print(f"== {title}: {len(rows)}  " + "  ".join(f"{k} {v}" for k, v in c.most_common()))
    for cid, name, status, used, text in sorted((r for r in rows if r[2] in todo), key=lambda r: -r[3])[:n]:
        print(f"   {status:8} {used:5}  {name[:26]:26} {text[:150]}")


def main() -> None:
    game = sys.argv[1] if len(sys.argv) > 1 else "sts2"
    what = sys.argv[2] if len(sys.argv) > 2 else "all"
    n = int(sys.argv[3]) if len(sys.argv) > 3 else 25
    from sts2bot import games
    games.select(game)
    named = named_ids()
    if what in ("cards", "all"):
        report(f"{game} cards", sts2_cards(named) if game == "sts2" else sts1_cards(named), n, ("unread", "partial"))
    if what in ("powers", "all"):
        report(f"{game} powers", powers(game, named), n, ("unnamed",))
    if what in ("relics", "all"):
        report(f"{game} relics", relics(game, named), n, ("text",))
    if what in ("monsters", "all"):
        report(f"{game} monsters", monsters(game, named), 0, ())


if __name__ == "__main__":
    main()
