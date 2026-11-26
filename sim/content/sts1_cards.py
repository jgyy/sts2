"""The first game's cards: what the text tables (sts2bot.games.sts1.text) leave out, and what their text cannot say.

META: type, cost, rarity and whether the card is aimed ("t"), one card a line. Then the handlers effects.py asks
for (see its docstring), for the cards whose text says more than the planner's parser reads, or says it under a
condition.
"""

from __future__ import annotations

from dataclasses import replace

from .. import effects as fx
from ..effects import add
from . import common

# key  type cost rarity [t]   (type: A S P St Cu; cost: number, X, U = unplayable; rarity: B C U R S = basic, common, uncommon, rare, special)
META = """
STRIKE_R A1Bt DEFEND_R S1B BASH A2Bt
ANGER A0Ct ARMAMENTS S1C BODY_SLAM A1Ct CLASH A0Ct CLEAVE A1C CLOTHESLINE A2Ct FLEX S0C HAVOC S1C HEADBUTT A1Ct HEAVY_BLADE A2Ct
IRON_WAVE A1Ct PERFECTED_STRIKE A2Ct POMMEL_STRIKE A1Ct SHRUG_IT_OFF S1C SWORD_BOOMERANG A1C THUNDERCLAP A1C TRUE_GRIT S1C TWIN_STRIKE A1Ct
WARCRY S0C WILD_STRIKE A1Ct
BATTLE_TRANCE S0U BLOOD_FOR_BLOOD A4Ut BLOODLETTING S0U BURNING_PACT S1U CARNAGE A2Ut COMBUST P1U DARK_EMBRACE P2U DISARM S1Ut DROPKICK A1Ut
DUAL_WIELD S1U ENTRENCH S2U EVOLVE P1U FEEL_NO_PAIN P1U FIRE_BREATHING P1U FLAME_BARRIER S2U GHOSTLY_ARMOR S1U HEMOKINESIS A1Ut
INFERNAL_BLADE S1U INFLAME P1U INTIMIDATE S0U METALLICIZE P1U POWER_THROUGH S1U PUMMEL A1Ut RAGE S0U RAMPAGE A1Ut RECKLESS_CHARGE A0Ut
RUPTURE P1U SEARING_BLOW A2Ut SECOND_WIND S1U SEEING_RED S1U SENTINEL S1U SEVER_SOUL A2Ut SHOCKWAVE S2U SPOT_WEAKNESS S1Ut UPPERCUT A2Ut
WHIRLWIND AXU
BARRICADE P3R BERSERK P0R BLUDGEON A3Rt BRUTALITY P0R CORRUPTION P3R DEMON_FORM P3R DOUBLE_TAP S1R EXHUME S1R FEED A1Rt FIEND_FIRE A2Rt
IMMOLATE A2R IMPERVIOUS S2R JUGGERNAUT P2R LIMIT_BREAK S1R OFFERING S0R REAPER A2R
STRIKE_G A1Bt DEFEND_G S1B NEUTRALIZE A0Bt SURVIVOR S1B
ACROBATICS S1C BACKFLIP S1C BANE A1Ct BLADE_DANCE S1C CLOAK_AND_DAGGER S1C DAGGER_SPRAY A1C DAGGER_THROW A1Ct DEADLY_POISON S1Ct
DEFLECT S0C DODGE_AND_ROLL S1C FLYING_KNEE A1Ct OUTMANEUVER S1C PIERCING_WAIL S1C POISONED_STAB A1Ct PREPARED S0C QUICK_SLASH A1Ct
SLICE A0Ct SNEAKY_STRIKE A2Ct SUCKER_PUNCH A1Ct
ACCURACY P1U ALL_OUT_ATTACK A1U BACKSTAB A0Ut BLUR S1U BOUNCING_FLASK S2U CALCULATED_GAMBLE S0U CALTROPS P1U CATALYST S1Ut CHOKE A2Ut
CONCENTRATE S0U CRIPPLING_CLOUD S2U DASH A2Ut DISTRACTION S1U ENDLESS_AGONY A0Ut ESCAPE_PLAN S0U EVISCERATE A3Ut EXPERTISE S1U
FINISHER A1Ut FLECHETTES A1Ut FOOTWORK P1U HEEL_HOOK A1Ut INFINITE_BLADES P1U LEG_SWEEP S2Ut MASTERFUL_STAB A0Ut NOXIOUS_FUMES P1U
PREDATOR A2Ut REFLEX SUU RIDDLE_WITH_HOLES A2Ut SETUP S1U SKEWER AXUt TACTICIAN SUU TERROR S1Ut WELL_LAID_PLANS P1U
A_THOUSAND_CUTS P2R ADRENALINE S0R AFTER_IMAGE P1R ALCHEMIZE S1R BULLET_TIME S3R BURST S1R CORPSE_EXPLOSION S2Rt DIE_DIE_DIE A1R
DOPPELGANGER SXR ENVENOM P2R GLASS_KNIFE A1Rt GRAND_FINALE A0R MALAISE SXRt NIGHTMARE S3R PHANTASMAL_KILLER S1R STORM_OF_STEEL S1R
TOOLS_OF_THE_TRADE P1R UNLOAD A1Rt WRAITH_FORM P3R
STRIKE_B A1Bt DEFEND_B S1B ZAP S1B DUALCAST S1B
BALL_LIGHTNING A1Ct BARRAGE A1Ct BEAM_CELL A0Ct CHARGE_BATTERY S1C CLAW A0Ct COLD_SNAP A1Ct COMPILE_DRIVER A1Ct COOLHEADED S1C
GO_FOR_THE_EYES A0Ct HOLOGRAM S1C LEAP S1C REBOUND A1Ct RECURSION S1C STACK S1C STEAM_BARRIER S0C STREAMLINE A2Ct SWEEPING_BEAM A1C TURBO S0C
AGGREGATE S1U AUTO_SHIELDS S1U BLIZZARD A1U BOOT_SEQUENCE S0U BULLSEYE A1Ut CAPACITOR P1U CHAOS S1U CHILL S0U CONSUME S2U DARKNESS S1U
DEFRAGMENT P1U DOOM_AND_GLOOM A2U DOUBLE_ENERGY S1U EQUILIBRIUM S2U FORCE_FIELD S4U FTL A0Ut FUSION S2U GENETIC_ALGORITHM S1U GLACIER S2U
HEATSINKS P1U HELLO_WORLD P1U LOOP P1U MELTER A1Ut OVERCLOCK S0U RECYCLE S1U REINFORCED_BODY SXU REPROGRAM S1U RIP_AND_TEAR A1U
SCRAPE A1Ut SELF_REPAIR P1U SKIM S1U STATIC_DISCHARGE P1U STORM P1U SUNDER A3Ut TEMPEST SXU WHITE_NOISE S1U
ALL_FOR_ONE A2Rt AMPLIFY S1R BIASED_COGNITION P1R BUFFER P2R CORE_SURGE A1Rt CREATIVE_AI P3R ECHO_FORM P3R ELECTRODYNAMICS P2R FISSION S0R
HYPERBEAM A2R MACHINE_LEARNING P1R METEOR_STRIKE A5Rt MULTI_CAST SXR RAINBOW S2R REBOOT S0R SEEK S0R THUNDER_STRIKE A3R
STRIKE_P A1Bt DEFEND_P S1B ERUPTION A2Bt VIGILANCE S2B
BOWLING_BASH A1Ct CONSECRATE A0C CRESCENDO S1C CRUSH_JOINTS A1Ct CUT_THROUGH_FATE A1Ct EMPTY_BODY S1C EMPTY_FIST A1Ct EVALUATE S1C
FLURRY_OF_BLOWS A0Ct FLYING_SLEEVES A1Ct FOLLOW_UP A1Ct HALT S0C JUST_LUCKY A0Ct PRESSURE_POINTS S1Ct PROSTRATE S0C PROTECT S2C SASH_WHIP A1Ct
TRANQUILITY S1C THIRD_EYE S1C
BATTLE_HYMN P1U CARVE_REALITY A1Ut COLLECT SXU CONCLUDE A1U DECEIVE_REALITY S1U EMPTY_MIND S1U FASTING P2U FEAR_NO_EVIL A1Ut
FOREIGN_INFLUENCE S0U FORESIGHT P1U INDIGNATION S1U INNER_PEACE S1U LIKE_WATER P1U MEDITATE S1U MENTAL_FORTRESS P1U NIRVANA P1U
PERSEVERANCE S1U PRAY S1U REACH_HEAVEN A2Ut RUSHDOWN P1U SANCTITY S1U SANDS_OF_TIME A4Ut SIGNATURE_MOVE A2Ut SIMMERING_FURY S1U STUDY P2U
SWIVEL S2U TALK_TO_THE_HAND A1Ut TANTRUM A1Ut WALLOP A2Ut WAVE_OF_THE_HAND S1U WEAVE A0Ut WHEEL_KICK A2Ut WINDMILL_STRIKE A2Ut WORSHIP S2U
WREATH_OF_FLAME S1U
ALPHA S1R BLASPHEMY S1R BRILLIANCE A1Rt CONJURE_BLADE SXR DEUS_EX_MACHINA SUR DEVA_FORM P3R DEVOTION P1R ESTABLISHMENT P1R JUDGMENT S1Rt
LESSON_LEARNED A2Rt MASTER_REALITY P1R OMNISCIENCE S4R RAGNAROK A3R SCRAWL S1R SPIRIT_SHIELD S2R VAULT S3R WISH S3R
BANDAGE_UP S0U BLIND S0Ut DARK_SHACKLES S0Ut DEEP_BREATH S0U DISCOVERY S1U DRAMATIC_ENTRANCE A0U ENLIGHTENMENT S0U FINESSE S0U
FLASH_OF_STEEL A0Ut FORETHOUGHT S0U GOOD_INSTINCTS S0U IMPATIENCE S0U JACK_OF_ALL_TRADES S0U MADNESS S1U MIND_BLAST A2Ut PANACEA S0U
PANIC_BUTTON S0U PURITY S0U SWIFT_STRIKE A0Ut TRIP S0Ut
APOTHEOSIS S2R CHRYSALIS S2R HAND_OF_GREED A2Rt MAGNETISM P2R MASTER_OF_STRATEGY S0R MAYHEM P2R METAMORPHOSIS S2R PANACHE P0R
SADISTIC_NATURE P0R SECRET_TECHNIQUE S0R SECRET_WEAPON S0R THE_BOMB S2R THINKING_AHEAD S0R TRANSMUTATION SXR VIOLENCE S0R
SHIV A0St MIRACLE S0S SMITE A1St INSIGHT S0S SAFETY S1S THROUGH_VIOLENCE A0St BETA S2S OMEGA P3S EXPUNGER A1St APPARITION S1S
BITE A1St J_A_X S0S RITUAL_DAGGER A1St
BURN StUS DAZED StUS SLIMED St1S VOID StUS WOUND StUS
ASCENDER_S_BANE CuUS CLUMSY CuUS CURSE_OF_THE_BELL CuUS DECAY CuUS DOUBT CuUS INJURY CuUS NECRONOMICURSE CuUS NORMALITY CuUS PAIN CuUS
PARASITE CuUS PRIDE Cu1S REGRET CuUS SHAME CuUS WRITHE CuUS
"""
TYPES = {"A": "Attack", "S": "Skill", "P": "Power", "St": "Status", "Cu": "Curse"}
RARITIES = {"B": "Basic", "C": "Common", "U": "Uncommon", "R": "Rare", "S": "Event"}
NAMES = {"STRIKE_R": "Strike", "STRIKE_G": "Strike", "STRIKE_B": "Strike", "STRIKE_P": "Strike", "DEFEND_R": "Defend", "DEFEND_G": "Defend",
         "DEFEND_B": "Defend", "DEFEND_P": "Defend", "J_A_X": "J.A.X.", "ASCENDER_S_BANE": "Ascender's Bane", "A_THOUSAND_CUTS": "A Thousand Cuts",
         "FTL": "FTL", "DIE_DIE_DIE": "Die Die Die"}
