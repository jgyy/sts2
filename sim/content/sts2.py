"""Slay the Spire 2 (v0.111.1), as the simulator plays it: cards, monsters, potions and relic names from sts2_data.json.

Powers are read from their own text: "At the start of your turn, X", "At the end of your turn, X", "Whenever you
play a card/an Attack/a Skill/a Power, X", "Whenever a card is Exhausted, X", "Whenever you gain Block, X", "Gain
[E] at the start of each turn", and what they do at once; X is done as a card with that text would do it.
"""

from __future__ import annotations

import json
import os
import re
from dataclasses import replace
from functools import lru_cache

from .. import effects as fx
from ..cards import Card, CardDef
from ..effects import add
from ..monsters import Foe
from ..run import Potion
from . import common, sts1, sts2_foes
from .sts1_cards import _pick

KEY = "sts2"
ACTS = 3
SHORT_REASON = True  # (the mod says why a card cannot be played: "EnergyCostTooHigh")
KEEP_POTIONS = {"FAIRY_IN_A_BOTTLE"}
UNKNOWN_ODDS, CARD_ODDS = sts1.UNKNOWN_ODDS, sts1.CARD_ODDS
ROOM_WEIGHTS = {"Monster": 0.42, "Unknown": 0.30, "RestSite": 0.12, "Shop": 0.05, "Elite": 0.07}
PLAYED = ("Main", "IRONCLAD", "SILENT", "DEFECT", "NECROBINDER", "REGENT")

with open(os.path.join(os.path.dirname(__file__), "sts2_data.json")) as _f:
    DATA = json.load(_f)


def _scarce(odds: list) -> list:
    """A7's card odds: half the rare chance, the other half to commons."""
    rare = dict(odds)["Rare"]
    return [(r, p / 2 if r == "Rare" else p + rare / 2 if r == "Common" else p) for r, p in odds]


SCARCE_ODDS = {k: _scarce(v) for k, v in CARD_ODDS.items()}


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
    add(f.pw, "FORGE", n)
    if _blade(f) is None:
        f.add_card(make("SOVEREIGN_BLADE"))


# What a card's text puts in our piles or on the enemy that the engine's generic part left out until 2026-11-26 (Fight Through's two
# Wounds, Boost Away's Dazed, Gunk Up's Slimed, Overclock's Burn, Turbo's Void; Fight Me's Strength for the enemy).
_STATUS_ADDED = re.compile(r"\b(?:add|shuffle)\s+(a|an|\d+)\s+(wound|dazed|slimed|burn|void|soot|toxic)s?\b[^.]*?\binto\s+your\s+(discard pile|draw pile|hand)", re.I)
_PILES = {"discard pile": "discard", "draw pile": "draw", "hand": "hand"}
_ENEMY_GAINS = re.compile(r"\bthe enemy gains (\d+) strength", re.I)
# ... and what it gives (Adaptive Strike's and Undeath's copies, Bundle of Joy's and Manifest Authority's Colorless cards, Quasar's choice,
# Stoke's new hand, Hyperbeam's lost Focus).
_SELF_COPY = re.compile(r"\badd a (0\[energy_icon\.png\] )?copy of this card into your discard pile", re.I)
_COLORLESS = re.compile(r"\badd (\d+) random colorless cards? into your hand", re.I)
_QUASAR = re.compile(r"\bchoose 1 of (\d+) random colorless cards to add into your hand", re.I)
_STOKE = re.compile(r"\bexhaust your hand\. add 1 random card into your hand for each card exhausted", re.I)
_FOCUS_TURN = re.compile(r"\blose (\d+) focus this turn", re.I)


