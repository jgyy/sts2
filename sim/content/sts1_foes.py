"""The first game's monsters, encounters, elites and bosses (Ascension 0 numbers, written from the game's rules)."""

from __future__ import annotations

from ..effects import add, debuff_me
from ..monsters import Move as M, Spec, counter, cycle, opening, weighted


def _fight(foe):
    return foe.mem["fight"]


def _set(foe, move: str, dmg=None, hits=None):
    if dmg is not None:
        foe.mem[("dmg", move)] = dmg
    if hits is not None:
        foe.mem[("hits", move)] = hits


# ------------------------------------------------------------------ hooks


def _louse_start(f, foe):
    foe.powers["CURL_UP"] = f.rng.randint(3, 7)
    _set(foe, "bite", f.rng.randint(5, 7))


def _split_when_half(into: str, count: int = 2):
    def hit(f, foe, amount, attack):
        if foe.alive and foe.hp <= foe.max_hp // 2 and not foe.mem.get("splitting"):
            foe.mem["splitting"] = True
            foe.mem["split_into"] = (into, count)
            foe.move_name = "split"
    return hit


def _split(f, foe):
    into, count = foe.mem.get("split_into", (None, 0))
    hp = foe.hp
    foe.gone = True
    for key in ([into] * count if isinstance(into, str) else into):
        new = f.game.spawn(key, f, hp=hp)
        f.foes.append(new)
        new.choose(f.rng)


_give_back_gold = lambda f, foe: setattr(f.run, "gold", f.run.gold + foe.mem.get("stolen", 0))


def _lagavulin_wake(f, foe):
    foe.powers.pop("METALLICIZE", None)
    foe.mem["awake"] = True
    foe.move_name = "stunned"


def _lagavulin(foe, rng):
    if not foe.mem.get("awake"):
        if (slept := add(foe.mem, "slept", 1)) <= 3:
            foe.powers["ASLEEP"] = 4 - slept
            return "sleep"
        foe.mem["awake"] = True
        foe.powers.pop("ASLEEP", None)
        foe.powers.pop("METALLICIZE", None)
    return ("attack", "attack", "siphon")[counter(foe, "i") % 3]


