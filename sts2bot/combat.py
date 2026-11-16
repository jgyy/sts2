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


def _dealt(e: Enemy, dmg_per_hit: int, hits: int, double_if_vulnerable: bool = False) -> int:
    """Damage an attack does to this enemy before its block."""
    if e.vulnerable:
        return int(dmg_per_hit * 1.5) * hits * (2 if double_if_vulnerable else 1)
    return dmg_per_hit * hits


def _pick_target(enemies: list[Enemy], dmg_per_hit: int, hits: int, double_if_vulnerable: bool = False) -> Enemy | None:
    alive = [e for e in enemies if e.alive]
    if not alive:
        return None

    def dealt(e: Enemy) -> int:
        return _dealt(e, dmg_per_hit, hits, double_if_vulnerable)

    killable = [e for e in alive if dealt(e) >= e.hp + e.block]
    if killable:
        # Remove the biggest threat first; waste the least damage on ties.
        return max(killable, key=lambda e: (e.incoming, e.hp + e.block))
    # Otherwise focus whoever dies soonest, counting Vulnerable as less health.
    return min(alive, key=lambda e: ((e.hp + e.block) / (1.5 if e.vulnerable else 1.0), -e.incoming))


def _weaken(e: Enemy) -> None:
    """A newly Weak enemy hits for 25% less this turn."""
    if not e.weak:
        e.weak = True
        e.incoming = int(e.incoming * 0.75)


def _hit(e: Enemy, dmg_per_hit: int, hits: int, double_if_vulnerable: bool = False) -> int:
    """Apply damage, return HP actually removed."""
    total = _dealt(e, dmg_per_hit, hits, double_if_vulnerable)
    absorbed = min(e.block, total)
    e.block -= absorbed
    lost = min(e.hp, total - absorbed)
    e.hp -= lost
    return lost


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
) -> Outcome | None:
    """Score playing `line` in order. None if it cannot be afforded."""
    foes = [replace(e) for e in enemies]
    score = 0.0
    first_target: str | None = None
    self_loss = 0
    dmg_value = 1.15 if tough_fight else 1.0

    for n, c in enumerate(line):
        if c.star_cost > stars:
            return None
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
        block += c.block * x

        if c.damage:
            per, hits = c.damage, c.hits * x
            alive = [e for e in foes if e.alive]
            if c.aoe:
                for e in alive:
                    score += _hit(e, per, hits, c.double_if_vulnerable) * dmg_value
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
                    score += _hit(t, per, hits, c.double_if_vulnerable) * dmg_value
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
            score += (7.0 + 3.0 * max(c.cost, 0)) * (1.5 if tough_fight else 1.0) * (1.0 if early else 0.35)
        elif c.type in ("Status", "Curse"):
            score += 0.1  # only worth clearing out with energy nothing else can use
        elif not c.known:
            score += 2.5 + max(c.cost, 0)

    kills = [e for e, orig in zip(foes, enemies) if not e.alive and orig.alive]
    score += 25.0 * len(kills)
    survivors = [e for e in foes if e.alive]
    if not survivors:
        score += WIN_BONUS

    incoming = sum(e.incoming for e in survivors)
    lost = max(0, incoming - block) + self_loss
    ratio = hp / max(1, max_hp)
    # A point of our HP is worth more than a point of enemy HP even at full
    # health: at 1.0 a Strike (6) beat a Defend (5) and the bot never blocked
    # until it was already hurt, bleeding HP in every ordinary fight.
    score -= lost * (1.5 + 2.5 * (1.0 - ratio))
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
    )


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
    danger = expected_loss(state) >= 0.3 * hp or hp <= 0.35 * max_hp
    if not (danger or state.get("state_type") in ("elite", "boss")):
        return []
    focus = min(enemies, key=lambda e: e.hp + e.block).entity_id
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
