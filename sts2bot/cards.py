"""Card understanding from the text the game shows on the card.

There is no card database here on purpose: the bot reads the rendered
description, so it keeps working when the game adds or rebalances cards.
"""

from __future__ import annotations

import re
from dataclasses import dataclass

from . import games
from .run.crowd import rating as crowd_rating

_WORD_NUM = {"twice": 2, "thrice": 3}
_BASIC = re.compile(r"^(strike|defend)(_[a-z]+)?$", re.I)


@dataclass(frozen=True)
class CardInfo:
    index: int
    name: str
    type: str  # Attack, Skill, Power, Status, Curse
    cost: int  # energy; X-cost cards report -1
    star_cost: int
    damage: int  # per hit
    hits: int
    double_if_vulnerable: bool  # "If the enemy is Vulnerable, hits twice."
    aoe: bool
    random_target: bool
    block: int
    vulnerable: int
    weak: int
    draw: int
    energy_gain: int
    hp_loss: int
    needs_target: bool
    playable: bool
    deadly: bool  # the text says it can kill us outright ("... die.")
    block_lock: bool  # stops us gaining Block afterwards
    known: bool  # False when no effect could be parsed from the text
    strength: int = 0  # Strength we gain when it is played
    strength_lasts: bool = False  # ... and keep after this turn
    enemy_strength_loss: int = 0  # "Enemy loses N Strength this turn."
    enemy_strength_lasts: bool = False  # ... or for good, without the "this turn" (Shared Fate, Resonance)
    key: str = ""  # card id without the upgrade mark, e.g. DEMON_FORM
    bound: bool = False  # afflicted with Bound: only one such card can be played per turn
    # --- other characters' mechanics
    poison: int = 0  # Poison applied (per application)
    poison_needs: bool = False  # ... only to an enemy that already has Poison (Bubble Bubble: nothing otherwise)
    doom: int = 0  # Doom applied
    shivs: int = 0  # Shivs (0-cost 4 damage cards) added to the hand
    star_gain: int = 0  # Stars gained (the Regent's second resource)
    forge: int = 0  # damage added to the Sovereign Blade for the fight
    summon: int = 0  # HP given to Osty, who takes hits for the Necrobinder
    dexterity: int = 0  # Dexterity gained
    dexterity_lasts: bool = False
    channel: tuple = ()  # orbs channeled, e.g. ("LIGHTNING", "FROST")
    evoke: int = 0  # times the rightmost orb is evoked
    next_energy: int = 0  # energy gained next turn
    next_draw: int = 0  # extra cards drawn next turn
    next_block: int = 0  # Block gained next turn
    keeps_hand: tuple = ()  # "Retain your Hand this turn": ("ANY",), or the Melodies it needs (Ki Rest: "If you have a Wah Melody or Cho Melody")
    enemy_hp_loss: int = 0  # "Enemy loses N HP." (ignores Block)
    souls: int = 0  # Souls added to the draw pile (each: draw 2 cards)
    evoke_all: int = 0  # times every orb is evoked (Shatter)
    short: bool = False  # cannot be played right now only for want of energy or Stars
    osty_attack: bool = False  # "Osty deals N damage": nothing happens while Osty is dead
    osty_dies: bool = False  # Bone Shards: Osty dies, and with him the HP that stood in front of us
    doom_kills: bool = False  # End of Days: enemies with as much Doom as HP die at once, not at the end of their turn
    debris: int = 0  # Debris (1 energy: Exhaust) it adds to our hand; -1: as many as the hand has room for (Crash Landing)
    heal: int = 0  # HP it gives back when played (Not Yet)
    ethereal: bool = False  # gone for the fight if it is still in the hand when the turn ends
    ends_turn: bool = False  # "End your turn." (Void Form), "Exhaust your Hand." (Fiend Fire, Stoke): nothing in the hand can be played after it
    replay: int = 0  # times it is played again by itself ("Replay 1": Sword Sage's Sovereign Blade, Hidden Gem, Transfigure)
    play_limit: int = -1  # "You can only play 1 more card this turn." (Encore): cards allowed after it (-1: no limit)
    # --- the Fool's Arrows: a card that Shoots needs an Arrow not standing in an enemy, or it does nothing
    shoots: bool = False
    aim_times: int = 1  # "Aim affects this card 2 times" (Heavy Shot): the Aim in its text counts this often
    aim: int = 0  # Aim gained ("Your next Arrow deals more damage"), per energy on an X card (Brace)
    stretch: int = 0  # Stretch gained ("The next time you gain Block from a card, gain additional Block")
    impaled_only: bool = False  # Nail: "If the enemy is Impaled, Deal 20 damage ...": nothing otherwise
    on_impaled: tuple = ()  # (energy, cards drawn) it adds when its target is Impaled (Bow Smack, Ki Hop)
    splash: bool = False  # Rain Shot: "Damage ALL other enemies equal to the damage dealt": the same again on each other enemy
    stretch_dealt: bool = False  # Blossom Shot: "gain Stretch equal to the damage dealt" (what its hits did, as Stretch)
    dislodge: bool = False  # takes our Arrow back out of its (Impaled) target: Uproot, Recall
    impaled_double: bool = False  # Wah Hammer: "Deals double damage to Impaled enemies"
    arrow_block: bool = False  # Cho Firm: "If you have an Arrow, gain Block an additional time"
    melody: tuple = ()  # the Melodies playing it gives for the turn: the Wah, Cho and Ki in its name
    melody_hits: bool = False  # "Hits an additional time for each Melody you have" (its text counts those there are)
    melody_block: int = 0  # "Gain 5 Block for each Melody you have" (Harmonic Shield): Block a Melody
    melody_twice: tuple = ()  # Wah Blast: "If you have a Cho Melody or Ki Melody, hits twice"
    verse: int = 0  # Ki Verse: "The next (2) "Wah", "Cho", or "Ki" card(s) you play this turn ... played an additional time"
    stance: str = ""  # the first game's Watcher: the stance the card puts her in (WRATH, CALM, DIVINITY, EXIT; games/sts1/stance.py)

    @property
    def is_x(self) -> bool:
        return self.cost < 0

    @property
    def total_damage(self) -> int:
        return self.damage * self.hits


MELODIES = ("WAH", "CHO", "KI")


def melodies(name) -> tuple:
    """The Fool's Melodies a card gives when played ("Wah Melody: activates if you played a "Wah" card this turn"): the words of its name."""
    return tuple(m for m in MELODIES if m in re.findall(r"[A-Z]+", str(name).upper()))


def key_of(name) -> str:
    """A card's name or id as the tables spell it: DEMON_FORM for "Demon Form+"."""
    return re.sub(r"[^A-Z0-9]+", "_", str(name or "").upper().rstrip("+")).strip("_")


