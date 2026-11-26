"""Slay the Spire 2's events, one rule each (2026-11-26, from the game's own texts in localization/eng/events.json, the
options the bot met in its logs, and what each named card, relic and enchantment does). A rule returns an option's score on
the generic scorer's scale (events._event_option_value: a relic 6, an upgrade 4), or None to leave it to that scorer, which
reads the text; `COVERED` lists every event and the ones whose generic score was checked and found right."""

from __future__ import annotations

import re
from dataclasses import replace

from ..relicworth import id_of as relic_id, worth as relic_worth
from .value import (CARD_REWARD, POTION, RANDOM_CURSE, RELIC, Ctx, counts, curse, gold, heal, hp_cost, max_hp_gain, max_hp_loss, num, potions_gained,
                    random_upgrades, removals, transforms, UPGRADE)

BASE = 0.5  # an option's worth before what it gives and costs (as the generic scorer)
# The chance the next act's map leads us to the War Historian the Lantern Key opens: not known (it never has been kept); at 0.35
# the key is still returned for its gold, as before. Raise it once a kept key is seen to bring the event.
MEET_WAR_HISTORIAN = 0.35


def _text(opt: dict) -> tuple[str, str]:
    return str(opt.get("title") or "").strip().lower(), str(opt.get("description") or "")


def relic(name: str, ctx: Ctx) -> float:
    """A relic by name: its worth beyond a relic's (relicworth.py), on top of a relic's."""
    return RELIC + relic_worth(relic_id(name), ctx.deck, ctx.act, ctx.hp, ctx.max_hp)


def _hallway_fight(ctx: Ctx) -> float:
    """An event's ordinary fight: its rewards less the HP it costs (some 8, 12, 14% of Max HP by act)."""
    share = {1: 0.08, 2: 0.12}.get(ctx.act, 0.14)
    return CARD_REWARD + 0.4 - hp_cost(round(share * ctx.max_hp), ctx)


# --------------------------------------------------------------------------------------------------- the rules

def abyssal_baths(opt, ctx):
    title, text = _text(opt)
    if title in ("immerse", "linger"):
        return BASE + max_hp_gain(num(r"gain (\d+) max hp", text)) - hp_cost(num(r"take (\d+) damage", text), ctx)
    if title == "abstain":
        return BASE + heal(num(r"heal (\d+)", text), ctx)
    return BASE if title == "exit baths" else None


def amalgamator(opt, ctx):
    # Two basics become one better card: Strikes first (the bot's removal order: a Strike is the weakest card), Defends after.
    title, _ = _text(opt)
    c = counts(ctx)
    if title == "combine strikes":
        return BASE + 5.5 if c["strikes"] >= 2 or not ctx.deck else -1.0
    if title == "combine defends":
        return BASE + 5.0 if c["defends"] >= 2 or not ctx.deck else -1.0
    return None


def aroma_of_chaos(opt, ctx):
    title, _ = _text(opt)
    if title == "maintain control":
        return BASE + UPGRADE
    if title == "let go":
        return BASE + transforms(ctx)  # (better than an upgrade only with a curse in the deck)
    return None


def battleworn_dummy(opt, ctx):
    """A dummy that never attacks, killed in 3 turns or nothing: 0 of 22 fights killed the 300 HP one (125 dealt on average,
    the best 247), 4 of 22 the 150 HP one, all 22 the 75 HP one (2026-11-26)."""
    title, text = _text(opt)
    hp = num(r"fight a (\d+) hp dummy", text)
    if not hp:
        return None
    dealt = three_turns(ctx)
    chance = max(0.0, min(1.0, (dealt - 0.8 * hp) / (0.4 * hp)))  # (0 below 80% of its HP, sure from 120%)
    if hp <= 80:
        chance = max(chance, 0.95)  # (the 75 HP one died in every logged fight)
    reward = RELIC if "relic" in text.lower() else 2 * 2.0 if "upgrade 2" in text.lower() else potions_gained(1, ctx)
    return BASE + chance * reward


