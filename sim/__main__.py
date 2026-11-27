"""`python3 -m sim`: play model runs, report on them, or print a digest of every decision.

    python3 -m sim run --game sts1 --character IRONCLAD --runs 1000 --jobs 4 --out sim-runs.jsonl
    python3 -m sim run --game sts2 --character all --runs 1000 --vary      # each run draws its own value of every setting
    python3 -m sim report sim-runs.jsonl [--md docs/strategy/simulation.md]
    python3 -m sim trace --game sts1 --runs 2                             # one hash per character: changes iff a decision does
    python3 -m sim prior sts2.jsonl sts1.jsonl --out sts2bot/run/model_cards.json   # the card table `model_lean` weighs in
    python3 -m sim prior runs.jsonl --onto sts2bot/run/model_cards.json --out new.json  # ... plus what runs that played it still say
    python3 -m sim run --game sts2 --vary --only model_table,caution --alt-cards new.json   # two tables (and caution) drawn per run
    python3 -m sim run --game sts1 --vary --only caution,belt_spare --grid caution=1,1.35       # ... caution from these two values only
"""

from __future__ import annotations

import argparse
import hashlib
import importlib
import json
import logging
import multiprocessing
import os
import random
import sys
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))

from sts2bot import games, learn

GRID = {  # the settings tried with --vary: the bot's own choices (learn.CHOICES) and a value on either side where that is cheap
    "caution": [0.75, 1.0, 1.35],
    "elite_worth": [0.8, 2.5, 6.0],
    "upgrade_worth": [1.3, 2.5, 4.0],
    "pickiness": [-1.0, 0.0, 1.0],
    "crowd": [0.0, 0.3, 0.6],
    "punch": [0.9, 2.7],
    "weak_want": [0.0, 1.0],
    "free_lean": [0.0, 1.0],
    "attack_room": [3.0, 99.0],
    "block_want": [0.0, 1.5],
    "cards_first": [0.0, 1.0],
    "debuff_worth": [1.0, 2.0, 3.0],
    "model_lean": [0.0, 1.0, 2.0],
    "belt_spare": [0.0, 1.0],  # a full belt is a reason to drink in a hallway fight (round 5: 0 in Slay the Spire 2, 1 in the first game)
}


def content(game: str):
    games.select(game)
    return importlib.import_module(f"sim.content.{game}")


TABLES: dict = {}  # model_table setting -> the card table learn.model_shift reads (1, 2, ...: --alt-cards)


def _job(args: tuple) -> dict:
    game, character, seed, knobs, asc = args
    from .run import Run
    logging.disable(logging.CRITICAL)
    if TABLES:
        learn._model = TABLES[int(knobs.get("model_table", 0))]
    try:
        return Run(content(game), character, seed, knobs, asc).play()
    except Exception as e:  # a model bug must not end a thousand-run batch: it is counted and reported instead
        import traceback
        return {"game": game, "character": character, "seed": seed, "knobs": knobs, "error": f"{type(e).__name__}: {e}", "trace": traceback.format_exc()[-1500:]}


def knobs_for(seed: int, vary: bool) -> dict:
    if not vary:
        return {}
    rng = random.Random(seed * 7919 + 17)
    return {k: rng.choice(v) for k, v in GRID.items()}


def fixed(a, character: str) -> dict:
    """Settings given on the command line (--set NAME=VALUE) and, per character, in a JSON file (--settings)."""
    out = {}
    if a.settings:
        with open(a.settings) as f:
            out.update(json.load(f).get(character, {}))
    for item in a.set or []:
        name, value = item.split("=", 1)
        out[name] = float(value)
    return out


