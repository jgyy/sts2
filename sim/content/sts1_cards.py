"""The first game's cards: what the text tables (sts2bot.games.sts1.text) leave out, and what their text cannot say.

META: type, cost, rarity and whether the card is aimed ("t"), one card a line. Then the handlers effects.py asks
for (see its docstring), for the cards whose text says more than the planner's parser reads, or says it under a
condition.
"""

from __future__ import annotations

from dataclasses import replace

from ..cards import Card
from .. import effects as fx

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
X_PLUS = {"MALAISE", "TEMPEST", "MULTI_CAST", "DOPPELGANGER", "COLLECT"}  # X cards whose upgrade adds one


def _discard(n, prompt="Choose {n} card(s) to Discard."):
    return lambda f, c, foe, x, dealt: fx.discard_from_hand(f, n, prompt)


def _to_hand(key, n=1, up=False, where="hand"):
    def add(f, c, foe, x, dealt):
        for _ in range(n):
            f.add_card(f.game.make(key, up=up or c.up and key in ("SMITE", "SAFETY", "INSIGHT", "THROUGH_VIOLENCE", "EXPUNGER")), where)
    return add


def _gain(key, base, up):
    """A handler that adds `base` (`up` once upgraded) to one of our powers."""
    return lambda f, c, *_: f.pw.__setitem__(key, f.pw.get(key, 0) + (up if c.up else base))


def _more(attr, base, up):
    """... and one that adds it to a counter of the fight's (Double Tap, Burst)."""
    return lambda f, c, *_: setattr(f, attr, getattr(f, attr) + (up if c.up else base))


def _power(key, base, up):
    return lambda f, c: f.pw.__setitem__(key, f.pw.get(key, 0) + (up if c.up else base))


def _if(test, **zeroed):
    """Read the card as if the named fields were 0 unless `test(f, c, foe)` holds."""
    return lambda f, c, foe, i: i if test(f, c, foe) else replace(i, **{k: (0 if k != "stance" else "") for k in zeroed})


def _attacking(f, c, foe):
    return foe is not None and foe.move.dmg > 0 and not foe.move.kind


def _exhaust_pick(n, prompt="Choose {n} card(s) to Exhaust."):
    def go(f, c, foe, x, dealt):
        for card in f.hand_pick(prompt.format(n=n), n):
            if card in f.hand:
                f.hand.remove(card)
                f.exhaust(card)
    return go


def _exhaust_where(test):
    def go(f, c, foe, *rest):
        for card in [h for h in f.hand if test(h)]:
            f.hand.remove(card)
            f.exhaust(card)
        return len([1 for _ in ()])
    return go


def _second_wind(f, c, foe, x, dealt):
    gone = [h for h in f.hand if h.type != "Attack"]
    for h in gone:
        f.hand.remove(h)
        f.exhaust(h)
        f.gain_block(f.block_value(7 if c.up else 5))


def _fiend_fire(f, c, foe, i):
    n = len(f.hand)
    for h in list(f.hand):
        f.hand.remove(h)
        f.exhaust(h)
    return replace(i, hits=n)


def _havoc(f, c, foe, x, dealt):
    if not f.draw and f.discard:
        f.draw, f.discard = f.discard, []
        f.rng.shuffle(f.draw)
    if f.draw:
        top = f.draw.pop()
        if top.cost() >= -1:
            fx.resolve(f, top, foe if foe is not None and foe.alive else (f.alive() or [None])[0], f.energy if top.cost() == -1 else 0)
        f.exhaust(top)


def _headbutt(f, c, foe, x, dealt):
    for card in f.pile_pick("Choose a card to put on top of your draw pile.", f.discard):
        f.discard.remove(card)
        f.draw.append(card)


def _exhume(f, c, foe, x, dealt):
    pool = [e for e in f.exhausted if e.key != "EXHUME"]
    for card in f.pile_pick("Choose a card to return to your hand.", pool):
        f.exhausted.remove(card)
        f.add_card(card)