X_PLUS = {"MALAISE", "TEMPEST", "MULTI_CAST", "DOPPELGANGER", "COLLECT"}  # X cards whose upgrade adds one (Malaise: X Strength down and X Weak, both)


def _gain(*specs):
    """A handler that adds to our powers: for each power its key, the amount, the amount once upgraded."""
    triples = list(zip(specs[::3], specs[1::3], specs[2::3]))
    return lambda f, c, *_: [add(f.pw, k, up if c.up else base) for k, base, up in triples]


def _to_hand(key, n=1, where="hand"):
    return lambda f, c, *_: f.add_new(key, n, where, c.up and key in ("SMITE", "SAFETY", "INSIGHT", "THROUGH_VIOLENCE", "EXPUNGER"))


def _if(test, **zeroed):
    """Read the card as if the named fields were 0 unless `test(f, c, foe)` holds."""
    return lambda f, c, foe, i: i if test(f, c, foe) else replace(i, **{k: (0 if k != "stance" else "") for k in zeroed})


_attacking = lambda f, c, foe: foe is not None and foe.move.dmg > 0 and not foe.move.kind


def _exhaust(f, cards) -> list:
    """Exhaust these cards from the hand; the list of them."""
    cards = list(cards)
    for card in cards:
        f.hand.remove(card)
        f.exhaust(card)
    return cards


