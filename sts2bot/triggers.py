"""Powers that act each time a card is played.

The planner used to price these only as "a Power worth playing". What they
do during a turn was invisible to it: with Sleight of Flesh 13 up, the
Necrobinder held Deathbringer (two debuffs: 26 damage) next to a four-hit
attack against a Test Subject on 33 HP about to hit for 50, did not see the
kill that would have cancelled the attack, and died on 10 HP.
"""

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


def counted(hand: list) -> list:
    """The hand, with the cards whose numbers depend on the hand read for this one.

    Flechettes, "Deal 5 damage for each Skill in your Hand": the text gives the 5 and nothing else, so it
    was one hit of 5 whatever it stood beside. (Fiend Fire, a hit for each card left when it is played, is
    counted in combat.simulate, where the cards played before it are known: read as its bare 7 it was never
    played, and an Ironclad with four cards beside it, one play allowed that turn and a boss on 44 Vulnerable
    played a Defend, then lost the fight with the boss on 2.)
    """
    skills = sum(1 for c in hand if c.type == "Skill")
    return [replace(c, hits=skills) if c.key == "FLECHETTES" else c for c in hand]


def more_doom(c, e) -> int:
    """Doom a card applies beyond the number its text begins with.

    No Escape: "Apply 10 Doom, plus an additional 5 Doom for every 10 Doom already on this enemy". Read as
    its 10 it was a card worth less than a Defile; on a boss carrying 67 Doom it is 40, three cards' worth
    for one energy (and it was played before the turn's other Doom as often as after it).
    """
    return 5 * (e.doom // 10) if c.key == "NO_ESCAPE" else 0


BURNS_HAND = {"FIEND_FIRE", "SECOND_WIND", "STOKE"}  # "Exhaust your Hand" (Second Wind: all of it that is not an Attack)


def shunned(c, state: dict) -> bool:
    """Cards the planner must not play here, whatever they seem to offer.

    Neurosurge (3 energy and 2 cards now, 3 Doom on ourselves every turn for
    the rest of the fight): Doom kills us at the end of our turn once it
    reaches our HP. Three boss fights ended that way with 24 to 31 HP left
    (Aeonglass in round 6 with two of them up; the Insatiable in round 14),
    all played on sight in round 1. It is for a fight that will be over before
    the Doom adds up: an ordinary one nearly won, or a boss or elite with few
    enough turns left in it at the pace this fight has gone so far, and our
    HP well above what the Doom will be by then. (Other players win 53% of
    the runs that hold it, the most of any Necrobinder card: their fights are
    short. Until 2026-11-20 it was never played against a boss or an elite.)
    """
    battle, player = state.get("battle") or {}, state.get("player") or {}
    if c.key in BURNS_HAND:
        # What it burns is gone for the fight, and the planner counts that as nothing: not while a Power is
        # in the hand beside it (a Demon Form it cannot pay for this turn is worth more than 7 damage).
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
    # Against the HP we will have by then, not the HP now: a boss takes some 8% of Max HP a round. Played on 67 of
    # 76 in round 5 of the Insatiable (six turns to go, 18 Doom, "under 40% of 67"), it killed a Necrobinder on
    # 13 HP in round 10 with the boss on 16. (Its ten other plays in the logs were ordinary and elite fights, all won.)
    then = int(player.get("hp") or 0) - 0.08 * int(player.get("max_hp") or 0) * turns
    return doom > 0.4 * max(0.0, then)


# Cards that make the next card of one type free ("The next Attack you play costs 0").
FREE_NEXT = {"UNRELENTING": "Attack", "POUNCE": "Skill"}
# ... and cards that make every card after them dearer this turn (Borrowed Time: 4 energy, "Cards cost an additional" 1).
SURCHARGE = {"BORROWED_TIME": 1}
# Cards that play the next card of one type an extra time ("This turn, your next Skill is played an extra time").
ECHO_NEXT = {"BURST": "Skill", "ONE_TWO_PUNCH": "Attack"}
# Cards that pay for others without saying "gain energy": Double Energy doubles what is left, and after
# Bullet Time everything in the hand is free. (Other players rate all four among their characters' best;
# read as nothing, they were worth a flat 3 points to the planner whatever was in the hand.)
PAYS = {"DOUBLE_ENERGY", "BULLET_TIME"}
READ = {*ECHO_NEXT, *PAYS, "CASCADE", "TIMES_UP", "TIME_S_UP"}  # cards with no numbers in their text that the planner works out all the same
