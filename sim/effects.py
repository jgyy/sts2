"""What cards, powers, potions and enemy moves do, for the engine (engine.Fight).

A card does what its text says as the planner reads it (sts2bot.cards.parse): damage, Block, debuffs, draw,
energy, Strength, Poison, orbs, stances. Cards whose text says more than that, or says it under a condition,
have a handler in the game's content module: `PRE[key](fight, card, foe, info) -> info` adjusts what is read
before it is done, `AFTER[key](fight, card, foe, x, dealt)` adds the rest, `DAMAGE[key](fight, card) -> base`
works out damage the text does not print. Powers are kept in `fight.pw` under their ids and act through the
hooks below.
"""

from __future__ import annotations

import re

from .cards import Card

DEBUFF_FX = {"weak", "vuln", "frail", "status", "str_down", "dex_down", "focus_down", "draw_down", "entangle", "hex", "constrict", "confuse", "steal_gold"}
BUFF_FX = {"str", "str_all", "ritual", "block_ally", "block_all", "heal", "heal_all", "summon", "metallicize", "plated", "artifact", "thorns", "intangible", "enrage"}
BACK_TO_DRAW = {"TANTRUM"}


def add(d: dict, key, n):
    """d[key] += n (from 0); the new value."""
    d[key] = v = d.get(key, 0) + n
    return v


def foe_hook(foe, event: str, *args):
    """The monster's own rule for an event ("start", "hit", "death", "end" ...), if it has one; what it returns."""
    fn = foe.spec.hooks.get(event)
    return fn(*args) if fn else None


def card_rule(table: dict, f, c: Card, *args) -> None:
    """The game's handler in `table` (AFTER, POWER, DRAWN, RETAINED) for this card, if it has one."""
    fn = table.get(c.key)
    if fn is not None:
        fn(f, c, *args)


def by_text(f, key: str) -> bool:
    """A Power whose effect Slay the Spire 2's text triggers carry out (so the first game's handler for its name stays out)."""
    return any(k == key for _w, _t, k in f.triggers)


def base_damage(f, c: Card) -> int | None:
    """A card's damage a hit before Strength, or None if it deals none."""
    rule = f.game.DAMAGE.get(c.key)
    if rule is not None:
        return rule(f, c)
    return c.info.damage + c.bonus + (f.pw.get("ACCURACY", 0) if c.key == "SHIV" else 0) if c.info.damage else None


def can_play(f, c: Card) -> bool:
    rule = f.game.PLAYABLE.get(c.key)
    return rule(f, c) if rule else True


def move_damage(f, foe, m) -> int:
    return foe.mem.get(("dmg", foe.move_name), m.dmg)


def move_hits(f, foe, m) -> int:
    return foe.mem.get(("hits", foe.move_name), m.hits)


# ---------------------------------------------------------------- playing a card


