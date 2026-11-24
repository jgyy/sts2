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


def content(f):
    return f.game


def by_text(f, key: str) -> bool:
    """A Power whose effect Slay the Spire 2's text triggers carry out (so the first game's handler for its name stays out)."""
    return any(k == key for _w, _t, k in f.triggers)


def base_damage(f, c: Card) -> int | None:
    """A card's damage a hit before Strength, or None if it deals none."""
    rule = content(f).DAMAGE.get(c.key)
    if rule is not None:
        return rule(f, c)
    i = c.info
    if not i.damage:
        return None
    return i.damage + c.bonus + (f.pw.get("ACCURACY", 0) if c.key == "SHIV" else 0)


def can_play(f, c: Card) -> bool:
    rule = content(f).PLAYABLE.get(c.key)
    return rule(f, c) if rule else True


def move_damage(f, foe, m) -> int:
    return foe.mem.get(("dmg", foe.move_name), m.dmg)


def move_hits(f, foe, m) -> int:
    return foe.mem.get(("hits", foe.move_name), m.hits)


# ---------------------------------------------------------------- playing a card


def resolve(f, c: Card, foe, x: int) -> None:
    """Everything one play of `c` does (`x`: energy spent on an X card)."""
    g = content(f)
    i = c.info
    pre = g.PRE.get(c.key)
    if pre is not None:
        i = pre(f, c, foe, i)
        if i is None:
            return
    xs = max(0, x + (1 if c.up and c.key in g.X_PLUS else 0)) if c.printed_cost == -1 else 1
    dealt = 0
    base = base_damage(f, c)
    if base is not None and (i.damage or c.key in g.DAMAGE):
        hits = i.hits * (xs if c.printed_cost == -1 and re.search(r"\bX times\b", c.text) else 1)
        dealt = attack(f, c, foe, base, hits, i)
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
            n = i.enemy_strength_loss * xs
            v.powers["STRENGTH"] = v.powers.get("STRENGTH", 0) - n
            if not i.enemy_strength_lasts:
                v.mem["str_back"] = v.mem.get("str_back", 0) + n
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
        for _ in range(xs if c.printed_cost == -1 else 1):
            f.gain_block(f.block_value(i.block + c.bonus))
    if i.hp_loss:
        f.lose_hp(i.hp_loss, from_card=True)
        if f.lost:
            return
    if i.heal:
        f.heal(i.heal)
    if i.strength:
        if i.strength_lasts or c.type == "Power":
            f.pw["STRENGTH"] = f.pw.get("STRENGTH", 0) + i.strength * xs
        else:
            f.temp["STRENGTH"] = f.temp.get("STRENGTH", 0) + i.strength * xs
    if i.dexterity:
        if i.dexterity_lasts or c.type == "Power":
            f.pw["DEXTERITY"] = f.pw.get("DEXTERITY", 0) + i.dexterity
        else:
            f.temp["DEXTERITY"] = f.temp.get("DEXTERITY", 0) + i.dexterity
    if i.energy_drain:
        f.pw["ENERGY_DOWN"] = f.pw.get("ENERGY_DOWN", 0) + i.energy_drain
    if i.energy_gain:
        f.energy += i.energy_gain * (xs if c.printed_cost == -1 else 1)
    if i.shivs:
        for _ in range(i.shivs * xs):
            f.add_card(g.make("SHIV", up=c.key == "CUNNING" or f.pw.get("MASTER_REALITY", 0) > 0))
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
        f.pw["ENERGY_NEXT_TURN"] = f.pw.get("ENERGY_NEXT_TURN", 0) + i.next_energy * xs
    if i.next_draw:
        f.pw["DRAW_CARDS_NEXT_TURN"] = f.pw.get("DRAW_CARDS_NEXT_TURN", 0) + i.next_draw * xs
    if i.next_block:
        f.pw["NEXT_BLOCK"] = f.pw.get("NEXT_BLOCK", 0) + i.next_block
    if i.enemy_hp_loss:
        for v in victims:
            f.hurt(v, i.enemy_hp_loss, attack=False)
    if i.draw:
        f.draw_cards(i.draw * (xs if c.printed_cost == -1 else 1))
    g.EXTRA(f, c, foe, i, xs)  # the game's own resources (Stars, Osty, Doom, Souls, Forge, Mantra)
    after(f, c, foe, xs, dealt)


