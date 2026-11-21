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
# A card needs about this many picks before its record counts for half. (6 until 2026-11-21. Two runs of
# one character end 12 floors apart on average, so a card's record after 15 picks is 3 floors either way
# by chance alone, and the spread of all the records was no wider than that chance: at 6 a card moved 0.7
# points on nothing. What cards truly differ by came out at about a floor a pick; 150 is what that asks for.)
PRIOR_PICKS = 150.0
# Floors gained per pick (beyond the character's average pick) that are worth one point of card rating.
# (3.0 until 2026-11-21. A point of other players' rating went with 0.4 floors a pick in our own record,
# and that measure is blurred downwards.)
FLOORS_PER_POINT = 1.0
MAX_SHIFT = 2.5  # a rating never moves further than this
# Head start for a card that has never been tried (a third of it is left after 8 picks).
# At 0.5 about a hundred cards offered 12 to 40 times each were never or once taken
# (Juggernaut, Hologram, Accelerant, Expertise, Feel No Pain...), so the record could
# say nothing about them and the hand-made guess stood for ever.
# (0.8 since 2026-11-20: by then most of them had been tried once or twice, and crowd.py gives the untried
# ones a rating from half a million runs. At 1.5 an untried card the crowd puts last still beat a proven one.)
# (0.4 since 2026-11-21: with the record counting for so little a card, a first try teaches next to nothing,
# and 0.8 for being new was as much as 137 bad picks took off Sow.)
CURIOSITY = 0.4


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

DEFAULTS = {"caution": 1.0, "elite_worth": 2.5, "upgrade_worth": 1.3, "pickiness": 0.0, "crowd": 0.6, "punch": 0.9}
CHOICES = {
    # Weight of HP lost in the combat planner. (1.35 and 1.8 were dropped on 2026-11-21. Runs since
    # 2026-11-19 20:06 that reached the act 2 boss got past it 44 times in 74 at 1.0 and 19 in 54 at 1.35,
    # and so for each character and ascension on its own (Necrobinder 19/33 against 3/22, Ascension 5 11/24
    # against 2/11); 1.8: 6 runs, mean floor 28. The sampler judges by floors, where the two were three
    # apart, and still drew 1.35 for more than a quarter of the runs.)
    "caution": [0.75, 1.0],
    # What an elite's rewards count for when planning a route. (Since the boss-odds curve was refitted on
    # 2026-11-20 this decides something: 0.8 never takes an elite, 2.5 takes one from full HP with a rest
    # site after it, 4.0 from 85% HP, 6.0 from 75%. 1.5 did the same as 0.8 and was dropped; 6.0 is new.)
    "elite_worth": [0.8, 2.5, 4.0, 6.0],
    # What an upgrade counts for against resting. (6.0 added on 2026-11-20: the record rose with every
    # step, +1.8 floors a run at 1.3, +2.7 at 2.5, +3.1 at 4.0, and the bot upgrades twice a run by the act 2 boss.)
    # (1.3 dropped on 2026-11-21: 24 runs since the 19th, mean floor 30.0 against 33.8, 33.4 and 31.8.)
    "upgrade_worth": [2.5, 4.0, 6.0],
    "pickiness": [-1.0, 0.0, 1.0],  # added to the rating a card reward needs to be taken
    # (`early_setup`, 1.7 times every lasting effect in rounds 1-3 of boss and elite fights, was dropped on
    # 2026-11-19: 21 runs, no better than the 100 without it.)
    # (`long_game`, Powers and lasting Strength priced by the turns a boss fight has left, was dropped on
    # 2026-11-20: 20 runs, 4.4 floors a run behind the 38 without it.)
    # How much of a card's rating comes from other players' runs (crowd.py) and how much from our own table
    # (cards._TIER). (0.0, ours alone, was dropped on 2026-11-21 and 1.0 added. Runs since 2026-11-20 21:05 that
    # reached the act 2 boss got past it 6 times in 20 at 0.0 and 12 in 26 at 0.6. And over the 189 cards taken
    # 8 times or more, how far the runs that took a card went beyond the character's other picks goes with the
    # crowd's rating (r = +0.28, 0.4 floors a pick for each point; so for every character on its own) and not
    # with ours (r = +0.06): mixed at 0.6 r = +0.26, at 0.8 +0.28. Sow: ours 5.0, theirs 1.9, 137 picks, 1.7
    # floors a pick under par; Big Bang: theirs 8.0, 5.7 over.)
    "crowd": [0.6, 1.0],
    # How hard a deck short of damage leans to attacks at a card reward: rating points for each point its
    # average damage an energy is under strategy.PUNCH_FLOOR (up to 2.2 times this). Added on 2026-11-21:
    # final decks since the 20th averaged 5.7-5.9 damage an energy with `crowd` 0.0, 4.5-4.8 at 0.6 and
    # 3.5-4.3 at 1.0 (the floor is 5.5), and ordinary act 2 fights cost the Regent 13.2%, 15.1% and 19.7%
    # of Max HP (4.0, 4.3 and 5.4 rounds; the Necrobinder 14.8% and 16.7%), while the act 2 boss went the
    # other way (7 in 24 against 12 in 29). Judge by both: `fightlog.py knobs`, and rounds and cost of
    # ordinary act 2 fights by arm.
    "punch": [0.9, 1.8],
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
