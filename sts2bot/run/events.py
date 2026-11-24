"""Events: what each option on offer is worth, and which one to take."""

from __future__ import annotations

import logging
import re

from . import relics
from ..cards import deck_value, is_basic, is_known as _known_card
from .potions import boss_worth
from .relicworth import energy_lean, id_of as relic_id, worth as relic_worth

Action = tuple[str, dict]

log = logging.getLogger("sts2bot.strategy")

_RELIC_WORD = r"\brelics?\b"
_GOOD = [(_RELIC_WORD, 6.0), (r"\bremove\b", 4.0), (r"\bupgrade\b", 4.0), (r"\btransform\b", 2.0), (r"\bpotions?\b", 1.5), (r"\bcards?\b", 1.0), (r"\benchant\b", 3.0),
         (r"draw \d+ additional cards?", 5.0), (r"energy_icon", 5.0), (r"\bgreater rewards\b", 6.0)]  # (more draw: every turn of the run; Punch Off: relic, card, potion)
# (The Lantern Key's fight costs about 27 HP and leaves an unplayable card; Foul Potions hit us as hard as the enemy.)
_BAD = [(r"\bfoul potions?\b", 1.0), (r"\bfight to obtain the key\b", 6.0), (r"\bcurse\b", 6.0), (r"\bfight\b", 2.0), (r"draw \d+ fewer cards?", 4.0), (r"\bdowngrade\b", 2.0), (r"\bsacrifice\b", 5.0)]
EVENT_FIGHT_HP = [(r"\bfight them\b", 32.0)]  # HP an option that starts a fight costs, priced as "Lose N HP" is (Punch Off's constructs)
_DUPLICATE_DECK = re.compile(r"duplicate (?:your|the) (?:whole )?deck")  # (doubles every draw pile and dilutes the good cards)
# What something bad received costs, where a relic is worth 6: little for a card drawn once a fight, much for one that hurts
# each time or can never be removed. Royal Poison (4 HP a fight) and the Sword of Stone (five elites) belong here too.
_CURSES = {
    "clumsy": 1.5, "guilty": 1.5, "poor sleep": 2.0, "folly": 2.5, "spore mind": 2.5, "injury": 3.0, "debt": 4.0,
    "decay": 4.5, "doubt": 4.5, "shame": 4.5, "greed": 5.0, "curse of the bell": 5.0, "regret": 5.0, "writhe": 5.0,
    "normality": 8.0, "enthralled": 8.0, "bad luck": 10.0, "royal poison": 9.0, "sword of stone": -0.5,
}
_CURSE_GAIN = re.compile(  # (the first game writes "Become Cursed - Regret.")
    r"\b(?:receive|obtain|add|gain|become cursed)\b[^.]*?\b(" + "|".join(_CURSES) + r"|wound|burn|dazed|hex|parasite|"
    r"pain|spite|void|mind rot|sloth|waste away|disintegration|curse)\b"
)
_RUINOUS = re.compile(r"\blose (all|everything)\b|\bset your max hp\b|\bmax hp (to|becomes) \d\b", re.I)  # the run ended in all but name
_JUNK_NAMES = {"CLUMSY", "GUILTY", "NORMALITY", "BADLUCK", "REGRET", "POORSLEEP", "INJURY", "WOUND", "DAZED", "SOOT", "SLIMED", "INFECTION", "ENTHRALLED", "DOUBT", "SHAME", "DECAY",
               "PARASITE", "PAIN", "WRITHE"}
