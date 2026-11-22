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
_GOOD = [
    (_RELIC_WORD, 6.0), (r"\bremove\b", 4.0), (r"\bupgrade\b", 4.0),
    (r"\btransform\b", 2.0), (r"\bpotions?\b", 1.5), (r"\bcards?\b", 1.0), (r"\benchant\b", 3.0),
    (r"draw \d+ additional cards?", 5.0),  # every turn, for the rest of the run
    (r"energy_icon", 5.0),
    (r"\bgreater rewards\b", 6.0),  # Punch Off: two 50 HP constructs that hit for 5x2, for a relic, a card and a potion
]
# The Lantern Key's fight (a Mysterious Knight) cost 27 HP on average over 17
# tries in the middle of act 2 and leaves an unplayable card in the deck; what
# it unlocks is an act 3 event that 4 of those runs lived to see. About one in
# five of them got past the act 2 boss, against 44% of the other runs.
# (Foul Potions hit us as hard as the enemy, so the bot hardly ever throws one:
# three of them fill the belt for nothing. The Potion Courier's other offer is
# one Uncommon potion.)
_BAD = [(r"\bfoul potions?\b", 1.0), (r"\bfight to obtain the key\b", 6.0), (r"\bcurse\b", 6.0), (r"\bfight\b", 2.0), (r"draw \d+ fewer cards?", 4.0), (r"\bdowngrade\b", 2.0), (r"\bsacrifice\b", 5.0)]
# An option that starts a fight is priced by the HP the fight takes, as "Lose N HP" is. Punch Off's "Fight them for
# Greater Rewards" (two constructs) took 16, 37 (a death) and 44 HP in the three it was taken on 2026-11-22, and was
# rated 4.5 at any HP, above Nab's 3.5: the +6 for the rewards and -2 for the word "fight" were all it cost. 32 is
# their mean (the act 1 and 2 elites cost 21 and 30 on the route table, strategy.FIGHT_COST).
EVENT_FIGHT_HP = [(r"\bfight them\b", 32.0)]
# Options that end the run in all but name. One event offers ever larger
# rewards for Max HP until the last step, "Lose Everything", leaves 1 Max HP;
# the bot walked into it nine times.
# Duplicating the whole deck doubles every draw pile and dilutes the good cards.
_DUPLICATE_DECK = re.compile(r"duplicate (?:your|the) (?:whole )?deck")
# Something bad we would receive or be given: it is not a relic, whatever the "Receive ..." wording
# suggests. What each costs, where a relic is worth 6: until 2026-11-21 every one cost 6 (10 as a card
# "added to your Deck"), so "Add Clumsy to your Deck. Obtain a random Relic." was refused 45 times in 45
# for "Lose 6 HP. Gain 52 Gold". Clumsy is "Unplayable. Ethereal": one draw a fight, then it is gone.
# Guilty leaves after 5 combats, Poor Sleep ("Retain") is drawn once a fight, Injury is a dead draw every
# time round the deck, Greed and the Bell's curse can never be removed; Decay, Doubt, Shame and Regret
# hurt each time they are held; Normality stops the turn at three cards and Bad Luck takes 13 HP.
# Two relics belong here too. Royal Poison ("At the start of each combat, lose 4 HP") came with "Heal to
# full HP" and was taken 6 times in 6, at 90% HP or more. The Sword of Stone ("Transforms into a powerful
# Relic after defeating 5 Elites") was taken 27 times in 27 over 115 Gold for 7 HP: 2 runs in 147 fought
# five elites (it is worth the half point over; the rest are costs).
_CURSES = {
    "clumsy": 1.5, "guilty": 1.5, "poor sleep": 2.0, "folly": 2.5, "spore mind": 2.5, "injury": 3.0, "debt": 4.0,
    "decay": 4.5, "doubt": 4.5, "shame": 4.5, "greed": 5.0, "curse of the bell": 5.0, "regret": 5.0, "writhe": 5.0,
    "normality": 8.0, "enthralled": 8.0, "bad luck": 10.0, "royal poison": 9.0, "sword of stone": -0.5,
}
_CURSE_GAIN = re.compile(
    r"\b(?:receive|obtain|add|gain)\b[^.]*?\b(" + "|".join(_CURSES) + r"|wound|burn|dazed|hex|parasite|"
    r"pain|spite|void|mind rot|sloth|waste away|disintegration|curse)\b"
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
# (The second and third Ancients, 2026-11-20. Cards every turn do more for this bot than energy every turn: its
# hands run out of cards before they run out of energy (4 energy or more at the last boss dealt 36.5 a turn,
# 3 energy 34.3). Of 166 runs that reached act 2, 51% reached act 3; with Snecko Eye 4 of 5 and 3 wins, Pael's
# Blood 15 of 19, Velvet Choker 5 of 6; with Yummy Cookie 1 of 9, Prismatic Gem 1 of 5, Philosopher's Stone 1 of
# 4, Sozu 1 of 3, and Electric Shrymp and Sand Castle (as Yummy Cookie, upgrades and nothing after) no win in 28. Other players' runs
# (tierspire.com, the share won with each relic of one Ancient) rank them the same way: Darv's Snecko Eye 56%,
# Runic Pyramid 56%, Pandora's Box 52% against Philosopher's Stone 45%, Velvet Choker 41%, Sozu 40%, Ectoplasm
# 38%; Orobas's Radiant Pearl and Prismatic Gem 50%, Alchemical Coffer 46%, Glass Eye 36%. Read word by word,
# every relic that gives energy scored 12.5 or more, so Ectoplasm (no more gold, with two acts of shops ahead)
# beat Snecko Eye.)
# (The third Ancient, the same day, over the 200 runs that reached act 3 (24 won, 12%): Blessed Antler held 16
# times and Spiked Gauntlets 15 without one win, Looming Fruit (+31 Max HP, taken 21 times in 21) 1; Whispering
# Earring 4 in 16, War Hammer 3 in 11, Lord's Parasol 3 in 10. Max HP itself does not separate wins from
# losses at the last boss (gained under 5: 4 of 20; over 30: 6 of 21).)
# (These are where each relic starts: relics.py moves every one of them by what the runs that held it did, up
# to 3 points either way. Seal of Gold, an energy every turn for 3 Gold, was 8.0 until 2026-11-21 and so never
# taken beside Very Hot Cocoa or Toasty Mittens: energy from the first turn is what both fight models say a
# deck is shortest of (scripts/demonfight.py: 33 to 56 points of a Knowledge Demon's win chance; roomfight.py:
# 8 of the 25 HP an ordinary act 2 fight costs), act 2 bosses met on 4 energy or more cost 18% of Max HP less
# than the same character's others (27 of 85 fights), and its 15 runs ended 5.7 floors over the usual.)
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
    # (Four that were read word by word until 2026-11-21, which made Astrolabe 13.5 and Empty Cage 11.5, over
    # Philosopher's Stone and Sozu every time: their runs ended 1.4 and 1.0 floors under the usual, Driftwood's 4.9.)
    "ASTROLABE": 9.0, "EMPTY_CAGE": 8.5, "THROWING_AXE": 7.5, "DRIFTWOOD": 6.5,
    # things the bot cannot make use of
    "WINGED_BOOTS": 4.0, "PAELS_WING": 4.0, "MEAT_CLEAVER": 3.5, "PAELS_EYE": 3.0, "DISTINGUISHED_CAPE": 3.0,
}


