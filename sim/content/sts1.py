"""The first Slay the Spire, as the simulator plays it: cards from the bot's own text tables, monsters from
sts1_foes.py, and the run's rewards, shops, relics and potions at Ascension 0 values."""

from __future__ import annotations

import re
from functools import lru_cache

from sts2bot.games.sts1 import text
from sts2bot.games.sts1.text import potions as potion_tables
from sts2bot.run import strategy

from ..cards import Card, CardDef
from ..effects import add
from ..monsters import Foe
from ..run import Potion
from . import common, sts1_cards, sts1_foes

KEY = "sts1"
ACTS = 3
SHORT_REASON = False  # (CommunicationMod marks a card it cannot pay for unplayable and says no more)
KEEP_POTIONS = set(potion_tables.KEEP)
UNKNOWN_ODDS = {"Monster": 0.1, "Shop": 0.03, "Treasure": 0.02}
ROOM_WEIGHTS = {"Monster": 0.53, "Unknown": 0.22, "RestSite": 0.12, "Shop": 0.05, "Elite": 0.08}
CARD_ODDS = {"monster": [("Rare", 0.03), ("Uncommon", 0.37), ("Common", 0.60)], "elite": [("Rare", 0.10), ("Uncommon", 0.40), ("Common", 0.50)]}

DAMAGE, PRE, AFTER, POWER = sts1_cards.DAMAGE, sts1_cards.PRE, sts1_cards.AFTER, sts1_cards.POWER
PLAYABLE, DRAWN, RETAINED, X_PLUS = sts1_cards.PLAYABLE, sts1_cards.DRAWN, sts1_cards.RETAINED, sts1_cards.X_PLUS

CHARACTERS = {
    "IRONCLAD": {"title": "The Ironclad", "hp": 80, "deck": ["STRIKE_R"] * 5 + ["DEFEND_R"] * 4 + ["BASH"], "relics": ["BURNING_BLOOD"]},
    "THE_SILENT": {"title": "The Silent", "hp": 70, "deck": ["STRIKE_G"] * 5 + ["DEFEND_G"] * 5 + ["NEUTRALIZE", "SURVIVOR"],
                   "relics": ["RING_OF_THE_SNAKE"]},
    "DEFECT": {"title": "The Defect", "hp": 75, "deck": ["STRIKE_B"] * 4 + ["DEFEND_B"] * 4 + ["ZAP", "DUALCAST"], "relics": ["CRACKED_CORE"], "orbs": 3},
    "WATCHER": {"title": "The Watcher", "hp": 72, "deck": ["STRIKE_P"] * 4 + ["DEFEND_P"] * 4 + ["ERUPTION", "VIGILANCE"], "relics": ["PURE_WATER"]},
}
COLORS = {k: char for char, mod in (("IRONCLAD", text.ironclad), ("THE_SILENT", text.silent), ("DEFECT", text.defect), ("WATCHER", text.watcher)) for k in mod.CARDS}


def _defs() -> dict[str, CardDef]:
    out = {}
    for key, kind, cost, rarity, aimed in re.findall(r"([A-Z_]+) (St|Cu|[ASP])(\d|X|U)([BCURS])(t?)", sts1_cards.META):
        cost_n = -1 if cost == "X" else -2 if cost == "U" else int(cost)
        base, up = text.UPGRADE_COST.get(key, (cost_n, cost_n))
        name = sts1_cards.NAMES.get(key) or key.replace("_", " ").title()
        ctype = sts1_cards.TYPES[kind]
        color = COLORS.get(key) or {"Status": "STATUS", "Curse": "CURSE"}.get(ctype, "COLORLESS")
        rar = {"Status": "Status", "Curse": "Curse"}.get(ctype, sts1_cards.RARITIES[rarity])
        out[key] = CardDef(key, name, ctype, base, rar, bool(aimed), text.card_text(key, name, False), text.card_text(key, name, True), up, color)
    return out


CARDS = _defs()


def make(key: str, up: bool = False) -> Card:
    return Card(CARDS[key], up)


@lru_cache(maxsize=None)
def pool(character: str) -> list[CardDef]:
    return [d for d in CARDS.values() if d.color == character and d.rarity in ("Common", "Uncommon", "Rare")]


@lru_cache(maxsize=None)
def colorless() -> list[CardDef]:
    return [d for d in CARDS.values() if d.color == "COLORLESS" and d.rarity in ("Uncommon", "Rare") and d.key not in _NO_SHOP]


