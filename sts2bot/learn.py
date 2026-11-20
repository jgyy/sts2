"""Card ratings learned from the bot's own runs.

The ratings in cards.py are one person's guess. This keeps score of what
actually happened: every time a card was taken from a card reward, how much
further did that run get than runs of the same character usually get from
that floor? Cards that keep beating the average drift up, cards that keep
falling short drift down, and the next card reward uses the new numbers.

It is slow (a run is a noisy measurement) and that is fine: the bot plays
hundreds of runs a day.
"""

from __future__ import annotations

import json
import logging
import random
from pathlib import Path

from .cards import key_of as card_key

log = logging.getLogger("sts2bot.learn")

WIN_BONUS = 15  # a win counts as this many floors beyond the last one
HISTORY = 300  # runs per character the "usual" result is taken from
PRIOR_PICKS = 6.0  # a card needs about this many picks before its record counts for half
FLOORS_PER_POINT = 3.0  # floors gained per pick (beyond the character's average pick) that are worth one point of card rating
MAX_SHIFT = 2.5  # a rating never moves further than this
# Head start for a card that has never been tried (a third of it is left after 8 picks).
# At 0.5 about a hundred cards offered 12 to 40 times each were never or once taken
# (Juggernaut, Hologram, Accelerant, Expertise, Feel No Pain...), so the record could
# say nothing about them and the hand-made guess stood for ever.
CURIOSITY = 1.5


def _load(path: Path | None, what: str) -> dict:
    """The JSON object kept in `path` ({} when there is none yet, or it cannot be read)."""
    if path is None:
        return {}
    try:
        data = json.loads(path.read_text())
        if isinstance(data, dict):
            return data
        raise ValueError("not a JSON object")
    except FileNotFoundError:
        return {}
    except (OSError, ValueError) as e:
        log.warning("could not read %s (%s); starting the %s afresh", path, e, what)
        return {}


def _save(path: Path | None, data: dict, what: str) -> None:
    if path is None:
        return
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        tmp = path.with_suffix(".tmp")
        tmp.write_text(json.dumps(data, indent=1, sort_keys=True))
        tmp.replace(path)
    except OSError as e:
        log.warning("could not save the %s: %s", what, e)


class CardStats:
    def __init__(self, path: Path | None = None):
        self.path = path
        self.finals: dict[str, list[int]] = {}  # character -> results of its latest runs
        self.cards: dict[str, dict[str, list[float]]] = {}  # character -> card -> [picks, floors gained in total]
        data = _load(path, "card statistics")
        try:
            self.finals = {k: list(v) for k, v in (data.get("finals") or {}).items()}
            self.cards = {k: {c: list(x) for c, x in v.items()} for k, v in (data.get("cards") or {}).items()}
        except (AttributeError, TypeError, ValueError) as e:
            log.warning("could not use %s (%s); starting the card statistics afresh", path, e)

    def usual(self, character: str, floor: int) -> float | None:
        """What a run of this character that got to `floor` usually ends on."""
        reached = [f for f in self.finals.get(character, []) if f >= floor]
        return sum(reached) / len(reached) if len(reached) >= 5 else None

    def shift(self, character: str, name: str) -> float:
        """How far the record moves this card's rating (0 for a character we know nothing about)."""
        table = self.cards.get(character)
        if table is None and not self.finals.get(character):
            return 0.0
        picks, gained = (table or {}).get(card_key(name), (0.0, 0.0))
        # Measured against the character's other picks, not against zero: the bot
        # has got better over time, so every card "beat the usual" (by 4 to 6
        # floors a pick) and a card far below the rest still drifted up.
        all_picks = sum(x[0] for x in (table or {}).values())
        par = sum(x[1] for x in (table or {}).values()) / all_picks if all_picks >= 50 else 0.0
        learned = (gained - picks * par) / (picks + PRIOR_PICKS) / FLOORS_PER_POINT
        return max(-MAX_SHIFT, min(MAX_SHIFT, learned)) + CURIOSITY / (1.0 + picks) ** 0.5

    def record(self, character: str, floor: int, won: bool, picks: list) -> None:
        """Add a finished run: its result, and the cards it took on the way."""
        if not character or not isinstance(floor, int):
            return
        final = floor + (WIN_BONUS if won else 0)
        table = self.cards.setdefault(character, {})
        for at, name in picks or []:
            usual = self.usual(character, int(at or 0))
            if usual is None:
                continue  # too few runs yet to say what is usual
            entry = table.setdefault(card_key(name), [0.0, 0.0])
            entry[0] += 1.0
            entry[1] += final - usual
        self.finals.setdefault(character, []).append(final)
        del self.finals[character][:-HISTORY]
        self.save()

    def save(self) -> None:
        _save(self.path, {"finals": self.finals, "cards": self.cards}, "card statistics")


