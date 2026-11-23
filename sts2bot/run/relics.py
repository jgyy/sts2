"""What the Ancients' relics have done in our own runs.

events._ANCIENT rates each relic an Ancient offers with one number, set by hand. This keeps score, as
learn.CardStats does for cards: every run that held one of them, how much further did it get than runs of
the same character and ascension usually get from the floor the relic is taken on? Relics that keep
beating the usual drift up, the others down (`shift`), and the next Ancient is answered with the new numbers.

Why (2026-11-21, 1,064 runs with their relics on record): the table and the record disagreed more than
chance allows. Runs holding Seal of Gold (rated 8.0, taken only when nothing else was on offer) ended 5.7
floors over the usual (15 runs) and Prismatic Gem 3.6 over (16), the two every-turn energy relics of act 2
that come without a second Ancient to confuse the count; Pael's Flesh (12.5) 2.2 under (51), Alchemical
Coffer (9.5) 4.0 under (38), and of Neow's the Stone Humidifier 5.1 over (56) where the Booming Conch was
4.1 under (56). Taking chance out, relics of one Ancient truly differ by about 2 floors a run (the means
spread 3 floors, of which 2 to 2.5 is chance): a run is 8 to 11 floors either way, so a record counts for
half after about 25 runs.

Nothing is kept on disk: the record is rebuilt from runs.jsonl when the bot starts, and each finished run
is added to it. When a relic was taken is read off the relic itself (each Ancient has its own, and each act
opens with one Ancient).
"""

from __future__ import annotations

import logging
import sys
from pathlib import Path

from ..learn import history

log = logging.getLogger("sts2bot.relics")

WIN_BONUS = 15  # as learn.WIN_BONUS: a win counts as this many floors beyond the last one
PRIOR_RUNS = 25.0  # runs a relic needs before its record counts for half
POINTS_PER_FLOOR = 1.0  # a floor further than usual, in the points of events._ANCIENT (its ratings spread about as wide as the floors do)
MAX_SHIFT = 3.0  # a rating never moves further than this
FEW = 5  # runs of a character on an ascension band before "the usual" means anything
BAND = 2  # ascension levels taken together for "the usual"

# The relics of the Ancients that open act 2 (Orobas, Pael, Tezcatara) and act 3 (Nonupeipe, Tanx, Vakuu), and
# Darv's, who was met 13 times in act 2 and 4 in act 3. Any other relic an Ancient offers is Neow's, on floor 1.
ACT2 = {
    "ALCHEMICAL_COFFER", "ARCHAIC_TOOTH", "DRIFTWOOD", "ELECTRIC_SHRYMP", "GEAR_GLASS", "GLASS_EYE", "LICH_GLASS", "NOBLE_GLASS",
    "VENOM_GLASS", "PRISMATIC_GEM", "RADIANT_PEARL", "SAND_CASTLE", "PAELS_BLOOD", "PAELS_CLAW", "PAELS_EYE", "PAELS_FLESH",
    "PAELS_GROWTH", "PAELS_HORN", "PAELS_LEGION", "PAELS_TEARS", "PAELS_TOOTH", "PAELS_WING", "BIIIG_HUG", "GOLDEN_COMPASS",
    "NUTRITIOUS_SOUP", "PUMPKIN_CANDLE", "SEAL_OF_GOLD", "STORYBOOK", "TOASTY_MITTENS", "TOY_BOX", "VERY_HOT_COCOA", "YUMMY_COOKIE",
}
ACT3 = {
    "BEAUTIFUL_BRACELET", "BLESSED_ANTLER", "BRILLIANT_SCARF", "DELICATE_FROND", "DIAMOND_DIADEM", "FUR_COAT", "GLITTER",
    "JEWELRY_BOX", "LOOMING_FRUIT", "SIGNET_RING", "CLAWS", "CROSSBOW", "IRON_CLUB", "MEAT_CLEAVER", "SAI", "SPIKED_GAUNTLETS",
    "TANXS_WHISTLE", "THROWING_AXE", "TRI_BOOMERANG", "WAR_HAMMER", "BLOOD_SOAKED_ROSE", "CHOICES_PARADOX", "DISTINGUISHED_CAPE",
    "FIDDLE", "JEWELED_MASK", "LORDS_PARASOL", "MUSIC_BOX", "PRESERVED_FOG", "WHISPERING_EARRING",
}
DARV = {
    "ASTROLABE", "BLACK_STAR", "CALLING_BELL", "DUSTY_TOME", "ECTOPLASM", "EMPTY_CAGE", "PHILOSOPHERS_STONE", "RUNIC_PYRAMID",
    "SNECKO_EYE", "SOZU", "VELVET_CHOKER",
}
FLOOR2, FLOOR3 = 18, 34  # the floors acts 2 and 3 open on

