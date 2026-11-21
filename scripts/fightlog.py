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

    python3 scripts/fightlog.py rooms SETTING ['YYYY-MM-DD HH:MM']
                                                        ordinary fights under each value of one setting, by
                                                        character and act: rounds, enemy HP a round, HP cost
                                                        (what a deck-building setting does long before the
                                                        bosses say anything)
    python3 scripts/fightlog.py surprises ['YYYY-MM-DD']
                                                        turns that cost 3 HP or more beyond what the plan
                                                        expected, by enemy: mechanics the planner does not model
    python3 scripts/fightlog.py idle ['YYYY-MM-DD']     turns ended with energy and a card it could pay for
                                                        while 8 HP or more was about to be lost: a penalty
                                                        that stops the planner playing at all
    python3 scripts/fightlog.py pit ['YYYY-MM-DD HH:MM']
                                                        the Insatiable's fights: won, lost on HP or eaten by the
                                                        Sandpit, the boss's HP at the end, and the Sandpit round
                                                        by round (E: a Frantic Escape was in that hand)
    python3 scripts/fightlog.py artifact ['YYYY-MM-DD HH:MM'] [boss,elite,monster]
                                                        fights against Artifact: the charges on the enemies as each
                                                        round began, and the round the last one was gone by
    python3 scripts/fightlog.py crabs ['YYYY-MM-DD HH:MM']
                                                        Crusher and Rocket: which crab died first, in which round and
                                                        with the other on how much, then both crabs' HP round by round
    python3 scripts/fightlog.py bosspace ['YYYY-MM-DD HH:MM']
                                                        boss fights by how much HP the boss lost a round (what
                                                        separates won from lost), then that under each value of
                                                        each setting, next to what the character usually deals
                                                        to that boss: says in 30 fights what `knobs` needs 300 runs for
    python3 scripts/fightlog.py powers                  powers seen on enemies (and on us) that no module of the
                                                        bot names, with their rules text: mechanics nobody modelled
    python3 scripts/fightlog.py events                  every event met: how often each option was taken, with
                                                        the scores of the last time (to spot a choice never made)
    python3 scripts/fightlog.py ancients                the Ancients' relics: each one's rating, how the runs that
                                                        held it went against the usual, and where that moves it
    python3 scripts/fightlog.py wiki                    save the game's text for every discovered card and relic
                                                        to wiki.json next to the logs (one request at a time: a
                                                        pool of them stalls the game while the bot is playing)

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


def rooms(name: str, since: str = "") -> None:
    table = collections.defaultdict(list)
    for f in load()[0]:
        value = ((f.get("run") or {}).get("knobs") or {}).get(name)
        if f["kind"] != "monster" or not f["turns"] or "died" not in f or value is None or f["t"] < since:
            continue
        out = 0 if f["died"] else f["hp_out"]
        if out is None:
            continue
        dealt = _enemy_hp(f["turns"][0][5]) - (_enemy_hp(f["turns"][-1][5]) if f["died"] else 0)
        for who in (f["char"], "all"):
            table[(who, f["act"], value)].append((f["rounds"], dealt / max(1, f["rounds"]), (f["hp_in"] - out) / f["max_hp"], f["died"]))
    for (who, act, value), v in sorted(table.items(), key=lambda kv: (kv[0][0] == "all", str(kv[0][0]), kv[0][1], kv[0][2])):
        if len(v) >= 8:
            print(f"{who:12} act {act} {name} {value:4}: fights {len(v):3}  rounds {sum(x[0] for x in v) / len(v):.2f}  enemy HP a round {sum(x[1] for x in v) / len(v):5.1f}"
                  f"  cost {100 * sum(x[2] for x in v) / len(v):4.1f}% of Max HP  deaths {sum(x[3] for x in v)}")


ENDING = re.compile(r"^(\S+ \S+) DEBUG ending r(\d+) on (\d+) HP, expecting to lose (\d+)")


def _endings():
    """(time, kind of fight, enemy names, the last combat state of the turn, HP expected to be lost, HP the next round began on or None)."""
    kind, last, pending = None, None, None
    for line in lines():
        m = ACT.match(line)
        if m or OVER.match(line):
            if pending:
                yield (*pending, None)
            kind, last, pending = (m.group(4) if m else kind), None, None
            continue
        m = COMBAT.match(line)
        if m:
            if pending and int(m.group(2)) == int(pending[3].group(2)) + 1:
                yield (*pending, int(m.group(3)))
                pending = None
            last = m
            continue
        m = ENDING.match(line)
        if m and last is not None:
            names = "+".join(sorted({re.sub(r"_\d+$", "", e.split(" ")[0]) for e in last.group(8).split(" ; ")}))
            pending = (m.group(1), kind, names, last, int(m.group(4)))


