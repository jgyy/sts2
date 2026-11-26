"""The first game's events, one rule each (2026-11-26: the game's texts from desktop-1.0.jar localization/eng/events.json, what
the bot met and chose in its logs). A rule returns an option's score on the generic scorer's scale (a relic 6, an upgrade 4),
or None to leave it to that scorer; `COVERED` names every event and what was checked of those left to it."""

from __future__ import annotations

import re

from ..relicworth import id_of as relic_id, worth as relic_worth
from .value import (CARD_REWARD, POTION, RELIC, UPGRADE, Ctx, card_worth, curse, gold, heal, hp_cost, is_curse, max_hp_gain, max_hp_loss, name_of, num,
                    potions_gained, random_upgrades, removals, transforms, ETERNAL)

BASE = 0.5
RUN = 25.0  # what the rest of the run is worth, in these points: a fight that may end it is priced by its chance of doing so


def _text(opt: dict) -> tuple[str, str]:
    return str(opt.get("title") or "").strip().lower(), str(opt.get("description") or "")


def _fight(share: float, ctx: Ctx) -> float:
    """A fight an event starts: the HP it is expected to cost (a share of Max HP)."""
    return hp_cost(round(share * ctx.max_hp), ctx)


def _lost_card(text: str, ctx: Ctx) -> float:
    """What losing the card an option names costs (a Strike or a curse gone is a gain)."""
    m = re.search(r"lose ([a-z' +.-]+?)(?:\.|$)", text, re.I)
    if not m:
        return 0.0
    card = next((c for c in ctx.deck if name_of(c) == name_of(m.group(1))), {"name": m.group(1).strip()})
    if is_curse(card):
        return -(curse(name_of(card)) + 1.0)
    from ...cards import is_basic
    if is_basic(card) or is_basic({"name": str(card.get("name") or "").rstrip("+")}):
        return -3.5
    return 0.8 * max(0.0, card_worth(card) * 2 + 3.0 - 2.0)


def back_to_basics(opt, ctx):
    # Elegance won 33 times in 33, at 5.5 against 4.5: upgrading every Strike and Defend is eight upgrades in an early deck.
    title, _ = _text(opt)
    if title == "elegance":
        return BASE + removals(ctx)
    if title == "simplicity":
        basics = [c for c in ctx.deck if ("strike" in name_of(c) or "defend" in name_of(c)) and not str(c.get("name") or "").endswith("+")]
        return BASE + min(10.0, 1.3 * len(basics)) if ctx.deck else BASE + 6.0
    return None


def beggar(opt, ctx):
    title, text = _text(opt)
    if title == "offer gold":
        return BASE + removals(ctx) + gold(-num(r"(\d+) gold", text, 75), ctx)
    return None


def big_fish(opt, ctx):
    title, text = _text(opt)
    if title == "banana":
        return BASE + heal(num(r"heal (\d+)", text), ctx)
    if title == "donut":
        return BASE + max_hp_gain(num(r"max hp \+(\d+)", text, 5))
    if title == "box":
        return BASE + RELIC - curse("regret")
    return None


def colosseum(opt, ctx):
    # VICTORY: two elites at once (Taskmaster and Gremlin Nob) for two relics, gold and a card; for most decks the end of the run.
    title, _ = _text(opt)
    if title == "victory":
        death = 0.4 if ctx.hp >= 0.9 * ctx.max_hp else 0.7  # (two elites at once: the bot loses a sixth of single act 2 elites)
        return BASE + 2 * RELIC + gold(100, ctx) + CARD_REWARD - _fight(0.6, ctx) - death * RUN
    if title == "cowardice":
        return BASE
    return None


def cursed_tome(opt, ctx):
    """Read on (1, 2, 3 HP) and take the book (10 HP: Necronomicon, Enchiridion or Nilry's Codex), or stop for 3. What has been
    paid is gone: each page is priced by what is still to come."""
    title, text = _text(opt)
    book = RELIC + 1.0
    if title == "read":
        return BASE + book - hp_cost(1 + 2 + 3 + 10, ctx)
    if title == "continue":
        step = num(r"lose (\d+) hp", text, 1)
        rest = {1: 2 + 3 + 10, 2: 3 + 10, 3: 10}.get(step, 10)
        return BASE + max(book - hp_cost(step + rest, ctx), -hp_cost(step + 3, ctx))
    if title == "take":
        return BASE + book - hp_cost(num(r"lose (\d+) hp", text, 10), ctx)
    if title == "stop":
        return BASE - hp_cost(num(r"lose (\d+) hp", text, 3), ctx)
    return None


