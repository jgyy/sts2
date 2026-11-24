"""What a card is worth in the deck that would take it, and what a card reward has to be worth to be taken.

The ratings in cards.py judge a card alone; this is what they cannot see (strategy.card_reward and
strategy.shop add it on). Moved out of strategy.py on 2026-11-21, when that file reached its 1000 lines.
"""

from __future__ import annotations

import re

from . import crowd
from ..cards import _card_key, deck_value, is_basic, parse as _parse
from ..learn import knob

SKIP_THRESHOLD = 3.0  # card rewards scoring below this are skipped
BIG_DECK = 24
# The Fool's Arrows: a Shot does nothing while our Arrow stands in an enemy (until three more hits on it, or its death),
# so a deck fires about one Shot a turn whatever it holds. Each Shot beyond the Arrows keeps this share of the one
# before it. (A guess from the rules, to be checked on Fool runs: its first ten held 2-4 Shots by floor 9-17.)
SHOT_SPARE = 0.5


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
    dealt = i.total_damage * (1.4 if i.aoe else 1.0) * x + 0.8 * i.doom + (1.5 if i.poison_needs else 3.0) * i.poison + 4.0 * i.shivs + i.enemy_hp_loss + 6.0 * len(i.channel)
    return dealt / max(1, 2 if i.is_x else i.cost)


ATTACK_TAPER = 3.0  # cards over which the lean to attacks lets go once the deck has its share of them


def _hitters(deck: list) -> int:
    """Cards the deck holds for their damage: 8 or more an energy, the Strikes (upgraded or not) and the Regent's own Falling Star left out."""
    plain = lambda c: is_basic(c) or is_basic({"name": str(c.get("name") or "").rstrip("+")})
    return sum(1 for c in deck if _punch(c) >= 8.0 and _card_key(c) != "FALLING_STAR" and not plain(c))


BLOCK_ROOM = 2  # Block cards a deck of up to 23 takes at the full lean (learn.CHOICES["block_want"]); half for the next, none after


# A Power that gives energy every turn, in rating points for the first energy a turn (2026-11-22). The Knowledge Demon
# solved exactly (`scripts/demonfight.py cards`): one such Power added to a deck without one took perfect play's chance
# of winning from 0.2% to 21.5%, 46% to 69% and 2.8% to 22.8% on three decks, where one more attack gave 0 to 10 points
# and one more Block card nothing; a second one 6 to 15 points more; the planner gets most of it (19%, 66%, 18%). Until
# then nothing at a reward saw it: Friendship stood at 5.0-5.6 and lost to Grave Warden, Scourge and the plain attacks
# (taken 6 times in 24 offers in a day's logs), and Demesne at 4.1 was skipped under the bar. (The bot's own record of
# Friendship, 99 picks, is 0.7 floors a pick under par: within what chance does to a card's record at that count.)
ENERGY_POWER = 4.0
STRENGTH_TOLL = 0.65  # of that lost to "Lose 2 Strength" in a deck whose every hit takes Strength (Friendship: +6-8 points there)
_ICONS = r"((?:\[[a-z_]*energy_icon\.png\]\s*)+)"
_TURN_ENERGY = (
    (re.compile(r"at the start of (?:your|each) turn, gain " + _ICONS, re.I), lambda m: 1.0),  # Demesne
    (re.compile(r"gain " + _ICONS + r"at the start of (?:your|each) turn", re.I), lambda m: 1.0),  # Pyre, Friendship
    (re.compile(r"every (\d+)\s*\[[a-z_]*energy_icon\.png\] you spend, gain " + _ICONS, re.I), lambda m: 3.0 / int(m.group(1))),  # Orbit, on 3 a turn
    (re.compile(r"every (\d+) cards you draw, gain " + _ICONS, re.I), lambda m: 5.5 / int(m.group(1))),  # Automation
)


def turn_energy(card: dict) -> float:
    """Energy a turn that a Power gives once it is in play (0 for anything else)."""
    if str(card.get("type")) != "Power":
        return 0.0
    text = str(card.get("description") or "")
    for pattern, rate in _TURN_ENERGY:
        m = pattern.search(text)
        if m:
            return rate(m) * len(re.findall(r"energy_icon", m.group(m.lastindex)))
    return 0.0