def EXTRA(f, c, foe, i, xs) -> None:
    """Stars, Forge, Osty, Doom, Souls, Debris, statuses, copies and cards moved: what Slay the Spire 2's text says that the engine's generic part leaves out."""
    for m in _STATUS_ADDED.finditer(c.text):
        f.add_new(m.group(2).upper(), 1 if m.group(1).lower() in ("a", "an") else int(m.group(1)), _PILES[m.group(3).lower()])
    if (m := _ENEMY_GAINS.search(c.text)) and foe is not None and foe.alive:
        add(foe.powers, "STRENGTH", int(m.group(1)))
    if i.osty_dies and f.osty > 0 and c.key not in AFTER:  # Bone Shards: "Osty dies" (until 2026-11-26 he lived on; Sacrifice has its own rule)
        f.osty = 0
    if m := _SELF_COPY.search(c.text):
        twin = c.copy()
        twin.cost_fight = 0 if m.group(1) else twin.cost_fight
        f.add_card(twin, "discard")
    if m := _COLORLESS.search(c.text):
        for _ in range(int(m.group(1))):
            f.add_card(Card(f.rng.choice(colorless())))
    if m := _QUASAR.search(c.text):
        for picked in f.pile_pick("Choose a card to add into your Hand.", [Card(d) for d in f.rng.sample(colorless(), int(m.group(1)))])[:1]:
            f.add_card(picked)
    if _STOKE.search(c.text):
        burnt = [h for h in f.hand if h is not c]
        for h in burnt:
            f.hand.remove(h)
            f.exhaust(h)
        for _ in burnt:
            f.add_card(Card(f.rng.choice(pool(f.run.character))))
    if m := _FOCUS_TURN.search(c.text):
        add(f.temp, "FOCUS", -int(m.group(1)))
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
    f.add_new("SOUL", i.souls * xs, "draw")
    if i.debris:
        f.add_new("DEBRIS", i.debris if i.debris > 0 else max(0, 10 - len(f.hand)))
    for kind in i.channel * xs:
        add(f.pw, f"{kind}_CHANNELED", 1)


def _summon(f, n: int) -> None:
    if f.osty < 0:
        return
    f.osty += n
    f.pw["OSTY_MAX"] = max(f.pw.get("OSTY_MAX", 0), f.osty)


def _star_hooks(f, n: int) -> None:
    if f.pw.get("BLACK_HOLE"):
        f.hurt_all(f.pw["BLACK_HOLE"] * abs(n))


DAMAGE = {
    "BODY_SLAM": lambda f, c: f.block,
    "SOVEREIGN_BLADE": lambda f, c: 10 + f.pw.get("FORGE", 0),
    "UNLEASH": lambda f, c: (9 if c.up else 6) + max(0, f.osty),
    "PROTECTOR": lambda f, c: (15 if c.up else 10) + f.pw.get("OSTY_MAX", 0),
    "TIMES_UP": lambda f, c: 0,
    "DEATH_MARCH": lambda f, c: (10 if c.up else 8) + (5 if c.up else 4) * f.pw.get("DRAWN_TURN", 0),
    "RAMPAGE": lambda f, c: 10 + c.bonus,
}


PRE = {
    "TIMES_UP": lambda f, c, foe, i: i if foe is None else replace(i, damage=foe.powers.get("DOOM", 0), hits=1),
    **dict.fromkeys(("FLATTEN", "POKE", "RATTLE", "SNAP", "SIC_EM", "UNLEASH"), lambda f, c, foe, i: replace(i, damage=0) if f.osty <= 0 and i.osty_attack else i),
    "BONE_SHARDS": lambda f, c, foe, i: replace(i, damage=0, block=0) if f.osty <= 0 else i,  # "If Osty is alive, he deals ... and you gain ..."
    **{k: sts1.PRE[k] for k in ("FIEND_FIRE", "BURNING_PACT", "SECOND_WIND")},
}


def _thrash(f, c, *_):
    """Thrash: "Exhaust a random Attack in your Hand and add its damage to this card" (a hit's worth, for the rest of the fight)."""
    attacks = [h for h in f.hand if h.type == "Attack" and h is not c]
    if attacks:
        h = f.rng.choice(attacks)
        f.hand.remove(h)
        f.exhaust(h)
        c.bonus += max(0, (h.info.damage or 0) + h.bonus)


def _nightmare(f, c, *_):
    """Nightmare: "Choose a card. Next turn, add 3 copies of that card into your Hand" (turn_start brings them)."""
    for h in f.hand_pick("Choose a card to copy.", 1):
        f.__dict__.setdefault("nightmares", []).append(h.copy())


def _summon_forth(f, c, foe, x, dealt):
    blade = _blade(f)
    if blade is None:
        f.add_card(make("SOVEREIGN_BLADE"))
    elif blade not in f.hand:
        (f.draw if blade in f.draw else f.discard).remove(blade)
        f.add_card(blade)


