#!/usr/bin/env python3
"""Read the bot's logs fight by fight (needs the bot to have run with -v).

    python3 scripts/fightlog.py bosses [ASCENSION]      win/loss per boss and character
    python3 scripts/fightlog.py costs                   HP cost and deaths per ordinary encounter
    python3 scripts/fightlog.py show 'YYYY-MM-DD HH:MM' [boss,elite,monster]
                                                        the first such fight from that time on: one
                                                        line per round (what was in hand), then the plays
    python3 scripts/fightlog.py deaths ['YYYY-MM-DD HH:MM']
                                                        the fight each run ended in, from that time on
    python3 scripts/fightlog.py knobs ['YYYY-MM-DDTHH:MM']
                                                        how runs went under each value of each setting
                                                        (runs.jsonl only, so it reaches back past the logs)
    python3 scripts/fightlog.py pace ['YYYY-MM-DD HH:MM']
                                                        rounds and enemy HP removed a round in ordinary
                                                        fights, by act: the first thing a planner change moves
    python3 scripts/fightlog.py reads                   per card: the damage and Block the planner read
                                                        against what the play did (finds misread cards)
    python3 scripts/fightlog.py unplayed [boss,elite,monster]
                                                        per card: played against left in the hand at the end
                                                        of a turn that could have paid for it at its start

`load()` returns one record per fight (act, floor, kind, enemies, HP in and out, rounds, every
logged combat state) joined to its run in runs.jsonl, for one-off questions from a Python prompt.
"""

from __future__ import annotations

import collections
import datetime
import json
import os
import re
import sys

LOGS = os.path.expanduser(os.environ.get("STS2BOT_STATE", "~/.local/state/sts2bot/"))
ACT = re.compile(r"^(\S+ \S+) INFO act (\d) floor (\d+)  (\w+)  hp (\d+)/(\d+)  gold (\d+)")
OVER = re.compile(r"^(\S+ \S+) INFO run \d+ over: (\w+) on act (\d) floor (\d+)")
COMBAT = re.compile(r"^(\S+ \S+) DEBUG combat r(\d+) hp (\d+)/(\d+) blk (\d+) en (\d+)(.*?) \| (.*?) \| hand (.*)$")


def lines():
    """Every log line, oldest first."""
    names = sorted((n for n in os.listdir(LOGS) if n.startswith("bot.log")), key=lambda n: -int(n.rsplit(".", 1)[1]) if n[-1].isdigit() else 1)
    for name in names:
        with open(os.path.join(LOGS, name), errors="replace") as f:
            for line in f:
                yield line.rstrip("\n")


def load():
    """(fights, runs). A fight that was still going when the log ends has no "died" key."""
    with open(os.path.join(LOGS, "runs.jsonl")) as f:
        runs = [json.loads(line) for line in f]
    by_time = {r["time"][:19].replace("T", " "): r for r in runs}
    fights, cur, in_run = [], None, []
    for line in lines():
        m = ACT.match(line)
        if m:
            t, act, floor, kind, hp, mx, _gold = m.groups()
            if cur and (int(floor), int(act)) != (cur["floor"], cur["act"]):
                cur["hp_out"], cur["died"] = int(hp), False
                cur = None
            if kind in ("monster", "elite", "boss") and cur is None:
                cur = dict(t=t, act=int(act), floor=int(floor), kind=kind, hp_in=int(hp), max_hp=int(mx), enemies=None, rounds=0, turns=[])
                fights.append(cur)
                in_run.append(cur)
            continue
        m = COMBAT.match(line)
        if m and cur:
            t, rnd, hp, _mx, blk, en, powers, enemies, hand = m.groups()
            if cur["enemies"] is None:
                cur["enemies"] = "+".join(sorted({re.sub(r"_\d+$", "", e.split(" ")[0]) for e in enemies.split(" ; ")}))
                cur["hp_in"] = int(hp)
            cur["rounds"] = max(cur["rounds"], int(rnd))
            cur["turns"].append((int(rnd), int(hp), int(blk), int(en), powers, enemies, hand, t))
            continue
        m = OVER.match(line)
        if m:
            t, result = m.group(1), m.group(2)
            at = datetime.datetime.strptime(t, "%Y-%m-%d %H:%M:%S")
            run = next((by_time[k] for d in range(-2, 8) if (k := (at + datetime.timedelta(seconds=d)).strftime("%Y-%m-%d %H:%M:%S")) in by_time), None)
            if cur:
                cur["died"], cur["hp_out"] = result == "loss", 0 if result == "loss" else None
            for f in in_run:
                f.update(run=run, char=run and run["character"].replace("The ", ""), asc=run and run.get("ascension"), result=result)
            cur, in_run = None, []
    for f in in_run:
        f.update(run=None, char=None, asc=None, result=None)
    return fights, runs