def _int(pattern: str, text: str) -> int:
    m = re.search(pattern, text, re.I)
    return int(m.group(1)) if m else 0


def _icons(text: str) -> int:
    """Energy gained, as the game writes it: "Gain" and one icon per point, or a number and one icon
    ("Gain 4[icon]": Supercritical and Borrowed Time were cards the planner could make nothing of)."""
    m = re.search(r"gain\s+(\d+)\s*\[[a-z_]*energy_icon\.png\]", text, re.I)
    if m:
        return int(m.group(1))
    m = re.search(r"gain\s+((?:\[[a-z_]*energy_icon\.png\]\s*)+)", text, re.I)
    return len(_ENERGY_ICON.findall(m.group(1))) if m else 0


def _cost(raw) -> int:
    if raw is None:
        return 0
    s = str(raw).strip()
    if s.upper() == "X":
        return -1
    try:
        return max(0, int(s))
    except ValueError:
        return 0


def _keeps_hand(text: str) -> tuple:
    """"Retain your Hand this turn" (Convergence): ("ANY",); behind a Melody (Ki Rest: "If you have a Wah Melody or Cho Melody,
    Retain your Hand this turn"): the Melodies that do it; () for every other card."""
    m = re.search(r"(?:if\s+you\s+have\s+([^,.]*?melody[^,.]*?),\s*)?retain\s+your\s+hand", text, re.I)
    if not m:
        return ()
    return tuple(w.upper() for w in re.findall(r"(\w+)\s+melody", m.group(1), re.I)) if m.group(1) else ("ANY",)


def _replay(card: dict, text: str) -> int:
    """Extra times the card plays itself: "Replay 2." in its own text, or the keyword alone. (Not a card that
    hands Replay to another: "Sovereign Blade gains Replay 1.")"""
    m = re.search(r"(?:^|[.\n]\s*)replay\s+(\d+)", text, re.I)
    if m:
        return int(m.group(1))
    if re.search(r"replay", text, re.I):
        return 0
    for k in card.get("keywords") or []:
        if isinstance(k, dict) and str(k.get("name")).lower() == "replay":
            return _int(r"additional\s+(\d+)\s+times", str(k.get("description") or "")) or 1
    return 0


def is_basic(card: dict) -> bool:
    return bool(_BASIC.match(str(card.get("id") or card.get("name") or "")))


def is_junk(card: dict) -> bool:
    return str(card.get("type")) in ("Status", "Curse")


def kind(card: dict) -> str:
    """What sort of card this is to a deck whose turns run out of energy before they run out of cards.

    "energy": it gives some, now or next turn. "free": an attack that costs none. "draw": it costs some
    and brings cards and little else. "" for every other card. The bot's boss turns end with about three
    cards it could not pay for and no energy, so the first two should be worth more to it than to the
    players crowd.py listens to, and the third less: learn.py keeps a record for each kind, and a card
    starts from its kind's record instead of from nothing. (Free Skills are no kind: the 140 picks of
    them ended under par, and Shared Fate is one the planner leaves in the hand.)
    """
    if str(card.get("type")) not in ("Attack", "Skill"):
        return ""
    info = parse(card)
    if (info.energy_gain or info.next_energy) and info.play_limit < 0:  # (Encore's energy pays for one card)
        return "energy"
    if info.cost == 0 and not info.star_cost:
        return "free" if str(card.get("type")) == "Attack" and info.total_damage else ""
    m = re.search(r"put (?:up to )?(a|\d+) cards? from your discard pile into your hand", str(card.get("description") or ""), re.I)
    draws = info.draw + 2 * info.souls + (0 if not m else 1 if m.group(1).lower() == "a" else int(m.group(1)))
    does = info.total_damage + info.doom + 3 * info.poison + 4 * info.shivs >= 8 or info.block >= 5
    return "draw" if draws >= 2 and not does else ""


# Sentences that describe something that happens later or only sometimes
# ("Whenever you gain Block, deal 6 damage ..."). Reading those as what the
# card does when played made the planner count on damage that never came.
_LATER = re.compile(
    r"^(whenever|every time|at the (start|end) of|next turn|if you|if your|if this|if fatal|the (first|next) time|this turn, your next)",
    re.I,
)
_ENERGY_ICON = re.compile(r"\[[a-z_]*energy_icon\.png\]", re.I)


def immediate_text(text: str) -> str:
    """The part of a card's text that happens as it is played."""
    keep = []
    for sentence in re.split(r"(?<=[.!?])\s+", text):
        s = sentence.strip()
        if not s or _LATER.match(s):
            continue
        # "Gain 12 Block. Whenever you are attacked this turn, deal 4 damage back."
        keep.append(re.split(r",?\s+(?:whenever|if you|if your)\b", s, flags=re.I)[0] if keep else s)
    return " ".join(keep)