# What the Ancients' relics (the event that opens each act) are worth to this bot, on the scale of the scores below (read word
# by word, energy with a curse scored below "upgrade a card"). relics.py moves each by what the runs that held it did.
_ANCIENT = {
    # more energy or cards every turn
    "BLOOD_SOAKED_ROSE": 6.5, "SPIKED_GAUNTLETS": 8.5, "PHILOSOPHERS_STONE": 10.0, "SOZU": 8.5,
    "BLESSED_ANTLER": 8.0, "PAELS_FLESH": 12.5, "PAELS_BLOOD": 13.5, "FIDDLE": 11.5, "WHISPERING_EARRING": 12.0,
    "PAELS_TEARS": 9.0, "VERY_HOT_COCOA": 11.5, "PUMPKIN_CANDLE": 8.5, "SEAL_OF_GOLD": 10.0, "BOOMING_CONCH": 5.5,
    "SNECKO_EYE": 13.0, "VELVET_CHOKER": 11.5, "PRISMATIC_GEM": 9.0, "ECTOPLASM": 7.0, "RUNIC_PYRAMID": 9.0,
    "IRON_CLUB": 9.5,
    # something every turn or every fight
    "SAI": 13.0, "CROSSBOW": 12.0, "TOASTY_MITTENS": 11.0, "MUSIC_BOX": 10.0, "DELICATE_FROND": 12.0,
    "CHOICES_PARADOX": 9.0, "DIAMOND_DIADEM": 9.0, "PAELS_LEGION": 8.0, "RADIANT_PEARL": 9.5, "JEWELED_MASK": 7.0,
    "BRILLIANT_SCARF": 6.5, "FUR_COAT": 10.0, "BLACK_STAR": 8.5, "ALCHEMICAL_COFFER": 9.5,
    # health, gold, relics
    "LOOMING_FRUIT": 8.5, "NUTRITIOUS_OYSTER": 8.5, "STONE_HUMIDIFIER": 9.5, "LORDS_PARASOL": 12.0,
    "SIGNET_RING": 9.5, "LARGE_CAPSULE": 8.5, "SMALL_CAPSULE": 8.0, "TOY_BOX": 9.0, "LAVA_ROCK": 8.0,
    "NEOWS_BONES": 7.0, "GOLDEN_PEARL": 6.5, "CURSED_PEARL": 6.0,
    # the deck
    "YUMMY_COOKIE": 7.5, "PAELS_TOOTH": 10.0, "JEWELRY_BOX": 10.0, "WAR_HAMMER": 10.5, "FISHING_ROD": 8.5,
    "PANDORAS_BOX": 10.5, "SAND_CASTLE": 8.5, "ELECTRIC_SHRYMP": 8.0, "GLASS_EYE": 6.0, "CALLING_BELL": 6.5,
    "BIIIG_HUG": 9.0, "ARCHAIC_TOOTH": 7.0, "NUTRITIOUS_SOUP": 8.5, "PAELS_CLAW": 7.0, "BEAUTIFUL_BRACELET": 8.0,
    "TRI_BOOMERANG": 7.5, "PAELS_GROWTH": 7.5, "PHIAL_HOLSTER": 7.5, "PRECISE_SCISSORS": 6.5, "PRESERVED_FOG": 7.0,
    "PRECARIOUS_SHEARS": 5.0, "LOST_COFFER": 6.5, "ARCANE_SCROLL": 6.5, "CLAWS": 7.0, "SILVER_CRUCIBLE": 7.0,
    "GLITTER": 7.0, "POMANDER": 6.5, "KALEIDOSCOPE": 6.5, "SCROLL_BOXES": 7.5, "DEMON_GLASS": 6.5,
    "LEAD_PAPERWEIGHT": 8.5, "DUSTY_TOME": 6.5, "STORYBOOK": 6.5, "NEOWS_TALISMAN": 6.0, "NEW_LEAF": 5.5,
    "DOWSING_ROD": 5.5, "TANXS_WHISTLE": 5.5, "NEOWS_TORMENT": 5.5, "PAELS_HORN": 5.5, "GOLDEN_COMPASS": 5.0,
    "ASTROLABE": 9.0, "EMPTY_CAGE": 8.5, "THROWING_AXE": 7.5, "DRIFTWOOD": 6.5,
    # things the bot cannot make use of
    "WINGED_BOOTS": 4.0, "PAELS_WING": 4.0, "MEAT_CLEAVER": 3.5, "PAELS_EYE": 3.0, "DISTINGUISHED_CAPE": 3.0,
}
RELIC = 6.0  # what a relic counts for in an event (_GOOD)
ACT3_HP = 1.6  # how much more a point of HP given up in an event costs in act 3 (the last boss is won from full HP)
FIGHT_DEATH_LOW, FIGHT_DEATH_HIGH = 0.22, 0.005  # chance an ordinary fight ends the run, entered under half HP / from half up