def resolve(f, c: Card, foe, x: int) -> None:
    """Everything one play of `c` does (`x`: energy spent on an X card)."""
    g = f.game
    i = c.info
    pre = g.PRE.get(c.key)
    if pre is not None:
        i = pre(f, c, foe, i)
        if i is None:
            return
    xs = max(0, x + (1 if c.up and c.key in g.X_PLUS else 0)) if c.printed_cost == -1 else 1  # (1 for a card that is not X)
    dealt = 0
    base = base_damage(f, c)
    if base is not None and (i.damage or c.key in g.DAMAGE):
        dealt = attack(f, c, foe, base, i.hits * (xs if c.printed_cost == -1 and re.search(r"\bX times\b", c.text) else 1), i)
        if f.won or f.lost:
            after(f, c, foe, xs, dealt)
            return
    victims = f.alive() if i.aoe else [foe] if foe is not None and foe.alive else []
    for v in victims:
        if i.vulnerable:
            debuff(f, v, "VULNERABLE", i.vulnerable * xs)
        if i.weak:
            debuff(f, v, "WEAK", i.weak * xs)
        if i.enemy_strength_loss and not v.powers.get("ARTIFACT"):
            add(v.powers, "STRENGTH", -i.enemy_strength_loss * xs)
            if not i.enemy_strength_lasts:
                add(v.mem, "str_back", i.enemy_strength_loss * xs)
        elif i.enemy_strength_loss:
            v.powers["ARTIFACT"] -= 1
    if i.poison and not i.random_target:
        for v in victims or ([foe] if foe else []):
            if v is not None and v.alive and (not i.poison_needs or v.powers.get("POISON")):
                debuff(f, v, "POISON", i.poison * xs)
    elif i.poison:
        for _ in range(max(1, xs)):
            alive = f.alive()
            if alive:
                debuff(f, f.rng.choice(alive), "POISON", i.poison)
    if i.block and not i.melody_block:
        for _ in range(xs):
            f.gain_block(f.block_value(i.block + c.bonus))
    if i.hp_loss:
        f.lose_hp(i.hp_loss, from_card=True)
        if f.lost:
            return
    if i.heal:
        f.heal(i.heal)
    if i.strength:
        add(f.pw if i.strength_lasts or c.type == "Power" else f.temp, "STRENGTH", i.strength * xs)
    if i.dexterity:
        add(f.pw if i.dexterity_lasts or c.type == "Power" else f.temp, "DEXTERITY", i.dexterity)
    if i.energy_drain:
        add(f.pw, "ENERGY_DOWN", i.energy_drain)
    if i.energy_gain:
        f.energy += i.energy_gain * xs
    if i.shivs:
        f.add_new("SHIV", i.shivs * xs, up=c.key == "CUNNING" or f.pw.get("MASTER_REALITY", 0) > 0)
    for kind in i.channel * xs:
        f.channel(kind)
    if i.evoke:
        f.evoke(i.evoke * xs)
    if i.evoke_all:
        while f.orbs:
            f.evoke(i.evoke_all)
    if i.stance:
        f.enter(i.stance)
    if i.next_energy:
        add(f.pw, "ENERGY_NEXT_TURN", i.next_energy * xs)
    if i.next_draw:
        add(f.pw, "DRAW_CARDS_NEXT_TURN", i.next_draw * xs)
    if i.next_block:
        add(f.pw, "NEXT_BLOCK", i.next_block)
    if i.enemy_hp_loss:
        for v in victims:
            f.hurt(v, i.enemy_hp_loss, attack=False)
    if i.draw:
        f.draw_cards(i.draw * xs)
    g.EXTRA(f, c, foe, i, xs)  # the game's own resources (Stars, Osty, Doom, Souls, Forge, Mantra)
    after(f, c, foe, xs, dealt)


def after(f, c: Card, foe, xs: int, dealt: int) -> None:
    card_rule(f.game.AFTER, f, c, foe, xs, dealt)
    if c.type == "Power":
        card_rule(f.game.POWER, f, c)


def attack(f, c: Card, foe, base: int, hits: int, i) -> int:
    """The hits of an attack; the HP they took off."""
    dealt = 0
    osty, atk = i.osty_attack, c.type == "Attack"
    if osty and f.osty == 0:
        return 0
    double = 2 if f.pw.get("DOUBLE_DAMAGE") and atk else 1
    if atk and f.run.counters.get("PEN_NIB", 0) >= 9 and f.run.has("PEN_NIB"):
        double *= 2
    for _ in range(max(0, hits)):
        alive = f.alive()
        targets = alive if i.aoe else ([f.rng.choice(alive)] if alive else []) if i.random_target else [foe] if foe is not None and foe.alive else []
        for t in targets:
            before = t.hp
            dealt += f.hurt(t, f.attack_value(base, t, osty, plain=not atk) * double, attack=atk)
            if atk and t.powers.get("THORNS") and not osty:
                f.take(t.powers["THORNS"], t, attack=False)
            if atk and t.powers.get("PERSONAL_HIVE"):
                f.add_new("DAZED", t.powers["PERSONAL_HIVE"], "draw")
            if atk and f.pw.get("REAPER_FORM") and t.alive and before > t.hp:
                debuff(f, t, "DOOM", before - t.hp)
            if atk and f.pw.get("ENVENOM") and t.hp < before and t.alive:
                debuff(f, t, "POISON", f.pw["ENVENOM"])
            if atk and t.mem.get("block_return"):
                f.gain_block(t.mem["block_return"])
        if f.won or f.lost:
            break
    if atk and f.pw.get("VIGOR"):
        f.pw.pop("VIGOR")
    return dealt


def debuff(f, foe, power: str, n: int) -> None:
    if n <= 0 or foe is None:
        return
    if foe.powers.get("ARTIFACT"):
        foe.powers["ARTIFACT"] -= 1
        return
    add(foe.powers, power, n)
    if f.pw.get("SADISTIC_NATURE"):
        f.hurt(foe, f.pw["SADISTIC_NATURE"], attack=False)


