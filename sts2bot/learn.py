"""Card ratings learned from the bot's own runs (a card taken scores how much further its run got than the character's runs
usually get from that floor), and the settings the bot tunes as it plays."""

from __future__ import annotations

import json
import logging
import random
from pathlib import Path

from . import games
from .cards import key_of as card_key

log = logging.getLogger("sts2bot.learn")
WIN_BONUS = 15  # a win counts as this many floors beyond the last one
HISTORY = 300  # runs per character the "usual" result is taken from
PRIOR_PICKS = 400.0  # picks before a card's own record counts for half (a run is a noisy measurement)
FLOORS_PER_POINT = 1.0  # floors gained per pick, beyond the character's average pick, worth one point of rating
MAX_SHIFT = 2.5  # a rating never moves further than this
CURIOSITY = 0.4  # head start for a card never tried, so that it gets tried (shrinks with picks)
KIND_PRIOR = 300.0  # picks of a kind (cards.kind) before the kind's record counts for half of where its cards start
MAX_KIND = 1.5  # ... and a kind never moves its cards further than this


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
        self.kinds: dict[str, str] = {}  # card -> its kind (cards.kind), as the card last looked when offered
        self._pool: dict | None = None  # kind -> card -> [picks, floors gained beyond its character's par]
        data = _load(path, "card statistics")
        try:
            self.finals = {k: list(v) for k, v in (data.get("finals") or {}).items()}
            self.cards = {k: {c: list(x) for c, x in v.items()} for k, v in (data.get("cards") or {}).items()}
            self.kinds = {str(k): str(v) for k, v in (data.get("kinds") or {}).items()}
        except (AttributeError, TypeError, ValueError) as e:
            log.warning("could not use %s (%s); starting the card statistics afresh", path, e)

    def usual(self, character: str, floor: int) -> float | None:
        """What a run of this character that got to `floor` usually ends on."""
        reached = [f for f in self.finals.get(character, []) if f >= floor]
        return sum(reached) / len(reached) if len(reached) >= 5 else None

    def _par(self, character: str) -> float:
        """Floors a pick of this character gains on average: what a card's record is measured against."""
        table = self.cards.get(character) or {}
        picks = sum(x[0] for x in table.values())
        return sum(x[1] for x in table.values()) / picks if picks >= 50 else 0.0

    def kind_shift(self, kind: str, but: str = "") -> float:
        """Where a card of this kind starts: what the kind's cards gained a pick, each against its character's par (`but` left out)."""
        if not kind:
            return 0.0
        if self._pool is None:
            self._pool = {}
            for who, table in self.cards.items():
                par = self._par(who)
                for key, (picks, gained) in table.items():
                    if self.kinds.get(key):
                        row = self._pool.setdefault(self.kinds[key], {}).setdefault(key, [0.0, 0.0])
                        row[0] += picks
                        row[1] += gained - picks * par
        rows = [x for key, x in (self._pool.get(kind) or {}).items() if key != but]
        learned = sum(x[1] for x in rows) / (sum(x[0] for x in rows) + KIND_PRIOR) / FLOORS_PER_POINT
        return max(-MAX_KIND, min(MAX_KIND, learned))

    def shift(self, character: str, name: str, kind: str | None = None) -> float:
        """How far the record moves this card's rating (0 for an unknown character); `kind`: cards.kind as offered (None: as last seen)."""
        key = card_key(name)
        if kind is not None and self.kinds.get(key) != kind:
            self.kinds[key] = kind
            self._pool = None
        table = self.cards.get(character)
        if table is None and not self.finals.get(character):
            return 0.0
        picks, gained = (table or {}).get(key, (0.0, 0.0))
        # Against the character's par, not zero (the bot gets better over time); with no record a card stands where its kind does.
        start = self.kind_shift(self.kinds.get(key, ""), key) * FLOORS_PER_POINT
        if self.kinds.get(key) == "free":
            start *= knob("free_lean")  # a setting under test (see CHOICES)
        learned = (gained - picks * self._par(character) + PRIOR_PICKS * start) / (picks + PRIOR_PICKS) / FLOORS_PER_POINT
        return max(-MAX_SHIFT, min(MAX_SHIFT, learned)) + CURIOSITY / (1.0 + picks) ** 0.5

    def record(self, character: str, floor: int, won: bool, picks: list) -> None:
        """Add a finished run: its result, and the cards it took on the way."""
        if not character or not isinstance(floor, int):
            return
        final = floor + (WIN_BONUS if won else 0)
        table = self.cards.setdefault(character, {})
        for at, name in picks or []:
            # Against the runs that left that floor alive, as this one did to be offered the card.
            usual = self.usual(character, int(at or 0) + 1)
            if usual is None:
                continue  # too few runs yet to say what is usual
            entry = table.setdefault(card_key(name), [0.0, 0.0])
            entry[0] += 1.0
            entry[1] += final - usual
        self.finals.setdefault(character, []).append(final)
        del self.finals[character][:-HISTORY]
        self._pool = None
        self.save()

    def save(self) -> None:
        _save(self.path, {"finals": self.finals, "cards": self.cards, "kinds": self.kinds}, "card statistics")