_NO_SHOP = {"DISCOVERY", "JACK_OF_ALL_TRADES", "CHRYSALIS", "METAMORPHOSIS", "TRANSMUTATION", "MAGNETISM", "MAYHEM", "THE_BOMB", "VIOLENCE",
            "SECRET_TECHNIQUE", "SECRET_WEAPON", "MASTER_OF_STRATEGY", "PURITY", "FORETHOUGHT", "IMPATIENCE"}

# ------------------------------------------------------------------ hooks the engine asks for


def EXTRA(f, c, foe, i, xs) -> None:
    for kind in i.channel * xs:
        add(f.pw, f"{kind}_CHANNELED", 1)


def START(f) -> None:
    run = f.run
    if run.has("CRACKED_CORE"):
        f.channel("LIGHTNING")
    f.add_new("MIRACLE", run.has("PURE_WATER") + 3 * run.has("HOLY_WATER"))
    if run.has("SNECKO_EYE"):
        f.pw["CONFUSED"] = 1
    if run.has("SLAVER_S_COLLAR") and f.kind != "monster":
        f.pw.setdefault("ENERGY_NEXT_TURN", 0)
    for foe in f.foes:
        if foe.key == "DARKLING":
            foe.powers["REATTACH"] = foe.max_hp // 2


def TURN_START(f) -> None:
    if f.run.has("SLAVER_S_COLLAR") and f.kind != "monster":
        f.energy += 1
    if f.run.has("INSERTER") and f.round % 2 == 0:
        f.orb_slots += 1


def TURN_CARDS(f) -> None:
    for _ in range(f.pw.get("CREATIVE_AI", 0)):
        f.add_card(Card(f.rng.choice([d for d in pool(f.run.character) if d.type == "Power"])))
    for _ in range(f.pw.get("HELLO_WORLD", 0)):
        f.add_card(Card(f.rng.choice([d for d in pool(f.run.character) if d.rarity == "Common"])))
    for _ in range(f.pw.get("MAYHEM", 0)):
        sts1_cards._havoc(f, None, (f.alive() or [None])[0], 0, 0)


TURN_END = lambda f: None
POTION_CARD, ROUND_END = common.potion_card, sts1_foes.darkling_round


# ------------------------------------------------------------------ relics

COMMON = ["ANCHOR", "BAG_OF_MARBLES", "BAG_OF_PREPARATION", "BLOOD_VIAL", "BRONZE_SCALES", "HAPPY_FLOWER", "LANTERN", "MEAL_TICKET", "NUNCHAKU",
          "ODDLY_SMOOTH_STONE", "ORICHALCUM", "PEN_NIB", "POTION_BELT", "PRESERVED_INSECT", "REGAL_PILLOW", "STRAWBERRY", "THE_BOOT",
          "TOY_ORNITHOPTER", "VAJRA", "WAR_PAINT", "WHETSTONE", "DREAM_CATCHER", "CERAMIC_FISH", "SMILING_MASK", "MAW_BANK"]
UNCOMMON = ["KUNAI", "SHURIKEN", "ORNAMENTAL_FAN", "LETTER_OPENER", "MEAT_ON_THE_BONE", "MERCURY_HOURGLASS", "HORN_CLEAT", "PEAR",
            "GREMLIN_HORN", "PAPER_PHROG", "MOLTEN_EGG", "TOXIC_EGG", "FROZEN_EGG", "INK_BOTTLE", "QUESTION_CARD", "STRIKE_DUMMY",
            "ETERNAL_FEATHER", "SINGING_BOWL"]
RARE = ["DEAD_BRANCH", "GINGER", "GIRYA", "INCENSE_BURNER", "LIZARD_TAIL", "MANGO", "OLD_COIN", "PEACE_PIPE", "PRAYER_WHEEL", "SHOVEL",
        "STONE_CALENDAR", "THREAD_AND_NEEDLE", "TORII", "TUNGSTEN_ROD", "TURNIP", "CALIPERS", "CAPTAIN_S_WHEEL", "FOSSILIZED_HELIX",
        "CHARON_S_ASHES", "BIRD_FACED_URN", "MAGIC_FLOWER"]