AFTER = {
    **{k: sts1.AFTER[k] for k in ("HAVOC", "TRUE_GRIT", "BATTLE_TRANCE", "DUAL_WIELD", "INFERNAL_BLADE", "RAGE", "DOUBLE_TAP", "LIMIT_BREAK",
                                  "SURVIVOR", "ACROBATICS", "DAGGER_THROW", "PREPARED", "CALCULATED_GAMBLE", "BULLET_TIME", "BURST",
                                  "STORM_OF_STEEL", "UNLOAD", "CHAOS", "DOUBLE_ENERGY", "WHITE_NOISE", "REBOOT", "SEEK", "HOLOGRAM",
                                  "STACK", "SECOND_WIND", "FEED", "ARMAMENTS", "ANGER", "HEADBUTT")},
    # What these move between piles (until 2026-11-26 nothing: the first game's Hologram, Headbutt, All for One and Secret Technique rules).
    "GRAVEBLAST": sts1.AFTER["HOLOGRAM"], "COSMIC_INDIFFERENCE": sts1.AFTER["HEADBUTT"], "ALL_FOR_ONE": sts1.AFTER["ALL_FOR_ONE"],
    "SECRET_TECHNIQUE": sts1.AFTER["SECRET_TECHNIQUE"], "DREDGE": lambda f, *_: _pick(f, "Choose a card to return to your hand.", f.discard, n=3),
    "THRASH": _thrash, "NIGHTMARE": _nightmare, "FOREGONE_CONCLUSION": lambda f, c, *_: f.__dict__.__setitem__("foregone", f.__dict__.get("foregone", 0) + (3 if c.up else 2)),
    "SACRIFICE": lambda f, *_: (setattr(f, "osty", 0), f.gain_block(3 * f.pw.get("OSTY_MAX", 0))) if f.osty > 0 else None,
    "SPUR": lambda f, *_: setattr(f, "osty", f.osty + 5) if f.osty > 0 else None,
    "EXPOSE": lambda f, c, foe, *_: (setattr(foe, "block", 0), foe.powers.pop("ARTIFACT", None)) if foe is not None else None,
    "UP_MY_SLEEVE": lambda f, c, *_: setattr(c, "cost_fight", max(0, c.cost() - 1)), "SUMMON_FORTH": _summon_forth,
    "KNOCKOUT_BLOW": lambda f, c, foe, *_: setattr(f, "stars", f.stars + 5) if foe is not None and not foe.alive else None,
    "HANG": lambda f, c, foe, *_: foe.powers.__setitem__("HANG", max(2, foe.powers.get("HANG", 0) * 2)) if foe is not None else None,
    "RAMPAGE": lambda f, c, *_: setattr(c, "bonus", c.bonus + (10 if c.up else 5)), "ONE_TWO_PUNCH": lambda f, *_: setattr(f, "double_next", f.double_next + 1),
    "HOTFIX": lambda f, c, *_: add(f.temp, "FOCUS", 3 if c.up else 2), "TERRAFORMING": lambda f, c, *_: add(f.pw, "VIGOR", 10 if c.up else 7),
    "MONOLOGUE": lambda f, *_: f.pw.__setitem__("MONOLOGUE", 1), "SHADOWMELD": lambda f, *_: f.pw.__setitem__("SHADOWMELD", 1),
    "FRANTIC_ESCAPE": lambda f, c, *_: ([v.powers.__setitem__("SANDPIT", v.powers["SANDPIT"] + 1) for v in f.alive() if v.powers.get("SANDPIT")],
                                        setattr(c, "cost_fight", c.cost() + 1)),
    "CORROSIVE_WAVE": lambda f, c, *_: add(f.pw, "CORROSIVE_WAVE", 3 if c.up else 2),
    "OBLIVION": lambda f, c, foe, *_: add(foe.mem, "oblivion", 3) if foe is not None else None, "SIGNAL_BOOST": lambda f, *_: add(f.pw, "AMPLIFY", 1),
}
# "Can only be played if ...": worded as in the first game (until 2026-11-26 none held here, and Grand Finale was 60 damage to ALL
# enemies for nothing whenever drawn: +7.5 floors a pick at A8, and the card table had it at its +3 cap).
PLAYABLE = {k: sts1.PLAYABLE[k] for k in ("CLASH", "GRAND_FINALE")}
DRAWN, RETAINED, X_PLUS = {}, {}, set()


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