def _lives_through(fights: int, hp: int, max_hp: int) -> float:
    """Chance of getting through `fights` more ordinary fights from `hp`: a reward that comes after them needs that."""
    death = FIGHT_DEATH_LOW if hp < 0.5 * max_hp else FIGHT_DEATH_HIGH
    return (1.0 - death) ** fights


def _event_option_value(opt: dict, hp: int, max_hp: int, bled: int = 0, act: int | None = None, potions: list | None = None,
                        held: list | None = None, deck: list | None = None, energy: int | None = None) -> float:
    text = f"{opt.get('title') or ''}. {opt.get('description') or ''}".lower()
    key = re.sub(r"[^A-Z0-9]+", "_", re.sub(r"['\u2019]", "", str(opt.get("title") or "")).upper()).strip("_")
    known = _ANCIENT.get(key)
    if known is not None:
        # (moved by how the runs that held it went, and for energy or a card every turn by the deck's energy)
        return known + relics.shift(key) + energy_lean(key, energy, held, deck)
    score = 0.5
    if opt.get("relic_name"):
        score += 6.0
    # Relics that come "after N combats" are had only if the run lives through the fights.
    lives = 1.0
    if m := re.search(r"\bobtain \d+ random relics after (\d+) combats\b", text):
        lives = _lives_through(int(m.group(1)), hp, max_hp)
    for pat, w in _GOOD:
        if re.search(pat, text) and not re.search(r"(?:lose|pay|give)[^.]*" + pat, text):
            score += w * (lives if pat == _RELIC_WORD else 1.0)
    # Gold is worth what it buys, a heal what is missing, Max HP its amount.
    if m := re.search(r"\bgain (\d+) max hp", text):
        score += min(6.0, 0.35 * int(m.group(1)))
    elif re.search(r"\bmax hp\b", text) and not re.search(r"(?:lose|pay|give)[^.]*\bmax hp\b", text):
        score += 3.0
    if m := re.search(r"\bgain (\d+) gold", text):
        score += min(6.0, int(m.group(1)) / 50.0)
    elif re.search(r"\bgold\b", text) and not re.search(r"(?:lose|pay|give)[^.]*\bgold\b", text):
        score += 2.0
    if m := re.search(r"\bheal(?: (\d+)(%?))?", text):
        amount = max_hp if not m.group(1) else int(m.group(1)) * max_hp // 100 if m.group(2) else int(m.group(1))
        score += 12.0 * min(amount, max(0, max_hp - hp)) / max(1, max_hp)
    cursed = {m.group(1) for m in _CURSE_GAIN.finditer(text)}
    score -= sum(_CURSES.get(name, 6.0) for name in cursed)
    # "Obtain the Sword of Stone", "Receive the Spoils Map": a relic by name (each judged on its own: "Receive Bad Luck" is not one).
    if not opt.get("relic_name") and "relic" not in text:
        for gain in re.finditer(r"\b(?:obtain|receive) (?:the )?(?!a\b|an\b|\d)[a-z][a-z' ]*", text):
            if not _CURSE_GAIN.search(gain.group(0)):
                score += 5.0
                break
    if _DUPLICATE_DECK.search(text):
        score -= 12.0
    if m := re.search(r"\b(?:pay|give|lose) (\d+) gold", text):
        score -= int(m.group(1)) / (300.0 if act is not None and act >= 3 else 150.0)  # (gold has little left to buy in act 3)
    # A potion handed over is one fewer at the boss, where the second one on the belt goes with twice the wins.
    for p in potions or []:
        name = str(p.get("name") or "").strip().lower()
        if name and re.search(r"\b(?:lose|give|insert)\b[^.]*\b" + re.escape(name), text):
            score -= (1.0 + boss_worth(p) / 12.0) * (1.5 if act is not None and act >= 3 else 1.0)
            break
    # A relic of ours handed over costs a relic and what it does (relicworth.py); one traded in brings what it does.
    ours = {str(r.get("id")) for r in held or []}
    for gone in re.finditer(r"\b(?:give|trade|lose) ([a-z][a-z' -]*?)(?= for |[.:]|$)", text):
        if relic_id(gone.group(1)) in ours:
            score -= RELIC + relic_worth(relic_id(gone.group(1)), deck, act, hp, max_hp)
            if m := re.search(r"\btrade [a-z' -]+? for ([a-z][a-z' -]*?)(?=[.:]|$)", text):
                score += RELIC + relic_worth(relic_id(m.group(1)), deck, act, hp, max_hp)
            break
    if m := re.search(r"\bobtain (\d+) random relics\b", text):
        score += RELIC * (int(m.group(1)) - 1) * lives  # (the word "relics" above counts for one)
    if potions is not None and len(potions) < 2 and re.search(r"\bprocure\b[^.]*\bpotions?\b", text) and "foul" not in text:
        score += 1.0  # (a belt short of two: the potion is worth more than the flat 1.5 above)
    for pat, w in _BAD:
        if re.search(pat, text):
            score -= w
    if _RUINOUS.search(text):
        score -= 1000.0
    if m := re.search(r"\.\s*(.+?) is removed from your deck", text):
        # A named card goes: good riddance to a Strike, Defend or curse, a loss otherwise.
        gone = {"name": m.group(1).strip().title()}
        junk = is_basic(gone) or re.sub(r"\W", "", gone["name"]).upper() in _JUNK_NAMES
        score += 2.0 if junk else -0.8 * max(0.0, deck_value(gone) - 2.0)
    if m := re.search(r"\badd (?:an? |1 )?([a-z' !-]+?) to your deck", text):
        gained = {"name": m.group(1).strip().title()}
        if re.sub(r"\W", "", gained["name"]).upper() in _JUNK_NAMES:
            score -= 0.0 if cursed else 4.0  # (a curse has been charged for by name)
        else:
            score += 0.5 * (deck_value(gained) - 3.0) if _known_card(gained) else 0.0
    if m := re.search(r"lose\s+(\d+)(%?)\s+max\s+hp", text):
        # Max HP never comes back: a small bite is a fair price for an upgrade or a relic, a big one is not.
        share = int(m.group(1)) / 100.0 if m.group(2) else int(m.group(1)) / max(1, max_hp)
        score -= 1.5 + 60.0 * share + (1000.0 if share >= 0.25 else 0.0)
    if m := re.search(r"(?:lose|take)\s+(\d+)\s+(?:hp|damage)", text):
        loss = int(m.group(1))
        score -= 1000.0 if loss >= hp else 12.0 * loss / max(1, hp) * (ACT3_HP if act is not None and act >= 3 else 1.0)
        if bled >= 5:
            score -= 2.0 + 0.5 * bled  # each further bite of one event costs more, so a chain of small losses stops
    for pat, cost in EVENT_FIGHT_HP:
        if re.search(pat, text):
            score -= 1000.0 if cost >= hp else 12.0 * cost / max(1, hp) * (ACT3_HP if act is not None and act >= 3 else 1.0)
            break
    return score


