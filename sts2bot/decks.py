"""What a card is worth in the deck that would take it, and what a card reward has to be worth to be taken.

The ratings in cards.py judge a card alone; this is what they cannot see (strategy.card_reward and
strategy.shop add it on). Moved out of strategy.py on 2026-11-21, when that file reached its 1000 lines.
"""

from __future__ import annotations

import re

from . import crowd
from .cards import _card_key, is_basic, parse as _parse
from .learn import knob

SKIP_THRESHOLD = 3.0  # card rewards scoring below this are skipped
BIG_DECK = 24


# The Regent's starting relic gives 3 Stars a fight, once. A boss fight goes through the deck two or
# three times, so they pay for half as much each time round: counted in full, a deck that needed
# 8 Stars a cycle and made 2 passed for supplied (Falling Star, Gamma Blast and Astral Pulse sat
# unplayable through most of a lost Vantom fight, next to one Venerate). Star-locked cards were
# left in the hand on 34% of the turns of lost boss fights, 23% of won ones.
# (1.5 until 2026-11-21, with the supply counted a fifth over: decks still came out short. By what
# the cards themselves need and give, a Regent deck 3 Stars or more short ended 53% of the turns of
# its last boss fight with a Star card locked in the hand, one 1-2 short 40%, one in surplus 22%
# (39 fights), and all nine Regent deaths at an act 1 boss were decks 2 or more short.)
STARS_AT_START = 1.0


def _stars(card: dict) -> tuple[int, int]:
    """(Stars the card needs, Stars it gives) each time it is played."""
    info = _parse(card)
    need = 2 if info.star_cost < 0 else info.star_cost
    text = str(card.get("description") or "")
    later = sum(len(re.findall(r"star_icon", m)) for m in re.findall(r"next turn, gain ((?:\[star_icon\.png\]\s*)+)", text, re.I))
    # Genesis ("At the start of your turn, gain 2 Stars"): about three turns to a cycle of the deck.
    later += 3 * sum(len(re.findall(r"star_icon", m)) for m in re.findall(r"start of (?:your|each) turn, gain ((?:\[star_icon\.png\]\s*)+)", text, re.I))
    give = info.star_gain
    if give and re.search(r"if this kills[^.]*gain", text, re.I):
        give = 1  # Knockout Blow's 5 Stars come with a kill: counted in full they paid for a Seven Stars on paper
    return need, give + later


# What the cards of a deck should do for an energy, on average, before damage stops being what it is
# short of (act -> damage). The starter decks do about 3; final decks 5.0, 6.5 and 8.2 by third.
# (Act 1 was 5.0 with a nudge of at most 1.5 for the first two hours: under the `crowd` setting 39% of
# act 1 picks were attacks, against 58% before and 54% without it, and its act 1 fights took 4.1
# rounds against 3.5. Now the first four or five picks of a run lean to attacks under either setting.)
PUNCH_FLOOR = {1: 5.5, 2: 5.5, 3: 6.0}


def _punch(card: dict) -> float:
    """Damage a card does for an energy, counting what stands in for damage (Doom, Poison, Shivs, orbs)."""
    i = _parse(card)
    x = 2 if i.is_x else 1
    dealt = i.total_damage * (1.4 if i.aoe else 1.0) * x + 0.8 * i.doom + 3.0 * i.poison + 4.0 * i.shivs + i.enemy_hp_loss + 6.0 * len(i.channel)
    return dealt / max(1, 2 if i.is_x else i.cost)


ATTACK_TAPER = 3.0  # cards over which the lean to attacks lets go once the deck has its share of them


def _hitters(deck: list) -> int:
    """Cards the deck holds for their damage: 8 or more an energy, the Strikes (upgraded or not) and the Regent's own Falling Star left out."""
    plain = lambda c: is_basic(c) or is_basic({"name": str(c.get("name") or "").rstrip("+")})
    return sum(1 for c in deck if _punch(c) >= 8.0 and _card_key(c) != "FALLING_STAR" and not plain(c))


BLOCK_ROOM = 2  # Block cards a deck of up to 23 takes at the full lean (learn.CHOICES["block_want"]); half for the next, none after


def _blocks(card: dict) -> bool:
    """A card taken for its Block: 5 or more when played, the Defends (upgraded or not) left out."""
    return _parse(card).block >= 5 and not is_basic(card) and not is_basic({"name": str(card.get("name") or "").rstrip("+")})


