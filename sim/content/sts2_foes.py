"""Slay the Spire 2's monsters, built from sts2_data.json: each one's moves and the state machine that picks them.

The data (Spire Codex's numbers, spire-mentor's state machines) names powers the engine has to act out; the
ones that change a fight are handled in `HOOKS` and `effects`, the rest are shown to the planner and do nothing.
"""

from __future__ import annotations

import re

from ..monsters import Foe, Move, Spec

PLAYER_DEBUFF = {"WEAK": "weak", "FRAIL": "frail", "VULNERABLE": "vuln"}
INTENT_KIND = {"stun": "Stun", "sleep": "Sleep", "escape": "Escape"}


def move_of(mv: dict, asc: int) -> Move:
    if mv.get("id") == "EXPLODE":
        return Move(1, 1, 0, (), "")
    dmg = mv.get("damage_a9", mv.get("damage")) if asc >= 9 else mv.get("damage_a8", mv.get("damage")) if asc >= 8 else mv.get("damage")
    hits = mv.get("hits_a9", mv.get("hits", 1)) if asc >= 9 else mv.get("hits", 1)
    block = mv.get("block_a9", mv.get("block_a8", mv.get("block"))) if asc >= 9 else mv.get("block_a8", mv.get("block")) if asc >= 8 else mv.get("block")
    fx = []
    for p in mv.get("powers") or []:
        name, n = str(p["power"]).upper(), p.get("amount_a9", p.get("amount", 1)) if asc >= 9 else p.get("amount", 1)
        target = str(p.get("target") or "self")
        if target.startswith("player"):
            fx.append((PLAYER_DEBUFF[name], n) if name in PLAYER_DEBUFF else ("player_power", name, n))
        elif target.startswith("all") or target.startswith("Torch"):
            fx.append(("str_all", n) if name == "STRENGTH" else ("power", name, n))
        else:
            fx.append(("str", n) if name == "STRENGTH" else ("power", name, n))
    for s in mv.get("status_cards") or []:
        card = str(s.get("card") or "")
        if card == "choice":
            fx.append(("call", _curse_of_knowledge))
            continue
        pile = "hand" if str(s.get("pile")).startswith("hand") else "draw" if str(s.get("pile")).startswith("draw") else "discard"
        count = s.get("count_a9", s.get("count", 1)) if asc >= 9 else s.get("count", 1)
        fx.append(("status", card, count, pile))
    if mv.get("heal"):
        fx.append(("heal", int(mv["heal"]) if str(mv["heal"]).isdigit() else 10))
    intents = [str(i).lower() for i in mv.get("intent") or []]
    if "summon" in intents:
        fx.append(("call", _summon))
    kind = next((INTENT_KIND[i] for i in intents if i in INTENT_KIND), "")
    if not dmg and not block and not fx and not kind:
        kind = "Unknown"
    return Move(int(dmg or 0), int(hits or 1), int(block or 0), tuple(fx), kind)


def pattern_of(m: dict) -> callable:
    states = {s["id"]: s for s in m["states"]}
    moves = {mv["id"] for mv in m["moves"]}

    def resolve(f, state_id: str, rng, depth: int = 0) -> str:
        st = states.get(state_id)
        if st is None or depth > 8:
            return state_id if state_id in moves else next(iter(moves))
        if st["type"] == "move":
            f.mem["state"] = st["id"]
            return st.get("move") or st["id"]
        if st["type"] == "conditional":
            for b in st.get("branches") or []:
                if _holds(f, b.get("if")):
                    return resolve(f, b.get("move"), rng, depth + 1) if b.get("move") in states else _as_move(f, b.get("move"), states)
            return next(iter(moves))
        branches = [b for b in st.get("branches") or [] if _allowed(f, b)]
        if not branches:
            branches = st.get("branches") or []
        total = sum(float(b.get("weight", 1.0)) for b in branches)
        r = rng.random() * total
        for b in branches:
            r -= float(b.get("weight", 1.0))
            if r <= 0:
                return resolve(f, b["move"], rng, depth + 1) if b["move"] in states and states[b["move"]]["type"] != "move" else _as_move(f, b["move"], states)
        return _as_move(f, branches[-1]["move"], states)

    def pick(f, rng):
        forced = f.mem.pop("force", None)
        if forced:
            return resolve(f, forced, rng)
        if "state" not in f.mem:
            return resolve(f, _initial(f, m, states, rng), rng)
        nxt = states.get(f.mem["state"], {}).get("next")
        return resolve(f, nxt or f.mem["state"], rng)
    return pick