BOSS = ["COFFEE_DRIPPER", "CURSED_KEY", "ECTOPLASM", "FUSION_HAMMER", "PHILOSOPHER_S_STONE", "RUNIC_DOME", "SLAVER_S_COLLAR", "SOZU",
        "VELVET_CHOKER", "BUSTED_CROWN", "SNECKO_EYE", "RUNIC_PYRAMID", "SACRED_BARK", "BLACK_STAR", "WHITE_BEAST_STATUE", "TINY_HOUSE",
        "ASTROLABE", "PANDORA_S_BOX", "CALLING_BELL", "EMPTY_CAGE", "ORRERY"]
OWN_BOSS = {"IRONCLAD": ["BLACK_BLOOD", "MARK_OF_PAIN"], "THE_SILENT": ["RING_OF_THE_SERPENT"], "DEFECT": ["FROZEN_CORE", "NUCLEAR_BATTERY", "INSERTER"],
            "WATCHER": ["HOLY_WATER", "VIOLET_LOTUS"]}
ENERGY = {"COFFEE_DRIPPER", "CURSED_KEY", "ECTOPLASM", "FUSION_HAMMER", "PHILOSOPHER_S_STONE", "RUNIC_DOME", "SOZU", "VELVET_CHOKER",
          "BUSTED_CROWN", "MARK_OF_PAIN", "NUCLEAR_BATTERY"}
REPLACES = {"BLACK_BLOOD": "BURNING_BLOOD", "RING_OF_THE_SERPENT": "RING_OF_THE_SNAKE", "FROZEN_CORE": "CRACKED_CORE", "HOLY_WATER": "PURE_WATER"}
TEXT = {
    **potion_tables.RELICS,
    "VELVET_CHOKER": "Gain 1 Energy at the start of each turn. You cannot play more than 6 cards per turn.",
    "COFFEE_DRIPPER": "Gain 1 Energy at the start of each turn. You can no longer Rest at Rest Sites.",
    "FUSION_HAMMER": "Gain 1 Energy at the start of each turn. You can no longer Smith at Rest Sites.",
    "ECTOPLASM": "Gain 1 Energy at the start of each turn. You can no longer gain Gold.",
}


def relic_text(rid: str) -> str:
    return TEXT.get(rid, "")


def relic_pickup(run, rid: str) -> None:
    if rid in ENERGY:
        run.energy += 1
    old = REPLACES.get(rid)
    if old in run.relics:
        run.relics.remove(old)
    bump = {"STRAWBERRY": 7, "PEAR": 10, "MANGO": 14, "TINY_HOUSE": 5}.get(rid, 0)
    run.max_hp += bump
    run.hp += bump
    if rid == "POTION_BELT":
        run.potions += [None, None]
    elif rid == "OLD_COIN":
        run.gold += 300
    elif rid in ("WAR_PAINT", "WHETSTONE"):
        common.upgrade_random(run, lambda c: c.type == ("Skill" if rid == "WAR_PAINT" else "Attack"))
    elif rid == "EMPTY_CAGE":
        run.remove_one()
        run.remove_one()
    elif rid == "PANDORA_S_BOX":
        basics = [c for c in run.deck if c.d.rarity == "Basic" and c.name.rstrip("+") in ("Strike", "Defend")]
        for c in basics:
            run.deck.remove(c)
            run.deck.append(Card(run.rng.choice(run.game.pool(run.character))))
    elif rid == "ASTROLABE":
        for _ in range(3):
            c = run.choose_from_deck("transform", "Choose a card to Transform.")
            if c is not None:
                run.deck.remove(c)
                run.deck.append(Card(run.rng.choice(run.game.pool(run.character)), up=True))
    elif rid == "CALLING_BELL":
        run.deck.append(run.game.make("CURSE_OF_THE_BELL") if "CURSE_OF_THE_BELL" in run.game.CARDS else run.game.random_curse(run))
        for _ in range(3):
            run.gain_relic(run.game.random_relic(run))
    elif rid == "TINY_HOUSE":
        run.gold += 50
        run.upgrade_one()
        run.add_potion(run.game.random_potion(run))
        run.card_reward(run.game.card_choices(run, "monster"))
    elif rid == "ORRERY":
        for _ in range(5):
            run.card_reward(run.game.card_choices(run, "monster"))


def random_relic(run, chest: bool = False) -> str:
    rng = run.rng
    r = rng.random()
    if chest:
        size = rng.random()
        tier = (COMMON if r < 0.75 else UNCOMMON) if size < 0.5 else (COMMON if r < 0.35 else UNCOMMON if r < 0.85 else RARE) if size < 0.83 else (UNCOMMON if r < 0.75 else RARE)
    else:
        tier = COMMON if r < 0.5 else UNCOMMON if r < 0.83 else RARE
    left = [x for x in tier if x not in run.relics] or [x for x in COMMON + UNCOMMON + RARE if x not in run.relics] or ["CIRCLET"]
    return rng.choice(left)