def debuff_all(f, power: str, n: int) -> None:
    for foe in f.alive():
        debuff(f, foe, power, n)


def debuff_me(f, power: str, n: int) -> None:
    if f.pw.get("ARTIFACT"):
        f.pw["ARTIFACT"] -= 1
        return
    if power == "WEAK" and f.run.has("GINGER") or power == "FRAIL" and f.run.has("TURNIP"):
        return
    add(f.pw, power, n)


def played(f, c: Card) -> None:
    """What happens after any card is played: powers and relics that count cards, and the enemies that react."""
    pw, run = f.pw, f.run
    if pw.get("AFTER_IMAGE"):
        f.gain_block(pw["AFTER_IMAGE"])
    if pw.get("A_THOUSAND_CUTS"):
        f.hurt_all(pw["A_THOUSAND_CUTS"])
    for foe in f.alive():
        if foe.mem.get("choke"):
            f.hurt(foe, foe.mem["choke"], attack=False)
    if pw.get("PANACHE") and len(f.turn_cards) % 5 == 0:
        f.hurt_all(pw["PANACHE"])
    if c.type == "Attack":
        if pw.get("RAGE"):
            f.gain_block(pw["RAGE"])
        if f.attacks_turn % 3 == 0:  # (Shuriken, Kunai, Ornamental Fan: every third Attack in one turn)
            if run.has("SHURIKEN"):
                add(pw, "STRENGTH", 1)
            if run.has("KUNAI"):
                add(pw, "DEXTERITY", 1)
            if run.has("ORNAMENTAL_FAN"):
                f.gain_block(4)
        if run.has("PEN_NIB"):
            run.counters["PEN_NIB"] = (run.counters.get("PEN_NIB", 0) + 1) % 10
    if c.type == "Skill":
        if run.has("LETTER_OPENER") and f.skills_turn % 3 == 0:
            f.hurt_all(5)
        for foe in f.alive():
            if foe.powers.get("ENRAGE"):
                add(foe.powers, "STRENGTH", foe.powers["ENRAGE"])
    if pw.get("MONOLOGUE"):
        add(f.temp, "STRENGTH", 1)
    if pw.get("TENDER"):
        add(f.temp, "STRENGTH", -1)
        add(f.temp, "DEXTERITY", -1)
    for foe in f.alive():
        if foe.mem.get("oblivion"):
            debuff(f, foe, "DOOM", foe.mem["oblivion"])
        if c.type == "Skill" and foe.powers.get("VITAL_SPARK"):
            add(foe.mem, "spark", foe.powers["VITAL_SPARK"])
    f.game_hook("play", c)
    if c.type != "Attack" and pw.get("HEX_DAZED"):
        f.add_new("DAZED", pw["HEX_DAZED"], "draw")
    if c.type == "Power":
        for foe in f.alive():
            if foe.powers.get("CURIOSITY"):
                add(foe.powers, "STRENGTH", foe.powers["CURIOSITY"])
        if pw.get("STORM") and not by_text(f, "STORM"):
            for _ in range(pw["STORM"]):
                f.channel("LIGHTNING")
        if pw.get("HEATSINKS") and not by_text(f, "HEATSINKS"):
            f.draw_cards(pw["HEATSINKS"])
    if run.has("NUNCHAKU") and c.type == "Attack" and add(run.counters, "NUNCHAKU", 1) % 10 == 0:
        f.energy += 1
    if run.has("INK_BOTTLE") and add(run.counters, "INK_BOTTLE", 1) % 10 == 0:
        f.draw_cards(1)
    for foe in f.alive():
        if foe.powers.get("TIME_WARP") is not None and foe.alive and add(foe.powers, "TIME_WARP", 1) >= 12:
            foe.powers["TIME_WARP"] = 0
            for x in f.alive():
                add(x.powers, "STRENGTH", 2)
            f.pw["TURN_OVER"] = 1
        if foe.powers.get("SHARP_HIDE") and c.type == "Attack":
            f.take(foe.powers["SHARP_HIDE"], foe, attack=False)
    if any(h.key == "PAIN" for h in f.hand):
        f.lose_hp(sum(h.key == "PAIN" for h in f.hand))


