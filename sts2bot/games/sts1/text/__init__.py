"""What the first game's cards and potions say: CommunicationMod sends no text, and the planner reads cards by it (cards.parse).
Keyed like cards.key_of spells a name; looked up by id, then name. "Deal 6(9) damage." is 6, or 9 once upgraded; "a||b" is text a, or b upgraded."""

from __future__ import annotations

import re

from ....cards import key_of
from . import colorless, defect, ironclad, potions, silent, watcher

CARDS: dict[str, str] = {**ironclad.CARDS, **silent.CARDS, **defect.CARDS, **watcher.CARDS, **colorless.CARDS}

# Ids that are not the card's name in key form.
ALIASES = {
    "UNDERHANDED_STRIKE": "SNEAKY_STRIKE", "CRIPPLING_POISON": "CRIPPLING_CLOUD", "VENOMOLOGY": "ALCHEMIZE", "WRAITH_FORM_V2": "WRAITH_FORM", "CONSERVE_BATTERY": "CHARGE_BATTERY", "GASH": "CLAW", "STEAM": "STEAM_BARRIER", "REDO": "RECURSION",
    "BOOTSEQUENCE": "BOOT_SEQUENCE", "UNDO": "EQUILIBRIUM", "LOCKON": "BULLSEYE", "STEAM_POWER": "OVERCLOCK", "GHOSTLY": "APPARITION", "BOWLINGBASH": "BOWLING_BASH", "CRUSHJOINTS": "CRUSH_JOINTS", "CUTTHROUGHFATE": "CUT_THROUGH_FATE",
    "EMPTYBODY": "EMPTY_BODY", "EMPTYFIST": "EMPTY_FIST", "EMPTYMIND": "EMPTY_MIND", "FLURRYOFBLOWS": "FLURRY_OF_BLOWS", "FLYINGSLEEVES": "FLYING_SLEEVES", "FOLLOWUP": "FOLLOW_UP", "JUSTLUCKY": "JUST_LUCKY", "PATHTOVICTORY": "PRESSURE_POINTS",
    "SASHWHIP": "SASH_WHIP", "CLEARTHEMIND": "TRANQUILITY", "THIRDEYE": "THIRD_EYE", "BATTLEHYMN": "BATTLE_HYMN", "CARVEREALITY": "CARVE_REALITY", "DECEIVEREALITY": "DECEIVE_REALITY", "FASTING2": "FASTING", "FEARNOEVIL": "FEAR_NO_EVIL",
    "FOREIGNINFLUENCE": "FOREIGN_INFLUENCE", "WIREHEADING": "FORESIGHT", "INNERPEACE": "INNER_PEACE", "LIKEWATER": "LIKE_WATER", "MENTALFORTRESS": "MENTAL_FORTRESS", "REACHHEAVEN": "REACH_HEAVEN", "ADAPTATION": "RUSHDOWN",
    "SANDSOFTIME": "SANDS_OF_TIME", "SIGNATUREMOVE": "SIGNATURE_MOVE", "VENGEANCE": "SIMMERING_FURY", "TALKTOTHEHAND": "TALK_TO_THE_HAND", "WAVEOFTHEHAND": "WAVE_OF_THE_HAND", "WHEELKICK": "WHEEL_KICK", "WINDMILLSTRIKE": "WINDMILL_STRIKE",
    "WREATHOFFLAME": "WREATH_OF_FLAME", "CONJUREBLADE": "CONJURE_BLADE", "DEUSEXMACHINA": "DEUS_EX_MACHINA", "DEVAFORM": "DEVA_FORM", "JUDGEMENT": "JUDGMENT", "LESSONLEARNED": "LESSON_LEARNED", "MASTERREALITY": "MASTER_REALITY",
    "SPIRITSHIELD": "SPIRIT_SHIELD", "HANDOFGREED": "HAND_OF_GREED", "RITUALDAGGER": "RITUAL_DAGGER", "PANICBUTTON": "PANIC_BUTTON", "ASCENDERSBANE": "ASCENDER_S_BANE", "CURSEOFTHEBELL": "CURSE_OF_THE_BELL", "BECOMEALMIGHTY": "BECOME_ALMIGHTY",
    "FAMEANDFORTUNE": "FAME_AND_FORTUNE", "LIVEFOREVER": "LIVE_FOREVER", "THROUGHVIOLENCE": "THROUGH_VIOLENCE",
}

