"""What a combat state says: the enemies, their intents and powers, and what our own hand will cost us.

The planner in combat.py works on what is read here.
"""

from __future__ import annotations

import re
from dataclasses import dataclass

from .cards import CardInfo, parse
from .crowd import rating as crowd_rating

LETHAL_PENALTY = 1_000_000.0
WIN_BONUS = 10_000.0
BEYOND_REACH = 100_000  # HP no enemy really has: the game's way of saying it cannot be hurt any more
REATTACH_REACH = 40  # Decimillipede: HP the other segments may have left for a kill to be the start of the end (they have two turns)
RAGE_BLOCK = 99  # Crab Rage: Block a crab gains for each ally that dies (gone again when its next turn starts)
RAGE_COST = 0.3  # ... and what the 6 Strength it keeps costs, in points for each HP it still has (2.0 until 2026-11-21: see combat.simulate)
SLIP_WORTH = 7.0  # what taking one Slippery stack off is worth, in damage: the hit it lets through later


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
    "SERPENT_FORM": 16.0, "NOXIOUS_FUMES": 14.0, "AFTERIMAGE": 10.0, "CREATIVE_AI": 12.0, "ABRASIVE": 9.0,
    "HAILSTORM": 9.0, "INFINITE_BLADES": 8.0, "ENVENOM": 8.0, "FAN_OF_KNIVES": 8.0, "SEEKING_EDGE": 8.0,
    "CAPACITOR": 7.0, "LETHALITY": 7.0, "ACCURACY": 6.0, "TOOLS_OF_THE_TRADE": 6.0, "SPEEDSTER": 6.0,
    "PARRY": 6.0, "CHILD_OF_THE_STARS": 6.0, "CALL_OF_THE_VOID": 6.0, "TYRANNY": 6.0, "SPECTRUM_SHIFT": 6.0,
    "WELL_LAID_PLANS": 5.0, "TRACKING": 5.0, "SHROUD": 5.0, "HAUNT": 5.0, "ORBIT": 5.0, "STORM": 5.0,
    "THUNDER": 5.0, "ACCELERANT": 4.0, "PILLAR_OF_CREATION": 4.0, "PALE_BLUE_DOT": 4.0, "FERAL": 4.0,
    "ITERATION": 3.0, "TRASH_TO_TREASURE": 3.0, "ROYALTIES": 2.0,
    "ECHO_FORM": 22.0, "FRIENDSHIP": 12.0, "MACHINE_LEARNING": 10.0, "DEFRAGMENT": 10.0, "GENESIS": 9.0,
    # (Energy and a card every turn; 5 more on the Sovereign Blade every turn; 6 Doom every turn.)
    "DEMESNE": 20.0, "FURNACE": 10.0, "COUNTDOWN": 8.0,
    # (Doom for every point of attack damage: half as much again to kill; two free cards every turn;
    # the Sovereign Blade twice; Strength for every card made; a quarter more from Vulnerable enemies.)
    "REAPER_FORM": 20.0, "VOID_FORM": 16.0, "SWORD_SAGE": 10.0, "ARSENAL": 8.0, "CRUELTY": 8.0, "MONARCHS_GAZE": 8.0,
    "AUTOMATION": 7.0, "DEVOUR_LIFE": 6.0, "SPIRIT_OF_ASH": 6.0,
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
    skittish: int = 0  # Skittish: Block it gains the first time it is hit this turn (0 once that has happened)
    hang: int = 0  # Hang: what the Hang cards' damage on it is multiplied by (2, 4, 8 ...; 0: none played yet)
    threat: float = 0.0  # damage a turn it has been doing in this fight (ordinary fights only; 0: not known)
    grows: float = 0.0  # Strength it gains a turn (Ritual, Territorial, or what its Strength so far comes to)
    reattach: int = 0  # Reattach (a Decimillipede segment): killed while another segment lives, it is back two turns later on this much HP
    hive: int = 0  # Personal Hive (the Entomancer): Dazed put into our draw pile for every Attack that hits it
    flutter: int = 0  # Flutter (the Thieving Hopper): Attack hits still to land before it is Stunned and takes full damage again
    thief: bool = False  # Swipe: it holds a card of ours, which comes back if it dies and is gone for good if it gets away
    shell: int = -1  # Hardened Shell (the Skulking Colony): HP it can still lose this turn (-1: no limit)

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