def three_turns(ctx: Ctx) -> float:
    """The damage the deck deals in three turns of all-out attack (the logged dummy fights: 75-247, 125 on average)."""
    from ...cards import parse
    per_card = [parse(c) for c in ctx.deck]
    if not ctx.deck or not any(i.total_damage for i in per_card):
        return 125.0  # (the deck's texts unknown: what the logged fights dealt)
    dmg = sum(max(0, i.total_damage) + 3 * i.poison + 4 * i.shivs + 0.8 * i.doom for i in per_card)
    energy = sum(max(1, i.cost) for i in per_card if i.total_damage > 0) or 1
    per_energy = dmg / energy
    per_card_drawn = dmg / len(per_card)
    turn = min(5 * per_card_drawn * 1.3, 3.0 * per_energy)  # (a hand of five, three energy: whichever runs out first)
    return 3.0 * turn


def brain_leech(opt, ctx):
    title, text = _text(opt)
    if title == "share knowledge":
        return BASE + 1.6  # (one card of five of our own)
    if title == "rip the leech off":
        return BASE + 1.0 - hp_cost(num(r"lose (\d+) hp", text), ctx)  # (a Colorless reward of three)
    return None


# The cards each colour gives work without the rest of their deck (Ironclad's Strength, Silent's Shivs and Poison) or need it
# (Osty, Stars, Arrows, Orbs): three rewards of Necrobinder cards were taken 12 times in 21 for a three-way tie.
_COLOUR = {"red": ("THE IRONCLAD", 1.0), "green": ("THE SILENT", 1.0), "blue": ("THE DEFECT", 0.6), "orange": ("THE REGENT", 0.3),
           "pink": ("THE NECROBINDER", 0.2), "yellow": ("THE FOOL", 0.3)}


def colorful_philosophers(opt, ctx):
    title, _ = _text(opt)
    if title in _COLOUR:
        return BASE + 3 * CARD_REWARD * _COLOUR[title][1]
    if title == "equality":
        return BASE + 2.5  # (Prismatic Shard: card rewards of every colour, as in the first game)
    return None


def colossal_flower(opt, ctx):
    """Three pages: the gold grows with each step down (35 at the top, more below), the centre holds a relic. Priced as the
    best way down from here, each step's HP taken off: the top page's "Reach Deeper" had only its 5 HP to show."""
    title, text = _text(opt)
    core = RELIC + 1.5  # Pollinous Core: cards drawn every few turns
    if title == "enter the center":
        return BASE + core - hp_cost(num(r"lose (\d+) hp", text, 7), ctx)
    if title == "reach deeper":
        step = num(r"lose (\d+) hp", text, 5)
        later = (6, 7) if step <= 5 else (7,)  # (the HP the steps below this one cost)
        left = replace(ctx, hp=ctx.hp - step)
        down = core - sum(hp_cost(x, replace(ctx, hp=left.hp - sum(later[:i]))) for i, x in enumerate(later))
        return BASE + max(down, gold(135 if step > 5 else 75, left)) - hp_cost(step, ctx)
    if title == "extract nectar":
        return BASE + gold(num(r"gain (\d+) gold", text), ctx)
    return None


def crystal_sphere(opt, ctx):
    # Each divination turned up 7-15 gold and now and then a potion (2026-11-24 logs): small either way, the Debt dearer.
    title, text = _text(opt)
    divine = 0.35
    if title == "uncover future":
        return BASE + gold(-num(r"pay (\d+) gold", text), ctx) + 3 * divine
    if title == "payment plan":
        return BASE - curse("debt") + 6 * divine
    return None


def dense_vegetation(opt, ctx):
    title, text = _text(opt)
    if title == "rest":
        healed = num(r"heal (\d+)", text)
        return BASE + heal(healed, ctx) + _hallway_fight(replace(ctx, hp=min(ctx.max_hp, ctx.hp + healed)))
    if title == "trudge on":
        return BASE + gold(num(r"gain (\d+) gold", text), ctx) - hp_cost(num(r"lose (\d+) hp", text), ctx)
    if title == "fight!":
        return BASE
    return None


def doll_room(opt, ctx):
    title, text = _text(opt)
    doll = RELIC - 1.0  # (a doll: some do little)
    if title == "pick at random":
        return BASE + doll
    if title == "take some time":
        return BASE + doll + 0.6 - hp_cost(num(r"lose (\d+) hp", text), ctx)
    if title.startswith("examine each"):
        return BASE + doll + 1.0 - hp_cost(num(r"lose (\d+) hp", text), ctx)
    return None


