"""Potions worth drinking in a fight, and the one-line picture of a fight for the debug log."""

from __future__ import annotations

import re

from ..fight.battle import end_of_turn_block, expected_loss, osty_hp, read_enemies
from .. import games
from ..cards import parse

Action = tuple[str, dict]

BOSS_FLOORS = (17, 33, 48)
STS1_BOSS_FLOORS = (16, 33, 50)  # the first game's acts are 17 floors each, the boss on the last


def boss_floors() -> tuple:
    """The floors each act's boss is met on, in the game being played."""
    return BOSS_FLOORS if games.uses_sts2_tables() else STS1_BOSS_FLOORS


# Floors before each act's boss from which a full belt is no longer a reason to drink.
# (Six in every act until 2026-11-22. A belt holds two from Ascension 4, so "full" spent one of them on
# round 1 of the next ordinary fight: in act 3 93 potions went on ordinary fights and 40 reached the last
# boss, which was met with one or none 14 times in 31 and never beaten so (5 in 17 with two; on 90% HP or
# more 5 in 14 against 0 in 4). The slot does not fill again in time there: a fight left holding one potion
# ten floors or more from the last boss met it with two 30 times in 52, left holding two 5 in 5. Act 2 the
# same way, less: holding one 78% from any distance, holding two 90%, and bosses met on 75% HP or more were
# beaten 37 times in 59 with two potions and 3 in 12 with fewer.)
SAVE_FLOORS = {1: 6, 2: 9, 3: 15}
SPARE_BEYOND = 3  # potions a boss fight is kept: more than that on the belt are drunk on the way

# (Dexterity for the whole fight, the Dexterity Potion's 2, is not one of these: it goes early, like Strength.)
_DEFENSIVE_POTION = re.compile(r"gain \d+ block|gain \d+ dexterity\. at the end of your turn|apply \d+ weak|enem(y|ies) loses? \d+ strength", re.I)
# Strength for one turn (Flex Potion: "Gain 5 Strength. At the end of your turn, lose 5 Strength."). Its own "lose 5
# Strength" used to file it with the defensive potions: 55 of them were drunk on turns a big hit was coming.
_ONE_TURN_STRENGTH = re.compile(r"gain \d+ strength\. at the end of your turn, lose", re.I)
_ENEMY_DEBUFF = re.compile(r"apply \d+ (weak|vulnerable|poison|doom)|enem(y|ies) loses? \d+ (strength|hp at the end)|enemy's attacks deal \d+% less", re.I)


# What a potion is worth in the boss fight it is being carried to, in points of damage or Block there (a fight of
# ten rounds: what lasts counts for all of them). The belt holds two from Ascension 4, an act drops four or five,
# and bosses met with two were beaten twice as often as with one (act 2: 34 in 60 against 9 in 37; inside every
# band of relics held too): so the one drunk on the way, and the one left behind for a better one, is the least
# of these. A potion not listed is priced from its text.
BOSS_WORTH = {
    "fairy in a bottle": 60, "mazaleth's gift": 50, "strength potion": 45, "focus potion": 40, "fysh oil": 38, "powdered demise": 36,
    "potion of doom": 33, "radiant tincture": 32, "king's courage": 30, "vulnerable potion": 30, "dexterity potion": 30, "orobic acid": 28,
    "power potion": 26, "heart of iron": 25, "gigantification potion": 25, "soldier's stew": 24, "distilled chaos": 24, "beetle juice": 24,
    "liquid bronze": 22, "blood potion": 20, "fire potion": 20, "poison potion": 20, "ship in a bottle": 20, "touch of insanity": 20,
    "lucky tonic": 18, "cunning potion": 18, "energy potion": 16, "clarity extract": 16, "regen potion": 15, "weak potion": 15,
    "potion of binding": 15, "bone brew": 15, "blessing of the forge": 15, "flex potion": 15, "potion-shaped rock": 15, "cure all": 14,
    "block potion": 12, "attack potion": 12, "duplicator": 12, "shackling potion": 12, "star potion": 12, "explosive ampoule": 10,
    "skill potion": 10, "colorless potion": 10, "liquid memories": 10, "speed potion": 10, "snecko oil": 10, "swift potion": 8,
    "bottled potential": 8, "glowwater potion": 8, "pot of ghouls": 8, "fortifier": 8, "droplet of precognition": 6, "gambler's brew": 5,
    "stable serum": 5, "foul potion": 4,
}
_FILLS_BELT = re.compile(r"fill all (?:your )?empty potion slots", re.I)