def do_text(f, text: str) -> None:
    """Carry out a sentence of a Power's (or a relic's) text as a card that says it would."""
    alive = f.alive()
    fx.resolve(f, Card(_effect_card(text)), f.rng.choice(alive) if alive else None, 0)


def _register(f, c) -> None:
    rules = power_rules(c.text)
    amount = int(m.group(1)) if (m := re.search(r"(\d+)", c.text)) else 1
    add(f.pw, c.key, amount)
    own = OWN_POWER.get(c.key)
    if own is not None:
        own(f, c)
        return
    for when, what in rules:
        if when == "now":
            do_text(f, what)
        elif when == "energy":
            add(f.pw, "TURN_ENERGY", what.count("energy_icon"))
        else:
            f.triggers.append((when, what, c.key))


OWN_POWER = {
    "BARRICADE": lambda f, c: f.pw.__setitem__("BARRICADE", 1),
    "CORRUPTION": lambda f, c: f.pw.__setitem__("CORRUPTION", 1),
    "ECHO_FORM": lambda f, c: f.pw.__setitem__("ECHO_FORM", 1),
    "BUFFER": lambda f, c: None,
    "ACCURACY": lambda f, c: None,
    "ENVENOM": lambda f, c: f.pw.__setitem__("ENVENOM", 1),
    "AFTERIMAGE": lambda f, c: add(f.pw, "AFTER_IMAGE", 1),
    "WELL_LAID_PLANS": lambda f, c: f.pw.__setitem__("KEEP_HAND_ALWAYS", 1),
    "VOID_FORM": lambda f, c: f.pw.update(VOID_FORM=2, TURN_OVER=1),
    "CAPACITOR": lambda f, c: setattr(f, "orb_slots", f.orb_slots + (3 if c.up else 2)),
    "BULK_UP": lambda f, c: setattr(f, "orb_slots", max(0, f.orb_slots - 1)),
    "DEFRAGMENT": lambda f, c: add(f.pw, "FOCUS", 2 if c.up else 1),
    "BIASED_COGNITION": lambda f, c: (add(f.pw, "FOCUS", 6 if c.up else 5), f.pw.__setitem__("BIASED_COGNITION", 1)),
    "WRAITH_FORM": lambda f, c: (add(f.pw, "INTANGIBLE", 3 if c.up else 2), f.pw.__setitem__("WRAITH_FORM", 1)),
    "NECRO_MASTERY": lambda f, c: _summon(f, 8 if c.up else 5),
    "SEEKING_EDGE": lambda f, c: _forge(f, 11 if c.up else 7),
    "FAN_OF_KNIVES": lambda f, c: f.add_new("SHIV", 4),
    "ABRASIVE": lambda f, c: add(f.pw, "THORNS", 6 if c.up else 4),
    "STONE_ARMOR": lambda f, c: add(f.pw, "PLATING", 6 if c.up else 4),
    "NEUTRON_AEGIS": lambda f, c: add(f.pw, "PLATING", 11 if c.up else 8),
    "NEUROSURGE": lambda f, c: (setattr(f, "energy", f.energy + 3), f.draw_cards(2), f.pw.__setitem__("NEUROSURGE", 3)),
}


def trigger(f, when: str, card=None) -> None:
    for w, what, key in list(f.triggers):
        want = _PLAY_KIND.get(key) if when == "play" and card is not None else None  # (a Power that names the type of card it waits for)
        if w == when and not (want and want not in (card.type, "card")):
            do_text(f, what)


