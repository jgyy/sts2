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
from itertools import combinations

from .cards import CardInfo, parse

Action = tuple[str, dict]

log = logging.getLogger("sts2bot.combat")

MAX_SEARCH_CARDS = 11
LETHAL_PENALTY = 1_000_000.0
WIN_BONUS = 10_000.0


# What playing each Power is worth in an ordinary fight, in points of damage
# (the planner multiplies it for elites and bosses, where it pays for longer).
# Powers that are not listed get a guess from their cost.
# What a freshly channeled orb does: (each turn, when evoked). Focus changes
# these; the live numbers for orbs already held come from the game.
ORB_VALUES = {"LIGHTNING": (3, 8), "FROST": (2, 5), "DARK": (6, 6), "PLASMA": (0, 0), "GLASS": (4, 8)}

POWER_VALUE = {
    "DEMON_FORM": 24.0, "ROLLING_BOULDER": 18.0, "PYRE": 16.0, "CRIMSON_MANTLE": 12.0, "MAYHEM": 9.0,
    "STONE_ARMOR": 8.0, "AGGRESSION": 8.0, "UNMOVABLE": 7.0, "FASTEN": 6.0, "JUGGERNAUT": 6.0, "HELLRAISER": 6.0,
    "STAMPEDE": 5.0, "PANACHE": 4.0, "JUGGLING": 4.0, "VICIOUS": 4.0, "DARK_EMBRACE": 4.0, "STRATAGEM": 4.0,
    "BARRICADE": 4.0, "FEEL_NO_PAIN": 3.0, "RUPTURE": 3.0,
    # other characters
    "SERPENT_FORM": 16.0, "NOXIOUS_FUMES": 14.0, "AFTERIMAGE": 10.0, "CREATIVE_AI": 10.0, "ABRASIVE": 9.0,
    "HAILSTORM": 9.0, "INFINITE_BLADES": 8.0, "ENVENOM": 8.0, "FAN_OF_KNIVES": 8.0, "SEEKING_EDGE": 8.0,
    "CAPACITOR": 7.0, "LETHALITY": 7.0, "ACCURACY": 6.0, "TOOLS_OF_THE_TRADE": 6.0, "SPEEDSTER": 6.0,
    "PARRY": 6.0, "CHILD_OF_THE_STARS": 6.0, "CALL_OF_THE_VOID": 6.0, "TYRANNY": 6.0, "SPECTRUM_SHIFT": 6.0,
    "WELL_LAID_PLANS": 5.0, "TRACKING": 5.0, "SHROUD": 5.0, "HAUNT": 5.0, "ORBIT": 5.0, "STORM": 5.0,
    "THUNDER": 5.0, "ACCELERANT": 4.0, "PILLAR_OF_CREATION": 4.0, "PALE_BLUE_DOT": 4.0, "FERAL": 4.0,
    "ITERATION": 3.0, "TRASH_TO_TREASURE": 3.0, "ROYALTIES": 2.0,
}


@dataclass
class Enemy:
    entity_id: str
    hp: int
    block: int
    vulnerable: bool
    weak: bool
    incoming: int  # damage this enemy intends to deal this turn
    slippery: int = 0  # the next N hits only remove 1 HP each
    damage_mult: float = 1.0  # e.g. Flutter: 50% less damage from attacks
    thorns: int = 0  # damage we take back per hit
    revives: bool = False  # Illusion: comes back next turn at full HP
    stun_at: int = 0  # Plow: Stunned (and loses its Strength) once HP drops to this
    slow: int = -1  # Slow: % extra attack damage taken so far this turn, +10 per card we play (-1: none)
    minion: bool = False  # leaves the fight when its leader dies
    incoming_hits: int = 0  # how many separate hits make up `incoming`
    keeps_block: bool = False  # Burrowed: its Block stays until we break it, which Stuns it
    chipped: int = 0  # Block our attacks have removed so far in this plan
    artifact: int = 0  # the next N debuffs we apply do nothing
    hit_cap: int = 0  # Hard to Kill: no single hit removes more HP than this
    enrage: int = 0  # Strength it gains whenever we play a Skill
    vuln_turns: int = 0  # turns of Vulnerable / Weak it has left
    weak_turns: int = 0
    vuln_added: int = 0  # ... and how many of those this plan added
    weak_added: int = 0
    poison: int = 0  # Poison on it: loses that much HP at the start of its turn, before it acts
    doom: int = 0  # Doom on it: dies at the end of its turn once Doom covers its HP
    flanker: bool = False  # Back Attack: hits 50% harder from behind us
    behind: bool = False  # ... and is behind us right now (its intent already shows the bonus)

    main_threat: bool = False  # a minion that is doing the real damage and is quicker to kill than its leader

    @property
    def worth(self) -> float:
        """How much damaging this enemy brings the fight closer to its end."""
        if self.revives:
            return 0.0
        if self.minion:
            return 1.0 if self.main_threat else 0.4
        return 1.0

    @property
    def alive(self) -> bool:
        return self.hp > 0