def _random_card(kind, cost0=True):
    def go(f, c, foe, x, dealt):
        pool = [d for d in f.game.pool(f.run.character) if d.type == kind]
        if pool:
            new = Card(f.rng.choice(pool))
            if cost0:
                new.cost_turn = 0
            f.add_card(new)
    return go


def _dual_wield(f, c, foe, x, dealt):
    pool = [h for h in f.hand if h.type in ("Attack", "Power")]
    for card in f.hand_pick("Choose a card to copy.", 1, pool):
        for _ in range(2 if c.up else 1):
            f.add_card(card.copy())


def _warcry(f, c, foe, x, dealt):
    for card in f.hand_pick("Put a card on top of your draw pile.", 1):
        f.hand.remove(card)
        f.draw.append(card)


def _true_grit(f, c, foe, x, dealt):
    if not f.hand:
        return
    if c.up:
        _exhaust_pick(1)(f, c, foe, x, dealt)
    else:
        card = f.rng.choice(f.hand)
        f.hand.remove(card)
        f.exhaust(card)


def _burning_pact(f, c, foe, i):
    _exhaust_pick(1)(f, c, foe, 0, 0)
    return i


def _limit_break(f, c, foe, x, dealt):
    f.pw["STRENGTH"] = f.pw.get("STRENGTH", 0) * 2


def _entrench(f, c, foe, x, dealt):
    f.gain_block(f.block)


def _feed(f, c, foe, x, dealt):
    if foe is not None and not foe.alive and not foe.minion:
        gain = 4 if c.up else 3
        f.run.max_hp += gain
        f.heal(gain)


def _reaper(f, c, foe, x, dealt):
    f.heal(dealt)


def _rampage(f, c, foe, x, dealt):
    c.bonus += 8 if c.up else 5


def _bane(f, c, foe, x, dealt):
    if foe is not None and foe.alive and foe.powers.get("POISON"):
        f.hurt(foe, f.attack_value(10 if c.up else 7, foe), attack=True)


def _catalyst(f, c, foe, x, dealt):
    if foe is not None and foe.powers.get("POISON"):
        foe.powers["POISON"] *= 3 if c.up else 2


def _calculated_gamble(f, c, foe, x, dealt):
    n = len(f.hand)
    for h in list(f.hand):
        f.hand.remove(h)
        f.discard.append(h)
        fx.discarded(f, h)
    f.draw_cards(n)


def _storm_of_steel(f, c, foe, x, dealt):
    n = len(f.hand)
    for h in list(f.hand):
        f.hand.remove(h)
        f.discard.append(h)
        fx.discarded(f, h)
    for _ in range(n):
        f.add_card(f.game.make("SHIV", up=c.up))


def _expertise(f, c, foe, x, dealt):
    f.draw_cards(max(0, (7 if c.up else 6) - len(f.hand)))


def _setup(f, c, foe, x, dealt):
    for card in f.hand_pick("Put a card on top of your draw pile.", 1):
        f.hand.remove(card)
        card.cost_fight = 0
        f.draw.append(card)


def _bullet_time(f, c, foe, x, dealt):
    for h in f.hand:
        h.cost_turn = 0
    f.no_draw = True


def _doppelganger(f, c, foe, x, dealt):
    f.pw["DRAW_CARDS_NEXT_TURN"] = f.pw.get("DRAW_CARDS_NEXT_TURN", 0) + x
    f.pw["ENERGY_NEXT_TURN"] = f.pw.get("ENERGY_NEXT_TURN", 0) + x


def _malaise(f, c, foe, i):
    return i  # (X Strength and X Weak: the generic X handling counts both once per energy)


def _unload(f, c, foe, x, dealt):
    for h in [h for h in f.hand if h.type != "Attack"]:
        f.hand.remove(h)
        f.discard.append(h)
        fx.discarded(f, h)


def _orbs_unique(f):
    return len({o[0] for o in f.orbs})


def _recursion(f, c, foe, x, dealt):
    if f.orbs:
        kind = f.orbs[0][0]
        f.evoke()
        f.channel(kind)