def surprises(since: str = "") -> None:
    rows = [(t, kind, names, last, expected, int(last.group(3)) - after) for t, kind, names, last, expected, after in _endings() if after is not None and t >= since]
    under = [r for r in rows if r[5] - r[4] >= 3]
    print(f"turns {len(rows)}; lost 3 HP or more beyond the plan: {len(under)}; 3 or more less: {sum(r[4] - r[5] >= 3 for r in rows)}")
    turns = collections.Counter(r[2][:44] for r in rows)
    for name, n in collections.Counter(r[2][:44] for r in under).most_common(15):
        print(f"  {name:44} {n:3} of {turns[name]:4} turns")
    for t, kind, _names, last, expected, lost in under[-12:]:
        print(f"{t[5:]} {kind} expected {expected} lost {lost} | r{last.group(2)} hp {last.group(3)} blk {last.group(5)} en {last.group(6)}{last.group(7)} | {last.group(8)[:150]} | {last.group(9)[:160]}")


def idle(since: str = "") -> None:
    rows = []
    for t, kind, names, last, expected, _after in _endings():
        energy = int(last.group(6))
        spare = [c.group(2).strip() for c in IN_HAND.finditer(last.group(9)) if not c.group(4) and c.group(3).isdigit() and int(c.group(3)) <= energy and c.group(5).strip()]
        if t >= since and energy >= 1 and spare and expected >= 8:
            rows.append((t, kind, names, last, expected))
    print(collections.Counter(r[2][:40] for r in rows).most_common(15))
    for t, kind, _names, last, expected in rows[-25:]:
        print(f"{t[5:]} {kind} energy {last.group(6)} expects to lose {expected} | hp {last.group(3)}{last.group(7)} | {last.group(8)[:150]} | {last.group(9)[:170]}")


def pit(since: str = "") -> None:
    fight, tally = [], collections.Counter()
    for line in lines():
        if " combat r" in line and "THE_INSATIABLE_0" in line or (fight and " ending r" in line):
            fight.append(line)
        elif "boss fight over" in line and "INSATIABLE" in line and fight:
            states = [x for x in fight if " combat r" in x]
            last, end, shown = states[-1], [x for x in fight if " ending r" in x][-1:], {}
            for x in states:  # the first state of each round: the Sandpit as the turn began
                rnd, sand = re.search(r"combat r(\d+) ", x).group(1), re.search(r"SANDPIT:(\d+)", x)
                shown.setdefault(rnd, (sand.group(1) if sand else "-") + ("E" if "Frantic Escape" in x.rsplit("| hand", 1)[-1] else ""))
            sand, hp = re.search(r"SANDPIT:(\d+)", last), int(re.search(r" hp (\d+)/", last).group(1))
            expect = int(re.search(r"expecting to lose (\d+)", end[0]).group(1)) if end else 0
            how = "won" if " won;" in line else "eaten" if sand and int(sand.group(1)) <= 1 and hp > expect else "lost"
            fight = []
            if line[:16] < since:
                continue
            tally[how] += 1
            boss = re.search(r"boss hp \d+ -> (\d+)", line).group(1)
            print(f"{line[5:16]} {how:5} rounds {len(shown):2}  boss left {boss:>3}  hp {hp:2}  Sandpit at the end {sand.group(1) if sand else '-'}  | {' '.join(shown.values())}")
    print(dict(tally))


def artifact(since: str = "", kinds: tuple = ("boss", "elite")) -> None:
    gone = collections.defaultdict(list)
    for f in load()[0]:
        rounds = {}
        for t in f["turns"]:
            rounds.setdefault(t[0], sum(map(int, re.findall(r"ARTIFACT:(\d+)", t[5]))))
        left = [rounds[r] for r in sorted(rounds)]
        if "died" not in f or not left or not left[0] or f["t"] < since:
            continue
        by = next((n + 1 for n, a in enumerate(left) if not a), None)
        gone[f["kind"]].append(((by or len(left) + 1) - 1, len(left)))
        if f["kind"] in kinds:
            print(f"{f['t'][5:16]} {f['kind']:7} {f['enemies'][:24]:24} {str(f['char']):11} {'lost' if f['died'] else 'won ':4} rounds {len(left):2}  gone by {by or '-':>2}  | {' '.join(map(str, left))}")
    for kind, rows in sorted(gone.items()):
        print(f"{kind:7} {len(rows):3} fights: Artifact stood for {sum(r[0] for r in rows) / len(rows):.1f} of {sum(r[1] for r in rows) / len(rows):.1f} rounds")