def doors_of_light_and_dark(opt, ctx):
    # A tie at 5.5 went to the upgrade 23 times in 23: a removal is the better door with a curse or Strikes in the deck.
    title, _ = _text(opt)
    if title == "light door":
        return BASE + UPGRADE
    if title == "dark door":
        return BASE + removals(ctx)
    return None


def drowning_beacon(opt, ctx):
    title, text = _text(opt)
    if title == "bottle":
        return BASE + potions_gained(1, ctx)
    if title == "climb":
        return BASE + relic("Fresnel Lens", ctx) - max_hp_loss(num(r"lose (\d+) max hp", text), ctx)
    return None


_BELT = {  # Endless Conveyor: what each dish is, before its gold
    "caviar": lambda text, ctx: max_hp_gain(num(r"gain (\d+) max hp", text, 5)),
    "clam roll": lambda text, ctx: heal(num(r"heal (\d+)", text, 10), ctx),
    "fried eel": lambda text, ctx: 0.4,  # (a random Colorless card)
    "golden fysh": lambda text, ctx: gold(num(r"gain (\d+) gold", text), ctx),
    "jelly liver": lambda text, ctx: transforms(ctx) - 0.5,  # ("Transform a card": ours to choose?)
    "seapunk salad": lambda text, ctx: -0.3,  # (Feeding Frenzy: Strength for a turn, a weak card)
    "spicy snappy": lambda text, ctx: random_upgrades(1, ctx),
    "suspicious condiment": lambda text, ctx: potions_gained(1, ctx),
}


def endless_conveyor(opt, ctx):
    title, text = _text(opt)
    if title == "observe the chef":
        return BASE + random_upgrades(1, ctx)
    if m := re.match(r"grab (.+?) off the belt", title):
        dish = _BELT.get(m.group(1))
        if dish is None:
            return None
        price = num(r"pay (\d+) gold", text)
        more = 0.4 if ctx.gold - price >= 40 else 0.0  # ("Continue feasting!": another dish to look at after it)
        return BASE + dish(text, ctx) + gold(-price, ctx) + more
    if title in ("leave", "broke"):
        return BASE
    return None


def field_of_man_sized_holes(opt, ctx):
    title, _ = _text(opt)
    if title == "enter your hole":
        return BASE + 2.0  # Perfect Fit: our best card back on top of every shuffle
    if title == "resist":
        return BASE + removals(ctx, 2) - curse("normality")
    return None


def grave_of_the_forgotten(opt, ctx):
    title, _ = _text(opt)
    if title.startswith("accept"):
        return BASE + RELIC - 1.0  # Forgotten Soul: 1 damage an Exhaust
    if title == "confront with truth":
        return BASE - curse("decay") + 1.5  # (Soul's Power: an Exhaust card stays)
    return None


def hungry_for_mushrooms(opt, ctx):
    title, text = _text(opt)
    if title == "big mushroom":
        return BASE + max_hp_gain(num(r"max hp by (\d+)", text, 20)) - 3.5  # (two cards fewer in every fight's first hand)
    if title == "fragrant mushroom":
        return BASE + random_upgrades(num(r"upgrade (\d+) random", text, 2), ctx) - hp_cost(num(r"lose (\d+) hp", text), ctx)
    return None


def jungle_maze_adventure(opt, ctx):
    title, text = _text(opt)
    if title in ("join forces", "solo quest"):
        return BASE + gold(num(r"gain (\d+) gold", text), ctx) - hp_cost(num(r"lose (\d+) hp", text), ctx)
    return None


def lost_wisp(opt, ctx):
    title, text = _text(opt)
    if title == "capture the wisp":
        return BASE + RELIC - 4.0 + min(4.0, 1.0 * counts(ctx)["powers"]) - curse("decay")  # (damage for every Power played)
    if title == "search the nearby area":
        return BASE + gold(num(r"gain (\d+) gold", text), ctx)
    return None