def parse(card: dict) -> CardInfo:
    full_text = str(card.get("description") or "")
    ctype = str(card.get("type") or "")
    target = str(card.get("target_type") or "")
    # A Power does nothing the moment it is played; its text is all "later".
    text = "" if ctype == "Power" else immediate_text(full_text)
    # "If the enemy is Impaled, ..." (the Fool's Arrow stands in it): Nail is nothing without one, and Bow
    # Smack's energy and Ki Hop's draw only come with one. Read as plain, Nail was 20 damage on any enemy.
    impaled_only, on_impaled = False, ()
    if m := re.search(r"if\s+the\s+enemy\s+is\s+impaled,?\s*((?:\[[^\]]*\]|[^.])*)\.?", text, re.I):  # (an icon's ".png" is not the end)
        if re.search(r"deal\s+\d+\s+damage|\bapply\b|\bdislodge\b", m.group(1), re.I):
            impaled_only = True  # (Uproot: "Dislodge and apply 3 Vulnerable"; Recall: "Dislodge and put a card ...")
        else:
            on_impaled = (_icons(m.group(1)), _int(r"draw\s+(\d+)\s+card", m.group(1)))
            text = text[: m.start()] + text[m.end() :]
    # "Aim affects this card 2 times" (Heavy Shot) is not 2 hits: one hit with the Aim counted twice. Nor is the
    # first game's Heavy Blade ("Strength affects this card 5 times": read as 14 damage 5 times, 70 for 2 energy).
    aim_times = 1
    if m := re.search(r"(aim|strength)\s+affects\s+this\s+card\s+(?:(\d+)\s+times?|twice)\.?", text, re.I):
        aim_times = int(m.group(2) or 2) if m.group(1).lower() == "aim" else 1
        text = text[: m.start()] + text[m.end() :]

    # "Next turn, gain 4 Block." is read separately below, not as this turn's.
    next_text = "" if ctype == "Power" else " ".join(x for x in re.split(r"(?<=[.!?])\s+", full_text) if x.strip().lower().startswith("next turn"))
    damage = _int(r"deals?\s+(\d+)\s+damage", text)
    if not damage:
        # "Deal damage equal to your Block. (Deals 14 damage)"
        damage = _int(r"\(deals\s+(\d+)\s+damage\)", text)
    hits = 1
    double_if_vulnerable = False
    if damage:
        # A conditional second hit must not count as an unconditional one.
        plain = text
        m = re.search(r"if\s+the\s+enemy\s+is\s+vulnerable,?\s+hits\s+twice\.?", text, re.I)
        if m:
            double_if_vulnerable = True
            plain = text[: m.start()] + text[m.end() :]
        hits = _int(r"(\d+)\s+times", plain) or 1
        for word, n in _WORD_NUM.items():
            # Only where it counts the hits: Debilitate ("Deal 10 damage. Vulnerable and
            # Weak are twice as effective ...") was planned as two hits and dealt one.
            if re.search(rf"(?:damage[^.]*|\bhits?\s+)\b{word}\b", plain, re.I):
                hits = n
    m = re.search(r"\(hits\s+(\d+)\s+times?\)", full_text, re.I)
    if m and damage:  # "Deal 5 damage for each Skill in your Hand. (Hits 2 times)"
        hits = int(m.group(1))
        if hits == 0:
            damage = 0
    block = _int(r"gain\s+(\d+)\s+block", text)
    if block and re.search(r"gain\s+\d+\s+block\s+twice", text, re.I):
        block *= _WORD_NUM["twice"]  # "Gain 5 Block twice" (Cho Limbs): two gains of 5, read as one
    melody_block = block if re.search(r"block\s+for\s+each\s+melody", text, re.I) else 0
    if m := re.search(r"\(gain\s+(\d+)\s+block\)", text, re.I):
        block = int(m.group(1))  # "Gain 5 Block for each Melody you have. (Gain 0 Block)": what it gives now
    if not block and damage and re.search(r"gain\s+block\s+equal\s+to\s+(?:the\s+)?damage\s+dealt", text, re.I):
        block = damage * hits  # Fisticuffs
    vulnerable = _int(r"apply\s+(\d+)\s+vulnerable", text) or _int(r"(\d+)\s+(?:weak\s+and\s+)?vulnerable", text)
    weak = _int(r"apply\s+(\d+)\s+weak", text) or _int(r"(\d+)\s+(?:vulnerable\s+and\s+)?weak", text)
    draw = _int(r"draw\s+(\d+)\s+card", text) or (1 if re.search(r"draw\s+a\s+card", text, re.I) else 0)
    energy_gain = _icons(text) or _int(r"gain\s+(\d+)\s+energy", text)  # the game writes energy as icons, one per point
    if re.search(r"when this card is exhausted,\s*gain", text, re.I):
        energy_gain = 0  # Drum of Battle: playing it only draws; the energy comes if something Exhausts it
    hp_loss = _int(r"lose\s+(\d+)\s+hp", text)
    stat_text = full_text if ctype == "Power" and not _LATER.match(full_text.strip()) else text

    def stat(name: str) -> tuple[int, bool]:
        """(Strength or Dexterity gained, whether it is kept after this turn)."""
        m = re.search(rf"gain\s+(\d+)\s+{name}(\s+this\s+turn)?", stat_text, re.I)
        return (int(m.group(1)), not m.group(2)) if m else (0, False)

    strength, strength_lasts = stat("strength")
    dexterity, dexterity_lasts = stat("dexterity")
    # "Lose 2 Strength." as the price of the card (Friendship, Shared Fate): for the rest of the fight.
    # Read as nothing, two Friendships went down on sight and the deck's Strikes hit for 2 from then on.
    if not strength and (m := re.search(r"(?:^|\.\s+)lose\s+(\d+)\s+strength\b(?!\s+this\s+turn)", stat_text, re.I)):
        strength, strength_lasts = -int(m.group(1)), True
    enemy_strength_loss = _int(r"enem(?:y|ies)\s+loses?\s+(\d+)\s+strength", text)
    enemy_strength_lasts = bool(enemy_strength_loss) and not re.search(r"enem(?:y|ies)\s+loses?\s+\d+\s+strength\s+this\s+turn", text, re.I)
    # "X" amounts on an X-cost card (Malaise: "Enemy loses X Strength. Apply X Weak."): 1 per energy spent.
    # Read as nothing, Malaise was played 9 times in 51 hands.
    if re.search(r"enem(?:y|ies)\s+loses?\s+X\s+strength", text, re.I):
        enemy_strength_loss = 1
    weak = weak or int(bool(re.search(r"apply\s+X\s+weak", text, re.I)))
    vulnerable = vulnerable or int(bool(re.search(r"apply\s+X\s+vulnerable", text, re.I)))
    poison = _int(r"apply\s+(\d+)\s+poison", text)
    if poison:
        poison *= _int(r"poison\s+to\s+a\s+random\s+enemy\s+(\d+)\s+times", text) or 1
    poison_needs = bool(poison and re.search(r"if\s+the\s+enemy\s+has\s+poison", text, re.I))
    doom = _int(r"apply\s+(\d+)\s+doom", text)
    if not doom and re.search(r"apply\s+doom\s+equal\s+to\s+damage\s+dealt", text, re.I):
        doom = _int(r"deals?\s+(\d+)\s+damage", text)  # Blight Strike: as much Doom as it hits for
    shivs = _int(r"add\s+(\d+)\s+(?:\w+\s+)?shivs?\s+into\s+your\s+hand", text)  # ("2 Inky Shivs" too: Blade of Ink)
    heal = _int(r"\bheal\s+(\d+)\s+hp", text)
    m = re.search(r"gain\s+(\d+)?\s*((?:\[star_icon\.png\]\s*)+)", text, re.I)
    star_gain = (int(m.group(1)) if m.group(1) else len(re.findall(r"star_icon", m.group(2)))) if m else 0
    forge = _int(r"forge\s+(\d+)", text)
    summon = _int(r"summon\s+(\d+)", text)
    channel = tuple(
        orb.upper()
        for count, orb in re.findall(r"channel\s+(\d+)\s+(lightning|frost|dark|plasma|glass)", text, re.I)
        for _ in range(int(count))
    )
    if not channel and (m := re.search(r"channel\s+X\s+(lightning|frost|dark|plasma|glass)", text, re.I)):
        channel = (m.group(1).upper(),)  # Tempest: one per energy spent
    evoke = 0
    if re.search(r"\bevoke\s+your", text, re.I):  # "Evoke your rightmost Orb twice." / "... 4 times."
        evoke = 2 if re.search(r"evoke[^.]*twice", text, re.I) else _int(r"evoke[^.]*?(\d+)\s+times", text) or 1
    m = re.search(r"\bevoke\s+all\s+(?:of\s+)?your\s+orbs(\s+twice)?", text, re.I)
    evoke_all = (2 if m.group(1) else 1) if m else 0
    next_energy = _icons(next_text)
    next_draw = _int(r"draw\s+(\d+)\s+cards?", next_text)
    next_block = _int(r"gain\s+(\d+)\s+block", next_text)
    enemy_hp_loss = _int(r"enemy\s+loses\s+(\d+)\s+hp", text)
    souls = _int(r"add\s+(\d+)\s+souls?", text) or (1 if re.search(r"add\s+a\s+soul", text, re.I) else 0)

    aoe = target == "AllEnemies" or bool(re.search(r"all\s+enemies", text, re.I))
    random_target = bool(re.search(r"random\s+enemy", text, re.I))
    aim, stretch = _int(r"gain\s+(\d+)\s+aim", text), _int(r"gain\s+(\d+)\s+stretch", text)
    known = any(
        (damage, block, vulnerable, weak, draw, energy_gain, strength, enemy_strength_loss, poison, doom, shivs, aim, stretch, melody_block, impaled_only)
        + (star_gain, forge, summon, dexterity, len(channel), evoke, evoke_all, next_energy, next_draw, next_block, enemy_hp_loss, souls, heal)
    )

    can_play = bool(card.get("can_play", True))
    return CardInfo(
        index=int(card.get("index", 0)), name=str(card.get("name") or card.get("id") or "?"), type=ctype, cost=_cost(card.get("cost")),
        star_cost=_cost(card.get("star_cost")) if card.get("star_cost") not in (None, "") else 0, key=_card_key(card),
        damage=damage, hits=hits, double_if_vulnerable=double_if_vulnerable, aoe=aoe, random_target=random_target, block=block,
        vulnerable=vulnerable, weak=weak, draw=draw, energy_gain=energy_gain, hp_loss=hp_loss, known=known, strength=strength,
        strength_lasts=strength_lasts, enemy_strength_loss=enemy_strength_loss, enemy_strength_lasts=enemy_strength_lasts, poison=poison, poison_needs=poison_needs, doom=doom, shivs=shivs, star_gain=star_gain,
        forge=forge, summon=summon, dexterity=dexterity, dexterity_lasts=dexterity_lasts, channel=channel, evoke=evoke, evoke_all=evoke_all,
        next_energy=next_energy, next_draw=next_draw, next_block=next_block, enemy_hp_loss=enemy_hp_loss, souls=souls, heal=heal,
        needs_target=target == "AnyEnemy", playable=can_play,
        deadly=bool(re.search(r"\bdie\b", full_text, re.I)),
        block_lock=bool(re.search(r"(cannot|can't|can not)\s+gain\s+(any\s+)?block", full_text, re.I)),
        osty_attack=bool(re.search(r"\bOsty\b[^.]*\bdeals\b|if\s+Osty\s+is\s+alive", full_text, re.I)),
        # (Or a keyword: the Spectral Knight's Hex gives every card "Hexed": "Add Ethereal to this card.")
        ethereal=bool(re.search(r"\bEthereal\b", full_text)) or any(isinstance(k, dict) and str(k.get("name")).lower() in ("ethereal", "hexed") for k in card.get("keywords") or []),
        ends_turn=bool(re.search(r"(?:^|\.\s*)(?:end\s+your\s+turn|exhaust\s+your\s+hand)\.", full_text, re.I)),
        replay=_replay(card, full_text),
        play_limit=_int(r"you\s+can\s+only\s+play\s+(\d+)\s+more\s+cards?\s+this\s+turn", full_text) if re.search(r"only\s+play\s+\d+\s+more\s+card", full_text, re.I) else -1,
        shoots=bool(re.search(r"\bshoot\s+(?:an?|\d+|x)\s+arrows?\b", text, re.I)), aim_times=aim_times, aim=aim, stretch=stretch,
        impaled_only=impaled_only, on_impaled=on_impaled if any(on_impaled) else (),
        splash=bool(re.search(r"damage\s+all\s+other\s+enemies\s+equal\s+to\s+the\s+damage\s+dealt", text, re.I)),
        stretch_dealt=bool(re.search(r"gain\s+stretch\s+equal\s+to\s+the\s+damage\s+dealt", text, re.I)),
        dislodge=bool(re.search(r"\bdislodge\b", text, re.I)), impaled_double=bool(re.search(r"double\s+damage\s+to\s+impaled", text, re.I)),
        arrow_block=bool(re.search(r"if\s+you\s+have\s+an\s+arrow,\s*gain\s+block\s+an\s+additional\s+time", full_text, re.I)),  # ("If you ..." is not in `text`)
        melody=melodies(card.get("name") or ""), melody_block=melody_block, keeps_hand=_keeps_hand(full_text),
        melody_hits=bool(re.search(r"additional\s+time\s+for\s+each\s+melody", text, re.I)),
        melody_twice=tuple(w.upper() for w in re.findall(r"(\w+)\s+melody", (re.search(r"if\s+you\s+have\s+an?\s+[^,.]*melody[^,.]*,\s*hits\s+twice", full_text, re.I) or re.match("", "")).group(0), re.I)),
        stance=(m.group(1).upper() if (m := re.search(r"\benter\s+(wrath|calm|divinity)\b", text, re.I)) else "EXIT" if re.search(r"\bexit\s+your\s+stance\b", text, re.I) else ""),
        verse=int(m.group(1) or 1) if (m := re.search(r"the\s+next\s+(?:(\d+)\s+)?\W*wah\W+cho\W+or\s+\W*ki\W*\s+cards?\s+you\s+play\s+this\s+turn\s+(?:is|are)\s+played\s+an\s+additional\s+time", full_text, re.I)) else 0,
        osty_dies=bool(re.search(r"\b(?:Osty|he)\s+dies", full_text, re.I)),  # (Sacrifice: "If Osty is alive, he dies and ...")
        doom_kills=bool(re.search(r"kill\s+enemies\s+with\s+at\s+least\s+as\s+much\s+Doom", full_text, re.I)),
        debris=-1 if re.search(r"fill\s+your\s+hand\s+with\s+debris", text, re.I) else _int(r"add\s+(\d+)\s+debris", text) or int(bool(re.search(r"add\s+a\s+debris", text, re.I))),
        bound=any(str(k.get("name")).lower() == "bound" for k in card.get("keywords") or [] if isinstance(k, dict)),
        short=not can_play
        and bool(card.get("unplayable_reason"))
        and all(r.strip() in ("EnergyCostTooHigh", "StarCostTooHigh") for r in str(card.get("unplayable_reason")).split(",")),
    )


