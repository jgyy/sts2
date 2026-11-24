"""Tables from model-run files: how each character did, under each value of each setting, and where runs ended."""

from __future__ import annotations

import json
import math
from collections import Counter, defaultdict

WIN_FLOORS = 15  # a win counts as this many floors beyond the last one (as learn.WIN_BONUS)


def load(files: list[str]) -> list[dict]:
    out = []
    for path in files:
        with open(path) as f:
            for line in f:
                line = line.strip()
                if line:
                    out.append(json.loads(line))
    return out


def score(r: dict) -> float:
    return r["floor"] + (WIN_FLOORS if r["won"] else 0)


def mean_se(xs: list[float]) -> tuple[float, float]:
    n = len(xs)
    if not n:
        return 0.0, 0.0
    m = sum(xs) / n
    var = sum((x - m) ** 2 for x in xs) / max(1, n - 1)
    return m, math.sqrt(var / n)


def pct(k: int, n: int) -> str:
    return f"{100.0 * k / n:.1f}%" if n else "-"


def boss_record(runs: list[dict]) -> list[str]:
    cells = []
    for act in (1, 2, 3):
        met = [f for r in runs for f in r["fights"] if f["kind"] == "boss" and f["act"] == act]
        cells.append(f"{pct(sum(f['won'] for f in met), len(met))} of {len(met)}")
    return cells


def knob_table(runs: list[dict], min_runs: int) -> list[str]:
    """For each setting, each value's runs: win rate, floor score, and how far it is from that setting's average."""
    lines = ["| Setting | Value | Runs | Wins | Score (floor +15 a win) | vs. the others |", "|---|---|---|---|---|---|"]
    names = sorted({k for r in runs for k in (r.get("knobs") or {})})
    for name in names:
        by: dict = defaultdict(list)
        for r in runs:
            if name in (r.get("knobs") or {}):
                by[r["knobs"][name]].append(r)
        if len(by) < 2:
            continue
        overall = mean_se([score(r) for r in runs if name in (r.get("knobs") or {})])[0]
        for value in sorted(by):
            rs = by[value]
            if len(rs) < min_runs:
                continue
            m, se = mean_se([score(r) for r in rs])
            lines.append(f"| {name} | {value:g} | {len(rs)} | {pct(sum(r['won'] for r in rs), len(rs))} | {m:.1f} ± {se:.1f} | {m - overall:+.1f} |")
    return lines


def best_values(runs: list[dict], min_runs: int) -> dict:
    """The value of each setting whose runs scored best, with its margin over the next and that margin's standard error."""
    out = {}
    names = sorted({k for r in runs for k in (r.get("knobs") or {})})
    for name in names:
        by: dict = defaultdict(list)
        for r in runs:
            if name in (r.get("knobs") or {}):
                by[r["knobs"][name]].append(score(r))
        rows = sorted(((mean_se(v), k, len(v)) for k, v in by.items() if len(v) >= min_runs), key=lambda t: -t[0][0])
        if len(rows) >= 2:
            (m1, s1), v1, n1 = rows[0]
            (m2, s2), v2, _ = rows[1]
            out[name] = (v1, m1 - m2, math.sqrt(s1 * s1 + s2 * s2), n1)
    return out


def card_table(runs: list[dict], top: int = 12, min_picks: int = 25) -> tuple[list, list]:
    """Cards taken from rewards, by how much further the runs that took them got than the character's runs that left the
    same floor alive (learn.CardStats' measure: a card taken late is not credited with the floors before it)."""
    finals = sorted(score(r) for r in runs)
    reached = lambda floor: [score(r) for r in runs if r["floor"] > floor or r["won"]]
    usual: dict = {}
    gained: dict = defaultdict(list)
    for r in runs:
        for floor, key in r.get("picks") or []:
            if floor not in usual:
                alive = reached(floor)
                usual[floor] = sum(alive) / len(alive) if alive else 0.0
            gained[key].append(score(r) - usual[floor])
    del finals
    rows = []
    for key, gains in gained.items():
        if len(gains) >= min_picks:
            m, se = mean_se(gains)
            rows.append((m, key, len(gains), m, se))
    rows.sort(reverse=True)
    return rows[:top], rows[-top:][::-1]


def deaths(runs: list[dict], top: int = 8) -> list[tuple]:
    where = Counter()
    for r in runs:
        if not r["won"] and r["fights"]:
            last = r["fights"][-1]
            where[(last["act"], last["kind"], "+".join(last["foes"]))] += 1
    return where.most_common(top)


def habits(runs: list[dict]) -> dict:
    n = max(1, len(runs))
    won = [r for r in runs if r["won"]]
    lost = [r for r in runs if not r["won"]]
    avg = lambda rs, k: sum(len(r[k]) if isinstance(r[k], list) else r[k] for r in rs) / max(1, len(rs))
    return {
        "elites": (avg(won, "elites"), avg(lost, "elites")),
        "rests": (avg(won, "rests"), avg(lost, "rests")),
        "smiths": (avg(won, "smiths"), avg(lost, "smiths")),
        "removals": (avg(won, "removals"), avg(lost, "removals")),
        "skips": (avg(won, "skips"), avg(lost, "skips")),
        "deck": (sum(len(r["deck"]) for r in won) / max(1, len(won)), sum(len(r["deck"]) for r in lost) / max(1, len(lost))),
        "n": n,
    }


def report(files: list[str], min_runs: int = 30) -> str:
    runs = load(files)
    errors = [r for r in runs if "error" in r]
    runs = [r for r in runs if "error" not in r]
    out = ["# Model runs", ""]
    if errors:
        out += [f"{len(errors)} runs ended in a model error and are left out.", ""]
    groups: dict = defaultdict(list)
    for r in runs:
        groups[(r["game"], r["character"], r.get("ascension", 0))].append(r)
    out += ["| Game | Character | Asc | Runs | Wins | Mean floor | Act 1 boss | Act 2 boss | Act 3 boss |", "|---|---|---|---|---|---|---|---|---|"]
    for (game, ch, asc), rs in sorted(groups.items()):
        m, se = mean_se([r["floor"] for r in rs])
        out.append(f"| {game} | {ch} | {asc} | {len(rs)} | {pct(sum(r['won'] for r in rs), len(rs))} | {m:.1f} ± {se:.1f} | " + " | ".join(boss_record(rs)) + " |")
    for (game, ch, asc), rs in sorted(groups.items()):
        out += ["", f"## {game} {ch} (Ascension {asc}, {len(rs)} runs)", ""]
        h = habits(rs)
        out.append("Won runs against lost ones, per run: " + ", ".join(f"{k} {wl[0]:.1f} / {wl[1]:.1f}" for k, wl in h.items() if k != "n"))
        out += ["", *knob_table(rs, min_runs)]
        good, bad = card_table(rs)
        if good:
            out += ["", "Cards whose takers went furthest (score over runs that left the same floor alive, picks):", ""]
            out.append(", ".join(f"{k} {d:+.1f} ({n})" for d, k, n, _m, _s in good))
            out += ["", "... and least far:", ""]
            out.append(", ".join(f"{k} {d:+.1f} ({n})" for d, k, n, _m, _s in bad))
        out += ["", "Where runs ended: " + ", ".join(f"act {a} {kind} {foes} {n}" for (a, kind, foes), n in deaths(rs))]
    return "\n".join(out) + "\n"
