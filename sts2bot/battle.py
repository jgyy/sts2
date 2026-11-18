"""What a combat state says: the enemies, their intents and powers, and what our own hand will cost us.

The planner in combat.py works on what is read here.
"""

from __future__ import annotations

import re
from dataclasses import dataclass

LETHAL_PENALTY = 1_000_000.0
WIN_BONUS = 10_000.0
SANDPIT_DEATH = 600.0  # what being eaten by the Sandpit costs, in points (the run ends there)
BEYOND_REACH = 100_000  # HP no enemy really has: the game's way of saying it cannot be hurt any more
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
    spent: bool = False  # already beaten and out of reach: only its last blow is still to come
    adapts: bool = False  # Adaptable (the Test Subject): killed, it is gone for the turn and back, stronger, the next
    adapted: bool = False  # ... and this plan has done that

    @property
    def worth(self) -> float:
        """How much damaging this enemy brings the fight closer to its end."""
        if self.revives or self.spent:
            return 0.0
        if self.minion:
            return 1.0 if self.main_threat else 0.4
        return 1.0

    @property
    def alive(self) -> bool:
        return self.hp > 0


def _is_attack(intent: dict) -> bool:
    """Intents that hit us. The Waterfall Giant's last move is a "DeathBlow":
    read as no attack at all, the bot spent that turn hitting a boss that was
    already dead and died to the blast with two Defends in hand."""
    kind = str(intent.get("type", "")).lower()
    return "attack" in kind or "deathblow" in kind


def intent_damage(intent: dict) -> int:
    if not _is_attack(intent):
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
    if not _is_attack(intent):
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


# (The mod turns line breaks into spaces: Beckon reads "... in your Hand,  lose 6 HP." with two.
# With one space in the pattern the Soul Fysh's Beckons went unseen, 6 HP a card every turn.)
_HAND_BURN = re.compile(r"at the end of your turn,\s*if this is in your hand,\s*(?:lose|take)\s+(\d+)\s+(?:hp|damage)", re.I)


def end_of_turn_loss(player: dict) -> int:
    """Damage we take when our turn ends, before the enemy moves (Disintegration). It eats
    into Block first. Leaving it out made the bot think it could afford a hit that killed it."""
    return max(0, power_amount(player, "DISINTEGRATION")) if _has_power(player, "DISINTEGRATION") else 0


def hand_burns(player: dict) -> tuple:
    """Cards that hurt if they are still in the hand when the turn ends, as
    (hand index, HP, whether Block stops it). Most cannot be played; Toxic and
    Beckon can (1 energy), and the bot sat on three Toxics, taking 15 a turn,
    with energy to spare. "Take N damage" goes through our Block first (two
    Wither+2 behind 30 Block and a 26 attack cost 14, not 18); "lose N HP"
    does not. An amount of -1 stands for Regret: 1 HP for every card left in
    the hand."""
    out = []
    for c in player.get("hand") or []:
        text = str(c.get("description") or "")
        m = _HAND_BURN.search(text)
        if m:
            amount = int(m.group(1))
            w = re.match(r"Wither(?:\+(\d+))?$", str(c.get("name") or ""))
            if w:  # whatever the text says, a Wither has cost 3 for each level
                amount = max(amount, 3 * (int(w.group(1) or 0) + 1))
            out.append((int(c.get("index", -1)), amount, "damage" in m.group(0).lower()))
        elif re.search(r"if this is in your hand,\s*lose 1 hp for each card in your hand", text, re.I):
            out.append((int(c.get("index", -1)), -1, False))
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
        # The Waterfall Giant, once killed, stands there on 999,999,999 HP for
        # the turn it takes to blow up. Nothing we do to it counts any more.
        spent = hp >= BEYOND_REACH
        out.append(
            Enemy(
                entity_id=str(e.get("entity_id")),
                hp=hp,
                spent=spent,
                block=int(e.get("block") or 0),
                vulnerable=_has_power(e, "VULNERABLE"),
                weak=_has_power(e, "WEAK"),
                vuln_turns=max(0, power_amount(e, "VULNERABLE")),
                weak_turns=max(0, power_amount(e, "WEAK")),
                slippery=max(0, power_amount(e, "SLIPPERY")),
                damage_mult=0.0 if spent else 0.5 if _has_power(e, "FLUTTER") or _has_power(e, "SOAR") else 1.0,
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
                scar=8.0 * _has_power(e, "PAPER_CUTS") + 8.0 * _has_power(e, "PAINFUL_STABS"),
                rage=_has_power(e, "CRAB_RAGE"),
                respawns=max(0, power_amount(e, "STOCK")),
                adapts=_has_power(e, "ADAPTABLE"),
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


def constrict(player: dict) -> int:
    """Damage we take at the end of our turn while the Strangler lives."""
    return max(0, power_amount(player, "CONSTRICT")) if _has_power(player, "CONSTRICT") else 0


def expected_loss(state: dict) -> int:
    """HP we lose this turn if we do nothing more."""
    player = state.get("player") or {}
    incoming = sum(e.incoming for e in read_enemies(state))
    return max(0, incoming - int(player.get("block") or 0) - end_of_turn_block(player) - osty_hp(player))