def event(state: dict, mem) -> list[Action]:
    ev = state.get("event") or {}
    if ev.get("in_dialogue"):
        return [("advance_dialogue", {})]
    opts = [o for o in ev.get("options") or [] if not o.get("is_locked")]
    if not opts:
        return [("advance_dialogue", {}), ("proceed", {})]
    player = state.get("player") or {}
    hp, max_hp = int(player.get("hp") or 1), int(player.get("max_hp") or 1)
    # One event is one floor: the same event on a later floor starts again from its own HP.
    eid = f"{ev.get('event_id') or ''}@{(state.get('run') or {}).get('floor')}"
    if eid != mem.event_id:
        mem.event_id, mem.event_hp0 = eid, hp
    bled = max(0, mem.event_hp0 - hp)
    act = (state.get("run") or {}).get("act")
    worth = lambda o: _event_option_value(o, hp, max_hp, bled, act if isinstance(act, int) else None, player.get("potions") or [],
                                          player.get("relics") or [], getattr(mem, "deck", None), getattr(mem, "max_energy", None))
    rest = sorted((o for o in opts if not o.get("is_proceed")), key=worth, reverse=True)
    if log.isEnabledFor(logging.DEBUG) and rest:
        log.debug("event %s: %s", ev.get("event_id"), " | ".join(f"[{worth(o):.1f}] {o.get('title')}: {o.get('description')}" for o in rest))
    ordered = [o for o in opts if o.get("is_proceed")] + rest
    return [("choose_event_option", {"index": int(o.get("index", 0))}) for o in ordered]
