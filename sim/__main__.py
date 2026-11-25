"""`python3 -m sim`: play model runs, report on them, or print a digest of every decision.

    python3 -m sim run --game sts1 --character IRONCLAD --runs 1000 --jobs 4 --out sim-runs.jsonl
    python3 -m sim run --game sts2 --character all --runs 1000 --vary      # each run draws its own value of every setting
    python3 -m sim report sim-runs.jsonl [--md docs/strategy/simulation.md]
    python3 -m sim trace --game sts1 --runs 2                             # one hash per character: changes iff a decision does
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
}


def content(game: str):
    games.select(game)
    return importlib.import_module(f"sim.content.{game}")


def _job(args: tuple) -> dict:
    game, character, seed, knobs, asc = args
    from .run import Run
    logging.disable(logging.CRITICAL)
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
    r.add_argument("--set", action="append", metavar="NAME=VALUE", help="play every run with this setting (repeatable)")
    r.add_argument("--settings", metavar="FILE", help="JSON {character: {setting: value}} to play each character with")
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
    a = ap.parse_args(argv)
    learn.KNOB.update(learn.DEFAULTS)
    return {"run": cmd_run, "trace": cmd_trace, "report": cmd_report}[a.cmd](a)


if __name__ == "__main__":
    raise SystemExit(main())