def random_curse(run) -> Card:
    return make(run.rng.choice(("REGRET", "DOUBT", "SHAME", "INJURY", "CLUMSY", "DECAY", "WRITHE")))


def random_potion(run) -> Potion:
    names = [k for k in potion_tables.POTIONS if k not in ("FAIRY_IN_A_BOTTLE", "SMOKE_BOMB", "ENTROPIC_BREW", "ELIXIR", "GAMBLER_S_BREW",
                                                           "LIQUID_MEMORIES", "SNECKO_OIL", "STANCE_POTION", "AMBROSIA", "BOTTLED_MIRACLE",
                                                           "ESSENCE_OF_DARKNESS", "POTION_OF_CAPACITY", "FOCUS_POTION", "CUNNING_POTION")]
    own = {"IRONCLAD": [], "THE_SILENT": ["CUNNING_POTION"], "DEFECT": ["FOCUS_POTION", "ESSENCE_OF_DARKNESS", "POTION_OF_CAPACITY"],
           "WATCHER": ["AMBROSIA", "BOTTLED_MIRACLE", "STANCE_POTION"]}[run.character]
    key = run.rng.choice(names + own + ["FAIRY_IN_A_BOTTLE"])
    return Potion(key, key.replace("_S_", "'s ").replace("_", " ").title(), text.potion_text(key, key))


# ------------------------------------------------------------------ the run