def dead_adventurer(opt, ctx):
    # Three searches hold 30 Gold, a relic and nothing; each may bring the elite back (its rewards come with its fight).
    title, text = _text(opt)
    if title in ("search", "continue"):
        p = num(r"(\d+)%", text, 25) / 100.0
        loot = (RELIC + gold(30, ctx)) / 3.0
        return BASE + (1 - p) * loot + p * (RELIC + CARD_REWARD - _fight(0.3, ctx))
    return None


def designer(opt, ctx):
    title, text = _text(opt)
    low = text.lower()
    price = gold(-num(r"lose (\d+) gold", text), ctx)
    if title == "adjustments":
        return BASE + price + (UPGRADE if "upgrade a card" in low else random_upgrades(2, ctx))
    if title == "clean up":
        return BASE + price + (removals(ctx) if "remove a card" in low else transforms(ctx, 2))
    if title == "full service":
        return BASE + price + removals(ctx) + random_upgrades(1, ctx)
    if title == "punch":
        return BASE - hp_cost(num(r"lose (\d+) hp", text, 3), ctx)
    return None


def drug_dealer(opt, ctx):
    title, _ = _text(opt)
    if title == "ingest mutagens":
        return BASE + RELIC - 2.0  # Mutagenic Strength: 3 Strength for the first turn of each fight
    if title == "become test subject":
        return BASE + transforms(ctx, 2)
    if title == "test j.a.x.":
        return BASE + 0.5
    return None


def duplicator(opt, ctx):
    title, _ = _text(opt)
    return BASE + 2.0 if title == "pray" else None  # (the best card, twice)


def facetrader(opt, ctx):
    title, text = _text(opt)
    if title == "touch":
        return BASE + gold(num(r"gain (\d+) gold", text), ctx) - hp_cost(num(r"lose (\d+) hp", text), ctx)
    if title == "trade":
        return BASE + 0.5  # (half a good face: Face of Cleric, Ssserpent Head; half a bad one: Gremlin Visage, N'loth's Hungry Face)
    return None


def falling(opt, ctx):
    # Each option loses the card it names (one of each type at random): all three were 0.5 and the first went, whatever it was.
    title, text = _text(opt)
    if title in ("land", "channel", "strike"):
        return BASE - _lost_card(text, ctx)
    return None


def forgotten_altar(opt, ctx):
    title, text = _text(opt)
    if title.startswith("offer"):
        return BASE + RELIC - 3.0 + 1.0  # Bloody Idol for the Golden Idol
    if title == "sacrifice":
        return BASE + max_hp_gain(num(r"gain (\d+) max hp", text, 5)) - hp_cost(num(r"lose (\d+) hp", text), ctx)
    if title == "desecrate":
        return BASE - curse("decay")
    return None


def fountain_of_cleansing(opt, ctx):
    title, _ = _text(opt)
    if title == "drink":
        return BASE + sum(curse(name_of(c)) + 1.0 for c in ctx.deck if is_curse(c) and name_of(c) not in ETERNAL)
    return None


def golden_shrine(opt, ctx):
    title, text = _text(opt)
    if title in ("pray", "desecrate"):
        return BASE + gold(num(r"gain (\d+) gold", text), ctx) - (curse("regret") if "regret" in text.lower() else 0.0)
    return None