def _shown(creature: dict, power_id: str, blank: int, missing=None):
    """The number the game shows on a power (`blank` if it shows none; `missing` if the creature has no such power).

    The game names them like VULNERABLE_POWER."""
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


# (The mod turns line breaks into spaces: Beckon reads "... in your Hand,  lose 6 HP." with two.
# With one space in the pattern the Soul Fysh's Beckons went unseen, 6 HP a card every turn.)
_WITHER = re.compile(r"Wither(?:\+(\d+))?$")
_HAND_BURN = re.compile(r"at the end of your turn,\s*if this is in your hand,\s*(?:lose|take)\s+(\d+)\s+(?:hp|damage)", re.I)


def end_of_turn_loss(player: dict) -> int:
    """Damage we take when our turn ends, before the enemy moves (Disintegration). It eats
    into Block first. Leaving it out made the bot think it could afford a hit that killed it."""
    return _stacks(player, "DISINTEGRATION")


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
        # (Regret first: its text begins like the others, "... lose 1 HP", and read as 1 it cost 5 with five cards held.)
        if re.search(r"if this is in your hand,\s*lose 1 hp for each card in your hand", text, re.I):
            out.append((int(c.get("index", -1)), -1, False))
        elif m:
            amount = int(m.group(1))
            w = _WITHER.match(str(c.get("name") or ""))
            if w:  # whatever the text says, a Wither has cost 3 for each level
                amount = max(amount, 3 * (int(w.group(1) or 0) + 1))
            out.append((int(c.get("index", -1)), amount, "damage" in m.group(0).lower()))
    return tuple(out)