def run_start(run) -> None:
    """Neow: three blessings, one of each kind, chosen by the bot's event handler."""
    rng = run.rng
    first = rng.choice([("Remove", "Remove a card from your Deck.", lambda r, a: r.remove_one()),
                        ("Upgrade", "Upgrade a card.", lambda r, a: r.upgrade_one()),
                        ("Learn", "Choose a card to add to your Deck.", lambda r, a: r.card_reward(card_choices(r, "monster"), can_skip=False))])
    second = rng.choice([("Relic", "Obtain a random common Relic.", lambda r, a: r.gain_relic(rng.choice([x for x in COMMON if x not in r.relics]))),
                         ("Gold", "Gain 100 Gold.", lambda r, a: setattr(r, "gold", r.gold + 100)),
                         ("Grow", f"Gain {run.max_hp // 10} Max HP.", lambda r, a: (setattr(r, "max_hp", r.max_hp + r.max_hp // 10), setattr(r, "hp", r.hp + r.max_hp // 11)))])
    third = rng.choice([("Rare", "Lose all Gold. Choose a Rare card to add to your Deck.",
                         lambda r, a: (setattr(r, "gold", 0), r.card_reward(card_choices(r, "boss"), can_skip=False))),
                        ("Brew", "Obtain 3 random potions.", lambda r, a: [r.add_potion(random_potion(r)) for _ in range(3)])])
    _choose(run, [first, second, third])


def _choose(run, options: list) -> None:
    state = run.screen("event", event={"event_id": "NEOW", "event_name": "Neow", "in_dialogue": False,
                                       "options": [{"index": i, "title": t, "description": d, "is_locked": False, "is_proceed": False}
                                                   for i, (t, d, _f) in enumerate(options)]})
    pick = 0
    for action, params in strategy.event(state, run.mem):
        if action == "choose_event_option":
            pick = params["index"]
            break
    run.digest.append(f"N{options[pick][0][:2]}")
    options[pick][2](run, 0)


def act_start(run) -> None:
    run.fights_in_act = 0


def make_map(run) -> dict:
    weights = dict(ROOM_WEIGHTS)
    if run.asc >= 1:
        weights["Elite"] *= 1.6
    return common.make_map(run.rng, weights, elite_from=6, treasure_row=9)


def encounter(run, kind: str) -> list[Foe]:
    act = run.act
    if kind == "monster":
        run.fights_in_act = getattr(run, "fights_in_act", 0) + 1
        pool_name = "weak" if run.fights_in_act <= sts1_foes.WEAK_FIGHTS[act] else "strong"
    else:
        pool_name = kind
    options = sts1_foes.ENCOUNTERS[act][pool_name]
    fresh = [e for e in options if _name(e) not in run.recent[-2:]] or options
    if kind == "boss":
        boss = run.__dict__.setdefault("bosses", {})
        if act not in boss:
            boss[act] = run.rng.choice(options)
        choice = boss[act]
    else:
        choice = run.rng.choice(fresh)
    run.recent.append(_name(choice))
    keys = choice(run.rng) if callable(choice) else list(choice)
    foes = [_new(key, n, run, kind) for n, key in enumerate(keys)]
    for foe in foes[1:] if keys[0] in sts1_foes.MINIONS else []:
        foe.minion = True
    return foes


def _name(e) -> str:
    """An encounter's name for the "not the last two again" rule (its id for a drawn one: lambdas all share a __name__)."""
    return f"drawn{id(e)}" if callable(e) else "+".join(e)


def _hp_mult(run, kind: str) -> float:
    a = run.asc
    return 1.0 + 0.08 * ((kind == "monster" and a >= 7) + (kind == "elite" and a >= 8) + (kind == "boss" and a >= 9))


def _new(key: str, n: int, run, kind: str, hp: int | None = None) -> Foe:
    spec = sts1_foes.S[key]
    return Foe(spec, n, hp if hp is not None else run.rng.randint(*spec.hp), _hp_mult(run, kind) if hp is None else 1.0)


def spawn(key: str, f, minion: bool = False, hp: int | None = None) -> Foe:
    if key == "GREMLIN":
        key = f.rng.choice(sts1_foes.GREMLINS)
    foe = _new(key, len(f.foes), f.run, f.kind, hp)
    foe.minion = minion or foe.minion
    foe.mem["fight"] = f
    if hp is not None:
        foe.max_hp = max(foe.max_hp, hp)
    return foe


def fight_won(run, f) -> None:
    heal = 6 * run.has("BURNING_BLOOD") + 12 * run.has("BLACK_BLOOD") + f.pw.get("SELF_REPAIR", 0)
    if run.has("MEAT_ON_THE_BONE") and run.hp <= run.max_hp // 2:
        heal += 12
    run.hp = min(run.max_hp, run.hp + heal)
    if run.has("MAW_BANK"):
        run.gold += 12


def boss_won(run) -> None:
    if run.act >= ACTS:
        return
    offered = run.rng.sample([b for b in BOSS + OWN_BOSS[run.character] if b not in run.relics], 3)
    offered.sort(key=lambda r: text.boss_relic_rank({"id": r, "name": r}))
    run.gain_relic(offered[0])
    run.digest.append(f"b{offered[0]}")
    run.card_reward(card_choices(run, "boss"))


def fight_gold(run, kind: str) -> int:
    if run.has("ECTOPLASM"):
        return 0
    return {"monster": run.rng.randint(10, 20), "elite": run.rng.randint(25, 35), "boss": run.rng.randint(95, 105)}[kind]


def card_choices(run, kind: str) -> list[Card]:
    n = 3 + run.has("QUESTION_CARD") - 2 * run.has("BUSTED_CROWN")
    up = {1: 0.0, 2: 0.25, 3: 0.5}.get(run.act, 0.5) * (0.5 if run.asc >= 12 else 1.0)
    return common.offer(run, pool(run.character), max(1, n), CARD_ODDS, kind if kind in CARD_ODDS or kind == "boss" else "monster", up)


def rest_options(run) -> list[dict]:
    opts = []
    if not run.has("COFFEE_DRIPPER"):
        opts.append("rest")
    if not run.has("FUSION_HAMMER"):
        opts.append("smith")
    if run.has("GIRYA") and run.counters.get("GIRYA", 0) < 3:
        opts.append("lift")
    if run.has("PEACE_PIPE"):
        opts.append("toke")
    if run.has("SHOVEL"):
        opts.append("dig")
    if run.has("ETERNAL_FEATHER"):
        run.hp = min(run.max_hp, run.hp + 3 * (len(run.deck) // 5))
    return [{"index": i, "id": o, "name": o.title(), "is_enabled": True} for i, o in enumerate(opts)]


def shop_stock(run) -> list[dict]:
    return common.shop_stock(run, {"Common": 50, "Uncommon": 75, "Rare": 150}, colorless_mult=1.2)


event = common.event


def max_hp_penalty(asc: int, character: str) -> int:
    return (5 if character == "IRONCLAD" else 4) if asc >= 14 else 0


def setup_run(run) -> None:
    if run.asc >= 10:
        run.deck.append(make("ASCENDER_S_BANE"))
    if run.asc >= 11:
        run.potions = run.potions[:2]