POWER, _PLAY_KIND = {}, {}
for _k, _d in CARDS.items():
    if _d.type == "Power":
        POWER[_k] = _register
        if _m := re.search(r"whenever you play an? (attack|skill|power|card)", _d.text, re.I):
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
    for foe in f.foes:
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
    if n := f.__dict__.pop("foregone", 0):  # Foregone Conclusion: "Next turn, put 2 cards from your Draw Pile into your Hand"
        _pick(f, "Choose a card to put into your hand.", f.draw, n=n)
    for h in f.__dict__.pop("nightmares", []):
        for _ in range(3):
            f.add_card(h.copy())
    if f.pw.get("NEUROSURGE"):
        add(f.pw, "DOOM", f.pw["NEUROSURGE"])
    for foe in f.alive():  # Rampart: the Living Shield's Turret Operator gains 25 Block as our turn starts
        if foe.powers.get("RAMPART"):
            for x in f.alive():
                x.block += foe.powers["RAMPART"] if x.key == "TURRET_OPERATOR" else 0
    trigger(f, "start")


def TURN_END(f) -> None:
    trigger(f, "end")
    if f.pw.get("PLATING"):  # (its Block is in: Plating wears off by 1 a turn)
        f.pw["PLATING"] -= 1
    if f.pw.get("DISINTEGRATION"):
        f.take(f.pw["DISINTEGRATION"], attack=False)
    if f.pw.get("DOOM") and f.pw["DOOM"] >= f.run.hp:
        f.lose_hp(f.run.hp)
    f.temp.pop("FOCUS", None)
    for k in ("MONOLOGUE", "SHADOWMELD", "CORROSIVE_WAVE"):
        f.pw.pop(k, None)
    for foe in f.foes:
        foe.mem.pop("oblivion", None)
    if f.pw.get("KEEP_HAND_ALWAYS"):
        f.pw["KEEP_HAND"] = 1


TURN_CARDS = lambda f: None
POTION_CARD = common.potion_card


def ROUND_END(f) -> None:
    sts2_foes.revive_round(f)
    for foe in f.alive():
        if foe.powers.get("SANDPIT"):
            foe.powers["SANDPIT"] -= 1
            if foe.powers["SANDPIT"] <= 0:
                f.lose_hp(f.run.hp)
        for k in ("TERRITORIAL", "HIGH_VOLTAGE"):
            if foe.powers.get(k):
                add(foe.powers, "STRENGTH", foe.powers[k])
        if foe.powers.get("PLATING"):
            foe.powers["PLATING"] -= 1
        if foe.powers.get("NEMESIS"):  # Intangible every other turn
            foe.mem["nemesis"] = not foe.mem.get("nemesis")
            if foe.mem["nemesis"]:
                add(foe.powers, "INTANGIBLE", 1)
        if foe.powers.get("SLUMBER"):
            sts2_foes.slumber(foe, turn=True)
        if foe.powers.get("ESCAPE_ARTIST"):
            foe.powers["ESCAPE_ARTIST"] -= 1
            if foe.powers["ESCAPE_ARTIST"] <= 0:
                foe.gone = True
        if foe.powers.get("ASLEEP"):
            foe.powers["ASLEEP"] -= 1
        foe.mem.pop("spark", None)
    for k in ("RINGING", "SMOGGY", "TENDER", "TANGLED"):
        if f.pw.get(k) and k not in f.fresh:
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


def _data_key(rid: str) -> str:
    """The relic's key in the game's data (the model keeps the first game's ids, "PHILOSOPHER_S_STONE")."""
    return {"PHILOSOPHER_S_STONE": "PHILOSOPHERS_STONE", "PANDORA_S_BOX": "PANDORAS_BOX", "CAPTAIN_S_WHEEL": "CAPTAINS_WHEEL",
            "CHARON_S_ASHES": "CHARONS_ASHES"}.get(rid, rid)


def relic_text(rid: str) -> str:
    return (_RELIC.get(_data_key(rid)) or ["", "", ""])[2]


def relic_name(rid: str) -> str:
    """As the game prints it ("Philosopher's Stone"): what the bot's tables key an offered relic by (events._ANCIENT)."""
    return (_RELIC.get(_data_key(rid)) or [""])[0] or rid.replace("_", " ").title()


relic_pickup, fight_won, rest_options, event = sts1.relic_pickup, sts1.fight_won, sts1.rest_options, common.event
max_hp_penalty = lambda asc, character: 0
start_hp = lambda run: run.max_hp  # (no damaged start: StS2's A6 prices removal instead)
boss_heal = lambda run: 0.0  # (no heal after a boss: the next act's Ancient heals, act_start)


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