# What the model runs (sim/, `python3 -m sim prior`) say of each card for each character: floors a pick gained over the runs that
# left the same floor alive, shrunk towards nothing. Used times the `model_lean` setting (0: not at all; see CHOICES).
# Since 2026-11-26 corrected twice by what the runs that played it still say of each card (`sim prior --onto`): on fresh seeds
# 2.6 floors further in Slay the Spire 2 at A7 and 3.6 in the first game at A0 (docs/strategy/simulation.md, round 4). Round 5
# found it had learned the model's own bugs (Grand Finale, Bone Shards, Fight Me, Fight Through, Reprogram: two dozen cards the model
# played unlike their text, most at the +3 cap): rebuilt on the corrected model, those cards from 0. With round 5's other changes,
# on fresh seeds: 1.8 floors further in StS2 at A8 (wins 5.7% -> 9.5%), 2.4 in the first game at A0 (17% -> 24.8%).
MODEL_CARDS = Path(__file__).resolve().parent / "run" / "model_cards.json"
_model: dict | None = None


def who_key(character) -> str:
    """A character as the model table keys it: "The Silent", "THE_SILENT" and "SILENT" alike."""
    key = "".join(ch if ch.isalpha() else "_" for ch in str(character or "").upper()).strip("_")
    return key[4:] if key.startswith("THE_") else key


def model_shift(character, name) -> float:
    """The model runs' rating shift for this card in this character's deck, times `model_lean`."""
    lean = knob("model_lean")
    if not lean:
        return 0.0
    global _model
    if _model is None:
        _model = _load(MODEL_CARDS, "model card table")
    table = (_model.get(games.active()) or {}).get(who_key(character)) or {}
    return lean * float(table.get(card_key(name), 0.0))


def history(runs_file: Path | None):
    """Each run in runs.jsonl as a dict, read one line at a time (the file grows by every run ever played)."""
    try:
        with open(runs_file, encoding="utf-8", errors="replace") as f:
            for line in f:
                try:
                    r = json.loads(line)
                except ValueError:
                    continue
                if isinstance(r, dict):
                    yield r
    except (OSError, TypeError):  # (TypeError: no file given)
        return


def seed_from_history(stats: CardStats, runs_file: Path) -> None:
    """Fill in the "usual result" from the run history when the statistics are new."""
    if stats.finals:
        return
    for r in history(runs_file):
        if r.get("character") and isinstance(r.get("floor"), int):
            final = r["floor"] + (WIN_BONUS if r.get("result") == "win" else 0)
            stats.finals.setdefault(str(r["character"]), []).append(final)
    for finals in stats.finals.values():
        del finals[:-HISTORY]


# ----------------------------------------------------------------- knobs
# Judgement calls that are plain numbers: each run plays one setting of each, drawn by Thompson sampling from those whose runs went furthest.
DEFAULTS = {"caution": 1.0, "elite_worth": 2.5, "upgrade_worth": 1.3, "pickiness": 0.0, "crowd": 0.6, "punch": 0.9, "weak_want": 0.0, "attack_room": 99.0, "block_want": 0.0, "cards_first": 0.0, "debuff_worth": 1.0, "free_lean": 1.0,
            "model_lean": 0.0, "belt_spare": 1.0, "boss_caution": 1.0, "early_rows": 8.0}
