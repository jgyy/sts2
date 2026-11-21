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

DEFAULTS = {"caution": 1.0, "elite_worth": 2.5, "upgrade_worth": 1.3, "pickiness": 0.0, "crowd": 0.6, "punch": 0.9, "weak_want": 0.0, "attack_room": 99.0, "block_want": 0.0, "cards_first": 0.0, "debuff_worth": 1.0}
CHOICES = {
    # Weight of HP lost in the combat planner. (1.35 and 1.8 were dropped on 2026-11-21. Runs since
    # 2026-11-19 20:06 that reached the act 2 boss got past it 44 times in 74 at 1.0 and 19 in 54 at 1.35,
    # and so for each character and ascension on its own (Necrobinder 19/33 against 3/22, Ascension 5 11/24
    # against 2/11); 1.8: 6 runs, mean floor 28. The sampler judges by floors, where the two were three
    # apart, and still drew 1.35 for more than a quarter of the runs.)
    # (0.75 on 2026-11-21 21:30: 70 runs in all, 0.8 floors a run under the usual where the 398 at 1.0 are 0.9
    # over. It deals 4 a round more at the act 1 and act 2 bosses, as it must, and has won 3 act 2 bosses in 9
    # against 33 in 75: left in, the sampler already draws it one run in six.)
    "caution": [0.75, 1.0],
    # What an elite's rewards count for when planning a route. (Since the boss-odds curve was refitted on
    # 2026-11-20 this decides something: 0.8 never takes an elite, 2.5 takes one from full HP with a rest
    # site after it, 4.0 from 85% HP, 6.0 from 75%. 1.5 did the same as 0.8 and was dropped; 6.0 is new.)
    # (0.8 dropped on 2026-11-21: over the 234 runs on Ascension 5 and up, 63 at 0.8 ended 0.9 floors under
    # their character's usual and got past the act 2 boss 12 times in 38, the 85 at 6.0 1.6 over and 30 in 60.)
    # (4.0 dropped on 2026-11-21 21:30. On Ascension 6 and up its 50 runs ended 1.8 floors under their
    # character's usual and got past the act 2 boss 12 times in 32; 6.0: 122 runs, 0.9 over, 45 in 85, and 7 of
    # the 8 wins; 2.5: 37 runs, 1.5 over, 11 in 28, no win.)
    "elite_worth": [2.5, 6.0],
    # What an upgrade counts for against resting. (6.0 added on 2026-11-20: the record rose with every
    # step, +1.8 floors a run at 1.3, +2.7 at 2.5, +3.1 at 4.0, and the bot upgrades twice a run by the act 2 boss.)
    # (1.3 dropped on 2026-11-21: 24 runs since the 19th, mean floor 30.0 against 33.8, 33.4 and 31.8.)
    # (6.0 dropped the same day: 42 runs in all, 1.5 floors a run under the usual, and on Ascension 5 and up
    # 16 runs 5 floors under, 9 of them past act 1; at 2.5 and 4.0 nothing separates 85 runs from 123.)
    # (From 2026-11-21 21:30 this is the worth in acts 2 and 3 only; act 1 is strategy.UPGRADE_EARLY. On
    # Ascension 6 and up 2.5 lost 20 runs in 86 in act 1 where 4.0 lost 16 in 143, and then got past the act 2
    # boss 29 times in 54 against 42 in 104, dealing 3.6 a round more there than its character's usual against
    # 1.8 less (31 and 53 fights): upgrades while the deck is small, HP when the fights cost a sixth of it each.)
    "upgrade_worth": [2.5, 4.0],
    # Added to the rating a card reward needs to be taken. (1.0 dropped on 2026-11-21: 24 runs, 3.6 floors
    # a run under the usual; -1.0 and 0.0 are 1.7 and 1.4 over, 211 and 355 runs.)
    "pickiness": [-1.0, 0.0],
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
    # (1.0 dropped on 2026-11-21 21:30: 73 runs on Ascension 6 and up, 1.0 floors a run under the usual
    # against 0.5 over for 161 at 0.6, 1 win against 7, and at the act 2 boss 3.5 a round under the
    # character's usual damage against 1.2 over (19 and 65 fights, 2.6 standard errors).)
    "crowd": [0.6],
    # How hard a deck short of damage leans to attacks at a card reward: rating points for each point its
    # average damage an energy is under decks.PUNCH_FLOOR (up to 2.2 times this). Added on 2026-11-21:
    # final decks since the 20th averaged 5.7-5.9 damage an energy with `crowd` 0.0, 4.5-4.8 at 0.6 and
    # 3.5-4.3 at 1.0 (the floor is 5.5), and ordinary act 2 fights cost the Regent 13.2%, 15.1% and 19.7%
    # of Max HP (4.0, 4.3 and 5.4 rounds; the Necrobinder 14.8% and 16.7%), while the act 2 boss went the
    # other way (7 in 24 against 12 in 29). Judge by both: `fightlog.py knobs`, and rounds and cost of
    # ordinary act 2 fights by arm.
    # (0.9 dropped the same day, 2.7 added: of 7 runs at 0.9 four died in act 1 (two decks with two attacks
    # at the act 1 boss, mean floor 21.3), of 20 at 1.8 one (34.7). Final decks at 1.8 still average 4.5 to
    # 5.0 against the floor of 5.5, and an ordinary act 2 fight still costs 17.6% of Max HP at 26 enemy HP
    # a round (the aim is 30). 2.7 gives the same at most, 4 points, and gets there from a smaller shortfall,
    # so it only differs in acts 2 and 3. DEFAULTS keeps 0.9: the tests of decks.deck_fit are written around it.)
    # (1.8 dropped on 2026-11-21 21:30: 56 runs on Ascension 6 and up, 1.9 floors a run under the usual and
    # past the act 2 boss 11 times in 37; 2.7: 173 runs, 1.1 over, 61 in 124.)
    "punch": [2.7],
    # Rating points for a card that applies Weak while the deck holds fewer than two that do (decks.deck_fit).
    # Added on 2026-11-21: of 87 final decks that met the last boss on Ascension 3+, those with none won 0 times
    # in 13, with one 3 in 31, with two or more 14 in 43 (and of the 26 last bosses in that day's logs the five
    # won brought 1.8 such cards, the lost 1.1); the act 2 bosses show nothing either way. Weak is a quarter off
    # attacks of 25 to 45, and other players' ratings do not see what the deck already holds.
    "weak_want": [0.0, 1.0],
    # How many cards taken for their damage a deck holds before the lean above lets go (decks.deck_fit:
    # in full up to this many, gone three cards later; 99 is "never", what the bot did until 2026-11-21).
    # Why: of 81 act 2 bosses met with the deck on record, decks holding ten or more attacks other than
    # Strikes won 13 in 37, the others 27 in 39; beside the HP they arrived on and their character, each
    # further such attack counts against them (3.2 standard errors), and deck size, Block, upgrades and
    # Powers show nothing. The runs that lost there had taken 4.5 attacks and 0.7 Powers in act 1, the
    # ones that won 3.6 and 1.2, and the same number of each in act 2. And the attacks buy nothing on the
    # way: an ordinary act 2 fight cost the same share of Max HP whatever the count (-0.15% an attack,
    # standard error 0.29; 76 runs), an act 1 fight if anything more. The lean itself stays: decks with
    # two attacks at the act 1 boss died there (punch 0.9, seven runs).
    # (99 dropped on 2026-11-21 18:05, on the decks rather than on its 9 runs against 12: of 143 act 1 bosses
    # the decks holding three or four such cards won 52 in 58, five or six 53 in 67, and with a Block card
    # beside at most three of them 19 in 19; of 105 act 2 bosses five or six won 21 in 33, seven or eight 19
    # in 37, nine or more 15 in 34; and a card more of them made the next act's fights no cheaper (+0.7% of
    # Max HP a fight). With 99 a card taken for damage also outranked every Block card through act 1,
    # whatever `block_want` said.)
    "attack_room": [3.0],
    # Rating points for a card that gives 5 Block or more while the deck holds few of them beyond its Defends
    # (decks.deck_fit: in full for the first two, three in a deck of 24 or more, half for the one after).
    # Added on 2026-11-21. The lean above had made 51% of act 1 picks cards taken for their damage and 11%
    # Block cards (taken 115 times of 484 offered; passed over, one was rated 2.1 under the best on offer):
    # decks met the act 1 boss with 4.4 attacks and 1.0 Block cards of their own. With none they beat it 31 times in 45, with one or more 81 in
    # 90, on the same HP at the door (135 fights, Ascension 6-7). And each one held there made every ordinary
    # act 2 fight 2.6% of Max HP cheaper (131 runs, character taken out, 4.3 standard errors: 15.2% of Max
    # HP a fight with none, 12.4% with one, 11.5% with two, 7.2% with three; the same way inside four of the
    # five characters), where an attack more, a Power more or another Skill changed nothing (+0.7%, +0.3%,
    # -0.7%). Act 3 the same way, weaker: 20.8%, 19.0%, 17.9%, 15.7%. At the act 2 boss it stops counting
    # after the first (3 in 8 with none, 40 in 69 with one to three, 9 in 21 with four or more).
    # Judge by `fightlog.py rooms block_want` (what an ordinary act 2 fight costs under each value) and by
    # the act 1 boss, long before `knobs` can say anything about floors.
    # (1.5 dropped on 2026-11-21 21:30. It won 21 act 1 bosses in 28 where the runs without it won 62 in 64,
    # dealing 1.9 a round less there than the character's usual against 1.5 more; and with every fight on
    # record (fights.jsonl, 350 ordinary act 2 fights, the encounter taken out) a Block card more makes a
    # fight no cheaper (-0.3% of Max HP, 0.5 standard errors) and counts against the deck at the act 2 boss
    # (one or two: 15 won in 23; three or more: 17 in 46). The effect it was added for was one window's.)
    "block_want": [0.0],
    # Whether a card removal has to beat the cards on a Merchant's shelf early in a run (strategy.shop: at 1.0
    # it is worth 5.5 in act 1 and 7.5 in act 2 where a relic is about 7 and a card its rating; at 0.0 it is
    # 9.0 in every act, before everything, as it was until 2026-11-21). Added that day, when the shelf first
    # went into the log: on floor 3, with 113 gold, the removal (100) was bought past a card rated 7.7 for
    # 24 gold and one rated 5.7 for 48. A deck's cards rate 0.5 on average in act 1, 2.1 in act 2 and 2.7 in
    # act 3 (a Strike -2.5), so by what it does to that average a removal is a card rated about 4 in act 1,
    # 7 in act 2 and 8.5 in act 3; and the Strikes a deck holds at a boss do not show in what its next
    # fights cost (-0.3% of Max HP a fight for one more, 131 runs), where a Block card shows at once.
    # From Ascension 6 the price also climbs 50 with each removal.
    "cards_first": [0.0, 1.0],
    # What a turn of Vulnerable or Weak that outlasts this one counts for in the combat planner, as a multiple
    # of what it was (7 points a turn of Vulnerable and 5 of Weak on a boss, 5 and 3.5 on an elite, 3 and 2
    # otherwise). Added on 2026-11-21 from the fight logs: a round that began with a lone boss Vulnerable
    # dealt 14.6 more than the other rounds of the same fight in act 1 (55 rounds, 26 fights), 10.9 in act 2
    # (44, 17) and 35 in act 3 (11, 5), which at 1.5 points a damage is 16 points and up; and bosses attack
    # for 10.7, 19.3 and 22.7 a round by act, so a turn of Weak is 2.7 to 5.7 of our HP at 2 to 3 points each.
    # Both say the old numbers are about half. What it changes is which card goes first when energy is short
    # (a Weak or Vulnerable card before a Strike or a Defend), so judge it by boss records and by what a boss
    # fight costs (`fightlog.py rooms debuff_worth` for the ordinary fights), then `knobs`.
    # (1.0 dropped on 2026-11-21 21:30, 3.0 added. At act 2 bosses 2.0 dealt 4.6 a round more than its
    # character usually does against that boss and 1.0 2.3 less (22 and 12 fights, 2 standard errors; the
    # same way in each of the five pairs of character and boss met under both, and in acts 1 and 3), won 9
    # in 22 against 3 in 12, its ordinary fights cost 5.6% and 13.7% of Max HP in acts 1 and 2 against 7.0%
    # and 16.3%, and its 31 runs ended 0.9 floors over the usual against 2.8 under for 20. The measurement
    # above said 2.3 times the old numbers; 3.0 is there to find where it stops paying
    # (`fightlog.py bosspace`). DEFAULTS keeps 1.0: the tests are written around it.)
    "debuff_worth": [2.0, 3.0],
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
