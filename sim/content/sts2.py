"""Slay the Spire 2 (v0.111.1), as the simulator plays it: cards, monsters, potions and relic names from sts2_data.json.

Powers are read from their own text: "At the start of your turn, X", "At the end of your turn, X", "Whenever you
play a card/an Attack/a Skill/a Power, X", "Whenever a card is Exhausted, X", "Whenever you gain Block, X", "Gain
[E] at the start of each turn", and what they do at once; X is done as a card with that text would do it.
"""

from __future__ import annotations

import json
import os
import re
from functools import lru_cache


from .. import effects as fx
from ..cards import Card, CardDef
from ..monsters import Foe
from ..run import Potion
from . import common, sts1, sts2_foes

KEY = "sts2"
ACTS = 3
SHORT_REASON = True  # (the mod says why a card cannot be played: "EnergyCostTooHigh")
KEEP_POTIONS = {"FAIRY_IN_A_BOTTLE"}
UNKNOWN_ODDS = {"Monster": 0.1, "Shop": 0.03, "Treasure": 0.02}
ROOM_WEIGHTS = {"Monster": 0.42, "Unknown": 0.30, "RestSite": 0.12, "Shop": 0.05, "Elite": 0.07, }
CARD_ODDS = {"monster": [("Rare", 0.03), ("Uncommon", 0.37), ("Common", 0.60)], "elite": [("Rare", 0.10), ("Uncommon", 0.40), ("Common", 0.50)]}
PLAYED = ("Main", "IRONCLAD", "SILENT", "DEFECT", "NECROBINDER", "REGENT")

with open(os.path.join(os.path.dirname(__file__), "sts2_data.json")) as _f:
    DATA = json.load(_f)

RARITY = {"Basic": "Basic", "Common": "Common", "Uncommon": "Uncommon", "Rare": "Rare", "Ancient": "Ancient", "Event": "Event",
          "Token": "Token", "Status": "Status", "Curse": "Curse"}


def _defs() -> dict[str, CardDef]:
    out = {}
    for key, (name, char, ctype, cost, cost_up, rarity, target, text, text_up, kws, kws_up, star, star_up) in DATA["cards"].items():
        if ctype == "Quest":
            continue
        out[key] = CardDef(key, name, ctype, cost, rarity, target == "AnyEnemy", text, text_up, cost_up, char,
                           None if star in (None, 0) else star, None if star_up in (None, 0) else star_up, tuple(kws), tuple(kws_up))
    return out


CARDS = _defs()
CHARACTERS = {k: v for k, v in DATA["characters"].items() if k in PLAYED}


def make(key: str, up: bool = False) -> Card:
    key = {"ASCENDER_S_BANE": "ASCENDERS_BANE"}.get(key, key)
    return Card(CARDS[key], up)


@lru_cache(maxsize=None)
def pool(character: str) -> list[CardDef]:
    return [d for d in CARDS.values() if d.color == character and d.rarity in ("Common", "Uncommon", "Rare")]


@lru_cache(maxsize=None)
def colorless() -> list[CardDef]:
    return [d for d in CARDS.values() if d.color == "COLORLESS" and d.rarity in ("Uncommon", "Rare")]


# ------------------------------------------------------------------ cards the text does not cover


def _blade(f) -> Card | None:
    return next((c for c in f.hand + f.draw + f.discard if c.key == "SOVEREIGN_BLADE"), None)


def _forge(f, n: int) -> None:
    f.pw["FORGE"] = f.pw.get("FORGE", 0) + n
    if _blade(f) is None:
        f.add_card(make("SOVEREIGN_BLADE"))