def boss_worth(potion: dict) -> float:
    """What this potion (one on the belt, or a reward or shop item) is worth kept for the next boss."""
    name = str(potion.get("name") or potion.get("potion_name") or "").strip().lower()
    if name in BOSS_WORTH:
        return float(BOSS_WORTH[name])
    text = str(potion.get("description") or potion.get("potion_description") or "")
    if _FILLS_BELT.search(text) or re.search(r"gain \d+ max hp", text, re.I):
        return 0.0  # (drunk at the first chance: it is not for a boss)
    m = re.search(r"deals? (\d+) damage", text, re.I)
    if m:
        return float(m.group(1))
    return 22.0 if re.search(r"\b(strength|dexterity|focus|ritual|plating)\b", text, re.I) and "at the end of your turn" not in text.lower() else 12.0


def saving(floor) -> bool:
    """The next boss is near enough that what is on the belt is kept for it (see SAVE_FLOORS)."""
    return isinstance(floor, int) and any(0 < boss - floor <= SAVE_FLOORS[act] for act, boss in enumerate(boss_floors(), 1))


def fortifier(player: dict) -> bool:
    """A potion that triples our Block is on the belt (the planner then counts on it when nothing else saves the turn)."""
    return any(p.get("can_use_in_combat", True) and re.search(r"triple your block", str(p.get("description") or ""), re.I) for p in player.get("potions") or [])


def _ill_timed(text: str, player: dict, line: list | None, block: int, incoming: int, danger: bool, losing: int = 0) -> bool:
    """A potion that does nothing, or next to nothing, drunk at this moment.

    Nine potions in ten went on round 1 of a boss fight whatever they said. Of 14 Fortifiers ("Triple your
    Block") 11 tripled a Block of 0; 4 Liquid Memories of 5 found the discard pile empty; 9 Glowwaters of 10
    Exhausted the opening hand. `line` is what the planner means to play this turn (None: not known).
    """
    hand = [parse(c) for c in player.get("hand") or []]
    cards = hand if line is None else line
    if re.search(r"triple your block", text, re.I):
        # ... once the turn's Block is up (the planner has nothing left to play) and still short of what is coming.
        return not (line == [] and block >= 8 and incoming > block)
    if re.search(r"from your discard pile", text, re.I):
        return int(player.get("discard_pile_count") or 0) < 3
    if re.search(r"exhaust your hand", text, re.I):
        # (Glowwater: "Exhaust your Hand. Draw 10 cards": for a hand of Wounds and Burns, or energy enough for ten cards.)
        return not danger and sum(c.type in ("Status", "Curse") for c in hand) < 2 and int(player.get("energy") or 0) < 5
    if _ONE_TURN_STRENGTH.search(text):
        return sum(c.hits for c in cards if c.type == "Attack" and c.damage) < (1 if danger else 3)
    if re.search(r"gain \d+ dexterity\. at the end of your turn", text, re.I):
        return sum(1 for c in cards if c.block) < (1 if danger else 2)  # (Speed Potion: 5 more on each Block card played this turn)
    icons = len(re.findall(r"energy_icon", text))
    if icons and re.fullmatch(r"gain (?:\[\w*energy_icon\.png\])+\.?", text.strip(), re.I):
        # Energy Potion: for a turn that has cards left over to spend it on. Of 34 drunk, 13 went on a turn that
        # ended with 2 energy or more unspent: round 1 of a boss that was not attacking, the hand's Defends
        # all that was left. (What the plan leaves in the hand: an attack, a Power, a card that draws, an X
        # card; a Block card only if the plan still loses HP.)
        planned = {c.index for c in line or []}
        spare = [c for c in hand if c.index not in planned and c.type not in ("Status", "Curse") and (c.playable or c.short)]
        wanted = [c for c in spare if c.is_x or (c.cost > 0 and (c.damage or c.type == "Power" or c.draw or (c.block and losing > 0)))]
        return sum(icons if c.is_x else c.cost for c in wanted) < icons
    if re.search(r"next card is played an extra time", text, re.I):
        return not any(c.type == "Power" or c.cost >= 2 for c in cards)  # (Duplicator: 9 of 11 doubled whatever came first)
    return False