def _chaos(f, c, foe, x, dealt):
    for _ in range(2 if c.up else 1):
        f.channel(f.rng.choice(("LIGHTNING", "FROST", "DARK", "PLASMA")))


def _chill(f, c, foe, x, dealt):
    for _ in f.alive():
        f.channel("FROST")


def _consume(f, c, foe, x, dealt):
    f.pw["FOCUS"] = f.pw.get("FOCUS", 0) + (3 if c.up else 2)
    f.orb_slots = max(0, f.orb_slots - 1)
    f.orbs = f.orbs[: f.orb_slots]


def _double_energy(f, c, foe, x, dealt):
    f.energy *= 2


def _recycle(f, c, foe, x, dealt):
    for card in f.hand_pick("Choose a card to Exhaust.", 1):
        f.hand.remove(card)
        f.energy += max(0, card.cost())
        f.exhaust(card)


def _reprogram(f, c, foe, x, dealt):
    n = 2 if c.up else 1
    for k, d in (("FOCUS", -n), ("STRENGTH", n), ("DEXTERITY", n)):
        f.pw[k] = f.pw.get(k, 0) + d


def _scrape(f, c, foe, x, dealt):
    before = len(f.hand)
    f.draw_cards(5 if c.up else 4)
    for h in f.hand[before:]:
        if h.cost() != 0:
            f.hand.remove(h)
            f.discard.append(h)


def _sunder(f, c, foe, x, dealt):
    if foe is not None and not foe.alive:
        f.energy += 3


def _fission(f, c, foe, x, dealt):
    n = len(f.orbs)
    while f.orbs:
        f.evoke() if c.up else f.orbs.pop(0)
    f.energy += n
    f.draw_cards(n)


def _multi_cast(f, c, foe, x, dealt):
    f.evoke(x)


def _rainbow(f, c, foe, x, dealt):
    f.channel("FROST")
    f.channel("DARK")


def _reboot(f, c, foe, x, dealt):
    f.draw.extend(f.hand + f.discard)
    f.hand, f.discard = [], []
    f.rng.shuffle(f.draw)
    f.draw_cards(6 if c.up else 4)


def _seek(f, c, foe, x, dealt):
    for card in f.pile_pick("Choose a card to put into your hand.", list(f.draw), 2 if c.up else 1):
        f.draw.remove(card)
        f.add_card(card)


def _all_for_one(f, c, foe, x, dealt):
    for h in [h for h in f.discard if h.cost() == 0]:
        if len(f.hand) < 10:
            f.discard.remove(h)
            f.hand.append(h)


def _hologram(f, c, foe, x, dealt):
    for card in f.pile_pick("Choose a card to return to your hand.", f.discard):
        f.discard.remove(card)
        f.add_card(card)


def _stack(f, c, foe, x, dealt):
    f.gain_block(f.block_value(len(f.discard) + (3 if c.up else 0)))


def _auto_shields(f, c, foe, x, dealt):
    if f.block == 0:
        f.gain_block(f.block_value(15 if c.up else 11))


def _melter(f, c, foe, i):
    if foe is not None:
        foe.block = 0
    return i


def _mantra(n_base, n_up):
    return lambda f, c, foe, x, dealt: f.gain_mantra(n_up if c.up else n_base)


def _inner_peace(f, c, foe, x, dealt):
    if f.stance == "CALM":
        f.draw_cards(4 if c.up else 3)
    else:
        f.enter("CALM")


def _indignation(f, c, foe, x, dealt):
    if f.stance == "WRATH":
        for v in f.alive():
            fx.debuff(f, v, "VULNERABLE", 5 if c.up else 3)
    else:
        f.enter("WRATH")


def _halt(f, c, foe, x, dealt):
    if f.stance == "WRATH":
        f.gain_block(f.block_value(14 if c.up else 9))


def _scry(n_base, n_up):
    return lambda f, c, foe, x, dealt: fx.scry(f, n_up if c.up else n_base)


