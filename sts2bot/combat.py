"""Combat decisions: pick the best set of cards for this turn, play one, re-plan.

Each call looks at the current hand, scores every affordable subset of it
with a small simulation (damage, kills, block against incoming intents) and
returns the first card of the winning line. Re-planning after every card means
draws and other surprises are picked up automatically.
"""

from __future__ import annotations

import logging
import re
from dataclasses import dataclass, replace
from itertools import combinations, permutations

from .battle import (  # noqa: F401  (also the names other modules and the tests reach through here)
    BEYOND_REACH, LETHAL_PENALTY, ORB_VALUES, POWER_VALUE, RAGE_BLOCK, SANDPIT_DEATH, WIN_BONUS, Enemy,
    _has_power, _is_attack, constrict, end_of_turn_block, end_of_turn_loss, expected_loss, hand_burns,
    intent_damage, intent_hits, osty_hp, power_amount, read_enemies,
)
from .cards import CardInfo, parse
from .potions import describe, potion_actions  # noqa: F401
from .learn import knob

Action = tuple[str, dict]

log = logging.getLogger("sts2bot.combat")

MAX_SEARCH_CARDS = 11


def _order_key(c: CardInfo) -> tuple:
    if c.type == "Power" or c.key == "FRANTIC_ESCAPE":
        rank = 0  # set-up first; an escape must not be left for energy that is gone
    elif c.draw or c.energy_gain or c.star_gain:
        rank = 1  # what later cards are paid with comes first
    elif c.vulnerable:
        rank = 2
    elif c.damage and not c.is_x:
        rank = 3
    elif c.is_x:
        rank = 4
    elif c.block:
        rank = 5
    else:
        rank = 6
    return (rank, -c.total_damage, c.index)


def _hit_count(e: Enemy, hits: int, double_if_vulnerable: bool = False) -> int:
    return hits * (2 if double_if_vulnerable and e.vulnerable else 1)


def _hit(e: Enemy, dmg_per_hit: int, hits: int, double_if_vulnerable: bool = False, raw: bool = False) -> int:
    """Apply an attack hit by hit, return HP actually removed.

    `raw` is damage that is not an Attack (an orb's): Vulnerable does nothing
    for it (a Lightning evoked twice did 16 to a Vulnerable enemy, 18 times in 18).
    """
    per = dmg_per_hit * e.damage_mult
    if not raw:
        per *= (e.vuln_mult if e.vulnerable else 1.0) * (e.weak_mult if e.weak else 1.0)
        if e.slow > 0:
            per *= 1.0 + e.slow / 100.0
    per = int(per)
    removed = 0
    for _ in range(_hit_count(e, hits, double_if_vulnerable)):
        if e.hp <= 0:
            break
        absorbed = min(e.block, per)
        e.block -= absorbed
        e.chipped += absorbed
        if absorbed and e.block == 0 and e.keeps_block:
            e.keeps_block = False
            e.incoming = 0  # "Stunned if all Block is removed."
        through = per - absorbed
        if through <= 0:
            continue
        if e.hit_cap:
            through = min(through, e.hit_cap)
        if e.slippery > 0:
            e.slippery -= 1
            through = 1
        lost = min(e.hp, through)
        e.hp -= lost
        removed += lost
        if e.stun_at and e.hp <= e.stun_at:
            e.stun_at = 0
            e.incoming = 0  # Stunned: its attack this turn never happens
    return removed


def _respawns(foes: list[Enemy]) -> None:
    """Stock: an enemy that dies while it has Stock is summoned again in its place at full HP.

    The death is not a kill: no win, no kill bonus, and the damage it took
    goes for nothing once it is back. Axebot came back at 85 HP after the
    bot had counted its death at 5 HP as the fight nearly won.
    """
    for e in foes:
        if not e.alive and e.respawns > 0 and e.max_hp > 0:
            e.hp, e.block = e.max_hp, 0
            e.respawns -= 1
        elif not e.alive and e.adapts:
            # The Test Subject's first two forms: beating one is not the fight
            # won. It leaves for the rest of the turn (nothing more can be
            # aimed at it, and its attack does not happen) and comes back next
            # turn with 100 HP more. Counted as a win, the kill used to come
            # before the Powers that the next 500 HP were going to need.
            e.hp, e.block, e.spent, e.damage_mult = BEYOND_REACH, 0, True, 0.0
            e.incoming = e.incoming_hits = 0
            e.poison = e.doom = 0
            e.adapts, e.adapted = False, True


def _rage_blocks(foes: list[Enemy], known_dead: set) -> None:
    """Crab Rage, as the line plays out: each ally that dies gives the crabs still standing 99 Block.

    The Block is only there until their next turn, so the damage we aim at
    them for the rest of this turn is absorbed. In one lost fight the bot hit
    a Rocket four times with 93 Block on it: its Block fell to 76 and its HP
    stayed at 113.
    """
    newly = [e for e in foes if not e.alive and e.entity_id not in known_dead]
    if not newly:
        return
    known_dead.update(e.entity_id for e in newly)
    for e in foes:
        if e.alive and e.rage:
            e.block += RAGE_BLOCK * len(newly)


def _orb_damage(kind: str, amount: int, foes: list[Enemy], chip_rate: float, dmg_value: float) -> float:
    """What an orb's damage is worth, taken off the enemies it lands on.

    Glass hits everyone, Dark the enemy with the least HP, Lightning one at
    random: with a single enemy left that is no gamble, and the damage is
    applied, so a kill by orb is seen as a kill (it used to be points only,
    and a boss on 12 HP facing two Lightning evokes was not "dead").
    """
    alive = [e for e in foes if e.alive]
    if amount <= 0 or not alive:
        return 0.0
    if kind == "GLASS":
        targets = alive
    elif kind == "DARK":
        targets = [min(alive, key=lambda e: e.hp)]
    elif len(alive) == 1:
        targets = alive
    else:
        return amount * 0.8 * dmg_value
    return sum(_progress(e, _hit(e, amount, 1, raw=True), chip_rate) * dmg_value * e.worth for e in targets)


