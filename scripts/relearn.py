#!/usr/bin/env python3
"""Work the bot's card record (card_stats.json) out again from runs.jsonl.

    python3 scripts/relearn.py              each kind of card's record, and the cards whose rating moves most
    python3 scripts/relearn.py --write      ... and put the new record in place (the old one is kept beside
                                            it as card_stats.json.old)

For when the rule that makes the record changes (learn.CardStats.record): every run in runs.jsonl goes
through it again, oldest first. What kind each card is (cards.kind) comes from wiki.json, which
`scripts/fightlog.py wiki` writes. Stop the bot before --write: it holds the record in memory and saves
its own after every run.
"""

from __future__ import annotations

import json
import os
import sys
from pathlib import Path

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from sts2bot import cards, learn  # noqa: E402

LOGS = Path(os.path.expanduser(os.environ.get("STS2BOT_STATE", "~/.local/state/sts2bot/")))


def rebuilt() -> learn.CardStats:
    stats = learn.CardStats(None)
    with open(LOGS / "runs.jsonl") as f:
        for line in f:
            r = json.loads(line)
            stats.record(str(r.get("character") or ""), r.get("floor"), r.get("result") == "win", r.get("picks") or [])
    try:
        wiki = json.loads((LOGS / "wiki.json").read_text())["cards"]
    except (OSError, ValueError, KeyError):
        wiki = {}
        print("no wiki.json: the kinds will fill in as the bot is offered each card (python3 scripts/fightlog.py wiki writes one)")
    for entry in wiki.values():
        base = entry.get("base") or {}
        stats.kinds[learn.card_key(base.get("name"))] = cards.kind(base)
    stats._pool = None
    return stats


def main(write: bool) -> None:
    old, new = learn.CardStats(LOGS / "card_stats.json"), rebuilt()
    for kind in ("free", "energy", "draw"):
        rows = (new._pool if new.kind_shift(kind) is not None else {}).get(kind) or {}
        picks, gained = sum(x[0] for x in rows.values()), sum(x[1] for x in rows.values())
        print(f"{kind:7} {len(rows):3} cards, {picks:5.0f} picks, {gained / max(1.0, picks):+5.2f} floors a pick beyond par: its cards start at {new.kind_shift(kind):+5.2f}")
    for who in sorted(new.cards):
        moved = []
        for key, (picks, _gained) in new.cards[who].items():
            was, now = old.shift(who, key), new.shift(who, key)
            moved.append((now - was, key, picks, was, now, new.kinds.get(key, "")))
        moved.sort()
        print(f"{who}: {sum(x[0] for x in new.cards[who].values()):.0f} picks, par {new._par(who):+.2f} floors a pick")
        for diff, key, picks, was, now, kind in moved[:6] + moved[-6:]:
            print(f"   {key:24} {picks:4.0f} picks  {was:+5.2f} -> {now:+5.2f}  {kind}")
    if write:
        path = LOGS / "card_stats.json"
        if path.exists():
            path.replace(path.with_suffix(".json.old"))
        new.path = path
        new.save()
        print("written:", path)


if __name__ == "__main__":
    main("--write" in sys.argv[1:])
