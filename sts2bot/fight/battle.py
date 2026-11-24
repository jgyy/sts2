"""What a combat state says: the enemies, their intents and powers, and what our own hand will cost us (for combat.py's planner)."""

from __future__ import annotations

import re
from dataclasses import dataclass

from ..cards import CardInfo, parse
from ..run.crowd import rating as crowd_rating

LETHAL_PENALTY = 1_000_000.0
WIN_BONUS = 10_000.0
BEYOND_REACH = 100_000  # HP no enemy really has: the game's way of saying it cannot be hurt any more
REATTACH_REACH = 40  # Decimillipede: HP the other segments may have left for a kill to be the start of the end (they have two turns)
RAGE_BLOCK = 99  # Crab Rage: Block a crab gains for each ally that dies (gone again when its next turn starts)
RAGE_COST = 0.3  # ... and what the 6 Strength it keeps costs, in points for each HP it still has
SLIP_WORTH = 7.0  # what taking one Slippery stack off is worth, in damage: the hit it lets through later
NEXT_ENERGY, SPARE_ENERGY = 6.0, 1.0  # energy next turn, in damage: while the next hand has cards to spend it on, and beyond that (next_room)
RINGERS = ("CEREMONIAL_BEAST",)  # whose Debuff leaves us Ringing ("You can only play 1 card this turn") next turn
# The Writhing Mass draws a new intent whenever an Attack hits it: a little over the mean, or its worst when that could be the end of us.
REACTIVE_HIT, REACTIVE_WORST, REACTIVE_SAFE_HP = 20, 32, 40
STAB_SCAR = 8.0  # Painful Stabs (the Test Subject's second form): what each Wound from a hit that gets through costs, in points

