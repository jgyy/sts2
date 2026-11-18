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

from .cards import CardInfo, parse
from .learn import knob

Action = tuple[str, dict]

log = logging.getLogger("sts2bot.combat")

MAX_SEARCH_CARDS = 11
LETHAL_PENALTY = 1_000_000.0
WIN_BONUS = 10_000.0
RAGE_BLOCK = 99  # Crab Rage: Block a crab gains for each ally that dies (gone again when its next turn starts)


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
    "ECHO_FORM": 14.0, "FRIENDSHIP": 12.0, "MACHINE_LEARNING": 10.0, "DEFRAGMENT": 10.0, "GENESIS": 9.0,
    "COOLANT": 8.0, "SPINNER": 8.0, "BUFFER": 8.0, "SENTRY_MODE": 8.0, "NECRO_MASTERY": 8.0, "LOOP": 7.0,
    "BLACK_HOLE": 7.0, "SLEIGHT_OF_FLESH": 6.0, "NEUROSURGE": 6.0, "DANSE_MACABRE": 4.0, "SUBROUTINE": 3.0,
    "PAGESTORM": 2.0,
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

    vuln_mult: float = 1.5  # what Vulnerable multiplies our attacks by (Debilitate doubles its effect)
    weak_cut: float = 0.75  # what Weak multiplies its attacks by
    weak_mult: float = 1.0  # Tracking: our attacks hit a Weak enemy this much harder
    rage: bool = False  # Crab Rage: gains 6 Strength and 99 Block when an ally dies
    scar: float = 0.0  # extra cost of each of its hits that gets through (Paper Cuts: Max HP; Painful Stabs: a Wound)

    main_threat: bool = False  # a minion that is doing the real damage and is quicker to kill than its leader
    respawns: int = 0  # Stock: when killed it is summoned again in its place, at full HP, this many more times
    max_hp: int = 0  # its full HP (what a Stock summon comes back with)

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
    """HP that goes at the end of the turn no matter what we play (Disintegration).
    Leaving it out made the bot think it could afford a hit that killed it."""
    return max(0, power_amount(player, "DISINTEGRATION")) if _has_power(player, "DISINTEGRATION") else 0


def hand_burns(player: dict) -> tuple:
    """Cards that hurt if they are still in the hand when the turn ends, as
    (hand index, HP). Most cannot be played; Toxic can (1 energy, Exhaust),
    and the bot sat on three of them, taking 15 a turn, with energy to spare.
    An amount of -1 stands for Regret: 1 HP for every card left in the hand."""
    out = []
    for c in player.get("hand") or []:
        text = str(c.get("description") or "")
        m = _HAND_BURN.search(text)
        if m:
            out.append((int(c.get("index", -1)), int(m.group(1))))
        elif re.search(r"if this is in your hand, lose 1 hp for each card in your hand", text, re.I):
            out.append((int(c.get("index", -1)), -1))
    return tuple(out)


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
                # Intangible: every hit takes 1 HP, whatever its size.
                hit_cap=1 if _has_power(e, "INTANGIBLE") else max(0, power_amount(e, "HARD_TO_KILL")),
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
                vuln_mult=2.0 if _has_power(e, "DEBILITATE") else 1.5,
                weak_cut=0.5 if _has_power(e, "DEBILITATE") else 0.75,
                weak_mult=1.5 if _has_power(state.get("player") or {}, "TRACKING") else 1.0,
                scar=8.0 * _has_power(e, "PAPER_CUTS") + 4.0 * _has_power(e, "PAINFUL_STABS"),
                rage=_has_power(e, "CRAB_RAGE"),
                respawns=max(0, power_amount(e, "STOCK")),
                max_hp=int(e.get("max_hp") or 0),
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