def _as_move(f, name: str, states: dict) -> str:
    st = states.get(name)
    if st is not None and st["type"] == "move":
        f.mem["state"] = st["id"]
        return st.get("move") or name
    f.mem["state"] = name
    return name


def _allowed(f, b: dict) -> bool:
    move, rule = b.get("move"), b.get("repeat")
    hist = f.history
    if rule == "CannotRepeat" and hist and hist[-1] == move:
        return False
    if rule == "CanRepeatXTimes":
        most = int(b.get("max_times") or 2)
        if len(hist) >= most and all(h == move for h in hist[-most:]):
            return False
    if rule == "UseOnlyOnce" and move in hist:
        return False
    return True


def _holds(f, cond) -> bool:
    fight = f.mem.get("fight")
    if cond in (None, "always_true"):
        return True
    if cond == "lagavulin_asleep":
        return f.powers.get("ASLEEP", 0) > 0
    if cond == "bowlbug_rock_is_off_balance":
        return bool(f.mem.pop("off_balance", False))
    if cond == "queen_has_amalgam_died":
        return any(x.key == "TORCH_HEAD_AMALGAM" and x.gone for x in fight.foes) if fight else False
    if cond in ("living_shield_has_allies", "living_shield_alone"):
        allies = sum(1 for x in fight.foes if x.alive and x is not f) if fight else 0
        return (allies > 0) == (cond == "living_shield_has_allies")
    if cond in ("fabricator_can_fabricate", "ovicopter_can_lay", "two_tailed_rat_can_summon"):
        return (sum(1 for x in fight.foes if x.alive) if fight else 5) < 4
    if cond == "frog_knight_should_charge":
        return f.hp < f.max_hp // 2 and not f.mem.get("charged")
    if cond == "knowledge_demon_curse_counter_lt_three":
        return f.mem.get("curses", 0) < 3
    if cond == "test_subject_respawns_lt_two":
        return f.mem.get("respawns", 0) < 2
    return False


def _initial(f, m: dict, states: dict, rng) -> str:
    init = str(m.get("initial") or "")
    if init in states:
        return init
    names = re.findall(r'"([A-Z_]+)"', init)
    if not names:
        return m["states"][0]["id"] if m["states"] else ""
    slot = f.mem.get("slot", 0)
    if "is_alone" in init:
        return names[0] if f.mem.get("alone") else names[1] if slot == 0 else names[-1]
    if "dread" in init:
        return names[-1]  # (Dread mode is the beta's: the usual opener is the other one)
    if init.startswith("if "):
        return names[0] if slot == 0 else names[-1]
    if "starter_idx" in init:
        return rng.choice(names)
    return names[min(slot, len(names) - 1)]


FIXES = {  # what the data leaves out: the Test Subject's first form (the planner's notes put it at 100 HP), the Pressure Gun's 20 (23),
    "TEST_SUBJECT": {"hp": [100, 100], "hp_a8": [110, 110]},  # the Aeonglass's Withering Presence (the bot's logs, scripts/bosses/aeonfight.py)
    "AEONGLASS": {"powers": [{"power": "ARTIFACT", "amount": 3}, {"power": "WITHERING_PRESENCE", "amount": 6}]},
}
MOVE_FIXES = {("WATERFALL_GIANT", "PRESSURE_GUN"): {"damage": 20, "damage_a9": 23}}


def spec_of(key: str, m: dict, asc: int) -> Spec:
    m = {**m, **FIXES.get(key, {})}
    moves = {mv["id"]: move_of({**mv, **MOVE_FIXES.get((key, mv["id"]), {})}, asc) for mv in m["moves"]}
    for st in m["states"]:  # (state ids that only name a move, "X_MOVE" for "X")
        if st["type"] == "move" and st.get("move") not in moves and st["id"] in moves:
            st["move"] = st["id"]
    powers = {}
    for p in m.get("powers") or []:
        powers[str(p["power"]).upper()] = int(p.get("amount_a9") if asc >= 9 and p.get("amount_a9") is not None else p.get("amount") or 1)
    hp = m["hp_a8"] if asc >= 8 else m["hp"]
    return Spec(key, m["name"], (int(hp[0]), int(hp[1])), moves, pattern_of(m), powers=powers, minion="MINION" in powers,
                hooks=dict(HOOKS.get(key, {})))