def _would_kill(e: Enemy, dmg_per_hit: int, hits: int, double_if_vulnerable: bool = False) -> bool:
    trial = replace(e)
    _hit(trial, dmg_per_hit, hits, double_if_vulnerable)
    return not trial.alive


def _pick_target(enemies: list[Enemy], dmg_per_hit: int, hits: int, double_if_vulnerable: bool = False) -> Enemy | None:
    alive = [e for e in enemies if e.alive]
    if not alive:
        return None

    killable = [e for e in alive if _would_kill(e, dmg_per_hit, hits, double_if_vulnerable)]
    if killable:
        # Remove the biggest threat first; waste the least damage on ties.
        # Killing something that revives only pays off if it is about to hit us.
        useful = [e for e in killable if e.worth > 0 or e.incoming > 0]
        if useful:
            return max(useful, key=lambda e: (e.worth > 0, e.incoming, e.hp + e.block))
    # Otherwise focus whoever dies soonest, counting Vulnerable as less health;
    # leaders before their minions, never something that just comes back.
    return min(alive, key=lambda e: (-e.worth, (e.hp + e.block) / (1.5 if e.vulnerable else 1.0), -e.incoming))


def _resists(e: Enemy) -> bool:
    """Artifact swallows a debuff. Mangle on an enemy with Artifact took none
    of its Strength away, and the bot walked into the full attack."""
    if e.artifact > 0:
        e.artifact -= 1
        return True
    return False


DEBUFF_CAP = 4  # turns of a debuff beyond this are too far off to count


def _expose(e: Enemy, turns: int = 1) -> None:
    if not _resists(e):
        e.vulnerable = True
        fresh = max(0, min(turns, DEBUFF_CAP - e.vuln_turns))
        e.vuln_turns += fresh
        e.vuln_added += fresh


def _sap(e: Enemy, strength: int) -> None:
    """The enemy loses Strength for this turn: every hit it makes is that much weaker."""
    if not _resists(e):
        e.incoming = max(0, e.incoming - strength * e.incoming_hits)


def _weaken(e: Enemy, turns: int = 1) -> None:
    """A newly Weak enemy hits for 25% less this turn."""
    if _resists(e):
        return
    fresh = max(0, min(turns, DEBUFF_CAP - e.weak_turns))
    e.weak_turns += fresh
    e.weak_added += fresh
    if not e.weak:
        e.weak = True
        e.incoming = int(e.incoming * e.weak_cut)


def _poison_total(stack: int, turns: int) -> int:
    """HP a Poison stack takes over the next `turns` turns (it loses 1 a turn)."""
    stack = max(0, stack)
    if stack >= turns:
        return turns * stack - turns * (turns - 1) // 2
    return stack * (stack + 1) // 2


def _progress(e: Enemy, hp_removed: int, chip_rate: float) -> float:
    """How far an attack moved the fight on: HP removed, plus Block worn down.

    Block an enemy keeps between turns is as good as HP. Only counting HP,
    the bot never attacked a Burrowed enemy (20 Block that stays, 24 damage
    a turn) and blocked every turn instead, for 180 rounds.
    """
    chip, e.chipped = e.chipped, 0
    return hp_removed + chip * (1.0 if e.keeps_block or (e.incoming == 0 and chip and e.block == 0 and chip_rate >= 1) else chip_rate)


@dataclass
class Outcome:
    score: float
    first_target: str | None
    lost: int = 0  # HP the line expects to lose when the turn ends