# A nudge for cards whose real strength the text does not show (or
# overstates). The text-based score below still does most of the work, so
# cards that are not listed are judged on what they say. Values are in the
# same units as deck_value: +1 is roughly one rarity step. (Only cards _TIER
# does not rate: a card it rates never gets as far as the nudge.)
_KNOWN = {
    "IMMOLATE": 3.0, "DISARM": 2.0, "REAPER": 2.0, "CARNAGE": 1.5, "SPOT_WEAKNESS": 1.5, "METALLICIZE": 1.5,
    "CLEAVE": 1.0, "POWER_THROUGH": 1.0, "GHOSTLY_ARMOR": 1.0, "SEEING_RED": 1.0, "INTIMIDATE": 1.0, "COMBUST": 0.5,
    "DOUBLE_TAP": 0.5, "ENTRENCH": -1.5, "CLASH": -1.0, "SEARING_BLOW": -1.0, "RECKLESS_CHARGE": -1.0,
    "FIRE_BREATHING": -1.0, "EVOLVE": -1.0, "BERSERK": -0.5, "WARCRY": -0.5, "WILD_STRIKE": -0.5, "DUAL_WIELD": -0.5,
    "LIMIT_BREAK": -0.5,
}


def _known_nudge(card: dict) -> float:
    if not games.uses_sts2_tables():
        return 0.0
    return next((_KNOWN[k] for k in (key_of(card.get("id")), key_of(card.get("name"))) if k in _KNOWN), 0.0)