def potion_actions(state: dict, planned_loss: int | None = None, line: list | None = None) -> list[Action]:
    """Potions worth drinking now: always in elite/boss fights, otherwise when in danger.

    `planned_loss` is what the planner expects to lose this turn after playing
    the best cards in hand (None: not known), `line` the cards it means to play.
    """
    player = state.get("player") or {}
    enemies = read_enemies(state)
    if not enemies:
        return []
    hp = int(player.get("hp") or 1)
    max_hp = int(player.get("max_hp") or 1)
    # Ordinary fights only get a potion in real trouble; the rest are kept
    # for elites and bosses, where the runs are actually lost.
    danger = expected_loss(state) >= 0.5 * hp or hp <= 0.3 * max_hp
    if planned_loss is not None:
        # Judge by what still gets through after the best cards in hand, not
        # by what is coming if we stand still. A Regent on 31 HP facing 20 from
        # a Beetle on 12 HP, with the kill in hand, drank a forge potion, a
        # Strength potion and a Foul Potion (12 to itself), three floors before
        # a boss it then met on 46 of 90 HP. The plan is redone after every
        # potion, so they also stop as soon as one has been enough.
        danger = planned_loss >= 0.5 * hp or (hp <= 0.3 * max_hp and planned_loss > 0)
    potions = player.get("potions") or []
    # A full belt cannot take the next potion a fight drops, so one of them
    # is free to use: holding three potions through a whole act wasted every
    # potion reward on the way.
    spare = 1 if len(potions) >= int(player.get("max_potion_slots") or 3) else 0
    kind = state.get("state_type")
    floor = (state.get("run") or {}).get("floor")
    if kind in ("monster", "elite") and saving(floor):
        # The boss is a few rooms away: what is on the belt goes there. Act 3
        # bosses were met with one potion on average, a third of them with none.
        # (Three floors until 2026-11-21. A slot emptied six floors or less before a boss was filled again
        # in time 7 times in 10, one emptied ten floors before it 19 times in 20.)
        spare = 0
    elif kind in ("monster", "elite"):
        # A belt that holds more than three (Potion Belt, Phial Holster) is never "full", so nothing on it
        # counted as spare: a Regent went from 53 HP to 31 to 16 in two ordinary fights holding five, and
        # drank them all in the fight it died in. What is beyond three is for the way there.
        spare = max(spare, len(potions) - SPARE_BEYOND)
    # Elites used to get the whole belt on round 1, and they are not where
    # runs end: 125 of 149 deaths were at a boss. Bosses met with two or three
    # potions were beaten far more often (act 2: 65% against 34% with one or
    # none; act 1: 93% against 72%). So an elite gets the spare one, and the
    # rest only once the fight has gone wrong.
    hurt = kind == "elite" and hp <= 0.5 * max_hp
    big_fight = danger or hurt or kind == "boss"
    # Delicate Frond fills the belt again at the start of every fight: nothing is being saved.
    if any("fill all empty potion slots" in str(r.get("description") or "").lower() for r in player.get("relics") or []):
        big_fight, spare = True, len(potions)
    # What a potion gives for good is worth the same in any fight, and the slot it sits in is half
    # the belt on Ascension 4: Fruit Juice ("Gain 5 Max HP") goes at the first chance.
    # (Entropic Brew, "Fill all your empty potion slots with random potions", the same: the slot it frees is one of them.)
    for_good = lambda p: re.search(r"gain \d+ max hp", str(p.get("description") or ""), re.I) or _FILLS_BELT.search(str(p.get("description") or ""))
    keeps = [("use_potion", {"slot": int(p.get("slot", 0))}) for p in potions if p.get("can_use_in_combat", True) and for_good(p)]
    if keeps:
        return keeps
    if kind != "boss":
        # What is drunk on the way to a boss is what will do the least there. (In the order of the belt, a
        # Strength Potion went on round 1 of an ordinary fight and a Swift Potion was carried to the boss.)
        potions = sorted(potions, key=boss_worth)
    if not (big_fight or spare):
        return []
    if any(e.asleep >= 2 for e in enemies) and not danger:
        # Asleep: until the turn before she wakes we only set up, and a potion drunk now is poured out. (An
        # Energy Potion on round 1 of the Lagavulin Matriarch, with nothing to play: that fight was lost
        # twelve rounds later with her on 10 HP. Of 24 potions drunk at a sleeper, 14 were cards, energy or
        # Block for a turn that used none of them.)
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
    # (The same while an enemy sleeps: what is thrown at it only wakes it early.)
    passing = any((e.adapts and e.enrage) or e.asleep >= 2 for e in enemies)
    shielded = next(e.artifact > 0 for e in enemies if e.entity_id == focus)
    for p in potions:
        if not p.get("can_use_in_combat", True):
            continue
        if not big_fight and len(out) >= spare:
            break
        text = str(p.get("description") or "")
        # (Nor what makes our hits bigger: two Flex Potions went on round 2 against a Vantom with 7 Slippery left.)
        if slippery and re.search(r"deals? \d+ damage|\bstrength\b|\bvigor\b|\bvulnerable\b|\bpoison\b|\bdoom\b", text, re.I):
            continue  # every hit on a Slippery enemy takes 1 HP: keep it for later
        if passing and not danger and (str(p.get("target_type")) in ("AnyEnemy", "AllEnemies") or re.search(r"enem(y|ies)", text, re.I)):
            continue
        # Artifact swallows what a potion would put on the enemy: a Weak Potion went into Aeonglass's
        # Artifact 3 on round 1. It waits until the cards have worn that off.
        if shielded and not danger and _ENEMY_DEBUFF.search(text):
            continue
        both = re.search(r"deals? (\d+) damage to all (?:players|characters)", text, re.I)
        if both:
            # Foul Potion hits us as hard as them: three of them at the start of
            # a boss fight took 36 of 75 HP for 36 damage. Only behind enough
            # Block, or to end the fight.
            amount = int(both.group(1))
            # "Enough" is what is left over after this turn's attacks: a Regent
            # on 38 HP behind 21 Block against 7x5 threw one and lost 26.
            spare_block = block + end_of_turn_block(player) - sum(e.incoming for e in enemies)
            if not (spare_block >= amount or (hp > amount and all(e.hp + e.block <= amount for e in enemies))):
                continue
        if re.search(r"\bheal\b|\bregen\b", text, re.I) and hp > 0.55 * max_hp:
            continue  # healing is wasted on a full bar; it is for later in the fight
        # (Snecko Oil, "Draw 7 cards. Randomize the cost of cards in your Hand this turn", was never drunk: "random
        # costs wreck the planner". They do not: the state shows each cost as it was rolled, as under Snecko Eye,
        # and a potion never drunk is half the belt on Ascension 4 and up.)
        if _ill_timed(text, player, line, block, sum(e.incoming for e in enemies), danger, planned_loss or 0):
            continue
        if _DEFENSIVE_POTION.search(text) and expected_loss(state) < min(12, hp):
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
    powers_of = lambda c: ",".join(f"{str(s.get('id', '')).replace('_POWER', '')}:{s.get('amount')}" for s in c.get("status") or [])
    foes = []
    for e in battle.get("enemies") or []:
        powers = powers_of(e)
        intents = "+".join(f"{i.get('type')}{i.get('label') or ''}" for i in e.get("intents") or [])
        foes.append(f"{e.get('entity_id')} {e.get('hp')}hp/{e.get('block')}blk [{powers}] {intents}")
    def read(c: dict) -> str:
        """What the planner makes of the card, e.g. " d6x2 b5" (so a misread shows in the log)."""
        i = parse(c)
        hits = i.hits * (1 + i.replay)  # (a card that plays itself again shows as that many hits)
        bits = [f"d{i.damage}" + (f"x{hits}" if hits > 1 else "")] if i.damage else []
        bits += [f"b{i.block}"] if i.block else []
        return (" " + " ".join(bits)) if bits else ""

    hand = ", ".join(
        f"{c.get('index')}:{c.get('name')}({c.get('cost')}{'' if c.get('can_play', True) else '!'}{read(c)})"
        for c in player.get("hand") or []
    )
    mine = powers_of(player)
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