def seed_from_history(stats: CardStats, runs_file: Path) -> None:
    """Fill in the "usual result" from the run history when the statistics are new."""
    if stats.finals:
        return
    try:
        lines = runs_file.read_text().splitlines()
    except OSError:
        return
    for line in lines:
        try:
            r = json.loads(line)
        except ValueError:
            continue
        if r.get("character") and isinstance(r.get("floor"), int):
            final = r["floor"] + (WIN_BONUS if r.get("result") == "win" else 0)
            stats.finals.setdefault(str(r["character"]), []).append(final)
    for finals in stats.finals.values():
        del finals[:-HISTORY]


# ----------------------------------------------------------------- knobs
#
# Some of the bot's judgement calls are plain numbers: how much a point of
# our HP is worth next to a point of damage, what an elite's relic is worth
# on the map, how picky to be with card rewards. Nobody knows the right
# values, so each run plays with one setting of each, drawn at random but
# more and more often from the settings whose runs have gone furthest
# (Thompson sampling). Good settings end up being used nearly always.

DEFAULTS = {"caution": 1.0, "elite_worth": 2.5, "upgrade_worth": 1.3, "pickiness": 0.0, "long_game": 1.0}
CHOICES = {
    "caution": [0.75, 1.0, 1.35, 1.8],  # weight of HP lost in the combat planner
    "elite_worth": [0.8, 1.5, 2.5, 4.0],  # what an elite's rewards count for when planning a route
    "upgrade_worth": [1.3, 2.5, 4.0],  # what an upgrade counts for against resting
    "pickiness": [-1.0, 0.0, 1.0],  # added to the rating a card reward needs to be taken
    # (`early_setup`, 1.7 times every lasting effect in rounds 1-3 of boss and elite fights, was dropped on
    # 2026-11-19: 21 runs, no better than the 100 without it.)
    "long_game": [0.0, 1.0],  # 1: Powers, Strength, Dexterity and Forge count for the turns a boss or elite fight has left
}
MIN_RUNS = 20  # runs a setting gets before the record alone decides how often it is drawn
SPREAD = 12.0  # how far apart two runs of the same character usually end, in floors

KNOB = dict(DEFAULTS)  # the settings in force right now


def knob(name: str) -> float:
    return KNOB.get(name, DEFAULTS[name])


class Knobs:
    def __init__(self, path: Path | None = None, rng=None):
        self.path = path
        self.rng = rng or random.Random()
        # knob -> setting -> [runs, floors gained in total]
        self.table: dict[str, dict[str, list[float]]] = {k: {c: list(x) for c, x in v.items()} for k, v in _load(path, "tuning").items()}

    def draw(self) -> dict:
        """Choose this run's settings and put them in force."""
        for name, choices in CHOICES.items():
            best, best_guess = DEFAULTS[name], None
            seen = [self.table.get(name, {}).get(str(v), (0.0, 0.0)) for v in choices]
            # An untried setting starts from what the tried ones average, not from
            # zero: runs beat "the usual" by about 5 floors whatever the settings
            # (the bot got better), so a new setting looked 5 floors worse than the
            # old ones and was drawn 4 times in 32.
            par = sum(g for _r, g in seen) / max(1.0, sum(r for r, _g in seen))
            for value in choices:
                runs, gained = self.table.get(name, {}).get(str(value), (0.0, 0.0))
                # What this setting's true average might be, given what we have seen of it.
                guess = self.rng.gauss((gained + par) / (runs + 1.0), SPREAD / (runs + 1.0) ** 0.5)
                if best_guess is None or guess > best_guess:
                    best, best_guess = value, guess
            # A setting needs a fair number of runs before four bad ones can bury it
            # (a run is a noisy measurement): half the time, play one that has few.
            few = [v for v, (r, _g) in zip(choices, seen) if r < MIN_RUNS]
            if few and self.rng.random() < 0.5:
                best = self.rng.choice(few)
            KNOB[name] = best
        return dict(KNOB)

    def record(self, gained: float | None) -> None:
        """A run with the current settings ended `gained` floors beyond what is usual for its character."""
        if gained is None:
            return
        for name in CHOICES:
            entry = self.table.setdefault(name, {}).setdefault(str(KNOB[name]), [0.0, 0.0])
            entry[0] += 1.0
            entry[1] += gained
        _save(self.path, self.table, "tuning")