RELIC = 6.0  # what a relic counts for in an event (_GOOD)
ACT3_HP = 1.6  # how much more a point of HP given up in an event costs in act 3 (the last boss is won from full HP: 6 in 16, none in 12 below 90%)
# Chance that an ordinary fight ends the run, from the bot's fights.jsonl (Ascension 7+, act 2 monster fights, all
# characters, 2026-11-22): entered under half HP 22% (71 of 326), from half up 0.4% (7 of 1,661).
FIGHT_DEATH_LOW, FIGHT_DEATH_HIGH = 0.22, 0.005


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
        # (the table's number, moved by how the runs that held it went, and for an energy or a card every turn by the
        # energy the deck already has: relicworth.energy_lean)
        return known + relics.shift(key) + energy_lean(key, energy, held, deck)
    score = 0.5
    if opt.get("relic_name"):
        score += 6.0
    # Relics that come "after N combats" (Wongo's Mystery Box, three after five) are had only if the run lives through
    # the fights: at 8 HP the box was rated 16.5 against 5.8 for one relic now, and the run died three fights later.
    lives = 1.0
    if m := re.search(r"\bobtain \d+ random relics after (\d+) combats\b", text):
        lives = _lives_through(int(m.group(1)), hp, max_hp)
    for pat, w in _GOOD:
        if re.search(pat, text) and not re.search(r"(?:lose|pay|give)[^.]*" + pat, text):
            score += w * (lives if pat == _RELIC_WORD else 1.0)
    # Gold is worth what it buys (a removal is 100 to 200, a relic 150 and up), a heal what is missing and
    # Max HP its amount: all three used to be flat, so 311 Gold was worth what 35 was, "Heal to full HP"
    # the same at 9 HP in 10, and 2 Max HP for 3 damage more than 10 HP back at two thirds health.
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
    # "Obtain the Sword of Stone", "Receive the Spoils Map": a relic by name.
    # Each "obtain/receive" is judged on its own, so "Receive Bad Luck" is not one.
    if not opt.get("relic_name") and "relic" not in text:
        for gain in re.finditer(r"\b(?:obtain|receive) (?:the )?(?!a\b|an\b|\d)[a-z][a-z' ]*", text):
            if not _CURSE_GAIN.search(gain.group(0)):
                score += 5.0
                break
    if _DUPLICATE_DECK.search(text):
        score -= 12.0
    m = re.search(r"\b(?:pay|give|lose) (\d+) gold", text)
    if m:
        score -= int(m.group(1)) / (300.0 if act is not None and act >= 3 else 150.0)  # (gold has little left to buy in act 3)
    # A potion handed over is one fewer at the boss, where the second one on the belt goes with twice the
    # wins. Until 2026-11-22 it cost nothing: "Give Attack Potion", "Give 100 Gold" and "Give Anchor" for a
    # relic all scored 6.5 and the first was taken, with 112 Gold in hand; two floors on a Clarity Extract
    # went for an upgraded card, and that run met the last boss holding one potion and 240 Gold.
    for p in potions or []:
        name = str(p.get("name") or "").strip().lower()
        if name and re.search(r"\b(?:lose|give|insert)\b[^.]*\b" + re.escape(name), text):
            score -= (1.0 + boss_worth(p) / 12.0) * (1.5 if act is not None and act >= 3 else 1.0)
            break
    # A relic of ours handed over costs a relic and what this one does (relicworth.py), one traded in brings what
    # it does: until 2026-11-22 the Relic Trader's three trades all scored 0.5 and the top one was taken (Gorget
    # for Akabeko over Joss Paper for Gremlin Horn), and Ranwid was given a Beating Remnant like any other relic.
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
            score -= 0.0 if cursed else 4.0  # (a curse has been charged for by name)
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
        score -= 1000.0 if loss >= hp else 12.0 * loss / max(1, hp) * (ACT3_HP if act is not None and act >= 3 else 1.0)
        if bled >= 5:
            # This event has already taken HP from us: each further bite costs more,
            # so a chain of small losses is stopped before it adds up.
            score -= 2.0 + 0.5 * bled
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