def after(f, c: Card, foe, xs: int, dealt: int) -> None:
    rule = content(f).AFTER.get(c.key)
    if rule is not None:
        rule(f, c, foe, xs, dealt)
    if c.type == "Power":
        rule = content(f).POWER.get(c.key)
        if rule is not None:
            rule(f, c)


def attack(f, c: Card, foe, base: int, hits: int, i) -> int:
    """The hits of an attack; the HP they took off."""
    dealt = 0
    osty = i.osty_attack
    if osty and f.osty == 0:
        return 0
    double = 2 if f.pw.get("DOUBLE_DAMAGE") and c.type == "Attack" else 1
    if c.type == "Attack" and f.run.counters.get("PEN_NIB", 0) >= 9 and f.run.has("PEN_NIB"):
        double *= 2
    for _ in range(max(0, hits)):
        if i.aoe:
            targets = f.alive()
        elif i.random_target:
            alive = f.alive()
            targets = [f.rng.choice(alive)] if alive else []
        else:
            targets = [foe] if foe is not None and foe.alive else []
        for t in targets:
            before = t.hp
            dealt += f.hurt(t, f.attack_value(base, t, osty, plain=c.type not in ("Attack",)) * double, attack=c.type == "Attack")
            if c.type == "Attack" and t.powers.get("THORNS") and not osty:
                f.take(t.powers["THORNS"], t, attack=False)
            if c.type == "Attack" and t.powers.get("PERSONAL_HIVE"):
                for _ in range(t.powers["PERSONAL_HIVE"]):
                    f.add_card(content(f).make("DAZED"), "draw")
            if c.type == "Attack" and f.pw.get("REAPER_FORM") and t.alive and before > t.hp:
                debuff(f, t, "DOOM", before - t.hp)
            if c.type == "Attack" and f.pw.get("ENVENOM") and t.hp < before and t.alive:
                debuff(f, t, "POISON", f.pw["ENVENOM"])
            if c.type == "Attack" and t.mem.get("block_return"):
                f.gain_block(t.mem["block_return"])
        if f.won or f.lost:
            break
    if c.type == "Attack" and f.pw.get("VIGOR"):
        f.pw.pop("VIGOR")
    return dealt


def debuff(f, foe, power: str, n: int) -> None:
    if n <= 0 or foe is None:
        return
    if power != "POISON" and foe.powers.get("ARTIFACT"):
        foe.powers["ARTIFACT"] -= 1
        return
    if power == "POISON" and foe.powers.get("ARTIFACT"):
        foe.powers["ARTIFACT"] -= 1
        return
    foe.powers[power] = foe.powers.get(power, 0) + n
    if f.pw.get("SADISTIC_NATURE"):
        f.hurt(foe, f.pw["SADISTIC_NATURE"], attack=False)


def debuff_me(f, power: str, n: int) -> None:
    if f.pw.get("ARTIFACT"):
        f.pw["ARTIFACT"] -= 1
        return
    if power == "WEAK" and f.run.has("GINGER") or power == "FRAIL" and f.run.has("TURNIP"):
        return
    f.pw[power] = f.pw.get(power, 0) + n


