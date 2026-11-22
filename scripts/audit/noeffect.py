"""Cards played for nothing: each play in bot.log*, the combat state before it against the state just before the next action.

    python3 scripts/audit/noeffect.py ['YYYY-MM-DD HH:MM']

A play that changed nothing the log shows (our HP, Block, energy, powers, orbs, Stars, Osty; every enemy's HP, Block,
powers and intent; the hand beyond the card itself) is listed by card with the share of its plays that did so. Use it
beside parse_vs_text.py after a game patch: a card the parser misreads is one the planner believes in, so the planner
scans (fightlog.py idle, surprises, unplayed) cannot see it (Bubble Bubble, edit 175, was found by hand this way).

Expected on the list, not misplays: Status cards that are played to be rid of them (Beckon, Toxic, Debris), energy
cards whose energy is spent at once (Wisp, TURBO, Supercritical, Luminesce). Powers can show when the state before the
next action was read before the power appeared (Panache, Subroutine): read the examples. First run (2026-11-22, 4.5 h
of logs): Purity (11 of 12 plays; "Exhaust up to 3 cards" confirmed with none chosen) and Restlessness (8 of 8; played
with cards still in hand, so "if your Hand is empty" never held), both rare.
"""
import collections
import re
import sys
from pathlib import Path

LOG = Path.home() / ".local/state/sts2bot"
STATE = re.compile(r"DEBUG combat r(\d+) (.*)$")
PLAY = re.compile(r"Playing '([^']+)'")


def split(s: str):
    """(our side, the enemies, the cards in hand) of a `combat r` line."""
    parts = s.split(" | ")
    hand = parts[-1] if parts[-1].startswith("hand") else ""
    foes = " | ".join(parts[1:-1] if hand else parts[1:])
    return parts[0], foes, [c.strip() for c in re.findall(r"\d+:([^,(]+)\(", hand)]


def scan(since: str = "") -> dict:
    stats = collections.defaultdict(lambda: [0, 0, []])  # card -> [plays, plays that changed nothing, examples]
    for f in [LOG / n for n in ("bot.log.3", "bot.log.2", "bot.log.1", "bot.log")]:
        if not f.exists():
            continue
        last = prev = pending = None
        for line in f.read_text(errors="replace").splitlines():
            if since and line[:16] < since:
                continue
            m = STATE.search(line)
            if m:
                last = (int(m.group(1)), m.group(2), line[:19])
                continue
            if "->" not in line or not any(k in line for k in ("play_card", "end_turn", "use_potion")):
                continue
            if last and pending and prev and last[0] == prev[0]:
                me0, foes0, hand0 = split(prev[1])
                me1, foes1, hand1 = split(last[1])
                rest = list(hand0)
                if pending in rest:
                    rest.remove(pending)
                row = stats[pending]
                row[0] += 1
                if me0 == me1 and foes0 == foes1 and sorted(rest) == sorted(hand1):
                    row[1] += 1
                    if len(row[2]) < 2:
                        row[2].append(f"{prev[2]} | {prev[1][:230]}")
            prev, pending = last, None
            m = PLAY.search(line)
            if m:
                pending = m.group(1)
    return stats


if __name__ == "__main__":
    rows = [(n, a, b, ex) for n, (a, b, ex) in scan(sys.argv[1] if len(sys.argv) > 1 else "").items() if b >= 2 and b / a >= 0.25]
    for n, a, b, ex in sorted(rows, key=lambda r: -r[2]):
        print(f"{n:28s} played {a:4d}  no visible effect {b:4d} ({100 * b / a:.0f}%)")
        for e in ex:
            print("     ", e)