# (2026-11-18: cards that scale over a fight were raised by 0.5 to 1.5. Damage per turn against
# bosses was the same 30 for every deck the bot built, and community rankings put these first.)
# How much each card we have met is worth in the deck, on the deck_value
# scale (about 3 = fine early on, 5 = good, 7+ = take it every time). Judged
# by hand from what the cards do and how the bot actually plays them: it
# plans one turn at a time, so cards that need a deck built around them rate
# low here even where a human would make them work. Cards that are not
# listed fall back to the text-based estimate in deck_value.
_TIER = {
    # commons
    "SHRUG_IT_OFF": 5.5, "POMMEL_STRIKE": 5.0, "BREAKTHROUGH": 5.0, "THUNDERCLAP": 4.5, "TWIN_STRIKE": 4.0,
    "ARMAMENTS": 3.0, "BLOOD_WALL": 4.0, "HEADBUTT": 3.5, "IRON_WAVE": 3.5, "CINDER": 4.0, "TRUE_GRIT": 3.5,
    "TAUNT": 3.5, "ANGER": 3.0, "SETUP_STRIKE": 3.0, "SWORD_BOOMERANG": 3.0, "TREMBLE": 2.5,
    "PERFECTED_STRIKE": 2.5, "BODY_SLAM": 1.5, "HAVOC": 1.0,
    # uncommons
    "SHOCKWAVE": 7.5, "UPPERCUT": 6.0, "FLAME_BARRIER": 6.0, "INFLAME": 6.5, "HEMOKINESIS": 6.0,
    "BATTLE_TRANCE": 5.5, "WHIRLWIND": 5.0, "ULTIMATE_STRIKE": 5.0, "ULTIMATE_DEFEND": 5.0,
    "THRUMMING_HATCHET": 5.0, "STONE_ARMOR": 5.0, "DRAMATIC_ENTRANCE": 5.0, "THE_BOMB": 5.0, "BLUDGEON": 5.5,
    "FIGHT_ME": 4.5, "OMNISLICE": 4.5, "DISMANTLE": 4.0, "RAMPAGE": 4.0, "FLASH_OF_STEEL": 4.0, "FINESSE": 4.0,
    "SALVO": 4.0, "SEEKER_STRIKE": 4.0, "SPITE": 3.5, "UNRELENTING": 3.5, "EVIL_EYE": 3.5, "BURNING_PACT": 3.5,
    "EQUILIBRIUM": 3.5, "DARK_SHACKLES": 3.5, "STOMP": 2.5, "HOWL_FROM_BEYOND": 3.5, "FISTICUFFS": 3.5,
    "VOLLEY": 3.5, "BULLY": 3.0, "COLOSSUS": 3.0, "FASTEN": 3.0, "BLOODLETTING": 3.0, "PRODUCTION": 3.0,
    "PILLAGE": 3.0, "VICIOUS": 2.5, "PANACHE": 2.5, "RAGE": 2.5, "INFERNAL_BLADE": 2.5, "FORGOTTEN_RITUAL": 2.5,
    "DISCOVERY": 2.5, "THINKING_AHEAD": 2.5, "PANIC_BUTTON": 2.5, "ASHEN_STRIKE": 2.5, "MIND_BLAST": 2.5,
    "SECOND_WIND": 2.5, "FEEL_NO_PAIN": 2.0, "JUGGLING": 2.0, "STRATAGEM": 2.0, "STAMPEDE": 2.0,
    "EXPECT_A_FIGHT": 2.0, "JACK_OF_ALL_TRADES": 2.0, "IMPATIENCE": 2.0, "RUPTURE": 1.5, "RESTLESSNESS": 1.5,
    "PURITY": 1.5, "PROLONG": 1.0,
    # rares and event cards
    "IMPERVIOUS": 8.0, "OFFERING": 7.5, "DEMON_FORM": 8.0, "PYRE": 7.0, "EXTERMINATE": 7.0, "FEED": 6.5,
    "CRIMSON_MANTLE": 6.0, "ROLLING_BOULDER": 6.0, "GIANT_ROCK": 6.0, "CONFLAGRATION": 5.5, "HAND_OF_GREED": 5.5,
    "MASTER_OF_STRATEGY": 5.0, "MANGLE": 4.5, "AGGRESSION": 4.5, "MAYHEM": 4.5, "NOT_YET": 4.5, "BOLAS": 4.5,
    "UNMOVABLE": 4.0, "BRAND": 4.0, "FIEND_FIRE": 4.0, "JUGGERNAUT": 3.5, "HELLRAISER": 3.5, "ONE_TWO_PUNCH": 3.5,
    "THRASH": 3.5, "BARRICADE": 3.0, "DARK_EMBRACE": 3.0, "CASCADE": 3.0, "SECRET_WEAPON": 3.0,
    "SECRET_TECHNIQUE": 3.0, "HIDDEN_GEM": 2.5, "GOLD_AXE": 2.5, "METAMORPHOSIS": 2.5, "PRIMAL_FORCE": 2.0,
    "STOKE": 2.0, "THE_GAMBIT": -6.0,
    # --- The Silent
    "ADRENALINE": 7.0, "NOXIOUS_FUMES": 7.5, "FOOTWORK": 6.0, "AFTERIMAGE": 6.0, "LEG_SWEEP": 6.0, "BACKFLIP": 5.5,
    "DASH": 5.5, "SERPENT_FORM": 5.5, "ASSASSINATE": 5.5, "DEADLY_POISON": 5.5, "BOUNCING_FLASK": 5.5,
    "ACROBATICS": 5.0, "DAGGER_SPRAY": 5.0, "BLADE_DANCE": 5.5, "PIERCING_WAIL": 5.0, "MALAISE": 5.0,
    "ECHOING_SLASH": 5.0, "NEUTRALIZE": 4.0, "SURVIVOR": 3.5, "POISONED_STAB": 4.5, "HAZE": 4.5,
    "DAGGER_THROW": 4.5, "SUCKER_PUNCH": 4.5, "CLOAK_AND_DAGGER": 4.5, "DODGE_AND_ROLL": 4.5, "PREDATOR": 4.5,
    "BACKSTAB": 4.5, "BLUR": 4.5, "SKEWER": 4.5, "FLICK_FLACK": 4.5, "WELL_LAID_PLANS": 4.5, "FAN_OF_KNIVES": 4.5,
    "ACCURACY": 5.5, "INFINITE_BLADES": 5.5, "TOOLS_OF_THE_TRADE": 4.0, "ENVENOM": 4.0, "ABRASIVE": 4.0,
    "SNAKEBITE": 4.0, "OUTBREAK": 4.0, "DEFLECT": 4.0, "THE_HUNT": 4.0, "POUNCE": 4.0, "UP_MY_SLEEVE": 4.0,
    "EXPOSE": 4.0, "SPEEDSTER": 3.5, "BUBBLE_BUBBLE": 3.5, "ESCAPE_PLAN": 3.5, "EXPERTISE": 3.5, "BURST": 3.5,
    "PRECISE_CUT": 3.5, "STRANGLE": 3.5, "RICOCHET": 3.5, "LEADING_STRIKE": 3.5, "ACCELERANT": 3.0,
    "TRACKING": 3.0, "SLICE": 3.0, "FINISHER": 3.0, "BULLET_TIME": 3.0, "STORM_OF_STEEL": 3.0, "MEMENTO_MORI": 3.0,
    "PINPOINT": 3.0, "UNTOUCHABLE": 3.0, "SHADOWMELD": 3.0, "CORROSIVE_WAVE": 3.0, "ANTICIPATE": 3.0,
    "PREPARED": 2.5, "CALCULATED_GAMBLE": 2.5, "FLECHETTES": 2.5, "NIGHTMARE": 2.5, "MURDER": 2.5, "MIRAGE": 2.5,
    "SIDESTEP": 2.5, "TACTICIAN": 2.0, "SHADOW_STEP": 2.0, "GRAND_FINALE": 1.0,
    # --- The Regent
    "COMET": 6.0, "GAMMA_BLAST": 5.5, "DYING_STAR": 5.5, "ASTRAL_PULSE": 5.0, "CRUSH_UNDER": 5.0,
    "GATHER_LIGHT": 5.0, "PROPHESIZE": 5.0, "PARTICLE_WALL": 5.0, "FALLING_STAR": 4.5, "VENERATE": 3.5,
    "SOLAR_STRIKE": 4.5, "CELESTIAL_MIGHT": 4.5, "PHOTON_CUT": 4.5, "HEGEMONY": 4.5, "HEIRLOOM_HAMMER": 4.5,
    "GLOW": 4.5, "CLOAK_OF_STARS": 4.5, "SPOILS_OF_BATTLE": 4.5, "GLITTERSTREAM": 4.5, "GLIMMER": 4.5,
    "SEEKING_EDGE": 4.5, "WROUGHT_IN_WAR": 4.0, "KNOCKOUT_BLOW": 4.0, "KINGLY_PUNCH": 4.0, "SHINING_STRIKE": 4.0,
    "BOMBARDMENT": 4.0, "HIDDEN_CACHE": 4.0, "KNOW_THY_PLACE": 4.0, "REFINE_BLADE": 4.0, "CHILD_OF_THE_STARS": 4.0,
    "PARRY": 4.0, "KINGLY_KICK": 3.5, "STARDUST": 3.5, "MAKE_IT_SO": 3.5, "BEAT_INTO_SHAPE": 3.5,
    "COSMIC_INDIFFERENCE": 3.5, "TERRAFORMING": 3.5, "SUMMON_FORTH": 3.5, "DECISIONS_DECISIONS": 3.5,
    "TYRANNY": 3.5, "ORBIT": 3.5, "MONOLOGUE": 3.0, "ROYAL_GAMBLE": 3.0, "ROYALTIES": 3.0,
    # (Crash Landing fills the hand with Debris: an AoE for ordinary fights and a finisher, a liability in
    # a boss fight. Regent bosses with it in the deck: 1 won in 6, against 22 in 34 without.)
    "CRASH_LANDING": 4.5, "SEVEN_STARS": 5.5,
    "PILLAR_OF_CREATION": 3.0, "SPECTRUM_SHIFT": 3.0, "PALE_BLUE_DOT": 3.0,
    # --- The Necrobinder
    # (Reset on 2026-11-18 from 67 Necrobinder runs, floors gained per pick: Eradicate +7.9, End of Days +6.4,
    # Misery +5.5, Necro Mastery +4.5, Severance +3.7, Deathbringer +3.5, Flatten +2.9, Defile +2.6, Poke +2.5;
    # Reap -4.1, Snap -3.9, Parse -2.7, Bury -1.2, Negative Pulse -0.2 over 44 picks, High Five 0 over 24.)
    "SOW": 5.0, "DEBILITATE": 5.0, "GRAVE_WARDEN": 6.0, "PUTREFY": 5.0, "DRAIN_POWER": 4.5, "DEFILE": 5.0,
    "BLIGHT_STRIKE": 4.5, "REAVE": 4.5, "BURY": 3.5, "DEFY": 4.5, "NEGATIVE_PULSE": 4.0, "PARSE": 3.5,
    "END_OF_DAYS": 5.5, "BODYGUARD": 3.5, "UNLEASH": 3.5, "FEAR": 4.0, "REAP": 3.0, "FLATTEN": 4.5,
    "PULL_AGGRO": 3.5, "UNDEATH": 4.0, "ENFEEBLING_TOUCH": 4.0, "CAPTURE_SPIRIT": 5.0, "DEATHS_DOOR": 4.0,
    "NO_ESCAPE": 4.0, "REANIMATE": 4.0, "SNAP": 3.0, "POKE": 4.0, "RATTLE": 3.5, "HANG": 3.5, "INVOKE": 3.5,
    "WISP": 3.5, "DIRGE": 5.0, "LETHALITY": 3.5, "SHROUD": 3.5, "HAUNT": 3.5, "CALL_OF_THE_VOID": 3.5,
    "ERADICATE": 5.5, "MISERY": 5.0, "NECRO_MASTERY": 5.5, "DEATHBRINGER": 5.5, "SEVERANCE": 4.5,
    "SLEIGHT_OF_FLESH": 5.0, "HIGH_FIVE": 4.5,
    "SIC_EM": 3.5, "GRAVEBLAST": 3.0, "DEATH_MARCH": 3.0, "PULL_FROM_BELOW": 2.5, "BORROWED_TIME": 2.5,
    "SEANCE": 2.5,
    # --- The Defect
    "COOLHEADED": 5.5, "COLD_SNAP": 5.0, "BALL_LIGHTNING": 5.0, "SWEEPING_BEAM": 5.0, "BOOT_SEQUENCE": 5.0,
    "SHADOW_SHIELD": 5.0, "ZAP": 4.0, "DUALCAST": 3.5, "SUNDER": 4.5, "CHARGE_BATTERY": 4.5,
    "LIGHTNING_ROD": 4.5, "FUSION": 4.5, "GLASSWORK": 4.5, "HAILSTORM": 4.5, "BULK_UP": 4.5, "CAPACITOR": 5.5,
    "CREATIVE_AI": 4.5, "ICE_LANCE": 4.5, "FIGHT_THROUGH": 4.0, "GO_FOR_THE_EYES": 4.0, "FOCUSED_STRIKE": 4.0,
    "BEAM_CELL": 4.0, "MOMENTUM_STRIKE": 4.0, "COMPILE_DRIVER": 4.0, "FTL": 4.0, "BOOST_AWAY": 4.0,
    "DARKNESS": 4.0, "MODDED": 4.0, "GUNK_UP": 3.5, "UPROAR": 3.5, "COLLISION_COURSE": 3.5, "TESLA_COIL": 3.5,
    "METEOR_STRIKE": 3.5, "HOLOGRAM": 3.5, "OVERCLOCK": 3.5, "COMPACT": 3.5, "WHITE_NOISE": 3.5,
    "DOUBLE_ENERGY": 3.5, "STORM": 3.5, "THUNDER": 3.5, "SCRAPE": 3.0, "ALL_FOR_ONE": 3.0, "HOTFIX": 3.0,
    "VOLTAIC": 3.0, "FERAL": 3.0, "TRASH_TO_TREASURE": 3.0, "ITERATION": 2.5,
    # --- met on 2026-11-18 (all characters)
    # (Friendship, energy every turn for 2 Strength, was 6.0 until 2026-11-20: 48 picks ending 1.4 floors short of
    # the Necrobinder's average pick, like every other way to more energy this bot has been given.)
    # (Six Necrobinder cards stood here a second time with their old ratings until 2026-11-19, and the
    # later entry of a dict wins: the ratings reset above from 67 runs had never applied.)
    "MACHINE_LEARNING": 6.5, "DEFRAGMENT": 7.5, "GLACIER": 6.0, "FRIENDSHIP": 4.5, "GENESIS": 5.5, "ECHO_FORM": 6.5,
    "HYPERBEAM": 5.5, "DEVASTATE": 5.0, "SKIM": 5.0, "COOLANT": 5.0, "BUFFER": 5.0, "SPINNER": 5.0, "QUADCAST": 5.0,
    "GUIDING_STAR": 4.5, "ADAPTIVE_STRIKE": 4.5, "BONE_SHARDS": 4.5,
    "NULL": 4.5, "LOOP": 4.5, "SENTRY_MODE": 4.5, "CHILL": 4.0,
    "TEMPEST": 4.0, "NEUROSURGE": 1.0, "BLACK_HOLE": 4.0, "LEAP": 3.5,
    "DELAY": 3.5, "CHAOS": 3.5, "SOUL_STORM": 3.5, "REFRACT": 3.5, "FETCH": 3.5, "REBOOT": 3.0, "ALIGNMENT": 3.0,
    "CRESCENT_SPEAR": 3.0, "CONQUEROR": 3.0, "SYNTHESIS": 3.0, "RIGHT_HAND_HAND": 3.0, "SPUR": 3.0, "CLEANSE": 3.0,
    "TIMES_UP": 3.0, "SHARED_FATE": 3.0, "DANSE_MACABRE": 3.0, "CLAW": 2.5, "MELANCHOLY": 2.5, "SUBROUTINE": 2.0,
    "PAGESTORM": 2.0, "SYNCHRONIZE": 2.0, "CHARGE": 1.5, "GUARDS": 1.0,
}