def drawn(f, c: Card) -> None:
    if f.pw.get("CORROSIVE_WAVE"):
        debuff_all(f, "POISON", f.pw["CORROSIVE_WAVE"])
    if c.key == "VOID":
        f.energy = max(0, f.energy - 1)
    if c.type == "Status" and f.pw.get("EVOLVE"):
        f.draw_cards(f.pw["EVOLVE"])
    if c.type in ("Status", "Curse") and f.pw.get("FIRE_BREATHING"):
        f.hurt_all(f.pw["FIRE_BREATHING"])
    if f.pw.get("CONFUSED") and c.cost() >= 0:
        c.cost_fight = f.rng.randint(0, 3)
    card_rule(f.game.DRAWN, f, c)


def exhausted(f, c: Card) -> None:
    f.game_hook("exhaust", c)
    if f.pw.get("FEEL_NO_PAIN") and f.game.KEY == "sts1":
        f.gain_block(f.pw["FEEL_NO_PAIN"])
    if f.pw.get("DARK_EMBRACE") and f.game.KEY == "sts1":
        f.draw_cards(f.pw["DARK_EMBRACE"])
    if c.key == "SENTINEL":
        f.energy += 3 if c.up else 2
    if f.run.has("CHARON_S_ASHES"):
        f.hurt_all(3)
    if f.run.has("DEAD_BRANCH") and (pool := f.game.pool(f.run.character)):
        f.add_card(Card(f.rng.choice(pool)))


def discarded(f, c: Card) -> None:
    """A card discarded from the hand by a card's own say (not at the end of the turn)."""
    add(f.pw, "DISCARDED_TURN", 1)
    if c.has("Sly") and c in f.discard:  # (Slay the Spire 2: a Sly card discarded from the hand is played for free)
        f.discard.remove(c)
        alive = f.alive()
        resolve(f, c, f.rng.choice(alive) if alive else None, f.energy if c.printed_cost == -1 else 0)
        played(f, c)
        f.put_away(c)
    if c.key == "REFLEX":
        f.draw_cards(3 if c.up else 2)
    elif c.key == "TACTICIAN":
        f.energy += 2 if c.up else 1
    for card in f.hand + f.draw + f.discard:
        if card.key == "EVISCERATE" and card.cost() > 0:
            card.cost_turn = max(0, card.cost() - 1)


def discard_from_hand(f, n: int, prompt: str = "Choose {n} card(s) to Discard.") -> int:
    picked = f.hand_pick(prompt.format(n=n), n)
    for c in picked:
        if c in f.hand:
            f.hand.remove(c)
            f.discard.append(c)
            discarded(f, c)
    return len(picked)


def block_gained(f, n: int) -> None:
    f.game_hook("block")
    if f.pw.get("JUGGERNAUT") and f.alive() and f.game.KEY == "sts1":
        f.hurt(f.rng.choice(f.alive()), f.pw["JUGGERNAUT"], attack=False)
    if f.pw.get("WAVE_OF_THE_HAND"):
        debuff_all(f, "WEAK", f.pw["WAVE_OF_THE_HAND"])


def stance_changed(f, new: str) -> None:
    if f.pw.get("MENTAL_FORTRESS"):
        f.gain_block(f.pw["MENTAL_FORTRESS"])
    if new == "WRATH" and f.pw.get("RUSHDOWN"):
        f.draw_cards(f.pw["RUSHDOWN"])


def retained(f, c: Card) -> None:
    card_rule(f.game.RETAINED, f, c)
    if f.pw.get("ESTABLISHMENT") and c.cost() > 0:
        c.cost_fight = max(0, c.cost() - f.pw["ESTABLISHMENT"])


def scry(f, n: int) -> None:
    """Look at the top n cards: the dead ones (statuses, curses, Strikes and Defends when the deck is fine) go."""
    if n <= 0:
        return
    f.refill()
    for c in f.draw[-n:]:
        if c.type in ("Status", "Curse"):
            f.draw.remove(c)
            f.discard.append(c)
    if f.pw.get("NIRVANA"):
        f.gain_block(f.pw["NIRVANA"])
    for c in [c for c in f.discard if c.key == "WEAVE"]:
        f.discard.remove(c)
        f.add_card(c)