def intent_damage(intent: dict) -> int:
    if "attack" not in str(intent.get("type", "")).lower():
        return 0
    for field in ("label", "description"):
        text = str(intent.get(field) or "")
        m = re.search(r"(\d+)\s*[xX×*]\s*(\d+)", text)
        if m:
            return int(m.group(1)) * int(m.group(2))
        m = re.search(r"(\d+)", text)
        if m:
            return int(m.group(1))
    return 0


def intent_hits(intent: dict) -> int:
    if "attack" not in str(intent.get("type", "")).lower():
        return 0
    for field in ("label", "description"):
        m = re.search(r"\d+\s*[xX×*]\s*(\d+)", str(intent.get(field) or ""))
        if m:
            return int(m.group(1))
    return 1 if intent_damage(intent) else 0


def power_amount(creature: dict, power_id: str) -> int:
    """Stacks of a power on a creature. The game names them like VULNERABLE_POWER."""
    for p in creature.get("status") or []:
        pid = str(p.get("id", "")).upper()
        if pid.endswith("_POWER"):
            pid = pid[: -len("_POWER")]
        if pid == power_id:
            amount = p.get("amount")
            return int(amount) if isinstance(amount, (int, float)) and amount else 1
    return 0


def _slow_percent(creature: dict) -> int:
    for p in creature.get("status") or []:
        if str(p.get("id", "")).upper() in ("SLOW", "SLOW_POWER"):
            amount = p.get("amount")
            return int(amount) if isinstance(amount, (int, float)) else 0
    return -1


def _has_power(creature: dict, power_id: str) -> bool:
    return power_amount(creature, power_id) != 0


def end_of_turn_block(player: dict) -> int:
    """Block the player gains by itself when the turn ends (Plating)."""
    return max(0, power_amount(player, "PLATING"))


_HAND_BURN = re.compile(r"at the end of your turn, if this is in your hand, (?:lose|take) (\d+) (?:hp|damage)", re.I)


def end_of_turn_loss(player: dict) -> int:
    """HP that goes at the end of the turn no matter what: Disintegration, and
    curses that hurt while they sit in the hand (they cannot be played away).
    Leaving it out made the bot think it could afford a hit that killed it."""
    loss = max(0, power_amount(player, "DISINTEGRATION")) if _has_power(player, "DISINTEGRATION") else 0
    for c in player.get("hand") or []:
        m = _HAND_BURN.search(str(c.get("description") or ""))
        if m and not c.get("can_play", True):
            loss += int(m.group(1))
    return loss


def osty_hp(player: dict) -> int:
    """HP of the Necrobinder's pet, who takes attacks before she does."""
    return sum(int(p.get("hp") or 0) for p in player.get("pets") or [] if p.get("alive", True))