def osty_hp(player: dict) -> int:
    """HP of the Necrobinder's pet, who takes attacks before she does."""
    return sum(int(p.get("hp") or 0) for p in player.get("pets") or [] if p.get("alive", True))


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
        # What it has done a turn so far in this fight. One that attacks and Buffs itself in the same
        # move is growing: what it does now and half as much again, whatever it did while it slept.
        kinds = [str(i.get("type", "")).lower() for i in e.get("intents") or []]
        grows = any("buff" in k and "debuff" not in k for k in kinds) and any(map(_is_attack, e.get("intents") or []))
        threat = float(seen.get(str(e.get("entity_id")), 0.0))
        if grows and seen:
            threat = 1.5 * max(threat, float(sum(intent_damage(i) for i in e.get("intents") or [])))
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
                vuln_turns=_stacks(e, "VULNERABLE"),
                weak_turns=_stacks(e, "WEAK"),
                slippery=_stacks(e, "SLIPPERY"),
                damage_mult=0.0 if spent else 0.5 if _has_power(e, "FLUTTER") or _has_power(e, "SOAR") else 1.0,
                # Intangible: every hit takes 1 HP, whatever its size.
                hit_cap=1 if _has_power(e, "INTANGIBLE") else _stacks(e, "HARD_TO_KILL"),
                enrage=_stacks(e, "ENRAGE"),
                thorns=_stacks(e, "THORNS"),
                revives=_has_power(e, "ILLUSION"),
                # (Shriek: the Eel on 77 before a 22 was hit to 64, Stunned, after 12 Block had gone up for nothing.)
                stun_at=_stacks(e, "PLOW") or _stacks(e, "SHRIEK"),
                shell=_shown(e, "HARDENED_SHELL", 0, -1),
                slow=_shown(e, "SLOW", 0, -1),
                minion=_has_power(e, "MINION"),
                keeps_block=_has_power(e, "BURROWED") or _has_power(e, "BARRICADE"),
                artifact=_stacks(e, "ARTIFACT"),
                poison=_stacks(e, "POISON"),
                doom=_stacks(e, "DOOM"),
                incoming=sum(intent_damage(i) for i in e.get("intents") or []),
                incoming_hits=sum(intent_hits(i) for i in e.get("intents") or []),
                vuln_mult=2.0 if _has_power(e, "DEBILITATE") else 1.5,
                weak_cut=0.5 if _has_power(e, "DEBILITATE") else 0.75,
                weak_mult=1.5 if _has_power(player, "TRACKING") else 1.0,
                scar=8.0 * _has_power(e, "PAPER_CUTS") + 8.0 * _has_power(e, "PAINFUL_STABS"),
                rage=_has_power(e, "CRAB_RAGE"),
                respawns=_stacks(e, "STOCK"),
                adapts=_has_power(e, "ADAPTABLE"),
                asleep=_stacks(e, "ASLEEP"),
                imbalanced=_has_power(e, "IMBALANCED"),
                hang=_stacks(e, "HANG"),
                threat=threat,
                hive=_stacks(e, "PERSONAL_HIVE"),
                flutter=_stacks(e, "FLUTTER"),
                thief=_has_power(e, "SWIPE"),
                reattach=_stacks(e, "REATTACH"),
                grows=max(float(_stacks(e, "RITUAL") + _stacks(e, "TERRITORIAL")), _stacks(e, "STRENGTH") / rounds),
                # (Block on it: it has been hit already. Curl Up, the Louse Progenitor's, is the same once a fight: 14.)
                skittish=0 if int(e.get("block") or 0) else _stacks(e, "SKITTISH") or _stacks(e, "CURL_UP"),
                max_hp=int(e.get("max_hp") or 0),
            )
        )
    # Surrounded: we face whoever we targeted last, and the one behind us hits
    # 50% harder. The fight starts with us facing right.
    if _has_power(player, "SURROUNDED"):
        facing = state.get("_facing")
        for e in out:
            left = _has_power(raw[e.entity_id], "BACK_ATTACK_LEFT")
            e.flanker = left or _has_power(raw[e.entity_id], "BACK_ATTACK_RIGHT")
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


def _hit_count(e: Enemy, hits: int, double_if_vulnerable: bool = False) -> int:
    return hits * (2 if double_if_vulnerable and e.vulnerable else 1)


def _hit(e: Enemy, dmg_per_hit: int, hits: int, double_if_vulnerable: bool = False, raw: bool = False) -> int:
    """Apply an attack hit by hit, return HP actually removed.

    `raw` is damage that is not an Attack (an orb's): Vulnerable does nothing
    for it (a Lightning evoked twice did 16 to a Vulnerable enemy, 18 times in 18).
    """
    per = dmg_per_hit * (e.damage_mult if not (raw and e.damage_mult) else 1.0)  # (Flutter and Soar: "less damage from Attacks")
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
        # Skittish (Phantasmal Gardener): the first hit of a turn lands, and the rest meet 6 Block.
        # Four of them took 11 rounds and 38 of 51 HP from a bot that hit one twice a turn.
        e.block, e.skittish = e.block + e.skittish, 0
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
        if e.shell >= 0:
            # "Cannot lose more than 20 HP each turn", and the power counts down as it goes: at 0 two more
            # attacks were played into it for nothing, with its hit still to come.
            lost = min(lost, e.shell)
            e.shell -= lost
        e.hp -= lost
        removed += lost
        if e.stun_at and e.hp <= e.stun_at:
            e.stun_at = 0
            e.incoming = 0  # Stunned: its attack this turn never happens
        if e.flutter and not raw:
            # Flutter, the Thieving Hopper's ("Deal attack damage 5 times to Stun it"): the fifth hit takes away
            # this turn's move, its 21 or its Escape with our card, and the 50% off our Attacks with it. Unread,
            # three Defends went up against the 21 with a 27-damage attack in hand and the Hopper two turns
            # from leaving on 18 HP; it left on 6, and the deck was a card short from then on.
            e.flutter -= 1
            if not e.flutter:
                e.incoming = e.incoming_hits = 0
                e.damage_mult, per = 1.0, per * 2
    return removed