# A Power whose damage comes every turn of a long fight, in rating points for as much damage again as a deck deals a turn
# (2026-11-22). The Knowledge Demon solved exactly (demonfight.py's model with the Power slot made to deal damage every
# turn, six decks holding no Power, perfect play from full HP): one more attack +0 to 10 points of the chance of winning,
# an energy-every-turn Power +18 on average (ENERGY_POWER's 4.0), Countdown's 6 Doom a turn +12, Lethality's half a hit
# more a turn +17, Reaper Form (each attack's damage again as Doom) +53 on its 3 energy, +30 on the strongest deck and
# +52 on the weakest. Until then a reward saw only their face: Reaper Form stood at 3.7-4.9 and was taken 2 times in 9
# offers, Countdown at 2.7-3.6 and 0 times in 27. (Doom counts as damage: the Demon's heal does not take it off.)
SCALE_POWER = 12.0
SCALE_CAP = 8.0
TURN_DAMAGE = 30.0  # what a deck deals a round at the act 2 boss (fightlog.py bosspace: under 30 a round 25% won, 35+ 70%)
PLAYS_A_TURN = 4.0  # cards played a turn, for what a Power adds to every play of a kind


def turn_damage(card: dict, deck: list) -> float:
    """Damage (Doom counted) a Power adds every turn against one enemy once it is in play (0 for anything else)."""
    if str(card.get("type")) != "Power":
        return 0.0
    text = str(card.get("description") or "")
    m = re.search(r"at the start of your turn, apply (\d+) doom to (?:a random enemy|all enemies)", text, re.I)  # Countdown (Neurosurge's is on us)
    if m:
        return float(m.group(1))
    infos = [_parse(c) for c in deck]
    share = PLAYS_A_TURN / max(1, len(deck))  # of each card in the deck, played a turn
    hits = [i.total_damage for i in infos if i.total_damage > 0]
    m = re.search(r"first attack each turn deals (\d+)% additional damage", text, re.I)  # Lethality
    if m:
        return int(m.group(1)) / 100.0 * (sum(hits) / len(hits) if hits else 6.0)
    if re.search(r"whenever attacks deal damage, apply that much doom", text, re.I):  # Reaper Form
        return share * sum(hits)
    m = re.search(r"osty's attacks deal (\d+) additional damage", text, re.I)  # Calcify
    if m:
        return int(m.group(1)) * share * sum(1 for i in infos if i.osty_attack)
    m = re.search(r"whenever you play a soul, a random enemy loses (\d+) hp", text, re.I)  # Haunt
    if m:
        return int(m.group(1)) * share * sum(i.souls for i in infos)
    return 0.0


TIMES_UP_KEYS = ("TIMES_UP", "TIME_S_UP")  # "Deal damage equal to the enemy's Doom" (the key is spelt both ways)


def makes_doom(card: dict) -> bool:
    """Whether a card puts Doom on an enemy (Neurosurge's 3 on the Necrobinder herself does not count)."""
    text = str(card.get("description") or "")
    if re.search(r"apply \d+ doom to yourself", text, re.I):
        return False
    # (The parser leaves out Doom in a delayed clause: a Power's at the start of each turn, Countdown's; Oblivion's "whenever you
    # play a card this turn"; and Reaper Form's is "that much".)
    return _parse(card).doom > 0 or bool(re.search(r"apply that much doom|apply \d+ doom to (?:the enemy|a random enemy|all enemies)", text, re.I))


# Cards that pay off only on what other cards make (2026-11-24): Poison (Accelerant, Bubble Bubble, Mirage), Shivs
# (Accuracy, Phantom Blades, Knife Trap), Souls (Haunt, Devour Life), cards Exhausted (Feel No Pain, Dark Embrace,
# Forgotten Ritual). Taken since 2026-11-21 into decks with nothing to feed them: Bubble Bubble 9 times of 20 (8 more
# beside one card that applies Poison), Accuracy 7 of 27, Phantom Blades 3 of 11, Haunt 5 of 23, Mirage 2 of 3. Bubble
# Bubble does nothing to an enemy without Poison (the planner holds it, `poison_needs`), Accuracy without a Shiv is an
# energy for nothing; and the lean to damage counted Bubble Bubble's 9 Poison as a hit (+1.3 to +4 points at a reward).
# Only cards that do nothing else: Soul Storm's 9 damage and Evil Eye's 8 Block stand on their own.
_FEEDS = {
    "poison": re.compile(r"apply (?:\d+|x) poison", re.I),
    "shiv": re.compile(r"add (?:\d+|x|an?) (?:inky )?shivs?\b", re.I),
    "soul": re.compile(r"add (?:\d+|x|an?) souls?\b|into soul\b", re.I),
    "exhaust": re.compile(r"\bexhaust\b(?! pile)", re.I),
}
_PAYS = {
    "poison": re.compile(r"\bpoison\b", re.I),
    "shiv": re.compile(r"\bshivs?\b", re.I),
    "soul": re.compile(r"\bsouls?\b", re.I),
    "exhaust": re.compile(r"whenever a card is exhausted|if you (?:have )?exhausted a card", re.I),
}
FEED_FULL = 2.0  # feeding cards a payoff needs to be worth its rating (a Power that feeds it every turn counts twice)
FEED_KEEP = 0.3  # of a payoff's worth kept with nothing yet to feed it (one may come later)
FEED_LEAN = 1.0  # rating points for a feeding card, for each payoff held, while the deck has less than FEED_FULL of them
FEED_LEAN_MOST = 2.0