# ------------------------------------------------------------------ special cases


def _summon(f, foe):
    minion = SUMMONS.get(foe.key)
    if minion and sum(1 for x in f.foes if x.alive) < 5:
        new = f.game.spawn(minion, f, minion=True)
        f.foes.append(new)
        new.choose(f.rng)


def _curse_of_knowledge(f, foe):
    """The Knowledge Demon's offer: the bot takes the burn (Disintegration) every time, as its afflictions.py mostly does."""
    foe.mem["curses"] = foe.mem.get("curses", 0) + 1
    f.pw["DISINTEGRATION"] = f.pw.get("DISINTEGRATION", 0) + 5 + foe.mem["curses"]


def _plow_hit(f, foe, amount, attack):
    if foe.powers.get("PLOW") and foe.hp <= foe.powers["PLOW"]:
        foe.powers.pop("PLOW")
        foe.powers.pop("STRENGTH", None)
        foe.mem["force"] = "STUN"
        foe.move_name = "STUN"
        foe.mem["state"] = "STUN"


def _shriek_hit(f, foe, amount, attack):
    if foe.powers.get("SHRIEK") and foe.hp <= foe.powers["SHRIEK"]:
        foe.powers.pop("SHRIEK")
        foe.stun()


def _adaptable_death(f, foe):
    """The Test Subject as the bot's logs have it (scripts/bosses/subjectfight.py): a form beaten does not attack, and the
    next (100 HP more) acts the round after with its Strength and debuffs at nothing. Form 2 has Painful Stabs and a
    Multi-Claw of 3 hits and one more each round; form 3 Nemesis, Intangible on its first round and every other one."""
    n = foe.mem.get("respawns", 0)
    if n >= 2:
        return False
    foe.mem["respawns"] = n + 1
    foe.half_dead, foe.hp = True, 0
    foe.mem["revive"] = 1
    foe.mem["form_hp"] = foe.max_hp + 100
    foe.powers = {"ADAPTABLE": 1, "PAINFUL_STABS": 1} if n == 0 else {"NEMESIS": 1}
    foe.mem["nemesis"] = False
    foe.mem[("hits", "MULTI_CLAW")] = 3
    foe.mem["force"] = ("MULTI_CLAW", "PHASE3_LACERATE")[n]
    return True


def _intensity(f, foe):
    """Increasing Intensity: every Wither goes up a level (3 more damage), and the Strength it gains grows by 1 each time."""
    if foe.move_name != "INCREASING_INTENSITY":
        return
    foe.powers["STRENGTH"] = foe.powers.get("STRENGTH", 0) + foe.mem.get("wither_level", 0)
    foe.mem["wither_level"] = level = foe.mem.get("wither_level", 0) + 1
    for c in f.hand + f.draw + f.discard:
        if c.key == "WITHER":
            c.d = f.game.wither(level)


def slumber(foe, turn: bool = False) -> None:
    """The Slumbering Beetle wakes after its Slumber's count of turns, or once it has lost HP three times."""
    if turn:
        foe.powers["SLUMBER"] -= 1
    else:
        foe.mem["slumber_hits"] = foe.mem.get("slumber_hits", 0) + 1
    if foe.powers.get("SLUMBER", 0) <= 0 or foe.mem.get("slumber_hits", 0) >= 3:
        foe.powers.pop("SLUMBER", None)
        foe.mem["force"] = "WAKE_UP_MOVE"


def _giant_death(f, foe):
    """The Waterfall Giant: beaten, it stands one more turn out of reach and blows up for its Steam Eruption."""
    if foe.mem.get("exploded"):
        return False
    foe.mem["exploded"] = True
    foe.hp = foe.max_hp = 999_999_999
    foe.mem[("dmg", "EXPLODE")] = max(10, foe.powers.get("STEAM_ERUPTION", 15))
    foe.move_name = "EXPLODE"
    foe.mem["state"] = "EXPLODE"
    return True