def on_death(f) -> None:
    run = f.run
    fairy = next((i for i, p in enumerate(run.potions) if p is not None and p.key == "FAIRY_IN_A_BOTTLE"), None)
    if fairy is not None:
        run.drop_potion(fairy)
        run.hp = max(1, int(run.max_hp * 0.3))
        return
    if run.has("LIZARD_TAIL") and not run.counters.get("LIZARD_TAIL_USED"):
        run.counters["LIZARD_TAIL_USED"] = 1
        run.hp = max(1, run.max_hp // 2)
        return
    run.hp = 0
    f.lost = True


def foe_hurt(f, foe, amount: int, attack: bool) -> None:
    if attack and foe.powers.get("ANGRY"):
        add(foe.powers, "STRENGTH", foe.powers["ANGRY"])
    if attack and foe.powers.get("MALLEABLE"):
        foe.block += foe.powers["MALLEABLE"]
        foe.powers["MALLEABLE"] += 1
    if attack and foe.powers.get("FLUTTER"):
        foe.powers["FLUTTER"] -= 1
        if foe.powers["FLUTTER"] <= 0:
            foe.powers.pop("FLUTTER")
            foe_hook(foe, "grounded", f, foe)
    if foe.powers.get("SHIFTING"):
        add(foe.powers, "STRENGTH", -amount)
        add(foe.mem, "str_back", amount)
    if foe.powers.get("ASLEEP"):
        foe.powers.pop("ASLEEP")
        foe_hook(foe, "wake", f, foe)
    foe_hook(foe, "hit", f, foe, amount, attack)


# ---------------------------------------------------------------- turns


def fight_start(f) -> None:
    f.game.START(f)
    for rid in f.run.relics:
        rule = RELIC_START.get(rid)
        if rule:
            rule(f)
    for foe in f.foes:
        foe_hook(foe, "start", f, foe)


RELIC_START = {
    "ANCHOR": lambda f: f.gain_block(10), "BLOOD_VIAL": lambda f: f.heal(2), "MUTAGENIC_STRENGTH": lambda f: add(f.temp, "STRENGTH", 3),
    "BAG_OF_MARBLES": lambda f: debuff_all(f, "VULNERABLE", 1), "RED_MASK": lambda f: debuff_all(f, "WEAK", 1), "TWISTED_FUNNEL": lambda f: debuff_all(f, "POISON", 4),
    "PHILOSOPHER_S_STONE": lambda f: [add(x.powers, "STRENGTH", 1) for x in f.alive()], "MARK_OF_PAIN": lambda f: f.add_new("WOUND", 2, "draw"),
    "PRESERVED_INSECT": lambda f: [setattr(x, "hp", int(x.hp * 0.75)) for x in f.alive()] if f.kind == "elite" else None,
    "NUCLEAR_BATTERY": lambda f: f.channel("PLASMA"), "SYMBIOTIC_VIRUS": lambda f: f.channel("DARK"),
    "GIRYA": lambda f: add(f.pw, "STRENGTH", f.run.counters.get("GIRYA", 0)),
    **{r: (lambda f, k=k, n=n: add(f.pw, k, n)) for r, k, n in (("VAJRA", "STRENGTH", 1), ("ODDLY_SMOOTH_STONE", "DEXTERITY", 1), ("BRONZE_SCALES", "THORNS", 3),
                                                              ("AKABEKO", "VIGOR", 8), ("THREAD_AND_NEEDLE", "PLATING", 4), ("CLOCKWORK_SOUVENIR", "ARTIFACT", 1),
                                                              ("FOSSILIZED_HELIX", "BUFFER", 1), ("DATA_DISK", "FOCUS", 1))},
}


def extra_draw(f) -> int:
    run, n = f.run, 0
    if f.round == 1:
        n += 2 * run.has("RING_OF_THE_SNAKE") + 2 * run.has("BAG_OF_PREPARATION")
    n += 2 * run.has("SNECKO_EYE") + run.has("RING_OF_THE_SERPENT") + f.pw.get("MACHINE_LEARNING", 0)
    return n


def turn_start(f) -> None:
    pw, run = f.pw, f.run
    if pw.get("NEXT_BLOCK"):
        f.gain_block(pw.pop("NEXT_BLOCK"))
    if f.round == 1:
        f.energy += run.has("LANTERN")
        if run.has("PANTOGRAPH") and f.kind == "boss":
            f.heal(25)
    if f.round == 2 and run.has("HORN_CLEAT"):
        f.gain_block(14)
    if f.round == 3 and run.has("CAPTAIN_S_WHEEL"):
        f.gain_block(18)
    if run.has("HAPPY_FLOWER") and add(run.counters, "HAPPY_FLOWER", 1) % 3 == 0:
        f.energy += 1
    if run.has("MERCURY_HOURGLASS"):
        f.hurt_all(3)
    if run.has("INCENSE_BURNER") and add(run.counters, "INCENSE_BURNER", 1) % 6 == 0:
        add(pw, "INTANGIBLE", 1)
    f.energy += sum(1 for o in f.orbs if o[0] == "PLASMA")
    if pw.get("DEMON_FORM") and not by_text(f, "DEMON_FORM"):
        add(pw, "STRENGTH", pw["DEMON_FORM"])
    if pw.get("NOXIOUS_FUMES") and not by_text(f, "NOXIOUS_FUMES"):
        debuff_all(f, "POISON", pw["NOXIOUS_FUMES"])
    if pw.get("BRUTALITY"):
        f.lose_hp(pw["BRUTALITY"])
        f.draw_cards(pw["BRUTALITY"])
    if pw.get("BERSERK"):
        f.energy += pw["BERSERK"]
    if pw.get("INFINITE_BLADES") and not by_text(f, "INFINITE_BLADES"):
        f.add_new("SHIV", pw["INFINITE_BLADES"])
    if pw.get("WRAITH_FORM"):
        add(pw, "DEXTERITY", -pw["WRAITH_FORM"])
    if pw.get("BIASED_COGNITION"):
        add(pw, "FOCUS", -pw["BIASED_COGNITION"])
    if pw.get("LOOP") and f.orbs:
        for _ in range(pw["LOOP"]):
            passive(f, f.orbs[0])
    if pw.get("DEVOTION"):
        f.gain_mantra(pw["DEVOTION"])
    if pw.get("DEVA_FORM"):
        f.energy += add(pw, "DEVA_ENERGY", pw["DEVA_FORM"])
    if pw.get("BATTLE_HYMN"):
        f.add_new("SMITE", pw["BATTLE_HYMN"])
    if pw.pop("SIMMERING", 0):
        f.enter("WRATH")
    if pw.get("BLASPHEMY_DUE"):
        f.lose_hp(f.run.hp)
    f.game.TURN_START(f)


def after_draw(f) -> None:
    pw = f.pw
    if pw.get("TOOLS_OF_THE_TRADE") and not by_text(f, "TOOLS_OF_THE_TRADE"):
        f.draw_cards(pw["TOOLS_OF_THE_TRADE"])
        discard_from_hand(f, pw["TOOLS_OF_THE_TRADE"])
    if pw.get("FORESIGHT"):
        scry(f, pw["FORESIGHT"])
    if pw.get("CREATIVE_AI") or pw.get("HELLO_WORLD") or pw.get("MAYHEM"):
        f.game.TURN_CARDS(f)


def passive(f, o: list) -> None:
    if o[0] == "DARK":
        o[1] += f.orb_passive(o)
    elif o[0] != "PLASMA":
        f.orb_effect(o, f.orb_passive(o), evoke=False)


def turn_end(f) -> None:
    pw, run = f.pw, f.run
    if pw.get("METALLICIZE"):
        f.gain_block(pw["METALLICIZE"])
    if pw.get("PLATING"):
        f.gain_block(pw["PLATING"])
    if pw.get("LIKE_WATER") and f.stance == "CALM":
        f.gain_block(pw["LIKE_WATER"])
    if pw.get("COMBUST"):
        f.lose_hp(pw.get("COMBUST_HP", 1))
        f.hurt_all(pw["COMBUST"])
    if pw.get("OMEGA"):
        f.hurt_all(pw["OMEGA"])
    if pw.get("STUDY"):
        f.add_new("INSIGHT", pw["STUDY"], "draw")
    for o in list(f.orbs):
        if f.won:
            break
        passive(f, o)
    if run.has("FROZEN_CORE") and len(f.orbs) < f.orb_slots:
        f.channel("FROST")
    if run.has("ORICHALCUM") and f.block == 0:
        f.gain_block(6)
    if run.has("STONE_CALENDAR") and f.round == 7:
        f.hurt_all(52)
    if pw.get("CONSTRICT"):
        f.take(pw["CONSTRICT"], attack=False)
    for c in list(f.hand):
        if c.key in ("BURN", "DECAY"):
            f.take(4 if c.up and c.key == "BURN" else 2, attack=False)
        elif c.key == "REGRET":
            f.lose_hp(len(f.hand))
        elif c.key in ("DOUBT", "SHAME"):
            debuff_me(f, "WEAK" if c.key == "DOUBT" else "FRAIL", 1)
    if pw.get("WAVE_OF_THE_HAND"):
        pw.pop("WAVE_OF_THE_HAND")
    if pw.get("DOUBLE_DAMAGE_NEXT"):
        pw["DOUBLE_DAMAGE"] = pw.pop("DOUBLE_DAMAGE_NEXT")
    elif pw.get("DOUBLE_DAMAGE"):
        pw.pop("DOUBLE_DAMAGE")
    if pw.get("BLUR"):
        pw["KEEP_BLOCK"] = 1
        pw["BLUR"] -= 1
    f.game.TURN_END(f)


def foe_round_end(f, foe) -> None:
    if foe.powers.get("RITUAL"):
        if foe.mem.get("ritual_ready"):
            add(foe.powers, "STRENGTH", foe.powers["RITUAL"])
        foe.mem["ritual_ready"] = True
    foe.block += foe.powers.get("METALLICIZE", 0) + foe.powers.get("PLATING", 0)
    if foe.powers.get("REGEN"):
        foe.hp = min(foe.max_hp, foe.hp + foe.powers["REGEN"])
    if foe.powers.get("MALLEABLE"):
        foe.powers["MALLEABLE"] = foe.mem.setdefault("malleable", foe.powers["MALLEABLE"])
    foe_hook(foe, "end", f, foe)


# ---------------------------------------------------------------- enemies


def foe_acts(f, foe, m) -> None:
    if m.kind in ("Sleep", "Stun", "Unknown") and not m.dmg:
        for fx in m.fx:
            do(f, foe, fx)
        return
    if m.dmg:
        base, hits = move_damage(f, foe, m), move_hits(f, foe, m)
        hp0 = f.run.hp
        for _ in range(hits):
            if f.lost or not foe.alive:
                break
            lost = f.take(f.foe_hit(foe, base), foe)
            if lost and f.pw.get("STATIC_DISCHARGE"):
                for _ in range(f.pw["STATIC_DISCHARGE"]):
                    f.channel("LIGHTNING")
            if lost and (foe.mem.get("painful_stabs") or foe.powers.get("PAINFUL_STABS")):
                f.add_new("WOUND", 1, "discard")
        if foe.powers.get("IMBALANCED") and f.run.hp == hp0:
            foe.mem["off_balance"] = True
    if m.block:
        foe.block += m.block
    for fx in m.fx:
        if f.lost:
            return
        do(f, foe, fx)
    foe_hook(foe, "acted", f, foe)


def do(f, foe, fx: tuple) -> None:
    """One effect of an enemy's move."""
    word, *args = fx
    n = args[0] if args else 0
    alive = f.alive()
    if word == "str":
        add(foe.powers, "STRENGTH", n)
    elif word == "str_all":
        for x in alive:
            add(x.powers, "STRENGTH", n)
    elif word in ("ritual", "metallicize", "plated", "artifact", "thorns", "intangible", "enrage", "regen", "curiosity"):
        add(foe.powers, {"plated": "PLATING"}.get(word, word.upper()), n)
    elif word == "block_ally":
        f.rng.choice([x for x in alive if x is not foe] or [foe]).block += n
    elif word == "block_all":
        for x in alive:
            x.block += n
    elif word in ("weak", "vuln", "frail"):
        debuff_me(f, {"weak": "WEAK", "vuln": "VULNERABLE", "frail": "FRAIL"}[word], n)
    elif word == "status":
        key, count, pile = args[0], args[1], args[2] if len(args) > 2 else "discard"
        f.add_new(key, count, pile, up=bool(foe.mem.get("burn_up")) and key == "BURN")
    elif word == "heal":
        foe.hp = min(foe.max_hp, foe.hp + n)
    elif word == "heal_all":
        for x in alive:
            x.hp = min(x.max_hp, x.hp + n)
    elif word == "summon":
        for _ in range(args[1] if len(args) > 1 else 1):
            if sum(1 for x in f.foes if x.alive) < 5:
                f.foes.append(f.game.spawn(args[0], f, minion=True))
                f.foes[-1].choose(f.rng)
    elif word in ("str_down", "dex_down", "focus_down"):
        add(f.pw, {"str_down": "STRENGTH", "dex_down": "DEXTERITY", "focus_down": "FOCUS"}[word], -n)
    elif word == "draw_down":
        f.pw["DRAW_REDUCTION"] = n
    elif word in ("entangle", "hex", "confuse"):
        f.pw[{"entangle": "ENTANGLED", "hex": "HEX_DAZED", "confuse": "CONFUSED"}[word]] = 1
    elif word == "constrict":
        add(f.pw, "CONSTRICT", n)
    elif word == "steal_gold":
        took = min(f.run.gold, n)
        f.run.gold -= took
        add(foe.mem, "stolen", took)
    elif word == "remove_debuffs":
        for k in ("WEAK", "VULNERABLE", "POISON"):
            foe.powers.pop(k, None)
        if foe.powers.get("STRENGTH", 0) < 0:
            foe.powers["STRENGTH"] = 0
    elif word == "die":
        f.kill(foe)
    elif word == "damage_all":  # an explosion
        f.take(n, foe, attack=False)
    elif word == "call":
        args[0](f, foe)
    elif word == "power":
        add(foe.powers, args[0], args[1])
    elif word == "player_power":
        add(f.pw, args[0], args[1])


# ---------------------------------------------------------------- potions


def potion_usable(f, pot) -> bool:
    return pot.key not in ("FAIRY_IN_A_BOTTLE", "SMOKE_BOMB")


def drink(f, pot, foe) -> None:
    t = pot.text
    low = t.lower()
    sacred = 2 if f.run.has("SACRED_BARK") else 1
    num = lambda pat: (int(m.group(1)) * sacred) if (m := re.search(pat, t, re.I)) else 0
    if (n := num(r"deal (\d+) damage to all enemies")):
        f.hurt_all(n)
    elif (n := num(r"deal (\d+) damage")) and foe is not None:
        f.hurt(foe, n, attack=False)
    if (n := num(r"gain (\d+) block")):
        f.gain_block(n)
    for word, power in (("strength", "STRENGTH"), ("dexterity", "DEXTERITY"), ("focus", "FOCUS"), ("artifact", "ARTIFACT"), ("plated armor", "PLATING"),
                        ("metallicize", "METALLICIZE"), ("thorns", "THORNS"), ("regen", "REGEN"), ("intangible", "INTANGIBLE"), ("ritual", "RITUAL_ME")):
        if (n := num(rf"gain (\d+) {word}")):
            add(f.temp if power in ("STRENGTH", "DEXTERITY") and "at the end of your turn" in low else f.pw, power, n)
    for word, power in (("weak", "WEAK"), ("vulnerable", "VULNERABLE"), ("poison", "POISON")):
        if (n := num(rf"apply (\d+) {word}")):
            for x in (f.alive() if "all enemies" in low else [foe] if foe else []):
                debuff(f, x, power, n)
    if (n := num(r"draw (\d+) cards")):
        f.draw_cards(n)
    if (n := num(r"gain (\d+) energy")) or (n := len(re.findall(r"energy_icon", t)) * sacred):
        f.energy += n
    if (n := num(r"heal (\d+)% of your max hp")):
        f.heal(f.run.max_hp * n // 100)
    if (n := num(r"gain (\d+) max hp")):
        f.run.max_hp += n
        f.heal(n)
    if (n := num(r"add (\d+) upgraded shivs")):
        f.add_new("SHIV", n, up=True)
    if "miracles" in low:
        f.add_new("MIRACLE", 2 * sacred)
    if "enter divinity" in low:
        f.enter("DIVINITY")
    elif "enter calm or wrath" in low:
        f.enter("WRATH")
    if "next card is played twice" in low:
        add(f.pw, "DUPLICATION", sacred)
    if (n := num(r"gain (\d+) orb slots")):
        f.orb_slots += n
    if "channel 1 dark for each orb slot" in low:
        for _ in range(f.orb_slots):
            f.channel("DARK")
    if "play the top 3 cards" in low:
        for _ in range(3 * sacred):
            f.refill()
            if f.draw and not f.won:
                c = f.draw.pop()
                resolve(f, c, (f.alive() or [None])[0], f.energy)
                f.put_away(c)
    if "upgrade all cards in your hand" in low:
        for c in f.hand:
            c.up = True
    if "add a random" in low or "choose 1 of 3" in low:
        f.game.POTION_CARD(f, pot)