def bosses(min_ascension: int = 0) -> None:
    table = collections.defaultdict(lambda: [0, 0])
    for f in load()[0]:
        if f["kind"] == "boss" and "died" in f and (f["asc"] or 0) >= min_ascension:
            for key in ((f["act"], f["enemies"], "all"), (f["act"], f["enemies"], f["char"])):
                table[key][0] += not f["died"]
                table[key][1] += 1
    for (act, boss, who), (won, n) in sorted(table.items(), key=lambda kv: (kv[0][0], kv[0][1], kv[0][2] != "all", str(kv[0][2]))):
        print(f"act {act} {boss[:28]:28} {who or '?':12} {won}/{n}")


def costs() -> None:
    table = collections.defaultdict(lambda: [0, 0, [], []])
    for f in load()[0]:
        if "died" not in f or not f["enemies"] or f["kind"] == "boss":
            continue
        row = table[(f["act"], f["kind"], f["enemies"][:56])]
        row[0] += 1
        row[1] += f["died"]
        row[3].append(f["rounds"])
        if not f["died"] and f["hp_out"] is not None:
            row[2].append((f["hp_in"] - f["hp_out"]) / f["max_hp"])
    rows = [(k, v, sum(v[2]) / max(1, len(v[2]))) for k, v in table.items() if v[0] >= 4]
    for (act, kind, name), v, cost in sorted(rows, key=lambda r: (r[0][0], -r[2])):
        print(f"act {act} {kind:7} {name:56} fights {v[0]:3}  deaths {v[1]}  cost {100 * cost:3.0f}% of Max HP  rounds {sum(v[3]) / len(v[3]):.1f}")


def show(start: str, kinds: tuple = ("boss",)) -> None:
    out, plays, rnd, on = [], [], None, False

    def flush() -> None:
        if plays:
            out.append("      plays: " + " > ".join(plays))
            plays.clear()

    for line in lines():
        if not on:
            m = ACT.match(line)
            if m and line[: len(start)] >= start and m.group(4) in kinds:
                on = True
                out.append(line[11:])
            continue
        if ACT.match(line) or OVER.match(line) or "boss fight over" in line:
            flush()
            out.append(line[11:])
            if "boss fight over" in line:
                continue
            break
        m = COMBAT.match(line)
        if m:
            if m.group(2) != rnd:
                flush()
                rnd = m.group(2)
                out.append(f"r{rnd} hp {m.group(3)}/{m.group(4)} blk {m.group(5)} en {m.group(6)}{m.group(7)} | {m.group(8)} | {m.group(9)}")
            continue
        if m := re.search(r"DEBUG play_card .*-> Playing '([^']+)'", line):
            plays.append(m.group(1))
        elif m := re.search(r"Using potion '([^']+)'", line):
            plays.append("POTION " + m.group(1))
        elif "DEBUG ending r" in line:
            plays.append("[" + line.split("DEBUG ")[1] + "]")
        elif "hand select" in line or "DEBUG choose" in line:
            plays.append("{" + line.split("DEBUG ")[1][:160] + "}")
        elif m := re.search(r"Selecting card from hand: (.*)|Choosing card: (.*)", line):
            plays.append(f"<pick {m.group(1) or m.group(2)}>")
    flush()
    print("\n".join(out))