def _stock_death(f, foe):
    if foe.powers.get("STOCK", 0) > 0:
        # "When killed, a new Axebot replaces it": the old one's Strength (Boot Up), Block and debuffs go with it.
        foe.powers, foe.block = {**foe.spec.powers, "STOCK": foe.powers["STOCK"] - 1}, 0
        foe.max_hp += 10
        foe.hp = foe.max_hp
        return True
    return False


def _illusion_death(f, foe):
    foe.half_dead, foe.hp = True, 0
    foe.mem["revive"] = 1
    return True


def _reattach_death(f, foe):
    others = [x for x in f.foes if x is not foe and x.alive and x.powers.get("REATTACH")]
    if not others:
        return False
    foe.half_dead, foe.hp = True, 0
    foe.mem["revive"] = 2
    return True


HOOKS = {
    "CEREMONIAL_BEAST": {"hit": _plow_hit},
    "WATERFALL_GIANT": {"death": _giant_death, "acted": lambda f, foe: f.kill(foe) if foe.mem.get("exploded") and foe.move_name == "EXPLODE" else None},
    "SPINY_TOAD": {"acted": lambda f, foe: foe.powers.pop("THORNS", None) if foe.move_name == "SPIKE_EXPLOSION" else None},
    # Soar: "Receives 50% less attack damage until it lands" (Spire Codex): Judicial Flight takes off, Verdict comes down.
    "OWL_MAGISTRATE": {"acted": lambda f, foe: foe.powers.pop("SOAR", None) if foe.move_name == "VERDICT" else None},
    "TERROR_EEL": {"hit": _shriek_hit},
    "TEST_SUBJECT": {"death": _adaptable_death,
                     "acted": lambda f, foe: foe.mem.__setitem__(("hits", "MULTI_CLAW"), foe.mem[("hits", "MULTI_CLAW")] + 1) if foe.move_name == "MULTI_CLAW" else None},
    "AEONGLASS": {"acted": _intensity},
    "SLUMBERING_BEETLE": {"hit": lambda f, foe, amount, attack: slumber(foe) if foe.powers.get("SLUMBER") else None},
    "AXEBOT": {"death": _stock_death},
    "EYE_WITH_TEETH": {"death": _illusion_death},
    "PARAFRIGHT": {"death": _illusion_death},
    **{k: {"death": _reattach_death} for k in ("DECIMILLIPEDE_SEGMENT", "DECIMILLIPEDE_SEGMENT_FRONT", "DECIMILLIPEDE_SEGMENT_MIDDLE",
                                                "DECIMILLIPEDE_SEGMENT_BACK")},
}
# Who a summoning move brings (the data says "summon" without naming it).
SUMMONS = {"FABRICATOR": "ZAPBOT", "OVICOPTER": "TOUGH_EGG", "TWO_TAILED_RAT": "TWO_TAILED_RAT", "PHROG_PARASITE": "WRIGGLER",
           "QUEEN": "TORCH_HEAD_AMALGAM", "THE_OBSCURA": "PARAFRIGHT", "KIN_PRIEST": "KIN_FOLLOWER", "SOUL_NEXUS": "WRIGGLER",
           "LIVING_SHIELD": "LIVING_SHIELD"}


def revive_round(f) -> None:
    """The half-dead come back: Illusions at full HP, Decimillipede segments on their Reattach HP, the Test Subject in its next form
    (the round's end then picks their move with everyone else's)."""
    for x in f.foes:
        if x.half_dead and not x.gone:
            x.mem["revive"] = x.mem.get("revive", 1) - 1
            if x.mem["revive"] <= 0:
                x.half_dead = False
                x.hp = x.powers.get("REATTACH") or x.max_hp
                if x.key == "TEST_SUBJECT":
                    x.max_hp = x.hp = x.mem.pop("form_hp", x.max_hp)


def new_foe(spec: Spec, n: int, hp: int, slot: int, alone: bool) -> Foe:
    foe = Foe(spec, n, hp)
    foe.mem["slot"], foe.mem["alone"] = slot, alone
    return foe
