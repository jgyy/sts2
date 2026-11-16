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

    @property
    def worth(self) -> float:
        """How much damaging this enemy brings the fight closer to its end."""
        if self.revives:
            return 0.0
        return 0.4 if self.minion else 1.0

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
                slippery=max(0, power_amount(e, "SLIPPERY")),
                damage_mult=0.5 if _has_power(e, "FLUTTER") else 1.0,
                thorns=max(0, power_amount(e, "THORNS")),
                revives=_has_power(e, "ILLUSION"),
                stun_at=max(0, power_amount(e, "PLOW")),
                slow=_slow_percent(e),
                minion=_has_power(e, "MINION"),
                incoming=sum(intent_damage(i) for i in e.get("intents") or []),
            )
        )
    return out


def _order_key(c: CardInfo) -> tuple:
    if c.type == "Power":
        rank = 0
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
        through = per - absorbed
        if through <= 0:
            continue
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


def _weaken(e: Enemy) -> None:
    """A newly Weak enemy hits for 25% less this turn."""
    if not e.weak:
        e.weak = True
        e.incoming = int(e.incoming * 0.75)


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
    tough_fight: bool,
    early: bool,
    card_limit: int | None = None,
    can_block: bool = True,
) -> Outcome | None:
    """Score playing `line` in order. None if it cannot be afforded."""
    if card_limit is not None and len(line) > card_limit:
        return None
    foes = [replace(e) for e in enemies]
    score = 0.0
    first_target: str | None = None
    self_loss = 0
    dmg_value = 1.15 if tough_fight else 1.0

    for n, c in enumerate(line):
        if c.star_cost > stars:
            return None
        for e in foes:
            if e.slow >= 0:
                e.slow += 10  # counts the card being played, like the game does
        stars -= c.star_cost
        x = 1
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
        if can_block:
            block += c.block * x
        if c.block_lock:
            # Trading away future block is only worth it to survive this turn.
            score -= 30.0
        if c.deadly:
            score -= LETHAL_PENALTY  # never gamble the run on one card

        if c.damage:
            per, hits = c.damage, c.hits * x
            alive = [e for e in foes if e.alive]
            if c.aoe:
                for e in alive:
                    self_loss += e.thorns * _hit_count(e, hits, c.double_if_vulnerable)
                    score += _hit(e, per, hits, c.double_if_vulnerable) * dmg_value * e.worth
                    if e.alive:
                        if c.vulnerable:
                            e.vulnerable = True
                        if c.weak:
                            _weaken(e)
            elif c.random_target and len(alive) > 1:
                score += per * hits * 0.8 * dmg_value
            else:
                t = _pick_target(foes, per, hits, c.double_if_vulnerable)
                if t is not None:
                    if n == 0:
                        first_target = t.entity_id
                    self_loss += t.thorns * _hit_count(t, hits, c.double_if_vulnerable)
                    score += _hit(t, per, hits, c.double_if_vulnerable) * dmg_value * t.worth
                    if c.vulnerable and t.alive:
                        t.vulnerable = True
                    if c.weak and t.alive:
                        _weaken(t)
        elif c.needs_target:
            alive = [e for e in foes if e.alive]
            if c.weak and not c.vulnerable:
                # Weaken the hardest hitter that is not Weak yet.
                t = max(alive, key=lambda e: (not e.weak, e.incoming), default=None)
            else:
                # Vulnerable goes on the enemy we will be hitting the longest.
                fresh = [e for e in alive if not e.vulnerable] or alive
                t = max(fresh, key=lambda e: e.hp + e.block, default=None)
            if t is not None:
                if n == 0:
                    first_target = t.entity_id
                if c.vulnerable:
                    t.vulnerable = True
                if c.weak:
                    _weaken(t)
        elif c.aoe:
            for e in foes:
                if e.alive:
                    if c.vulnerable:
                        e.vulnerable = True
                    if c.weak:
                        _weaken(e)

        if c.vulnerable:
            score += min(c.vulnerable, 3) * 2.5
        if c.weak:
            score += min(c.weak, 3) * 1.0  # later turns; this turn is in `incoming`
        score += c.draw * 2.0
        if c.type == "Power":
            score += (5.0 + 2.0 * max(c.cost, 0)) * (1.5 if tough_fight else 1.0) * (1.0 if early else 0.35)
        elif c.type in ("Status", "Curse"):
            score += 0.1  # only worth clearing out with energy nothing else can use
        elif not c.known:
            score += 2.5 + max(c.cost, 0)

    kills = [e for e, orig in zip(foes, enemies) if not e.alive and orig.alive]
    score += 25.0 * sum(e.worth for e in kills)
    survivors = [e for e in foes if e.alive]
    # The fight is over once every leader is dead: minions leave with them.
    if not any(not e.minion for e in survivors) and any(not e.minion for e in enemies):
        score += WIN_BONUS
        survivors = []
    elif not survivors:
        score += WIN_BONUS

    incoming = sum(e.incoming for e in survivors)
    lost = max(0, incoming - block) + self_loss
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
        block=int(player.get("block") or 0) + end_of_turn_block(player),
        tough_fight=state.get("state_type") in ("elite", "boss"),
        early=int(battle.get("round") or 1) <= 3,
        card_limit=cards_left_this_turn(state),
        can_block=not _has_power(player, "NO_BLOCK"),
    )


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

    baseline = simulate([], enemies, **ctx)
    best_score = baseline.score if baseline else float("-inf")
    best_line: list[CardInfo] = []
    best_target: str | None = None
    for size in range(1, len(playable) + 1):
        found = False
        for line in combinations(playable, size):  # keeps _order_key order
            out = simulate(list(line), enemies, **ctx)
            if out is None:
                continue
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
    return max(0, incoming - int(player.get("block") or 0) - end_of_turn_block(player))


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
    hand = ", ".join(
        f"{c.get('index')}:{c.get('name')}({c.get('cost')}{'' if c.get('can_play', True) else '!'})" for c in player.get("hand") or []
    )
    mine = ",".join(f"{str(s.get('id', '')).replace('_POWER', '')}:{s.get('amount')}" for s in player.get("status") or [])
    return (
        f"r{battle.get('round')} hp {player.get('hp')}/{player.get('max_hp')} blk {player.get('block')} "
        f"en {player.get('energy')} [{mine}] | {' ; '.join(foes)} | hand {hand}"
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
