"""Powers that act each time a card is played.

The planner used to price these only as "a Power worth playing". What they
do during a turn was invisible to it: with Sleight of Flesh 13 up, the
Necrobinder held Deathbringer (two debuffs: 26 damage) next to a four-hit
attack against a Test Subject on 33 HP about to hit for 50, did not see the
kill that would have cancelled the attack, and died on 10 HP.
"""

from __future__ import annotations

from dataclasses import dataclass, field
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

    def amount(power: str) -> int:
        return max(0, power_amount(player, power))

    def on_foes(power: str) -> dict:
        return {str(e.get("entity_id")): n for e in foes if (n := max(0, power_amount(e, power)))}

    t = Triggers(
        per_debuff=amount("SLEIGHT_OF_FLESH"),
        per_card=amount("SERPENT_FORM"),
        per_soul=amount("HAUNT"),
        card_block=amount("AFTERIMAGE"),
        attack_block=amount("RAGE"),
        costly_block=amount("DANSE_MACABRE"),
        doom_block=amount("SHROUD"),
        strangle=on_foes("STRANGLE"),
        oblivion=on_foes("OBLIVION"),
    )
    return None if t == Triggers() else t


NONE = Triggers()


def marks(foes: list[Enemy]) -> list[tuple]:
    """Each enemy's debuffs, to see afterwards which of them a card added to."""
    return [(e.vulnerable, e.vuln_turns, e.weak, e.weak_turns, e.doom, e.poison) for e in foes]


def fire(t: Triggers, c, foes: list[Enemy], before: list[tuple], hit: Callable, gain: Callable) -> tuple[float, int]:
    """What the triggers do once card `c` has been played: (points scored, Block gained).

    `hit(enemy, damage)` takes HP off an enemy through its Block and returns
    the HP removed; `gain(enemy, hp_removed)` prices that.
    """
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

    if c.doom_kills:
        # End of Days: "Kill enemies with at least as much Doom as HP", now, before they act.
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


def shunned(c, state: dict) -> bool:
    """Cards the planner must not play here, whatever they seem to offer.

    Neurosurge (3 energy and 2 cards now, 3 Doom on ourselves every turn for
    the rest of the fight): Doom kills us at the end of our turn once it
    reaches our HP. Three boss fights ended that way with 24 to 31 HP left
    (Aeonglass in round 6 with two of them up; the Insatiable in round 14).
    It is only for fights that will be over in a few turns.
    """
    if c.key != "NEUROSURGE":
        return False
    foes = sum(int(e.get("hp") or 0) for e in (state.get("battle") or {}).get("enemies") or [])
    return state.get("state_type") in ("boss", "elite") or foes > 60