def EXTRA(f, c, foe, i, xs) -> None:
    """Stars, Forge, Osty, Doom, Souls and Debris: what Slay the Spire 2's text says that the engine's generic part leaves out."""
    if i.star_gain:
        f.stars += i.star_gain
        _star_hooks(f, i.star_gain)
    if i.forge:
        _forge(f, i.forge * xs)
    if i.summon:
        _summon(f, i.summon * xs)
    if i.doom:
        targets = f.alive() if i.aoe else [foe] if foe is not None and foe.alive else []
        for v in targets:
            n = i.doom * xs + (5 * (v.powers.get("DOOM", 0) // 10) if c.key == "NO_ESCAPE" else 0)
            if c.key == "BLIGHT_STRIKE":
                n = f.attack_value(i.damage + c.bonus, v)
            fx.debuff(f, v, "DOOM", n)
            if f.pw.get("SHROUD"):
                f.gain_block(f.pw["SHROUD"])
        if i.doom_kills:
            for v in f.alive():
                if v.powers.get("DOOM", 0) >= v.hp:
                    f.kill(v)
    for _ in range(i.souls * xs):
        f.add_card(make("SOUL"), "draw")
    if i.debris:
        for _ in range(i.debris if i.debris > 0 else max(0, 10 - len(f.hand))):
            f.add_card(make("DEBRIS"))
    for kind in i.channel * xs:
        f.pw[f"{kind}_CHANNELED"] = f.pw.get(f"{kind}_CHANNELED", 0) + 1


def _summon(f, n: int) -> None:
    if f.osty < 0:
        return
    f.osty += n
    f.pw["OSTY_MAX"] = max(f.pw.get("OSTY_MAX", 0), f.osty)


def _star_hooks(f, n: int) -> None:
    if f.pw.get("BLACK_HOLE"):
        for v in f.alive():
            f.hurt(v, f.pw["BLACK_HOLE"] * abs(n), attack=False)


def _unleash(f, c):
    return (9 if c.up else 6) + max(0, f.osty)


def _protector(f, c):
    return (15 if c.up else 10) + f.pw.get("OSTY_MAX", 0)


DAMAGE = {
    "BODY_SLAM": lambda f, c: f.block,
    "SOVEREIGN_BLADE": lambda f, c: 10 + f.pw.get("FORGE", 0),
    "UNLEASH": _unleash,
    "PROTECTOR": _protector,
    "TIMES_UP": lambda f, c: 0,
    "DEATH_MARCH": lambda f, c: (10 if c.up else 8) + (5 if c.up else 4) * f.pw.get("DRAWN_TURN", 0),
    "RAMPAGE": lambda f, c: 10 + c.bonus,
}


def _times_up(f, c, foe, i):
    return i if foe is None else __import__("dataclasses").replace(i, damage=foe.powers.get("DOOM", 0), hits=1)


def _osty_alive(f, c, foe, i):
    from dataclasses import replace
    if f.osty <= 0 and i.osty_attack:
        return replace(i, damage=0)
    return i


PRE = {
    "TIMES_UP": _times_up,
    "FLATTEN": _osty_alive, "POKE": _osty_alive, "RATTLE": _osty_alive, "SNAP": _osty_alive, "SIC_EM": _osty_alive, "UNLEASH": _osty_alive,
    "FIEND_FIRE": sts1.sts1_cards._fiend_fire, "BURNING_PACT": sts1.sts1_cards._burning_pact,
    "SECOND_WIND": lambda f, c, foe, i: __import__("dataclasses").replace(i, block=0),
}


def _sacrifice(f, c, foe, x, dealt):
    if f.osty > 0:
        f.osty = 0
        f.gain_block(3 * f.pw.get("OSTY_MAX", 0))


def _spur(f, c, foe, x, dealt):
    if f.osty > 0:
        f.osty += 5


def _expose(f, c, foe, x, dealt):
    if foe is not None:
        foe.block = 0
        foe.powers.pop("ARTIFACT", None)


def _up_my_sleeve(f, c, foe, x, dealt):
    c.cost_fight = max(0, c.cost() - 1)


def _summon_forth(f, c, foe, x, dealt):
    blade = _blade(f)
    if blade is None:
        f.add_card(make("SOVEREIGN_BLADE"))
    elif blade not in f.hand:
        (f.draw if blade in f.draw else f.discard).remove(blade)
        f.add_card(blade)


def _knockout(f, c, foe, x, dealt):
    if foe is not None and not foe.alive:
        f.stars += 5


def _hang(f, c, foe, x, dealt):
    if foe is not None:
        foe.powers["HANG"] = max(2, foe.powers.get("HANG", 0) * 2)


def _sweep(f, c, foe, x, dealt):
    pass


AFTER = {
    **{k: sts1.AFTER[k] for k in ("HAVOC", "TRUE_GRIT", "BATTLE_TRANCE", "DUAL_WIELD", "INFERNAL_BLADE", "RAGE", "DOUBLE_TAP", "LIMIT_BREAK",
                                  "SURVIVOR", "ACROBATICS", "DAGGER_THROW", "PREPARED", "CALCULATED_GAMBLE", "BULLET_TIME", "BURST",
                                  "STORM_OF_STEEL", "UNLOAD", "CHAOS", "DOUBLE_ENERGY", "WHITE_NOISE", "REBOOT", "SEEK", "HOLOGRAM",
                                  "STACK", "SECOND_WIND", "FEED", "ARMAMENTS", "ANGER", "HEADBUTT")},
    "SACRIFICE": _sacrifice, "SPUR": _spur, "EXPOSE": _expose, "UP_MY_SLEEVE": _up_my_sleeve, "SUMMON_FORTH": _summon_forth,
    "KNOCKOUT_BLOW": _knockout, "HANG": _hang, "RAMPAGE": lambda f, c, foe, x, d: setattr(c, "bonus", c.bonus + (10 if c.up else 5)),
    "ONE_TWO_PUNCH": lambda f, c, foe, x, d: setattr(f, "double_next", f.double_next + 1),
    "HOTFIX": lambda f, c, foe, x, d: f.temp.__setitem__("FOCUS", f.temp.get("FOCUS", 0) + (3 if c.up else 2)),
    "TERRAFORMING": lambda f, c, foe, x, d: f.pw.__setitem__("VIGOR", f.pw.get("VIGOR", 0) + (10 if c.up else 7)),
    "MONOLOGUE": lambda f, c, foe, x, d: f.pw.__setitem__("MONOLOGUE", 1),
    "FRANTIC_ESCAPE": lambda f, c, foe, x, d: ([v.powers.__setitem__("SANDPIT", v.powers["SANDPIT"] + 1) for v in f.alive() if v.powers.get("SANDPIT")],
                                               setattr(c, "cost_fight", c.cost() + 1)),
    "SHADOWMELD": lambda f, c, foe, x, d: f.pw.__setitem__("SHADOWMELD", 1),
    "CORROSIVE_WAVE": lambda f, c, foe, x, d: f.pw.__setitem__("CORROSIVE_WAVE", f.pw.get("CORROSIVE_WAVE", 0) + (3 if c.up else 2)),
    "OBLIVION": lambda f, c, foe, x, d: foe.mem.__setitem__("oblivion", foe.mem.get("oblivion", 0) + 3) if foe is not None else None,
    "SIGNAL_BOOST": lambda f, c, foe, x, d: f.pw.__setitem__("AMPLIFY", f.pw.get("AMPLIFY", 0) + 1),
    "NIGHTMARE": lambda f, c, foe, x, d: None,
}
PLAYABLE = {}
DRAWN = {}
RETAINED = {}
X_PLUS = set()


# ------------------------------------------------------------------ powers, read from their text

_TRIGGERS = (
    (re.compile(r"^at the start of your turn, (.+)$", re.I), "start"),
    (re.compile(r"^at the end of your turn, (.+)$", re.I), "end"),
    (re.compile(r"^whenever you play an? (attack|skill|power|card)(?: that costs [^,]*)?, (.+)$", re.I), "play"),
    (re.compile(r"^whenever a card is exhausted, (.+)$", re.I), "exhaust"),
    (re.compile(r"^whenever you gain block, (.+)$", re.I), "block"),
    (re.compile(r"^gain ((?:\[energy_icon\.png\])+) at the start of each turn\.?$", re.I), "energy"),
)
_DONE_BY_PARSE = re.compile(r"^(gain|lose) \d+ (strength|dexterity)\.?$", re.I)  # (what the planner's parser already reads in a Power)
_SENTENCES = re.compile(r"(?<=[.!?])\s+")


@lru_cache(maxsize=None)
def power_rules(text: str) -> tuple:
    """(trigger, what it does) for each sentence of a Power's text; trigger "now" for what happens as it is played."""
    out = []
    for s in _SENTENCES.split(text.strip()):
        s = s.strip()
        if not s or _DONE_BY_PARSE.match(s):
            continue
        for pat, when in _TRIGGERS:
            m = pat.match(s)
            if m:
                out.append((when, m.group(m.lastindex)))
                break
        else:
            out.append(("now", s))
    return tuple(out)


@lru_cache(maxsize=None)
def _effect_card(text: str) -> CardDef:
    return CardDef("_EFFECT", "Effect", "Skill", 0, "Token", False, text[0].upper() + text[1:] if text else text, text, 0, "COLORLESS")


def do_text(f, text: str, kind: str = "") -> None:
    """Carry out a sentence of a Power's (or a relic's) text as a card that says it would."""
    if kind and not text:
        return
    c = Card(_effect_card(text))
    alive = f.alive()
    foe = f.rng.choice(alive) if alive else None
    fx.resolve(f, c, foe, 0)


POWER: dict = {}


def _register(f, c) -> None:
    rules = power_rules(c.text)
    amount = int(m.group(1)) if (m := re.search(r"(\d+)", c.text)) else 1
    f.pw[c.key] = f.pw.get(c.key, 0) + amount
    own = OWN_POWER.get(c.key)
    if own is not None:
        own(f, c)
        return
    for when, what in rules:
        if when == "now":
            do_text(f, what)
        elif when == "energy":
            f.pw["TURN_ENERGY"] = f.pw.get("TURN_ENERGY", 0) + what.count("energy_icon")
        else:
            f.triggers.append((when, what, c.key))


def _void_form(f, c):
    f.pw["VOID_FORM"] = 2
    f.pw["TURN_OVER"] = 1


OWN_POWER = {
    "BARRICADE": lambda f, c: f.pw.__setitem__("BARRICADE", 1),
    "CORRUPTION": lambda f, c: f.pw.__setitem__("CORRUPTION", 1),
    "ECHO_FORM": lambda f, c: f.pw.__setitem__("ECHO_FORM", 1),
    "BUFFER": lambda f, c: None,
    "ACCURACY": lambda f, c: None,
    "ENVENOM": lambda f, c: f.pw.__setitem__("ENVENOM", 1),
    "FEEL_NO_PAIN": lambda f, c: None,
    "DARK_EMBRACE": lambda f, c: None,
    "JUGGERNAUT": lambda f, c: None,
    "AFTERIMAGE": lambda f, c: f.pw.__setitem__("AFTER_IMAGE", f.pw.get("AFTER_IMAGE", 0) + 1),
    "STORM": lambda f, c: None,
    "WELL_LAID_PLANS": lambda f, c: f.pw.__setitem__("KEEP_HAND_ALWAYS", 1),
    "VOID_FORM": _void_form,
    "CAPACITOR": lambda f, c: setattr(f, "orb_slots", f.orb_slots + (3 if c.up else 2)),
    "BULK_UP": lambda f, c: setattr(f, "orb_slots", max(0, f.orb_slots - 1)),
    "DEFRAGMENT": lambda f, c: f.pw.__setitem__("FOCUS", f.pw.get("FOCUS", 0) + (2 if c.up else 1)),
    "BIASED_COGNITION": lambda f, c: (f.pw.__setitem__("FOCUS", f.pw.get("FOCUS", 0) + (6 if c.up else 5)),
                                      f.pw.__setitem__("BIASED_COGNITION", 1)),
    "WRAITH_FORM": lambda f, c: (f.pw.__setitem__("INTANGIBLE", f.pw.get("INTANGIBLE", 0) + (3 if c.up else 2)),
                                 f.pw.__setitem__("WRAITH_FORM", 1)),
    "NECRO_MASTERY": lambda f, c: _summon(f, 8 if c.up else 5),
    "SEEKING_EDGE": lambda f, c: _forge(f, 11 if c.up else 7),
    "FAN_OF_KNIVES": lambda f, c: [f.add_card(make("SHIV")) for _ in range(4)],
    "ABRASIVE": lambda f, c: f.pw.__setitem__("THORNS", f.pw.get("THORNS", 0) + (6 if c.up else 4)),
    "STONE_ARMOR": lambda f, c: f.pw.__setitem__("PLATING", f.pw.get("PLATING", 0) + (6 if c.up else 4)),
    "NEUTRON_AEGIS": lambda f, c: f.pw.__setitem__("PLATING", f.pw.get("PLATING", 0) + (11 if c.up else 8)),
    "INFINITE_BLADES": lambda f, c: f.triggers.append(("start", "Add 1 Shiv into your Hand.", c.key)),
    "NEUROSURGE": lambda f, c: (setattr(f, "energy", f.energy + 3), f.draw_cards(2), f.pw.__setitem__("NEUROSURGE", 3)),
}
for _k, _d in CARDS.items():
    if _d.type == "Power":
        POWER[_k] = _register


def trigger(f, when: str, card=None) -> None:
    for w, what, key in list(f.triggers):
        if w != when:
            continue
        if when == "play" and card is not None:
            want = _PLAY_KIND.get(key)
            if want and want != card.type and want != "card":
                continue
        do_text(f, what)


_PLAY_KIND = {}
for _k, _d in CARDS.items():
    if _d.type == "Power":
        _m = re.search(r"whenever you play an? (attack|skill|power|card)", _d.text, re.I)
        if _m:
            _PLAY_KIND[_k] = _m.group(1).title() if _m.group(1).lower() != "card" else "card"


# ------------------------------------------------------------------ the engine's hooks


def START(f) -> None:
    run = f.run
    f.triggers = []
    if run.has("CRACKED_CORE"):
        f.channel("LIGHTNING")
    if run.has("DIVINE_RIGHT"):
        f.stars += 3
    if any(foe.powers.get("BACK_ATTACK_LEFT") or foe.powers.get("BACK_ATTACK_RIGHT") for foe in f.foes):
        f.pw["SURROUNDED"] = 1
    for i, foe in enumerate(f.foes):
        foe.mem["fight"] = f
        if foe.powers.get("ILLUSION"):
            foe.minion = True


def TURN_START(f) -> None:
    if f.run.has("BOUND_PHYLACTERY"):
        _summon(f, 1)
    f.energy += f.pw.get("TURN_ENERGY", 0)
    if f.pw.get("VOID_FORM"):
        f.pw["VOID_FREE"] = f.pw["VOID_FORM"]
    f.pw.pop("DRAWN_TURN", None)
    if f.pw.get("NEUROSURGE"):
        f.pw["DOOM"] = f.pw.get("DOOM", 0) + f.pw["NEUROSURGE"]
    trigger(f, "start")


def TURN_END(f) -> None:
    trigger(f, "end")
    if f.pw.get("DISINTEGRATION"):
        f.take(f.pw["DISINTEGRATION"], attack=False)
    if f.pw.get("DOOM") and f.pw["DOOM"] >= f.run.hp:
        f.lose_hp(f.run.hp)
    f.temp.pop("FOCUS", None)
    f.pw.pop("MONOLOGUE", None)
    f.pw.pop("SHADOWMELD", None)
    f.pw.pop("CORROSIVE_WAVE", None)
    for foe in f.foes:
        foe.mem.pop("oblivion", None)
    if f.pw.get("KEEP_HAND_ALWAYS"):
        f.pw["KEEP_HAND"] = 1


def TURN_CARDS(f) -> None:
    pass


def POTION_CARD(f, pot) -> None:
    kind = {"ATTACK_POTION": "Attack", "SKILL_POTION": "Skill", "POWER_POTION": "Power"}.get(pot.key)
    choices = [d for d in (pool(f.run.character) if kind else colorless()) if kind is None or d.type == kind]
    if choices:
        c = Card(f.rng.choice(choices))
        c.cost_turn = 0
        f.add_card(c)


def ROUND_END(f) -> None:
    sts2_foes.revive_round(f)
    for foe in f.alive():
        if foe.powers.get("SANDPIT"):
            foe.powers["SANDPIT"] -= 1
            if foe.powers["SANDPIT"] <= 0:
                f.lose_hp(f.run.hp)
        if foe.powers.get("TERRITORIAL"):
            foe.powers["STRENGTH"] = foe.powers.get("STRENGTH", 0) + foe.powers["TERRITORIAL"]
        if foe.powers.get("ESCAPE_ARTIST"):
            foe.powers["ESCAPE_ARTIST"] -= 1
            if foe.powers["ESCAPE_ARTIST"] <= 0:
                foe.gone = True
        if foe.powers.get("ASLEEP"):
            foe.powers["ASLEEP"] -= 1
        foe.mem.pop("spark", None)
    for k in ("RINGING", "SMOGGY", "TENDER"):
        if f.pw.get(k):
            f.pw[k] -= 1
    if not any(x.alive and not x.minion for x in f.foes):
        f.won = True


# ------------------------------------------------------------------ relics and potions

SHARED = sts1.COMMON + sts1.UNCOMMON + sts1.RARE
_RELIC = DATA["relics"]
COMMON = [r for r in SHARED if _RELIC.get(r, ["", ""])[1] == "Common"]
UNCOMMON = [r for r in SHARED if _RELIC.get(r, ["", ""])[1] == "Uncommon"]
RARE = [r for r in SHARED if _RELIC.get(r, ["", ""])[1] in ("Rare", "Shop")]
ANCIENT = ["SOZU", "VELVET_CHOKER", "PHILOSOPHER_S_STONE", "ECTOPLASM", "SNECKO_EYE", "RUNIC_PYRAMID", "BLACK_STAR", "ASTROLABE",
           "CALLING_BELL", "EMPTY_CAGE", "PANDORA_S_BOX", "ORRERY", "WHITE_BEAST_STATUE"]
STARTERS = {"BURNING_BLOOD", "RING_OF_THE_SNAKE", "CRACKED_CORE", "BOUND_PHYLACTERY", "DIVINE_RIGHT"}


def relic_text(rid: str) -> str:
    key = {"PHILOSOPHER_S_STONE": "PHILOSOPHERS_STONE", "PANDORA_S_BOX": "PANDORAS_BOX", "CAPTAIN_S_WHEEL": "CAPTAINS_WHEEL",
           "CHARON_S_ASHES": "CHARONS_ASHES"}.get(rid, rid)
    return (_RELIC.get(key) or ["", "", ""])[2]


def relic_pickup(run, rid: str) -> None:
    sts1.relic_pickup(run, rid)


def random_relic(run, chest: bool = False) -> str:
    rng = run.rng
    r = rng.random()
    tier = COMMON if r < 0.5 else UNCOMMON if r < 0.83 else RARE
    left = [x for x in tier if x not in run.relics] or [x for x in COMMON + UNCOMMON + RARE if x not in run.relics] or ["ANCHOR"]
    return rng.choice(left)


def random_curse(run) -> Card:
    return make(run.rng.choice(("REGRET", "DOUBT", "SHAME", "INJURY", "CLUMSY", "DECAY", "WRITHE")) if "REGRET" in CARDS else "WOUND")


_POTIONS = {k: v for k, v in DATA["potions"].items() if k not in ("ENTROPIC_BREW", "ASHWATER", "GAMBLERS_BREW", "DROPLET_OF_PRECOGNITION",
                                                                  "LIQUID_MEMORIES", "SNECKO_OIL", "TOUCH_OF_INSANITY", "STABLE_SERUM",
                                                                  "SOLDIERS_STEW", "BOTTLED_POTENTIAL", "COSMIC_CONCOCTION", "OROBIC_ACID",
                                                                  "GLOWWATER_POTION", "FOUL_POTION", "POTION_SHAPED_ROCK", "AMBERGRIS")}
_OWN_POTION = {"FOCUS_POTION": "DEFECT", "POTION_OF_CAPACITY": "DEFECT", "ESSENCE_OF_DARKNESS": "DEFECT", "CUNNING_POTION": "SILENT",
               "BONE_BREW": "NECROBINDER", "POT_OF_GHOULS": "NECROBINDER", "POTION_OF_DOOM": "NECROBINDER", "STAR_POTION": "REGENT",
               "KINGS_COURAGE": "REGENT"}


def random_potion(run) -> Potion:
    rng = run.rng
    r = rng.random()
    want = "Common" if r < 0.65 else "Uncommon" if r < 0.9 else "Rare"
    keys = [k for k, (_n, rar, _t) in _POTIONS.items() if rar == want and _OWN_POTION.get(k, run.character) == run.character]
    key = rng.choice(keys)
    name, _rar, text = _POTIONS[key]
    return Potion(key, name, text, want)


# ------------------------------------------------------------------ the run


def run_start(run) -> None:
    """Neow: heals, then offers three relics (one of each kind of blessing here), the bot's event handler choosing."""
    rng = run.rng
    first = rng.sample([r for r in COMMON + UNCOMMON if r not in run.relics], 2)
    options = [(rid.replace("_", " ").title(), f"Obtain {rid.replace('_', ' ').title()}. {relic_text(rid)}", (lambda r, a, rid=rid: r.gain_relic(rid)))
               for rid in first]
    options.append(("Remove", "Remove a card from your Deck.", lambda r, a: r.remove_one()))
    sts1._choose(run, options)


def act_start(run) -> None:
    run.fights_in_act = 0
    run.unknown_odds = dict(UNKNOWN_ODDS)
    if run.act == 1:
        run.act_id = run.rng.choice(("OVERGROWTH", "UNDERDOCKS"))
        return
    run.act_id = {2: "HIVE", 3: "GLORY"}[run.act]
    heal = (run.max_hp - run.hp) * (1.0 if run.asc < 2 else 0.8)
    run.hp = min(run.max_hp, run.hp + int(heal))
    offered = run.rng.sample([r for r in ANCIENT if r not in run.relics], 3)
    options = [(rid.replace("_", " ").title(), f"Obtain {rid.replace('_', ' ').title()}. {relic_text(rid)}", (lambda r, a, rid=rid: r.gain_relic(rid)))
               for rid in offered]
    sts1._choose(run, options)


def make_map(run) -> dict:
    act = next(a for a in DATA["acts"] if a["id"] == run.act_id)
    weights = dict(ROOM_WEIGHTS)
    if run.asc >= 1:
        weights["Elite"] *= 1.6
    rows = act["map_rooms"]
    return common.make_map(run.rng, weights, elite_from=6, treasure_row=rows - 6, rows=rows)


def encounter(run, kind: str) -> list[Foe]:
    act = next(a for a in DATA["acts"] if a["id"] == run.act_id)
    if kind == "monster":
        run.fights_in_act = getattr(run, "fights_in_act", 0) + 1
        pool_name = "weak" if run.fights_in_act <= act["weak_fights"] else "normal"
    else:
        pool_name = kind
    options = act[pool_name]
    if kind == "boss":
        bosses = run.__dict__.setdefault("bosses", {})
        if run.act not in bosses:
            bosses[run.act] = run.rng.sample(options, 2 if run.act == 3 and run.asc >= 10 else 1)
        picks = bosses[run.act]
        name = picks[run.__dict__.setdefault("boss_fights", 0) % len(picks)] if False else picks[0]
    else:
        fresh = [e for e in options if e not in run.recent[-2:]] or options
        name = run.rng.choice(fresh)
    run.recent.append(name)
    enc = DATA["encounters"][name]
    keys = list(enc["monsters"])
    if enc.get("randomized"):
        alts = enc.get("options") or keys
        keys = [run.rng.choice([a for a in alts if a.split("_")[-1] == k.split("_")[-1]] or [k]) for k in keys]
    foes, seen = [], {}
    for n, key in enumerate(keys):
        slot = seen.get(key, 0)
        seen[key] = slot + 1
        foes.append(_new(key, n, run, slot, len(keys) == 1))
    return foes


@lru_cache(maxsize=None)
def _spec(key: str, asc: int):
    return sts2_foes.spec_of(key, DATA["monsters"][key], asc)


def _new(key: str, n: int, run, slot: int = 0, alone: bool = False, hp: int | None = None) -> Foe:
    spec = _spec(key, run.asc)
    return sts2_foes.new_foe(spec, n, hp if hp is not None else run.rng.randint(*spec.hp), slot, alone)


def spawn(key: str, f, minion: bool = False, hp: int | None = None) -> Foe:
    foe = _new(key, len(f.foes), f.run, sum(1 for x in f.foes if x.key == key), False, hp)
    foe.minion = minion or foe.minion
    foe.mem["fight"] = f
    return foe


def fight_won(run, f) -> None:
    sts1.fight_won(run, f)


def boss_won(run) -> None:
    if run.act >= ACTS:
        return
    run.card_reward(card_choices(run, "boss"))


def fight_gold(run, kind: str) -> int:
    if run.has("ECTOPLASM"):
        return 0
    g = {"monster": run.rng.randint(10, 20), "elite": run.rng.randint(35, 45), "boss": 100}[kind]
    return int(g * (0.75 if run.asc >= 3 else 1.0))


def card_choices(run, kind: str) -> list[Card]:
    n = 3 + run.has("QUESTION_CARD") - 2 * run.has("BUSTED_CROWN")
    up = {1: 0.0, 2: 0.25, 3: 0.5}.get(run.act, 0.5) * (0.5 if run.asc >= 7 else 1.0)
    return common.offer(run, pool(run.character), max(1, n), CARD_ODDS, kind if kind in CARD_ODDS or kind == "boss" else "monster", up)


def rest_options(run) -> list[dict]:
    return sts1.rest_options(run)


def shop_stock(run) -> list[dict]:
    items = sts1.shop_stock(run)
    for it in items:
        if it["category"] == "card_removal" and run.asc >= 6:
            n = run.counters.get("removals", 0)
            it["price"] = it["cost"] = 100 + 50 * n
        if it["category"] == "relic":
            it["price"] = it["cost"] = int(it["price"] * 0.85)
    return items


def event(run) -> None:
    common.event(run)


def max_hp_penalty(asc: int, character: str) -> int:
    return 0


def setup_run(run) -> None:
    if run.asc >= 4:
        run.potions = run.potions[:2]
    if run.asc >= 5:
        run.deck.append(make("ASCENDERS_BANE"))


def ON(f, what: str, card=None) -> None:
    """The engine's events, for Powers read from their text: a card played, a card Exhausted, Block gained."""
    if f.triggers:
        trigger(f, what, card)