CHOICES = {
    "caution": [0.75, 1.0, 1.35],  # weight of HP lost in the combat planner (model runs: 1.35 best in Slay the Spire 2 at A5, no better at A7)
    "elite_worth": [2.5, 6.0],  # what an elite's rewards count for when planning a route
    "upgrade_worth": [2.5, 4.0],  # what an upgrade counts for against resting in acts 2 and 3 (act 1: strategy.UPGRADE_EARLY)
    "pickiness": [-1.0, 0.0],  # added to the rating a card reward needs to be taken
    "crowd": [0.0, 0.3, 0.6],  # share of a card's rating from other players' runs (crowd.py) over our table (cards._TIER); model runs: 0.6 best in StS1, worst in StS2
    "punch": [0.9, 2.7],  # how hard a deck short of damage leans to attacks: points per damage an energy under decks.PUNCH_FLOOR (model runs at A7: 0.9 a floor ahead)
    "weak_want": [0.0, 1.0],  # rating points for a card that applies Weak while the deck holds fewer than two (decks.deck_fit)
    "free_lean": [0.0, 1.0],  # how much of its kind's start a free attack gets (learn.CardStats.shift)
    "attack_room": [3.0],  # damage cards a deck holds before that lean lets go (gone three later; 99: never): attack-heavy decks lost the act 2 boss
    "block_want": [0.0, 1.5],  # rating points for a card with 5 Block or more while the deck holds few beyond its Defends (decks.deck_fit; model runs: 1.5 ahead in both games)
    "cards_first": [0.0, 1.0],  # 1.0: an early card removal has to beat the cards on a Merchant's shelf (strategy.shop)
    "debuff_worth": [1.0, 2.0, 3.0],  # a lasting turn of Vulnerable or Weak in the combat planner, as a multiple of its old worth (model runs: 1.0 best in the first game)
    "model_lean": [0.0, 1.0, 2.0],  # how much of what the model runs say of each card goes into its rating (model_shift; in the model 1.0 went 3-4 floors further, 2.0 a floor more in StS2)
}
# What the model runs found best in each game (docs/strategy/simulation.md), over DEFAULTS: played with --no-tuning and by the
# model runs' own default (DEFAULTS stay what the tests are written against). 2026-11-25, Slay the Spire 2 at A7 and the first game
# at A0: model_lean 1 went 3-4 floors further in both on seeds its table had not seen; crowd 0.6 was the worst of three in StS2 four
# batches running, the best in StS1; attack_room 3 about a floor ahead in StS2; cards_first 1 1.4-1.9 floors ahead in StS1, elite_worth 6 a little.
# 2026-11-26: model_lean 2 another 0.9 +- 0.4 floors in StS2 on fresh seeds (3 fell back 0.6); in StS1 2 made no difference (+0.2 +- 0.55).
# Round 5 (StS2 at A8): belt_spare 0, no potion for a hallway fight just because the belt is full, 1.2 +- 0.6 floors further in StS2
# and 1.5 +- 0.65 short in StS1, so StS2's alone (not in CHOICES: the two games want opposite values).
GAME_DEFAULTS: dict[str, dict] = {
    "sts2": {"crowd": 0.3, "attack_room": 3.0, "model_lean": 2.0, "belt_spare": 0.0},
    "sts1": {"elite_worth": 6.0, "cards_first": 1.0, "model_lean": 1.0},
}


def defaults(game: str | None = None) -> dict:
    """The settings to play without tuning: DEFAULTS, with this game's (the one being played if None) from GAME_DEFAULTS."""
    return {**DEFAULTS, **GAME_DEFAULTS.get(game or games.active(), {})}


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
            # An untried setting starts from what the tried ones average, not from zero (every run beats "the usual" by a few floors).
            par = sum(g for _r, g in seen) / max(1.0, sum(r for r, _g in seen))
            for value in choices:
                runs, gained = self.table.get(name, {}).get(str(value), (0.0, 0.0))
                # What this setting's true average might be, given what we have seen of it.
                guess = self.rng.gauss((gained + par) / (runs + 1.0), SPREAD / (runs + 1.0) ** 0.5)
                if best_guess is None or guess > best_guess:
                    best, best_guess = value, guess
            # Half the time play a setting with few runs, so that a few bad ones cannot bury it.
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
