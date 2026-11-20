#!/usr/bin/env python3
"""Read the bot's logs fight by fight (needs the bot to have run with -v).

    python3 scripts/fightlog.py bosses [ASCENSION]      win/loss per boss and character
    python3 scripts/fightlog.py costs                   HP cost and deaths per ordinary encounter
    python3 scripts/fightlog.py show 'YYYY-MM-DD HH:MM' [boss,elite,monster]
                                                        the first such fight from that time on: one
                                                        line per round (what was in hand), then the plays

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


if __name__ == "__main__":
    args = sys.argv[1:]
    if args[:1] == ["bosses"]:
        bosses(int(args[1]) if len(args) > 1 else 0)
    elif args[:1] == ["costs"]:
        costs()
    elif args[:1] == ["show"] and len(args) > 1:
        show(args[1], tuple(args[2].split(",")) if len(args) > 2 else ("boss",))
    else:
        print(__doc__)