def read_enemies(state: dict) -> list[Enemy]:
    out = []
    for e in (state.get("battle") or {}).get("enemies") or []:
        hp = int(e.get("hp") or 0)
        if hp <= 0:
            continue
        out.append(
            Enemy(
                entity_id=str(e.get("entity_id")),
                hp=hp,
                block=int(e.get("block") or 0),
                vulnerable=_has_power(e, "VULNERABLE"),
                weak=_has_power(e, "WEAK"),
                vuln_turns=max(0, power_amount(e, "VULNERABLE")),
                weak_turns=max(0, power_amount(e, "WEAK")),
                slippery=max(0, power_amount(e, "SLIPPERY")),
                damage_mult=0.5 if _has_power(e, "FLUTTER") or _has_power(e, "SOAR") else 1.0,
                hit_cap=max(0, power_amount(e, "HARD_TO_KILL")),
                enrage=max(0, power_amount(e, "ENRAGE")),
                thorns=max(0, power_amount(e, "THORNS")),
                revives=_has_power(e, "ILLUSION"),
                stun_at=max(0, power_amount(e, "PLOW")),
                slow=_slow_percent(e),
                minion=_has_power(e, "MINION"),
                keeps_block=_has_power(e, "BURROWED") or _has_power(e, "BARRICADE"),
                artifact=max(0, power_amount(e, "ARTIFACT")),
                poison=max(0, power_amount(e, "POISON")),
                doom=max(0, power_amount(e, "DOOM")),
                incoming=sum(intent_damage(i) for i in e.get("intents") or []),
                incoming_hits=sum(intent_hits(i) for i in e.get("intents") or []),
            )
        )
    # Surrounded: we face whoever we targeted last, and the one behind us hits
    # 50% harder. The fight starts with us facing right.
    if _has_power(state.get("player") or {}, "SURROUNDED"):
        facing = state.get("_facing")
        raw = {str(e.get("entity_id")): e for e in (state.get("battle") or {}).get("enemies") or []}
        for e in out:
            left = _has_power(raw.get(e.entity_id) or {}, "BACK_ATTACK_LEFT")
            right = _has_power(raw.get(e.entity_id) or {}, "BACK_ATTACK_RIGHT")
            e.flanker = left or right
            if e.flanker:
                e.behind = e.entity_id != facing if facing in raw else left

    # Minions leave when their leader dies, so the leader is normally the
    # target. Not when the leader only buffs and one minion with far less HP
    # deals all the damage: against the Queen the bot hit her 400 HP while her
    # 199 HP minion took 30 to 50 HP off it every turn.
    leaders = [e for e in out if not e.minion]
    if leaders:
        leader_hp = sum(e.hp + e.block for e in leaders)
        leader_hits = sum(e.incoming for e in leaders)
        for e in out:
            if e.minion and e.incoming >= 15 and e.incoming > 2 * leader_hits and e.hp + e.block <= 0.6 * leader_hp:
                e.main_threat = True
    return out


def _order_key(c: CardInfo) -> tuple:
    if c.type == "Power" or c.key == "FRANTIC_ESCAPE":
        rank = 0  # set-up first; an escape must not be left for energy that is gone
    elif c.draw or c.energy_gain:
        rank = 1
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