def cmd_run(a) -> int:
    mod = content(a.game)
    if a.alt_cards:  # a second card table to set against the shipped one: `--set model_table=1`, or drawn per run with --vary
        TABLES.update({0: json.load(open(learn.MODEL_CARDS)), **{n: json.load(open(f)) for n, f in enumerate(a.alt_cards.split(","), 1)}})
        GRID["model_table"] = sorted(TABLES)
    if a.only:  # --vary over these settings alone, the rest at each game's defaults
        for name in set(GRID) - set(a.only.split(",")):
            del GRID[name]
    for item in a.grid or []:  # --grid NAME=V1,V2: the values --vary draws this setting from
        name, values = item.split("=", 1)
        GRID[name] = [float(v) for v in values.split(",")]
    chars = list(mod.CHARACTERS) if a.character == "all" else [c.strip().upper() for c in a.character.split(",")]
    # (Interleaved, so that every character's numbers grow together; seeds are shared by characters and arms.)
    jobs = [(a.game, ch, a.seed + n, {**knobs_for(a.seed + n, a.vary), **fixed(a, ch)}, a.ascension) for n in range(a.runs) for ch in chars]
    out = open(a.out, "a") if a.out else sys.stdout
    t0, done, errors = time.time(), 0, 0
    # Forked workers: under forkserver or spawn (Python 3.14's default on Linux) a worker cannot find _job, which lives in __main__.
    ctx = multiprocessing.get_context("fork" if "fork" in multiprocessing.get_all_start_methods() else None)
    with ctx.Pool(a.jobs) as pool:
        for r in pool.imap_unordered(_job, jobs, chunksize=1):
            done += 1
            errors += "error" in r
            out.write(json.dumps(r) + "\n")
            out.flush()
            if a.out and (done % 50 == 0 or done == len(jobs)):
                print(f"{done}/{len(jobs)} runs, {errors} errors, {time.time() - t0:.0f}s", file=sys.stderr)
    return 0


def cmd_trace(a) -> int:
    """A hash of every decision in a few seeded runs per character: equal before and after a change iff the bot chose alike."""
    from .run import Run
    logging.disable(logging.CRITICAL)
    for game in a.game.split(","):
        mod = content(game)
        for ch in mod.CHARACTERS:
            h = hashlib.sha256()
            for n in range(a.runs):
                r = Run(mod, ch, a.seed + n, {}, a.ascension)
                result = r.play()
                h.update(("|".join(r.digest) + json.dumps(result.get("deck"))).encode())
            print(f"{game} {ch:12s} {h.hexdigest()[:16]}")
    return 0


def cmd_prior(a) -> int:
    """learn.MODEL_CARDS from run files: per game and character, the floors each card's picks gained over the runs that left the same
    floor alive (report.card_table's measure), less the character's average pick, shrunk by `--prior` picks and capped. Runs that
    played with the table (model_lean) are left out, so that it never learns from itself."""
    from collections import defaultdict
    from .report import load, score
    from sts2bot.cards import key_of
    groups = defaultdict(list)
    base = json.load(open(a.onto)) if a.onto else None
    for r in load(a.files):
        lean = r.get("model_lean", (r.get("knobs") or {}).get("model_lean", 0.0))
        if base is not None:  # --onto: only runs that played the base table at its game's own lean (learn.GAME_DEFAULTS)
            keep = lean and lean == learn.GAME_DEFAULTS.get(r["game"], {}).get("model_lean") and (r.get("knobs") or {}).get("model_table", 0) == a.table
        else:
            keep = not lean  # (not runs that leaned on the table)
        if "error" not in r and keep:
            groups[(r["game"], r["character"])].append(r)
    out: dict = json.loads(json.dumps(base)) if base is not None else {}
    for (game, ch), runs in sorted(groups.items()):
        mod = content(game)
        usual, gained = {}, defaultdict(list)
        for r in runs:
            for floor, key in r.get("picks") or []:
                if floor not in usual:
                    alive = [score(x) for x in runs if x["floor"] > floor or x["won"]]
                    usual[floor] = sum(alive) / max(1, len(alive))
                gained[key].append(score(r) - usual[floor])
        picks = sum(map(len, gained.values()))
        par = sum(map(sum, gained.values())) / max(1, picks)
        who = learn.who_key(mod.CHARACTERS[ch]["title"])
        # On top of a base table (--onto), what the runs that leaned on it still say: a card whose picks still gained moves up.
        table = dict((base or {}).get(game, {}).get(who, {}))
        for key, g in gained.items():
            name = key_of(mod.CARDS[key].name if key in mod.CARDS else key)
            shift = max(-a.most, min(a.most, table.get(name, 0.0) + (sum(g) - par * len(g)) / (len(g) + a.prior)))
            table[name] = round(shift, 2)
        out.setdefault(game, {})[who] = dict(sorted((k, v) for k, v in table.items() if abs(v) >= 0.05))
        print(f"{game} {ch}: {len(runs)} runs, {picks} picks, {len(table)} cards", file=sys.stderr)
    text = json.dumps(out, indent=1, sort_keys=True) + "\n"
    if a.out:
        with open(a.out, "w") as f:
            f.write(text)
    else:
        sys.stdout.write(text)
    return 0


