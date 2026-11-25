"""Powers that act each time a card is played, and other card rules the planner applies by name."""

from __future__ import annotations

from dataclasses import dataclass, field, replace
from typing import Callable

from .battle import Enemy, power_amount


@dataclass(frozen=True)
class Triggers:
    per_debuff: int = 0  # Sleight of Flesh: an enemy takes this for each debuff we put on it
    per_card: int = 0  # Serpent Form: damage to a random enemy for every card
    per_soul: int = 0  # Haunt: HP a random enemy loses for every Soul
    card_block: int = 0  # Afterimage: Block for every card
    attack_block: int = 0  # Rage: Block for every Attack
    costly_block: int = 0  # Danse Macabre: Block for every card that costs 2 or more
    doom_block: int = 0  # Shroud: Block whenever we apply Doom
    strangle: dict = field(default_factory=dict)  # enemy id -> HP it loses for every card (Strangle)
    oblivion: dict = field(default_factory=dict)  # enemy id -> Doom it gains for every card (Oblivion)


def read(state: dict) -> Triggers | None:
    """The card-play triggers in force, or None when there are none (the usual case)."""
    player = state.get("player") or {}
    foes = (state.get("battle") or {}).get("enemies") or []
    amount = lambda power: max(0, power_amount(player, power))
    on_foes = lambda power: {str(e.get("entity_id")): n for e in foes if (n := max(0, power_amount(e, power)))}
    t = Triggers(per_debuff=amount("SLEIGHT_OF_FLESH"), per_card=amount("SERPENT_FORM"), per_soul=amount("HAUNT"), card_block=amount("AFTERIMAGE"), attack_block=amount("RAGE"),
                 costly_block=amount("DANSE_MACABRE"), doom_block=amount("SHROUD"), strangle=on_foes("STRANGLE"), oblivion=on_foes("OBLIVION"))
    return None if t == Triggers() else t


NONE = Triggers()


def marks(foes: list[Enemy]) -> list[tuple]:
    """Each enemy's debuffs, to see afterwards which of them a card added to."""
    return [(e.vulnerable, e.vuln_turns, e.weak, e.weak_turns, e.doom, e.poison) for e in foes]


def fire(t: Triggers, c, foes: list[Enemy], before: list[tuple], hit: Callable, gain: Callable) -> tuple[float, int]:
    """What the triggers do once card `c` has been played: (points, Block). `hit(enemy, damage)` returns the HP removed, `gain` prices it."""
    score, block = 0.0, 0
    alive = [e for e in foes if e.alive]

    def lose(e: Enemy, n: int) -> float:  # HP loss: no Block in the way
        lost = min(e.hp, n)
        e.hp -= lost
        return gain(e, lost)

    def at_random(n: int, through_block: bool) -> float:
        if len(alive) == 1 and alive[0].alive:
            e = alive[0]
            return gain(e, hit(e, n)) if through_block else lose(e, n)
        return n * 0.8 if alive else 0.0  # no telling who: counted, but nobody is planned dead on it

    if c.doom_kills:  # End of Days: "Kill enemies with at least as much Doom as HP", now, before they act
        for e in alive:
            if e.doom >= e.hp and not e.hit_cap:
                score += lose(e, e.hp)
    doomed = False
    for e, was in zip(foes, before):
        now = (e.vulnerable, e.vuln_turns, e.weak, e.weak_turns, e.doom, e.poison)
        debuffs = (now[:2] != was[:2]) + (now[2:4] != was[2:4]) + (now[4] > was[4]) + (now[5] > was[5])
        doomed = doomed or now[4] > was[4]
        if t.per_debuff and debuffs and e.alive:
            for _ in range(debuffs):
                score += gain(e, hit(e, t.per_debuff))
    if t.per_card:
        score += at_random(t.per_card, True)
    if t.per_soul and c.key == "SOUL":
        score += at_random(t.per_soul, False)
    for e in foes:
        if e.alive and (n := t.strangle.get(e.entity_id)):
            score += lose(e, n)
        if e.alive and (n := t.oblivion.get(e.entity_id)):
            e.doom += n
            score += gain(e, n) * 0.8
    block += t.card_block
    if c.type == "Attack":
        block += t.attack_block
    if c.cost >= 2:
        block += t.costly_block
    if doomed:
        block += t.doom_block
    return score, block