def kill_reach(survivors: list, blow_is_all: bool) -> float:
    """HP still in the way of the kill that would save a turn lost on paper (infinite: no kill would).

    Either every leader dies (the fight is over) or, when the attacks are all
    that kills us, everything that is about to attack. Poison bites before
    they move.
    """
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


def deck_count(player: dict) -> tuple:
    """(cards we are cycling through this fight: hand, draw and discard piles; how many of them are dead weight)."""
    piles = [c for pile in ("hand", "draw_pile", "discard_pile") for c in (player.get(pile) if isinstance(player.get(pile), list) else [])]
    if not isinstance(player.get("draw_pile"), list):
        return (len(piles) + int(player.get("draw_pile_count") or 0) + int(player.get("discard_pile_count") or 0), 0)
    dead = lambda c: bool(re.search(r"\bunplayable\b", str(c.get("description") or ""), re.I)) or str(c.get("description") or "").strip() == "Exhaust."
    return (len(piles), sum(map(dead, piles)))


def junk_cost(fight: str, round_no: int, deck: tuple, added: int = 1) -> float:
    """What `added` more dead cards in the deck cost over the rest of the fight, in damage.

    Each comes round again with every shuffle and takes a card's place in the
    hand: half a Strike a time, more as the deck fills up with them. A Regent
    played Crash Landing ("Fill your Hand with Debris") on rounds 3, 8 and 9
    of a boss fight and by round 12 drew three Debris in a hand of five.
    """
    cards, junk = deck
    turns = max(1, {"boss": 10, "elite": 6}.get(fight, 4) - round_no)
    size = max(10, cards + added)
    return added * (turns * 5.0 / size) * 3.0 * (1.0 + 2.0 * (junk + added) / size)


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
            m = _WITHER.match(str(c.get("name") or ""))
            if m:
                level = max(level, int(m.group(1) or 0))
    return 3 * (level + 1)


WITHER_FIGHT = 11  # the round an Aeonglass fight is over by when it is won (10 and 11; the lost ones end on 7)


def _wither_bill(player: dict, round_no: int, wither_hp: int) -> float:
    """What each card played costs in Withers still to come, in points: a sixth of what one does after its first turn.

    A Wither goes round with the deck, so it burns again (a level worse by then, and Block may stop it)
    and takes a card's place for as often as it is drawn before the fight ends: once or twice for one
    made on round 2, hardly at all for one made on round 8. Until 2026-11-21 this was (4 x its damage
    + 20) / 6 whatever the round, which grows as the fight runs out: 5.3 a card on rounds 1-3, 7.3 on
    4-6 and 9.3 from round 7, when a Strike is worth 9 and six of nine lost fights ended.
    """
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
    if _has_power(player, "SLOTH"):  # "You cannot play more than 3 cards each turn."
        # (By the game's own count, the number on the power: a card that is played twice counts twice.)
        limits.append(played + 3 - max(0, _shown(player, "SLOTH", 0) or 0))
    for c in [*(player.get("hand") or []), *(player.get("relics") or [])]:  # (a relic too: Velvet Choker, 6 a turn)
        m = re.search(r"cannot play more than (\d+) cards", str(c.get("description") or ""), re.I)
        if m:
            limits.append(int(m.group(1)))
    return max(0, min(limits) - played) if limits else None