def _hit(e: Enemy, dmg_per_hit: int, hits: int, double_if_vulnerable: bool = False) -> int:
    """Apply an attack hit by hit, return HP actually removed."""
    per = dmg_per_hit * (e.vuln_mult if e.vulnerable else 1.0) * (e.weak_mult if e.weak else 1.0) * e.damage_mult
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
    burns: tuple = (),  # (hand index, HP) for cards that hurt while they stay in the hand
    hand_size: int = 0,
    focus: str | None = None,  # aim every targeted card at this enemy while it lives
    orbs: tuple = (),  # (kind, passive, evoke) for each orb, oldest first
    orb_slots: int = 0,
    shiv_damage: int = 4,
    tender: bool = False,  # every card played costs 1 Strength and 1 Dexterity for the turn
    vigor: int = 0,  # extra damage on the next Attack only (the card text shows it on all of them)
    giant: bool = False,  # the next Attack deals triple damage (same)
    buffer: int = 0,  # times we lose no HP when we otherwise would
    taint: int = 0,  # Vital Spark: each Skill played makes every hit on us this turn 2 harder
    draw_worth: tuple = (2.0, 0.0, 0.0),  # of a drawn card: (points if free, share that cost energy, points per energy)
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
    zaps = 0
    for kind, passive, _burst in held:
        if kind == "FROST" and can_block:
            block += passive
        elif kind in ("LIGHTNING", "GLASS"):
            score += passive * 0.7 * dmg_value
            zaps += passive
    # With one enemy left there is no doubt whom the Lightning hits, and it
    # hits before the enemy acts: an enemy on 3 HP is as good as dead.
    standing = [e for e in foes if e.alive]
    if zaps and len(standing) == 1 and not standing[0].slippery and not standing[0].hit_cap:
        if standing[0].hp + standing[0].block <= zaps:
            standing[0].hp = 0
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
    _respawns(foes)  # a Stock enemy killed by the last card comes back too
    kills = [e for e, orig in zip(foes, enemies) if not e.alive and orig.alive]
    score += 25.0 * sum(e.worth for e in kills)
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
            score -= {2: 60.0, 3: 25.0, 4: 8.0}.get(left, 0.0)

    if faced is not None:
        # Turning to face an enemy takes away its 50% bonus and gives it to the other one.
        for e in survivors:
            if e.flanker and e.behind and e.entity_id == faced:
                e.incoming = round(e.incoming / 1.5)
            elif e.flanker and not e.behind and e.entity_id != faced:
                e.incoming = round(e.incoming * 1.5)

    incoming = sum(e.incoming + tainted * e.incoming_hits for e in survivors)
    through = max(0, incoming - block)
    if through and buffer >= sum(e.incoming_hits for e in survivors if e.incoming):
        through = 0  # Buffer swallows each hit that would have hurt
    if through:
        score -= sum(e.scar * e.incoming_hits for e in survivors if e.incoming)
    played = {c.index for c in line}
    left = max(0, hand_size - len(line))
    burnt = sum((left if amount < 0 else amount) for index, amount in burns if index not in played)
    if wither_in > 0 and len(line) >= wither_in:
        # A Wither sits in the hand and burns when the turn ends, then comes
        # round again with the deck: the sixth card has to be worth that.
        burnt += wither_hp * (1 + (len(line) - wither_in) // 6)
    if wither_in > 0:
        # ... and it stays in the deck: it burns again every time it is drawn
        # and takes a card's place in the hand (by round 7 one fight had five
        # of them in an eight-card hand, 25 HP a turn). The count carries over
        # between turns, so every card played is a sixth of that bill.
        score -= len(line) * (4.0 * wither_hp + 20.0) / 6.0
    lost = through + self_loss + end_loss + burnt
    ratio = hp / max(1, max_hp)
    # Osty keeps the HP he is not hit for: unlike Block it is still there next
    # turn (and Unleash hits harder for it). The bot never summoned on a turn
    # without an attack coming, which is exactly when it costs nothing.
    score += 1.1 * min(summoned, max(0, block - incoming))
    # Every leader left is Doomed to die at the end of its turn: the fight is
    # won if we live through that turn, so more damage is wasted energy.
    doomed = bool(survivors) and all(e.doom >= e.hp for e in survivors if not e.minion) and any(not e.minion for e in survivors)
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
        end_loss=end_of_turn_loss(player) + constrict(player),
        burns=hand_burns(player),
        hand_size=len(player.get("hand") or []),
        wither_in=max((power_amount(e, "WITHERING_PRESENCE") for e in battle.get("enemies") or []), default=0),
        wither_hp=_next_wither(player),
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
        shiv_damage=4 + max(0, power_amount(player, "ACCURACY")),
        tender=_has_power(player, "TENDER"),
        vigor=max(0, power_amount(player, "VIGOR")),
        giant=_has_power(player, "GIGANTIFICATION"),
        buffer=max(0, power_amount(player, "BUFFER")),
        taint=2 if any(_has_power(e, "VITAL_SPARK") for e in [player, *(battle.get("enemies") or [])]) else 0,
    )


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


def _next_wither(player: dict) -> int:
    """HP the next Wither takes: 3 for the first, one more for each made so far
    (the cards are named "Wither+2" and so on). Guessing a flat 5 let the bot
    play a sixth card at 8 HP into two Wither+3s."""
    worst = -1
    for pile in ("hand", "draw_pile", "discard_pile", "exhaust_pile"):
        for c in player.get(pile) or []:
            m = re.match(r"Wither(?:\+(\d+))?$", str(c.get("name") or ""))
            if m:
                worst = max(worst, int(m.group(1) or 0))
    return 3 + worst + 1 + 2  # the next one up, and room for a card that plays another card


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


def constrict(player: dict) -> int:
    """Damage we take at the end of our turn while the Strangler lives."""
    return max(0, power_amount(player, "CONSTRICT")) if _has_power(player, "CONSTRICT") else 0


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
    potions = player.get("potions") or []
    # A full belt cannot take the next potion a fight drops, so one of them
    # is free to use: holding three potions through a whole act wasted every
    # potion reward on the way.
    spare = 1 if len(potions) >= int(player.get("max_potion_slots") or 3) else 0
    big_fight = danger or state.get("state_type") in ("elite", "boss")
    if not (big_fight or spare):
        return []
    # Aim at something that stays dead and matters: the leader that hits hardest.
    focus = max(enemies, key=lambda e: (e.worth, e.incoming, e.hp)).entity_id
    out: list[Action] = []
    slippery = any(e.slippery for e in enemies)
    for p in potions:
        if not p.get("can_use_in_combat", True):
            continue
        if not big_fight and len(out) >= spare:
            break
        if slippery and re.search(r"deals? \d+ damage", str(p.get("description") or ""), re.I):
            continue  # every hit on a Slippery enemy takes 1 HP: keep it for later
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
