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
    BEYOND_REACH, LETHAL_PENALTY, ORB_VALUES, POWER_VALUE, RAGE_BLOCK, REATTACH_REACH, SLIP_WORTH, WIN_BONUS, Enemy,
    _has_power, _is_attack, constrict, end_of_turn_block, end_of_turn_loss, expected_loss, hand_burns,
    intent_damage, intent_hits, kill_reach, osty_hp, power_amount, read_enemies, _bare_block, _draw_worth, _escape_odds, _next_wither, _sandpit, _wither_bill,
    deck_count, junk_cost, cards_left_this_turn, free_plays, _hit, _hit_count, _poison_total, _power_worth, _set_up, _rage_blocks, _respawns, loss_cap,
)
from .cards import CardInfo, order_key, parse
from .potions import fortifier, describe, potion_actions  # noqa: F401
from . import sandpit as pit
from . import artifact, triggers
from .learn import knob

Action = tuple[str, dict]

log = logging.getLogger("sts2bot.combat")

MAX_SEARCH_CARDS = 11
THREAT_FLOOR = 6.0  # damage a turn every enemy is taken to do at least, when choosing whom to hit first
HIVE_COST = 9.0  # Personal Hive: what a Dazed in our draw pile costs once the hand has no card to spare, in points
HIVE_PACE = 22.0  # ... and the damage a round the Entomancer is taken to lose, for how long the fight still is


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
        # A bolt at random can finish the wrong crab: Crusher on 3 HP, Rocket on 20 about to hit
        # for 52, and a Lightning pushed out by Coolheaded killed the Crusher. The Rocket took its
        # 99 Block, and the two attacks in hand that would have killed it first did nothing.
        risk = sum(4.0 * o.hp + 10.0 for e in alive if e.rage and e.hp + e.block <= amount for o in alive if o is not e and o.rage)
        return amount * 0.8 * dmg_value - risk / len(alive)
    return sum(_progress(e, _hit(e, amount, 1, raw=True), chip_rate) * dmg_value * e.worth for e in targets)


def _would_kill(e: Enemy, dmg_per_hit: int, hits: int, double_if_vulnerable: bool = False) -> bool:
    trial = replace(e)
    _hit(trial, dmg_per_hit, hits, double_if_vulnerable)
    return not trial.alive


def _pick_target(enemies: list[Enemy], dmg_per_hit: int, hits: int, double_if_vulnerable: bool = False) -> Enemy | None:
    alive = [e for e in enemies if e.alive]
    if not alive:
        return None

    # Reattach (the Decimillipede, 130 HP in three segments): one killed while another lives is back two
    # turns later on 25 HP, so killing them one by one never ends. A Regent went in on 85/85, killed a
    # segment seven times in twelve rounds and died with two of them on 25. Until the others are within
    # reach the healthiest one is hit and none is finished: they come down together and die together.
    segs = [e for e in alive if e.reattach]
    stay = {e.entity_id for e in segs if sum(o.hp + o.block for o in segs if o is not e) > REATTACH_REACH} if len(segs) > 1 else set()
    if stay:
        spare = [e for e in segs if not (e.entity_id in stay and _would_kill(e, dmg_per_hit, hits, double_if_vulnerable))]
        if len(segs) == len(alive):
            return max(spare or segs, key=lambda e: e.hp + e.block)
    killable = [e for e in alive if e.entity_id not in stay and _would_kill(e, dmg_per_hit, hits, double_if_vulnerable)]
    if killable:
        # Remove the biggest threat first; waste the least damage on ties.
        # Killing something that revives only pays off if it is about to hit us.
        useful = [e for e in killable if e.worth > 0 or e.incoming > 0]
        # (Not a minion that is not even attacking, with an attack more than twice its size: the
        # leader's death takes it along. Squeeze, 35 damage, went into a 17-HP egg three times while
        # the Ovicopter that lays them stood at 105, and that fight was lost from 69 HP.)
        size = dmg_per_hit * hits
        useful = [e for e in useful if not (e.minion and not e.main_threat and not e.incoming and size > 2 * (e.hp + e.block) and any(o.worth >= 1.0 for o in alive))]
        if useful:
            return max(useful, key=lambda e: (e.worth > 0, e.incoming, e.hp + e.block))
    # Otherwise focus whoever dies soonest, counting Vulnerable as less health;
    # leaders before their minions, never something that just comes back.
    # (Not one whose Block would swallow the whole attack, when another would lose HP to it.)
    # ... and, where the enemies are known to differ, the one that does the most damage for
    # its health: least is lost in all by killing in that order. Lowest health first left the
    # Slumbering Beetle (86 HP, 16 a turn and 2 more every turn once awake) until both Bowlbugs
    # were dead: 5 runs in 24 died in that room, two of them from over 50 HP. Enemies that have
    # shown under 6 a turn count as 6, so equals are still taken lowest first.
    # Doom is health already gone (it dies at the end of its turn once Doom covers its HP), and one
    # that Doom has finished needs nothing more. Read as plain HP, the Necrobinder put Scourge on one
    # Nibbit and its attacks on the other, then went on hitting the one already done for: seven rounds
    # and 34 of 49 HP for two 42-HP enemies. (15 of its 44 act 1 bosses were met on under 75% HP,
    # 7 of those won; of the 29 met on more, 24.)
    soaks = lambda e: e.block >= dmg_per_hit * hits and not e.keeps_block
    left = lambda e: (max(1, e.hp - e.doom) + e.block) / (1.5 if e.vulnerable else 1.0)
    return min(alive, key=lambda e: (_fated(e), -e.worth, soaks(e), left(e) / max(e.threat, THREAT_FLOOR), -e.incoming))


def _fated(e: Enemy) -> bool:
    """Doom covers its HP: it dies when its turn ends, whatever else we do to it (a boss with another form to come does not)."""
    return e.doom >= e.hp > 0 and not e.adapts


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


