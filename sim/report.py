"""Tables from model-run files: how each character did, what each setting did to it, which cards went with deep runs,
and where runs ended. A run's score is the floor it ended on, plus 15 for a win (as learn.WIN_BONUS)."""

from __future__ import annotations

import json
import math
from collections import Counter, defaultdict

WIN_FLOORS = 15


def load(files: list[str]) -> list[dict]:
    out = []
    for path in files:
        with open(path) as f:
            out += [json.loads(line) for line in f if line.strip()]
    return out


def score(r: dict) -> float:
    return r["floor"] + (WIN_FLOORS if r["won"] else 0)


def mean_se(xs: list[float]) -> tuple[float, float]:
    n = len(xs)
    if not n:
        return 0.0, 0.0
    m = sum(xs) / n
    return m, math.sqrt(sum((x - m) ** 2 for x in xs) / max(1, n - 1) / n)


def pct(k: int, n: int) -> str:
    return f"{100.0 * k / n:.1f}%" if n else "-"


def boss_record(runs: list[dict]) -> list[str]:
    cells = []
    for act in (1, 2, 3):
        met = [f for r in runs for f in r["fights"] if f["kind"] == "boss" and f["act"] == act]
        cells.append(f"{pct(sum(f['won'] for f in met), len(met))} of {len(met)}")
    return cells


def effects(runs: list[dict]) -> dict:
    """setting -> value -> (score over the setting's average in these runs, its standard error, runs)."""
    out: dict = {}
    for name in sorted({k for r in runs for k in (r.get("knobs") or {})}):
        by: dict = defaultdict(list)
        for r in runs:
            if name in (r.get("knobs") or {}):
                by[r["knobs"][name]].append(score(r))
        if len(by) < 2:
            continue
        base = mean_se([s for v in by.values() for s in v])[0]
        out[name] = {v: (mean_se(s)[0] - base, mean_se(s)[1], len(s)) for v, s in sorted(by.items())}
    return out


def pooled(groups: dict) -> dict:
    """Each character's effects, pooled over characters weighted by runs (every character's own average taken out)."""
    acc: dict = defaultdict(lambda: defaultdict(lambda: [0.0, 0.0, 0]))
    for runs in groups.values():
        for name, values in effects(runs).items():
            for v, (d, se, n) in values.items():
                a = acc[name][v]
                a[0] += d * n
                a[1] += (se * n) ** 2
                a[2] += n
    return {name: {v: (a[0] / a[2], math.sqrt(a[1]) / a[2], a[2]) for v, a in vals.items()} for name, vals in acc.items()}


def best(values: dict) -> tuple:
    """(best value, its margin over the runner-up, that margin's standard error)."""
    rows = sorted(values.items(), key=lambda kv: -kv[1][0])
    (v1, (d1, s1, _)), (_v2, (d2, s2, _)) = rows[0], rows[1]
    return v1, d1 - d2, math.sqrt(s1 * s1 + s2 * s2)


def mark(d: float, se: float) -> str:
    return "**" if se and abs(d) > 2.58 * se else "*" if se and abs(d) > 1.96 * se else ""


def settings_table(groups: dict, game: str) -> list[str]:
    chars = [ch for (g, ch, _a) in sorted(groups) if g == game]
    per = {ch: effects(rs) for (g, ch, _a), rs in groups.items() if g == game}
    pool = pooled({k: v for k, v in groups.items() if k[0] == game})
    lines = ["| Setting | Value | " + " | ".join(chars) + " | All |", "|---|---|" + "---|" * (len(chars) + 1)]
    for name, values in pool.items():
        for v, (d, se, n) in values.items():
            cells = []
            for ch in chars:
                cd = per.get(ch, {}).get(name, {}).get(v)
                cells.append(f"{cd[0]:+.1f}{mark(cd[0], cd[1])}" if cd else "")
            lines.append(f"| {name} | {v:g} | " + " | ".join(cells) + f" | {d:+.1f}{mark(d, se)} |")
    return lines


