"""Potions worth drinking in a fight, and the one-line picture of a fight for the debug log."""

from __future__ import annotations

import re

from .battle import expected_loss, osty_hp, read_enemies
from .cards import parse

Action = tuple[str, dict]

BOSS_FLOORS = (17, 33, 48)

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
    kind = state.get("state_type")
    floor = (state.get("run") or {}).get("floor")
    if kind in ("monster", "elite") and isinstance(floor, int) and any(0 < boss - floor <= 3 for boss in BOSS_FLOORS):
        # The boss is a room or two away: what is on the belt goes there. Act 3
        # bosses were met with one potion on average, a third of them with none.
        spare = 0
    # Elites used to get the whole belt on round 1, and they are not where
    # runs end: 125 of 149 deaths were at a boss. Bosses met with two or three
    # potions were beaten far more often (act 2: 65% against 34% with one or
    # none; act 1: 93% against 72%). So an elite gets the spare one, and the
    # rest only once the fight has gone wrong.
    hurt = kind == "elite" and hp <= 0.5 * max_hp
    big_fight = danger or hurt or kind == "boss"
    if not (big_fight or spare):
        return []
    left = sum(e.hp + e.block for e in enemies if e.worth > 0)
    if left <= 0 or (left <= 25 and not danger):
        return []  # nothing left to hit (a boss already beaten), or the fight is as good as over
    block = int(player.get("block") or 0)
    # Aim at something that stays dead and matters: the leader that hits hardest.
    focus = max(enemies, key=lambda e: (e.worth, e.incoming, e.hp)).entity_id
    out: list[Action] = []
    slippery = any(e.slippery or e.hit_cap == 1 for e in enemies)  # (Intangible: the same, for a turn)
    # The Test Subject's first form (100 HP, Enrage): what is thrown at it is
    # gone when it revives (Vulnerable from a potion lasted two turns of a
    # fifteen-turn fight). Potions aimed at the enemy wait for the second
    # form, 200 HP that hit 30, 40, 50 a turn: every turn off it counts most.
    passing = any(e.adapts and e.enrage for e in enemies)
    for p in potions:
        if not p.get("can_use_in_combat", True):
            continue
        if not big_fight and len(out) >= spare:
            break
        if slippery and re.search(r"deals? \d+ damage", str(p.get("description") or ""), re.I):
            continue  # every hit on a Slippery enemy takes 1 HP: keep it for later
        text = str(p.get("description") or "")
        if passing and not danger and (str(p.get("target_type")) in ("AnyEnemy", "AllEnemies") or re.search(r"enem(y|ies)", text, re.I)):
            continue
        both = re.search(r"deals? (\d+) damage to all (?:players|characters)", text, re.I)
        if both:
            # Foul Potion hits us as hard as them: three of them at the start of
            # a boss fight took 36 of 75 HP for 36 damage. Only behind enough
            # Block, or to end the fight.
            amount = int(both.group(1))
            if not (block >= amount or (hp > amount and all(e.hp + e.block <= amount for e in enemies))):
                continue
        if re.search(r"\bheal\b|\bregen\b", text, re.I) and hp > 0.55 * max_hp:
            continue  # healing is wasted on a full bar; it is for later in the fight
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