def _meditate(f, c, foe, x, dealt):
    for card in f.pile_pick("Choose a card to return to your hand.", f.discard, 2 if c.up else 1):
        f.discard.remove(card)
        f.add_card(card)
    f.enter("CALM")
    f.pw["TURN_OVER"] = 1


def _spirit_shield(f, c, foe, x, dealt):
    f.gain_block(f.block_value((4 if c.up else 3) * len(f.hand)))


def _wallop(f, c, foe, x, dealt):
    f.gain_block(dealt)


def _judgment(f, c, foe, x, dealt):
    if foe is not None and foe.alive and foe.hp <= (40 if c.up else 30):
        f.kill(foe)


def _pressure_points(f, c, foe, x, dealt):
    if foe is not None:
        foe.powers["MARK"] = foe.powers.get("MARK", 0) + (11 if c.up else 8)
    for v in f.alive():
        if v.powers.get("MARK"):
            f.hurt(v, v.powers["MARK"], attack=False)


def _conjure(f, c, foe, x, dealt):
    blade = f.game.make("EXPUNGER")
    blade.bonus = 0
    f.pw["EXPUNGER_X"] = x + (1 if c.up else 0)
    f.add_card(blade, "draw")


def _vault(f, c, foe, x, dealt):
    f.pw["EXTRA_TURN"] = 1
    f.pw["TURN_OVER"] = 1


def _scrawl(f, c, foe, x, dealt):
    f.draw_cards(10 - len(f.hand))


def _apotheosis(f, c, foe, x, dealt):
    for h in f.hand + f.draw + f.discard:
        h.up = True


def _panic(f, c, foe, x, dealt):
    f.pw["NO_BLOCK"] = 2


def _enlightenment(f, c, foe, x, dealt):
    for h in f.hand:
        if h.cost() > 1:
            h.cost_turn = 1


def _deep_breath(f, c, foe, x, dealt):
    f.draw.extend(f.discard)
    f.discard = []
    f.rng.shuffle(f.draw)
    f.draw_cards(2 if c.up else 1)


def _secret(kind):
    def go(f, c, foe, x, dealt):
        pool = [d for d in f.draw if d.type == kind]
        for card in f.pile_pick("Choose a card to put into your hand.", pool):
            f.draw.remove(card)
            f.add_card(card)
    return go


def _madness(f, c, foe, x, dealt):
    pool = [h for h in f.hand if h.cost() > 0]
    if pool:
        f.rng.choice(pool).cost_fight = 0


def _thinking_ahead(f, c, foe, x, dealt):
    _warcry(f, c, foe, x, dealt)


def _master_of_strategy(f, c, foe, x, dealt):
    pass


def _alchemize(f, c, foe, x, dealt):
    f.run.add_potion(f.game.random_potion(f.run))


def _sneaky(f, c, foe, i):
    return i if f.pw.get("DISCARDED_TURN") else replace(i, energy_gain=0)


def _eviscerate(f, c, foe, i):
    return replace(i, hits=3)


def _corpse(f, c, foe, x, dealt):
    if foe is not None:
        foe.mem["corpse"] = True


def _choke(f, c, foe, x, dealt):
    if foe is not None:
        foe.mem["choke"] = foe.mem.get("choke", 0) + (5 if c.up else 3)


def _talk(f, c, foe, x, dealt):
    if foe is not None:
        foe.mem["block_return"] = foe.mem.get("block_return", 0) + (3 if c.up else 2)


def _swivel(f, c, foe, x, dealt):
    f.free_attack = True


def _simmering(f, c, foe, x, dealt):
    f.pw["SIMMERING"] = 1
    f.pw["DRAW_CARDS_NEXT_TURN"] = f.pw.get("DRAW_CARDS_NEXT_TURN", 0) + (3 if c.up else 2)


def _flame_barrier(f, c, foe, x, dealt):
    f.temp["FLAME_BARRIER"] = f.temp.get("FLAME_BARRIER", 0) + (6 if c.up else 4)


def _equilibrium(f, c, foe, x, dealt):
    for h in f.hand:
        h.retained = -1  # (kept by the end of turn below)
    f.pw["KEEP_HAND"] = 1