def recommendations(groups: dict, game: str) -> list[str]:
    out = []
    for (g, ch, _a), rs in sorted(groups.items()):
        if g != game:
            continue
        picks = []
        for name, values in effects(rs).items():
            v, margin, se = best(values)
            if margin > 1.96 * se:
                picks.append(f"{name} {v:g} (+{margin:.1f} ± {se:.1f})")
        out.append(f"- **{ch}**: " + (", ".join(picks) if picks else "no setting separates from the rest at 95%"))
    return out


def card_table(runs: list[dict], top: int = 10, min_picks: int = 30) -> tuple[list, list]:
    """Cards taken from rewards, by how much further the runs that took them got than the character's runs that left the
    same floor alive (learn.CardStats' measure: a card taken late is not credited with the floors before it)."""
    usual: dict = {}
    gained: dict = defaultdict(list)
    for r in runs:
        for floor, key in r.get("picks") or []:
            if floor not in usual:
                alive = [score(x) for x in runs if x["floor"] > floor or x["won"]]
                usual[floor] = sum(alive) / len(alive) if alive else 0.0
            gained[key].append(score(r) - usual[floor])
    rows = sorted(((mean_se(g)[0], k, len(g), mean_se(g)[1]) for k, g in gained.items() if len(g) >= min_picks), reverse=True)
    return rows[:top], rows[-top:][::-1]


def deaths(runs: list[dict], top: int = 6) -> list[tuple]:
    where = Counter()
    for r in runs:
        if not r["won"] and r["fights"]:
            last = r["fights"][-1]
            where[(last["act"], last["kind"], "+".join(last["foes"]))] += 1
    return where.most_common(top)


def habits(runs: list[dict]) -> str:
    won, lost = [r for r in runs if r["won"]], [r for r in runs if not r["won"]]
    avg = lambda rs, k: sum(len(v) if isinstance(v := r.get(k, 0), list) else v for r in rs) / max(1, len(rs))
    return ", ".join(f"{k} {avg(won, k):.1f} / {avg(lost, k):.1f}" for k in ("elites", "rests", "smiths", "removals", "skips", "potions", "deck"))


def report(files: list[str], min_runs: int = 30) -> str:
    runs = load(files)
    errors = [r for r in runs if "error" in r]
    runs = [r for r in runs if "error" not in r]
    groups: dict = defaultdict(list)
    for r in runs:
        groups[(r["game"], r["character"], r.get("ascension", 0))].append(r)
    out = ["# Model runs", ""] + ([f"{len(errors)} runs ended in a model error and are left out.", ""] if errors else [])
    out += ["| Game | Character | Asc | Runs | Wins | Mean floor | Act 1 boss | Act 2 boss | Act 3 boss |", "|---|---|---|---|---|---|---|---|---|"]
    for (game, ch, asc), rs in sorted(groups.items()):
        m, se = mean_se([r["floor"] for r in rs])
        out.append(f"| {game} | {ch} | {asc} | {len(rs)} | {pct(sum(r['won'] for r in rs), len(rs))} | {m:.1f} ± {se:.1f} | " + " | ".join(boss_record(rs)) + " |")
    for game in sorted({g for g, _c, _a in groups}):
        out += ["", f"## {game}: settings", "", "Score over the setting's average, per character and pooled (* 95%, ** 99%):", ""]
        out += settings_table(groups, game)
        out += ["", "Best value where it beats the runner-up at 95%:", "", *recommendations(groups, game)]
    for (game, ch, asc), rs in sorted(groups.items()):
        out += ["", f"## {game} {ch} (Ascension {asc}, {len(rs)} runs)", "", "Won / lost runs, per run: " + habits(rs)]
        good, bad = card_table(rs)
        if good:
            out += ["", "Cards that went with the deepest runs (score over runs that left the same floor alive; picks):",
                    ", ".join(f"{k} {d:+.1f} ({n})" for d, k, n, _s in good), "", "... and the shallowest:",
                    ", ".join(f"{k} {d:+.1f} ({n})" for d, k, n, _s in bad)]
        out += ["", "Where runs ended: " + ", ".join(f"act {a} {kind} {foes} {n}" for (a, kind, foes), n in deaths(rs))]
    return "\n".join(out) + "\n"