def _hit(e: Enemy, dmg_per_hit: int, hits: int, double_if_vulnerable: bool = False) -> int:
    """Apply an attack hit by hit, return HP actually removed."""
    per = dmg_per_hit * (1.5 if e.vulnerable else 1.0) * e.damage_mult
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
        e.incoming = int(e.incoming * 0.75)


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
    focus: str | None = None,  # aim every targeted card at this enemy while it lives
    orbs: tuple = (),  # (kind, passive, evoke) for each orb, oldest first
    orb_slots: int = 0,
    shiv_damage: int = 4,
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

    for n, c in enumerate(line):
        if c.star_cost > stars:
            return None
        for e in foes:
            if e.slow >= 0:
                e.slow += 10  # counts the card being played, like the game does
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
            block += (c.block + nimble) * x
        if can_block and c.summon:
            # Osty stands in front of the Necrobinder: his HP soaks hits like
            # Block that does not go away at the end of the turn.
            block += c.summon * x
            lasting += c.summon * x * 0.25
        if c.block_lock:
            # Trading away future block is only worth it to survive this turn.
            score -= 30.0
        if c.deadly:
            score -= LETHAL_PENALTY  # never gamble the run on one card

        if c.damage:
            per, hits = c.damage + might, c.hits * x
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
                    score += c.doom * x * 0.6 * dmg_value * e.worth
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
        score += c.star_gain * 1.5 + c.souls * 2.5
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
                    score += old[2] * 0.8 * dmg_value
            passive, burst = ORB_VALUES.get(kind, (3, 6))
            held.append((kind, passive, burst))
            lasting += {"FROST": 3.0, "LIGHTNING": 3.0, "DARK": 4.0, "PLASMA": 6.0, "GLASS": 4.0}.get(kind, 3.0)
        for _ in range(c.evoke):
            if held:
                kind, passive, burst = held[-1]
                if kind == "FROST" and can_block:
                    block += burst
                elif kind in ("LIGHTNING", "DARK", "GLASS"):
                    score += burst * 0.8 * dmg_value
                elif kind == "PLASMA":
                    energy += 2
        if c.evoke and held:
            held.pop()  # evoking uses the orb up
        if c.key == "FRANTIC_ESCAPE":
            escapes += 1
        if c.type == "Skill":
            # Enrage: every Skill makes each of its hits stronger for the rest of the fight.
            score -= sum(e.enrage * 3.0 * max(1, e.incoming_hits) for e in foes if e.alive)
        score += c.draw * 2.0
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

    # Breaking Block that an enemy was keeping Stuns it.
    score += 15.0 * sum(1 for e in foes if e.alive and e.entity_id in kept_block and not e.keeps_block)
    # The Defect's orbs act when the turn ends: Frost adds Block, Lightning hits something.
    for kind, passive, _burst in held:
        if kind == "FROST" and can_block:
            block += passive
        elif kind in ("LIGHTNING", "GLASS"):
            score += passive * 0.7 * dmg_value
    score += lasting * span + later
    # Poison bites at the start of the enemy's turn, before it acts: one that
    # is poisoned through is as good as dead now. Otherwise it is damage on
    # its way, a little less each turn.
    poison_turns = {"boss": 4.0, "elite": 3.0}.get(fight, 2.0)
    for e, orig in zip(foes, enemies):
        if not e.alive:
            continue
        if e.poison >= e.hp:
            e.hp = 0
            continue
        added = e.poison - orig.poison
        if added > 0:
            score += min(e.hp, added * poison_turns) * e.worth
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
    kills = [e for e, orig in zip(foes, enemies) if not e.alive and orig.alive]
    score += 25.0 * sum(e.worth for e in kills)
    survivors = [e for e in foes if e.alive]
    # The fight is over once every leader is dead: minions leave with them.
    if not any(not e.minion for e in survivors) and any(not e.minion for e in enemies):
        score += WIN_BONUS
        survivors = []
    elif not survivors:
        score += WIN_BONUS

    if sandpit is not None and survivors:
        # The Insatiable: Sandpit counts down every turn and at 0 we are eaten,
        # whatever our HP. Each Frantic Escape played pushes it back by one.
        left = sandpit + escapes
        if left <= 1:
            score -= LETHAL_PENALTY
        else:
            score -= {2: 60.0, 3: 25.0, 4: 8.0}.get(left, 0.0)

    if faced is not None:
        # Turning to face an enemy takes away its 50% bonus and gives it to the other one.
        for e in survivors:
            if e.flanker and e.behind and e.entity_id == faced:
                e.incoming = round(e.incoming / 1.5)
            elif e.flanker and not e.behind and e.entity_id != faced:
                e.incoming = round(e.incoming * 1.5)

    incoming = sum(e.incoming for e in survivors)
    lost = max(0, incoming - block) + self_loss + end_loss
    ratio = hp / max(1, max_hp)
    # A point of our HP is worth more than a point of enemy HP even at full
    # health: at 1.0 a Strike (6) beat a Defend (5) and the bot never blocked
    # until it was already hurt, bleeding HP in every ordinary fight.
    # ... and worth more still when this one turn would take a big bite out
    # of what is left: at 37 HP facing 27 the bot attacked with two Defends
    # in hand and was dead a turn later.
    bite = min(1.0, lost / max(1, hp))
    score -= lost * (1.5 + 2.5 * (1.0 - ratio) + 3.0 * bite)
    if lost >= hp:
        score -= LETHAL_PENALTY
    return Outcome(score, first_target)


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
        end_loss=end_of_turn_loss(player),
        orbs=tuple(
            (
                str(o.get("id") or o.get("name") or "").upper().replace("_ORB", ""),
                int(o.get("passive_val") or 0),
                int(o.get("evoke_val") or 0),
            )
            for o in player.get("orbs") or []
        ),
        orb_slots=int(player.get("orb_slots") or 0),
        shiv_damage=4 + max(0, power_amount(player, "ACCURACY")),
    )


def _sandpit(state: dict) -> int | None:
    """Turns until the Sandpit swallows us (None when there is none)."""
    creatures = [state.get("player") or {}, *((state.get("battle") or {}).get("enemies") or [])]
    for c in creatures:
        for p in c.get("status") or []:
            if str(p.get("id", "")).upper() in ("SANDPIT", "SANDPIT_POWER"):
                amount = p.get("amount")
                return int(amount) if isinstance(amount, (int, float)) else 1
    return None


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
    if not playable or not enemies:
        return None
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
    for size in range(1, len(playable) + 1):
        found = False
        for line in combinations(playable, size):  # keeps _order_key order
            if bound_left is not None and sum(c.bound for c in line) > bound_left:
                continue  # "Only 1 Bound card can be played each turn."
            for aim in aims:
                out = simulate(list(line), enemies, **ctx, focus=aim)
                if out is None:
                    break
                found = True
                if out.score > best_score + 1e-9:
                    best_score, best_line, best_target = out.score, list(line), out.first_target
        if not found:
            break
    if not best_line:
        return None

    card = best_line[0]
    params: dict = {"card_index": card.index}
    if card.needs_target:
        params["target"] = best_target or min(enemies, key=lambda e: e.hp + e.block).entity_id
    return ("play_card", params)