def _exhaust_pick(n, prompt="Choose {n} card(s) to Exhaust."):
    def go(f, *_):
        for card in f.hand_pick(prompt.format(n=n), n):
            if card in f.hand:
                f.hand.remove(card)
                f.exhaust(card)
    return go


def _discard(f, cards) -> list:
    """Discard these cards from the hand (by a card's say); the list of them."""
    cards = list(cards)
    for h in cards:
        f.hand.remove(h)
        f.discard.append(h)
        fx.discarded(f, h)
    return cards


def _pick(f, prompt, pile, pool=None, n=1, put=None):
    """Up to n cards the bot picks from a pile (or from `pool`, part of it), each taken out and put in the hand (or by `put`)."""
    for card in f.pile_pick(prompt, pile if pool is None else pool, n):
        pile.remove(card)
        (put or f.add_card)(card)


def _to_top(f, c, *_, free=False):
    """A card of the hand on top of the draw pile (Warcry, Thinking Ahead; Setup: free to play this fight)."""
    for card in f.hand_pick("Put a card on top of your draw pile.", 1):
        f.hand.remove(card)
        if free:
            card.cost_fight = 0
        f.draw.append(card)


def _random_card(kind):
    return lambda f, *_: common.free_card(f, [d for d in f.game.pool(f.run.character) if d.type == kind])