def _relic_options(rids: list) -> list:
    return [(relic_name(rid), f"Obtain {relic_name(rid)}. {relic_text(rid)}", (lambda r, a, rid=rid: r.gain_relic(rid))) for rid in rids]


def run_start(run) -> None:
    """Neow: two relics or a card removed, the bot's event handler choosing."""
    options = _relic_options(run.rng.sample([r for r in COMMON + UNCOMMON if r not in run.relics], 2))
    sts1._choose(run, options + [("Remove", "Remove a card from your Deck.", lambda r, a: r.remove_one())])


def act_start(run) -> None:
    run.fights_in_act = 0
    run.unknown_odds = dict(UNKNOWN_ODDS)
    if run.act == 1:
        run.act_id = run.rng.choice(("OVERGROWTH", "UNDERDOCKS"))
        return
    run.act_id = {2: "HIVE", 3: "GLORY"}[run.act]
    heal = (run.max_hp - run.hp) * (1.0 if run.asc < 2 else 0.8)
    run.hp = min(run.max_hp, run.hp + int(heal))
    sts1._choose(run, _relic_options(run.rng.sample([r for r in ANCIENT if r not in run.relics], 3)))


def _act(run) -> dict:
    return next(a for a in DATA["acts"] if a["id"] == run.act_id)


def make_map(run) -> dict:
    act = _act(run)
    weights = dict(ROOM_WEIGHTS)
    if run.asc >= 1:
        weights["Elite"] *= 1.6
    rows = act["map_rooms"]
    return common.make_map(run.rng, weights, elite_from=6, treasure_row=rows - 6, rows=rows)


def encounter(run, kind: str) -> list[Foe]:
    act = _act(run)
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
        name = bosses[run.act][0]
    else:
        fresh = [e for e in options if e not in run.recent[-2:]] or options
        name = run.rng.choice(fresh)
    run.recent.append(name)
    enc = DATA["encounters"][name]
    keys = list(enc["monsters"])
    if enc.get("randomized"):
        alts = enc.get("options") or keys
        keys = [run.rng.choice([a for a in alts if a.split("_")[-1] == k.split("_")[-1]] or [k]) for k in keys]
    return [_new(key, n, run, keys[:n].count(key), len(keys) == 1) for n, key in enumerate(keys)]  # (slot: how many of its kind came before)


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
    odds = SCARCE_ODDS if run.asc >= 7 else CARD_ODDS  # (A7: rares half as common)
    return common.offer(run, pool(run.character), max(1, n), odds, kind if kind in CARD_ODDS or kind == "boss" else "monster", up)


def shop_stock(run) -> list[dict]:
    items = sts1.shop_stock(run)
    for it in items:
        if it["category"] == "card_removal" and run.asc >= 6:
            n = run.counters.get("removals", 0)
            it["price"] = it["cost"] = 100 + 50 * n
        if it["category"] == "relic":
            it["price"] = it["cost"] = int(it["price"] * 0.85)
    return items


def setup_run(run) -> None:
    if run.asc >= 4:
        run.potions = run.potions[:2]
    if run.asc >= 5:
        run.deck.append(make("ASCENDERS_BANE"))


def ON(f, what: str, card=None) -> None:
    """The engine's events, for Powers read from their text: a card played, a card Exhausted, Block gained."""
    if f.triggers:
        trigger(f, what, card)
    if what == "play":
        for foe in f.alive():  # the Aeonglass's Withering Presence: a Wither into our hand every 6 cards played
            if foe.powers.get("WITHERING_PRESENCE"):
                foe.powers["WITHERING_PRESENCE"] -= 1
                if foe.powers["WITHERING_PRESENCE"] <= 0:
                    foe.powers["WITHERING_PRESENCE"] = 6
                    f.add_card(Card(wither(foe.mem.get("wither_level", 0))))


@lru_cache(maxsize=None)
def wither(level: int) -> CardDef:
    """A Wither of the Aeonglass's level: each time it buffs, every Wither does 3 more."""
    d = CARDS["WITHER"]
    return replace(d, name=f"Wither+{level}", text=f"Unplayable. At the end of your turn, if this is in your Hand, take {3 * level + 3} damage.") if level else d
