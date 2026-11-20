"""Events: what each option on offer is worth, and which one to take."""

from __future__ import annotations

import logging
import re

from .cards import deck_value, is_basic, is_known as _known_card

Action = tuple[str, dict]

log = logging.getLogger("sts2bot.strategy")

_GOOD = [
    (r"\brelics?\b", 6.0), (r"\bremove\b", 4.0), (r"\bupgrade\b", 4.0), (r"\bmax hp\b", 3.0), (r"\bheal\b", 2.5), (r"\bgold\b", 2.0),
    (r"\btransform\b", 2.0), (r"\bpotions?\b", 1.5), (r"\bcards?\b", 1.0), (r"\benchant\b", 3.0),
    (r"draw \d+ additional cards?", 5.0),  # every turn, for the rest of the run
    (r"energy_icon", 5.0),
]
# The Lantern Key's fight (a Mysterious Knight) cost 27 HP on average over 17
# tries in the middle of act 2 and leaves an unplayable card in the deck; what
# it unlocks is an act 3 event that 4 of those runs lived to see. About one in
# five of them got past the act 2 boss, against 44% of the other runs.
# (Foul Potions hit us as hard as the enemy, so the bot hardly ever throws one:
# three of them fill the belt for nothing. The Potion Courier's other offer is
# one Uncommon potion.)
_BAD = [(r"\bfoul potions?\b", 1.0), (r"\bfight to obtain the key\b", 6.0), (r"\bcurse\b", 6.0), (r"\bfight\b", 2.0), (r"draw \d+ fewer cards?", 4.0), (r"\bdowngrade\b", 2.0), (r"\bsacrifice\b", 5.0)]
# Options that end the run in all but name. One event offers ever larger
# rewards for Max HP until the last step, "Lose Everything", leaves 1 Max HP;
# the bot walked into it nine times.
# Duplicating the whole deck doubles every draw pile and dilutes the good cards.
_DUPLICATE_DECK = re.compile(r"duplicate (?:your|the) (?:whole )?deck")
# Something bad we would receive or be given: it is not a relic, whatever the
# "Receive ..." wording suggests.
_CURSE_GAIN = re.compile(
    r"\b(?:receive|obtain|add|gain)\b[^.]*\b(?:bad luck|regret|doubt|shame|decay|injury|wound|burn|dazed|hex|parasite|"
    r"writhe|normality|pain|debt|greed|spite|void|mind rot|sloth|waste away|disintegration|clumsy|curse)\b"
)
_RUINOUS = re.compile(r"\blose (all|everything)\b|\bset your max hp\b|\bmax hp (to|becomes) \d\b", re.I)


_JUNK_NAMES = {
    "CLUMSY", "GUILTY", "NORMALITY", "BADLUCK", "REGRET", "POORSLEEP", "INJURY", "WOUND", "DAZED", "SOOT",
    "SLIMED", "INFECTION", "ENTHRALLED", "DOUBT", "SHAME", "DECAY", "PARASITE", "PAIN", "WRITHE",
}


# What the relics offered by the Ancients (the event that opens each act) are
# worth to this bot, on the scale of the scores below. Reading their text word
# by word got these badly wrong: energy every turn with a curse attached
# scored below "upgrade a card", and +31 Max HP below a random relic.
# (Neow's offers re-rated on 2026-11-20 from 282 runs, floors beyond what the same character reaches on the same
# ascension: Booming Conch -4.4 over 34 picks (the bot meets half an elite an act; 12 of the 34 died in act 1),
# Stone Humidifier +9.6 over 9 with 4 wins (it rests at seven rest sites in ten), Lead Paperweight +7.1 over 12,
# Scroll Boxes +4.4 over 10, Large Capsule -1.3 over 24, Fishing Rod +0.2 over 42, Arcane Scroll -2.8 over 19.
# Act 2 picks, among runs that got there: Very Hot Cocoa +3.7 over 14 with 5 wins, Archaic Tooth -2.5 over 18
# with none, Pael's Claw -7.3 over 5.)
_ANCIENT = {
    # more energy or cards every turn
    "BLOOD_SOAKED_ROSE": 6.5, "SPIKED_GAUNTLETS": 13.5, "PHILOSOPHERS_STONE": 13.5, "SOZU": 12.5,
    "BLESSED_ANTLER": 12.5, "PAELS_FLESH": 12.5, "PAELS_BLOOD": 13.0, "FIDDLE": 11.5, "WHISPERING_EARRING": 11.0,
    "PAELS_TEARS": 9.0, "VERY_HOT_COCOA": 11.5, "PUMPKIN_CANDLE": 8.5, "SEAL_OF_GOLD": 8.0, "BOOMING_CONCH": 5.5,
    # something every turn or every fight
    "SAI": 13.0, "CROSSBOW": 12.0, "TOASTY_MITTENS": 11.0, "MUSIC_BOX": 10.0, "DELICATE_FROND": 10.0,
    "CHOICES_PARADOX": 9.0, "DIAMOND_DIADEM": 9.0, "PAELS_LEGION": 8.0, "RADIANT_PEARL": 7.5, "JEWELED_MASK": 7.0,
    "BRILLIANT_SCARF": 6.5, "FUR_COAT": 12.0,
    # health, gold, relics
    "LOOMING_FRUIT": 13.0, "NUTRITIOUS_OYSTER": 8.5, "STONE_HUMIDIFIER": 9.5, "LORDS_PARASOL": 12.0,
    "SIGNET_RING": 9.5, "LARGE_CAPSULE": 8.5, "SMALL_CAPSULE": 8.0, "TOY_BOX": 9.0, "LAVA_ROCK": 8.0,
    "NEOWS_BONES": 7.0, "GOLDEN_PEARL": 6.5, "CURSED_PEARL": 6.0,
    # the deck
    "YUMMY_COOKIE": 10.5, "PAELS_TOOTH": 10.0, "JEWELRY_BOX": 10.0, "WAR_HAMMER": 9.0, "FISHING_ROD": 8.5,
    "BIIIG_HUG": 9.0, "ARCHAIC_TOOTH": 7.0, "NUTRITIOUS_SOUP": 8.5, "PAELS_CLAW": 7.0, "BEAUTIFUL_BRACELET": 8.0,
    "TRI_BOOMERANG": 7.5, "PAELS_GROWTH": 7.5, "PHIAL_HOLSTER": 7.5, "PRECISE_SCISSORS": 6.5, "PRESERVED_FOG": 7.0,
    "PRECARIOUS_SHEARS": 5.0, "LOST_COFFER": 6.5, "ARCANE_SCROLL": 6.5, "CLAWS": 7.0, "SILVER_CRUCIBLE": 7.0,
    "GLITTER": 7.0, "POMANDER": 6.5, "KALEIDOSCOPE": 6.5, "SCROLL_BOXES": 7.5, "DEMON_GLASS": 6.5,
    "LEAD_PAPERWEIGHT": 8.5, "DUSTY_TOME": 6.5, "STORYBOOK": 6.5, "NEOWS_TALISMAN": 6.0, "NEW_LEAF": 5.5,
    "DOWSING_ROD": 5.5, "TANXS_WHISTLE": 5.5, "NEOWS_TORMENT": 5.5, "PAELS_HORN": 5.5, "GOLDEN_COMPASS": 5.0,
    # things the bot cannot make use of
    "WINGED_BOOTS": 4.0, "PAELS_WING": 4.0, "MEAT_CLEAVER": 3.5, "PAELS_EYE": 3.0, "DISTINGUISHED_CAPE": 3.0,
}