def _debuff(e: Enemy, c: CardInfo, x: int = 1) -> None:
    """Everything a card puts on one enemy. (Cards that sap ALL enemies used to do nothing in the plan:
    Piercing Wail was played twice in 456 hands, and a Silent died behind a 60-damage attack it would have cut to 24.)"""
    if c.vulnerable:
        _expose(e, c.vulnerable * x)
    if c.weak:
        _weaken(e, c.weak * x)
    if c.enemy_strength_loss and not _resists(e):  # the enemy loses Strength for this turn: every hit is that much weaker
        e.incoming = max(0, e.incoming - c.enemy_strength_loss * x * e.incoming_hits)
    if c.key == "DEBILITATE" and e.vuln_mult < 2.0 and not _resists(e):
        # "Vulnerable and Weak are twice as effective": for the attacks that follow it this turn too.
        e.incoming = int(e.incoming * 0.5 / e.weak_cut) if e.weak else e.incoming
        e.vuln_mult, e.weak_cut = 2.0, 0.5


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


def _progress(e: Enemy, hp_removed: int, chip_rate: float) -> float:
    """How far an attack moved the fight on: HP removed, plus Block worn down.

    Block an enemy keeps between turns is as good as HP. Only counting HP,
    the bot never attacked a Burrowed enemy (20 Block that stays, 24 damage
    a turn) and blocked every turn instead, for 180 rounds.
    """
    chip, e.chipped = e.chipped, 0
    # A Slippery stack taken off is a later hit that lands in full. Counted as the 1 HP it removes,
    # eight stacks were not worth a Defend: Vantom fights that still had Slippery on round 5 were
    # lost 4 times in 8 (2 in 19 when it was gone by round 4; one bot took until round 8).
    strip, e.stripped = e.stripped, 0
    hp_removed += strip * min(SLIP_WORTH, e.hp / 3.0)
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
    loss_cap: int = 0,  # Beating Remnant: the most HP a turn can cost us (0: no limit)
    taint: int = 0,  # Vital Spark: each Skill played makes every hit on us this turn this much harder
    draw_worth: tuple = (2.0, 0.0, 0.0),  # of a drawn card: (points if free, share that cost energy, points per energy)
    bare_block: int = 0,  # Orichalcum: Block gained if the turn ends with none
    regen: int = 0,  # Regen: HP healed when the turn ends, before the enemy moves
    focus_power: int = 0,  # Focus: added to what a newly channeled orb does
    escape_odds: tuple = (0.3, 0.3),  # chance of no Frantic Escape in the next hand, and in a hand later on
    wither_in: int = 0,  # Withering Presence: cards we can still play before a Wither lands in our hand
    wither_hp: int = 5,  # ... and what that Wither will cost (each one is a point worse than the last)
    wither_bill: float = 7.0,  # ... and what each card played costs in Withers to come (battle._wither_bill)
    trig: triggers.Triggers | None = None,  # powers that act on every card played
    pet: int = -1,  # Osty's HP (already part of `block`); -1: no pet in the state
    hail: int = 0,  # Hailstorm: damage to ALL enemies when the turn ends with a Frost orb held
    deck: tuple = (20, 0),  # cards in the hand and both piles, and how many of them are dead weight
    mastery: int = 0,  # Necro Mastery: whenever Osty loses HP, ALL enemies lose that much HP as well (times its amount)
    might0: int = 0,  # Strength that is on its way and not in the card texts yet (Toasty Mittens, once its Exhaust is chosen)
    holds: bool = False,  # Barricade: Block is not removed when our next turn starts
    strip: tuple = (0.0, 0.0),  # Artifact: (what a charge taken off an enemy is worth, debuffs the deck will still apply), artifact.outlook
    friends: int = 0,  # Friendships already in play (each is an energy a turn, and the next one is worth less)
    free: int = 0,  # Void Form: this many of the next cards cost no energy
    doubled: bool = False,  # Duplication (the Duplicator potion): the next card is played an extra time, whatever it is
    fortify: int = 1,  # a Fortifier waits on the belt for this turn's Block ("Triple your Block": 3)
) -> Outcome | None:
    """Score playing `line` in order. None if it cannot be afforded."""
    if card_limit is not None and len(line) > card_limit:
        return None
    if any(c.ends_turn for c in line[:-1]):
        return None  # "End your turn": nothing is played after it
    foes = [replace(e, chipped=0, stripped=0, vuln_added=0, weak_added=0) for e in enemies]
    score = 0.0
    first_target: str | None = None
    self_loss = 0
    # Long fights are races: bosses and elites grow stronger every turn, so a
    # turn spent only blocking makes the next one worse. The bot used to sit
    # behind Defends for 19 rounds against a boss gaining Strength and lose.
    dmg_value = {"boss": 1.5, "elite": 1.3}.get(fight, 1.0) + min(0.4, 0.08 * max(0, round_no - 5))
    # An enemy that gains Strength every turn is a race whatever the fight: a turn of its life saved
    # is its biggest hit not taken. The Devoted Sculptor (Ritual 9: 12, 21, 30, 39, 48) cost 3 HP when
    # it was dead in three rounds, 13 in four, 29 in five, and killed two of the three runs that took six.
    dmg_value *= 1.0 + min(1.5, 0.15 * max((e.grows for e in enemies if e.worth >= 1.0), default=0.0))
    # A fight that drags on is one the bot is not winning by waiting: lean
    # harder on attacking every round until something gives.
    dmg_value += 0.5 * max(0, round_no - 12)
    chip_rate = min(1.0, 0.25 + 0.05 * max(0, round_no - 10))  # worth of temporary Block removed
    kept_block = {e.entity_id for e in enemies if e.keeps_block}
    escapes = 0  # Frantic Escape cards played in this line
    osty_hits = 0  # Osty attacks played earlier in this line
    free_next: str | None = None  # the kind of card that costs nothing next, after one that says so
    surcharge = 0  # energy every further card costs on top of its own, after one that says so
    echo: str | None = "Any" if doubled else None  # the kind of card that is played an extra time next (Burst, One-Two Punch)
    dooming = False  # a card earlier in this line applied Doom
    free_all = False  # Bullet Time: everything left in the hand is free this turn
    healed = 0  # HP this line gives back
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
    # (Pricing what lasts the whole fight (Powers, Strength, Dexterity, Forge) by the turns a boss
    # or elite fight has left, 8 times an ordinary fight on round 1 instead of 3, was tried as a
    # setting from 2026-11-19 to 2026-11-20: 20 runs ended on floor 30.6 on average against 35.0
    # for 38 without, 75% past act 1 against 89%, and Powers were not played any oftener. Removed.)
    enduring = 0.0  # worth of what stays for the whole fight
    future = 0.0  # everything in the score that only pays off after this turn
    might = might0  # Strength gained from cards earlier in this line
    attacks = 0  # Attacks played so far in this line
    plays = 0  # times a card has been played in this line (one that is played again counts again)
    summoned = 0  # HP this line gives Osty
    pet0 = pet  # Osty's HP as the card texts in hand were written
    drawn = 0  # cards this line draws
    debris = 0  # Debris this line leaves in our deck
    dazed = 0  # Dazed this line's Attacks bring into it (Personal Hive)
    tainted = 0
    known_dead = {e.entity_id for e in foes if not e.alive}

    def evoke(orb: tuple, plasma: bool = True) -> None:
        nonlocal block, score, energy
        kind, _passive, burst = orb
        if kind == "FROST" and can_block:
            block += burst
        elif kind in ("LIGHTNING", "DARK", "GLASS"):
            score += _orb_damage(kind, burst, foes, chip_rate, dmg_value)
        elif kind == "PLASMA" and plasma:
            energy += 2

    for n, c in enumerate(line):
        _respawns(foes)
        _rage_blocks(foes, known_dead)  # a crab that died with the last card hands its ally Block
        before = triggers.marks(foes) if trig else []
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
            score += 8.0 * x * dmg_value * (c.key == "CASCADE")  # "Play the top X cards of your Draw Pile": a card is about 8
        else:
            # Flatten "costs 0 if Osty has attacked this turn", Rattle "hits an additional
            # time for each other time he has attacked": after a Poke in the same line too.
            cost = 0 if (c.key == "FLATTEN" and osty_hits) or c.type == free_next or free_all or n < free else c.cost + surcharge
            free_next = None if c.type == free_next else free_next
            if cost > energy:
                return None
            energy -= cost
        # Played again (its own Replay, or the card before said so): everything it does, it does again, and each
        # time is one of the cards a turn allows. (Sloth, three a turn: with Sword Sage up the Sovereign Blade took
        # the count from 0 to 2, the plan's third card could not be played and 41 HP went on 42, the Demon on 88.)
        echoed = bool(echo) and echo in ("Any", c.type)
        k, echo = 1 + c.replay + echoed, None if echoed else echo
        if card_limit is not None and plays >= card_limit:
            return None
        plays += k
        if k > 1:
            more = dict(block=c.block * k, draw=c.draw * k, summon=c.summon * k, doom=c.doom * k, poison=c.poison * k, shivs=c.shivs * k, souls=c.souls * k)
            c = replace(c, hits=c.hits * (k if c.damage else 1), energy_gain=c.energy_gain * k, **more)
        echo = triggers.ECHO_NEXT.get(c.key, echo)
        energy *= 2 if c.key == "DOUBLE_ENERGY" else 1
        free_all = free_all or c.key == "BULLET_TIME"
        energy += c.energy_gain
        free_next = triggers.FREE_NEXT.get(c.key, free_next)  # Unrelenting: "The next Attack you play costs 0"
        surcharge += triggers.SURCHARGE.get(c.key, 0)
        healed += c.heal
        self_loss += c.hp_loss
        if c.osty_attack and pet == 0:  # Osty is dead: his attacks do nothing (seen 12 times in 12)
            c = replace(c, damage=0, block=0 if c.osty_dies else c.block)
        elif c.osty_attack:
            c, osty_hits = (replace(c, hits=c.hits + osty_hits) if c.key == "RATTLE" else c), osty_hits + 1
            # Unleash adds Osty's HP to its damage and Protector his Max HP: what was Summoned earlier
            # in the line counts (the text only shows it afterwards). Unleash used to go before the
            # Bodyguard in the same hand, 5 damage short every time.
            grown = pet - pet0 if c.key == "UNLEASH" else summoned if c.key == "PROTECTOR" else 0
            c = replace(c, damage=max(0, c.damage + grown)) if grown else c
        if c.osty_dies and pet > 0:
            block, pet = block - min(block, pet), 0  # ... and the HP he stood in front of us with goes with him
        pet += c.summon * x if pet >= 0 else 0
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

        xs = x if c.is_x else 1
        alive = [e for e in foes if e.alive]
        t = focused = next((e for e in alive if e.entity_id == focus), None)  # the enemy this card is aimed at, if any
        if c.key in ("TIMES_UP", "TIME_S_UP") and alive:  # "Deal damage equal to the enemy's Doom": no number in its text (played 16 times, left 37)
            t = focused = focused or max(alive, key=lambda e: e.doom)
            c = replace(c, damage=t.doom, hits=1)
        if c.key in ("DEATHS_DOOR", "DEATH_S_DOOR") and dooming and can_block:  # "If you applied Doom this turn, gain Block 2 additional times"
            block += 2 * max(0, c.block + nimble)
        dooming = dooming or bool(c.doom)
        if c.damage:
            per, hits = c.damage + (0 if c.osty_attack else might), c.hits * x  # (Osty's attacks take no Strength: Unleash+ read 11 at Strength 10)
            if c.type == "Attack":
                if attacks:  # Vigor and Gigantification went with the first Attack
                    per = round(per / 3) if giant else per
                    per -= vigor
                attacks += 1
            per = max(0, per)
            if c.aoe:
                t, struck = None, alive
            elif c.random_target and len(alive) > 1:
                t, struck = None, []
                score += per * hits * 0.8 * dmg_value
            else:
                t = focused or _pick_target(foes, per, hits, c.double_if_vulnerable)
                if c.key == "HANG" and t is not None:
                    # "Double the damage ALL Hang cards deal to this enemy": 10, 20, 40, 80 on the same enemy, and
                    # the card's text goes on saying 10. (The third one did 40 of the 57 a lost fight's last turn dealt.)
                    per, t.hang = per * max(1, t.hang), 2 * max(1, t.hang)
                struck = [t] if t is not None else []
            # Several hits on ALL enemies land one round at a time: a crab that dies of the first hands the other
            # its 99 Block for the rest. (Exterminate+, 10 x 4, with the Rocket on 7 and the Crusher on 39 about
            # to hit for 24 on our 4 HP: counted as 40 on each it was the win, and it did 10. The three other
            # cards in the hand added up to the Crusher's 39.)
            waves = hits if c.aoe and hits > 1 and any(e.rage for e in struck) else 1
            for _ in range(waves):
                for e in struck:
                    if not e.alive:
                        continue
                    n_hits = hits // waves
                    self_loss += e.thorns * _hit_count(e, n_hits, c.double_if_vulnerable)
                    dazed += e.hive * _hit_count(e, n_hits, c.double_if_vulnerable) * (c.type == "Attack")
                    done = _fated(e)  # damage that does not kill it now changes nothing
                    gain = _progress(e, _hit(e, per, n_hits, c.double_if_vulnerable), chip_rate) * dmg_value * e.worth
                    score += gain * (0.2 if done and e.alive else 1.0)
                if waves > 1:
                    _rage_blocks(foes, known_dead)
            for e in struck:
                if e.alive:
                    _debuff(e, c, xs)
        elif c.needs_target:
            if focused is not None:
                pass
            elif c.enemy_strength_loss:
                t = max(alive, key=lambda e: min(e.incoming, c.enemy_strength_loss * e.incoming_hits), default=None)
            elif c.weak and not c.vulnerable:
                # Weaken the hardest hitter that is not Weak yet.
                t = max(alive, key=lambda e: (not e.weak, e.incoming), default=None)
            elif c.doom and not c.vulnerable:
                t = _pick_target(foes, 0, 0)  # Doom goes where the attacks go: together they end one enemy sooner
            else:
                # Vulnerable goes on the enemy we will be hitting the longest.
                fresh = [e for e in alive if not e.vulnerable] or alive
                t = max(fresh, key=lambda e: e.hp + e.block, default=None)
            if t is not None:
                _debuff(t, c, xs)
        else:
            t = None
            for e in alive if c.aoe else []:
                _debuff(e, c, xs)
        if t is not None:
            faced = t.entity_id
            if n == 0:
                first_target = t.entity_id

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
                    # (Doom beyond its HP is worth nothing, unless another form follows.)
                    score += (c.doom * x if e.adapts else min(c.doom * x, max(0, e.hp - e.doom))) * 0.8 * dmg_value * e.worth
                    e.doom += c.doom * x
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
        if trig or c.doom_kills:
            won, shield = triggers.fire(
                trig or triggers.NONE, c, foes, before, lambda e, d: _hit(e, d, 1, raw=True), lambda e, hp: _progress(e, hp, chip_rate) * dmg_value * e.worth
            )
            score, block = score + won, block + (shield if can_block else 0)
        # Stars are worth what they will be spent on: a Regent sat on 15 of them, still
        # playing Hidden Cache and Venerate for more while it lost 71 HP to three bugs.
        score += c.star_gain * (2.5 if stars < 4 else 1.0 if stars < 8 else 0.2) + c.souls * 2.5
        stars += c.star_gain
        enduring += c.forge * 0.5
        # (Energy next turn at a flat 5: with the Queen only buffing, Invoke+ (3 energy) lost to a 7-damage attack.)
        later += (c.next_energy * 6.0 + c.next_draw * 2.0) * dmg_value + c.next_block * 0.8
        if c.dexterity:
            nimble += c.dexterity
            if c.dexterity_lasts:
                enduring += c.dexterity * 3.5
        if c.channel or c.evoke or c.evoke_all:
            _rage_blocks(foes, known_dead)  # a crab this card killed has already handed on its Block
        for kind in c.channel * xs:
            if orb_slots and len(held) >= orb_slots:
                evoke(held.pop(0), plasma=False)  # no room: the oldest orb is evoked to make space
            passive, burst = ORB_VALUES.get(kind, (3, 6))
            if kind != "PLASMA":
                passive, burst = max(0, passive + focus_power), max(0, burst + focus_power)
            held.append((kind, passive, burst))
            lasting += {"FROST": 3.0, "LIGHTNING": 3.0, "DARK": 4.0, "PLASMA": 6.0, "GLASS": 4.0}.get(kind, 3.0)
        # "Evoke your rightmost Orb": the one channeled first, which the game
        # lists first. Taking it for the newest, the bot counted on a fresh
        # Frost's Block from Dualcast, got a Glass orb's damage instead and
        # died 3 HP short with the boss on 51.
        for _ in range(c.evoke * xs if held else 0):
            evoke(held[0])
        if c.evoke and held:
            lasting -= 6.0 * (held[0][0] == "PLASMA")  # its energy every turn goes with it
            held.pop(0)  # evoking uses the orb up
        if c.evoke_all:  # Shatter: every orb goes off, and all of them are used up
            for orb in held:
                for _ in range(c.evoke_all):
                    evoke(orb)
            lasting -= 6.0 * sum(o[0] == "PLASMA" for o in held)
            held = []
        if c.key == "FRANTIC_ESCAPE":
            escapes += 1
        if c.debris:  # (Crash Landing fills what room the hand has left: fewest when it is played first)
            debris += c.debris if c.debris > 0 else max(0, 10 - (hand_size - (n + 1) + drawn + debris))
        buffer += c.key == "BUFFER"  # played now, it stops this turn's hit (held at 6 HP in front of 27: dead)
        for e in foes:
            if e.slow >= 0:
                e.slow += 10  # for the cards after this one (measured: the card itself gets no bonus)
        if tender:
            might -= 1
            nimble -= 1
        if c.type == "Skill":
            tainted += taint
            # Enrage: every Skill makes each of its hits stronger for as long as
            # it lives. Against the Test Subject's first form (100 HP) the bot
            # played seven Skills and then took a 39-point hit: the price is the
            # extra damage on every turn the enemy still has in it.
            score -= sum(e.enrage * 2.0 * max(1, e.incoming_hits) * min(4.0, 1.0 + e.hp / 35.0) for e in foes if e.alive)
        drawn += c.draw * xs
        if c.strength:
            might += c.strength
            if c.strength_lasts:
                # Pays off on every attack for the rest of the fight (or costs on every one: Friendship, Shared Fate).
                future += c.strength * 4.0 * span
        if c.enemy_strength_lasts:
            # ... and Strength taken off an enemy for good comes off every hit it still has in it: most from
            # the ones that hit many times. (This turn's share is in `incoming` already.)
            future += sum(c.enemy_strength_loss * max(1, e.incoming_hits) * 2.0 * e.worth for e in (alive if c.aoe or t is None else [t]) if e.alive) * span
        if c.type == "Power":
            # (An Ethereal Power not played now is never played: Echo Form was left to vanish 12 times
            # in the 15 boss and elite turns that could pay for it.)
            # (Not in an ordinary fight: three energy on round 1 of four for a card doubled on the other three.)
            # (A second Friendship is energy the hand has no cards for, at the same 2 Strength: half, then a quarter.)
            again, friends = (0.5**friends, friends + 1) if c.key == "FRIENDSHIP" else (1.0, friends)
            future += 0.0 if c.strength_lasts and c.strength > 0 else again * _power_worth(c, fight != "monster") * span * (1.6 if c.ethereal and fight != "monster" else 1.0)
        elif c.type in ("Status", "Curse"):
            # Only worth clearing out with energy nothing else can use, unless playing it is what
            # gets it out of the deck (Debris, Slimed): then it is every later draw it would have been.
            score += 0.1 + (junk_cost(fight, round_no, (deck[0] - 1, max(0, deck[1] - 1))) * dmg_value if c.key in ("DEBRIS", "SLIMED") else 0.0)
        elif not c.known and c.key not in triggers.READ:
            score += 2.5 + max(c.cost, 0)

    if healed:  # HP given back is HP not lost, at the price HP has right now; and it is there for this turn's blow
        healed = min(healed, max(0, max_hp - hp))
        score += healed * (1.5 + 2.5 * (1.0 - hp / max(1, max_hp))) * knob("caution")
        hp += healed
    # What the cards drawn are likely to be worth: the free ones as they
    # come, the rest as far as the energy left over stretches.
    # (Only as many as may still be played: under Sloth, three cards a turn, the third went on a Soul,
    # "Draw 2 cards", five energy in hand and nothing more allowed.)
    drawn = drawn if card_limit is None else min(drawn, max(0, card_limit - plays))
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
    if hail and any(o[0] == "FROST" for o in held):
        score += sum(_progress(e, _hit(e, hail, 1, raw=True), chip_rate) * dmg_value * e.worth for e in foes if e.alive)
    future += (lasting + enduring) * span + later
    score += future
    if debris:
        score -= junk_cost(fight, round_no, deck, debris) * dmg_value
    if dazed:
        # A Dazed is one draw lost (it is Ethereal: drawn once and gone), so it costs a card of ours, as far
        # as the deck still has cards that are not dead: the Entomancer (1, then 2, then 3 a hit) is for few
        # big attacks and whatever is not an Attack. Hitting it four times a turn with small ones, a bot drew
        # five Dazed in a hand of six by round 7. (39% of Max HP, the dearest act 2 elite.)
        # ... but not for no attacks at all. At a full card for every Dazed, a 10-damage attack was never
        # worth two of them, and a Necrobinder that came in on 65/76 ended four turns with 4 to 7 energy
        # and its attacks in hand while the Entomancer grew from 4x7 to 7x7: dead in round 11 with it on 18.
        # The first two of a turn cost little (a hand of five has cards to spare for the energy there is),
        # the ones after that a card each; and only as far as the fight will last long enough to draw them.
        live = max(10, deck[0])
        left = sum(e.hp + e.block for e in foes if e.alive and e.hive)
        in_time = min(1.0, max(0.25, (left / HIVE_PACE) / (live / 5.0))) if left else 0.0
        score -= HIVE_COST * (0.35 * min(dazed, 2) + max(0, dazed - 2)) * in_time * max(0.3, 1.0 - deck[1] / live)
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
    # (Still undersold, by the logs of 2026-11-21: a round begun with the boss Vulnerable dealt 11 to 15 more
    # than the other rounds of the same fight, 17 points and up where this says 7; and Weak is a quarter off
    # 19 a round at an act 2 boss, some 12 points of our HP where this says 5. On trial as `debuff_worth`.)
    vuln_turn = {"boss": 7.0, "elite": 5.0}.get(fight, 3.0) * knob("debuff_worth")
    weak_turn = {"boss": 5.0, "elite": 3.5}.get(fight, 2.0) * knob("debuff_worth")
    for e in foes:
        if e.alive:
            # The first turn of Weak is already in `incoming`; Vulnerable used
            # this turn is already in the damage dealt.
            score += (max(0, e.vuln_added - 1) + 0.5 * min(1, e.vuln_added)) * vuln_turn * e.worth
            score += (max(0, e.weak_added - 1) + 0.3 * min(1, e.weak_added)) * weak_turn * (1.0 if e.incoming_hits or not e.minion else 0.5)
    score += sum(artifact.gain(orig.artifact, e.artifact, strip) * e.worth for e, orig in zip(foes, enemies) if e.alive and e.artifact < orig.artifact)
    # Doom finishes an enemy at the end of its turn once it covers its HP.
    score += 20.0 * sum(e.worth for e, orig in zip(foes, enemies) if e.alive and e.doom >= e.hp and orig.doom < orig.hp)
    _respawns(foes)  # a Stock enemy killed by the last card comes back too
    kills = [e for e, orig in zip(foes, enemies) if not e.alive and orig.alive]
    score += 25.0 * sum(e.worth for e in kills)
    score += 40.0 * sum(1 for e in foes if e.adapted)  # a form beaten: that much of the fight is done
    survivors = [e for e in foes if e.alive]
    # The fight is over once every leader is dead: minions leave with them.
    if not survivors or (all(e.minion for e in survivors) and not all(e.minion for e in enemies)):
        score += WIN_BONUS
        survivors = []

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
        # The Insatiable: Sandpit counts down every turn and at 0 we are eaten, whatever our HP; each
        # Frantic Escape played pushes it back by one. Priced by the rounds the fight still needs (sandpit.py).
        if sandpit + escapes <= 1:
            score -= LETHAL_PENALTY
        else:
            dealt = max((e.max_hp - e.hp for e in enemies if not e.minion), default=0)
            score -= pit.price(sandpit + escapes, max(e.hp for e in survivors), dealt, round_no, escape_odds)

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
    # (Osty stops attacks and nothing else: with him on 20 two Toxics still took 10, with him on 27 a
    # Wither+1 took 6, and each time the plan had counted on losing nothing.)
    guard = min(max(pet, 0), max(block, 0))
    own = max(0, block - guard) * fortify  # Block that is ours
    burnt += max(0, scorch - own)
    through = max(0, incoming - max(0, own - scorch) - guard)
    if through and buffer >= sum(e.incoming_hits for e in survivors if e.incoming):
        through = 0  # Buffer swallows each hit that would have hurt
    if through:
        # Only the hits that get through leave a scar: Block that stops four of
        # six hits saves four Wounds, though it saves no more HP than usual.
        for e in survivors:
            if e.incoming and e.scar:
                per = max(1, e.incoming // max(1, e.incoming_hits))
                score -= e.scar * min(e.incoming_hits, -(-through // per))
    if wither_in > 0:
        # ... and it stays in the deck: it burns again every time it is drawn
        # and takes a card's place in the hand (by round 7 one fight had five
        # of them in an eight-card hand, 25 HP a turn). The count carries over
        # between turns, so every card played is a sixth of that bill.
        score -= len(line) * wither_bill
    lost = through + self_loss + end_loss + burnt
    lost = max(0, lost - min(regen, max(0, max_hp - hp + self_loss)))  # Regen mends what was missing first
    lost = min(lost, loss_cap) if loss_cap else lost
    ratio = hp / max(1, max_hp)
    # Imbalanced (Bowlbug Rock): an attack of his that our Block stops whole leaves him Stunned for
    # his next turn. He moves first; with 15 Block or more up he was Stunned 58 times in 67, with
    # less never in 87, and the bot had that Block up on four turns in ten. (Osty's HP is left
    # out: whether a hit he takes counts is not known.)
    cover = max(0, block - max(pet, 0) - scorch)
    for e in survivors:
        hit = e.incoming + tainted * e.incoming_hits
        if e.imbalanced and hit and cover >= hit:
            score += 0.85 * hit * (1.5 + 2.5 * (1.0 - ratio)) * knob("caution")
        cover -= min(cover, hit)
    # Osty keeps the HP he is not hit for: unlike Block it is still there next turn (and Unleash
    # hits harder for it). The bot never summoned on a turn without an attack coming, which is
    # exactly when it costs nothing. And the HP he already has is ours one turn removed: what he
    # soaks now he cannot soak later. Counted as free Block, he lost 14 HP a boss fight and was
    # never above 9 on average; of 934 turns that ended with an Osty of 6 or more in the way of
    # an attack, 140 had a Block card and the energy for it left in hand.
    if pet > 0:
        kept = min(pet, max(0, max(0, own - scorch) + guard - incoming))
        # (Less than our own HP: the fight may end before he is needed, sooner in an ordinary one.)
        score += kept * (1.5 + 2.5 * (1.0 - ratio)) * knob("caution") * {"boss": 0.85, "elite": 0.7}.get(fight, 0.5)
        if mastery:  # ... and under Necro Mastery what he is hit for, every enemy is hit for as well
            score += (pet - kept) * mastery * dmg_value * sum(e.worth for e in survivors)
    if holds:  # Block that outlasts the turn is next turn's Block (as Osty's HP is, and a little surer)
        score += max(0, block - max(pet, 0) - scorch - incoming) * (1.5 + 2.5 * (1.0 - ratio)) * knob("caution") * {"boss": 0.8, "elite": 0.7}.get(fight, 0.5)
    # Every leader left is Doomed to die at the end of its turn: the fight is
    # won if we live through that turn, so more damage is wasted energy.
    leaders = [e for e in survivors if not e.minion]
    doomed = bool(leaders) and all(e.doom >= e.hp for e in leaders) and not any(e.adapts or e.spent for e in survivors)
    if doomed:
        score += WIN_BONUS / 2
    # A point of our HP is worth more than a point of enemy HP even at full
    # health: at 1.0 a Strike (6) beat a Defend (5) and the bot never blocked
    # until it was already hurt, bleeding HP in every ordinary fight.
    # ... and worth more still when this one turn would take a big bite out
    # of what is left: at 37 HP facing 27 the bot attacked with two Defends
    # in hand and was dead a turn later.
    bite = min(1.0, lost / max(1, hp))
    worth = 1.5 + 2.5 * (1.0 - ratio) + 3.0 * bite
    # (Pricing HP in boss fights as a race, cheap whatever is left of it, was
    # tried as a setting on 2026-11-18: 0 of 13 act 3 bosses against 5 of 20,
    # no better in acts 1 and 2. Removed.)
    # (Pricing HP 1.4 times higher in the last five rooms before a boss was tried as a setting
    # until 2026-11-19: 32 runs, no win, against 5 wins in 54; no further either. Removed.)
    price = lost * worth * (4.0 if doomed else 1.0) * knob("caution")
    score -= price
    if len(line) < wither_in:
        # The count carries over, so a line that stops short of the next Wither has only put it off: it
        # lands with the first card of the next turn. Charged to the card that crosses and to nothing
        # else, the bot stopped one card short turn after turn (three energy, a Block card and a Power
        # left in hand under a 25, then the Wither on the next turn's first card all the same).
        score -= wither_hp * (1.5 + 2.5 * (1.0 - ratio)) * knob("caution")
    if lost >= hp:
        # Dead on paper, and that is dead: of 115 turns ended this way none was lived
        # through (the loss expected is exact: 3 under-estimates of 3 HP or more in 7,880
        # turns, 111 over-estimates). So Block that still leaves us short buys next to
        # nothing, and the HP is not priced: what counts is cards drawn while there is
        # energy to play them (one may be the Block that is missing) and how near the
        # line comes to a kill that stops the blow, since our damage is the number the
        # planner gets wrong: a Silent blocked 17 of 35 and died 3 short with the Queen
        # on 25, where the Shivs and Serpent Form it did not count would have killed her.
        short = lost - hp
        score += price - max(0.0, future)  # (nor is there a later)
        score -= LETHAL_PENALTY - (150.0 if energy > 0 else 30.0) * min(drawn, 3)
        score += 400.0 * max(0.0, 1.0 - short / 12.0) + 3000.0 * max(0.0, 1.0 - kill_reach(survivors, lost - through < hp) / 30.0)
    return Outcome(score, first_target, int(lost))


def expected_end_loss(state: dict) -> int | None:
    """HP the planner expects to lose if the turn ended now (logged, to catch what it does not model)."""
    enemies = read_enemies(state)
    if not enemies:
        return None
    return simulate([], enemies, **_context(state)).lost  # (an empty line can always be afforded)


def _context(state: dict) -> dict:
    player = state.get("player") or {}
    battle = state.get("battle") or {}
    return dict(
        energy=int(player.get("energy") or 0),
        stars=int(player.get("stars") or 0),
        # Doom on us kills at the end of our turn once it reaches our HP: only the HP above it counts.
        hp=max(1, int(player.get("hp") or 1) - max(0, power_amount(player, "DOOM")) - max(0, power_amount(player, "NEUROSURGE"))),
        max_hp=int(player.get("max_hp") or 1),
        block=int(player.get("block") or 0) + end_of_turn_block(player) + osty_hp(player),
        fortify=int(state.get("_fortify") or 1),
        fight=str(state.get("state_type") or "monster"),
        round_no=int(battle.get("round") or 1),
        card_limit=cards_left_this_turn(state),
        can_block=not _has_power(player, "NO_BLOCK"),
        sandpit=_sandpit(state),
        escape_odds=_escape_odds(player),
        # Disintegration is damage, and Block stops it (seen live: 8 a turn, 0 lost behind 25 Block).
        # So is Constrict ("take 3 damage" at the end of our turn while the Strangler lives): 8 Block and
        # no attack lost nothing, 13 Block in front of 12 lost 2. Counted as HP lost whatever we did, it
        # left three Defends and the energy for them in the hand on turns it took 6.
        burns=hand_burns(player) + tuple((-1 - n, hp, True) for n, hp in enumerate((end_of_turn_loss(player), constrict(player))) if hp),
        hand_size=len(player.get("hand") or []),
        wither_in=max((power_amount(e, "WITHERING_PRESENCE") for e in battle.get("enemies") or []), default=0),
        wither_hp=_next_wither(player, int(battle.get("round") or 1)),
        wither_bill=_wither_bill(player, int(battle.get("round") or 1), _next_wither(player, int(battle.get("round") or 1))),
        draw_worth=_draw_worth(player),
        orbs=tuple(
            (str(o.get("id") or o.get("name") or "").upper().replace("_ORB", ""), int(o.get("passive_val") or 0), int(o.get("evoke_val") or 0))
            for o in player.get("orbs") or []
        ),
        orb_slots=int(player.get("orb_slots") or 0),
        bare_block=_bare_block(player),
        regen=max(0, power_amount(player, "REGEN")),
        focus_power=power_amount(player, "FOCUS") if _has_power(player, "FOCUS") else 0,
        shiv_damage=4 + max(0, power_amount(player, "ACCURACY")),
        tender=_has_power(player, "TENDER"),
        hail=max(0, power_amount(player, "HAILSTORM")),
        deck=deck_count(player),
        strip=artifact.outlook(player, str(state.get("state_type") or "monster"), int(battle.get("round") or 1)) if any(_has_power(e, "ARTIFACT") for e in battle.get("enemies") or []) else (0.0, 0.0),
        vigor=max(0, power_amount(player, "VIGOR")),
        giant=_has_power(player, "GIGANTIFICATION"),
        buffer=max(0, power_amount(player, "BUFFER")),
        loss_cap=loss_cap(player),
        # (The amount grows as the Infested Prism buffs itself: at Vital Spark 4, four Skills
        # turned a 5x3 attack into 13x3 while the bot counted 2 a Skill, and it died behind 17 Block.)
        trig=triggers.read(state),
        pet=osty_hp(player) if player.get("pets") else -1,
        mastery=max(0, power_amount(player, "NECRO_MASTERY")),  # (two of them: "2 times that much HP")
        might0=int(state.get("_might") or 0),
        holds=_has_power(player, "BARRICADE"),
        friends=max(0, power_amount(player, "FRIENDSHIP")),
        free=free_plays(state),
        # (Echo Form: "The first card you play each turn is played an extra time", and a second one the second card.)
        doubled=_has_power(player, "DUPLICATION") or max(0, power_amount(player, "ECHO_FORM")) > int(state.get("_plays_this_turn") or 0),
        taint=max((power_amount(e, "VITAL_SPARK") for e in [player, *(battle.get("enemies") or [])]), default=0),
    )


def choose_card(state: dict, plan: dict | None = None) -> Action | None:
    """Best card to play right now, or None to end the turn (`plan["lost"]`: HP the best line still loses)."""
    player = state.get("player") or {}
    enemies = read_enemies(state)
    ctx = _context(state)
    hand = [parse(c) for c in player.get("hand") or []]
    free = ctx["free"]
    if free:
        # Void Form: with free plays left every card shows as costing 0. What each costs after them is what the
        # deck said when the fight began, and the free plays are for the dearest. (Read as shown, two Strikes
        # went first and a 3-cost Knockout Blow stayed in the hand.)
        hand = [replace(c, cost=(state.get("_costs") or {}).get(c.name, c.cost)) for c in hand]
    playable = [c for c in hand if c.playable and (c.is_x or free or c.cost <= ctx["energy"]) and c.star_cost <= ctx["stars"]]
    playable = [c for c in playable if not triggers.shunned(c, state)]
    if not enemies and state.get("_revive_pending"):
        return _set_up(playable, ctx["energy"])
    if len(enemies) == 1 and enemies[0].asleep >= 2 and ctx["round_no"] <= 2:
        # Asleep ("Awakens upon losing HP or after 3 turns"): she wakes on her own time whatever
        # we do, so the turns before the last one are free for everything that lasts, and the
        # attacks wait. The bot used to hit the Lagavulin Matriarch for 3 to 30 through her 12
        # Block on round 1 and face her 19 a turn from round 2, Powers still in the deck.
        return _set_up(playable, ctx["energy"])
    if not playable or not enemies:
        return None
    # Cards we cannot pay for yet can still come later in a line that first
    # makes the energy or Stars for them. Left out, the Regent held Falling
    # Star all fight next to the Venerate that pays for it, and played Strikes.
    # Evoking a Plasma orb is energy too: a Defect on 1 energy held Dualcast next to
    # a Plasma (4 energy) and an Exterminate it could not pay for.
    orbs = ctx["orbs"]
    pays = lambda c: bool(c.energy_gain or c.star_gain or c.key in triggers.PAYS or (c.evoke and orbs[:1] and orbs[0][0] == "PLASMA") or (c.evoke_all and any(o[0] == "PLASMA" for o in orbs)))
    makers = any(pays(c) or c.key in triggers.FREE_NEXT for c in playable)
    if makers:
        playable += [c for c in hand if c.short and c not in playable]
    if len(playable) > MAX_SEARCH_CARDS:
        playable = sorted(playable, key=lambda c: -(c.total_damage + c.block + 3 * c.draw))[:MAX_SEARCH_CARDS]
    if ctx["card_limit"] is not None and ctx["card_limit"] <= 0:
        return None
    if ctx["card_limit"] is not None and ctx["card_limit"] <= 3:
        # Few plays allowed: spend them on what does the most right now.
        playable.sort(key=lambda c: (-(c.total_damage + c.block), c.index))
    elif any(e.slow >= 0 for e in enemies):
        # Slow: every card played makes later attacks hit harder this turn,
        # so everything else goes first and the biggest attack goes last.
        playable.sort(key=lambda c: (1 if c.damage else 0, c.total_damage, c.index))
    elif any(e.slippery for e in enemies):
        # Slippery turns each hit into 1 damage: burn it off with the weakest
        # hits and keep the big ones for afterwards.
        playable.sort(key=lambda c: (order_key(c)[0] if not c.damage else 3, c.damage, c.index))
    else:
        # (Flatten and Rattle after Osty's other attacks: one is free then, the other hits again.)
        after = lambda c: (order_key(c)[0], 1e3, c.index) if c.key in ("FLATTEN", "RATTLE") else order_key(c)
        playable.sort(key=lambda c: ((1,) + order_key(c)[1:]) if pays(c) else after(c))
    # (Void Form itself says "End your turn": played first, as a Power, it threw away two potions and
    # 62 damage in hand on round 1 of the Queen, and the turn's Block with them. It goes last.)
    dear = sorted((c for c in playable if c.cost > 0), key=lambda c: -c.cost)[:free]
    if ctx["doubled"] and playable and not any(c.type == "Power" for c in playable):
        dear.append(max(playable, key=lambda c: c.total_damage + c.block + 3 * c.draw))  # the card worth playing twice leads
    playable.sort(key=lambda c: (c.ends_turn, c not in dear))  # (stable: the rest keep their order)

    # Bound cards: one per turn. Once one has been played the game marks the
    # rest unplayable, so any still playable means the allowance is unused.
    bound_left = 1 if any(c.bound for c in playable) else None
    baseline = simulate([], enemies, **ctx)
    best_score, best_lost = (baseline.score, baseline.lost) if baseline else (float("-inf"), None)
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
        for line in combinations(playable, size):  # keeps order_key order
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
                        best_score, best_line, best_target, best_lost = out.score, list(order), out.first_target, out.lost
                if not usable and not nth and not any(c.is_x or pays(c) or c.key == "FLATTEN" or c.key in triggers.FREE_NEXT for c in line):
                    break  # cannot be paid for in any order
        if not found:
            break
    if plan is not None:
        plan["lost"], plan["score"], plan["line"] = best_lost, best_score, best_line
    if not best_line:
        return None

    card = best_line[0]
    params: dict = {"card_index": card.index}
    if card.needs_target:
        params["target"] = best_target or min(enemies, key=lambda e: e.hp + e.block).entity_id
    return ("play_card", params)


def actions(state: dict) -> list[Action]:
    """Ordered candidate actions for a combat state (first workable one is taken)."""
    if log.isEnabledFor(logging.DEBUG):
        log.debug("combat %s", describe(state))
    if log.isEnabledFor(logging.DEBUG) and _sandpit(state) is not None:
        log.debug("sandpit %s, no Escape next hand %.2f, later %.2f", _sandpit(state), *_escape_odds(state.get("player") or {}))
    plan: dict = {}
    card = choose_card(state, plan)
    if plan.get("lost", 0) >= int((state.get("player") or {}).get("hp") or 1) and fortifier(state.get("player") or {}):
        # Dead on paper with a Fortifier on the belt. It is drunk after the last card (potions._ill_timed), so
        # the cards have to be chosen for the Block it will triple: a Regent on 4 HP in front of 36, a Beckon
        # in hand (6 HP if it stays there), played three Block cards for 66 once tripled and died of the
        # Beckon, where the Beckon and two of them were 45 and the Soul Fysh had 6 HP left.
        braced: dict = {}
        with_it = choose_card({**state, "_fortify": 3}, braced)
        if braced.get("lost", plan["lost"]) < plan["lost"]:
            # (The loss the potions go by stays the one without it: that is the danger the Fortifier is drunk for.)
            card, plan = with_it, {**plan, "line": braced.get("line", [])}
    out = potion_actions(state, plan["lost"] if "lost" in plan else expected_end_loss(state), plan.get("line", []))  # (nothing playable: no plan)
    if card is not None:
        out.append(card)
    out.append(("end_turn", {}))
    return out