def golden_wing(opt, ctx):
    title, text = _text(opt)
    if title == "pray":
        return BASE + removals(ctx) - hp_cost(num(r"lose (\d+) hp", text), ctx)
    if title == "destroy":
        lo, hi = num(r"gain (\d+)", text, 50), num(r"-\s*(\d+)", text, 80)
        return BASE + gold((lo + hi) // 2, ctx)
    return None


def lab(opt, ctx):
    title, _ = _text(opt)
    return BASE + potions_gained(3, ctx) if title == "search" else None


def liars_game(opt, ctx):
    title, text = _text(opt)
    if title == "agree":
        return BASE + gold(num(r"gain (\d+) gold", text), ctx) - curse("doubt")
    return BASE if title == "disagree" else None


def living_wall(opt, ctx):
    title, _ = _text(opt)
    if title == "forget":
        return BASE + removals(ctx)
    if title == "change":
        return BASE + transforms(ctx)
    if title == "grow":
        return BASE + UPGRADE
    return None


def masked_bandits(opt, ctx):
    # "Lose ALL of your Gold" was read as the end of the run (-999.5): it is the gold we hold, and their fight can cost a third of our HP.
    title, _ = _text(opt)
    if title == "pay":
        return BASE + gold(-ctx.gold, ctx)
    if title == "fight!":
        return BASE + RELIC - 4.0 + CARD_REWARD + gold(30, ctx) - _fight(0.3, ctx)  # (Red Mask, gold, a card)
    return None


def mushrooms(opt, ctx):
    title, text = _text(opt)
    if title == "stomp":
        return BASE + RELIC - 3.0 + CARD_REWARD - _fight(0.15, ctx)  # (three Fungi Beasts, Odd Mushroom)
    if title == "eat":
        return BASE + heal(num(r"heal (\d+)", text), ctx) - curse("parasite")
    return None


def mysterious_sphere(opt, ctx):
    title, _ = _text(opt)
    if title == "open sphere":
        return BASE + RELIC + 2.0 + CARD_REWARD - _fight(0.4, ctx)  # (two Orb Walkers for a Rare relic: one alone costs a fifth of Max HP)
    return None


def neow_event(opt, ctx):
    """Neow's blessings, by what each does (both the small four after a short run and the full set): a random boss relic
    for the starter one is the strongest start there is for most decks; three fights of 1-HP enemies cost nothing."""
    title, text = _text(opt)
    low = f"{title} {text}".lower()
    if "boss relic" in low and "starting relic" in low:
        return BASE + 3.5  # (most boss relics are an energy a turn; Runic Dome would hide the intents the planner reads)
    if "next three combats have 1 hp" in low:
        return BASE + 2.5
    return None


def nest(opt, ctx):
    title, text = _text(opt)
    if title == "stay in line":
        return BASE + 1.5 - hp_cost(num(r"lose (\d+) hp", text), ctx)  # Ritual Dagger: 15 damage, more for each kill
    if title == "smash and grab":
        return BASE + gold(num(r"obtain (\d+) gold", text), ctx)
    return None


def note_for_yourself(opt, ctx):
    # The card stored for the next run leaves this deck: the worst one (strategy.card_select takes "store" worst-first).
    title, text = _text(opt)
    if title == "take and give":
        m = re.search(r"receive ([a-z' +.-]+?) and store", text, re.I)
        return BASE + (card_worth(m.group(1)) + 1.0 if m else 0.5) + removals(ctx) * 0.8
    return BASE if title == "ignore" else None


def n_loth(opt, ctx):
    # N'loth's Gift (three times the Rare cards) for one of our relics: given 7 times in 7 at 0.5 each, Pure Water and Cauldron among them.
    title, _ = _text(opt)
    if title.startswith("offer"):
        m = re.match(r"offer:?\s*(.+)", title)
        held = relic_id(m.group(1)) if m else ""
        return BASE + 2.0 - (RELIC + relic_worth(held, ctx.deck, ctx.act, ctx.hp, ctx.max_hp))
    return None


def purifier(opt, ctx):
    title, _ = _text(opt)
    return BASE + removals(ctx) if title == "pray" else None


def scrap_ooze(opt, ctx):
    # Each reach costs a little more HP for a better chance at a relic: priced on its own, not as one more bite of a bleeding event.
    title, text = _text(opt)
    if title in ("reach inside", "deeper"):
        p = num(r"(\d+)%", text, 25) / 100.0
        return BASE + p * RELIC - hp_cost(num(r"lose (\d+) hp", text, 3), ctx)
    return None


def sensory_stone(opt, ctx):
    title, text = _text(opt)
    if title == "recall":
        return BASE + 0.9 * num(r"add (\d+) colorless", text, 1) - hp_cost(num(r"lose (\d+) hp", text), ctx)
    return None


def shining_light(opt, ctx):
    title, text = _text(opt)
    if title == "enter":
        return BASE + random_upgrades(2, ctx) - hp_cost(num(r"lose (\d+) hp", text), ctx)
    return None


def the_cleric(opt, ctx):
    title, text = _text(opt)
    price = gold(-num(r"(\d+) gold", text), ctx)
    if title == "heal":
        return BASE + heal(num(r"heal (\d+)", text), ctx) + price
    if title == "purify":
        return BASE + removals(ctx) + price
    return None


def the_joust(opt, ctx):
    title, text = _text(opt)
    if title in ("murderer", "owner"):
        bet, win = num(r"bet (\d+)", text, 50), num(r"win (\d+)", text, 100)
        p = num(r"(\d+)%", text, 70) / 100.0
        return BASE + p * gold(win - bet, ctx) + (1 - p) * gold(-bet, ctx)
    return None


def the_library(opt, ctx):
    # A card of our choosing from twenty: the best a reward can be (it lost to a heal of a few HP at 1.5).
    title, text = _text(opt)
    if title == "read":
        return BASE + 3.0
    if title == "sleep":
        return BASE + heal(num(r"heal (\d+)", text), ctx)
    return None


def the_mausoleum(opt, ctx):
    title, text = _text(opt)
    if title == "open coffin":
        return BASE + RELIC - num(r"(\d+)%", text, 50) / 100.0 * curse("writhe")
    return None


def the_moai_head(opt, ctx):
    title, text = _text(opt)
    if title == "jump inside":
        return BASE + heal(ctx.max_hp, ctx) - max_hp_loss(num(r"lose (\d+) max hp", text), ctx)
    if title.startswith("offer"):
        return BASE + gold(num(r"gain (\d+) gold", text, 333), ctx) - 2.0  # (the Golden Idol's quarter more of the gold still to come goes with it)
    return None


def the_woman_in_blue(opt, ctx):
    # Three potions for 40 Gold beat one for 20 with room on the belt: all three were 4.0 and one was bought every time.
    title, text = _text(opt)
    if m := re.match(r"buy (\d+) potions?", title):
        return BASE + potions_gained(int(m.group(1)), ctx) + gold(-num(r"(\d+) gold", text), ctx)
    if title == "leave":
        return BASE - hp_cost(num(r"take (\d+) damage", text), ctx)
    return None


def thinking_with_puddles(opt, ctx):
    title, text = _text(opt)
    if title == "gooped":
        return BASE - 3.5  # (a Rare card of ours)
    if title == "land face first":
        return BASE - hp_cost(num(r"take (\d+) damage", text), ctx) - curse("injury")
    return None


def tomb_of_lord_red_mask(opt, ctx):
    title, text = _text(opt)
    if title.startswith("offer"):
        return BASE + RELIC + 0.5 + gold(-ctx.gold, ctx)  # Red Mask for all our gold (-993.5 before: "Lose all Gold" was ruin)
    if title == "don the red mask":
        return BASE + gold(num(r"gain (\d+) gold", text, 222), ctx)
    return None


def transmorgrifier(opt, ctx):
    title, _ = _text(opt)
    return BASE + transforms(ctx) if title == "pray" else None


def upgrade_shrine(opt, ctx):
    title, _ = _text(opt)
    return BASE + UPGRADE if title == "pray" else None


def we_meet_again(opt, ctx):
    title, text = _text(opt)
    if title == "give card":
        return BASE + RELIC - _lost_card(text, ctx)
    if title == "give gold":
        return BASE + RELIC + gold(-num(r"lose (\d+) gold", text), ctx)
    if title == "give potion":
        return BASE + RELIC - POTION - 0.5
    return BASE if title == "attack" else None


def winding_halls(opt, ctx):
    title, text = _text(opt)
    if title == "embrace madness":
        return BASE + 2 * 0.8 - hp_cost(num(r"lose (\d+) hp", text), ctx)  # (Madness: a card of the hand free)
    if title == "retrace your steps":
        return BASE - max_hp_loss(num(r"lose (\d+) max hp", text), ctx)
    if title == "focus":
        return BASE + heal(num(r"heal (\d+)", text), ctx) - curse("writhe")
    return None


def world_of_goop(opt, ctx):
    title, text = _text(opt)
    if title == "gather gold":
        return BASE + gold(num(r"gain (\d+) gold", text), ctx) - hp_cost(num(r"lose (\d+) hp", text), ctx)
    if title == "leave it":
        return BASE + gold(-num(r"lose (\d+) gold", text), ctx)
    return None


def addict(opt, ctx):
    title, text = _text(opt)
    if title == "offer gold":
        return BASE + RELIC + gold(-num(r"(\d+) gold", text, 85), ctx)
    if title == "rob":
        return BASE + RELIC - curse("shame")
    return None


RULES = {
    "ADDICT": addict, "BACK_TO_BASICS": back_to_basics, "BEGGAR": beggar, "BIG_FISH": big_fish, "COLOSSEUM": colosseum, "CURSED_TOME": cursed_tome,
    "DEAD_ADVENTURER": dead_adventurer, "DESIGNER": designer, "DRUG_DEALER": drug_dealer, "DUPLICATOR": duplicator, "FACETRADER": facetrader, "FALLING": falling,
    "FORGOTTEN_ALTAR": forgotten_altar, "FOUNTAIN_OF_CLEANSING": fountain_of_cleansing, "GOLDEN_SHRINE": golden_shrine, "GOLDEN_WING": golden_wing, "LAB": lab,
    "LIARS_GAME": liars_game, "LIVING_WALL": living_wall, "MASKED_BANDITS": masked_bandits, "MUSHROOMS": mushrooms, "MYSTERIOUS_SPHERE": mysterious_sphere,
    "NEOW_EVENT": neow_event, "NEST": nest, "NOTEFORYOURSELF": note_for_yourself, "N_LOTH": n_loth, "PURIFIER": purifier, "SCRAP_OOZE": scrap_ooze,
    "SENSORYSTONE": sensory_stone, "SHINING_LIGHT": shining_light, "THE_CLERIC": the_cleric, "THE_JOUST": the_joust, "THE_LIBRARY": the_library,
    "THE_MAUSOLEUM": the_mausoleum, "THE_MOAI_HEAD": the_moai_head, "THE_WOMAN_IN_BLUE": the_woman_in_blue, "THINKING_WITH_PUDDLES": thinking_with_puddles,
    "TOMB_OF_LORD_RED_MASK": tomb_of_lord_red_mask, "TRANSMORGRIFIER": transmorgrifier, "UPGRADE_SHRINE": upgrade_shrine, "WEMEETAGAIN": we_meet_again,
    "WINDING_HALLS": winding_halls, "WORLD_OF_GOOP": world_of_goop,
}
GENERIC_CHECKED = {
    "ACCURSED_BLACKSMITH": "an upgrade against Warped Tongs and Pain (edit 217)", "BONFIRE_ELEMENTALS": "the offer is the card select's, worst first",
    "GHOSTS": "half our Max HP for five Apparitions: refused (the planner does not price our Intangible)", "GOLDEN_IDOL": "the idol, then the least of three traps",
    "KNOWING_SKULL": "PAID_EXIT (edit 228)", "MATCH_AND_KEEP": "a memory game the option scores cannot play: first option", "MINDBLOOM": "edit 239",
    "SECRETPORTAL": "TO_BOSS (edit 237)", "SPIRE_HEART": "act 4's door", "VAMPIRES": "a third of our Max HP for Bites: refused", "WHEEL_OF_CHANGE": "one option",
    "PROCEED_SCREEN": "not an event",
}
COVERED = set(RULES) | set(GENERIC_CHECKED)


def value(event_id: str, opt: dict, ctx: Ctx) -> float | None:
    rule = RULES.get(event_id)
    return rule(opt, ctx) if rule else None