def luminous_choir(opt, ctx):
    title, text = _text(opt)
    if title == "offer tribute":
        return BASE + RELIC + gold(-num(r"pay (\d+) gold", text), ctx)
    if title == "reach into the flesh":
        return BASE + removals(ctx, 2) - curse("spore mind")
    return None


def morphic_grove(opt, ctx):
    # "Lose ALL of your Gold" was read as the end of the run (-996.5): it is the gold we hold, little some of the time.
    title, text = _text(opt)
    if title == "group":
        return BASE + transforms(ctx, num(r"transform (\d+) cards", text, 2)) + gold(-ctx.gold, ctx)
    if title == "loner":
        return BASE + max_hp_gain(num(r"gain (\d+) max hp", text))
    return None


def museum_of_torture(opt, ctx):
    title, text = _text(opt)
    if title == "take it":
        m = re.search(r"obtain ([a-z' ]+?)\.", text, re.I)
        return BASE + (relic(m.group(1), ctx) if m else RELIC) - RANDOM_CURSE
    if title == "break out":
        return BASE - max_hp_loss(num(r"lose (\d+) max hp", text), ctx)
    return BASE if title == "continue" else None


def round_tea_party(opt, ctx):
    # Royal Poison takes 4 HP at the start of every fight left: early it is a run's worth, near the end a full heal for little.
    title, text = _text(opt)
    if title == "enjoy your tea":
        return BASE + heal(ctx.max_hp, ctx) - 4 * ctx.fights_left * 12.0 / max(1, ctx.max_hp) * 0.5
    if title == "pick a fight":
        return BASE + RELIC - hp_cost(num(r"lose (\d+) hp", text), ctx)
    return BASE if title == "continue" else None


def room_full_of_cheese(opt, ctx):
    title, text = _text(opt)
    if title == "search":
        return BASE + RELIC - 3.0 + min(8.0, 0.35 * ctx.fights_left) - hp_cost(num(r"lose (\d+) hp", text), ctx)  # (1 Max HP a fight)
    if title == "gorge":
        return BASE + 2.0  # (two of eight Commons)
    return None


def spiraling_whirlpool(opt, ctx):
    title, text = _text(opt)
    if title == "drink":
        return BASE + heal(num(r"heal (\d+)", text), ctx)
    if title == "observe":
        return BASE + (3.0 if counts(ctx)["basics"] or not ctx.deck else 0.0)  # Spiral: a Strike or Defend played twice
    if title == "reach in":
        return BASE + RELIC - curse("writhe")
    return None


def spirit_grafter(opt, ctx):
    title, text = _text(opt)
    if title == "let it in":
        return BASE + heal(num(r"heal (\d+)", text), ctx) + 1.0  # (Metamorphosis: three free Attacks for 2 energy)
    if title == "rejection":
        return BASE + UPGRADE - hp_cost(num(r"lose (\d+) hp", text), ctx)
    return None


def stone_of_all_time(opt, ctx):
    from ..potions import boss_worth
    title, text = _text(opt)
    if title == "drink and lift":
        m = re.search(r"lose ([a-z' -]+?)\.", text, re.I)
        held = next((p for p in ctx.potions if m and str(p.get("name") or "").lower() == m.group(1).lower()), {"name": m.group(1) if m else ""})
        return BASE + max_hp_gain(num(r"gain (\d+) max hp", text)) - (1.0 + boss_worth(held) / 12.0)
    if title == "push":
        return BASE + 2.0 - hp_cost(num(r"lose (\d+) hp", text), ctx)  # Vigorous 8: 8 more on an Attack's first play
    return None


def sunken_statue(opt, ctx):
    # The Sword of Stone wants 5 Elites after it is taken: 6 runs in 298 fought 5 in the whole run.
    title, text = _text(opt)
    if title == "grab the sword":
        return BASE
    if title == "dive into the water":
        return BASE + gold(num(r"gain (\d+) gold", text), ctx) - hp_cost(num(r"lose (\d+) hp", text), ctx)
    return None


def sunken_treasury(opt, ctx):
    title, text = _text(opt)
    if title in ("first chest", "second chest"):
        return BASE + gold(num(r"gain (\d+) gold", text), ctx) - (curse("greed") if "greed" in text.lower() else 0.0)
    return None


