"""Combat decisions: pick the best set of cards for this turn, play one, re-plan.

Each call scores every affordable subset of the hand with a small simulation (damage, kills, Block against the intents)
and returns the first card of the winning line; re-planning after every card picks up draws and other surprises.
"""

from __future__ import annotations

import logging
from dataclasses import dataclass, replace
from itertools import combinations, permutations

from .battle import (  # noqa: F401  (also the names other modules and the tests reach through here)
    BEYOND_REACH, ENERGY_DRAIN, LETHAL_PENALTY, ORB_VALUES, POWER_VALUE, RAGE_BLOCK, RAGE_COST, REATTACH_REACH, SLIP_WORTH, WIN_BONUS, Enemy, clone, _has_power, _is_attack, constrict,
    end_of_turn_block, end_of_turn_loss, expected_loss, hand_burns, intent_damage, intent_hits, kill_reach, osty_hp, power_amount, read_enemies, _bare_block, _draw_worth, _escape_odds,
    _next_wither, _sandpit, _wither_bill, deck_count, junk_cost, cards_left_this_turn, next_room, next_energy_worth, free_plays, _hit, _hit_count, _poison_total, _progress, _would_kill,
    _power_worth, _set_up, _rage_blocks, _resists, _respawns, loss_cap, THREAT_FLOOR, _fated, _pick_target,
)
from ..cards import CardInfo, order_key, parse
from ..run.potions import fortifier, describe, potion_actions
from . import sandpit as pit, artifact, closing, triggers
from .fool import sadness as sad_hand
from .fool.turn import FoolTurn, arrows_owned, concerns, fool_powers, impaled_pool, played_melodies
from ..games.sts1.stance import MANTRA_DIVINITY, MANTRA_LATER, TAKEN, Stance, carry_cost, read as read_stance, stuck_odds
from ..learn import knob

Action = tuple[str, dict]

log = logging.getLogger("sts2bot.combat")

MAX_SEARCH_CARDS = 11
HP_BASE, HP_SLOPE, HP_BITE = 1.5, 2.5, 3.0  # what a point of our HP costs a line, in damage: at full health, more as it runs out, more for a turn that takes it all
HIVE_COST = 9.0  # Personal Hive: what a Dazed in our draw pile costs once the hand has no card to spare, in points
HIVE_PACE = 22.0  # ... and the damage a round the Entomancer is taken to lose, for how long the fight still is

_idle = lambda *_: None  # stands in for a rule that cannot apply in this fight


def _orb_damage(kind: str, amount: int, foes: list[Enemy], chip_rate: float, dmg_value: float) -> float:
    """What an orb's damage is worth, taken off the enemies it lands on: Glass hits everyone, Dark the weakest, Lightning one at random."""
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
        # A bolt at random may kill the wrong crab, and the other gains 99 Block (_rage_blocks).
        risk = sum(4.0 * o.hp + 10.0 for e in alive if e.rage and e.hp + e.block <= amount for o in alive if o is not e and o.rage)
        return amount * 0.8 * dmg_value - risk / len(alive)
    return sum(_progress(e, _hit(e, amount, 1, raw=True), chip_rate) * dmg_value * e.worth for e in targets)


DEBUFF_CAP = 4  # turns of a debuff beyond this are too far off to count
FORM_WORTH = 40.0  # the Test Subject: what beating one of its forms is worth, in points (that much of the fight is done)
ENRAGE_PRICE = 2.0  # Enrage: points a Skill costs for each Strength it gives, per hit coming and per 35 HP the enemy has left


def _expose(e: Enemy, turns: int = 1) -> None:
    if not _resists(e):
        e.vulnerable = True
        fresh = max(0, min(turns, DEBUFF_CAP - e.vuln_turns))
        e.vuln_turns += fresh
        e.vuln_added += fresh


def _debuff(e: Enemy, c: CardInfo, x: int = 1) -> None:
    """Everything a card puts on one enemy."""
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


@dataclass
class Outcome:
    score: float
    first_target: str | None
    lost: int = 0  # HP the line expects to lose when the turn ends