_AFFLICTION = {"SLOTH": -6.5, "MIND_ROT": -7.5, "WASTE_AWAY": -8.5, "DISINTEGRATION": -9.5}


def _card_key(card: dict) -> str:
    return key_of(card.get("id") or card.get("name"))


def is_known(card: dict) -> bool:
    """True if the card is one we have rated by hand."""
    return games.uses_sts2_tables() and _card_key(card) in _TIER


_RARITY = {"Basic": 0.0, "Common": 1.0, "Uncommon": 2.2, "Rare": 3.5, "Ancient": 4.5, "Event": 2.0}
# What the crowd's numbers (crowd.py) make a card of each rarity worth on average (433 rated cards, sd 0.8-1.0): what a card
# with no numbers of its own (every Fool card, on the beta) is mixed with, so that it is rated on the same scale as the rest.
_RARITY_CROWD = {"Common": 3.69, "Uncommon": 4.85, "Rare": 6.26}
IMPALED_SHARE = 0.5  # of what a card that needs an Impaled enemy does, counted at rewards (see deck_value)
CONDITIONAL_SHARE = 0.3  # of what a card that "can only be played if" something holds does, counted at rewards

# Cards the crowd's numbers (crowd.py) are not asked about: what makes them good in other
# hands is what this bot cannot do. The Gambit can end the run.
# (Neurosurge stood here while triggers.shunned kept it out of every boss and elite fight.)
# Stoke Exhausts what is left of the hand (the planner would play it last, on the cards it could not pay
# for); GUARDS!!! turns the hand into Minion Sacrifices; Banshee's Cry (9 energy, 2 less for each Ethereal card
# played) and Knife Trap (every Shiv in the Exhaust Pile) are only cards in decks built for them.
_OWN_VIEW = {"THE_GAMBIT", "STOKE", "GUARDS", "BANSHEES_CRY", "BANSHEE_S_CRY", "KNIFE_TRAP"}