def _text(card: dict) -> str:
    return str(card.get("description") or "")


def feeds(card: dict, what: str) -> float:
    """How much a card feeds payoffs of `what` (0: not at all; a Power that does it every turn counts 2)."""
    text = _text(card)
    if not _FEEDS[what].search(text) or (what == "exhaust" and _PAYS["exhaust"].search(text)):
        return 0.0
    if what == "poison" and _parse(card).poison_needs:
        return 0.0  # Bubble Bubble's Poison needs Poison already there
    return 2.0 if str(card.get("type")) == "Power" else 1.0


def pays_off(card: dict) -> str | None:
    """What a card needs other cards to make before it does anything (None: it stands on its own)."""
    info = _parse(card)
    if info.total_damage > 0 or info.block > 0:
        return None
    text = _text(card)
    for what, pattern in _PAYS.items():
        if pattern.search(text) and (not feeds(card, what) or (what == "poison" and info.poison_needs)):
            return what
    return None


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
    if _card_key(card) in TIMES_UP_KEYS and not any(makes_doom(c) for c in deck):
        # Time's Up deals the Doom the enemy already has. A deck that puts none on one has nothing for it to hit: 20 of the
        # 63 fights a Necrobinder held it in (2026-11-22 logs) had none of the Doom cards the logs name (Scourge, No Escape,
        # Countdown, Reaper Form, Blight Strike), while cards.py rated it a flat 3.0 (4-5 at the offers).
        return -deck_value(card)
    fit = 0.0
    if _parse(card).shoots:
        beyond = sum(1 for c in deck if _parse(c).shoots) - sum(1 for c in deck if _card_key(c) == "QUIVER")
        fit -= max(0.0, deck_value(card)) * (1.0 - SHOT_SPARE**beyond) if beyond > 0 else 0.0
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
    rate = turn_energy(card)
    if rate:
        # Each energy a turn already there (from such Powers, or a relic's that the last fight began with) halves the
        # next: on the Demon a deck went 0.2% -> 61% -> 82% on 3, 4 and 5 energy a turn.
        held = sum(map(turn_energy, deck)) + max(0, energy - 3)
        gain = ENERGY_POWER * min(rate, 1.0) * 0.5**held
        if _parse(card).strength < 0 and _parse(card).strength_lasts:
            # Friendship's 2 Strength comes off every hit that takes Strength: not Osty's, not Doom, not Poison.
            hitters = [c for c in deck if _parse(c).total_damage > 0]
            gain *= 1.0 - STRENGTH_TOLL * (sum(1 for c in hitters if not _parse(c).osty_attack) / max(1, len(hitters)))
        fit += gain
    added = turn_damage(card, deck)
    if added:
        fit += min(SCALE_CAP, SCALE_POWER * added / TURN_DAMAGE)
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
    for what in _FEEDS:
        made = feeds(card, what)
        if made:
            # A feeding card for payoffs the deck already holds and cannot yet feed.
            payoffs = sum(1 for c in deck if pays_off(c) == what)
            fed = sum(feeds(c, what) for c in deck)
            if payoffs and fed < FEED_FULL:
                fit += min(FEED_LEAN_MOST, FEED_LEAN * payoffs) * (1.0 - fed / FEED_FULL)
    need = pays_off(card)
    if need:
        fed = sum(feeds(c, need) for c in deck)
        share = FEED_KEEP + (1.0 - FEED_KEEP) * min(1.0, fed / FEED_FULL)
        fit -= (1.0 - share) * max(0.0, deck_value(card) + fit)
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