def counted(hand: list) -> list:
    """The hand, with the cards whose numbers depend on the hand read for this one (Flechettes: "Deal 5 damage for each Skill in your Hand")."""
    skills = sum(1 for c in hand if c.type == "Skill")
    return [replace(c, hits=skills) if c.key == "FLECHETTES" else c for c in hand]


def more_doom(c, e) -> int:
    """Doom a card applies beyond the number its text begins with (No Escape: "plus an additional 5 Doom for every 10 Doom already on this enemy")."""
    return 5 * (e.doom // 10) if c.key == "NO_ESCAPE" else 0


BURNS_HAND = {"FIEND_FIRE", "SECOND_WIND", "STOKE"}  # "Exhaust your Hand" (Second Wind: all of it that is not an Attack)
# The first game's Necronomicurse: playable under Blue Candle (1 HP, Exhausted), and back in the hand at once whenever Exhausted;
# 2026-11-25 20:47 the bot played it over and over, a 0-cost card worth +0.1 to the planner, losing 1 HP each time.
COMES_BACK = {"NECRONOMICURSE"}


def shunned(c, state: dict) -> bool:
    """Cards the planner must not play here, whatever they seem to offer. Neurosurge puts 3 Doom on us every turn, which kills us once
    it reaches our HP: only for a fight that will be over before the Doom adds up, with our HP well above it by then."""
    battle, player = state.get("battle") or {}, state.get("player") or {}
    if c.key in COMES_BACK:
        return True
    if c.key in BURNS_HAND:  # the planner counts what it burns as nothing: not while a Power is in the hand beside it
        return any(str(h.get("type")) == "Power" for h in player.get("hand") or [])
    if c.key != "NEUROSURGE":
        return False
    foes = [e for e in battle.get("enemies") or [] if 0 < int(e.get("hp") or 0) < 100_000]
    left = sum(int(e.get("hp") or 0) + int(e.get("block") or 0) for e in foes)
    if state.get("state_type") not in ("boss", "elite"):
        return left > 60
    rounds = int(battle.get("round") or 1) - 1
    dealt = sum(max(0, int(e.get("max_hp") or 0) - int(e.get("hp") or 0)) for e in foes)
    pace = max(15.0, dealt / rounds) if rounds >= 2 else 25.0
    turns = -(-left // pace) + 1  # (and one to spare: fights run over)
    doom = max(0, power_amount(player, "DOOM")) + (3 + max(0, power_amount(player, "NEUROSURGE"))) * turns
    then = int(player.get("hp") or 0) - 0.08 * int(player.get("max_hp") or 0) * turns  # our HP by then: a boss takes some 8% of Max HP a round
    return doom > 0.4 * max(0.0, then)


ETHEREAL_KEEP = 0.3  # share of what an Ethereal card does when played that keeping it in the deck is worth, as a lasting effect (x span)


def kept(c, dmg_value: float) -> float:
    """What playing an Ethereal card is worth beyond the play: left in the hand it is Exhausted, lost for the fight (not Powers: combat.simulate)."""
    if not c.ethereal or not c.playable or c.type not in ("Attack", "Skill"):
        return 0.0
    return ETHEREAL_KEEP * (c.total_damage * dmg_value + 0.5 * c.block)


FREE_NEXT = {"UNRELENTING": "Attack", "POUNCE": "Skill"}  # cards that make the next card of one type free
SURCHARGE = {"BORROWED_TIME": 1}  # ... and cards that make every card after them dearer this turn
ECHO_NEXT = {"BURST": "Skill", "ONE_TWO_PUNCH": "Attack"}  # cards that play the next card of one type an extra time
PAYS = {"DOUBLE_ENERGY", "BULLET_TIME"}  # cards that pay for others without saying "gain energy" (energy doubled; the hand free)
READ = {*ECHO_NEXT, *PAYS, "CASCADE", "TIMES_UP", "TIME_S_UP", "KI_VERSE"}  # cards with no numbers in their text that the planner works out all the same
