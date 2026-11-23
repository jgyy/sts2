"""Cards whose text names an amount that `cards.parse` read as nothing, or a condition it ignored.

    python3 scripts/audit/parse_vs_text.py

Reads the wiki dump (`fightlog.py wiki` writes it). Most rows are Powers, which `decks.turn_damage` and
`combat._power_worth` price by other means; look at the Skills and Attacks. On 2026-11-22 it found Bubble Bubble
("If the enemy has Poison, apply 9 Poison.") read as plain Poison: a dead card the planner valued at +65.7 points.
The second list is every card with an "if"/"when"/"unless" in its text: check each against what `parse` gives.
"""

import json
import os
import re
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
from sts2bot import cards

CHECKS = [
    ("damage", r"deal\s+(\d+)\s+damage", lambda i: i.damage or i.total_damage),
    ("block", r"gain\s+(\d+)\s+block", lambda i: i.block),
    ("weak", r"apply\s+(\d+)\s+weak", lambda i: i.weak),
    ("vulnerable", r"apply\s+(\d+)\s+vulnerable", lambda i: i.vulnerable),
    ("poison", r"apply\s+(\d+)\s+poison", lambda i: i.poison),
    ("doom", r"apply\s+(\d+)\s+doom", lambda i: i.doom),
    ("draw", r"draw\s+(\d+)\s+card", lambda i: i.draw),
    ("strength", r"gain\s+(\d+)\s+strength", lambda i: i.strength),
]


def main() -> None:
    wiki = json.load(open(os.path.expanduser("~/.local/state/sts2bot/wiki.json")))["cards"]
    print("-- named in the text, read as 0")
    seen = set()
    for cid, v in sorted(wiki.items()):
        for variant in ("base", "upgraded"):
            c = v.get(variant)
            text = str((c or {}).get("description") or "")
            for name, rx, get in CHECKS:
                if c and re.search(rx, text, re.I) and not get(cards.parse(c)) and (cid, name) not in seen:
                    seen.add((cid, name))
                    print(f"{cid:22s} {v.get('type'):6s} {name:10s} {text.replace(chr(10), ' ')[:110]}")
    print("-- conditions in the text (the card's own effect, not a Power's trigger)")
    for cid, v in sorted(wiki.items()):
        text = str((v.get("base") or {}).get("description") or "")
        if v.get("type") in ("Attack", "Skill") and re.search(r"\bif\b|\bunless\b|\bonly\b", text, re.I):
            i = cards.parse(v["base"])
            print(f"{cid:22s} {text.replace(chr(10), ' ')[:100]:100s} d{i.damage} b{i.block} p{i.poison} dr{i.draw}")


if __name__ == "__main__":
    main()