def crabs(since: str = "") -> None:
    tally = collections.Counter()
    for f in load()[0]:
        if f["enemies"] != "CRUSHER+ROCKET" or "died" not in f or f["t"] < since:
            continue
        rounds = {}
        for t in f["turns"]:
            hp = {m.group(1): int(m.group(2)) for m in re.finditer(r"(CRUSHER|ROCKET)_\d+ (\d+)hp", t[5])}
            rounds.setdefault(t[0], (hp.get("CRUSHER", 0), hp.get("ROCKET", 0)))
        path = [rounds[r] for r in sorted(rounds)]
        at = next((n for n, (c, r) in enumerate(path) if not c or not r), None)
        first = "neither" if at is None else "Crusher" if not path[at][0] else "Rocket"
        tally[(first, "lost" if f["died"] else "won")] += 1
        when = f"by round {at + 1:2}, the other on {max(path[at]):3}" if at is not None else " " * 31
        print(f"{f['t'][5:16]} {str(f['char']):11} {'lost' if f['died'] else 'won ':4} rounds {len(path):2}  first to die: {first:7} {when} | {' '.join(f'{c}/{r}' for c, r in path)}")
    print({" ".join(k): v for k, v in sorted(tally.items())})


def powers() -> None:
    root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "sts2bot")
    code = "".join(open(os.path.join(root, n)).read() for n in sorted(os.listdir(root)) if n.endswith(".py"))
    seen: dict = {}
    for line in lines():
        m = re.search(r" INFO power (\w+?)(?:_POWER)? \(on (.+?)\): (.*)$", line)
        if m and f'"{m.group(1)}"' not in code:
            ours = m.group(2).startswith("The ") and m.group(2).split()[-1] in ("Ironclad", "Silent", "Regent", "Necrobinder", "Defect")
            seen.setdefault((ours, m.group(1)), (m.group(2), m.group(3)))
    for (ours, name), (who, text) in sorted(seen.items()):
        print(f"{'on us    ' if ours else 'on enemy '} {name:28} {who[:22]:22} {text[:150]}")


def events() -> None:
    met: dict = {}
    last = None
    for line in lines():
        m = re.search(r" DEBUG event (\w+): (.*)$", line)
        if m:
            last = m.groups()
            continue
        m = re.search(r"Choosing event option: (.*)$", line)
        if m and last and m.group(1).strip() != "Proceed":
            entry = met.setdefault(last[0], [collections.Counter(), ""])
            entry[0][m.group(1).strip()[:40]] += 1
            entry[1], last = last[1], None
    for name, (taken, text) in sorted(met.items(), key=lambda kv: -sum(kv[1][0].values())):
        print(f"{name:28} {dict(taken)}\n      {text[:400]}")


def wiki() -> None:
    import time
    import urllib.parse
    import urllib.request

    def get(path: str) -> dict:
        with urllib.request.urlopen("http://localhost:15526/api/v1" + path, timeout=20) as r:
            return json.load(r)

    profile, out = get("/profile"), {"cards": {}, "relics": {}}
    for kind in ("card", "relic"):
        for item in profile.get(f"discovered_{kind}s") or []:
            key = item if isinstance(item, str) else item.get("id")
            found = get(f"/wiki?q={urllib.parse.quote(str(key))}&type={kind}").get("results") or []
            out[kind + "s"].update({key: hit for hit in found if hit.get("id") == key})
            time.sleep(0.05)
    with open(os.path.join(LOGS, "wiki.json"), "w") as f:
        json.dump(out, f)
    print(len(out["cards"]), "cards and", len(out["relics"]), "relics in", os.path.join(LOGS, "wiki.json"))