def deaths(since: str = "") -> None:
    for f in load()[0]:
        if f.get("died") and f["t"] >= since:
            knobs = (f.get("run") or {}).get("knobs") or {}
            settings = " ".join(f"{k}={v}" for k, v in knobs.items())
            print(f"{f['t'][5:16]} {str(f.get('char'))[:11]:11} a{f.get('asc')} act {f['act']} floor {f['floor']:2} {f['kind']:7} "
                  f"{str(f['enemies'])[:34]:34} rounds {f['rounds']:2}  entered on {f['hp_in']}/{f['max_hp']}  {settings}")


def knobs(since: str = "") -> None:
    """Per setting and value: runs, where they ended, and the bosses they got past.

    A run that died at a boss met it; one that ended later got past it. Runs in
    progress at a restart have no settings recorded and are left out.
    """
    with open(os.path.join(LOGS, "runs.jsonl")) as f:
        runs = [r for r in map(json.loads, f) if r.get("knobs") and r["time"] >= since]
    names = sorted({k for r in runs for k in r["knobs"]})
    for name in names:
        table = collections.defaultdict(list)
        for r in runs:
            if name in r["knobs"]:
                table[r["knobs"][name]].append(r)
        for value, rs in sorted(table.items()):
            won = sum(r["result"] == "win" for r in rs)
            met2 = sum((r["act"] == 2 and r["floor"] == 33) or r["act"] >= 3 for r in rs)
            met3 = sum((r["act"] == 3 and r["floor"] == 48) or r["result"] == "win" for r in rs)
            print(f"{name:14} {value:5}  runs {len(rs):3}  mean floor {sum(r['floor'] for r in rs) / len(rs):4.1f}  past act 1 {sum(r['act'] >= 2 for r in rs):3}/{len(rs):<3}"
                  f"  past the act 2 boss {sum(r['act'] >= 3 for r in rs):2}/{met2:<2}  last boss {won}/{met3}")


PLAYED = re.compile(r"DEBUG play_card (\{.*?\}) -> Playing '(.+?)'(?: targeting .*)?$")
IN_HAND = re.compile(r"(\d+):(.+?)\((\w+)(!?)((?: [db]\d+(?:x\d+)?)*)\)(?:, |$)")


def _enemy_hp(enemies: str) -> int:
    """HP of everything on the other side (an enemy "on 999999999" is one already beaten)."""
    return sum(int(x) for x in re.findall(r" (\d+)hp/", enemies) if int(x) < 100_000)


def pace(since: str = "") -> None:
    table = collections.defaultdict(list)
    for f in load()[0]:
        if f["kind"] == "monster" and f["turns"] and "died" in f and f["t"] >= since:
            dealt = _enemy_hp(f["turns"][0][5]) - (_enemy_hp(f["turns"][-1][5]) if f["died"] else 0)
            table[f["act"]].append((f["rounds"], dealt / max(1, f["rounds"]), f["died"]))
    for act, rows in sorted(table.items()):
        print(f"act {act}: ordinary fights {len(rows):4}  rounds {sum(r[0] for r in rows) / len(rows):.2f}  enemy HP removed a round {sum(r[1] for r in rows) / len(rows):.1f}  deaths {sum(r[2] for r in rows)}")