def symbiote(opt, ctx):
    title, _ = _text(opt)
    if title == "approach":
        return BASE + (2.5 if counts(ctx)["big_attacks"] else 1.0)  # Corrupted: half again the damage of our best Attack, 2 HP a play
    if title == "kill with fire":
        return BASE + transforms(ctx)
    return None


def tablet_of_truth(opt, ctx):
    title, text = _text(opt)
    if title == "smash":
        return BASE + heal(num(r"heal (\d+)", text), ctx)
    if title == "give up":
        return BASE
    if "upgrade all" in text.lower():
        return BASE + min(12.0, 0.8 * len(ctx.deck)) - max_hp_loss(num(r"lose (\d+) max hp", text), ctx)
    if "decipher" in title:
        return BASE + random_upgrades(1, ctx) - max_hp_loss(num(r"lose (\d+) max hp", text), ctx)
    return None


def the_lantern_key(opt, ctx):
    """Keep the Key: a fight (about 27 HP), then the Key (an Unplayable card) opens the War Historian's event in the next act: two
    relics and two potions, or History Course. Nothing in act 3, which has no next act. It had been returned 16 times in 16."""
    title, text = _text(opt)
    if title == "return the key":
        return BASE + gold(num(r"gain (\d+) gold", text), ctx)
    if title == "keep the key":
        prize = MEET_WAR_HISTORIAN * (2 * RELIC + 2 * POTION) if ctx.act < 3 else 0.0  # (if the event is met)
        return BASE + prize - hp_cost(27, ctx) - 1.0
    return BASE if title == "fight" else None


def the_legends_were_true(opt, ctx):
    # The Spoils Map: an Unplayable card that marks 600 Gold on the next act's map (the route walks to it: strategy.SPOILS).
    title, text = _text(opt)
    if title == "nab the map":
        return BASE + (0.8 * gold(600, ctx) if ctx.act < 3 else 0.0) - 1.0
    if title == "slowly find an exit":
        return BASE + potions_gained(1, ctx) - hp_cost(num(r"lose (\d+) hp", text), ctx)
    return None


def this_or_that(opt, ctx):
    title, text = _text(opt)
    if title == "that":
        m = re.search(r"add ([a-z' ]+?) to your deck", text, re.I)
        return BASE + RELIC - curse(m.group(1) if m else "clumsy")
    if title == "this":
        return BASE + gold(num(r"gain (\d+) gold", text), ctx) - hp_cost(num(r"lose (\d+) hp", text), ctx)
    return None


_RIDER = {"energized": 3.0, "wisdom": 2.5, "violence": 2.5, "choking": 2.2, "expertise": 2.0, "sapping": 1.8, "improvement": 1.5, "chaos": 1.0, "curious": 0.8}