def _berserk(f, c):
    f.pw["VULNERABLE"] = f.pw.get("VULNERABLE", 0) + (1 if c.up else 2)
    f.pw["BERSERK"] = f.pw.get("BERSERK", 0) + 1


def _wraith(f, c):
    f.pw["INTANGIBLE"] = f.pw.get("INTANGIBLE", 0) + (3 if c.up else 2)
    f.pw["WRAITH_FORM"] = f.pw.get("WRAITH_FORM", 0) + 1


def _capacitor(f, c):
    f.orb_slots += 3 if c.up else 2


def _electro(f, c):
    f.pw["ELECTRODYNAMICS"] = 1
    for _ in range(3 if c.up else 2):
        f.channel("LIGHTNING")


def _biased(f, c):
    f.pw["FOCUS"] = f.pw.get("FOCUS", 0) + (5 if c.up else 4)
    f.pw["BIASED_COGNITION"] = f.pw.get("BIASED_COGNITION", 0) + 1


def _combust(f, c):
    f.pw["COMBUST"] = f.pw.get("COMBUST", 0) + (7 if c.up else 5)
    f.pw["COMBUST_HP"] = f.pw.get("COMBUST_HP", 0) + 1


def _deva(f, c):
    f.pw["DEVA_FORM"] = f.pw.get("DEVA_FORM", 0) + 1


def _the_bomb(f, c):
    f.pw.setdefault("BOMBS", [])  # type: ignore[arg-type]


def _mayhem(f, c):
    f.pw["MAYHEM"] = f.pw.get("MAYHEM", 0) + 1


def _perfected(f, c):
    strikes = sum(1 for x in f.hand + f.draw + f.discard + f.exhausted if "Strike" in x.name) + 1
    return 6 + (3 if c.up else 2) * strikes


def _heavy_blade(f, c):
    return 14 + ((5 if c.up else 3) - 1) * f.strength()