def _crowd_share() -> float:
    from .learn import knob  # (learn.py reads this module for its card keys)

    return knob("crowd")


def deck_value(card: dict) -> float:
    """How much we want this card in the deck (higher is better).

    Used for card rewards, shop buys, and upgrade / removal picks.
    """
    ctype = str(card.get("type") or "")
    if ctype == "Curse":
        return -10.0
    if ctype == "Status":
        # Some bosses make us pick one of these to keep for the fight; take the
        # one that hurts the least. Disintegration (damage every turn, more
        # with every copy) lost several boss fights on its own, while a limit
        # of three cards a turn rarely matters to how the bot plays.
        return _AFFLICTION.get(_card_key(card), -8.0)
    key = _card_key(card)
    upgraded = 0.7 if card.get("is_upgraded") else 0.0
    # Our own rating and the crowd's, mixed (a setting under test, see learn.CHOICES): where
    # ours put the plain attacks first, half a million runs a character put them last.
    crowd = None if key in _OWN_VIEW else crowd_rating(key)
    share = _crowd_share() if crowd is not None else 0.0
    tier = _TIER.get(key) if games.uses_sts2_tables() else None
    if tier is not None:
        return (1.0 - share) * tier + share * (crowd or 0.0) + upgraded
    info = parse(card)
    if crowd is None and key not in _OWN_VIEW and (info.known or ctype == "Power") and (crowd := _RARITY_CROWD.get(str(card.get("rarity") or card.get("card_rarity") or ""))):
        share = _crowd_share()  # (the Fool's cards: on the bare text rating they skipped 29% of rewards, the others 0-11%)
    if info.deadly:
        return -6.0  # e.g. "If you take unblocked attack damage this combat, die."
    cost = max(1, info.cost) if not info.is_x else 2
    score = base = _RARITY.get(str(card.get("rarity") or card.get("card_rarity") or ""), 1.0)

    dmg = info.total_damage * (1.6 if info.aoe else 1.0)
    # (Capped: uncapped, Crash Landing came to 15.9 and Seven Stars to 12.1, above every card rated by hand.)
    score += min(4.0, max(0.0, dmg / cost - 6.0) * 0.45)
    score += max(0.0, info.block / cost - 5.0) * 0.45
    score += min(info.strength, 4) * (1.0 if info.strength_lasts else 0.4)
    score += info.draw * 0.8 + info.energy_gain * 1.2
    score += info.poison * (0.15 if info.poison_needs else 0.3) + info.doom * 0.12 + info.shivs * 0.7 + info.star_gain * 0.6 + info.forge * 0.12
    score += info.summon * 0.15 + len(info.channel) * 1.3 + info.evoke * 0.8 + info.souls * 0.6
    score += info.dexterity * (1.5 if info.dexterity_lasts else 0.4)
    # (Energy next turn: a hand spends about 2 more, whatever comes. The Fool's turn after Ki Whip spent 1.6 of its 2, after
    # Ki Whip+ 2.0 of 3, Ki Rest 2.0 of 5, Ki Rest+ 2.4 of 6; at 0.8 each Ki Rest topped offers at 6.6. See battle.next_room.)
    score += min(info.next_energy, 2) * 0.8 + max(0, info.next_energy - 2) * 0.2 + info.next_draw * 0.5 + info.next_block * 0.08 + info.enemy_hp_loss * 0.15
    score += min(info.vulnerable, 3) * 0.6 + min(info.weak, 3) * 0.4
    if info.impaled_only:  # only with our Arrow in the enemy: 39-55% of the turns Nail, Uproot, Recall and Wrest were drawn
        score = base + (score - base) * IMPALED_SHARE
    if re.search(r"can\s+only\s+be\s+played\s+if", str(card.get("description") or ""), re.I):
        # Clash ("if every card in your hand is an Attack") read as 14 free damage and topped the first STS1 rewards at
        # 9.7; held beside Defends it is dead most turns (Grand Finale, Signature Move: the same).
        score = base + (score - base) * CONDITIONAL_SHARE
    if info.cost == 0 and info.known:
        score += 0.5
    if ctype == "Power":
        # Powers we cannot read are a gamble; they also cost a turn of tempo.
        score += 1.5 if info.known else -0.5
    elif not info.known:
        score -= 1.0  # the planner cannot use what it cannot read
    if info.cost >= 3:
        score -= 0.8
    score -= 0.9 * (2 if info.star_cost < 0 else info.star_cost)  # Stars are a second price
    if info.block_lock:
        score -= 3.0
    score -= info.hp_loss * 0.1
    if is_basic(card):
        score -= 2.5
    score += _known_nudge(card)
    value = (1.0 - share) * score + share * (crowd or 0.0) + upgraded
    # Encore (rated 7.4): "You can only play 1 more card this turn", at best a cantrip played last.
    return min(value, 2.0 + upgraded) if info.play_limit >= 0 else value