def deck_fit(card: dict, deck: list, act: int | None = None, energy: int = 3) -> float:
    """How much better or worse a card is in *this* deck than on its own.

    The ratings in cards.py judge a card alone. Three things they cannot see:
    the Regent's Stars (a 5-Star Comet is dead weight in a deck that makes
    two Stars a cycle; one run reached its boss holding five such cards it
    could never play), copies (the third Astral Pulse is worth less than
    the first), and a deck that cannot kill: one Necrobinder reached floor 25
    with 21 cards of Block, Souls and Powers around five small attacks, dealt
    76 damage in eight rounds of an ordinary fight and died there from 55 HP
    (and other players' runs rate the plain attacks lowest of all, see crowd.py).
    """
    if not deck:
        return 0.0
    fit = 0.0
    # A card that costs more than a turn's energy is only ever played with help (Bury, 4 for 52 damage,
    # on the Necrobinder's 3: taken 24 times as the best damage for its cost on offer, 2.1 floors a pick
    # under par, and seen in the hand at "4!" all through a Knowledge Demon fight lost at 12 damage a round).
    dear = _parse(card).cost > max(3, energy) and not re.search(r"reduce its cost|costs?\b[^.]*\bless", str(card.get("description") or ""), re.I)
    if dear:
        fit -= 3.0 if sum(1 for c in deck if _parse(c).energy_gain or _card_key(c) == "FRIENDSHIP") < 2 else 1.0
    floor = PUNCH_FLOOR.get(act) if len(deck) >= 8 else None
    if floor and _punch(card) >= 8.0 and not dear:
        # Worth more for as long as the deck is short of damage: about the first three attacks
        # of a run (a flat 0.8 for every act 1 attack until 2026-11-20), and any later deck that
        # has filled up with other things.
        # (How hard is a setting under test since 2026-11-21, see learn.CHOICES["punch"]: at 0.9 a point of
        # shortfall the decks built on other players' ratings stayed a point under the floor.)
        lean = knob("punch")
        short = min(4.0, 2.0 * lean / 0.9, max(0.0, lean * (floor - sum(map(_punch, deck)) / len(deck))))
        # ... up to a point: a deck's average hardly moves for one more attack, so this stayed on for two
        # acts, and the decks that lost the act 2 boss were the ones with the most attacks in them
        # (a setting under test, see learn.CHOICES["attack_room"]).
        fit += short * min(1.0, max(0.0, 1.0 - (_hitters(deck) - knob("attack_room")) / ATTACK_TAPER))
        # ... and the first one of a run comes before whatever pays off later. Other players' ratings put
        # the Powers first, and a Regent that took Pillar of Creation, Pale Blue Dot twice and Void Form
        # from its first four rewards died on floor 7. (The 13 runs whose first four picks held no attack
        # ended on floor 27 on average; with one, two or three of them, 32 to 35 alike.)
        if act == 1 and not any(_punch(c) >= 8.0 and _card_key(c) != "FALLING_STAR" for c in deck):
            fit += 1.5
    if _parse(card).weak and sum(1 for c in deck if _parse(c).weak) < 2:
        fit += knob("weak_want")  # the first two sources of Weak (a setting under test, see learn.CHOICES)
    if knob("block_want") and _blocks(card) and not dear:
        # The first Block cards beyond the Defends (a setting under test, see learn.CHOICES["block_want"]):
        # two in a deck of up to 23 cards, one more for every eight after, and half the lean for the next.
        room = max(BLOCK_ROOM, len(deck) // 8)
        fit += knob("block_want") * min(1.0, max(0.0, (room + 1 - sum(map(_blocks, deck))) / 2.0))
    need, give = _stars(card)
    if need or give:
        pairs = [_stars(c) for c in deck]
        demand = sum(n for n, _ in pairs)
        supply = STARS_AT_START + sum(g for _, g in pairs)
        if need:
            short = demand + need - supply
            if short > 0:
                fit -= min(4.0, 0.8 * short)
        if give and demand > supply:
            fit += min(2.5, 0.5 * (demand - supply))
        elif give and supply - STARS_AT_START >= demand + 3:
            # Stars nothing in the deck spends: a Regent with Venerate, Hidden Cache,
            # Gather Light and Genesis ended a fight holding 15 of them.
            fit -= min(2.0, 0.5 * give)
    key = _card_key(card)
    copies = sum(1 for c in deck if _card_key(c) == key)
    fit -= 0.6 * copies
    if not copies and not dear:
        fit += crowd.partner(key, {_card_key(c) for c in deck})  # cards other players win with together
    if key == "FRIENDSHIP" and copies:
        fit -= 2.5  # the planner leaves a second one in the hand (2 more Strength for energy it has no cards for)
    return fit


def reward_bar(deck_size: int, act: int | None) -> float:
    """What a card reward has to be worth to be taken."""
    # Be pickier as the deck grows: every mediocre card makes the good ones
    # show up less often. But acts 2 and 3 are where the deck has to do the
    # damage: the bot skipped 37% of act 2 rewards and 56% of act 3 ones, most
    # of them 3-4 point attacks, and most act 3 bosses were still above half
    # their HP when the run ended. So there the growth is halved.
    growth = 0.0 if deck_size < 16 else 1.0 if deck_size < 20 else 2.0 if deck_size < BIG_DECK else 3.0
    if isinstance(act, int) and act >= 2:
        growth *= 0.5
    return SKIP_THRESHOLD + knob("pickiness") + growth