DAMAGE = {
    "BODY_SLAM": lambda f, c: f.block,
    "PERFECTED_STRIKE": _perfected,
    "HEAVY_BLADE": _heavy_blade,
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
    "SPOT_WEAKNESS": _if(_attacking, strength=0),
    "GO_FOR_THE_EYES": _if(_attacking, weak=0),
    "FEAR_NO_EVIL": _if(_attacking, stance=1),
    "FOLLOW_UP": _if(lambda f, c, foe: f.last_type == "Attack", energy_gain=0),
    "SANCTITY": _if(lambda f, c, foe: f.last_type == "Skill", draw=0),
    "CRUSH_JOINTS": _if(lambda f, c, foe: f.last_type == "Skill", vulnerable=0),
    "SASH_WHIP": _if(lambda f, c, foe: f.last_type == "Attack", weak=0),
    "FTL": _if(lambda f, c, foe: len(f.turn_cards) - 1 < (4 if c.up else 3), draw=0),
    "SNEAKY_STRIKE": _sneaky,
    "EVISCERATE": _eviscerate,
    "FIEND_FIRE": _fiend_fire,
    "BURNING_PACT": _burning_pact,
    "SECOND_WIND": lambda f, c, foe, i: replace(i, block=0),
    "COMPILE_DRIVER": lambda f, c, foe, i: replace(i, draw=_orbs_unique(f)),
    "FINISHER": lambda f, c, foe, i: replace(i, hits=max(0, f.attacks_turn)),
    "FLECHETTES": lambda f, c, foe, i: replace(i, hits=sum(1 for h in f.hand if h.type == "Skill")),
    "BARRAGE": lambda f, c, foe, i: replace(i, hits=len(f.orbs)),
    "BOWLING_BASH": lambda f, c, foe, i: replace(i, hits=len(f.alive())),
    "THUNDER_STRIKE": lambda f, c, foe, i: replace(i, hits=f.pw.get("LIGHTNING_CHANNELED", 0), random_target=True),
    "EXPUNGER": lambda f, c, foe, i: replace(i, hits=max(1, f.pw.get("EXPUNGER_X", 1))),
    "MELTER": _melter,
    "MALAISE": _malaise,
    "AUTO_SHIELDS": lambda f, c, foe, i: replace(i, block=0),
    "BLIZZARD": lambda f, c, foe, i: replace(i, aoe=True),
}
AFTER = {
    "ANGER": lambda f, c, foe, x, d: f.add_card(c.copy(), "discard"), "ARMAMENTS": lambda f, c, foe, x, d: _armaments(f, c),
    "HAVOC": _havoc, "HEADBUTT": _headbutt, "TRUE_GRIT": _true_grit, "WARCRY": _warcry, "WILD_STRIKE": _to_hand("WOUND", where="draw"),
    "BATTLE_TRANCE": lambda f, c, foe, x, d: setattr(f, "no_draw", True), "DUAL_WIELD": _dual_wield, "ENTRENCH": _entrench,
    "FLAME_BARRIER": _flame_barrier, "INFERNAL_BLADE": _random_card("Attack"), "POWER_THROUGH": _to_hand("WOUND", 2), "RAGE": _gain("RAGE", 3, 5),
    "RAMPAGE": _rampage, "RECKLESS_CHARGE": _to_hand("DAZED", where="draw"), "SECOND_WIND": _second_wind,
    "SEVER_SOUL": _exhaust_where(lambda h: h.type != "Attack"), "DOUBLE_TAP": _more("double_next", 1, 2), "EXHUME": _exhume, "FEED": _feed,
    "IMMOLATE": _to_hand("BURN", where="discard"), "LIMIT_BREAK": _limit_break, "REAPER": _reaper,
    "SURVIVOR": _discard(1), "ACROBATICS": _discard(1), "DAGGER_THROW": _discard(1), "PREPARED": lambda f, c, foe, x, d: fx.discard_from_hand(f, 2 if c.up else 1),
    "BANE": _bane, "ALL_OUT_ATTACK": lambda f, c, foe, x, d: _random_discard(f), "BLUR": _gain("BLUR", 1, 1), "CALCULATED_GAMBLE": _calculated_gamble,
    "CATALYST": _catalyst, "CHOKE": _choke, "CONCENTRATE": lambda f, c, foe, x, d: fx.discard_from_hand(f, 2 if c.up else 3),
    "DISTRACTION": _random_card("Skill"), "EXPERTISE": _expertise, "SETUP": _setup, "BULLET_TIME": _bullet_time, "BURST": _more("burst", 1, 2),
    "CORPSE_EXPLOSION": _corpse, "DOPPELGANGER": _doppelganger, "GLASS_KNIFE": lambda f, c, foe, x, d: setattr(c, "bonus", c.bonus - 2),
    "PHANTASMAL_KILLER": _gain("DOUBLE_DAMAGE_NEXT", 1, 1), "STORM_OF_STEEL": _storm_of_steel, "UNLOAD": _unload, "ALCHEMIZE": _alchemize,
    "CLAW": lambda f, c, foe, x, d: f.pw.__setitem__("CLAW_BONUS", f.pw.get("CLAW_BONUS", 0) + 2), "RECURSION": _recursion,
    "STACK": _stack, "STEAM_BARRIER": lambda f, c, foe, x, d: setattr(c, "bonus", c.bonus - 1),
    "STREAMLINE": lambda f, c, foe, x, d: setattr(c, "cost_fight", max(0, c.cost() - 1)), "TURBO": _to_hand("VOID", where="discard"),
    "AUTO_SHIELDS": _auto_shields, "CHAOS": _chaos, "CHILL": _chill, "CONSUME": _consume, "DOUBLE_ENERGY": _double_energy,
    "EQUILIBRIUM": _equilibrium, "GENETIC_ALGORITHM": lambda f, c, foe, x, d: setattr(c, "bonus", c.bonus + (3 if c.up else 2)),
    "OVERCLOCK": _to_hand("BURN", where="discard"), "RECYCLE": _recycle, "REPROGRAM": _reprogram, "SCRAPE": _scrape, "SUNDER": _sunder,
    "WHITE_NOISE": _random_card("Power"), "ALL_FOR_ONE": _all_for_one, "AMPLIFY": _gain("AMPLIFY", 1, 2), "CORE_SURGE": lambda f, c, foe, x, d: f.pw.__setitem__("ARTIFACT", f.pw.get("ARTIFACT", 0) + 1),
    "FISSION": _fission, "HYPERBEAM": lambda f, c, foe, x, d: f.pw.__setitem__("FOCUS", f.pw.get("FOCUS", 0) - 3), "MULTI_CAST": _multi_cast,
    "RAINBOW": _rainbow, "REBOOT": _reboot, "SEEK": _seek, "HOLOGRAM": _hologram,
    "PROSTRATE": _mantra(2, 3), "PRAY": lambda f, c, foe, x, d: (f.gain_mantra(4 if c.up else 3), f.add_card(f.game.make("INSIGHT"), "draw")),
    "WORSHIP": _mantra(5, 5), "INNER_PEACE": _inner_peace, "INDIGNATION": _indignation, "HALT": _halt, "THIRD_EYE": _scry(3, 5), "JUST_LUCKY": _scry(1, 2),
    "CUT_THROUGH_FATE": _scry(2, 3), "MEDITATE": _meditate, "SPIRIT_SHIELD": _spirit_shield, "CONJURE_BLADE": _conjure,
    "DECEIVE_REALITY": _to_hand("SAFETY"), "EVALUATE": _to_hand("INSIGHT", where="draw"), "CARVE_REALITY": _to_hand("SMITE"),
    "REACH_HEAVEN": _to_hand("THROUGH_VIOLENCE", where="draw"), "ALPHA": _to_hand("BETA", where="draw"), "BETA": _to_hand("OMEGA", where="draw"),
    "WALLOP": _wallop, "JUDGMENT": _judgment, "PRESSURE_POINTS": _pressure_points, "TALK_TO_THE_HAND": _talk, "WAVE_OF_THE_HAND": _gain("WAVE_OF_THE_HAND", 1, 2),
    "SWIVEL": _swivel, "SIMMERING_FURY": _simmering, "WREATH_OF_FLAME": _gain("VIGOR", 5, 8), "WISH": _gain("STRENGTH", 3, 4), "VAULT": _vault, "SCRAWL": _scrawl,
    "BLASPHEMY": lambda f, c, foe, x, d: f.pw.__setitem__("BLASPHEMY_DUE", 1),
    "CONCLUDE": lambda f, c, foe, x, d: f.pw.__setitem__("TURN_OVER", 1),
    "APOTHEOSIS": _apotheosis, "PANIC_BUTTON": _panic, "ENLIGHTENMENT": _enlightenment, "DEEP_BREATH": _deep_breath,
    "SECRET_TECHNIQUE": _secret("Skill"), "SECRET_WEAPON": _secret("Attack"), "MADNESS": _madness, "THINKING_AHEAD": _thinking_ahead,
    "JACK_OF_ALL_TRADES": _random_card("Skill"), "DISCOVERY": _random_card("Attack"), "MAGNETISM": lambda f, c, foe, x, d: None,
    "FOREIGN_INFLUENCE": _random_card("Attack"), "PURITY": _exhaust_pick(3, "Choose up to {n} card(s) to Exhaust."),
}
POWER = {
    "DEMON_FORM": _power("DEMON_FORM", 2, 3), "METALLICIZE": _power("METALLICIZE", 3, 4), "BARRICADE": _power("BARRICADE", 1, 1),
    "FEEL_NO_PAIN": _power("FEEL_NO_PAIN", 3, 4), "DARK_EMBRACE": _power("DARK_EMBRACE", 1, 1), "CORRUPTION": _power("CORRUPTION", 1, 1),
    "JUGGERNAUT": _power("JUGGERNAUT", 5, 7), "EVOLVE": _power("EVOLVE", 1, 2), "FIRE_BREATHING": _power("FIRE_BREATHING", 6, 10),
    "RUPTURE": _power("RUPTURE", 1, 2), "COMBUST": _combust, "BRUTALITY": _power("BRUTALITY", 1, 1), "BERSERK": _berserk,
    "NOXIOUS_FUMES": _power("NOXIOUS_FUMES", 2, 3), "AFTER_IMAGE": _power("AFTER_IMAGE", 1, 1), "A_THOUSAND_CUTS": _power("A_THOUSAND_CUTS", 1, 2),
    "ACCURACY": _power("ACCURACY", 4, 6), "INFINITE_BLADES": _power("INFINITE_BLADES", 1, 1), "CALTROPS": _power("THORNS", 3, 5),
    "ENVENOM": _power("ENVENOM", 1, 1), "TOOLS_OF_THE_TRADE": _power("TOOLS_OF_THE_TRADE", 1, 1), "WRAITH_FORM": _wraith,
    "WELL_LAID_PLANS": _power("WELL_LAID_PLANS", 1, 2),
    "DEFRAGMENT": _power("FOCUS", 1, 2), "CAPACITOR": _capacitor, "LOOP": _power("LOOP", 1, 2), "STORM": _power("STORM", 1, 1),
    "STATIC_DISCHARGE": _power("STATIC_DISCHARGE", 1, 2), "HEATSINKS": _power("HEATSINKS", 1, 2), "HELLO_WORLD": _power("HELLO_WORLD", 1, 1),
    "CREATIVE_AI": _power("CREATIVE_AI", 1, 1), "MACHINE_LEARNING": _power("MACHINE_LEARNING", 1, 1), "BUFFER": _power("BUFFER", 1, 2),
    "ECHO_FORM": _power("ECHO_FORM", 1, 1), "ELECTRODYNAMICS": _electro, "BIASED_COGNITION": _biased, "SELF_REPAIR": _power("SELF_REPAIR", 7, 10),
    "MENTAL_FORTRESS": _power("MENTAL_FORTRESS", 4, 6), "RUSHDOWN": _power("RUSHDOWN", 2, 2), "LIKE_WATER": _power("LIKE_WATER", 5, 7),
    "NIRVANA": _power("NIRVANA", 3, 4), "DEVOTION": _power("DEVOTION", 2, 3), "DEVA_FORM": _deva, "BATTLE_HYMN": _power("BATTLE_HYMN", 1, 1),
    "STUDY": _power("STUDY", 1, 1), "FORESIGHT": _power("FORESIGHT", 3, 4), "ESTABLISHMENT": _power("ESTABLISHMENT", 1, 1),
    "MASTER_REALITY": _power("MASTER_REALITY", 1, 1), "OMEGA": _power("OMEGA", 50, 60), "PANACHE": _power("PANACHE", 10, 14),
    "SADISTIC_NATURE": _power("SADISTIC_NATURE", 5, 7), "MAYHEM": _mayhem, "MAGNETISM": _power("MAGNETISM", 1, 1),
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
    "DEUS_EX_MACHINA": lambda f, c: (f.hand.remove(c), f.exhaust(c), [f.add_card(f.game.make("MIRACLE")) for _ in range(3 if c.up else 2)]),
}
RETAINED = {
    "WINDMILL_STRIKE": lambda f, c: setattr(c, "bonus", c.bonus + (5 if c.up else 4)),
    "PERSEVERANCE": lambda f, c: setattr(c, "bonus", c.bonus + (3 if c.up else 2)),
    "SANDS_OF_TIME": lambda f, c: setattr(c, "cost_fight", max(0, c.cost() - 1)),
}


def _armaments(f, c):
    if c.up:
        for h in f.hand:
            h.up = True
        return
    pool = [h for h in f.hand if not h.up and h.type not in ("Status", "Curse")]
    for card in f.hand_pick("Choose a card to Upgrade.", 1, pool):
        card.up = True


def _random_discard(f):
    if f.hand:
        card = f.rng.choice(f.hand)
        f.hand.remove(card)
        f.discard.append(card)
        fx.discarded(f, card)