def _turns():
    """(kind of fight, first state of a turn, [(state before a play, card name, state after)], last state) for every turn ended."""
    kind = first = last = None
    plays, pending = [], None
    for line in lines():
        m = ACT.match(line)
        if m:
            kind, first, last, plays, pending = m.group(4), None, None, [], None
            continue
        m = COMBAT.match(line)
        if m:
            if first is None or first.group(2) != m.group(2):
                first, plays = m, []
            if pending is not None:
                plays.append((pending[0], pending[1], m))
                pending = None
            last = m
            continue
        m = PLAYED.search(line)
        if m and last is not None:
            pending = (last, m.group(2))
        elif "DEBUG end_turn" in line and "->" in line and first is not None:
            yield kind, first, plays, last
            first = last = pending = None


def reads() -> None:
    """Single-enemy plays only, with nothing on the enemy that bends damage: read against done."""
    odd = ("SLIPPERY", "INTANGIBLE", "HARD_TO_KILL", "FLUTTER", "SOAR", "SKITTISH", "BURROWED", "PLATING", "ASLEEP", "ADAPTABLE", "SLOW", "VULNERABLE", "HANG")
    table = collections.defaultdict(list)
    for _kind, _first, plays, _last in _turns():
        for before, name, after in plays:
            if " ; " in before.group(8) or any(o in before.group(8) for o in odd) or _enemy_hp(after.group(8)) == 0:
                continue
            card = next((c for c in IN_HAND.finditer(before.group(9)) if c.group(2).strip() == name), None)
            if card is None:
                continue
            d = re.search(r"d(\d+)(?:x(\d+))?", card.group(5))
            b = re.search(r"b(\d+)", card.group(5))
            took = lambda g: _enemy_hp(g.group(8)) + sum(int(x) for x in re.findall(r"hp/(\d+)blk", g.group(8)))
            table[name.rstrip("+")].append((int(d.group(1)) * int(d.group(2) or 1) if d else 0, took(before) - took(after), int(b.group(1)) if b else 0, int(after.group(5)) - int(before.group(5))))
    rows = [(n, len(v), *(sum(x[i] for x in v) / len(v) for i in range(4))) for n, v in table.items() if len(v) >= 5]
    for n, k, rd, dd, rb, gb in sorted(rows, key=lambda r: -(abs(r[2] - r[3]) + abs(r[4] - r[5])))[:50]:
        print(f"{n:24} n {k:4}  damage read {rd:5.1f} did {dd:5.1f}   Block read {rb:5.1f} got {gb:5.1f}")


def unplayed(kinds: tuple = ("boss", "elite")) -> None:
    played, left = collections.Counter(), collections.Counter()
    for kind, first, plays, last in _turns():
        if kind not in kinds:
            continue
        played.update(name.rstrip("+") for _before, name, _after in plays)
        energy = int(first.group(6))
        for c in IN_HAND.finditer(last.group(9)):
            if not c.group(4) and c.group(3).isdigit() and int(c.group(3)) <= energy:
                left[c.group(2).strip().rstrip("+")] += 1
    rows = [(left[n] / (left[n] + played[n]), n) for n in set(left) | set(played) if left[n] + played[n] >= 12]
    for share, n in sorted(rows, reverse=True)[:45]:
        print(f"{n:24} played {played[n]:4}  left {left[n]:4}  ({100 * share:.0f}% left)")


if __name__ == "__main__":
    args = sys.argv[1:]
    if args[:1] == ["bosses"]:
        bosses(int(args[1]) if len(args) > 1 else 0)
    elif args[:1] == ["costs"]:
        costs()
    elif args[:1] == ["deaths"]:
        deaths(args[1] if len(args) > 1 else "")
    elif args[:1] == ["knobs"]:
        knobs(args[1] if len(args) > 1 else "")
    elif args[:1] == ["pace"]:
        pace(args[1] if len(args) > 1 else "")
    elif args[:1] == ["reads"]:
        reads()
    elif args[:1] == ["unplayed"]:
        unplayed(tuple(args[1].split(",")) if len(args) > 1 else ("boss", "elite"))
    elif args[:1] == ["show"] and len(args) > 1:
        show(args[1], tuple(args[2].split(",")) if len(args) > 2 else ("boss",))
    else:
        print(__doc__)