def upgrade_gain(base: dict, upgraded: dict) -> float:
    """What upgrading a card adds, in the planner's points per play.

    Read from the two versions of the card's text. Upgrading by rating alone
    spent upgrades on good cards whose upgrade is small (Coolheaded: one more
    card drawn) while Devastate (+12 damage) and cost reductions waited.
    """
    a, b = parse({**base, "target_type": "AnyEnemy"}), parse({**upgraded, "target_type": "AnyEnemy"})

    def worth(i: CardInfo) -> float:
        channels = len(i.channel) if i.type != "Power" else 0
        return (
            i.total_damage * (1.6 if i.aoe else 1.0) + i.block + i.draw * 3.0 + i.energy_gain * 5.0
            + min(i.vulnerable, 4) * 2.5 + min(i.weak, 4) * 2.0
            + i.strength * (4.0 if i.strength_lasts else 1.5) + i.dexterity * (4.0 if i.dexterity_lasts else 1.5)
            + i.poison * (1.0 if i.poison_needs else 2.0) + i.doom * 0.6 + i.shivs * 4.0 + i.star_gain * 2.5 + i.forge * 0.6 + i.summon
            + channels * 4.0 + i.evoke * 3.0 + i.enemy_strength_loss * 1.5
            + i.next_energy * 4.0 + i.next_draw * 1.5 + i.next_block * 0.7 + i.enemy_hp_loss + i.souls * 3.0
        )

    cost = lambda i: 3 if i.is_x else i.cost
    gain = worth(b) - worth(a) + 6.0 * (cost(a) - cost(b)) + 2.5 * (max(a.star_cost, 0) - max(b.star_cost, 0))
    if gain <= 0.0:
        # Nothing we can read changed (a Power's numbers, an added keyword): still worth something.
        gain = 3.0 if str(base.get("description")) != str(upgraded.get("description")) else 0.5
    return gain


def order_key(c: CardInfo) -> tuple:
    if c.ends_turn:
        rank = 9  # whatever else the turn can do comes before the card that ends it
    elif c.type == "Power" or c.key == "FRANTIC_ESCAPE" or c.verse:
        rank = 0  # set-up first; an escape must not be left for energy that is gone; Ki Verse before the cards it plays twice
    elif c.draw or c.energy_gain or c.star_gain:
        rank = 1  # what later cards are paid with comes first
    elif c.vulnerable or (c.summon and not c.is_x) or (c.doom and not c.damage):
        rank = 2  # (a Summon before Osty's attacks: Unleash hits for his HP; Doom before what counts it)
    elif c.damage and not c.is_x:
        rank = 3
    elif c.is_x:
        rank = 4
    elif c.block:
        rank = 5
    else:
        rank = 6
    return (rank, -c.total_damage, c.index)