def expected_loss(state: dict) -> int:
    """HP we lose this turn if we do nothing more."""
    player = state.get("player") or {}
    incoming = sum(e.incoming for e in read_enemies(state))
    return max(0, incoming - int(player.get("block") or 0) - end_of_turn_block(player) - osty_hp(player))


_DEFENSIVE_POTION = re.compile(r"gain \d+ (block|dexterity)|apply \d+ weak|loses? \d+ strength", re.I)


def potion_actions(state: dict) -> list[Action]:
    """Potions worth drinking now: always in elite/boss fights, otherwise when in danger."""
    player = state.get("player") or {}
    enemies = read_enemies(state)
    if not enemies:
        return []
    hp = int(player.get("hp") or 1)
    max_hp = int(player.get("max_hp") or 1)
    # Ordinary fights only get a potion in real trouble; the rest are kept
    # for elites and bosses, where the runs are actually lost.
    danger = expected_loss(state) >= 0.5 * hp or hp <= 0.3 * max_hp
    if not (danger or state.get("state_type") in ("elite", "boss")):
        return []
    # Aim at something that stays dead and matters: the leader that hits hardest.
    focus = max(enemies, key=lambda e: (e.worth, e.incoming, e.hp)).entity_id
    out: list[Action] = []
    for p in player.get("potions") or []:
        if not p.get("can_use_in_combat", True):
            continue
        if "SNECKO" in f"{p.get('id')} {p.get('name')}".upper().replace(" ", "_"):
            continue  # random card costs wreck a planner that counts on its energy
        if _DEFENSIVE_POTION.search(str(p.get("description") or "")) and expected_loss(state) < min(12, hp):
            continue  # Block, Weak and the like are for the turn something big is coming
        params: dict = {"slot": int(p.get("slot", 0))}
        if str(p.get("target_type")) == "AnyEnemy":
            params["target"] = focus
        out.append(("use_potion", params))
    return out


def describe(state: dict) -> str:
    """One line with everything the planner looked at (for the debug log)."""
    player = state.get("player") or {}
    battle = state.get("battle") or {}
    foes = []
    for e in battle.get("enemies") or []:
        powers = ",".join(f"{str(s.get('id', '')).replace('_POWER', '')}:{s.get('amount')}" for s in e.get("status") or [])
        intents = "+".join(f"{i.get('type')}{i.get('label') or ''}" for i in e.get("intents") or [])
        foes.append(f"{e.get('entity_id')} {e.get('hp')}hp/{e.get('block')}blk [{powers}] {intents}")
    def read(c: dict) -> str:
        """What the planner makes of the card, e.g. " d6x2 b5" (so a misread shows in the log)."""
        i = parse(c)
        bits = [f"d{i.damage}" + (f"x{i.hits}" if i.hits > 1 else "")] if i.damage else []
        bits += [f"b{i.block}"] if i.block else []
        return (" " + " ".join(bits)) if bits else ""

    hand = ", ".join(
        f"{c.get('index')}:{c.get('name')}({c.get('cost')}{'' if c.get('can_play', True) else '!'}{read(c)})"
        for c in player.get("hand") or []
    )
    mine = ",".join(f"{str(s.get('id', '')).replace('_POWER', '')}:{s.get('amount')}" for s in player.get("status") or [])
    extra = ""
    if player.get("stars") is not None:
        extra += f" stars {player.get('stars')}"
    if player.get("orbs") is not None:
        extra += " orbs " + ("".join(str(o.get("name") or "?")[0] for o in player.get("orbs") or []) or "-")
    if player.get("pets"):
        extra += f" osty {osty_hp(player)}"
    return (
        f"r{battle.get('round')} hp {player.get('hp')}/{player.get('max_hp')} blk {player.get('block')} "
        f"en {player.get('energy')}{extra} [{mine}] | {' ; '.join(foes)} | hand {hand}"
    )


def actions(state: dict) -> list[Action]:
    """Ordered candidate actions for a combat state (first workable one is taken)."""
    if log.isEnabledFor(logging.DEBUG):
        log.debug("combat %s", describe(state))
    out = potion_actions(state)
    card = choose_card(state)
    if card is not None:
        out.append(card)
    out.append(("end_turn", {}))
    return out