# Cards whose upgrade lowers the cost: (cost, cost upgraded).
UPGRADE_COST = {
    "BODY_SLAM": (1, 0), "HAVOC": (1, 0), "BLOOD_FOR_BLOOD": (4, 3), "EXHUME": (1, 0), "BARRICADE": (3, 2), "CORRUPTION": (3, 2), "ENTRENCH": (2, 1), "INFERNAL_BLADE": (1, 0), "SEEING_RED": (1, 0), "DARK_EMBRACE": (2, 1), "BULLET_TIME": (3, 2),
    "DISTRACTION": (1, 0), "NIGHTMARE": (3, 2), "PHANTASMAL_KILLER": (1, 0), "TERROR": (1, 0), "SETUP": (1, 0), "ALCHEMIZE": (1, 0), "RECYCLE": (1, 0), "DOUBLE_ENERGY": (1, 0), "CREATIVE_AI": (3, 2), "FUSION": (2, 1), "ZAP": (1, 0),
    "WHITE_NOISE": (1, 0), "ERUPTION": (2, 1), "TRANQUILITY": (1, 0), "CRESCENDO": (1, 0), "OMNISCIENCE": (4, 3), "VAULT": (3, 2), "SCRAWL": (1, 0), "STUDY": (2, 1), "RUSHDOWN": (1, 0), "APOTHEOSIS": (2, 1), "MADNESS": (1, 0),
    "MASTER_REALITY": (1, 0), "MAGNETISM": (2, 1), "MAYHEM": (2, 1), "MIND_BLAST": (2, 1),
}

_UPGRADE = re.compile(r"(\d+)\((\d+)\)")
_ENERGY = re.compile(r"\bgain\s+(\d+)\s+energy\b", re.I)


def expand(text: str, upgraded: bool) -> str:
    """One version of a table entry, energy written the way the parser reads it ("Gain 2[energy_icon.png]")."""
    if "||" in text:
        text = text.split("||", 1)[1 if upgraded else 0]
    text = _UPGRADE.sub(lambda m: m.group(2) if upgraded else m.group(1), text)
    return _ENERGY.sub(lambda m: f"Gain {m.group(1)}[energy_icon.png]", text)


_DEAL = re.compile(r"\b(deal)\s+\d+\s+damage", re.I)
_GAIN_BLOCK = re.compile(r"\b(gain)\s+\d+\s+block", re.I)
# Sentences whose effect the game's own damage number already holds.
_FOLDED_IN = re.compile(r"\s*(?:strength\s+affects\s+this\s+card\s+\d+\s+times|deals?\s+\d+\s+additional\s+damage\s+for\s+all\s+your\s+cards\s+containing\s+strike)\.?", re.I)


def live_numbers(desc: str, ctype: str, card: dict) -> str:
    """A hand card's text with the damage and Block the game worked out for it (the bot's mod sends them), as StS2 prints them."""
    dmg, base_dmg = card.get("damage"), card.get("base_damage")
    if ctype == "Attack" and isinstance(dmg, int) and isinstance(base_dmg, int) and base_dmg >= 0:
        n = max(0, dmg)
        desc = _FOLDED_IN.sub("", desc)
        desc = re.sub(r"\bdeal\s+damage\s+equal\s+to\s+[^.]*", f"Deal {n} damage", desc, flags=re.I)  # (Body Slam, Mind Blast)
        desc = _DEAL.sub(lambda m: f"{m.group(1)} {n} damage", desc)
    blk, base_blk = card.get("block"), card.get("base_block")
    if ctype in ("Attack", "Skill") and isinstance(blk, int) and isinstance(base_blk, int) and base_blk >= 0:
        desc = _GAIN_BLOCK.sub(lambda m: f"{m.group(1)} {max(0, blk)} Block", desc)
    return desc


def card_key(card_id, name=None) -> str | None:
    """The table key for a card, or None if it is not in the tables."""
    for raw in (card_id, name):
        if not raw:
            continue
        key = key_of(raw)
        key = ALIASES.get(key, key)
        if key in CARDS:
            return key
    return None


def card_text(card_id, name=None, upgraded: bool = False) -> str:
    """What the card does ("" for one not in the tables)."""
    key = card_key(card_id, name)
    return expand(CARDS[key], upgraded) if key else ""


def potion_key(potion_id, name=None) -> str:
    for raw in (name, potion_id):
        key = key_of(raw)
        if key in potions.POTIONS:
            return key
    return key_of(name or potion_id)


def potion_text(potion_id, name=None) -> str:
    return potions.POTIONS.get(potion_key(potion_id, name), "")


def relic_text(relic_id, name=None) -> str:
    return potions.RELICS.get(key_of(name or relic_id), "") or potions.RELICS.get(key_of(relic_id), "")


def boss_relic_rank(relic: dict) -> int:
    """Lower is better (the order the boss's relics are offered to the bot in)."""
    key = key_of(relic.get("name") or relic.get("id"))
    if key in potions.AVOID:
        return 1000 + potions.AVOID.index(key)
    return potions.BOSS_RELICS.index(key) if key in potions.BOSS_RELICS else 500