def ancients() -> None:
    sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
    from pathlib import Path

    from sts2bot import events as ev, relics

    relics.load(Path(LOGS) / "runs.jsonl", ev._ANCIENT)
    rec = relics.record()
    later = relics.ACT2 | relics.ACT3 | relics.DARV
    for title, pool in (("Neow (floor 1)", None), ("act 2: Orobas, Pael, Tezcatara", relics.ACT2), ("Darv (act 2 or 3)", relics.DARV), ("act 3: Nonupeipe, Tanx, Vakuu", relics.ACT3)):
        print(title)
        names = [k for k in ev._ANCIENT if (k in pool if pool is not None else k not in later)]
        for k in sorted(names, key=lambda k: -(ev._ANCIENT[k] + relics.shift(k))):
            n, gain = rec.get(k, (0, 0.0))
            print(f"   {k:20} {ev._ANCIENT[k]:5.1f} {relics.shift(k):+5.1f} = {ev._ANCIENT[k] + relics.shift(k):5.1f}   held {n:3}" + (f"  {gain:+5.1f} floors a run" if n else ""))


def boss_pace(f: dict) -> float:
    """HP the other side lost a round in this fight, from one round's first look to the next (a heal or a new form is not a loss)."""
    first = {}
    for turn in f["turns"]:
        first.setdefault(turn[0], _enemy_hp(turn[5]))
    hp = [first[r] for r in sorted(first)]
    return sum(max(0, a - b) for a, b in zip(hp, hp[1:])) / max(1, len(hp) - 1)


def bosspace(since: str = "") -> None:
    rows = [(f, boss_pace(f)) for f in load()[0] if f["kind"] == "boss" and "died" in f and f["rounds"] >= 3 and f["char"] and f["t"] >= since]
    for act in sorted({f["act"] for f, _ in rows}):
        sub = [(f, p) for f, p in rows if f["act"] == act]
        bands = [(0, 20), (20, 25), (25, 30), (30, 35), (35, 45), (45, 999)]
        won = lambda v: f"{sum(not f['died'] for f, _ in v)}/{len(v)}"
        print(f"act {act}: {len(sub)} boss fights, {sum(p for _, p in sub) / len(sub):.1f} a round; won by HP a round: " + "  ".join(f"{lo}-{hi if hi < 999 else ''}: {won([x for x in sub if lo <= x[1] < hi])}" for lo, hi in bands))
        usual = collections.defaultdict(list)
        for f, p in sub:
            usual[f["char"], f["enemies"]].append(p)
        usual = {k: sum(v) / len(v) for k, v in usual.items()}
        names = sorted({k for f, _ in sub for k in ((f["run"] or {}).get("knobs") or {})})
        for name in names:
            arms = collections.defaultdict(list)
            for f, p in sub:
                value = ((f["run"] or {}).get("knobs") or {}).get(name)
                if value is not None:
                    arms[value].append((p - usual[f["char"], f["enemies"]], not f["died"]))
            if len(arms) < 2:
                continue
            cells = []
            for value, v in sorted(arms.items()):
                mean = sum(d for d, _ in v) / len(v)
                se = (sum((d - mean) ** 2 for d, _ in v) / max(1, len(v) - 1) / len(v)) ** 0.5
                cells.append(f"{value}: {mean:+5.1f} a round (se {se:.1f}), won {sum(w for _, w in v)}/{len(v)}")
            print(f"    {name:14} " + "   ".join(cells))


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
    elif args[:1] == ["rooms"] and len(args) > 1:
        rooms(args[1], args[2] if len(args) > 2 else "")
    elif args[:1] == ["surprises"]:
        surprises(args[1] if len(args) > 1 else "")
    elif args[:1] == ["idle"]:
        idle(args[1] if len(args) > 1 else "")
    elif args[:1] == ["pit"]:
        pit(args[1] if len(args) > 1 else "")
    elif args[:1] == ["bosspace"]:
        bosspace(args[1] if len(args) > 1 else "")
    elif args[:1] == ["crabs"]:
        crabs(args[1] if len(args) > 1 else "")
    elif args[:1] == ["artifact"]:
        artifact(args[1] if len(args) > 1 else "", tuple(args[2].split(",")) if len(args) > 2 else ("boss", "elite"))
    elif args[:1] in (["powers"], ["events"], ["wiki"], ["ancients"]):
        {"powers": powers, "events": events, "wiki": wiki, "ancients": ancients}[args[0]]()
    elif args[:1] == ["show"] and len(args) > 1:
        show(args[1], tuple(args[2].split(",")) if len(args) > 2 else ("boss",))
    else:
        print(__doc__)