_known: set = set()  # every relic an Ancient offers that we rate (events._ANCIENT)
_runs: list = []  # (character, ascension band, floors reached with the win bonus, relics held)
_table: dict = {}  # relic -> [runs that held it, floors beyond the usual in total]


def picked_at(relic: str, held: set) -> int:
    """The floor a run holding `held` took this Ancient's relic on."""
    if relic in ACT3:
        return FLOOR3
    if relic in ACT2:
        return FLOOR2
    if relic in DARV:
        return FLOOR3 if held & ACT2 else FLOOR2  # (one Ancient opens each act: with another act 2 relic held, Darv was the third)
    return 1


def _rebuild() -> None:
    by_group: dict = {}
    for who, band, final, _held in _runs:
        by_group.setdefault((who, band), []).append(final)
    usual: dict = {}  # (character, band, floor) -> what such a run that got to that floor usually ends on
    for group, finals in by_group.items():
        for floor in (1, FLOOR2, FLOOR3):
            reached = [f for f in finals if f >= floor]
            if len(reached) >= FEW:
                usual[(*group, floor)] = sum(reached) / len(reached)
    _table.clear()
    for who, band, final, held in _runs:
        for relic in held & _known:
            floor = picked_at(relic, held)
            par = usual.get((who, band, floor))
            if par is None or final < floor:
                continue
            entry = _table.setdefault(relic, [0.0, 0.0])
            entry[0] += 1.0
            entry[1] += final - par


def note(run: dict, rebuild: bool = True) -> None:
    """Add a finished run (a line of runs.jsonl) to the record."""
    relics, floor = run.get("relics"), run.get("floor")
    if not run.get("character") or relics is None or not isinstance(floor, int):
        return  # (the first hundred runs were written down without their relics)
    final = floor + (WIN_BONUS if run.get("result") == "win" else 0)
    # (Only the relics the record is about are kept, one shared copy of each name: a run holds a dozen or more.)
    held = frozenset(sys.intern(str(r)) for r in relics if str(r) in _known or str(r) in ACT2)
    _runs.append((sys.intern(str(run["character"])), int(run.get("ascension") or 0) // BAND, final, held))
    if rebuild:
        _rebuild()


def load(runs_file: Path | None, known) -> None:
    """Start the record afresh from the run history (`known`: the relics that have a rating to shift)."""
    _known.clear()
    _known.update(known)
    _runs.clear()
    _table.clear()
    for run in history(runs_file):
        try:
            note(run, rebuild=False)
        except (ValueError, TypeError, AttributeError):
            continue
    _rebuild()
    if _table:
        moved = sorted(((shift(k), k) for k in _table), reverse=True)
        log.info("Ancient relics by our own record (%d runs): up %s; down %s", len(_runs),
                 ", ".join(f"{k} {s:+.1f}" for s, k in moved[:5]), ", ".join(f"{k} {s:+.1f}" for s, k in moved[-5:]))


def shift(relic: str) -> float:
    """How far the record moves this relic's rating (0 for one never held)."""
    runs, gained = _table.get(relic, (0.0, 0.0))
    return max(-MAX_SHIFT, min(MAX_SHIFT, POINTS_PER_FLOOR * gained / (runs + PRIOR_RUNS)))


def record() -> dict:
    """relic -> (runs that held it, floors beyond the usual a run), for scripts."""
    return {k: (int(n), g / n) for k, (n, g) in _table.items() if n}