def played(f, c: Card) -> None:
    """What happens after any card is played: powers and relics that count cards, and the enemies that react."""
    pw, run = f.pw, f.run
    if pw.get("AFTER_IMAGE"):
        f.gain_block(pw["AFTER_IMAGE"])
    if pw.get("A_THOUSAND_CUTS"):
        for foe in f.alive():
            f.hurt(foe, pw["A_THOUSAND_CUTS"], attack=False)
    for foe in f.alive():
        if foe.mem.get("choke"):
            f.hurt(foe, foe.mem["choke"], attack=False)
    if pw.get("PANACHE") and len(f.turn_cards) % 5 == 0:
        for foe in f.alive():
            f.hurt(foe, pw["PANACHE"], attack=False)
    if c.type == "Attack":
        if pw.get("RAGE"):
            f.gain_block(pw["RAGE"])
        if f.attacks_turn % 3 == 0:  # (Kunai, Shuriken, Ornamental Fan: every third Attack in one turn)
            for relic, fn in ATTACK_COUNTERS:
                if run.has(relic):
                    fn(f)
        if run.has("PEN_NIB"):
            run.counters["PEN_NIB"] = (run.counters.get("PEN_NIB", 0) + 1) % 10
    if c.type == "Skill":
        if run.has("LETTER_OPENER") and f.skills_turn % 3 == 0:
            for foe in f.alive():
                f.hurt(foe, 5, attack=False)
        for foe in f.alive():
            if foe.powers.get("ENRAGE"):
                foe.powers["STRENGTH"] = foe.powers.get("STRENGTH", 0) + foe.powers["ENRAGE"]
    if pw.get("MONOLOGUE"):
        f.temp["STRENGTH"] = f.temp.get("STRENGTH", 0) + 1
    if pw.get("TENDER"):
        f.temp["STRENGTH"] = f.temp.get("STRENGTH", 0) - 1
        f.temp["DEXTERITY"] = f.temp.get("DEXTERITY", 0) - 1
    for foe in f.alive():
        if foe.mem.get("oblivion"):
            debuff(f, foe, "DOOM", foe.mem["oblivion"])
        if c.type == "Skill" and foe.powers.get("VITAL_SPARK"):
            foe.mem["spark"] = foe.mem.get("spark", 0) + foe.powers["VITAL_SPARK"]
    f.game_hook("play", c)
    if c.type != "Attack" and pw.get("HEX_DAZED"):
        for _ in range(pw["HEX_DAZED"]):
            f.add_card(content(f).make("DAZED"), "draw")
    if c.type == "Power":
        for foe in f.alive():
            if foe.powers.get("CURIOSITY"):
                foe.powers["STRENGTH"] = foe.powers.get("STRENGTH", 0) + foe.powers["CURIOSITY"]
        if pw.get("STORM") and not by_text(f, "STORM"):
            for _ in range(pw["STORM"]):
                f.channel("LIGHTNING")
        if pw.get("HEATSINKS") and not by_text(f, "HEATSINKS"):
            f.draw_cards(pw["HEATSINKS"])
    if run.has("NUNCHAKU") and c.type == "Attack":
        n = run.counters["NUNCHAKU"] = run.counters.get("NUNCHAKU", 0) + 1
        if n % 10 == 0:
            f.energy += 1
    if run.has("INK_BOTTLE"):
        n = run.counters["INK_BOTTLE"] = run.counters.get("INK_BOTTLE", 0) + 1
        if n % 10 == 0:
            f.draw_cards(1)
    for foe in f.alive():
        if foe.powers.get("TIME_WARP") is not None and foe.alive:
            foe.powers["TIME_WARP"] = foe.powers.get("TIME_WARP", 0) + 1
            if foe.powers["TIME_WARP"] >= 12:
                foe.powers["TIME_WARP"] = 0
                for x in f.alive():
                    x.powers["STRENGTH"] = x.powers.get("STRENGTH", 0) + 2
                f.pw["TURN_OVER"] = 1
        if foe.powers.get("SHARP_HIDE") and c.type == "Attack":
            f.take(foe.powers["SHARP_HIDE"], foe, attack=False)
    if any(h.key == "PAIN" for h in f.hand):
        f.lose_hp(sum(h.key == "PAIN" for h in f.hand))


def _shuriken(f):
    f.pw["STRENGTH"] = f.pw.get("STRENGTH", 0) + 1


def _kunai(f):
    f.pw["DEXTERITY"] = f.pw.get("DEXTERITY", 0) + 1


def _fan(f):
    f.gain_block(4)