def simulate(
    line: list[CardInfo], enemies: list[Enemy], *, energy: int, stars: int, hp: int, max_hp: int, block: int, fight: str = "monster", round_no: int = 1, card_limit: int | None = None,
    can_block: bool = True, sandpit: int | None = None, end_loss: int = 0, burns: tuple = (), hand_size: int = 0, focus: str | None = None, orbs: tuple = (), orb_slots: int = 0,
    shiv_damage: int = 4, tender: bool = False, vigor: int = 0, giant: bool = False, buffer: int = 0, loss_cap: int = 0, taint: int = 0, draw_worth: tuple = (2.0, 0.0, 0.0),
    bare_block: int = 0, regen: int = 0, focus_power: int = 0, escape_odds: tuple = (0.3, 0.3), wither_in: int = 0, wither_hp: int = 5, wither_bill: float = 7.0,
    trig: triggers.Triggers | None = None, pet: int = -1, hail: int = 0, deck: tuple = (20, 0), mastery: int = 0, might0: int = 0, holds: bool = False, strip: tuple = (0.0, 0.0),
    friends: int = 0, drained: int = 0, free: int = 0, doubled: bool = False, fortify: int = 1, pace: float = 0.0, room: tuple = (99.0, 1.0),
    arrows: int = -1, aiming: int = 0, stretch: int = 0, tune: frozenset = frozenset(), chord: int = 0, boom: int = 0, fool: tuple = (), stance0: str = "",
    wrath: tuple = (), sadness: tuple = (), mantra0: int = 0,
) -> Outcome | None:
    """Score playing `line` in order (None if it cannot be afforded), in the turn _context reads. Among the keywords: end_loss: HP we lose
    at the end of the turn whatever we play; burns: (hand index, HP, stopped by Block) of cards that hurt while held; focus: aim every
    targeted card at this enemy while it lives; orbs: (kind, passive, evoke), oldest first; tender: each card played costs 1 Strength and
    1 Dexterity; vigor, giant: the next Attack only; taint: Vital Spark (each Skill makes every hit on us harder); pet: Osty's HP, already
    in `block` (-1: none); might0: Strength not in the card texts yet; holds: Barricade; free: Void Form's free plays; doubled: the next card
    is played twice; fortify: a Fortifier's multiplier on our Block; wither_*: Withering Presence; arrows to fool: the Fool's (fool/turn.py);
    wrath: (the chance next turn's hand has no way out of Wrath, each enemy's (blow, share of turns it attacked) so far) for a Watcher (`_wrath_outlook`);
    sadness: the Fool's Sadness and what is to come of it (fight/fool/sadness.py `outlook`); mantra0: the Watcher's Mantra."""
    if card_limit is not None and len(line) > card_limit:
        return None
    if any(c.ends_turn for c in line[:-1]):
        return None  # "End your turn": nothing is played after it
    foes = [clone(e, chipped=0, stripped=0, vuln_added=0, weak_added=0) for e in enemies]
    score = 0.0
    turn_to, focus = (focus[1], None) if isinstance(focus, tuple) else (None, focus)  # ("last", id): the last aimed card goes at that enemy, so the turn ends facing it
    last_aimed = max((n for n, c in enumerate(line) if c.needs_target), default=-1)
    first_target: str | None = None
    # Damage is worth more where the fight is a race: bosses and elites, enemies that gain Strength every turn, fights that drag on.
    dmg_value = {"boss": 1.5, "elite": 1.3}.get(fight, 1.0) + min(0.4, 0.08 * max(0, round_no - 5))
    dmg_value *= 1.0 + min(1.5, 0.15 * max((e.grows for e in enemies if e.worth >= 1.0), default=0.0))
    dmg_value += 0.5 * max(0, round_no - 12)
    chip_rate = min(1.0, 0.25 + 0.05 * max(0, round_no - 10))  # worth of temporary Block removed
    kept_block = {e.entity_id for e in enemies if e.keeps_block}
    # As the line goes on:
    self_loss, paid = 0, 0  # HP it costs us, and of that what the cards themselves cost (Offering, Hemokinesis)
    escapes, osty_hits, attacks, plays = 0, 0, 0, 0  # Frantic Escapes, Osty attacks, Attacks and all cards played (one played again counts again)
    free_next: str | None = None  # the kind of card that costs nothing next, after one that says so
    surcharge = 0  # energy every further card costs on top of its own, after one that says so
    echo: str | None = "Any" if doubled else None  # the kind of card that is played an extra time next (Burst, One-Two Punch)
    dooming, free_all = False, False  # a card applied Doom; Bullet Time: everything left in the hand is free this turn
    healed, nimble, might, summoned, pet0 = 0, 0, might0, 0, pet  # HP given back; Dexterity and Strength gained; HP given Osty (and his HP as the texts were written)
    faced: str | None = None  # the last enemy a card was aimed at
    held = list(orbs)  # the Defect's orbs
    drawn, debris, dazed, tainted = 0, 0, 0, 0  # cards drawn; Debris and Dazed (Personal Hive) put in our deck; Vital Spark's extra damage a hit
    lasting, enduring, future = 0.0, 0.0, 0.0  # worth of effects that pay off over the fight (x `span`), of what stays all fight, of all that pays off after this turn
    later, nexte, kept = 0.0, 0, False  # worth of things that arrive next turn; energy next turn, and whether the hand is Retained
    mantra = mantra0  # the Watcher's Mantra as the line goes on
    sad, sad_rate, sad_draw = 0, 0, 0  # Sadness the line puts in the hand, and more (or fewer) each turn from its Powers, with their draw
    # What a lasting effect is still worth: boss fights go on for a dozen turns, ordinary ones are over in four.
    span = {"boss": 3.0, "elite": 2.0}.get(fight, 1.0)
    late = {"boss": 7, "elite": 5}.get(fight, 3)
    span *= 1.0 if round_no <= late else 0.4
    ft, st = FoolTurn(arrows, aiming, stretch, tune, chord, boom, fool, foes), Stance(stance0)  # the Fool's Arrows, Aim, Stretch and Melodies; the Watcher's stance
    known_dead = {e.entity_id for e in foes if not e.alive}
    beating, softens = any(e.beat for e in foes), any(e.malleable for e in foes)  # (the first game's Heart; its Snake Plant)
    # (Only Stock and Adaptable enemies come back and only crabs hand on Block: for the rest both are skipped.)
    respawn, rage_blocks = (_respawns if any(e.respawns > 0 or e.adapts for e in foes) else _idle), (_rage_blocks if any(e.rage for e in foes) else _idle)

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
        respawn(foes)
        rage_blocks(foes, known_dead)  # a crab that died with the last card hands its ally Block
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
            # Flatten costs 0 once Osty has attacked this turn, after a Poke in the same line too.
            cost = 0 if (c.key == "FLATTEN" and osty_hits) or c.type == free_next or free_all or n < free else c.cost + surcharge
            free_next = None if c.type == free_next else free_next
            if cost > energy:
                return None
            energy -= cost
        # Played again (its own Replay, or the card before said so): everything it does, it does again, and each time is one of the cards a turn allows.
        echoed = bool(echo) and echo in ("Any", c.type)
        k, echo = 1 + c.replay + echoed + ft.encore(c), None if echoed else echo  # (Ki Verse: the next Wah, Cho or Ki card again)
        if card_limit is not None and plays >= card_limit:
            return None
        plays += k
        if c.play_limit >= 0:  # Encore: "You can only play 1 more card this turn"
            card_limit = plays + c.play_limit if card_limit is None else min(card_limit, plays + c.play_limit)
        if k > 1:
            more = dict(block=c.block * k, draw=c.draw * k, summon=c.summon * k, doom=c.doom * k, poison=c.poison * k, shivs=c.shivs * k, souls=c.souls * k)
            c = replace(c, hits=c.hits * (k if c.damage else 1), energy_gain=c.energy_gain * k, **more)
        echo = triggers.ECHO_NEXT.get(c.key, echo)
        if concerns(c) or (c.block and (ft.stretch or ft.stretch0)):
            c = ft.before(c, foes, turn_to if turn_to and n == last_aimed else focus)
        energy *= 2 if c.key == "DOUBLE_ENERGY" else 1
        free_all = free_all or c.key == "BULLET_TIME"
        energy += c.energy_gain
        free_next = triggers.FREE_NEXT.get(c.key, free_next)  # Unrelenting: "The next Attack you play costs 0"
        surcharge += triggers.SURCHARGE.get(c.key, 0)
        healed += c.heal
        self_loss, paid = self_loss + c.hp_loss, paid + c.hp_loss
        if c.hp_loss and paid >= hp + healed:  # a card's HP is paid as it is played: no Regen, Block or kill at the end of the turn gives it back
            score -= LETHAL_PENALTY
        if c.osty_attack and pet == 0:  # Osty is dead: his attacks do nothing
            c = replace(c, damage=0, block=0 if c.osty_dies else c.block)
        elif c.osty_attack:
            c, osty_hits = (replace(c, hits=c.hits + osty_hits) if c.key == "RATTLE" else c), osty_hits + 1
            # Unleash adds Osty's HP to its damage and Protector his Max HP: what was Summoned earlier in the line counts too.
            grown = pet - pet0 if c.key == "UNLEASH" else summoned if c.key == "PROTECTOR" else 0
            c = replace(c, damage=max(0, c.damage + grown)) if grown else c
        if c.osty_dies and pet > 0:
            block, pet = block - min(block, pet), 0  # ... and the HP he stood in front of us with goes with him
        pet += c.summon * x if pet >= 0 else 0
        if can_block and c.block:
            block += max(0, c.block + nimble) * x
        if can_block and c.summon:  # Osty's HP soaks hits like Block that does not go away at the end of the turn
            block += c.summon * x
            summoned += c.summon * x
        if c.block_lock:  # trading away future Block is only worth it to survive this turn
            score -= 30.0
        if c.deadly:
            score -= LETHAL_PENALTY  # never gamble the run on one card
        xs = x if c.is_x else 1
        alive = [e for e in foes if e.alive]
        t = focused = next((e for e in alive if e.entity_id == aim), None) if (aim := turn_to if turn_to and n == last_aimed else focus) is not None else None  # the enemy this card is aimed at, if any
        if c.key in ("TIMES_UP", "TIME_S_UP") and alive:  # "Deal damage equal to the enemy's Doom": no number in its text
            t = focused = focused or max(alive, key=lambda e: e.doom)
            c = replace(c, damage=t.doom, hits=1)
        if c.key in ("DEATHS_DOOR", "DEATH_S_DOOR") and dooming and can_block:  # "If you applied Doom this turn, gain Block 2 additional times"
            block += 2 * max(0, c.block + nimble)
        dooming = dooming or bool(c.doom)
        if c.key == "FIEND_FIRE":  # "Exhaust your Hand. Deal 7 damage for each card Exhausted": a hit for every card still in it
            c = replace(c, hits=max(0, hand_size - n - 1 + drawn))
        dealt = 0  # what the card's hits do to its target: a Shot's Stretch (fool/turn.py), Rain Shot's splash below
        if c.damage:
            per, hits = c.damage + (0 if c.osty_attack else might), c.hits * x  # (Osty's attacks take no Strength)
            if c.type == "Attack":
                if attacks:  # Vigor and Gigantification went with the first Attack
                    per = round(per / 3) if giant else per
                    per -= vigor
                attacks += 1
                per = st.deal(per)
            per = max(0, per)
            dealt = per * hits
            if c.aoe:
                t, struck = None, alive
            elif c.random_target and len(alive) > 1:
                t, struck = None, []
                score += per * hits * 0.8 * dmg_value
            else:
                t = focused or _pick_target(impaled_pool(c, foes), per, hits, c.double_if_vulnerable)
                if c.key == "HANG" and t is not None:
                    # "Double the damage ALL Hang cards deal to this enemy": 10, 20, 40, 80 on the same enemy, though the text goes on saying 10.
                    per, t.hang = per * max(1, t.hang), 2 * max(1, t.hang)
                struck = [t] if t is not None else []
            # Several hits on ALL enemies land one round at a time: a crab that dies of the first hands the other its 99 Block for the rest.
            waves = hits if c.aoe and hits > 1 and any(e.rage for e in struck) else 1
            for _ in range(waves):
                for e in struck:
                    if not e.alive:
                        continue
                    n_hits = hits // waves
                    self_loss += e.thorns * _hit_count(e, n_hits, c.double_if_vulnerable)
                    dazed += e.hive * _hit_count(e, n_hits, c.double_if_vulnerable) * (c.type == "Attack")
                    done = _fated(e)  # damage that does not kill it now changes nothing
                    gain = _progress(e, _hit(e, per, n_hits, c.double_if_vulnerable), chip_rate) * dmg_value * e.worth * e.focus
                    score += gain * (0.2 if done and e.alive else 1.0)
                    if e.reacts and e.alive and c.type == "Attack":  # (a new intent, which may be its worst: battle.REACTIVE_HIT)
                        e.incoming, e.incoming_hits = max(e.incoming, e.reacts), max(e.incoming_hits, 1)
                if waves > 1:
                    rage_blocks(foes, known_dead)
            for e in struck:
                if e.alive:
                    _debuff(e, c, xs)
        elif c.needs_target:
            if focused is not None:
                pass
            elif c.enemy_strength_loss:
                t = max(alive, key=lambda e: min(e.incoming, c.enemy_strength_loss * e.incoming_hits), default=None)
            elif c.weak and not c.vulnerable:
                t = max(alive, key=lambda e: (not e.weak, e.incoming), default=None)  # the hardest hitter not Weak yet
            elif c.doom and not c.vulnerable:
                t = _pick_target(foes, 0, 0)  # Doom goes where the attacks go: together they end one enemy sooner
            elif c.poison_needs:
                t = max(alive, key=lambda e: (e.poison, e.worth), default=None)  # (Bubble Bubble) only the poisoned one takes it
            else:
                fresh = [e for e in impaled_pool(c, alive) if not e.vulnerable] or impaled_pool(c, alive)  # (Uproot: the Impaled one)
                t = max(fresh, key=lambda e: (e.focus, e.hp + e.block), default=None)  # Vulnerable: on whom we will hit the longest (the next to die, by a kill order)
            if t is not None:
                _debuff(t, c, xs)
        else:
            t = None
            for e in alive if c.aoe else []:
                _debuff(e, c, xs)
        if ft.books(c) and ((chord := ft.after(c, t, xs, dealt, foes)) or any(ft.got)):  # Violent Chord, Explosive Arrows: a Wah Melody gained, an enemy Impaled hits ALL enemies
            block, drawn, energy = block + ft.got[0] * can_block, drawn + ft.got[1], energy + ft.got[2]  # (Song Path, Lullaby; Motif; Yah!)
            score += sum(_progress(e, _hit(e, chord, 1, raw=True), chip_rate) * dmg_value * e.worth for e in foes if e.alive) if chord else 0.0
        if c.splash and t is not None and dealt:  # Rain Shot: "Damage ALL other enemies equal to the damage dealt"
            score += sum(_progress(e, _hit(e, dealt, 1, raw=True), chip_rate) * dmg_value * e.worth for e in foes if e.alive and e is not t)
        if t is not None:
            faced = t.entity_id
            if n == 0:
                first_target = t.entity_id
        alive_now = [e for e in foes if e.alive]
        aimed = next((e for e in alive_now if e.entity_id == aim), None) if (aim := faced or focus) is not None else None
        if c.poison or c.doom or c.enemy_hp_loss or c.shivs:
            if c.aoe:
                victims = alive_now
            elif aimed is not None and (c.needs_target or c.damage):
                victims = [aimed]
            else:  # untargeted or random: the enemy that matters most
                victims = sorted(alive_now, key=lambda e: (-e.worth, e.hp))[:1]
            for e in victims:
                if c.poison and (e.poison > 0 or not c.poison_needs) and not _resists(e):
                    e.poison += c.poison * x
                if c.doom and not _resists(e):
                    doom = c.doom * x + triggers.more_doom(c, e)
                    score += (doom if e.adapts else min(doom, max(0, e.hp - e.doom))) * 0.8 * dmg_value * e.worth  # (Doom beyond its HP: nothing, unless a form follows)
                    e.doom += doom
                if c.enemy_hp_loss:
                    lost_hp = min(e.hp, c.enemy_hp_loss)
                    e.hp -= lost_hp
                    score += lost_hp * dmg_value * e.worth
                if c.shivs and e is victims[0]:  # Shivs are free 4-damage cards: as good as that damage now
                    score += _progress(e, _hit(e, shiv_damage + might, c.shivs * x), chip_rate) * dmg_value * e.worth * 0.9
        if c.needs_target and not c.damage and aimed is None and alive_now and (c.poison or c.doom) and n == 0 and first_target is None:
            first_target = sorted(alive_now, key=lambda e: (-e.worth, -e.hp))[0].entity_id
        if trig or c.doom_kills:
            won, shield = triggers.fire(trig or triggers.NONE, c, foes, before, lambda e, d: _hit(e, d, 1, raw=True), lambda e, hp: _progress(e, hp, chip_rate) * dmg_value * e.worth)
            score, block = score + won, block + (shield if can_block else 0)
        # Stars are worth what they will be spent on: less the more of them there are.
        score += c.star_gain * (2.5 if stars < 4 else 1.0 if stars < 8 else 0.2) + c.souls * 2.5
        stars += c.star_gain
        enduring += c.forge * 0.5
        later, nexte = later + c.next_draw * 2.0 * dmg_value + c.next_block * 0.8, nexte + c.next_energy * xs  # (energy: priced after the line)
        kept = kept or c.keeps_hand == ("ANY",) or bool(ft.tune & set(c.keeps_hand))  # (Ki Rest with a Wah or Cho Melody)
        energy += st.enter(c.stance)  # (after the card's own damage: Eruption hits, then she is in Wrath)
        if c.mantra:  # (the Watcher's: at 10 she enters Divinity at once, and the 10 are used up)
            mantra += c.mantra * xs
            if mantra >= MANTRA_DIVINITY:
                mantra -= MANTRA_DIVINITY
                energy += st.enter("DIVINITY")
        if c.dexterity:
            nimble += c.dexterity
            if c.dexterity_lasts:
                enduring += c.dexterity * 3.5
        if c.channel or c.evoke or c.evoke_all:
            rage_blocks(foes, known_dead)  # a crab this card killed has already handed on its Block
        for kind in c.channel * xs:
            if orb_slots and len(held) >= orb_slots:
                evoke(held.pop(0), plasma=False)  # no room: the oldest orb is evoked to make space
            passive, burst = ORB_VALUES.get(kind, (3, 6))
            if kind != "PLASMA":
                passive, burst = max(0, passive + focus_power), max(0, burst + focus_power)
            held.append((kind, passive, burst))
            lasting += {"FROST": 3.0, "LIGHTNING": 3.0, "DARK": 4.0, "PLASMA": 6.0, "GLASS": 4.0}.get(kind, 3.0)
        # "Evoke your rightmost Orb": the one channeled first, which the game lists first.
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
        escapes += c.key == "FRANTIC_ESCAPE"
        if c.sadness or c.sad_turn:
            sad += c.sadness
            sad_rate, sad_draw = (sad_rate + c.sad_turn[0], sad_draw + c.sad_turn[1]) if c.sad_turn else (sad_rate, sad_draw)
        if c.debris:  # (Crash Landing fills what room the hand has left: fewest when it is played first)
            debris += c.debris if c.debris > 0 else max(0, 10 - (hand_size - (n + 1) + drawn + debris))
        buffer += c.key == "BUFFER"  # played now, it stops this turn's hit
        for e in foes:
            if e.slow >= 0:
                e.slow += 10  # for the cards after this one (the card itself gets no bonus)
        if tender:
            might, nimble = might - 1, nimble - 1
        if c.type == "Skill":
            tainted += taint
            score -= sum(e.enrage * ENRAGE_PRICE * max(1, e.incoming_hits) * min(4.0, 1.0 + e.hp / 35.0) for e in foes if e.alive)  # Enrage: each Skill, Strength for good
        drawn += c.draw * xs
        if c.strength:
            might += c.strength
            if c.strength_lasts:  # pays off on every attack for the rest of the fight (or costs on every one: Friendship, Shared Fate)
                future += c.strength * 4.0 * span
        if c.enemy_strength_lasts:  # ... and Strength taken off an enemy for good comes off every hit it has left (this turn's share: in `incoming`)
            future += sum(c.enemy_strength_loss * max(1, e.incoming_hits) * 2.0 * e.worth for e in (alive if c.aoe or t is None else [t]) if e.alive) * span
        if c.type == "Power":
            score -= sum(e.curious * ENRAGE_PRICE * max(1, e.incoming_hits) * min(4.0, 1.0 + e.hp / 35.0) for e in foes if e.alive)  # (as Enrage: Curiosity)
            # (An Ethereal Power not played now is never played: more in a long fight. A second Friendship is energy the hand has no cards for.)
            again, friends = (0.5**friends, friends + 1) if c.key == "FRIENDSHIP" else (1.0, friends)
            # (Ennui's Sadness and draw: as fight/fool/sadness.py works them out over the turns left.)
            told = (c.strength_lasts and c.strength > 0) or (sadness and c.sad_turn and c.sad_turn[0] > 0)
            future += 0.0 if told else again * _power_worth(c, fight != "monster") * span * (1.6 if c.ethereal and fight != "monster" else 1.0)
            future -= c.energy_drain * ENERGY_DRAIN * (1 + drained) * span
            drained += c.energy_drain
        elif c.type in ("Status", "Curse"):  # only to use up energy nothing else can, unless playing it gets it out of the deck (Debris, Slimed)
            score += 0.1 + (junk_cost(fight, round_no, (deck[0] - 1, max(0, deck[1] - 1))) * dmg_value if c.key in ("DEBRIS", "SLIMED") else 0.0)
        elif not c.known and c.key not in triggers.READ:
            score += 2.5 + max(c.cost, 0)
        future += triggers.kept(c, dmg_value) * span  # (an Ethereal card not played now is Exhausted)
        if beating and (beat := sum(e.beat for e in foes if e.alive) * k):  # Beat of Death: each play hits us once it has resolved, Block first
            soak = min(max(0, block), beat)
            block, self_loss = block - soak, self_loss + beat - soak
        for e in foes if softens else ():  # Malleable: the Block its hits earned it comes once the card is done
            e.block, e.soft = e.block + e.soft, 0

    if healed:  # HP given back is HP not lost, at the price HP has right now; and it is there for this turn's blow
        healed = min(healed, max(0, max_hp - hp))
        score += healed * (1.5 + 2.5 * (1.0 - hp / max(1, max_hp))) * knob("caution")
        hp += healed
    # Cards drawn: the free ones as they come, the rest as far as the energy left over stretches (and only as many as may still be played).
    drawn = drawn if card_limit is None else min(drawn, max(0, card_limit - plays))
    free, paid_share, per_energy = draw_worth
    score += drawn * max(1.0, free) + min(float(max(energy, 0)), drawn * paid_share) * per_energy
    score += 15.0 * sum(1 for e in foes if e.alive and e.entity_id in kept_block and not e.keeps_block)  # breaking Block it was keeping Stuns it
    # The Defect's orbs act when the turn ends, before the enemy does (Frost adds Block, Lightning hits something): one that dies of them does not attack.
    respawn(foes)
    rage_blocks(foes, known_dead)
    for kind, passive, _burst in held:
        if kind == "FROST" and can_block:
            block += passive
        elif kind in ("LIGHTNING", "GLASS"):
            score += 0.9 * _orb_damage(kind, passive, foes, chip_rate, dmg_value)
    if hail and any(o[0] == "FROST" for o in held):
        score += sum(_progress(e, _hit(e, hail, 1, raw=True), chip_rate) * dmg_value * e.worth for e in foes if e.alive)
    later += max(0, mantra - mantra0) * MANTRA_LATER  # (Mantra short of Divinity: on the way to it)
    if sadness and (sad or sad_rate or sad_draw):
        later += sad_hand.worth(sadness, sad, sad_rate, sad_draw)
    later += ft.later(dmg_value) + next_energy_worth(nexte + st.later_energy(), room, max(0, hand_size + drawn - len(line)) if kept else 0) * dmg_value  # (Aim, Stretch)
    future += (lasting + enduring) * span + later
    score += future
    if debris:
        score -= junk_cost(fight, round_no, deck, debris) * dmg_value
    if dazed:
        # A Dazed is a draw lost (it is Ethereal): a card of ours, as far as the deck has live ones. The first two a turn cost little (a hand
        # of five has cards to spare), the rest a card each, and only as far as the fight will last long enough to draw them.
        live = max(10, deck[0])
        left = sum(e.hp + e.block for e in foes if e.alive and e.hive)
        in_time = min(1.0, max(0.25, (left / HIVE_PACE) / (live / 5.0))) if left else 0.0
        score -= HIVE_COST * (0.35 * min(dazed, 2) + max(0, dazed - 2)) * in_time * max(0.3, 1.0 - deck[1] / live)
    # Poison bites before the enemy acts: poisoned through is as good as dead. Otherwise what is added ticks for as long as the stack lasts.
    poison_turns = ({"boss": 7, "elite": 4}.get(fight, 3)) if round_no <= late else 3
    for e, orig in zip(foes, enemies):
        if not e.alive:
            continue
        if (min(e.poison, e.hit_cap) if e.hit_cap else e.poison) >= e.hp:  # (not through Intangible)
            e.hp = 0
            continue
        if e.poison > orig.poison:
            gain = _poison_total(e.poison, poison_turns) - _poison_total(orig.poison, poison_turns)
            score += min(e.hp, gain) * e.worth * (dmg_value if e.hp > 60 else 1.0)
        score += min(e.hp, e.poison) * 0.3 * e.worth  # the tick that comes this turn
    # Vulnerable is half again our damage and Weak a quarter off theirs, for every turn they last: in long fights more than any single attack.
    vuln_turn = {"boss": 7.0, "elite": 5.0}.get(fight, 3.0) * knob("debuff_worth")
    weak_turn = {"boss": 5.0, "elite": 3.5}.get(fight, 2.0) * knob("debuff_worth")
    for e in foes:
        if e.alive:
            # (The first turn of Weak is in `incoming` already, and Vulnerable used this turn in the damage dealt.)
            score += (max(0, e.vuln_added - 1) + 0.5 * min(1, e.vuln_added)) * vuln_turn * e.worth
            score += (max(0, e.weak_added - 1) + 0.3 * min(1, e.weak_added)) * weak_turn * (1.0 if e.incoming_hits or not e.minion else 0.5)
    score += sum(artifact.gain(orig.artifact, e.artifact, strip) * e.worth for e, orig in zip(foes, enemies) if e.alive and e.artifact < orig.artifact)
    score += 20.0 * sum(e.worth for e, orig in zip(foes, enemies) if e.alive and e.doom >= e.hp and orig.doom < orig.hp)  # Doom kills at the end of its turn
    respawn(foes)  # a Stock enemy killed by the last card comes back too
    kills = [e for e, orig in zip(foes, enemies) if not e.alive and orig.alive]
    score += 25.0 * sum(e.worth for e in kills)
    score += FORM_WORTH * sum(1 for e in foes if e.adapted)  # a form beaten: that much of the fight is done
    survivors = [e for e in foes if e.alive]
    if not survivors or (all(e.minion for e in survivors) and not all(e.minion for e in enemies)):  # every leader dead: minions leave with them
        score += WIN_BONUS
        survivors = []
    if kills and survivors:
        # Crab Rage: the one left standing gets 99 Block (its attacks this turn are wasted, see _rage_blocks) and 6 Strength for good.
        for e in survivors:
            if e.rage:
                score -= len(kills) * RAGE_COST * e.hp
                e.incoming += 6 * e.incoming_hits * len(kills)
    if sandpit is not None and survivors:
        # The Insatiable: Sandpit counts down every turn and at 0 we are eaten; each Frantic Escape played pushes it back by one (sandpit.py).
        if sandpit + escapes <= 1:
            score -= LETHAL_PENALTY
        else:
            dealt = max((e.max_hp - e.hp for e in enemies if not e.minion), default=0)
            score -= pit.price(sandpit + escapes, max(e.hp for e in survivors), dealt, round_no, escape_odds)
    if faced is not None:  # turning to face an enemy takes away its 50% bonus and gives it to the other one
        for e in survivors:
            if e.flanker and e.behind and e.entity_id == faced:
                e.incoming = round(e.incoming / 1.5)
            elif e.flanker and not e.behind and e.entity_id != faced:
                e.incoming = round(e.incoming * 1.5)
    incoming = round(sum(e.incoming + tainted * e.incoming_hits for e in survivors) * st.taken())
    if block <= 0 and can_block:
        block = bare_block
    played = {c.index for c in line}
    left = max(0, hand_size - len(line))
    # Cards that burn in the hand go off when our turn ends, before the enemy moves: the ones that deal damage eat into our Block first.
    scorch = sum(amount for index, amount, blocked in burns if blocked and index not in played)
    burnt = sum((left if amount < 0 else amount) for index, amount, blocked in burns if not blocked and index not in played)
    if wither_in > 0 and len(line) >= wither_in:  # a Wither lands in the hand and burns when the turn ends
        scorch += wither_hp * (1 + (len(line) - wither_in) // 6)
    guard = min(max(pet, 0), max(block, 0))  # (Osty stops attacks and nothing else)
    own = max(0, block - guard) * fortify  # Block that is ours
    burnt += max(0, scorch - own)
    through = max(0, incoming - max(0, own - scorch) - guard)
    if through and buffer >= sum(e.incoming_hits for e in survivors if e.incoming):
        through = 0  # Buffer swallows each hit that would have hurt
    if through:  # only the hits that get through leave a scar
        for e in survivors:
            if e.incoming and e.scar:
                per = max(1, e.incoming // max(1, e.incoming_hits))
                score -= e.scar * min(e.incoming_hits, -(-through // per))
    if wither_in > 0:  # ... and a Wither stays in the deck, burning whenever it is drawn: every card played is a sixth of that bill
        score -= len(line) * wither_bill
    lost = through + self_loss + end_loss + burnt
    lost = max(0, lost - min(regen, max(0, max_hp - hp + self_loss)))  # Regen mends what was missing first
    lost = min(lost, loss_cap) if loss_cap else lost
    ratio = hp / max(1, max_hp)
    # Imbalanced (Bowlbug Rock): an attack of his that our Block stops whole leaves him Stunned for his next turn (Osty's HP left out).
    cover = max(0, block - max(pet, 0) - scorch)
    for e in survivors:
        hit = e.incoming + tainted * e.incoming_hits
        if e.imbalanced and hit and cover >= hit:
            score += 0.85 * hit * (1.5 + 2.5 * (1.0 - ratio)) * knob("caution")
        cover -= min(cover, hit)
    # Osty keeps the HP he is not hit for: unlike Block it is still there next turn (and what he soaks now he cannot soak later).
    if pet > 0:
        kept = min(pet, max(0, max(0, own - scorch) + guard - incoming))
        score += kept * (1.5 + 2.5 * (1.0 - ratio)) * knob("caution") * {"boss": 0.85, "elite": 0.7}.get(fight, 0.5)  # (less than ours: the fight may end first)
        if mastery:  # ... and under Necro Mastery what he is hit for, every enemy is hit for as well
            score += (pet - kept) * mastery * dmg_value * sum(e.worth for e in survivors)
    if holds:  # Block that outlasts the turn is next turn's Block (as Osty's HP is, and a little surer)
        score += max(0, block - max(pet, 0) - scorch - incoming) * (1.5 + 2.5 * (1.0 - ratio)) * knob("caution") * {"boss": 0.8, "elite": 0.7}.get(fight, 0.5)
    # Every leader left is Doomed to die at the end of its turn: the fight is won if we live through that turn, so more damage is wasted energy.
    leaders = [e for e in survivors if not e.minion]
    doomed = bool(leaders) and all(e.doom >= e.hp for e in leaders) and not any(e.adapts or e.spent for e in survivors)
    if doomed:
        score += WIN_BONUS / 2
    # A point of our HP is worth more than a point of enemy HP even at full health, and more still when this one turn would take a big bite of what is left.
    bite = min(1.0, lost / max(1, hp))
    worth = HP_BASE + HP_SLOPE * (1.0 - ratio) + HP_BITE * bite
    price = lost * worth * (4.0 if doomed else 1.0) * knob("caution")
    price *= closing.discount(pace, lost, hp, survivors)  # (a boss about to die: the HP we keep past its death buys nothing)
    score -= price
    if wrath and st.now == "WRATH" and survivors and lost < hp:
        # Ending the turn in Wrath: next turn's blows land double unless the next hand holds a way out (2026-11-25: a Hyper Beam of 104).
        odds, blows = wrath
        seen = [blows.get(e.entity_id, (e.incoming / TAKEN.get(st.start, 1.0), 1.0)) for e in survivors]  # (a first turn: what it shows now)
        score -= carry_cost(odds, sum(b for b, _ in seen), hp - lost, max((share for b, share in seen if b > 0), default=0.0))
    if len(line) < wither_in:  # stopping short of the next Wither only puts it off (the count carries over) to the next turn's first card
        score -= wither_hp * (1.5 + 2.5 * (1.0 - ratio)) * knob("caution")
    if lost >= hp:
        # Dead on paper is dead: the HP is not priced; what counts is cards drawn while there is energy to play them, and how near the line comes to a kill.
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
    draws = _draw_worth(player)
    return dict(
        energy=int(player.get("energy") or 0), stars=int(player.get("stars") or 0),
        hp=max(1, int(player.get("hp") or 1) - max(0, power_amount(player, "DOOM")) - max(0, power_amount(player, "NEUROSURGE"))),  # (Doom on us kills once it reaches our HP)
        max_hp=int(player.get("max_hp") or 1), block=int(player.get("block") or 0) + end_of_turn_block(player) + osty_hp(player), fortify=int(state.get("_fortify") or 1),
        fight=str(state.get("state_type") or "monster"), round_no=int(battle.get("round") or 1), card_limit=cards_left_this_turn(state), can_block=not _has_power(player, "NO_BLOCK"),
        sandpit=_sandpit(state), escape_odds=_escape_odds(player),
        burns=hand_burns(player) + tuple((-1 - n, hp, True) for n, hp in enumerate((end_of_turn_loss(player), constrict(player))) if hp),  # (damage that Block stops)
        hand_size=len(player.get("hand") or []), wither_in=max((power_amount(e, "WITHERING_PRESENCE") for e in battle.get("enemies") or []), default=0),
        wither_hp=_next_wither(player, int(battle.get("round") or 1)), wither_bill=_wither_bill(player, int(battle.get("round") or 1), _next_wither(player, int(battle.get("round") or 1))),
        draw_worth=draws,
        orbs=tuple((str(o.get("id") or o.get("name") or "").upper().replace("_ORB", ""), int(o.get("passive_val") or 0), int(o.get("evoke_val") or 0)) for o in player.get("orbs") or []),
        orb_slots=int(player.get("orb_slots") or 0), bare_block=_bare_block(player), regen=max(0, power_amount(player, "REGEN")),
        focus_power=power_amount(player, "FOCUS") if _has_power(player, "FOCUS") else 0, shiv_damage=4 + max(0, power_amount(player, "ACCURACY")), tender=_has_power(player, "TENDER"),
        hail=max(0, power_amount(player, "HAILSTORM")), deck=deck_count(player),
        strip=artifact.outlook(player, str(state.get("state_type") or "monster"), int(battle.get("round") or 1)) if any(_has_power(e, "ARTIFACT") for e in battle.get("enemies") or []) else (0.0, 0.0),
        vigor=max(0, power_amount(player, "VIGOR")), giant=_has_power(player, "GIGANTIFICATION"), buffer=max(0, power_amount(player, "BUFFER")), loss_cap=loss_cap(player),
        trig=triggers.read(state), pet=osty_hp(player) if player.get("pets") else -1, mastery=max(0, power_amount(player, "NECRO_MASTERY")),
        might0=int(state.get("_might") or 0), holds=_has_power(player, "BARRICADE"), friends=max(0, power_amount(player, "FRIENDSHIP")),
        drained=max(0, power_amount(player, "ENERGY_DOWN")), free=free_plays(state),
        doubled=_has_power(player, "DUPLICATION") or max(0, power_amount(player, "ECHO_FORM")) > int(state.get("_plays_this_turn") or 0),  # (Echo Form: the first card(s) a turn)
        taint=max((power_amount(e, "VITAL_SPARK") for e in [player, *(battle.get("enemies") or [])]), default=0), pace=closing.pace(state), room=next_room(state),
        arrows=arrows_owned(player), aiming=max(0, power_amount(player, "AIM")), stretch=max(0, power_amount(player, "STRETCH")),
        tune=played_melodies(state.get("_played_names")), chord=max(0, power_amount(player, "VIOLENT_CHORD")), boom=max(0, power_amount(player, "EXPLOSIVE_ARROWS")),
        fool=fool_powers(player, bool(state.get("_dislodged"))), stance0=str(player.get("stance") or ""), wrath=_wrath_outlook(state),
        sadness=sad_hand.outlook(state, draws), mantra0=max(0, power_amount(player, "MANTRA")),
    )


def _wrath_outlook(state: dict) -> tuple:
    """For a Watcher who is in Wrath or holds a card that enters it: the chance that next turn's hand holds no way out, and each
    enemy's blow on the turns it attacked, as it would be outside Wrath, with how often it attacked (`_blow`, kept by the bot). Else ()."""
    player = state.get("player") or {}
    if player.get("stance") is None:
        return ()  # (a game without stances)
    if read_stance(player.get("stance")) != "WRATH" and not any(parse(c).stance == "WRATH" for c in player.get("hand") or []):
        return ()
    return (stuck_odds(player), dict(state.get("_blow") or {}))


def choose_card(state: dict, plan: dict | None = None) -> Action | None:
    """Best card to play right now, or None to end the turn (`plan["lost"]`: HP the best line still loses)."""
    player = state.get("player") or {}
    enemies = read_enemies(state)
    ctx = _context(state)
    hand = triggers.counted([parse(c) for c in player.get("hand") or []])
    free = ctx["free"]
    if free:
        # Void Form: with free plays left every card shows as costing 0; what each costs after them is what the deck said when the fight began.
        hand = [replace(c, cost=(state.get("_costs") or {}).get(c.name, c.cost)) for c in hand]
    playable = [c for c in hand if c.playable and (c.is_x or free or c.cost <= ctx["energy"]) and c.star_cost <= ctx["stars"]]
    playable = [c for c in playable if not triggers.shunned(c, state)]
    if not enemies and state.get("_revive_pending"):
        return _set_up(playable, ctx["energy"])
    if len(enemies) == 1 and enemies[0].asleep >= 2 and ctx["round_no"] <= 2:
        # Asleep ("Awakens upon losing HP or after 3 turns"): she wakes on her own time whatever we do, so the turns before the last are for what lasts.
        return _set_up(playable, ctx["energy"])
    if not playable or not enemies:
        return None
    # Cards we cannot pay for yet can still come later in a line that first makes the energy or Stars for them (evoking a Plasma orb too).
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
        playable.sort(key=lambda c: (-(c.total_damage + c.block), c.index))  # few plays allowed: spend them on what does the most now
    elif any(e.slow >= 0 for e in enemies):
        playable.sort(key=lambda c: (1 if c.damage else 0, c.total_damage, c.index))  # Slow: every card played makes later attacks hit harder
    elif any(e.slippery for e in enemies):
        playable.sort(key=lambda c: (order_key(c)[0] if not c.damage else 3, c.damage, c.index))  # Slippery: burn it off with the weakest hits
    else:
        # (Flatten and Rattle after Osty's other attacks: one is free then, the other hits again. No Escape after the other Doom.)
        after = lambda c: (order_key(c)[0], 1e3, c.index) if c.key in ("FLATTEN", "RATTLE", "NO_ESCAPE") else order_key(c)
        playable.sort(key=lambda c: ((1,) + order_key(c)[1:]) if pays(c) else after(c))
    # The free plays go to the dearest cards; a card that ends the turn (Void Form itself) goes last.
    dear = sorted((c for c in playable if c.cost > 0), key=lambda c: -c.cost)[:free]
    if ctx["doubled"] and playable and not any(c.type == "Power" for c in playable):
        dear.append(max(playable, key=lambda c: c.total_damage + c.block + 3 * c.draw))  # the card worth playing twice leads
    playable.sort(key=lambda c: (c.ends_turn, c not in dear))  # (stable: the rest keep their order)
    # Bound cards: one per turn (once one is played the game marks the rest unplayable, so any still playable means it is unused).
    bound_left = 1 if any(c.bound for c in playable) else None
    skill_left = 1 if _has_power(state.get("player") or {}, "SMOGGY") else None  # Smoggy ("You can only play 1 Skill per turn") the same way
    # Copies of a card that differ only in their place in the hand, and a line with nothing aimed at every aim, play out alike: simulated once.
    burning, sorts = {b[0] for b in ctx["burns"]}, {}
    sort_of = {id(c): (sorts.setdefault(c if c.index in burning else replace(c, index=-1), len(sorts)), bool(c.damage or c.needs_target or c.osty_attack or c.key in ("TIMES_UP", "TIME_S_UP"))) for c in playable}
    memo: dict = {}
    baseline = simulate([], enemies, **ctx)
    best_score, best_lost = (baseline.score, baseline.lost) if baseline else (float("-inf"), None)
    best_line: list[CardInfo] = []
    best_target: str | None = None
    # Each line is also tried aimed wholly at one enemy: when surrounded (whom we face decides who hits us from behind), and in any small group.
    aims: list = [None] + [("last", e.entity_id) for e in enemies if e.flanker] + [e.entity_id for e in enemies if e.flanker or (1 < len(enemies) <= 3 and e.incoming > 0)]
    # Order matters too (the Slow bonus, what an X card leaves): short lines are tried in every order, the usual one first so that it wins ties.
    budget = 2500
    for size in range(1, len(playable) + 1):
        found = False
        for line in combinations(playable, size):  # keeps order_key order
            if bound_left is not None and sum(c.bound for c in line) > bound_left:
                continue  # "Only 1 Bound card can be played each turn."
            if skill_left is not None and sum(c.type == "Skill" for c in line) > skill_left:
                continue
            orders = [line]
            if 1 < size <= 4:
                orders = permutations(line)  # starts with `line` itself
            for nth, order in enumerate(orders):
                if nth and budget <= 0:
                    break
                usable, sig = True, tuple(sort_of[id(c)] for c in order)
                for aim in aims:
                    if nth:
                        budget -= 1
                    key = (sig, aim if any(a for _, a in sig) else None)
                    if (out := memo.get(key, memo)) is memo:
                        out = memo[key] = simulate(list(order), enemies, **ctx, focus=aim)
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
        # Dead on paper with a Fortifier on the belt: it is drunk after the last card (potions._ill_timed), so the cards are chosen for the Block it will triple.
        braced: dict = {}
        with_it = choose_card({**state, "_fortify": 3}, braced)
        if braced.get("lost", plan["lost"]) < plan["lost"]:  # (the potions still go by the loss without it: the danger it is drunk for)
            card, plan = with_it, {**plan, "line": braced.get("line", [])}
    out = potion_actions(state, plan["lost"] if "lost" in plan else expected_end_loss(state), plan.get("line", []))  # (nothing playable: no plan)
    if card is not None:
        out.append(card)
    out.append(("end_turn", {}))
    return out