def _event_option_value(opt: dict, hp: int, max_hp: int, bled: int = 0) -> float:
    text = f"{opt.get('title') or ''}. {opt.get('description') or ''}".lower()
    known = _ANCIENT.get(re.sub(r"[^A-Z0-9]+", "_", re.sub(r"['\u2019]", "", str(opt.get("title") or "")).upper()).strip("_"))
    if known is not None:
        return known
    score = 0.5
    if opt.get("relic_name"):
        score += 6.0
    for pat, w in _GOOD:
        if re.search(pat, text) and not re.search(r"(?:lose|pay|give)[^.]*" + pat, text):
            score += w
    if _CURSE_GAIN.search(text):
        score -= 6.0
    # "Obtain the Sword of Stone", "Receive the Spoils Map": a relic by name.
    # Each "obtain/receive" is judged on its own, so "Receive Bad Luck" is not one.
    if not opt.get("relic_name") and "relic" not in text:
        for gain in re.finditer(r"\b(?:obtain|receive) (?:the )?(?!a\b|an\b|\d)[a-z][a-z' ]*", text):
            if not _CURSE_GAIN.search(gain.group(0)):
                score += 5.0
                break
    if _DUPLICATE_DECK.search(text):
        score -= 12.0
    m = re.search(r"\bpay (\d+) gold", text)
    if m:
        score -= int(m.group(1)) / 150.0
    for pat, w in _BAD:
        if re.search(pat, text):
            score -= w
    if _RUINOUS.search(text):
        score -= 1000.0
    m = re.search(r"\.\s*(.+?) is removed from your deck", text)
    if m:
        # A named card goes: good riddance to a Strike, Defend or curse, a loss otherwise.
        gone = {"name": m.group(1).strip().title()}
        junk = is_basic(gone) or re.sub(r"\W", "", gone["name"]).upper() in _JUNK_NAMES
        score += 2.0 if junk else -0.8 * max(0.0, deck_value(gone) - 2.0)
    m = re.search(r"\badd (?:an? |1 )?([a-z' !-]+?) to your deck", text)
    if m:
        gained = {"name": m.group(1).strip().title()}
        if re.sub(r"\W", "", gained["name"]).upper() in _JUNK_NAMES:
            score -= 4.0
        else:
            score += 0.5 * (deck_value(gained) - 3.0) if _known_card(gained) else 0.0
    m = re.search(r"lose\s+(\d+)(%?)\s+max\s+hp", text)
    if m:
        # Max HP never comes back: a small bite is a fair price for an
        # upgrade or a relic, a big one is not worth anything on offer.
        share = int(m.group(1)) / 100.0 if m.group(2) else int(m.group(1)) / max(1, max_hp)
        score -= 1.5 + 60.0 * share + (1000.0 if share >= 0.25 else 0.0)
    m = re.search(r"(?:lose|take)\s+(\d+)\s+(?:hp|damage)", text)
    if m:
        loss = int(m.group(1))
        score -= 1000.0 if loss >= hp else 12.0 * loss / max(1, hp)
        if bled >= 5:
            # This event has already taken HP from us: each further bite costs more,
            # so a chain of small losses is stopped before it adds up.
            score -= 2.0 + 0.5 * bled
    if re.search(r"heal", text) and hp >= 0.9 * max_hp:
        score -= 2.0
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
    worth = lambda o: _event_option_value(o, hp, max_hp, bled)
    rest = sorted((o for o in opts if not o.get("is_proceed")), key=worth, reverse=True)
    if log.isEnabledFor(logging.DEBUG) and rest:
        log.debug("event %s: %s", ev.get("event_id"), " | ".join(f"[{worth(o):.1f}] {o.get('title')}: {o.get('description')}" for o in rest))
    ordered = [o for o in opts if o.get("is_proceed")] + rest
    return [("choose_event_option", {"index": int(o.get("index", 0))}) for o in ordered]