def cmd_report(a) -> int:
    from .report import report
    text = report(a.files, a.min_runs)
    if a.md:
        with open(a.md, "w") as f:
            f.write(text)
    print(text)
    return 0


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(prog="python3 -m sim", description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run", help="play model runs and write one JSON line per run")
    r.add_argument("--game", choices=games.GAMES, default="sts1")
    r.add_argument("--character", default="all", help="a character id, several joined by commas, or all")
    r.add_argument("--runs", type=int, default=100, help="runs per character")
    r.add_argument("--seed", type=int, default=1)
    r.add_argument("--ascension", type=int, default=0)
    r.add_argument("--vary", action="store_true", help="each run draws its own value of every setting in GRID")
    r.add_argument("--only", metavar="NAMES", help="with --vary: vary only these settings (comma-separated)")
    r.add_argument("--grid", action="append", metavar="NAME=V1,V2", help="with --vary: draw this setting from these values (repeatable)")
    r.add_argument("--set", action="append", metavar="NAME=VALUE", help="play every run with this setting (repeatable)")
    r.add_argument("--settings", metavar="FILE", help="JSON {character: {setting: value}} to play each character with")
    r.add_argument("--alt-cards", metavar="FILE", help="another card table (sim prior) for runs with model_table 1 (several, comma-separated: 1, 2, ...)")
    r.add_argument("--jobs", type=int, default=os.cpu_count() or 2)
    r.add_argument("--out", default=None)
    t = sub.add_parser("trace", help="hash every decision of a few seeded runs (a refactor must leave these unchanged)")
    t.add_argument("--game", default="sts1,sts2")
    t.add_argument("--runs", type=int, default=2)
    t.add_argument("--seed", type=int, default=1)
    t.add_argument("--ascension", type=int, default=0)
    p = sub.add_parser("report", help="summarise run files")
    p.add_argument("files", nargs="+")
    p.add_argument("--md", default=None)
    p.add_argument("--min-runs", type=int, default=30)
    q = sub.add_parser("prior", help="the model's card table the bot reads (learn.MODEL_CARDS) from run files")
    q.add_argument("files", nargs="+")
    q.add_argument("--out", default=None)
    q.add_argument("--prior", type=float, default=40.0, help="picks before a card's own record counts for half")
    q.add_argument("--most", type=float, default=3.0, help="no card moves further than this")
    q.add_argument("--onto", metavar="TABLE", help="add to this table what the runs that played it (at the game's default model_lean) still say")
    q.add_argument("--table", type=int, default=0, help="with --onto: the runs whose model_table was this (1: run with --alt-cards TABLE)")
    a = ap.parse_args(argv)
    learn.KNOB.update(learn.DEFAULTS)
    return {"run": cmd_run, "trace": cmd_trace, "report": cmd_report, "prior": cmd_prior}[a.cmd](a)


if __name__ == "__main__":
    raise SystemExit(main())