def _scry(base, up):
    return lambda f, c, *_: fx.scry(f, up if c.up else base)


def _secret(kind):
    return lambda f, *_: _pick(f, "Choose a card to put into your hand.", f.draw, [d for d in f.draw if d.type == kind])


def _second_wind(f, c, *_):
    for h in [h for h in f.hand if h.type != "Attack"]:
        f.hand.remove(h)
        f.exhaust(h)
        f.gain_block(f.block_value(7 if c.up else 5))


def _havoc(f, c, foe, *_):
    f.refill()
    if f.draw:
        top = f.draw.pop()
        if top.cost() >= -1:
            fx.resolve(f, top, foe if foe is not None and foe.alive else (f.alive() or [None])[0], f.energy if top.cost() == -1 else 0)
        f.exhaust(top)


def _feed(f, c, foe, *_):
    if foe is not None and not foe.alive and not foe.minion:
        f.run.max_hp += 4 if c.up else 3
        f.heal(4 if c.up else 3)


def _recursion(f, *_):
    if f.orbs:
        kind = f.orbs[0][0]
        f.evoke()
        f.channel(kind)


def _scrape(f, c, *_):
    before = len(f.hand)
    f.draw_cards(5 if c.up else 4)
    for h in f.hand[before:]:
        if h.cost() != 0:
            f.hand.remove(h)
            f.discard.append(h)


def _fission(f, c, *_):
    n = len(f.orbs)
    while f.orbs:
        f.evoke() if c.up else f.orbs.pop(0)
    f.energy += n
    f.draw_cards(n)


def _all_for_one(f, *_):
    for h in [h for h in f.discard if h.cost() == 0]:
        if len(f.hand) < 10:
            f.discard.remove(h)
            f.hand.append(h)