def free_plays(state: dict) -> int:
    """Void Form ("The first 2 cards you play each turn are free to play"): how many of the next cards cost nothing.

    The game shows every card in the hand as costing 0 until they are used up, so a cost on show means none is left."""
    player = state.get("player") or {}
    if any(str(c.get("cost") or "0").strip() not in ("0", "X") for c in player.get("hand") or []):
        return 0
    return max(0, _stacks(player, "VOID_FORM") - int(state.get("_plays_this_turn") or 0))


def _poison_total(stack: int, turns: int) -> int:
    """HP a Poison stack takes over the next `turns` turns (it loses 1 a turn)."""
    stack = max(0, stack)
    if stack >= turns:
        return turns * stack - turns * (turns - 1) // 2
    return stack * (stack + 1) // 2


def _power_worth(c: CardInfo, long_fight: bool = True) -> float:
    """What playing a Power is worth in an ordinary fight: from the table, else a guess from its cost.

    In a boss or elite fight not less than other players' results say (crowd.py): a Power they
    win with and the bot will not pay for is a dead card in the deck. An Ironclad took Barricade
    (3 energy, 4 points here) and Dark Embrace as its best rewards and held them unplayed through
    the fight it died in. (Not in an ordinary fight, four rounds long: with the floor there too,
    act 2 rooms took 6.3 rounds instead of 4.5 for a quarter of an hour.)
    """
    worth = POWER_VALUE.get(c.key, 5.0 + 2.0 * max(c.cost, 0))
    crowd = crowd_rating(c.key) if long_fight else None
    return worth if crowd is None else max(worth, 2.5 * max(c.cost, 1) + 2.0 * (crowd - 4.0))


def _set_up(playable: list[CardInfo], energy: int) -> tuple | None:
    """A card worth playing with nobody to aim at: the enemy is gone for the turn and comes back.

    Between two of the Test Subject's forms the bot used to end the turn with
    energy in hand and Powers unplayed (Child of the Stars, Prowess, Loop).
    Whatever lasts is worth playing now: Powers, orbs, Strength, next turn's
    energy; and cards drawn while there is energy to use what they bring.
    """
    best, best_worth = None, 0.0
    for c in playable:
        if c.needs_target or c.type in ("Status", "Curse") or c.deadly or c.block_lock or (c.is_x and energy <= 0):
            continue
        # (X cards too: Dirge, "Summon 3 X times", stayed in the hand with 2 energy spare and Osty all that stood before 30 damage.)
        x, cost = (energy, energy) if c.is_x else (1, c.cost)
        worth = _power_worth(c) if c.type == "Power" else 0.0
        worth += (4.0 * len(c.channel) + c.summon + 1.5 * c.souls) * x + 0.5 * c.forge + 2.5 * c.star_gain
        worth += 4.0 * c.strength * c.strength_lasts + 3.5 * c.dexterity * c.dexterity_lasts
        worth += 5.0 * c.next_energy + 1.5 * c.next_draw + 0.8 * c.next_block
        if energy - cost > 0 or c.energy_gain:
            worth += 1.0 * c.draw + 2.0 * c.energy_gain
        worth -= 3.0 * c.hp_loss
        worth /= cost + 0.5
        if worth > best_worth:
            best, best_worth = c, worth
    return ("play_card", {"card_index": best.index}) if best is not None else None


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
            # The new one spends this turn on Defend+Buff: the attack of the one we killed is gone.
            # (Seen 2026-11-21: Axebot on 22 about to hit for 13x2, killed, and 3 Block lost nothing.
            # Counted as still coming, a kill for the whole hand lost to 10 damage and a Block card.)
            e.incoming = e.incoming_hits = 0
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


def loss_cap(player: dict) -> int:
    """Beating Remnant: "You cannot lose more than 20 HP in a single turn." (0: no such relic)"""
    for r in player.get("relics") or []:
        m = re.search(r"cannot lose more than (\d+) HP in a single turn", str(r.get("description") or ""), re.I)
        if m:
            return int(m.group(1))
    return 0