ATTACK_COUNTERS = (("SHURIKEN", _shuriken), ("KUNAI", _kunai), ("ORNAMENTAL_FAN", _fan))


def drawn(f, c: Card) -> None:
    if f.pw.get("CORROSIVE_WAVE"):
        for foe in f.alive():
            debuff(f, foe, "POISON", f.pw["CORROSIVE_WAVE"])
    if c.key == "VOID":
        f.energy = max(0, f.energy - 1)
    if c.type == "Status" and f.pw.get("EVOLVE"):
        f.draw_cards(f.pw["EVOLVE"])
    if c.type in ("Status", "Curse") and f.pw.get("FIRE_BREATHING"):
        for foe in f.alive():
            f.hurt(foe, f.pw["FIRE_BREATHING"], attack=False)
    if f.pw.get("CONFUSED") and c.cost() >= 0:
        c.cost_fight = f.rng.randint(0, 3)
    rule = content(f).DRAWN.get(c.key)
    if rule:
        rule(f, c)


def exhausted(f, c: Card) -> None:
    f.game_hook("exhaust", c)
    if f.pw.get("FEEL_NO_PAIN") and content(f).KEY == "sts1":
        f.gain_block(f.pw["FEEL_NO_PAIN"])
    if f.pw.get("DARK_EMBRACE") and content(f).KEY == "sts1":
        f.draw_cards(f.pw["DARK_EMBRACE"])
    if c.key == "SENTINEL":
        f.energy += 3 if c.up else 2
    if f.run.has("CHARON_S_ASHES"):
        for foe in f.alive():
            f.hurt(foe, 3, attack=False)
    if f.run.has("DEAD_BRANCH"):
        pool = content(f).pool(f.run.character)
        if pool:
            f.add_card(Card(f.rng.choice(pool)))


def discarded(f, c: Card) -> None:
    """A card discarded from the hand by a card's own say (not at the end of the turn)."""
    f.pw["DISCARDED_TURN"] = f.pw.get("DISCARDED_TURN", 0) + 1
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
    if f.pw.get("JUGGERNAUT") and f.alive() and content(f).KEY == "sts1":
        f.hurt(f.rng.choice(f.alive()), f.pw["JUGGERNAUT"], attack=False)
    if f.pw.get("WAVE_OF_THE_HAND"):
        for foe in f.alive():
            debuff(f, foe, "WEAK", f.pw["WAVE_OF_THE_HAND"])


def stance_changed(f, new: str) -> None:
    if f.pw.get("MENTAL_FORTRESS"):
        f.gain_block(f.pw["MENTAL_FORTRESS"])
    if new == "WRATH" and f.pw.get("RUSHDOWN"):
        f.draw_cards(f.pw["RUSHDOWN"])


def retained(f, c: Card) -> None:
    rule = content(f).RETAINED.get(c.key)
    if rule:
        rule(f, c)
    if f.pw.get("ESTABLISHMENT") and c.cost() > 0:
        c.cost_fight = max(0, c.cost() - f.pw["ESTABLISHMENT"])


def scry(f, n: int) -> None:
    """Look at the top n cards: the dead ones (statuses, curses, Strikes and Defends when the deck is fine) go."""
    if n <= 0:
        return
    if not f.draw and f.discard:
        f.draw, f.discard = f.discard, []
        f.rng.shuffle(f.draw)
    top = f.draw[-n:]
    for c in top:
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
        foe.powers["STRENGTH"] = foe.powers.get("STRENGTH", 0) + foe.powers["ANGRY"]
    if attack and foe.powers.get("MALLEABLE"):
        foe.block += foe.powers["MALLEABLE"]
        foe.powers["MALLEABLE"] += 1
    if attack and foe.powers.get("FLUTTER"):
        foe.powers["FLUTTER"] -= 1
        if foe.powers["FLUTTER"] <= 0:
            foe.powers.pop("FLUTTER")
            hook = foe.spec.hooks.get("grounded")
            if hook:
                hook(f, foe)
    if foe.powers.get("SHIFTING"):
        foe.powers["STRENGTH"] = foe.powers.get("STRENGTH", 0) - amount
        foe.mem["str_back"] = foe.mem.get("str_back", 0) + amount
    if foe.powers.get("ASLEEP"):
        foe.powers.pop("ASLEEP")
        hook = foe.spec.hooks.get("wake")
        if hook:
            hook(f, foe)
    hook = foe.spec.hooks.get("hit")
    if hook:
        hook(f, foe, amount, attack)