DAMAGE = {
    "BODY_SLAM": lambda f, c: f.block,
    "PERFECTED_STRIKE": lambda f, c: 6 + (3 if c.up else 2) * (sum(1 for x in f.hand + f.draw + f.discard + f.exhausted if "Strike" in x.name) + 1),
    "HEAVY_BLADE": lambda f, c: 14 + ((5 if c.up else 3) - 1) * f.strength(),
    "MIND_BLAST": lambda f, c: len(f.draw),
    "BOWLING_BASH": lambda f, c: 10 if c.up else 7,
    "FLECHETTES": lambda f, c: 6 if c.up else 4,
    "FINISHER": lambda f, c: 8 if c.up else 6,
    "BARRAGE": lambda f, c: 7 if c.up else 5,
    "BRILLIANCE": lambda f, c: (16 if c.up else 12) + f.pw.get("MANTRA_GAINED", 0),
    "BLIZZARD": lambda f, c: (3 if c.up else 2) * f.pw.get("FROST_CHANNELED", 0),
    "THUNDER_STRIKE": lambda f, c: 9 if c.up else 7,
    "EXPUNGER": lambda f, c: 15 if c.up else 9,
    "RAMPAGE": lambda f, c: 8 + c.bonus,
    "CLAW": lambda f, c: (5 if c.up else 3) + f.pw.get("CLAW_BONUS", 0),
    "GLASS_KNIFE": lambda f, c: (12 if c.up else 8) + c.bonus,
    "WINDMILL_STRIKE": lambda f, c: (10 if c.up else 7) + c.bonus,
}
PRE = {
    "DROPKICK": _if(lambda f, c, foe: foe is not None and foe.powers.get("VULNERABLE"), energy_gain=0, draw=0),
    "HEEL_HOOK": _if(lambda f, c, foe: foe is not None and foe.powers.get("WEAK"), energy_gain=0, draw=0),
    "SPOT_WEAKNESS": _if(_attacking, strength=0), "GO_FOR_THE_EYES": _if(_attacking, weak=0), "FEAR_NO_EVIL": _if(_attacking, stance=1),
    "FOLLOW_UP": _if(lambda f, c, foe: f.last_type == "Attack", energy_gain=0), "SANCTITY": _if(lambda f, c, foe: f.last_type == "Skill", draw=0),
    "CRUSH_JOINTS": _if(lambda f, c, foe: f.last_type == "Skill", vulnerable=0), "SASH_WHIP": _if(lambda f, c, foe: f.last_type == "Attack", weak=0),
    "FTL": _if(lambda f, c, foe: len(f.turn_cards) - 1 < (4 if c.up else 3), draw=0),
    "SNEAKY_STRIKE": _if(lambda f, c, foe: f.pw.get("DISCARDED_TURN"), energy_gain=0),
    "EVISCERATE": lambda f, c, foe, i: replace(i, hits=3),
    "FIEND_FIRE": lambda f, c, foe, i: replace(i, hits=len(_exhaust(f, f.hand))),
    "BURNING_PACT": lambda f, c, foe, i: _exhaust_pick(1)(f) or i,
    "SECOND_WIND": lambda f, c, foe, i: replace(i, block=0),
    "COMPILE_DRIVER": lambda f, c, foe, i: replace(i, draw=len({o[0] for o in f.orbs})),
    "FINISHER": lambda f, c, foe, i: replace(i, hits=max(0, f.attacks_turn)),
    "FLECHETTES": lambda f, c, foe, i: replace(i, hits=sum(1 for h in f.hand if h.type == "Skill")),
    "BARRAGE": lambda f, c, foe, i: replace(i, hits=len(f.orbs)),
    "BOWLING_BASH": lambda f, c, foe, i: replace(i, hits=len(f.alive())),
    "THUNDER_STRIKE": lambda f, c, foe, i: replace(i, hits=f.pw.get("LIGHTNING_CHANNELED", 0), random_target=True),
    "EXPUNGER": lambda f, c, foe, i: replace(i, hits=max(1, f.pw.get("EXPUNGER_X", 1))),
    "MELTER": lambda f, c, foe, i: (setattr(foe, "block", 0) or i) if foe is not None else i,
    "AUTO_SHIELDS": lambda f, c, foe, i: replace(i, block=0),
    "BLIZZARD": lambda f, c, foe, i: replace(i, aoe=True),
}
AFTER = {
    "ANGER": lambda f, c, *_: f.add_card(c.copy(), "discard"),
    "ARMAMENTS": lambda f, c, *_: [setattr(h, "up", True) for h in (f.hand if c.up else f.hand_pick("Choose a card to Upgrade.", 1, [h for h in f.hand if not h.up and h.type not in ("Status", "Curse")]))],
    "HAVOC": _havoc, "HEADBUTT": lambda f, *_: _pick(f, "Choose a card to put on top of your draw pile.", f.discard, put=f.draw.append),
    "TRUE_GRIT": lambda f, c, *_: (_exhaust_pick(1)(f) if c.up else _exhaust(f, [f.rng.choice(f.hand)])) if f.hand else None,
    "WARCRY": _to_top, "WILD_STRIKE": _to_hand("WOUND", where="draw"), "BATTLE_TRANCE": lambda f, *_: setattr(f, "no_draw", True),
    "DUAL_WIELD": lambda f, c, *_: [f.add_card(k.copy()) for k in f.hand_pick("Choose a card to copy.", 1, [h for h in f.hand if h.type in ("Attack", "Power")]) for _ in range(2 if c.up else 1)],
    "ENTRENCH": lambda f, *_: f.gain_block(f.block), "FLAME_BARRIER": lambda f, c, *_: add(f.temp, "FLAME_BARRIER", 6 if c.up else 4),
    "INFERNAL_BLADE": _random_card("Attack"), "POWER_THROUGH": _to_hand("WOUND", 2), "RAGE": _gain("RAGE", 3, 5),
    "RAMPAGE": lambda f, c, *_: setattr(c, "bonus", c.bonus + (8 if c.up else 5)), "RECKLESS_CHARGE": _to_hand("DAZED", where="draw"),
    "SECOND_WIND": _second_wind, "SEVER_SOUL": lambda f, *_: _exhaust(f, [h for h in f.hand if h.type != "Attack"]),
    "DOUBLE_TAP": lambda f, c, *_: setattr(f, "double_next", f.double_next + (2 if c.up else 1)),
    "EXHUME": lambda f, *_: _pick(f, "Choose a card to return to your hand.", f.exhausted, [e for e in f.exhausted if e.key != "EXHUME"]),
    "FEED": _feed, "IMMOLATE": _to_hand("BURN", where="discard"), "LIMIT_BREAK": lambda f, *_: f.pw.__setitem__("STRENGTH", f.pw.get("STRENGTH", 0) * 2),
    "REAPER": lambda f, c, foe, x, dealt: f.heal(dealt),
    **dict.fromkeys(("SURVIVOR", "ACROBATICS", "DAGGER_THROW"), lambda f, *_: fx.discard_from_hand(f, 1)),
    "PREPARED": lambda f, c, *_: fx.discard_from_hand(f, 2 if c.up else 1), "CONCENTRATE": lambda f, c, *_: fx.discard_from_hand(f, 2 if c.up else 3),
    "BANE": lambda f, c, foe, *_: f.hurt(foe, f.attack_value(10 if c.up else 7, foe), attack=True) if foe is not None and foe.alive and foe.powers.get("POISON") else None,
    "ALL_OUT_ATTACK": lambda f, *_: _discard(f, [f.rng.choice(f.hand)]) if f.hand else None, "BLUR": _gain("BLUR", 1, 1),
    "CALCULATED_GAMBLE": lambda f, *_: f.draw_cards(len(_discard(f, f.hand))),
    "CATALYST": lambda f, c, foe, *_: foe.powers.__setitem__("POISON", foe.powers["POISON"] * (3 if c.up else 2)) if foe is not None and foe.powers.get("POISON") else None,
    "CHOKE": lambda f, c, foe, *_: add(foe.mem, "choke", 5 if c.up else 3) if foe is not None else None,
    "DISTRACTION": _random_card("Skill"), "EXPERTISE": lambda f, c, *_: f.draw_cards(max(0, (7 if c.up else 6) - len(f.hand))),
    "SETUP": lambda f, c, *_: _to_top(f, c, free=True), "BULLET_TIME": lambda f, *_: ([setattr(h, "cost_turn", 0) for h in f.hand], setattr(f, "no_draw", True)),
    "BURST": lambda f, c, *_: setattr(f, "burst", f.burst + (2 if c.up else 1)),
    "CORPSE_EXPLOSION": lambda f, c, foe, *_: foe.mem.__setitem__("corpse", True) if foe is not None else None,
    "DOPPELGANGER": lambda f, c, foe, x, dealt: (add(f.pw, "DRAW_CARDS_NEXT_TURN", x), add(f.pw, "ENERGY_NEXT_TURN", x)),
    "GLASS_KNIFE": lambda f, c, *_: setattr(c, "bonus", c.bonus - 2), "PHANTASMAL_KILLER": _gain("DOUBLE_DAMAGE_NEXT", 1, 1),
    "STORM_OF_STEEL": lambda f, c, *_: f.add_new("SHIV", len(_discard(f, f.hand)), up=c.up),
    "UNLOAD": lambda f, *_: _discard(f, [h for h in f.hand if h.type != "Attack"]), "ALCHEMIZE": lambda f, *_: f.run.add_potion(f.game.random_potion(f.run)),
    "CLAW": _gain("CLAW_BONUS", 2, 2), "RECURSION": _recursion, "STACK": lambda f, c, *_: f.gain_block(f.block_value(len(f.discard) + (3 if c.up else 0))),
    "STEAM_BARRIER": lambda f, c, *_: setattr(c, "bonus", c.bonus - 1), "STREAMLINE": lambda f, c, *_: setattr(c, "cost_fight", max(0, c.cost() - 1)),
    "TURBO": _to_hand("VOID", where="discard"), "AUTO_SHIELDS": lambda f, c, *_: f.gain_block(f.block_value(15 if c.up else 11)) if f.block == 0 else None,
    "CHAOS": lambda f, c, *_: [f.channel(f.rng.choice(("LIGHTNING", "FROST", "DARK", "PLASMA"))) for _ in range(2 if c.up else 1)],
    "CHILL": lambda f, *_: [f.channel("FROST") for _ in f.alive()],
    "CONSUME": lambda f, c, *_: (add(f.pw, "FOCUS", 3 if c.up else 2), setattr(f, "orb_slots", max(0, f.orb_slots - 1)), setattr(f, "orbs", f.orbs[: f.orb_slots])),
    "DOUBLE_ENERGY": lambda f, *_: setattr(f, "energy", f.energy * 2),
    "EQUILIBRIUM": lambda f, *_: ([setattr(h, "retained", -1) for h in f.hand], f.pw.__setitem__("KEEP_HAND", 1)),  # (kept by the end of turn)
    "GENETIC_ALGORITHM": lambda f, c, *_: setattr(c, "bonus", c.bonus + (3 if c.up else 2)), "OVERCLOCK": _to_hand("BURN", where="discard"),
    "RECYCLE": lambda f, *_: [(f.hand.remove(k), setattr(f, "energy", f.energy + max(0, k.cost())), f.exhaust(k)) for k in f.hand_pick("Choose a card to Exhaust.", 1)],
    "REPROGRAM": _gain("FOCUS", -1, -2),  # (its Strength and Dexterity are the parse's: until 2026-11-26 counted twice) "SCRAPE": _scrape,
    "SUNDER": lambda f, c, foe, *_: setattr(f, "energy", f.energy + 3) if foe is not None and not foe.alive else None,
    "WHITE_NOISE": _random_card("Power"), "ALL_FOR_ONE": _all_for_one, "AMPLIFY": _gain("AMPLIFY", 1, 2), "CORE_SURGE": _gain("ARTIFACT", 1, 1),
    "FISSION": _fission, "HYPERBEAM": _gain("FOCUS", -3, -3), "MULTI_CAST": lambda f, c, foe, x, dealt: f.evoke(x),
    "RAINBOW": lambda f, *_: (f.channel("FROST"), f.channel("DARK")),
    "REBOOT": lambda f, c, *_: (f.draw.extend(f.hand + f.discard), setattr(f, "hand", []), setattr(f, "discard", []), f.rng.shuffle(f.draw), f.draw_cards(6 if c.up else 4)),
    "SEEK": lambda f, c, *_: _pick(f, "Choose a card to put into your hand.", f.draw, n=2 if c.up else 1),
    "HOLOGRAM": lambda f, *_: _pick(f, "Choose a card to return to your hand.", f.discard),
    "PROSTRATE": lambda f, c, *_: f.gain_mantra(3 if c.up else 2), "WORSHIP": lambda f, *_: f.gain_mantra(5),
    "PRAY": lambda f, c, *_: (f.gain_mantra(4 if c.up else 3), f.add_new("INSIGHT", 1, "draw")),
    "INNER_PEACE": lambda f, c, *_: f.draw_cards(4 if c.up else 3) if f.stance == "CALM" else f.enter("CALM"),
    "INDIGNATION": lambda f, c, *_: fx.debuff_all(f, "VULNERABLE", 5 if c.up else 3) if f.stance == "WRATH" else f.enter("WRATH"),
    "HALT": lambda f, c, *_: f.gain_block(f.block_value(14 if c.up else 9)) if f.stance == "WRATH" else None,
    "THIRD_EYE": _scry(3, 5), "JUST_LUCKY": _scry(1, 2), "CUT_THROUGH_FATE": _scry(2, 3),
    "MEDITATE": lambda f, c, *_: (_pick(f, "Choose a card to return to your hand.", f.discard, n=2 if c.up else 1), f.enter("CALM"), f.pw.__setitem__("TURN_OVER", 1)),
    "SPIRIT_SHIELD": lambda f, c, *_: f.gain_block(f.block_value((4 if c.up else 3) * len(f.hand))),
    "CONJURE_BLADE": lambda f, c, foe, x, dealt: (f.pw.__setitem__("EXPUNGER_X", x + (1 if c.up else 0)), f.add_new("EXPUNGER", 1, "draw")),
    "DECEIVE_REALITY": _to_hand("SAFETY"), "EVALUATE": _to_hand("INSIGHT", where="draw"), "CARVE_REALITY": _to_hand("SMITE"),
    "REACH_HEAVEN": _to_hand("THROUGH_VIOLENCE", where="draw"), "ALPHA": _to_hand("BETA", where="draw"), "BETA": _to_hand("OMEGA", where="draw"),
    "WALLOP": lambda f, c, foe, x, dealt: f.gain_block(dealt),
    "JUDGMENT": lambda f, c, foe, *_: f.kill(foe) if foe is not None and foe.alive and foe.hp <= (40 if c.up else 30) else None,
    "PRESSURE_POINTS": lambda f, c, foe, *_: (add(foe.powers, "MARK", 11 if c.up else 8) if foe is not None else None,
                                              [f.hurt(v, v.powers["MARK"], attack=False) for v in f.alive() if v.powers.get("MARK")]),
    "TALK_TO_THE_HAND": lambda f, c, foe, *_: add(foe.mem, "block_return", 3 if c.up else 2) if foe is not None else None,
    "WAVE_OF_THE_HAND": _gain("WAVE_OF_THE_HAND", 1, 2), "SWIVEL": lambda f, *_: setattr(f, "free_attack", True),
    "SIMMERING_FURY": lambda f, c, *_: (f.pw.__setitem__("SIMMERING", 1), add(f.pw, "DRAW_CARDS_NEXT_TURN", 3 if c.up else 2)),
    "WREATH_OF_FLAME": _gain("VIGOR", 5, 8), "WISH": _gain("STRENGTH", 3, 4), "VAULT": lambda f, *_: f.pw.update(EXTRA_TURN=1, TURN_OVER=1),
    "SCRAWL": lambda f, *_: f.draw_cards(10 - len(f.hand)),
    "BLASPHEMY": lambda f, *_: f.pw.__setitem__("BLASPHEMY_DUE", 1), "CONCLUDE": lambda f, *_: f.pw.__setitem__("TURN_OVER", 1),
    "APOTHEOSIS": lambda f, *_: [setattr(h, "up", True) for h in f.hand + f.draw + f.discard], "PANIC_BUTTON": lambda f, *_: f.pw.__setitem__("NO_BLOCK", 2),
    "ENLIGHTENMENT": lambda f, *_: [setattr(h, "cost_turn", 1) for h in f.hand if h.cost() > 1],
    "DEEP_BREATH": lambda f, c, *_: (f.draw.extend(f.discard), setattr(f, "discard", []), f.rng.shuffle(f.draw), f.draw_cards(2 if c.up else 1)),
    "SECRET_TECHNIQUE": _secret("Skill"), "SECRET_WEAPON": _secret("Attack"),
    "MADNESS": lambda f, *_: setattr(f.rng.choice(pool), "cost_fight", 0) if (pool := [h for h in f.hand if h.cost() > 0]) else None,
    "THINKING_AHEAD": _to_top, "JACK_OF_ALL_TRADES": _random_card("Skill"), "DISCOVERY": _random_card("Attack"),
    "FOREIGN_INFLUENCE": _random_card("Attack"), "PURITY": _exhaust_pick(3, "Choose up to {n} card(s) to Exhaust."),
}
POWER = {
    **{k: _gain(k, base, up) for k, base, up in (
        ("DEMON_FORM", 2, 3), ("METALLICIZE", 3, 4), ("BARRICADE", 1, 1), ("FEEL_NO_PAIN", 3, 4), ("DARK_EMBRACE", 1, 1), ("CORRUPTION", 1, 1), ("JUGGERNAUT", 5, 7),
        ("EVOLVE", 1, 2), ("FIRE_BREATHING", 6, 10), ("RUPTURE", 1, 2), ("BRUTALITY", 1, 1), ("NOXIOUS_FUMES", 2, 3), ("AFTER_IMAGE", 1, 1), ("A_THOUSAND_CUTS", 1, 2),
        ("ACCURACY", 4, 6), ("INFINITE_BLADES", 1, 1), ("ENVENOM", 1, 1), ("TOOLS_OF_THE_TRADE", 1, 1), ("WELL_LAID_PLANS", 1, 2), ("LOOP", 1, 2), ("STORM", 1, 1),
        ("STATIC_DISCHARGE", 1, 2), ("HEATSINKS", 1, 2), ("HELLO_WORLD", 1, 1), ("CREATIVE_AI", 1, 1), ("MACHINE_LEARNING", 1, 1), ("BUFFER", 1, 2), ("ECHO_FORM", 1, 1),
        ("SELF_REPAIR", 7, 10), ("MENTAL_FORTRESS", 4, 6), ("RUSHDOWN", 2, 2), ("LIKE_WATER", 5, 7), ("NIRVANA", 3, 4), ("DEVOTION", 2, 3), ("DEVA_FORM", 1, 1),
        ("BATTLE_HYMN", 1, 1), ("STUDY", 1, 1), ("FORESIGHT", 3, 4), ("ESTABLISHMENT", 1, 1), ("MASTER_REALITY", 1, 1), ("OMEGA", 50, 60), ("PANACHE", 10, 14),
        ("SADISTIC_NATURE", 5, 7), ("MAYHEM", 1, 1), ("MAGNETISM", 1, 1))},
    "CALTROPS": _gain("THORNS", 3, 5), "DEFRAGMENT": _gain("FOCUS", 1, 2), "COMBUST": _gain("COMBUST", 5, 7, "COMBUST_HP", 1, 1),
    "BERSERK": _gain("VULNERABLE", 2, 1, "BERSERK", 1, 1), "WRAITH_FORM": _gain("INTANGIBLE", 2, 3, "WRAITH_FORM", 1, 1),
    "BIASED_COGNITION": _gain("FOCUS", 4, 5, "BIASED_COGNITION", 1, 1), "CAPACITOR": lambda f, c: setattr(f, "orb_slots", f.orb_slots + (3 if c.up else 2)),
    "ELECTRODYNAMICS": lambda f, c: (f.pw.__setitem__("ELECTRODYNAMICS", 1), [f.channel("LIGHTNING") for _ in range(3 if c.up else 2)]),
}
PLAYABLE = {
    "CLASH": lambda f, c: all(h.type == "Attack" for h in f.hand if h is not c),
    "GRAND_FINALE": lambda f, c: not f.draw,
    "SIGNATURE_MOVE": lambda f, c: not any(h.type == "Attack" for h in f.hand if h is not c),
    "SECRET_TECHNIQUE": lambda f, c: any(d.type == "Skill" for d in f.draw),
    "SECRET_WEAPON": lambda f, c: any(d.type == "Attack" for d in f.draw),
}
DRAWN = {
    "ENDLESS_AGONY": lambda f, c: f.add_card(c.copy()),
    "DEUS_EX_MACHINA": lambda f, c: (f.hand.remove(c), f.exhaust(c), f.add_new("MIRACLE", 3 if c.up else 2)),
}
RETAINED = {
    "WINDMILL_STRIKE": lambda f, c: setattr(c, "bonus", c.bonus + (5 if c.up else 4)),
    "PERSEVERANCE": lambda f, c: setattr(c, "bonus", c.bonus + (3 if c.up else 2)),
    "SANDS_OF_TIME": lambda f, c: setattr(c, "cost_fight", max(0, c.cost() - 1)),
}