ORB_VALUES = {"LIGHTNING": (3, 8), "FROST": (2, 5), "DARK": (6, 6), "PLASMA": (0, 0), "GLASS": (4, 8)}  # a new orb: (each turn, when evoked), before Focus
ENERGY_DRAIN = 20.0  # a Power that takes an energy away every turn (Fasting), in POWER_VALUE's units, per energy and per copy already in play
# What playing each Power is worth in an ordinary fight, in points of damage (more for elites and bosses); unlisted ones: a guess from the cost.
POWER_VALUE = {
    "DEMON_FORM": 24.0, "ROLLING_BOULDER": 18.0, "PYRE": 16.0, "CRIMSON_MANTLE": 12.0, "MAYHEM": 9.0, "STONE_ARMOR": 8.0, "AGGRESSION": 8.0, "UNMOVABLE": 7.0, "FASTEN": 6.0,
    "JUGGERNAUT": 6.0, "HELLRAISER": 6.0, "STAMPEDE": 5.0, "PANACHE": 4.0, "JUGGLING": 4.0, "VICIOUS": 4.0, "DARK_EMBRACE": 4.0, "STRATAGEM": 4.0, "BARRICADE": 4.0,
    "FEEL_NO_PAIN": 3.0, "RUPTURE": 3.0, "SERPENT_FORM": 16.0, "NOXIOUS_FUMES": 14.0, "AFTERIMAGE": 10.0, "CREATIVE_AI": 12.0, "ABRASIVE": 9.0, "HAILSTORM": 9.0,
    "INFINITE_BLADES": 8.0, "ENVENOM": 8.0, "FAN_OF_KNIVES": 8.0, "SEEKING_EDGE": 8.0, "CAPACITOR": 7.0, "LETHALITY": 7.0, "ACCURACY": 6.0, "TOOLS_OF_THE_TRADE": 6.0,
    "SPEEDSTER": 6.0, "PARRY": 6.0, "CHILD_OF_THE_STARS": 6.0, "CALL_OF_THE_VOID": 6.0, "TYRANNY": 6.0, "SPECTRUM_SHIFT": 6.0, "WELL_LAID_PLANS": 5.0, "TRACKING": 5.0,
    "SHROUD": 5.0, "HAUNT": 5.0, "ORBIT": 5.0, "STORM": 5.0, "THUNDER": 5.0, "ACCELERANT": 4.0, "PILLAR_OF_CREATION": 4.0, "PALE_BLUE_DOT": 4.0, "FERAL": 4.0,
    "ITERATION": 3.0, "TRASH_TO_TREASURE": 3.0, "ROYALTIES": 2.0, "ECHO_FORM": 22.0, "FRIENDSHIP": 12.0, "MACHINE_LEARNING": 10.0, "DEFRAGMENT": 10.0, "GENESIS": 9.0,
    "DEMESNE": 20.0, "FURNACE": 10.0, "COUNTDOWN": 8.0, "REAPER_FORM": 20.0, "VOID_FORM": 16.0, "SWORD_SAGE": 10.0, "ARSENAL": 8.0, "CRUELTY": 8.0, "MONARCHS_GAZE": 8.0,
    "AUTOMATION": 7.0, "DEVOUR_LIFE": 6.0, "SPIRIT_OF_ASH": 6.0, "COOLANT": 8.0, "SPINNER": 8.0, "BUFFER": 8.0, "SENTRY_MODE": 8.0, "NECRO_MASTERY": 8.0, "LOOP": 7.0,
    "BLACK_HOLE": 7.0, "SLEIGHT_OF_FLESH": 6.0, "NEUROSURGE": 6.0, "DANSE_MACABRE": 4.0, "SUBROUTINE": 3.0, "PAGESTORM": 2.0,
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
    stun_at: int = 0  # Plow, Shriek (the Terror Eel): Stunned once HP drops to this, and its move this turn is gone
    slow: int = -1  # Slow: % extra attack damage taken so far this turn, +10 per card we play (-1: none)
    minion: bool = False  # leaves the fight when its leader dies
    incoming_hits: int = 0  # how many separate hits make up `incoming`
    keeps_block: bool = False  # Burrowed: its Block stays until we break it, which Stuns it
    chipped: int = 0  # Block our attacks have removed so far in this plan
    stripped: int = 0  # ... and Slippery stacks our hits have taken off
    artifact: int = 0  # the next N debuffs we apply do nothing
    hit_cap: int = 0  # Hard to Kill: no single hit removes more HP than this
    enrage: int = 0  # Strength it gains whenever we play a Skill
    curious: int = 0  # ... a Power (Curiosity: the first game's Awakened One)
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
    spent: bool = False  # already beaten and out of reach: only its last blow is still to come
    adapts: bool = False  # Adaptable (the Test Subject): killed, it is gone for the turn and back, stronger, the next
    adapted: bool = False  # ... and this plan has done that
    asleep: int = 0  # Asleep: turns until it wakes by itself (losing HP wakes it at once)
    imbalanced: bool = False  # Imbalanced (Bowlbug Rock): Stunned next turn if its attack is blocked whole
    reacts: int = 0  # Reactive (the first game's Writhing Mass): an Attack that hits it draws a new intent; what to expect of that
    skittish: int = 0  # Skittish: Block it gains the first time it is hit this turn (0 once that has happened)
    hang: int = 0  # Hang: what the Hang cards' damage on it is multiplied by (2, 4, 8 ...; 0: none played yet)
    threat: float = 0.0  # damage a turn it has been doing in this fight (ordinary fights only; 0: not known)
    grows: float = 0.0  # Strength it gains a turn (Ritual, Territorial, or what its Strength so far comes to)
    reattach: int = 0  # Reattach (a Decimillipede segment): killed while another segment lives, it is back two turns later on this much HP
    hive: int = 0  # Personal Hive (the Entomancer): Dazed put into our draw pile for every Attack that hits it
    flutter: int = 0  # Flutter (the Thieving Hopper): Attack hits still to land before it is Stunned and takes full damage again
    thief: bool = False  # Swipe: it holds a card of ours, which comes back if it dies and is gone for good if it gets away
    shell: int = -1  # Hardened Shell (the Skulking Colony): HP it can still lose this turn (-1: no limit)
    focus: float = 1.0  # what a point of damage on it is worth next to one on the enemy to kill first (KNOWN_THREAT fights only)
    impaled: int = 0  # Impaled: our Arrow stands in it until this many more hits land (or it dies); 0: none

    @property
    def worth(self) -> float:
        """How much damaging this enemy brings the fight closer to its end."""
        if self.revives or self.spent:
            return 0.0
        if self.minion:
            return 1.0 if self.main_threat else 0.4
        return 1.3 if self.thief else 1.0

    @property
    def alive(self) -> bool:
        return self.hp > 0


def clone(e: Enemy, **changes) -> Enemy:
    """`dataclasses.replace` for an Enemy at a third of the cost: a plan copies the enemies thousands of times."""
    new = object.__new__(Enemy)
    new.__dict__.update(e.__dict__)
    new.__dict__.update(changes)
    return new


def _is_attack(intent: dict) -> bool:
    """Intents that hit us (the Waterfall Giant's last move, a "DeathBlow", too)."""
    kind = str(intent.get("type", "")).lower()
    return "attack" in kind or "deathblow" in kind


def intent_damage(intent: dict) -> int:
    if not _is_attack(intent):
        return 0
    for field in ("label", "description"):
        text = str(intent.get(field) or "")
        if m := re.search(r"(\d+)\s*[xX×*]\s*(\d+)", text):
            return int(m.group(1)) * int(m.group(2))
        if m := re.search(r"(\d+)", text):
            return int(m.group(1))
    return 0


def intent_hits(intent: dict) -> int:
    if not _is_attack(intent):
        return 0
    for field in ("label", "description"):
        if m := re.search(r"\d+\s*[xX×*]\s*(\d+)", str(intent.get(field) or "")):
            return int(m.group(1))
    return 1 if intent_damage(intent) else 0


def _shown(creature: dict, power_id: str, blank: int, missing=None):
    """The number the game shows on a power, named like VULNERABLE_POWER (`blank` if it shows none; `missing` if there is no such power)."""
    for p in creature.get("status") or []:
        if str(p.get("id", "")).upper().removesuffix("_POWER") == power_id:
            amount = p.get("amount")
            return int(amount) if isinstance(amount, (int, float)) else blank
    return missing


def power_amount(creature: dict, power_id: str) -> int:
    """Stacks of a power on a creature (1 for a power that shows no number)."""
    n = _shown(creature, power_id, 1)
    return 0 if n is None else n or 1


def _has_power(creature: dict, power_id: str) -> bool:
    return power_amount(creature, power_id) != 0


def _stacks(creature: dict, power_id: str) -> int:
    return max(0, power_amount(creature, power_id))


def end_of_turn_block(player: dict) -> int:
    """Block the player gains by itself when the turn ends (Plating)."""
    return _stacks(player, "PLATING")


# (\s*: the mod turns line breaks into spaces, sometimes two.)
_WITHER = re.compile(r"Wither(?:\+(\d+))?$")
_HAND_BURN = re.compile(r"at the end of your turn,\s*if this is in your hand,\s*(?:lose|take)\s+(\d+)\s+(?:hp|damage)", re.I)


def end_of_turn_loss(player: dict) -> int:
    """Damage we take when our turn ends, before the enemy moves (Disintegration). It eats into Block first."""
    return _stacks(player, "DISINTEGRATION")


def hand_burns(player: dict) -> tuple:
    """Cards that hurt if still in the hand when the turn ends: (hand index, HP, whether Block stops it ("take N damage", not "lose N HP")).
    An amount of -1 stands for Regret: 1 HP for every card left in the hand."""
    out = []
    for c in player.get("hand") or []:
        text = str(c.get("description") or "")
        m = _HAND_BURN.search(text)
        if re.search(r"if this is in your hand,\s*lose 1 hp for each card in your hand", text, re.I):  # (Regret first: its text begins like the others)
            out.append((int(c.get("index", -1)), -1, False))
        elif m:
            amount = int(m.group(1))
            if w := _WITHER.match(str(c.get("name") or "")):  # whatever the text says, a Wither has cost 3 for each level
                amount = max(amount, 3 * (int(w.group(1) or 0) + 1))
            out.append((int(c.get("index", -1)), amount, "damage" in m.group(0).lower()))
    return tuple(out)


def osty_hp(player: dict) -> int:
    """HP of the Necrobinder's pet, who takes attacks before she does."""
    return sum(int(p.get("hp") or 0) for p in player.get("pets") or [] if p.get("alive", True))


# Damage a turn some enemies do over a whole fight, where their first intents say much less: it orders whom to hit first (`Enemy.threat`).
KNOWN_THREAT = {"MAGI_KNIGHT": 22.0, "FLAIL_KNIGHT": 16.0, "SPECTRAL_KNIGHT": 12.0, "ROCKET": 20.0, "CRUSHER": 13.0, "BOWLBUG_NECTAR": 19.0}
FOCUS_FLOOR = 6.0  # damage a turn every enemy is taken to do at least, in that order (as combat.THREAT_FLOOR)
# Minions to kill before their leader although his death takes them along (most of the damage, in hits too small for read_enemies' rule).
MAIN_MINIONS = {"KIN_FOLLOWER"}


def _set_focus(enemies: list) -> None:
    """In a fight with a KNOWN_THREAT enemy, what damage on each is worth: by kill order (threat over health left), a point on the
    first brings all the deaths after it sooner and is worth the whole threat, one on the last only its own."""
    live = [e for e in enemies if e.alive and not e.minion and not e.spent]
    if len(live) < 2 or not any(e.threat and re.sub(r"_\d+$", "", e.entity_id) in KNOWN_THREAT for e in live):
        return
    left = lambda e: (max(1, e.hp - e.doom) + e.block) / (1.5 if e.vulnerable else 1.0)
    order = sorted(live, key=lambda e: -max(e.threat, FOCUS_FLOOR) / left(e))
    total = sum(max(e.threat, FOCUS_FLOOR) for e in order)
    for n, e in enumerate(order):
        e.focus = sum(max(o.threat, FOCUS_FLOOR) for o in order[n:]) / total


def read_enemies(state: dict) -> list[Enemy]:
    out = []
    player = state.get("player") or {}
    listed = (state.get("battle") or {}).get("enemies") or []
    raw = {str(e.get("entity_id")): e for e in listed}
    seen = (state.get("_threat") or {}) if state.get("state_type") == "monster" else {}
    rounds = max(1, int((state.get("battle") or {}).get("round") or 1) - 1)
    for e in listed:
        hp = int(e.get("hp") or 0)
        if hp <= 0:
            continue
        # Damage a turn so far; one that attacks and Buffs itself in the same move is growing: what it does now and half again.
        kinds = [str(i.get("type", "")).lower() for i in e.get("intents") or []]
        grows = any("buff" in k and "debuff" not in k for k in kinds) and any(map(_is_attack, e.get("intents") or []))
        threat = float(seen.get(str(e.get("entity_id")), 0.0))
        if grows and seen:
            threat = 1.5 * max(threat, float(sum(intent_damage(i) for i in e.get("intents") or [])))
        threat = max(threat, KNOWN_THREAT.get(re.sub(r"_\d+$", "", str(e.get("entity_id"))), 0.0))
        spent = hp >= BEYOND_REACH  # (the Waterfall Giant, once killed, stands on 999,999,999 HP for the turn it takes to blow up)
        out.append(
            Enemy(
                entity_id=str(e.get("entity_id")), hp=hp, spent=spent, block=int(e.get("block") or 0), vulnerable=_has_power(e, "VULNERABLE"), weak=_has_power(e, "WEAK"),
                vuln_turns=_stacks(e, "VULNERABLE"), weak_turns=_stacks(e, "WEAK"), slippery=_stacks(e, "SLIPPERY"),
                damage_mult=0.0 if spent else 0.5 if _has_power(e, "FLUTTER") or _has_power(e, "SOAR") else 1.0,
                hit_cap=1 if _has_power(e, "INTANGIBLE") else _stacks(e, "HARD_TO_KILL"),  # (Intangible: every hit takes 1 HP)
                enrage=_stacks(e, "ENRAGE"), curious=_stacks(e, "CURIOSITY"), thorns=_stacks(e, "THORNS"), revives=_has_power(e, "ILLUSION"),
                stun_at=_stacks(e, "PLOW") or _stacks(e, "SHRIEK"), shell=_shown(e, "HARDENED_SHELL", 0, -1), slow=_shown(e, "SLOW", 0, -1), minion=_has_power(e, "MINION"),
                keeps_block=_has_power(e, "BURROWED") or _has_power(e, "BARRICADE"), artifact=_stacks(e, "ARTIFACT"), poison=_stacks(e, "POISON"), doom=_stacks(e, "DOOM"),
                incoming=sum(intent_damage(i) for i in e.get("intents") or []), incoming_hits=sum(intent_hits(i) for i in e.get("intents") or []),
                vuln_mult=2.0 if _has_power(e, "DEBILITATE") else 1.5, weak_cut=0.5 if _has_power(e, "DEBILITATE") else 0.75, weak_mult=1.5 if _has_power(player, "TRACKING") else 1.0,
                scar=8.0 * _has_power(e, "PAPER_CUTS") + STAB_SCAR * _has_power(e, "PAINFUL_STABS"), rage=_has_power(e, "CRAB_RAGE"), respawns=_stacks(e, "STOCK"),
                adapts=_has_power(e, "ADAPTABLE"), asleep=_stacks(e, "ASLEEP"), imbalanced=_has_power(e, "IMBALANCED"),
                reacts=(REACTIVE_HIT if int(player.get("hp") or 0) >= REACTIVE_SAFE_HP else REACTIVE_WORST) + 3 * _stacks(e, "STRENGTH") if _has_power(e, "REACTIVE") else 0,
                hang=_stacks(e, "HANG"), threat=threat, hive=_stacks(e, "PERSONAL_HIVE"), flutter=_stacks(e, "FLUTTER"), thief=_has_power(e, "SWIPE"), reattach=_stacks(e, "REATTACH"),
                grows=max(float(_stacks(e, "RITUAL") + _stacks(e, "TERRITORIAL")), _stacks(e, "STRENGTH") / rounds),
                skittish=0 if int(e.get("block") or 0) else _stacks(e, "SKITTISH") or _stacks(e, "CURL_UP"), max_hp=int(e.get("max_hp") or 0), impaled=_stacks(e, "IMPALED"),
            )
        )
    # Surrounded: we face whoever we targeted last, and the one behind us hits 50% harder. The fight starts with us facing right.
    if _has_power(player, "SURROUNDED"):
        facing = state.get("_facing")
        for e in out:
            left = _has_power(raw[e.entity_id], "BACK_ATTACK_LEFT")
            e.flanker = left or _has_power(raw[e.entity_id], "BACK_ATTACK_RIGHT")
            if e.flanker:
                e.behind = e.entity_id != facing if facing in raw else left
    # Minions leave when their leader dies, so he is normally the target; not when a minion with far less HP deals all the damage.
    _set_focus(out)
    leaders = [e for e in out if not e.minion]
    if leaders:
        leader_hp = sum(e.hp + e.block for e in leaders)
        leader_hits = sum(e.incoming for e in leaders)
        for e in out:
            hard = e.incoming / (e.weak_cut if e.weak else 1.0) >= 15  # (its hit as it would be without our Weak)
            if e.minion and hard and e.incoming > 2 * leader_hits and e.hp + e.block <= 0.6 * leader_hp:
                e.main_threat = True
            elif e.minion and re.sub(r"_\d+$", "", e.entity_id) in MAIN_MINIONS:
                e.main_threat = True  # (worth what the leader is: the health order then takes it first while he is far from dead)
    return out


def _hit_count(e: Enemy, hits: int, double_if_vulnerable: bool = False) -> int:
    return hits * (2 if double_if_vulnerable and e.vulnerable else 1)


def _per_hit(e: Enemy, dmg_per_hit: int, raw: bool = False) -> int:
    """What one hit takes off this enemy's Block and HP (see `_hit`)."""
    per = dmg_per_hit * (e.damage_mult if not (raw and e.damage_mult) else 1.0)  # (Flutter and Soar: "less damage from Attacks")
    if not raw:
        per *= (e.vuln_mult if e.vulnerable else 1.0) * (e.weak_mult if e.weak else 1.0)
        if e.slow > 0:
            per *= 1.0 + e.slow / 100.0
    return int(per)


def _would_kill(e: Enemy, dmg_per_hit: int, hits: int, double_if_vulnerable: bool = False) -> bool:
    """Whether this attack leaves the enemy dead, without touching it (worked out directly in the plain case: it is asked very often)."""
    if e.skittish or e.hit_cap or e.slippery or e.shell >= 0 or e.flutter:
        trial = clone(e)
        _hit(trial, dmg_per_hit, hits, double_if_vulnerable)
        return not trial.alive
    return e.hp <= 0 or _per_hit(e, dmg_per_hit) * _hit_count(e, hits, double_if_vulnerable) >= e.hp + e.block


def _progress(e: Enemy, hp_removed: int, chip_rate: float) -> float:
    """How far an attack moved the fight on: HP removed, plus Block worn down (Block an enemy keeps between turns is as good as HP)."""
    chip, e.chipped = e.chipped, 0
    # A Slippery stack taken off is a later hit that lands in full.
    strip, e.stripped = e.stripped, 0
    hp_removed += strip * min(SLIP_WORTH, e.hp / 3.0)
    return hp_removed + chip * (1.0 if e.keeps_block or (e.incoming == 0 and chip and e.block == 0 and chip_rate >= 1) else chip_rate)


def _hit(e: Enemy, dmg_per_hit: int, hits: int, double_if_vulnerable: bool = False, raw: bool = False) -> int:
    """Apply an attack hit by hit, return HP actually removed. `raw` is damage that is not an Attack (an orb's): Vulnerable does nothing for it."""
    per = _per_hit(e, dmg_per_hit, raw)
    removed = 0
    for _ in range(_hit_count(e, hits, double_if_vulnerable)):
        if e.hp <= 0:
            break
        if e.impaled and not raw:
            e.impaled -= 1  # (Block or not)
        absorbed = min(e.block, per)
        e.block -= absorbed
        e.chipped += absorbed
        if absorbed and e.block == 0 and e.keeps_block:
            e.keeps_block = False
            e.incoming = 0  # "Stunned if all Block is removed."
        e.block, e.skittish = e.block + e.skittish, 0  # Skittish: the first hit of a turn lands, the rest meet its Block
        through = per - absorbed
        if through <= 0:
            continue
        if e.hit_cap:
            through = min(through, e.hit_cap)
        if e.slippery > 0:
            e.slippery -= 1
            e.stripped += 1
            through = 1
        lost = min(e.hp, through)
        if e.shell >= 0:  # "Cannot lose more than 20 HP each turn", counted down as it goes
            lost = min(lost, e.shell)
            e.shell -= lost
        e.hp -= lost
        removed += lost
        if e.stun_at and e.hp <= e.stun_at:
            e.stun_at = 0
            e.incoming = 0  # Stunned: its attack this turn never happens
        if e.flutter and not raw:
            # Flutter ("Deal attack damage 5 times to Stun it"): the fifth hit takes away this turn's move, and the 50% off our Attacks.
            e.flutter -= 1
            if not e.flutter:
                e.incoming = e.incoming_hits = 0
                e.damage_mult, per = 1.0, per * 2
    return removed


def kill_reach(survivors: list, blow_is_all: bool) -> float:
    """HP still in the way of the kill that would save a turn lost on paper (infinite: none would): every leader, or every attacker
    when the attacks are all that kills us. Poison bites before they move."""
    def left(group: list) -> float:
        if not group or any(e.spent or e.hit_cap or e.slippery or e.respawns for e in group):
            return float("inf")
        return float(sum(max(0, e.hp - e.poison) + e.block for e in group))

    leaders = [e for e in survivors if not e.minion]
    attackers = [e for e in survivors if e.incoming > 0]
    return min(left(leaders), left(attackers) if blow_is_all else float("inf"))


def constrict(player: dict) -> int:
    """Damage we take at the end of our turn while the Strangler lives."""
    return _stacks(player, "CONSTRICT")


def expected_loss(state: dict) -> int:
    """HP we lose this turn if we do nothing more."""
    player = state.get("player") or {}
    incoming = sum(e.incoming for e in read_enemies(state))
    return max(0, incoming - int(player.get("block") or 0) - end_of_turn_block(player) - osty_hp(player))


def _sandpit(state: dict) -> int | None:
    """Turns until the Sandpit swallows us (None when there is none)."""
    creatures = [state.get("player") or {}, *((state.get("battle") or {}).get("enemies") or [])]
    return next((n for c in creatures if (n := _shown(c, "SANDPIT", 1)) is not None), None)


def _escape_odds(player: dict, hand_draw: int = 5) -> tuple:
    """Chance that the next hand holds no Frantic Escape (from the piles as they are), and a hand a few turns on (from the whole deck)."""
    is_escape = lambda c: str(c.get("name") or "").startswith("Frantic Escape")

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


def deck_count(player: dict) -> tuple:
    """(cards we are cycling through this fight: hand, draw and discard piles; how many of them are dead weight)."""
    piles = [c for pile in ("hand", "draw_pile", "discard_pile") for c in (player.get(pile) if isinstance(player.get(pile), list) else [])]
    if not isinstance(player.get("draw_pile"), list):
        return (len(piles) + int(player.get("draw_pile_count") or 0) + int(player.get("discard_pile_count") or 0), 0)
    dead = lambda c: bool(re.search(r"\bunplayable\b", str(c.get("description") or ""), re.I)) or str(c.get("description") or "").strip() == "Exhaust."
    return (len(piles), sum(map(dead, piles)))


def junk_cost(fight: str, round_no: int, deck: tuple, added: int = 1) -> float:
    """What `added` more dead cards in the deck cost over the rest of the fight, in damage: each takes a card's place in a hand, every shuffle."""
    cards, junk = deck
    turns = max(1, {"boss": 10, "elite": 6}.get(fight, 4) - round_no)
    size = max(10, cards + added)
    return added * (turns * 5.0 / size) * 3.0 * (1.0 + 2.0 * (junk + added) / size)


def _bare_block(player: dict) -> int:
    """Block a relic gives when the turn ends without any (Orichalcum)."""
    return next((int(m.group(1)) for r in player.get("relics") or [] if (m := re.search(r"end your turn without block, gain (\d+) block", str(r.get("description") or ""), re.I))), 0)


def _draw_worth(player: dict) -> tuple:
    """What drawing a card is worth right now, from what is left to draw: (points if free, share that cost energy, points per energy)."""
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
    """Damage the next Wither does if still in the hand when the turn ends: 3 a level ("Wither+2" does 9); Aeonglass adds a level every third round."""
    level = max(0, (round_no - 1) // 3)
    for pile in ("hand", "draw_pile", "discard_pile", "exhaust_pile"):
        for c in player.get(pile) or []:
            if m := _WITHER.match(str(c.get("name") or "")):
                level = max(level, int(m.group(1) or 0))
    return 3 * (level + 1)


WITHER_FIGHT = 11  # the round an Aeonglass fight is over by when it is won


def _wither_bill(player: dict, round_no: int, wither_hp: int) -> float:
    """What each card played costs in Withers still to come, in points: a sixth of what one does as it comes round again with the deck."""
    deck = len(player.get("hand") or []) + sum(int(player.get(p + "_count") or len(player.get(p) or [])) for p in ("draw_pile", "discard_pile"))
    again = min(1.5, max(1, WITHER_FIGHT - round_no) / max(3.0, deck / 5.0))  # times it comes round again
    return again * (2.5 * (wither_hp + 3) + 6.0) / 6.0


def cards_left_this_turn(state: dict) -> int | None:
    """How many more cards we may play this turn, when something limits it."""
    player = state.get("player") or {}
    played = int(state.get("_plays_this_turn") or 0)
    limits = []
    if _has_power(player, "RINGING"):  # "You can only play 1 card this turn."
        limits.append(1)
    if _has_power(player, "SLOTH"):  # "You cannot play more than 3 cards each turn." (by the number on the power: a card played twice counts twice)
        limits.append(played + 3 - max(0, _shown(player, "SLOTH", 0) or 0))
    for c in [*(player.get("hand") or []), *(player.get("relics") or [])]:  # (a relic too: Velvet Choker, 6 a turn)
        if m := re.search(r"cannot play more than (\d+) cards", str(c.get("description") or ""), re.I):
            limits.append(int(m.group(1)))
    left = [max(0, n - played) for n in limits]
    # The first game's Time Eater counts every card we play (Time Warp, kept from turn to turn): the 12th ends our turn.
    left += [12 - max(0, warp) for e in (state.get("battle") or {}).get("enemies") or [] if (warp := _shown(e, "TIME_WARP", 0)) is not None]
    encore = _shown(player, "ENCORE", 1)  # "You can only play 1 more card this turn": the game counts it down, 1 then 0
    if encore is not None:
        left.append(max(0, encore))
    return min(left) if left else None


def next_room(state: dict) -> tuple:
    """(Energy the next hand can spend beyond what it will have already, what each card kept in the hand adds to that): five cards
    of the draw pile at their mean cost (X and over 3 as 3, Unplayable as 0) against the energy coming. None before a Ringing turn."""
    player = state.get("player") or {}
    for e in (state.get("battle") or {}).get("enemies") or []:
        if re.sub(r"_\d+$", "", str(e.get("entity_id"))) in RINGERS and any("debuff" in str(i.get("type", "")).lower() for i in e.get("intents") or []):
            return (-99.0, 0.0)
    pile = list(player.get("draw_pile") or [])
    if len(pile) < 5:
        pile += list(player.get("discard_pile") or [])  # (shuffled back in before the hand is drawn)
    if not pile:
        return (99.0, 1.0)  # (no piles in the state: every energy counts)

    def cost(c: dict) -> float:
        if re.search(r"\bunplayable\b", str(c.get("description") or ""), re.I):
            return 0.0
        n = str(c.get("cost") or "0")
        return min(3.0, float(n)) if n.isdigit() else 3.0

    per = sum(map(cost, pile)) / len(pile)
    draws = 5 + max(0, power_amount(player, "DRAW_CARDS_NEXT_TURN")) - max(0, power_amount(player, "MIND_ROT"))  # ("Draw 1 fewer card each turn")
    have = int(player.get("max_energy") or 3) + max(0, power_amount(player, "ENERGY_NEXT_TURN"))
    caps = [3] if _has_power(player, "SLOTH") else []  # (the Knowledge Demon's: "You cannot play more than 3 cards each turn")
    caps += [int(m.group(1)) for r in player.get("relics") or [] if (m := re.search(r"cannot play more than (\d+) cards", str(r.get("description") or ""), re.I))]
    if caps and draws >= min(caps):
        # The plays a turn allows are used up by what it draws: a Retained hand adds no play, it only stands in for a card drawn.
        return (min(caps) * per - have, 0.0)
    return (draws * per - have, per)


def next_energy_worth(gained: int, room: tuple, kept: int = 0) -> float:
    """What `gained` energy next turn is worth, in damage, with `kept` cards retained in the hand (next_room's `room`)."""
    usable = min(float(gained), max(0.0, room[0] + kept * room[1]))
    return NEXT_ENERGY * usable + SPARE_ENERGY * (gained - usable)


def free_plays(state: dict) -> int:
    """Void Form ("The first 2 cards you play each turn are free to play"): how many of the next cards cost nothing (a cost on show: none)."""
    player = state.get("player") or {}
    if any(str(c.get("cost") or "0").strip() not in ("0", "X") for c in player.get("hand") or []):
        return 0
    return max(0, _stacks(player, "VOID_FORM") - int(state.get("_plays_this_turn") or 0))


def _resists(e: Enemy) -> bool:
    """Artifact swallows a debuff (and a charge goes with it)."""
    if e.artifact > 0:
        e.artifact -= 1
        return True
    return False


def _poison_total(stack: int, turns: int) -> int:
    """HP a Poison stack takes over the next `turns` turns (it loses 1 a turn)."""
    stack = max(0, stack)
    if stack >= turns:
        return turns * stack - turns * (turns - 1) // 2
    return stack * (stack + 1) // 2


def _power_worth(c: CardInfo, long_fight: bool = True) -> float:
    """What playing a Power is worth in an ordinary fight: from the table, else a guess from its cost; in a long fight, not less than crowd.py says."""
    worth = POWER_VALUE.get(c.key, 5.0 + 2.0 * max(c.cost, 0))
    crowd = crowd_rating(c.key) if long_fight else None
    return worth if crowd is None else max(worth, 2.5 * max(c.cost, 1) + 2.0 * (crowd - 4.0))


def _set_up(playable: list[CardInfo], energy: int) -> tuple | None:
    """A card worth playing with nobody to aim at (the enemy is gone for the turn and comes back): whatever lasts, and draws while energy is left."""
    best, best_worth = None, 0.0
    for c in playable:
        if c.needs_target or c.type in ("Status", "Curse") or c.deadly or c.block_lock or (c.is_x and energy <= 0):
            continue
        x, cost = (energy, energy) if c.is_x else (1, c.cost)
        worth = _power_worth(c) if c.type == "Power" else 0.0
        worth += (4.0 * len(c.channel) + c.summon + 1.5 * c.souls) * x + 0.5 * c.forge + 2.5 * c.star_gain
        worth += 4.0 * c.strength * c.strength_lasts + 3.5 * c.dexterity * c.dexterity_lasts - ENERGY_DRAIN * c.energy_drain
        worth += 5.0 * c.next_energy + 1.5 * c.next_draw + 0.8 * c.next_block
        if energy - cost > 0 or c.energy_gain:
            worth += 1.0 * c.draw + 2.0 * c.energy_gain
        worth -= 3.0 * c.hp_loss
        worth /= cost + 0.5
        if worth > best_worth:
            best, best_worth = c, worth
    return ("play_card", {"card_index": best.index}) if best is not None else None


def _respawns(foes: list[Enemy]) -> None:
    """Stock: an enemy that dies while it has Stock is summoned again at full HP (not a kill). Adaptable: gone for the turn, back stronger."""
    for e in foes:
        if not e.alive and e.respawns > 0 and e.max_hp > 0:
            e.hp, e.block = e.max_hp, 0
            e.respawns -= 1
            e.incoming = e.incoming_hits = 0  # the new one spends this turn on Defend and Buff: the attack of the one we killed is gone
        elif not e.alive and e.adapts:  # the Test Subject's first two forms: out of reach for the rest of the turn, and its attack with it
            e.hp, e.block, e.spent, e.damage_mult = BEYOND_REACH, 0, True, 0.0
            e.incoming = e.incoming_hits = 0
            e.poison = e.doom = 0
            e.adapts, e.adapted = False, True


def _rage_blocks(foes: list[Enemy], known_dead: set) -> None:
    """Crab Rage, as the line plays out: each ally that dies gives the crabs still standing 99 Block for the rest of this turn."""
    newly = [e for e in foes if not e.alive and e.entity_id not in known_dead]
    if not newly:
        return
    known_dead.update(e.entity_id for e in newly)
    for e in foes:
        if e.alive and e.rage:
            e.block += RAGE_BLOCK * len(newly)


def loss_cap(player: dict) -> int:
    """Beating Remnant: "You cannot lose more than 20 HP in a single turn." (0: no such relic)"""
    return next((int(m.group(1)) for r in player.get("relics") or [] if (m := re.search(r"cannot lose more than (\d+) HP in a single turn", str(r.get("description") or ""), re.I))), 0)


THREAT_FLOOR = 6.0  # damage a turn every enemy is taken to do at least, when choosing whom to hit first


def _pick_target(enemies: list[Enemy], dmg_per_hit: int, hits: int, double_if_vulnerable: bool = False) -> Enemy | None:
    """Whom an attack goes at."""
    alive = [e for e in enemies if e.alive]
    if not alive:
        return None
    # Reattach: a segment killed while another lives comes back, so until the others are within reach none is finished: they die together.
    segs = [e for e in alive if e.reattach]
    stay = {e.entity_id for e in segs if sum(o.hp + o.block for o in segs if o is not e) > REATTACH_REACH} if len(segs) > 1 else set()
    if stay:
        spare = [e for e in segs if not (e.entity_id in stay and _would_kill(e, dmg_per_hit, hits, double_if_vulnerable))]
        if len(segs) == len(alive):
            return max(spare or segs, key=lambda e: e.hp + e.block)
    killable = [e for e in alive if e.entity_id not in stay and _would_kill(e, dmg_per_hit, hits, double_if_vulnerable)]
    if killable:
        # The biggest threat first, wasting the least damage; not one that revives unless it is about to hit us, nor an idle minion hit with twice its size.
        useful = [e for e in killable if e.worth > 0 or e.incoming > 0]
        size = dmg_per_hit * hits
        useful = [e for e in useful if not (e.minion and not e.main_threat and not e.incoming and size > 2 * (e.hp + e.block) and any(o.worth >= 1.0 for o in alive))]
        if useful:
            return max(useful, key=lambda e: (e.worth > 0, e.incoming, e.hp + e.block))
    # Else whoever dies soonest for the damage it does (Doom as health gone), leaders first, not one Doomed already or whose Block swallows it all.
    soaks = lambda e: e.block >= dmg_per_hit * hits and not e.keeps_block
    left = lambda e: (max(1, e.hp - e.doom) + e.block) / (1.5 if e.vulnerable else 1.0)
    return min(alive, key=lambda e: (_fated(e), -e.worth, soaks(e), left(e) / max(e.threat, THREAT_FLOOR), -e.incoming))


def _fated(e: Enemy) -> bool:
    """Doom covers its HP: it dies when its turn ends, whatever else we do to it (a boss with another form to come does not)."""
    return e.doom >= e.hp > 0 and not e.adapts