# ---------------------------------------------------------------- turns


def fight_start(f) -> None:
    run, g = f.run, content(f)
    g.START(f)
    for rid in run.relics:
        rule = RELIC_START.get(rid)
        if rule:
            rule(f)
    for foe in f.foes:
        hook = foe.spec.hooks.get("start")
        if hook:
            hook(f, foe)


RELIC_START = {
    "ANCHOR": lambda f: f.gain_block(10),
    "VAJRA": lambda f: f.pw.__setitem__("STRENGTH", f.pw.get("STRENGTH", 0) + 1),
    "ODDLY_SMOOTH_STONE": lambda f: f.pw.__setitem__("DEXTERITY", f.pw.get("DEXTERITY", 0) + 1),
    "BLOOD_VIAL": lambda f: f.heal(2),
    "BRONZE_SCALES": lambda f: f.pw.__setitem__("THORNS", f.pw.get("THORNS", 0) + 3),
    "BAG_OF_MARBLES": lambda f: [debuff(f, x, "VULNERABLE", 1) for x in f.alive()],
    "RED_MASK": lambda f: [debuff(f, x, "WEAK", 1) for x in f.alive()],
    "AKABEKO": lambda f: f.pw.__setitem__("VIGOR", f.pw.get("VIGOR", 0) + 8),
    "THREAD_AND_NEEDLE": lambda f: f.pw.__setitem__("PLATING", f.pw.get("PLATING", 0) + 4),
    "CLOCKWORK_SOUVENIR": lambda f: f.pw.__setitem__("ARTIFACT", f.pw.get("ARTIFACT", 0) + 1),
    "FOSSILIZED_HELIX": lambda f: f.pw.__setitem__("BUFFER", f.pw.get("BUFFER", 0) + 1),
    "PHILOSOPHER_S_STONE": lambda f: [x.powers.__setitem__("STRENGTH", x.powers.get("STRENGTH", 0) + 1) for x in f.alive()],
    "MARK_OF_PAIN": lambda f: [f.add_card(content(f).make("WOUND"), "draw") for _ in range(2)],
    "PRESERVED_INSECT": lambda f: [setattr(x, "hp", int(x.hp * 0.75)) for x in f.alive()] if f.kind == "elite" else None,
    "DATA_DISK": lambda f: f.pw.__setitem__("FOCUS", f.pw.get("FOCUS", 0) + 1),
    "NUCLEAR_BATTERY": lambda f: f.channel("PLASMA"),
    "SYMBIOTIC_VIRUS": lambda f: f.channel("DARK"),
    "GIRYA": lambda f: f.pw.__setitem__("STRENGTH", f.pw.get("STRENGTH", 0) + f.run.counters.get("GIRYA", 0)),
    "MUTAGENIC_STRENGTH": lambda f: f.temp.__setitem__("STRENGTH", f.temp.get("STRENGTH", 0) + 3),
    "TWISTED_FUNNEL": lambda f: [debuff(f, x, "POISON", 4) for x in f.alive()],
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
    if run.has("HAPPY_FLOWER"):
        n = run.counters["HAPPY_FLOWER"] = run.counters.get("HAPPY_FLOWER", 0) + 1
        if n % 3 == 0:
            f.energy += 1
    if run.has("MERCURY_HOURGLASS"):
        for foe in f.alive():
            f.hurt(foe, 3, attack=False)
    if run.has("INCENSE_BURNER"):
        n = run.counters["INCENSE_BURNER"] = run.counters.get("INCENSE_BURNER", 0) + 1
        if n % 6 == 0:
            pw["INTANGIBLE"] = pw.get("INTANGIBLE", 0) + 1
    f.energy += sum(1 for o in f.orbs if o[0] == "PLASMA")
    if pw.get("DEMON_FORM") and not by_text(f, "DEMON_FORM"):
        pw["STRENGTH"] = pw.get("STRENGTH", 0) + pw["DEMON_FORM"]
    if pw.get("NOXIOUS_FUMES") and not by_text(f, "NOXIOUS_FUMES"):
        for foe in f.alive():
            debuff(f, foe, "POISON", pw["NOXIOUS_FUMES"])
    if pw.get("BRUTALITY"):
        f.lose_hp(pw["BRUTALITY"])
        f.draw_cards(pw["BRUTALITY"])
    if pw.get("BERSERK"):
        f.energy += pw["BERSERK"]
    if pw.get("INFINITE_BLADES") and not by_text(f, "INFINITE_BLADES"):
        for _ in range(pw["INFINITE_BLADES"]):
            f.add_card(content(f).make("SHIV"))
    if pw.get("WRAITH_FORM"):
        pw["DEXTERITY"] = pw.get("DEXTERITY", 0) - pw["WRAITH_FORM"]
    if pw.get("BIASED_COGNITION"):
        pw["FOCUS"] = pw.get("FOCUS", 0) - pw["BIASED_COGNITION"]
    if pw.get("LOOP") and f.orbs:
        for _ in range(pw["LOOP"]):
            passive(f, f.orbs[0])
    if pw.get("DEVOTION"):
        f.gain_mantra(pw["DEVOTION"])
    if pw.get("DEVA_FORM"):
        pw["DEVA_ENERGY"] = pw.get("DEVA_ENERGY", 0) + pw["DEVA_FORM"]
        f.energy += pw["DEVA_ENERGY"]
    if pw.get("BATTLE_HYMN"):
        for _ in range(pw["BATTLE_HYMN"]):
            f.add_card(content(f).make("SMITE"))
    if pw.pop("SIMMERING", 0):
        f.enter("WRATH")
    if pw.get("BLASPHEMY_DUE"):
        f.lose_hp(f.run.hp)
    content(f).TURN_START(f)


def after_draw(f) -> None:
    pw = f.pw
    if pw.get("TOOLS_OF_THE_TRADE") and not by_text(f, "TOOLS_OF_THE_TRADE"):
        f.draw_cards(pw["TOOLS_OF_THE_TRADE"])
        discard_from_hand(f, pw["TOOLS_OF_THE_TRADE"])
    if pw.get("FORESIGHT"):
        scry(f, pw["FORESIGHT"])
    if pw.get("CREATIVE_AI") or pw.get("HELLO_WORLD") or pw.get("MAYHEM"):
        content(f).TURN_CARDS(f)


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
        for foe in f.alive():
            f.hurt(foe, pw["COMBUST"], attack=False)
    if pw.get("OMEGA"):
        for foe in f.alive():
            f.hurt(foe, pw["OMEGA"], attack=False)
    if pw.get("STUDY"):
        for _ in range(pw["STUDY"]):
            f.add_card(content(f).make("INSIGHT"), "draw")
    for o in list(f.orbs):
        if f.won:
            break
        passive(f, o)
    if run.has("FROZEN_CORE") and len(f.orbs) < f.orb_slots:
        f.channel("FROST")
    if run.has("ORICHALCUM") and f.block == 0:
        f.gain_block(6)
    if run.has("STONE_CALENDAR") and f.round == 7:
        for foe in f.alive():
            f.hurt(foe, 52, attack=False)
    if pw.get("CONSTRICT"):
        f.take(pw["CONSTRICT"], attack=False)
    for c in list(f.hand):
        if c.key == "BURN":
            f.take(4 if c.up else 2, attack=False)
        elif c.key == "DECAY":
            f.take(2, attack=False)
        elif c.key == "REGRET":
            f.lose_hp(len(f.hand))
        elif c.key == "DOUBT":
            debuff_me(f, "WEAK", 1)
        elif c.key == "SHAME":
            debuff_me(f, "FRAIL", 1)
    if pw.get("WAVE_OF_THE_HAND"):
        pw.pop("WAVE_OF_THE_HAND")
    if pw.get("DOUBLE_DAMAGE_NEXT"):
        pw["DOUBLE_DAMAGE"] = pw.pop("DOUBLE_DAMAGE_NEXT")
    elif pw.get("DOUBLE_DAMAGE"):
        pw.pop("DOUBLE_DAMAGE")
    if pw.get("BLUR"):
        pw["KEEP_BLOCK"] = 1
        pw["BLUR"] -= 1
    content(f).TURN_END(f)


def foe_round_end(f, foe) -> None:
    if foe.powers.get("RITUAL"):
        if foe.mem.get("ritual_ready"):
            foe.powers["STRENGTH"] = foe.powers.get("STRENGTH", 0) + foe.powers["RITUAL"]
        foe.mem["ritual_ready"] = True
    if foe.powers.get("METALLICIZE"):
        foe.block += foe.powers["METALLICIZE"]
    if foe.powers.get("PLATING"):
        foe.block += foe.powers["PLATING"]
    if foe.powers.get("REGEN"):
        foe.hp = min(foe.max_hp, foe.hp + foe.powers["REGEN"])
    if foe.powers.get("MALLEABLE"):
        foe.powers["MALLEABLE"] = foe.mem.setdefault("malleable", foe.powers["MALLEABLE"])
    hook = foe.spec.hooks.get("end")
    if hook:
        hook(f, foe)


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
                f.add_card(content(f).make("WOUND"), "discard")
        if foe.powers.get("IMBALANCED") and f.run.hp == hp0:
            foe.mem["off_balance"] = True
    if m.block:
        foe.block += m.block
    for fx in m.fx:
        if f.lost:
            return
        do(f, foe, fx)
    hook = foe.spec.hooks.get("acted")
    if hook:
        hook(f, foe)


def do(f, foe, fx: tuple) -> None:
    """One effect of an enemy's move."""
    word, *args = fx
    n = args[0] if args else 0
    alive = f.alive()
    if word == "str":
        foe.powers["STRENGTH"] = foe.powers.get("STRENGTH", 0) + n
    elif word == "str_all":
        for x in alive:
            x.powers["STRENGTH"] = x.powers.get("STRENGTH", 0) + n
    elif word in ("ritual", "metallicize", "plated", "artifact", "thorns", "intangible", "enrage", "regen", "curiosity"):
        key = {"plated": "PLATING"}.get(word, word.upper())
        foe.powers[key] = foe.powers.get(key, 0) + n
    elif word == "block_ally":
        others = [x for x in alive if x is not foe] or [foe]
        f.rng.choice(others).block += n
    elif word == "block_all":
        for x in alive:
            x.block += n
    elif word in ("weak", "vuln", "frail"):
        debuff_me(f, {"weak": "WEAK", "vuln": "VULNERABLE", "frail": "FRAIL"}[word], n)
    elif word == "status":
        key, count, pile = args[0], args[1], args[2] if len(args) > 2 else "discard"
        for _ in range(count):
            f.add_card(content(f).make(key, up=bool(foe.mem.get("burn_up")) and key == "BURN"), pile)
    elif word == "heal":
        foe.hp = min(foe.max_hp, foe.hp + n)
    elif word == "heal_all":
        for x in alive:
            x.hp = min(x.max_hp, x.hp + n)
    elif word == "summon":
        key, count = args[0], args[1] if len(args) > 1 else 1
        for _ in range(count):
            if sum(1 for x in f.foes if x.alive) < 5:
                f.foes.append(content(f).spawn(key, f, minion=True))
                f.foes[-1].choose(f.rng)
    elif word == "str_down":
        f.pw["STRENGTH"] = f.pw.get("STRENGTH", 0) - n
    elif word == "dex_down":
        f.pw["DEXTERITY"] = f.pw.get("DEXTERITY", 0) - n
    elif word == "focus_down":
        f.pw["FOCUS"] = f.pw.get("FOCUS", 0) - n
    elif word == "draw_down":
        f.pw["DRAW_REDUCTION"] = n
    elif word == "entangle":
        f.pw["ENTANGLED"] = 1
    elif word == "hex":
        f.pw["HEX_DAZED"] = 1
    elif word == "constrict":
        f.pw["CONSTRICT"] = f.pw.get("CONSTRICT", 0) + n
    elif word == "confuse":
        f.pw["CONFUSED"] = 1
    elif word == "steal_gold":
        took = min(f.run.gold, n)
        f.run.gold -= took
        foe.mem["stolen"] = foe.mem.get("stolen", 0) + took
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
        foe.powers[args[0]] = foe.powers.get(args[0], 0) + args[1]
    elif word == "player_power":
        f.pw[args[0]] = f.pw.get(args[0], 0) + args[1]


# ---------------------------------------------------------------- potions


def potion_usable(f, pot) -> bool:
    return pot.key not in ("FAIRY_IN_A_BOTTLE", "SMOKE_BOMB") and not (pot.key == "FRUIT_JUICE" and False)


def drink(f, pot, foe) -> None:
    t = pot.text
    low = t.lower()
    sacred = 2 if f.run.has("SACRED_BARK") else 1
    num = lambda pat: (int(m.group(1)) * sacred) if (m := re.search(pat, t, re.I)) else 0
    if (n := num(r"deal (\d+) damage to all enemies")):
        for x in f.alive():
            f.hurt(x, n, attack=False)
    elif (n := num(r"deal (\d+) damage")) and foe is not None:
        f.hurt(foe, n, attack=False)
    if (n := num(r"gain (\d+) block")):
        f.gain_block(n)
    if (n := num(r"gain (\d+) strength")):
        (f.temp if "at the end of your turn" in low else f.pw)["STRENGTH"] = (f.temp if "at the end of your turn" in low else f.pw).get("STRENGTH", 0) + n
    if (n := num(r"gain (\d+) dexterity")):
        (f.temp if "at the end of your turn" in low else f.pw)["DEXTERITY"] = (f.temp if "at the end of your turn" in low else f.pw).get("DEXTERITY", 0) + n
    for word, power in (("focus", "FOCUS"), ("artifact", "ARTIFACT"), ("plated armor", "PLATING"), ("metallicize", "METALLICIZE"),
                        ("thorns", "THORNS"), ("regen", "REGEN"), ("intangible", "INTANGIBLE"), ("ritual", "RITUAL_ME")):
        if (n := num(rf"gain (\d+) {word}")):
            f.pw[power] = f.pw.get(power, 0) + n
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
        for _ in range(n):
            f.add_card(content(f).make("SHIV", up=True))
    if "miracles" in low:
        for _ in range(2 * sacred):
            f.add_card(content(f).make("MIRACLE"))
    if "enter divinity" in low:
        f.enter("DIVINITY")
    elif "enter calm or wrath" in low:
        f.enter("WRATH")
    if "next card is played twice" in low:
        f.pw["DUPLICATION"] = f.pw.get("DUPLICATION", 0) + sacred
    if (n := num(r"gain (\d+) orb slots")):
        f.orb_slots += n
    if "channel 1 dark for each orb slot" in low:
        for _ in range(f.orb_slots):
            f.channel("DARK")
    if "play the top 3 cards" in low:
        for _ in range(3 * sacred):
            if not f.draw and f.discard:
                f.draw, f.discard = f.discard, []
                f.rng.shuffle(f.draw)
            if f.draw and not f.won:
                c = f.draw.pop()
                resolve(f, c, (f.alive() or [None])[0], f.energy)
                f.put_away(c)
    if "upgrade all cards in your hand" in low:
        for c in f.hand:
            c.up = True
    if "add a random" in low or "choose 1 of 3" in low:
        content(f).POTION_CARD(f, pot)