def tinker_time(opt, ctx):
    title, _ = _text(opt)
    if title in _RIDER:
        return BASE + _RIDER[title]
    if title in ("weapon", "protector", "gadget"):
        c = counts(ctx)
        short_of_block = c["blocks"] < max(3, c["attacks"] // 2)
        return BASE + {"weapon": 1.0 if short_of_block else 1.5, "protector": 1.5 if short_of_block else 1.0, "gadget": 0.6}[title]
    return BASE + 1.0 if title == "accept" else None


def trial(opt, ctx):
    title, text = _text(opt)
    low = text.lower()
    if title in ("accept",):
        return BASE + 1.0
    if title == "double down":
        return -1000.0  # "Face lethal repercussions."
    if title.startswith("decide"):
        score = BASE
        for name in ("regret", "shame", "doubt"):
            if name in low:
                score -= curse(name)
        if "2 random relics" in low:
            score += 2 * RELIC
        if m := re.search(r"obtain (\d+) gold", low):
            score += gold(int(m.group(1)), ctx)
        if "upgrade 2" in low:
            score += 2 * 3.5
        if "transform 2" in low:
            score += transforms(ctx, 2)
        if "2 card rewards" in low:
            score += 2 * CARD_REWARD
        if m := re.search(r"heal (\d+)", low):
            score += heal(int(m.group(1)), ctx)
        return score
    return None


def unrest_site(opt, ctx):
    # Poor Sleep is Unplayable and Retained: drawn once, it holds a card's place in every hand after.
    title, text = _text(opt)
    if title == "rest anyways":
        return BASE + heal(ctx.max_hp, ctx) - curse("poor sleep")
    if title == "kill the trees":
        return BASE + RELIC - max_hp_loss(num(r"lose (\d+) max hp", text), ctx)
    return None


def war_historian_repy(opt, ctx):
    title, _ = _text(opt)
    if title == "unlock the chest":
        return BASE + 2 * RELIC + potions_gained(2, ctx)
    if title == "unlock the cage":
        return BASE + RELIC + 4.0  # History Course: our last Attack played again every turn
    return None


def waterfall_of_obvious_secrets(opt, ctx):
    # Dingy Dagger is a card (0 cost, 7 damage, upgraded any number of times), not a relic: Be Nice won 14 times in 14 as one.
    title, text = _text(opt)
    if title == "be nice":
        return BASE + 0.8
    if title == "beat up":
        return BASE + RELIC - num(r"add (\d+) guilty", text, 2) * curse("guilty")
    if title == "negotiate":
        return BASE + 2.5 + gold(-num(r"lose (\d+) gold", text), ctx)  # (a random Rare)
    return BASE if title == "enter waterfall" else None


def waterlogged_scriptorium(opt, ctx):
    title, text = _text(opt)
    if title == "bloody ink":
        return BASE + max_hp_gain(num(r"gain (\d+) max hp", text, 6))
    if title in ("prickly sponge", "tentacle quill"):
        return BASE + 1.0 * num(r"enchant (\d+) cards", text, 1) + gold(-num(r"pay (\d+) gold", text), ctx)  # Steady: Retain
    return None


def wellspring(opt, ctx):
    title, text = _text(opt)
    if title == "bathe":
        return BASE + removals(ctx) - num(r"add (\d+) guilty", text, 1) * curse("guilty")  # (Guilty goes by itself after 5 fights)
    if title == "bottle":
        return BASE + potions_gained(1, ctx)
    return None


def whispering_hollow(opt, ctx):
    title, text = _text(opt)
    if title == "exchange gold":
        return BASE + potions_gained(2, ctx) + gold(-num(r"lose (\d+) gold", text), ctx)
    if title == "hug the tree":
        return BASE + transforms(ctx) - hp_cost(num(r"lose (\d+) hp", text), ctx)
    return None


def wood_carvings(opt, ctx):
    # Slither ("randomize its cost from 0 to 3") is a loss on a card that costs 1: only a 2- or 3-cost card is worth it.
    from ...cards import parse
    title, _ = _text(opt)
    if title == "snake":
        dear = any(parse(c).cost >= 2 for c in ctx.deck)
        return BASE + (1.5 if dear else -0.5)
    if title in ("bird", "torus"):
        return BASE + (2.5 if counts(ctx)["basics"] or not ctx.deck else 0.0)  # (a Strike into Peck, a Defend into Toric Toughness)
    return None


def wuppy_in_trouble(opt, ctx):
    # Wuppy soaks every attack's damage until its HP is gone: 60 HP of it for an Injury beats 30 for 2 HP.
    title, text = _text(opt)
    soak = lambda n: 0.6 * 12.0 * n / max(1, ctx.max_hp)
    if title == "be cautious":
        return BASE + soak(num(r"with (\d+) hp", text, 30)) - hp_cost(num(r"lose (\d+) hp", text), ctx)
    if title == "help immediately":
        return BASE + soak(num(r"with (\d+) hp", text, 60)) - curse("injury")
    if title == "ignore":
        return BASE + potions_gained(1, ctx)
    return None


def zen_weaver(opt, ctx):
    title, text = _text(opt)
    price = gold(-num(r"pay (\d+) gold", text), ctx)
    if title == "emotional awareness":
        return BASE + removals(ctx) + price
    if title == "arachnid acupuncture":
        return BASE + removals(ctx, 2) + price
    if title == "breathing techniques":
        return BASE + 0.3 + price  # (Enlightenment: costs of the hand to 1 for a turn)
    return None


RULES = {
    "ABYSSAL_BATHS": abyssal_baths, "AMALGAMATOR": amalgamator, "AROMA_OF_CHAOS": aroma_of_chaos, "BATTLEWORN_DUMMY": battleworn_dummy, "BRAIN_LEECH": brain_leech,
    "COLORFUL_PHILOSOPHERS": colorful_philosophers, "COLOSSAL_FLOWER": colossal_flower, "CRYSTAL_SPHERE": crystal_sphere, "DENSE_VEGETATION": dense_vegetation,
    "DOLL_ROOM": doll_room, "DOORS_OF_LIGHT_AND_DARK": doors_of_light_and_dark, "DROWNING_BEACON": drowning_beacon, "ENDLESS_CONVEYOR": endless_conveyor,
    "FIELD_OF_MAN_SIZED_HOLES": field_of_man_sized_holes, "GRAVE_OF_THE_FORGOTTEN": grave_of_the_forgotten, "HUNGRY_FOR_MUSHROOMS": hungry_for_mushrooms,
    "JUNGLE_MAZE_ADVENTURE": jungle_maze_adventure, "LOST_WISP": lost_wisp, "LUMINOUS_CHOIR": luminous_choir, "MORPHIC_GROVE": morphic_grove,
    "MUSEUM_OF_TORTURE": museum_of_torture, "ROUND_TEA_PARTY": round_tea_party, "ROOM_FULL_OF_CHEESE": room_full_of_cheese, "SPIRALING_WHIRLPOOL": spiraling_whirlpool,
    "SPIRIT_GRAFTER": spirit_grafter, "STONE_OF_ALL_TIME": stone_of_all_time, "SUNKEN_STATUE": sunken_statue, "SUNKEN_TREASURY": sunken_treasury, "SYMBIOTE": symbiote,
    "TABLET_OF_TRUTH": tablet_of_truth, "THE_LANTERN_KEY": the_lantern_key, "THE_LEGENDS_WERE_TRUE": the_legends_were_true, "THIS_OR_THAT": this_or_that,
    "TINKER_TIME": tinker_time, "TRIAL": trial, "UNREST_SITE": unrest_site, "WAR_HISTORIAN_REPY": war_historian_repy, "WATERFALL_OF_OBVIOUS_SECRETS": waterfall_of_obvious_secrets,
    "WATERLOGGED_SCRIPTORIUM": waterlogged_scriptorium, "WELLSPRING": wellspring, "WHISPERING_HOLLOW": whispering_hollow, "WOOD_CARVINGS": wood_carvings,
    "WUPPY_IN_TROUBLE": wuppy_in_trouble, "ZEN_WEAVER": zen_weaver,
}
# Every event in the game (localization/eng/events.json, v0.113.1), and the ones the generic scorer is left to: checked one by one.
GENERIC_CHECKED = {
    "BUGSLAYER": "two cards by their ratings", "BYRDONIS_NEST": "7 Max HP against an egg to hatch", "INFESTED_AUTOMATON": "two random cards",
    "POTION_COURIER": "one Uncommon potion against three Foul ones", "PUNCH_OFF": "the fight priced by EVENT_FIGHT_HP", "RANWID_THE_ELDER": "relic trades by relicworth",
    "REFLECTIONS": "upgrades against a doubled deck and Bad Luck", "RELIC_TRADER": "relic trades by relicworth", "SAPPHIRE_SEED": "heal and upgrade against Sown",
    "SELF_HELP_BOOK": "an enchantment a card type", "SHRINE_OF_THE_WARLORD": "Mighty Wings and Clumsy against two cards", "SLIPPERY_BRIDGE": "the named card against HP",
    "TEA_MASTER": "three small teas", "THE_FUTURE_OF_POTIONS": "a potion for an upgraded card", "TRASH_HEAP": "HP for a relic against gold and a card",
    "WELCOME_TO_WONGOS": "relics for gold, the Mystery Box by _lives_through", "THERMAE_NONUPEIPE": "relics by name", "MOCK_EVENT_MODEL": "not in play",
}
COVERED = set(RULES) | set(GENERIC_CHECKED)


def value(event_id: str, opt: dict, ctx: Ctx) -> float | None:
    rule = RULES.get(event_id)
    return rule(opt, ctx) if rule else None