def simulate(
    line: list[CardInfo],
    enemies: list[Enemy],
    *,
    energy: int,
    stars: int,
    hp: int,
    max_hp: int,
    block: int,
    fight: str = "monster",  # monster, elite or boss
    round_no: int = 1,
    card_limit: int | None = None,
    can_block: bool = True,
    sandpit: int | None = None,
    end_loss: int = 0,  # HP we lose at the end of the turn whatever we play
    burns: tuple = (),  # (hand index, HP, stopped by Block) for cards that hurt while they stay in the hand
    hand_size: int = 0,
    focus: str | None = None,  # aim every targeted card at this enemy while it lives
    orbs: tuple = (),  # (kind, passive, evoke) for each orb, oldest first
    orb_slots: int = 0,
    shiv_damage: int = 4,
    tender: bool = False,  # every card played costs 1 Strength and 1 Dexterity for the turn
    vigor: int = 0,  # extra damage on the next Attack only (the card text shows it on all of them)
    giant: bool = False,  # the next Attack deals triple damage (same)
    buffer: int = 0,  # times we lose no HP when we otherwise would
    taint: int = 0,  # Vital Spark: each Skill played makes every hit on us this turn this much harder
    draw_worth: tuple = (2.0, 0.0, 0.0),  # of a drawn card: (points if free, share that cost energy, points per energy)
    bare_block: int = 0,  # Orichalcum: Block gained if the turn ends with none
    regen: int = 0,  # Regen: HP healed when the turn ends, before the enemy moves
    focus_power: int = 0,  # Focus: added to what a newly channeled orb does
    escape_odds: tuple = (0.3, 0.3),  # chance of no Frantic Escape in the next hand, and in a hand later on
    wither_in: int = 0,  # Withering Presence: cards we can still play before a Wither lands in our hand
    wither_hp: int = 5,  # ... and what that Wither will cost (each one is a point worse than the last)
) -> Outcome | None:
    """Score playing `line` in order. None if it cannot be afforded."""
    if card_limit is not None and len(line) > card_limit:
        return None
    foes = [replace(e, chipped=0, vuln_added=0, weak_added=0) for e in enemies]
    score = 0.0
    first_target: str | None = None
    self_loss = 0
    # Long fights are races: bosses and elites grow stronger every turn, so a
    # turn spent only blocking makes the next one worse. The bot used to sit
    # behind Defends for 19 rounds against a boss gaining Strength and lose.
    dmg_value = {"boss": 1.5, "elite": 1.3}.get(fight, 1.0) + min(0.4, 0.08 * max(0, round_no - 5))
    # A fight that drags on is one the bot is not winning by waiting: lean
    # harder on attacking every round until something gives.
    dmg_value += 0.5 * max(0, round_no - 12)
    chip_rate = min(1.0, 0.25 + 0.05 * max(0, round_no - 10))  # worth of temporary Block removed
    kept_block = {e.entity_id for e in enemies if e.keeps_block}
    escapes = 0  # Frantic Escape cards played in this line
    faced: str | None = None  # the last enemy a card was aimed at
    nimble = 0  # Dexterity gained from cards earlier in this line
    held = list(orbs)  # the Defect's orbs as the line goes on
    lasting = 0.0  # worth of effects that pay off over the rest of the fight (scaled by `span` below)
    later = 0.0  # worth of things that arrive next turn
    # What a lasting effect is still worth: fights against bosses go on for a
    # dozen turns, ordinary ones are over in four.
    span = {"boss": 3.0, "elite": 2.0}.get(fight, 1.0)
    late = {"boss": 7, "elite": 5}.get(fight, 3)
    span *= 1.0 if round_no <= late else 0.4
    might = 0  # Strength gained from cards earlier in this line
    attacks = 0  # Attacks played so far in this line
    summoned = 0  # HP this line gives Osty
    drawn = 0  # cards this line draws
    tainted = 0
    known_dead = {e.entity_id for e in foes if not e.alive}

    for n, c in enumerate(line):
        _respawns(foes)
        _rage_blocks(foes, known_dead)  # a crab that died with the last card hands its ally Block
        if c.star_cost > stars:
            return None
        x = 1
        if c.star_cost < 0:  # costs X Stars: all of them
            if stars <= 0:
                return None
            x, stars = stars, 0
        else:
            stars -= c.star_cost
        if c.is_x:
            if energy <= 0:
                return None
            x, energy = energy, 0
        else:
            if c.cost > energy:
                return None
            energy -= c.cost
        energy += c.energy_gain
        self_loss += c.hp_loss
        if can_block and c.block:
            block += max(0, c.block + nimble) * x
        if can_block and c.summon:
            # Osty stands in front of the Necrobinder: his HP soaks hits like
            # Block that does not go away at the end of the turn.
            block += c.summon * x
            summoned += c.summon * x
        if c.block_lock:
            # Trading away future block is only worth it to survive this turn.
            score -= 30.0
        if c.deadly:
            score -= LETHAL_PENALTY  # never gamble the run on one card

        if c.damage:
            per, hits = c.damage + might, c.hits * x
            if c.type == "Attack":
                if attacks:  # Vigor and Gigantification went with the first Attack
                    per = round(per / 3) if giant else per
                    per -= vigor
                attacks += 1
            per = max(0, per)
            alive = [e for e in foes if e.alive]
            if c.aoe:
                for e in alive:
                    self_loss += e.thorns * _hit_count(e, hits, c.double_if_vulnerable)
                    score += _progress(e, _hit(e, per, hits, c.double_if_vulnerable), chip_rate) * dmg_value * e.worth
                    if e.alive:
                        if c.vulnerable:
                            _expose(e, c.vulnerable)
                        if c.weak:
                            _weaken(e, c.weak)
            elif c.random_target and len(alive) > 1:
                score += per * hits * 0.8 * dmg_value
            else:
                t = next((e for e in foes if e.alive and e.entity_id == focus), None) or _pick_target(
                    foes, per, hits, c.double_if_vulnerable
                )
                if t is not None:
                    faced = t.entity_id
                    if n == 0:
                        first_target = t.entity_id
                    self_loss += t.thorns * _hit_count(t, hits, c.double_if_vulnerable)
                    score += _progress(t, _hit(t, per, hits, c.double_if_vulnerable), chip_rate) * dmg_value * t.worth
                    if c.vulnerable and t.alive:
                        _expose(t, c.vulnerable)
                    if c.weak and t.alive:
                        _weaken(t, c.weak)
                    if c.enemy_strength_loss and t.alive:
                        _sap(t, c.enemy_strength_loss)
        elif c.needs_target:
            alive = [e for e in foes if e.alive]
            if c.enemy_strength_loss:
                t = max(alive, key=lambda e: min(e.incoming, c.enemy_strength_loss * e.incoming_hits), default=None)
            elif c.weak and not c.vulnerable:
                # Weaken the hardest hitter that is not Weak yet.
                t = max(alive, key=lambda e: (not e.weak, e.incoming), default=None)
            else:
                # Vulnerable goes on the enemy we will be hitting the longest.
                fresh = [e for e in alive if not e.vulnerable] or alive
                t = max(fresh, key=lambda e: e.hp + e.block, default=None)
            t = next((e for e in alive if e.entity_id == focus), None) or t
            if t is not None:
                faced = t.entity_id
                if n == 0:
                    first_target = t.entity_id
                if c.vulnerable:
                    _expose(t, c.vulnerable)
                if c.weak:
                    _weaken(t, c.weak)
                if c.enemy_strength_loss:
                    _sap(t, c.enemy_strength_loss)
        elif c.aoe:
            for e in foes:
                if e.alive:
                    if c.vulnerable:
                        _expose(e, c.vulnerable)
                    if c.weak:
                        _weaken(e, c.weak)

        alive_now = [e for e in foes if e.alive]
        aimed = next((e for e in alive_now if e.entity_id == (faced or focus)), None)
        if c.poison or c.doom or c.enemy_hp_loss or c.shivs:
            if c.aoe:
                victims = alive_now
            elif aimed is not None and (c.needs_target or c.damage):
                victims = [aimed]
            else:  # untargeted or random: the enemy that matters most
                victims = sorted(alive_now, key=lambda e: (-e.worth, e.hp))[:1]
            for e in victims:
                if c.poison and not _resists(e):
                    e.poison += c.poison * x
                if c.doom and not _resists(e):
                    e.doom += c.doom * x
                    score += c.doom * x * 0.8 * dmg_value * e.worth
                if c.enemy_hp_loss:
                    lost_hp = min(e.hp, c.enemy_hp_loss)
                    e.hp -= lost_hp
                    score += lost_hp * dmg_value * e.worth
                if c.shivs and e is victims[0]:
                    # Shivs are free 4-damage cards: as good as that damage now.
                    score += _progress(e, _hit(e, shiv_damage + might, c.shivs * x), chip_rate) * dmg_value * e.worth * 0.9
        if c.needs_target and not c.damage and aimed is None and alive_now and (c.poison or c.doom):
            if n == 0 and first_target is None:
                first_target = sorted(alive_now, key=lambda e: (-e.worth, -e.hp))[0].entity_id
        stars += c.star_gain
        score += c.star_gain * 2.5 + c.souls * 2.5
        lasting += c.forge * 0.5
        later += c.next_energy * 5.0 + c.next_draw * 1.5 + c.next_block * 0.8
        if c.dexterity:
            nimble += c.dexterity
            if c.dexterity_lasts:
                lasting += c.dexterity * 3.5
        for kind in c.channel:
            if orb_slots and len(held) >= orb_slots:
                old = held.pop(0)  # no room: the oldest orb is evoked to make space
                if old[0] == "FROST" and can_block:
                    block += old[2]
                elif old[0] in ("LIGHTNING", "DARK", "GLASS"):
                    score += _orb_damage(old[0], old[2], foes, chip_rate, dmg_value)
            passive, burst = ORB_VALUES.get(kind, (3, 6))
            if kind != "PLASMA":
                passive, burst = max(0, passive + focus_power), max(0, burst + focus_power)
            held.append((kind, passive, burst))
            lasting += {"FROST": 3.0, "LIGHTNING": 3.0, "DARK": 4.0, "PLASMA": 6.0, "GLASS": 4.0}.get(kind, 3.0)
        # "Evoke your rightmost Orb": the one channeled first, which the game
        # lists first. Taking it for the newest, the bot counted on a fresh
        # Frost's Block from Dualcast, got a Glass orb's damage instead and
        # died 3 HP short with the boss on 51.
        for _ in range(c.evoke * (x if c.is_x else 1)):
            if held:
                kind, passive, burst = held[0]
                if kind == "FROST" and can_block:
                    block += burst
                elif kind in ("LIGHTNING", "DARK", "GLASS"):
                    score += _orb_damage(kind, burst, foes, chip_rate, dmg_value)
                elif kind == "PLASMA":
                    energy += 2
        if c.evoke and held:
            held.pop(0)  # evoking uses the orb up
        if c.evoke_all:  # Shatter: every orb goes off, and all of them are used up
            for kind, _passive, burst in held:
                for _ in range(c.evoke_all):
                    if kind == "FROST" and can_block:
                        block += burst
                    elif kind in ("LIGHTNING", "DARK", "GLASS"):
                        score += _orb_damage(kind, burst, foes, chip_rate, dmg_value)
                    elif kind == "PLASMA":
                        energy += 2
            held = []
        if c.key == "FRANTIC_ESCAPE":
            escapes += 1
        for e in foes:
            if e.slow >= 0:
                e.slow += 10  # for the cards after this one (measured: the card itself gets no bonus)
        if tender:
            might -= 1
            nimble -= 1
        if c.type == "Skill":
            tainted += taint
        if c.type == "Skill":
            # Enrage: every Skill makes each of its hits stronger for as long as
            # it lives. Against the Test Subject's first form (100 HP) the bot
            # played seven Skills and then took a 39-point hit: the price is the
            # extra damage on every turn the enemy still has in it.
            score -= sum(e.enrage * 2.0 * max(1, e.incoming_hits) * min(4.0, 1.0 + e.hp / 35.0) for e in foes if e.alive)
        drawn += c.draw * x if c.is_x else c.draw
        if c.strength:
            might += c.strength
            if c.strength_lasts:
                # Pays off on every attack for the rest of the fight.
                score += c.strength * 4.0 * span
        if c.type == "Power" and c.strength_lasts:
            pass  # valued just above
        elif c.type == "Power":
            score += POWER_VALUE.get(c.key, 5.0 + 2.0 * max(c.cost, 0)) * span
        elif c.type in ("Status", "Curse"):
            score += 0.1  # only worth clearing out with energy nothing else can use
        elif not c.known:
            score += 2.5 + max(c.cost, 0)

    # What the cards drawn are likely to be worth: the free ones as they
    # come, the rest as far as the energy left over stretches.
    free, paid_share, per_energy = draw_worth
    score += drawn * max(1.0, free) + min(float(max(energy, 0)), drawn * paid_share) * per_energy
    # Breaking Block that an enemy was keeping Stuns it.
    score += 15.0 * sum(1 for e in foes if e.alive and e.entity_id in kept_block and not e.keeps_block)
    # The Defect's orbs act when the turn ends: Frost adds Block, Lightning hits something.
    # They hit before the enemy acts: one that dies of them does not attack.
    _respawns(foes)
    _rage_blocks(foes, known_dead)
    for kind, passive, _burst in held:
        if kind == "FROST" and can_block:
            block += passive
        elif kind in ("LIGHTNING", "GLASS"):
            score += 0.9 * _orb_damage(kind, passive, foes, chip_rate, dmg_value)
    score += lasting * span + later
    # Poison bites at the start of the enemy's turn, before it acts: one that
    # is poisoned through is as good as dead now. Otherwise it is damage on
    # its way, a little less each turn.
    # What is added to a stack keeps ticking for as long as the stack lasts: 5
    # Poison on a fresh boss is 15 damage, the same 5 on top of 10 is 35 over
    # seven turns. Priced at a flat 4 a point, stacking it was never worth
    # more than a Strike.
    poison_turns = ({"boss": 7, "elite": 4}.get(fight, 3)) if round_no <= late else 3
    for e, orig in zip(foes, enemies):
        if not e.alive:
            continue
        # (Not through Intangible: 8 Poison took 1 HP off a Soul Fysh on 6, which the
        # bot had counted as dead, and it sat on two Defends while it was hit for 9.)
        if (min(e.poison, e.hit_cap) if e.hit_cap else e.poison) >= e.hp:
            e.hp = 0
            continue
        if e.poison > orig.poison:
            gain = _poison_total(e.poison, poison_turns) - _poison_total(orig.poison, poison_turns)
            score += min(e.hp, gain) * e.worth * (dmg_value if e.hp > 60 else 1.0)
        score += min(e.hp, e.poison) * 0.3 * e.worth  # the tick that comes this turn
    # Vulnerable is half again our damage and Weak a quarter off theirs, for
    # every turn they last. A flat bonus undersold both in long fights, where
    # they are worth more than any single attack.
    vuln_turn = {"boss": 7.0, "elite": 5.0}.get(fight, 3.0)
    weak_turn = {"boss": 5.0, "elite": 3.5}.get(fight, 2.0)
    for e in foes:
        if e.alive:
            # The first turn of Weak is already in `incoming`; Vulnerable used
            # this turn is already in the damage dealt.
            score += (max(0, e.vuln_added - 1) + 0.5 * min(1, e.vuln_added)) * vuln_turn * e.worth
            score += (max(0, e.weak_added - 1) + 0.3 * min(1, e.weak_added)) * weak_turn * (1.0 if e.incoming_hits or not e.minion else 0.5)
    # Doom finishes an enemy at the end of its turn once it covers its HP.
    score += 20.0 * sum(e.worth for e, orig in zip(foes, enemies) if e.alive and e.doom >= e.hp and orig.doom < orig.hp)
    _respawns(foes)  # a Stock enemy killed by the last card comes back too
    kills = [e for e, orig in zip(foes, enemies) if not e.alive and orig.alive]
    score += 25.0 * sum(e.worth for e in kills)
    score += 40.0 * sum(1 for e in foes if e.adapted)  # a form beaten: that much of the fight is done
    survivors = [e for e in foes if e.alive]
    # The fight is over once every leader is dead: minions leave with them.
    if not any(not e.minion for e in survivors) and any(not e.minion for e in enemies):
        score += WIN_BONUS
        survivors = []
    elif not survivors:
        score += WIN_BONUS

    if kills and survivors:
        # Crab Rage: the one left standing gets 99 Block (its attacks this turn
        # are wasted, see _rage_blocks) and 6 Strength for good. Killing the
        # first crab with the second on 126 HP lost that fight, so the cost
        # grows with the HP left on the survivor. It has no flat base: two
        # crabs at 6 HP must still be killed one after the other, or the bot
        # ends every turn and is hit to death.
        for e in survivors:
            if e.rage:
                score -= len(kills) * 2.0 * e.hp
                e.incoming += 6 * e.incoming_hits * len(kills)

    if sandpit is not None and survivors:
        # The Insatiable: Sandpit counts down every turn and at 0 we are eaten,
        # whatever our HP. Each Frantic Escape played pushes it back by one.
        left = sandpit + escapes
        if left <= 1:
            score -= LETHAL_PENALTY
        else:
            # Ending the turn on 2 means the next hand has to hold an Escape,
            # and three hands in ten do not: eight of fifteen lost fights
            # ended in the pit with HP to spare, one with the boss on 4 HP.
            # So the margin is priced by the chance of running out of it.
            no_escape, usually = escape_odds
            risk = no_escape * usually ** (left - 2)
            # (The floor is what the risk is worth at the usual odds: a bot that
            # trusted better ones ended a turn on 2 with an Escape and the energy
            # for it in hand, and was eaten on 47 HP.)
            score -= max(SANDPIT_DEATH * risk, {2: 180.0, 3: 55.0, 4: 16.0, 5: 5.0}.get(left, 0.0))

    if faced is not None:
        # Turning to face an enemy takes away its 50% bonus and gives it to the other one.
        for e in survivors:
            if e.flanker and e.behind and e.entity_id == faced:
                e.incoming = round(e.incoming / 1.5)
            elif e.flanker and not e.behind and e.entity_id != faced:
                e.incoming = round(e.incoming * 1.5)

    incoming = sum(e.incoming + tainted * e.incoming_hits for e in survivors)
    if block <= 0 and can_block:
        block = bare_block
    played = {c.index for c in line}
    left = max(0, hand_size - len(line))
    # Cards that burn in the hand go off when our turn ends, before the enemy
    # moves: the ones that deal damage eat into our Block first.
    scorch = sum(amount for index, amount, blocked in burns if blocked and index not in played)
    burnt = sum((left if amount < 0 else amount) for index, amount, blocked in burns if not blocked and index not in played)
    if wither_in > 0 and len(line) >= wither_in:
        # A Wither sits in the hand and burns when the turn ends, then comes
        # round again with the deck: the sixth card has to be worth that.
        scorch += wither_hp * (1 + (len(line) - wither_in) // 6)
    burnt += max(0, scorch - block)
    through = max(0, incoming - max(0, block - scorch))
    if through and buffer >= sum(e.incoming_hits for e in survivors if e.incoming):
        through = 0  # Buffer swallows each hit that would have hurt
    if through:
        score -= sum(e.scar * e.incoming_hits for e in survivors if e.incoming)
    if wither_in > 0:
        # ... and it stays in the deck: it burns again every time it is drawn
        # and takes a card's place in the hand (by round 7 one fight had five
        # of them in an eight-card hand, 25 HP a turn). The count carries over
        # between turns, so every card played is a sixth of that bill.
        score -= len(line) * (4.0 * wither_hp + 20.0) / 6.0
    lost = through + self_loss + end_loss + burnt
    lost = max(0, lost - min(regen, max(0, max_hp - hp + self_loss)))  # Regen mends what was missing first
    ratio = hp / max(1, max_hp)
    # Osty keeps the HP he is not hit for: unlike Block it is still there next
    # turn (and Unleash hits harder for it). The bot never summoned on a turn
    # without an attack coming, which is exactly when it costs nothing.
    score += 1.1 * min(summoned, max(0, block - incoming))
    # Every leader left is Doomed to die at the end of its turn: the fight is
    # won if we live through that turn, so more damage is wasted energy.
    doomed = (
        bool(survivors)
        and all(e.doom >= e.hp for e in survivors if not e.minion)
        and any(not e.minion for e in survivors)
        and not any(e.adapts or e.spent for e in survivors)
    )
    if doomed:
        score += WIN_BONUS / 2
    # A point of our HP is worth more than a point of enemy HP even at full
    # health: at 1.0 a Strike (6) beat a Defend (5) and the bot never blocked
    # until it was already hurt, bleeding HP in every ordinary fight.
    # ... and worth more still when this one turn would take a big bite out
    # of what is left: at 37 HP facing 27 the bot attacked with two Defends
    # in hand and was dead a turn later.
    bite = min(1.0, lost / max(1, hp))
    score -= lost * (1.5 + 2.5 * (1.0 - ratio) + 3.0 * bite) * (4.0 if doomed else 1.0) * knob("caution")
    if lost >= hp:
        # Dead on paper. If every line is, the least dead one is the one to
        # play: the most Block, and cards drawn while there is energy left to
        # play them (one of them may be the Block that is missing). A flat
        # penalty left that choice to the damage dealt, and a bot 4 HP short
        # played two Powers with a Defend+ in hand.
        score -= LETHAL_PENALTY + 100.0 * (lost - hp) - (150.0 if energy > 0 else 30.0) * min(drawn, 3)
    return Outcome(score, first_target, int(lost))


def expected_end_loss(state: dict) -> int | None:
    """HP the planner expects to lose if the turn ended now (logged, to catch what it does not model)."""
    enemies = read_enemies(state)
    if not enemies:
        return None
    out = simulate([], enemies, **_context(state))
    return out.lost if out is not None else None


def _context(state: dict) -> dict:
    player = state.get("player") or {}
    battle = state.get("battle") or {}
    return dict(
        energy=int(player.get("energy") or 0),
        stars=int(player.get("stars") or 0),
        hp=int(player.get("hp") or 1),
        max_hp=int(player.get("max_hp") or 1),
        block=int(player.get("block") or 0) + end_of_turn_block(player) + osty_hp(player),
        fight=str(state.get("state_type") or "monster"),
        round_no=int(battle.get("round") or 1),
        card_limit=cards_left_this_turn(state),
        can_block=not _has_power(player, "NO_BLOCK"),
        sandpit=_sandpit(state),
        escape_odds=_escape_odds(player),
        end_loss=end_of_turn_loss(player) + constrict(player),
        burns=hand_burns(player),
        hand_size=len(player.get("hand") or []),
        wither_in=max((power_amount(e, "WITHERING_PRESENCE") for e in battle.get("enemies") or []), default=0),
        wither_hp=_next_wither(player, int(battle.get("round") or 1)),
        draw_worth=_draw_worth(player),
        orbs=tuple(
            (
                str(o.get("id") or o.get("name") or "").upper().replace("_ORB", ""),
                int(o.get("passive_val") or 0),
                int(o.get("evoke_val") or 0),
            )
            for o in player.get("orbs") or []
        ),
        orb_slots=int(player.get("orb_slots") or 0),
        bare_block=_bare_block(player),
        regen=max(0, power_amount(player, "REGEN")),
        focus_power=power_amount(player, "FOCUS") if _has_power(player, "FOCUS") else 0,
        shiv_damage=4 + max(0, power_amount(player, "ACCURACY")),
        tender=_has_power(player, "TENDER"),
        vigor=max(0, power_amount(player, "VIGOR")),
        giant=_has_power(player, "GIGANTIFICATION"),
        buffer=max(0, power_amount(player, "BUFFER")),
        # (The amount grows as the Infested Prism buffs itself: at Vital Spark 4, four Skills
        # turned a 5x3 attack into 13x3 while the bot counted 2 a Skill, and it died behind 17 Block.)
        taint=max((power_amount(e, "VITAL_SPARK") for e in [player, *(battle.get("enemies") or [])]), default=0),
    )


def _bare_block(player: dict) -> int:
    """Block a relic gives when the turn ends without any (Orichalcum)."""
    for r in player.get("relics") or []:
        m = re.search(r"end your turn without block, gain (\d+) block", str(r.get("description") or ""), re.I)
        if m:
            return int(m.group(1))
    return 0


def _draw_worth(player: dict) -> tuple:
    """What drawing a card is worth right now, from what is left to draw.

    A flat 2 points a card made "Draw 6 cards" for 2 energy look worse than
    two Strikes whatever the deck held, and made drawing into a pile of
    Wounds look as good as drawing into a pile of free attacks.
    """
    pile = list(player.get("draw_pile") or [])
    if len(pile) < 3:
        pile += list(player.get("discard_pile") or [])  # it is about to be shuffled back in
    if len(pile) < 3:
        return (2.0, 0.0, 0.0)
    free = 0.0
    paid: list[float] = []
    for c in pile:
        text = str(c.get("description") or "")
        if re.search(r"\bunplayable\b", text, re.I):
            continue
        info = parse(c)
        worth = 1.2 * info.total_damage * (1.4 if info.aoe else 1.0) + info.block + 2.0 * (info.vulnerable > 0) + 1.5 * info.draw
        if not info.known:
            worth = 3.0
        if info.cost == 0 and info.star_cost <= 0:
            free += worth
        else:
            paid.append(worth / max(1, 2 if info.is_x else info.cost))
    return (free / len(pile), len(paid) / len(pile), sum(paid) / len(paid) if paid else 0.0)


def _next_wither(player: dict, round_no: int = 1) -> int:
    """Damage the next Wither does if it is still in the hand when the turn ends.

    A plain Wither does 3. Every third round Aeonglass makes all of them one
    step worse, the ones in our deck and the ones still to come, and a step is
    3 more damage: "Wither+2" does 9, not 5. (Measured from HP lost with
    Withers in hand; a bot on 35 HP that expected to lose 31 died of it.)
    """
    level = max(0, (round_no - 1) // 3)
    for pile in ("hand", "draw_pile", "discard_pile", "exhaust_pile"):
        for c in player.get(pile) or []:
            m = re.match(r"Wither(?:\+(\d+))?$", str(c.get("name") or ""))
            if m:
                level = max(level, int(m.group(1) or 0))
    return 3 * (level + 1)


def _sandpit(state: dict) -> int | None:
    """Turns until the Sandpit swallows us (None when there is none)."""
    creatures = [state.get("player") or {}, *((state.get("battle") or {}).get("enemies") or [])]
    for c in creatures:
        for p in c.get("status") or []:
            if str(p.get("id", "")).upper() in ("SANDPIT", "SANDPIT_POWER"):
                amount = p.get("amount")
                return int(amount) if isinstance(amount, (int, float)) else 1
    return None


def _escape_odds(player: dict, hand_draw: int = 5) -> tuple:
    """Chance that the next hand holds no Frantic Escape, and the same for a hand a few turns on.

    The first from where the Escapes are right now (the draw pile, or what
    will be shuffled back into it); the second from the deck as a whole.
    """
    def is_escape(c: dict) -> bool:
        return str(c.get("name") or "").startswith("Frantic Escape")

    def none(n: int, k: int, d: int) -> float:
        """No Escape among d cards drawn from n that hold k of them."""
        if n <= 0 or d <= 0:
            return 1.0
        d = min(d, n)
        p = 1.0
        for i in range(d):
            p *= max(0.0, (n - k - i) / (n - i))
        return p

    draw = player.get("draw_pile")
    if not isinstance(draw, list):
        return (0.3, 0.3)
    later = list(player.get("discard_pile") or []) + list(player.get("hand") or [])
    n, k = len(draw), sum(map(is_escape, draw))
    m, j = len(later), sum(map(is_escape, later))
    if n >= hand_draw:
        next_hand = none(n, k, hand_draw)
    else:  # the pile runs out: the rest of the hand comes from the reshuffled discards
        next_hand = none(n, k, n) * none(m, j, hand_draw - n)
    usual = none(n + m, k + j, hand_draw) if n + m >= hand_draw else 0.3
    return (next_hand, max(usual, 0.05))


def cards_left_this_turn(state: dict) -> int | None:
    """How many more cards we may play this turn, when something limits it."""
    player = state.get("player") or {}
    played = int(state.get("_plays_this_turn") or 0)
    limits = []
    if _has_power(player, "RINGING"):  # "You can only play 1 card this turn."
        limits.append(1)
    if _has_power(player, "SLOTH"):  # "You cannot play more than 3 cards each turn."
        limits.append(3)
    for c in player.get("hand") or []:
        m = re.search(r"cannot play more than (\d+) cards", str(c.get("description") or ""), re.I)
        if m:
            limits.append(int(m.group(1)))
    return max(0, min(limits) - played) if limits else None


def choose_card(state: dict) -> Action | None:
    """Best card to play right now, or None to end the turn."""
    player = state.get("player") or {}
    enemies = read_enemies(state)
    ctx = _context(state)
    hand = [parse(c) for c in player.get("hand") or []]
    playable = [c for c in hand if c.playable and (c.is_x or c.cost <= ctx["energy"]) and c.star_cost <= ctx["stars"]]
    if not enemies and state.get("_revive_pending"):
        return _set_up(playable, ctx["energy"])
    if not playable or not enemies:
        return None
    # Cards we cannot pay for yet can still come later in a line that first
    # makes the energy or Stars for them. Left out, the Regent held Falling
    # Star all fight next to the Venerate that pays for it, and played Strikes.
    makers = any(c.star_gain or c.energy_gain for c in playable)
    if makers:
        playable += [c for c in hand if c.short and c not in playable]
    if len(playable) > MAX_SEARCH_CARDS:
        playable = sorted(playable, key=lambda c: -(c.total_damage + c.block + 3 * c.draw))[:MAX_SEARCH_CARDS]
    if ctx["card_limit"] is not None:
        if ctx["card_limit"] <= 0:
            return None
        # Few plays allowed: spend them on what does the most right now.
        playable.sort(key=lambda c: (-(c.total_damage + c.block), c.index))
    elif any(e.slow >= 0 for e in enemies):
        # Slow: every card played makes later attacks hit harder this turn,
        # so everything else goes first and the biggest attack goes last.
        playable.sort(key=lambda c: (1 if c.damage else 0, c.total_damage, c.index))
    elif any(e.slippery for e in enemies):
        # Slippery turns each hit into 1 damage: burn it off with the weakest
        # hits and keep the big ones for afterwards.
        playable.sort(key=lambda c: (_order_key(c)[0] if not c.damage else 3, c.damage, c.index))
    else:
        playable.sort(key=_order_key)

    # Bound cards: one per turn. Once one has been played the game marks the
    # rest unplayable, so any still playable means the allowance is unused.
    bound_left = 1 if any(c.bound for c in playable) else None
    baseline = simulate([], enemies, **ctx)
    best_score = baseline.score if baseline else float("-inf")
    best_line: list[CardInfo] = []
    best_target: str | None = None
    # When surrounded, who we aim at decides who hits us from behind, so each
    # line is also tried aimed entirely at one enemy.
    # The same goes for any small group: the default target rule looks at one
    # card at a time, so it never saw that two cards together kill the enemy
    # about to hit us (it died at 18 HP next to a 21 HP Parafright it ignored).
    aims: list[str | None] = [None] + [e.entity_id for e in enemies if e.flanker or (1 < len(enemies) <= 3 and e.incoming > 0)]
    # The order within a line matters too (which attack gets the Slow bonus,
    # what is played before an X card takes the rest of the energy): once the
    # bot left an elite on 1 HP because the bigger attack went second. Short
    # lines are tried in every order, the usual one first so that it wins ties.
    budget = 2500
    for size in range(1, len(playable) + 1):
        found = False
        for line in combinations(playable, size):  # keeps _order_key order
            if bound_left is not None and sum(c.bound for c in line) > bound_left:
                continue  # "Only 1 Bound card can be played each turn."
            orders = [line]
            if 1 < size <= 4:
                orders = permutations(line)  # starts with `line` itself
            for nth, order in enumerate(orders):
                if nth and budget <= 0:
                    break
                usable = True
                for aim in aims:
                    if nth:
                        budget -= 1
                    out = simulate(list(order), enemies, **ctx, focus=aim)
                    if out is None:
                        usable = False
                        break
                    found = True
                    if out.score > best_score + 1e-9:
                        best_score, best_line, best_target = out.score, list(order), out.first_target
                if not usable and not nth and not any(c.is_x or c.energy_gain or c.star_gain for c in line):
                    break  # cannot be paid for in any order
        if not found:
            break
    if not best_line:
        return None

    card = best_line[0]
    params: dict = {"card_index": card.index}
    if card.needs_target:
        params["target"] = best_target or min(enemies, key=lambda e: e.hp + e.block).entity_id
    return ("play_card", params)


def _set_up(playable: list[CardInfo], energy: int) -> Action | None:
    """A card worth playing with nobody to aim at: the enemy is gone for the turn and comes back.

    Between two of the Test Subject's forms the bot used to end the turn with
    energy in hand and Powers unplayed (Child of the Stars, Prowess, Loop).
    Whatever lasts is worth playing now: Powers, orbs, Strength, next turn's
    energy; and cards drawn while there is energy to use what they bring.
    """
    best, best_worth = None, 0.0
    for c in playable:
        if c.needs_target or c.type in ("Status", "Curse") or c.deadly or c.block_lock or c.is_x:
            continue
        worth = POWER_VALUE.get(c.key, 5.0 + 2.0 * max(c.cost, 0)) if c.type == "Power" else 0.0
        worth += 4.0 * len(c.channel) + 0.5 * c.forge + c.summon + 2.5 * c.star_gain
        worth += 4.0 * c.strength * c.strength_lasts + 3.5 * c.dexterity * c.dexterity_lasts
        worth += 5.0 * c.next_energy + 1.5 * c.next_draw + 0.8 * c.next_block
        if energy - c.cost > 0 or c.energy_gain:
            worth += 1.0 * c.draw + 2.0 * c.energy_gain
        worth -= 3.0 * c.hp_loss
        worth /= c.cost + 0.5
        if worth > best_worth:
            best, best_worth = c, worth
    return ("play_card", {"card_index": best.index}) if best is not None else None


def actions(state: dict) -> list[Action]:
    """Ordered candidate actions for a combat state (first workable one is taken)."""
    if log.isEnabledFor(logging.DEBUG):
        log.debug("combat %s", describe(state))
    if log.isEnabledFor(logging.DEBUG) and _sandpit(state) is not None:
        log.debug("sandpit %s, no Escape next hand %.2f, later %.2f", _sandpit(state), *_escape_odds(state.get("player") or {}))
    out = potion_actions(state)
    card = choose_card(state)
    if card is not None:
        out.append(card)
    out.append(("end_turn", {}))
    return out