def _hexa(foe, rng):
    n = len(foe.history)
    if n == 0:
        return "activate"
    if n == 1:
        _set(foe, "divider", _fight(foe).run.hp // 12 + 1)
        return "divider"
    return ("sear", "tackle", "sear", "inflame", "tackle", "sear", "inferno")[(n - 2) % 7]


def _hexa_inferno(f, foe):
    foe.mem["burn_up"] = True
    for c in f.draw + f.discard + f.hand:
        if c.key == "BURN":
            c.up = True


def _guardian(foe, rng):
    if step := foe.mem.get("defensive"):  # (shifted to its defensive mode: two moves, then its Twin Slam)
        foe.mem["defensive"] = step + 1
        if step < 3:
            return ("close_up", "roll")[step - 1]
        foe.mem["defensive"] = 0
        foe.powers.pop("SHARP_HIDE", None)
        foe.mem["i"] = 3
        return "twin_slam"
    return ("charge_up", "fierce_bash", "vent_steam", "whirlwind")[counter(foe, "i") % 4]


def _guardian_hit(f, foe, amount, attack):
    if not foe.mem.get("defensive") and add(foe.mem, "taken", amount) >= foe.mem.get("shift", 30):
        foe.mem["taken"], foe.mem["shift"] = 0, foe.mem.get("shift", 30) + 10
        foe.mem["defensive"] = 1
        foe.block += 20
        foe.move_name = _guardian(foe, f.rng)


def _chosen(foe, rng):
    n = len(foe.history)
    if n < 2:
        return ("poke", "hex")[n]
    return rng.choice(("debilitate", "drain")) if n % 2 == 0 else "zap" if rng.random() < 0.4 else "poke"


def _red_slaver(foe, rng):
    if not foe.history:
        return "stab"
    if not foe.mem.get("entangled") and rng.random() < 0.25:
        foe.mem["entangled"] = True
        return "entangle"
    return weighted({"scrape": 55, "stab": 45}, {"scrape": 2, "stab": 2})(foe, rng)


def _byrd(foe, rng):
    if g := foe.mem.get("grounded", 0):  # (knocked down: stunned, then a headbutt, then up again)
        foe.mem["grounded"] = g - 1
        return "headbutt" if g == 2 else "fly"
    return weighted({"peck": 50, "swoop": 20, "caw": 30}, {"peck": 2, "swoop": 1, "caw": 1})(foe, rng)


def _book(foe, rng):
    m = weighted({"multi": 85, "single": 15}, {"multi": 2, "single": 1})(foe, rng)
    if m == "multi":
        foe.mem["stabs"] = foe.mem.get("stabs", 1) + 1
        _set(foe, "multi", hits=foe.mem["stabs"])
    return m


def _leader(foe, rng):
    f = _fight(foe)
    minions = sum(1 for x in f.foes if x.alive and x.minion)
    if minions < 2 and foe.last() != ["rally"]:
        return "rally"
    return weighted({"encourage": 50, "stab": 50}, {"encourage": 1, "stab": 2})(foe, rng)


def _collector(foe, rng):
    n = len(foe.history)
    f = _fight(foe)
    if n == 0:
        return "spawn"
    if n == 3:
        return "mega_debuff"
    if not any(x.alive and x.minion for x in f.foes) and rng.random() < 0.25:
        return "spawn"
    return weighted({"fireball": 70, "buff": 30}, {"fireball": 2, "buff": 1})(foe, rng)


def _champ(foe, rng):
    if foe.hp < foe.max_hp // 2 and not foe.mem.get("angry"):
        foe.mem["angry"] = True
        return "anger"
    n = len(foe.history)
    if foe.mem.get("angry"):
        return "execute" if foe.last() != ["execute"] else weighted({"heavy_slash": 50, "face_slap": 50})(foe, rng)
    if n % 4 == 3:
        return "taunt"
    table = {"face_slap": 30, "heavy_slash": 30, "gloat": 20} | ({"defensive": 20} if foe.history.count("defensive") < 2 else {})
    return weighted(table, {"defensive": 1, "gloat": 1})(foe, rng)


def _darkling_death(f, foe):
    if not any(x is not foe and x.key == "DARKLING" and not x.gone and not x.half_dead for x in f.foes):
        for x in f.foes:
            if x.key == "DARKLING":
                x.gone = True
        return False
    foe.half_dead, foe.hp = True, 0
    foe.mem["revive"] = 2
    foe.move_name = "regrow"
    return True


def darkling_round(f):
    """The half-dead Darklings rise again at half HP, two rounds on."""
    for x in f.foes:
        if x.half_dead and x.key == "DARKLING":
            x.mem["revive"] -= 1
            if x.mem["revive"] <= 0:
                x.half_dead, x.hp = False, x.max_hp // 2
                x.choose(f.rng)


def _transient_end(f, foe):
    """Fading 5: it loses a stack at the start of each of its turns and dies instead of acting on the fifth."""
    foe.powers["FADING"] = foe.powers.get("FADING", 5) - 1
    if foe.powers["FADING"] <= 1:
        f.kill(foe)


def _giant_head(foe, rng):
    n = len(foe.history)
    if n < 4:
        return weighted({"count": 50, "glare": 50}, {"count": 2, "glare": 2})(foe, rng)
    _set(foe, "it_is_time", min(60, 30 + 5 * (n - 4)))
    return "it_is_time"


def _nemesis(foe, rng):
    if foe.last() != ["scythe"] and "scythe" not in foe.last(2) and rng.random() < 0.35:
        return "scythe"
    return weighted({"attack": 50, "debuff": 50}, {"attack": 2, "debuff": 1})(foe, rng)


def _awakened_death(f, foe):
    if foe.mem.get("reborn"):
        return False
    foe.mem["reborn"] = True
    foe.half_dead, foe.hp = True, 0
    foe.powers = {k: v for k, v in foe.powers.items() if k in ("STRENGTH",) and v > 0}
    foe.move_name = "rebirth"
    return True


def _awakened(foe, rng):
    if foe.half_dead:
        foe.half_dead, foe.hp = False, foe.max_hp
        foe.powers.pop("STOCK", None)
        foe.powers.pop("CURIOSITY", None)
        return "dark_echo"
    if foe.mem.get("reborn"):
        return weighted({"sludge": 50, "tackle": 50}, {"sludge": 2, "tackle": 2})(foe, rng)
    if not foe.history:
        return "slash"
    return weighted({"slash": 75, "soul_strike": 25}, {"slash": 2, "soul_strike": 1})(foe, rng)


def _time_eater(foe, rng):
    if foe.hp < foe.max_hp // 2 and not foe.mem.get("hasted"):
        foe.mem["hasted"] = True
        return "haste"
    return weighted({"reverberate": 45, "head_slam": 35, "ripple": 20}, {"reverberate": 2, "head_slam": 1, "ripple": 1})(foe, rng)


def _haste(f, foe):
    foe.hp = max(foe.hp, foe.max_hp // 2)
    for k in ("WEAK", "VULNERABLE", "POISON"):
        foe.powers.pop(k, None)


def _spire_growth(foe, rng):
    f = _fight(foe)
    if not f.pw.get("CONSTRICT") and foe.last() != ["constrict"]:
        return "constrict"
    return weighted({"quick": 50, "smash": 50}, {"quick": 2, "smash": 2})(foe, rng)


def _maw(foe, rng):
    if not foe.history:
        return "roar"
    m = weighted({"slam": 30, "nom": 40, "drool": 30}, {"slam": 1, "nom": 1, "drool": 1})(foe, rng)
    if m == "nom":
        _set(foe, "nom", hits=1 + add(foe.mem, "noms", 1))
    return m


S = {}


def spec(key, name, hp, moves, pattern, **kw) -> None:
    """A monster of the table S. Each keeps its fight in mind from the start (then does its own "start" hook, if any)."""
    hooks = kw.pop("hooks", {})
    start = hooks.get("start")
    hooks["start"] = lambda f, foe: (foe.mem.__setitem__("fight", f), start and start(f, foe))
    S[key] = Spec(key, name, hp, moves, pattern, hooks=hooks, **kw)


# ------------------------------------------------------------------ act 1

spec("JAW_WORM", "Jaw Worm", (40, 44), {"chomp": M(11), "thrash": M(7, block=5), "bellow": M(block=6, fx=(("str", 3),))},
     opening("chomp", then=weighted({"bellow": 45, "thrash": 30, "chomp": 25}, {"chomp": 1, "bellow": 1, "thrash": 2})))
spec("CULTIST", "Cultist", (48, 54), {"incantation": M(fx=(("ritual", 3),)), "dark_strike": M(6)}, opening("incantation", then=cycle("dark_strike")))
spec("RED_LOUSE", "Red Louse", (10, 15), {"bite": M(6), "grow": M(fx=(("str", 3),))}, weighted({"bite": 75, "grow": 25}, {"bite": 2, "grow": 1}), hooks={"start": _louse_start})
spec("GREEN_LOUSE", "Green Louse", (11, 17), {"bite": M(6), "spit": M(fx=(("weak", 2),))}, weighted({"bite": 75, "spit": 25}, {"bite": 2, "spit": 1}), hooks={"start": _louse_start})
spec("ACID_SLIME_S", "Acid Slime (S)", (8, 12), {"tackle": M(3), "lick": M(fx=(("weak", 1),))}, cycle("lick", "tackle"))
spec("SPIKE_SLIME_S", "Spike Slime (S)", (10, 14), {"tackle": M(5)}, cycle("tackle"))
_SLIMED = lambda n: ("status", "SLIMED", n, "discard")
spec("ACID_SLIME_M", "Acid Slime (M)", (28, 32), {"spit": M(7, fx=(_SLIMED(1),)), "tackle": M(10), "lick": M(fx=(("weak", 1),))},
     weighted({"spit": 30, "tackle": 40, "lick": 30}, {"spit": 2, "tackle": 1, "lick": 1}))
spec("SPIKE_SLIME_M", "Spike Slime (M)", (28, 32), {"flame_tackle": M(8, fx=(_SLIMED(1),)), "lick": M(fx=(("frail", 1),))}, weighted({"flame_tackle": 30, "lick": 70}, {"flame_tackle": 2, "lick": 2}))
spec("ACID_SLIME_L", "Acid Slime (L)", (65, 69), {"spit": M(11, fx=(_SLIMED(2),)), "tackle": M(16), "lick": M(fx=(("weak", 2),)),
                                                   "split": M(kind="Unknown", fx=(("call", _split),))},
     lambda f, r: "split" if f.mem.get("splitting") else weighted({"spit": 30, "tackle": 40, "lick": 30}, {"spit": 2, "tackle": 1, "lick": 1})(f, r),
     hooks={"hit": _split_when_half("ACID_SLIME_M")})
spec("SPIKE_SLIME_L", "Spike Slime (L)", (64, 70), {"flame_tackle": M(16, fx=(_SLIMED(2),)), "lick": M(fx=(("frail", 2),)),
                                                     "split": M(kind="Unknown", fx=(("call", _split),))},
     lambda f, r: "split" if f.mem.get("splitting") else weighted({"flame_tackle": 30, "lick": 70}, {"flame_tackle": 2, "lick": 2})(f, r),
     hooks={"hit": _split_when_half("SPIKE_SLIME_M")})
spec("FUNGI_BEAST", "Fungi Beast", (22, 28), {"bite": M(6), "grow": M(fx=(("str", 3),))}, weighted({"bite": 60, "grow": 40}, {"bite": 2, "grow": 1}),
     hooks={"death": lambda f, foe: debuff_me(f, "VULNERABLE", 2)})
spec("LOOTER", "Looter", (44, 48), {"mug": M(10, fx=(("steal_gold", 15),)), "lunge": M(12, fx=(("steal_gold", 15),)), "smoke": M(block=6),
                                    "escape": M(kind="Escape")}, opening("mug", "mug", "lunge", "smoke", then=cycle("escape")), hooks={"death": _give_back_gold})
spec("MUGGER", "Mugger", (48, 52), {"mug": M(10, fx=(("steal_gold", 15),)), "lunge": M(16, fx=(("steal_gold", 15),)), "smoke": M(block=11),
                                    "escape": M(kind="Escape")}, opening("mug", "mug", "lunge", "smoke", then=cycle("escape")), hooks={"death": _give_back_gold})
spec("MAD_GREMLIN", "Mad Gremlin", (20, 24), {"scratch": M(4)}, cycle("scratch"), powers={"ANGRY": 1})
spec("SNEAKY_GREMLIN", "Sneaky Gremlin", (10, 14), {"puncture": M(9)}, cycle("puncture"))
spec("FAT_GREMLIN", "Fat Gremlin", (13, 17), {"smash": M(4, fx=(("weak", 1),))}, cycle("smash"))
spec("SHIELD_GREMLIN", "Shield Gremlin", (12, 15), {"protect": M(fx=(("block_ally", 7),)), "bash": M(6)}, lambda f, r: "bash" if sum(1 for x in _fight(f).foes if x.alive) == 1 else "protect")
spec("GREMLIN_WIZARD", "Gremlin Wizard", (21, 25), {"charge": M(kind="Unknown"), "blast": M(25)}, lambda f, r: "blast" if counter(f, "i") % 4 == 3 else "charge")
spec("BLUE_SLAVER", "Blue Slaver", (46, 50), {"stab": M(12), "rake": M(7, fx=(("weak", 1),))}, weighted({"stab": 60, "rake": 40}, {"stab": 2, "rake": 2}))
spec("RED_SLAVER", "Red Slaver", (46, 50), {"stab": M(13), "scrape": M(8, fx=(("vuln", 1),)), "entangle": M(fx=(("entangle",),))}, _red_slaver)
spec("GREMLIN_NOB", "Gremlin Nob", (82, 86), {"bellow": M(fx=(("enrage", 2),)), "rush": M(14), "skull_bash": M(6, fx=(("vuln", 2),))},
     opening("bellow", then=weighted({"skull_bash": 33, "rush": 67}, {"rush": 2, "skull_bash": 1})))
spec("LAGAVULIN", "Lagavulin", (109, 111), {"sleep": M(kind="Sleep"), "stunned": M(kind="Stun"), "attack": M(18),
                                            "siphon": M(fx=(("str_down", 1), ("dex_down", 1)))}, _lagavulin,
     powers={"METALLICIZE": 8, "ASLEEP": 3}, hooks={"wake": _lagavulin_wake})
spec("SENTRY", "Sentry", (38, 42), {"beam": M(9), "bolt": M(fx=(("status", "DAZED", 2, "discard"),))}, cycle("bolt", "beam"), powers={"ARTIFACT": 1})
spec("SENTRY_MID", "Sentry", (38, 42), {"beam": M(9), "bolt": M(fx=(("status", "DAZED", 2, "discard"),))}, cycle("beam", "bolt"), powers={"ARTIFACT": 1})
spec("SLIME_BOSS", "Slime Boss", (140, 140), {"goop": M(fx=(_SLIMED(3),)), "prep": M(kind="Unknown"), "slam": M(35),
                                              "split": M(kind="Unknown", fx=(("call", _split),))},
     lambda f, r: "split" if f.mem.get("splitting") else cycle("goop", "prep", "slam")(f, r),
     hooks={"hit": _split_when_half(("ACID_SLIME_L", "SPIKE_SLIME_L"), 1)})
spec("HEXAGHOST", "Hexaghost", (250, 250), {"activate": M(kind="Unknown"), "divider": M(1, hits=6), "sear": M(6, fx=(("status", "BURN", 1, "discard"),)),
                                            "tackle": M(5, hits=2), "inflame": M(block=12, fx=(("str", 2),)),
                                            "inferno": M(2, hits=6, fx=(("status", "BURN", 3, "discard"), ("call", _hexa_inferno)))}, _hexa)
spec("THE_GUARDIAN", "The Guardian", (240, 240), {"charge_up": M(block=9), "fierce_bash": M(32), "vent_steam": M(fx=(("weak", 2), ("vuln", 2))),
                                                  "whirlwind": M(5, hits=4), "close_up": M(fx=(("call", lambda f, x: x.powers.__setitem__("SHARP_HIDE", 3)),),
                                                                                         kind="Buff"),
                                                  "roll": M(9), "twin_slam": M(8, hits=2)}, _guardian, hooks={"hit": _guardian_hit})

# ------------------------------------------------------------------ act 2

spec("SPHERIC_GUARDIAN", "Spheric Guardian", (20, 20), {"activate": M(block=25), "attack_frail": M(10, fx=(("frail", 5),)), "slam": M(10, hits=2),
                                                        "harden": M(10, block=15)},
     lambda f, r: opening("activate", "attack_frail", then=cycle("slam", "harden"))(f, r), powers={"BARRICADE": 1, "ARTIFACT": 3}, block=40)
spec("CHOSEN", "Chosen", (95, 99), {"poke": M(5, hits=2), "hex": M(fx=(("hex",),)), "debilitate": M(10, fx=(("vuln", 2),)), "drain": M(fx=(("weak", 3), ("str", 3))), "zap": M(18)}, _chosen)
spec("SHELLED_PARASITE", "Shelled Parasite", (68, 72), {"double": M(6, hits=2), "suck": M(10, fx=(("heal", 10),)), "fell": M(18, fx=(("frail", 2),))},
     weighted({"double": 40, "suck": 40, "fell": 20}, {"double": 2, "suck": 2, "fell": 1}), powers={"PLATING": 14},
     hooks={"hit": lambda f, foe, n, attack: foe.powers.__setitem__("PLATING", max(0, foe.powers.get("PLATING", 0) - 1)) if attack else None})
spec("BYRD", "Byrd", (25, 31), {"peck": M(1, hits=5), "swoop": M(12), "caw": M(fx=(("str", 1),)), "stunned": M(kind="Stun"), "headbutt": M(3),
                                "fly": M(fx=(("call", lambda f, x: x.powers.__setitem__("FLUTTER", 3)),), kind="Buff")}, _byrd,
     powers={"FLUTTER": 3}, hooks={"grounded": lambda f, foe: (foe.mem.__setitem__("grounded", 2), setattr(foe, "move_name", "stunned"))})
spec("SNAKE_PLANT", "Snake Plant", (75, 79), {"chomp": M(7, hits=3), "spores": M(fx=(("weak", 2), ("frail", 2)))},
     weighted({"chomp": 65, "spores": 35}, {"chomp": 2, "spores": 1}), powers={"MALLEABLE": 3})
spec("SNECKO", "Snecko", (114, 120), {"glare": M(fx=(("confuse",),)), "bite": M(15), "tail_whip": M(8, fx=(("vuln", 2),))}, opening("glare", then=weighted({"bite": 60, "tail_whip": 40}, {"bite": 2})))
spec("CENTURION", "Centurion", (76, 80), {"slash": M(12), "fury": M(6, hits=3), "defend": M(fx=(("block_ally", 15),))}, weighted({"slash": 40, "fury": 30, "defend": 30}, {"fury": 1, "defend": 1}))
spec("MYSTIC", "Mystic", (48, 56), {"heal": M(fx=(("heal_all", 16),)), "buff": M(fx=(("str_all", 2),)), "attack": M(8, fx=(("frail", 2),))},
     weighted({"attack": 50, "buff": 30, "heal": 20}, {"buff": 1, "heal": 1}))
spec("BOOK_OF_STABBING", "Book of Stabbing", (160, 164), {"multi": M(6, hits=2), "single": M(21)}, _book, hooks={"start": lambda f, foe: foe.mem.__setitem__("painful_stabs", True)})
spec("GREMLIN_LEADER", "Gremlin Leader", (140, 148), {"rally": M(fx=(("summon", "GREMLIN", 2),)), "encourage": M(fx=(("str_all", 3), ("block_all", 6))), "stab": M(6, hits=3)}, _leader)
spec("TASKMASTER", "Taskmaster", (54, 60), {"whip": M(7, fx=(("status", "WOUND", 1, "discard"),))}, cycle("whip"))
spec("TORCH_HEAD", "Torch Head", (38, 40), {"tackle": M(7)}, cycle("tackle"), minion=True)
spec("BRONZE_ORB", "Bronze Orb", (52, 58), {"beam": M(8), "support": M(fx=(("block_ally", 12),)), "stasis": M(fx=(("draw_down", 1),))},
     opening("stasis", then=weighted({"beam": 60, "support": 40}, {"beam": 2, "support": 1})), minion=True)
spec("BRONZE_AUTOMATON", "Bronze Automaton", (300, 300), {"spawn": M(fx=(("summon", "BRONZE_ORB", 2),)), "flail": M(7, hits=2),
                                                          "boost": M(block=9, fx=(("str", 3),)), "hyper_beam": M(45), "stunned": M(kind="Stun")},
     opening("spawn", then=cycle("flail", "boost", "flail", "boost", "hyper_beam", "stunned")), powers={"ARTIFACT": 3})
spec("THE_COLLECTOR", "The Collector", (282, 282), {"spawn": M(fx=(("summon", "TORCH_HEAD", 2),)), "fireball": M(18),
                                                    "buff": M(block=15, fx=(("str_all", 3),)), "mega_debuff": M(fx=(("weak", 3), ("vuln", 3), ("frail", 3)))},
     _collector)
spec("THE_CHAMP", "The Champ", (420, 420), {"defensive": M(block=15, fx=(("metallicize", 5),)), "face_slap": M(12, fx=(("frail", 2), ("vuln", 2))),
                                            "heavy_slash": M(16), "gloat": M(fx=(("str", 2),)), "taunt": M(fx=(("weak", 2), ("vuln", 2))),
                                            "anger": M(fx=(("remove_debuffs",), ("str", 6))), "execute": M(10, hits=2)}, _champ)

# ------------------------------------------------------------------ act 3

spec("DARKLING", "Darkling", (48, 56), {"nip": M(9), "chomp": M(8, hits=2), "harden": M(block=12), "regrow": M(kind="Unknown")},
     weighted({"nip": 40, "chomp": 30, "harden": 30}, {"chomp": 1, "harden": 1}), powers={"REATTACH": 25},
     hooks={"death": _darkling_death, "start": lambda f, foe: _set(foe, "nip", f.rng.randint(7, 11))})
spec("ORB_WALKER", "Orb Walker", (90, 96), {"laser": M(10, fx=(("status", "BURN", 1, "discard"),)), "claw": M(15)},
     weighted({"laser": 60, "claw": 40}, {"laser": 2, "claw": 2}), powers={"RITUAL": 3},
     hooks={"start": lambda f, foe: foe.mem.__setitem__("ritual_ready", True)})
spec("SPIKER", "Spiker", (42, 56), {"cut": M(7), "spike": M(fx=(("thorns", 2),))}, weighted({"cut": 50, "spike": 50}, {"spike": 2, "cut": 2}), powers={"THORNS": 3})
spec("REPULSOR", "Repulsor", (29, 35), {"repulse": M(fx=(("status", "DAZED", 2, "draw"),)), "bash": M(11)}, weighted({"repulse": 80, "bash": 20}, {"bash": 1}))
spec("EXPLODER", "Exploder", (30, 30), {"slam": M(9), "explode": M(30, fx=(("die",),))}, opening("slam", "slam", then=cycle("explode")))
spec("SPIRE_GROWTH", "Spire Growth", (170, 170), {"quick": M(16), "smash": M(22), "constrict": M(fx=(("constrict", 10),))}, _spire_growth)
spec("TRANSIENT", "Transient", (999, 999), {"attack": M(30)}, lambda f, r: _set(f, "attack", 30 + 10 * len(f.history)) or "attack", powers={"FADING": 5, "SHIFTING": 1}, hooks={"end": _transient_end})
spec("MAW", "The Maw", (300, 300), {"roar": M(fx=(("weak", 3), ("frail", 3))), "slam": M(25), "nom": M(5, hits=1), "drool": M(fx=(("str", 3),))}, _maw)
_WRITHE = weighted({"strong": 20, "multi": 25, "flail": 25, "wither": 30})  # (the Writhing Mass picks again whenever an attack hits it)
spec("WRITHING_MASS", "Writhing Mass", (160, 160), {"strong": M(32), "multi": M(7, hits=3), "flail": M(15, block=16), "wither": M(10, fx=(("weak", 2), ("vuln", 2)))},
     _WRITHE, powers={"REACTIVE": 1, "MALLEABLE": 3}, hooks={"hit": lambda f, foe, n, attack: setattr(foe, "move_name", _WRITHE(foe, f.rng)) if attack and foe.alive else None})
spec("JAW_WORM_HORDE", "Jaw Worm", (40, 44), {"chomp": M(11), "thrash": M(7, block=5), "bellow": M(block=6, fx=(("str", 3),))},
     opening("bellow", then=weighted({"bellow": 45, "thrash": 30, "chomp": 25}, {"chomp": 1, "bellow": 1, "thrash": 2})))
spec("GIANT_HEAD", "Giant Head", (500, 500), {"count": M(13), "glare": M(fx=(("weak", 1),)), "it_is_time": M(30)}, _giant_head, powers={"SLOW": 1})
spec("NEMESIS", "Nemesis", (185, 185), {"debuff": M(fx=(("status", "BURN", 5, "discard"),)), "attack": M(6, hits=3), "scythe": M(45)}, _nemesis,
     hooks={"end": lambda f, foe: foe.powers.__setitem__("INTANGIBLE", 1) if not foe.powers.get("INTANGIBLE") else None})
spec("SNAKE_DAGGER", "Snake Dagger", (20, 25), {"stab": M(9, fx=(("status", "WOUND", 1, "discard"),)), "explode": M(25, fx=(("die",),))},
     lambda f, r: "stab" if not f.history else "explode", minion=True)
spec("REPTOMANCER", "Reptomancer", (180, 190), {"summon": M(fx=(("summon", "SNAKE_DAGGER", 1),)), "snake_strike": M(13, hits=2, fx=(("weak", 1),)),
                                                "big_bite": M(30)},
     opening("snake_strike", then=weighted({"summon": 33, "snake_strike": 33, "big_bite": 34}, {"summon": 1, "big_bite": 1, "snake_strike": 1})))
spec("AWAKENED_ONE", "Awakened One", (300, 300), {"slash": M(20), "soul_strike": M(6, hits=4), "rebirth": M(kind="Unknown"), "dark_echo": M(40),
                                                  "sludge": M(18, fx=(("status", "VOID", 1, "draw"),)), "tackle": M(10, hits=3)}, _awakened,
     powers={"CURIOSITY": 1, "REGEN": 10, "STOCK": 1}, hooks={"death": _awakened_death})
spec("TIME_EATER", "Time Eater", (456, 456), {"reverberate": M(7, hits=3), "head_slam": M(26, fx=(("draw_down", 1),)),
                                              "ripple": M(block=20, fx=(("vuln", 1), ("weak", 1), ("frail", 1))), "haste": M(fx=(("call", _haste),), kind="Buff")},
     _time_eater, powers={"TIME_WARP": 0})
spec("DONU", "Donu", (250, 250), {"circle": M(fx=(("str_all", 3),)), "beam": M(10, hits=2)}, cycle("circle", "beam"), powers={"ARTIFACT": 2})
spec("DECA", "Deca", (250, 250), {"beam": M(10, hits=2, fx=(("status", "DAZED", 2, "discard"),)), "square": M(fx=(("block_all", 16),))}, cycle("beam", "square"), powers={"ARTIFACT": 2})

GREMLINS = ("MAD_GREMLIN", "MAD_GREMLIN", "SNEAKY_GREMLIN", "SNEAKY_GREMLIN", "FAT_GREMLIN", "FAT_GREMLIN", "SHIELD_GREMLIN", "GREMLIN_WIZARD")
SHAPES = ("SPIKER", "REPULSOR", "EXPLODER")


def _louses(n):
    return lambda rng: [rng.choice(("RED_LOUSE", "GREEN_LOUSE")) for _ in range(n)]


def _small_slimes(rng):
    return [rng.choice(("ACID_SLIME_M", "SPIKE_SLIME_M")), rng.choice(("ACID_SLIME_S", "SPIKE_SLIME_S"))]


# act -> pool -> list of encounters; an encounter is a list of keys or a function of the random generator giving one
ENCOUNTERS = {
    1: {
        "weak": [["CULTIST"], ["JAW_WORM"], _louses(2), _small_slimes],
        "strong": [lambda rng: rng.sample(GREMLINS, 4), lambda rng: [rng.choice(("ACID_SLIME_L", "SPIKE_SLIME_L"))],
                   ["SPIKE_SLIME_S", "SPIKE_SLIME_S", "SPIKE_SLIME_S", "ACID_SLIME_S", "ACID_SLIME_S"], ["BLUE_SLAVER"], ["RED_SLAVER"], _louses(3),
                   ["FUNGI_BEAST", "FUNGI_BEAST"], lambda rng: ["LOOTER", rng.choice(("BLUE_SLAVER", "RED_LOUSE", "ACID_SLIME_M"))],
                   lambda rng: ["FUNGI_BEAST", rng.choice(("JAW_WORM", "RED_LOUSE"))], ["LOOTER"]],
        "elite": [["GREMLIN_NOB"], ["LAGAVULIN"], ["SENTRY", "SENTRY_MID", "SENTRY"]],
        "boss": [["SLIME_BOSS"], ["HEXAGHOST"], ["THE_GUARDIAN"]],
    },
    2: {
        "weak": [["SPHERIC_GUARDIAN"], ["CHOSEN"], ["SHELLED_PARASITE"], ["BYRD", "BYRD", "BYRD"], ["LOOTER", "MUGGER"]],
        "strong": [["CHOSEN", "BYRD"], ["CULTIST", "CHOSEN"], ["SENTRY", "SPHERIC_GUARDIAN"], ["SNAKE_PLANT"], ["SNECKO"], ["CENTURION", "MYSTIC"],
                   ["CULTIST", "CULTIST", "CULTIST"], ["SHELLED_PARASITE", "FUNGI_BEAST"]],
        "elite": [lambda rng: ["GREMLIN_LEADER", *rng.sample(GREMLINS, 2)], ["BLUE_SLAVER", "TASKMASTER", "RED_SLAVER"], ["BOOK_OF_STABBING"]],
        "boss": [["BRONZE_AUTOMATON"], ["THE_COLLECTOR"], ["THE_CHAMP"]],
    },
    3: {
        "weak": [["DARKLING", "DARKLING", "DARKLING"], ["ORB_WALKER"], lambda rng: [rng.choice(SHAPES) for _ in range(3)]],
        "strong": [["SPIRE_GROWTH"], ["TRANSIENT"], lambda rng: [rng.choice(SHAPES) for _ in range(4)], ["MAW"],
                   lambda rng: ["SPHERIC_GUARDIAN", rng.choice(SHAPES), rng.choice(SHAPES)], ["JAW_WORM_HORDE"] * 3,
                   ["DARKLING", "DARKLING", "DARKLING"], ["WRITHING_MASS"]],
        "elite": [["GIANT_HEAD"], ["NEMESIS"], ["REPTOMANCER", "SNAKE_DAGGER", "SNAKE_DAGGER"]],
        "boss": [["AWAKENED_ONE", "CULTIST", "CULTIST"], ["TIME_EATER"], ["DONU", "DECA"]],
    },
}
WEAK_FIGHTS = {1: 3, 2: 2, 3: 2}  # the first fights of an act are drawn from its weak pool
MINIONS = {"GREMLIN_LEADER": GREMLINS, "REPTOMANCER": ("SNAKE_DAGGER",)}
