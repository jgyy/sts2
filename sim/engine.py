"""One fight: the bot's planner picks every card and potion, the engine carries out what they and the enemies do.

The order of a round is the first game's: our turn (Block gone unless kept, energy, start-of-turn powers, draw,
cards, end-of-turn powers and orbs, discard), then each enemy in turn (its Block gone, Poison, its move), then the
debuffs tick down and the enemies choose their next moves. Damage is worked out the game's way: base plus
Strength, times Weak, stance and Vulnerable, rounded down, Block first.
"""

from __future__ import annotations

import math

from sts2bot.fight import combat
from sts2bot.run import strategy

from . import effects
from .cards import Card
from .monsters import Foe

HAND_MAX = 10
MAX_ROUNDS = 45  # a fight still going then is lost (a stalemate the planner cannot break)
MAX_ACTIONS = 80  # actions in one turn before the engine ends it for the bot
ORB_BASE = {"LIGHTNING": (3, 8), "FROST": (2, 5), "DARK": (6, 6), "PLASMA": (1, 2)}
DEBUFFS = ("WEAK", "VULNERABLE", "FRAIL")
# Counters the engine keeps among our powers that the game would not show as powers.
INTERNAL = {"DRAWN_TURN", "OSTY_MAX", "FORGE", "TURN_ENERGY", "VOID_FREE", "KEEP_HAND_ALWAYS", "SURCHARGE", "MANTRA_GAINED", "CLAW_BONUS",
            "EXPUNGER_X", "DISCARDED_TURN", "BLASPHEMY_DUE", "SIMMERING", "DEVA_ENERGY", "KEEP_HAND", "KEEP_BLOCK", "TURN_OVER", "EXTRA_TURN",
            "COMBUST_HP", "DOUBLE_DAMAGE_NEXT", "AMPLIFY", "CONFUSED", "BLUR", "WELL_LAID_PLANS"} | {f"{o}_CHANNELED" for o in ("LIGHTNING", "FROST", "DARK", "PLASMA", "GLASS")}


class Fight:
    def __init__(self, run, foes: list[Foe], kind: str):
        self.run, self.game, self.rng, self.kind = run, run.game, run.rng, kind
        self.foes = foes
        deck = [c.copy() for c in run.deck]
        self.rng.shuffle(deck)
        innate = [c for c in deck if c.has("Innate")]
        self.draw: list[Card] = [c for c in deck if c not in innate] + innate  # (the end of the list is the top)
        self.hand: list[Card] = []
        self.discard: list[Card] = []
        self.exhausted: list[Card] = []
        self.block = 0
        self.energy = 0
        self.pw: dict[str, int] = {}  # our powers
        self.temp: dict[str, int] = {}  # Strength and Dexterity that go at the end of this turn
        self.orbs: list[list] = []  # [kind, Dark's stored damage], oldest first
        self.orb_slots = run.orb_slots
        self.stance = ""
        self.mantra = 0
        self.stars = 0
        self.osty = 0 if run.character == "NECROBINDER" else -1
        self.round = 0
        self.turn_cards: list[str] = []
        self.attacks_turn = self.skills_turn = 0
        self.last_type = ""
        self.no_draw = False
        self.double_next = 0  # Double Tap: Attacks to play twice
        self.burst = 0  # Burst: Skills to play twice
        self.free_attack = False
        self.threat: dict[str, list] = {}
        self.facing: str | None = None
        self.won = self.lost = False
        self.escaped = False
        self.plays = 0  # cards the planner has played (for the trace)
        self.digest: list[str] = []
        self.triggers: list[tuple] = []  # Slay the Spire 2's Powers, read from their text: (when, what, card key)

    # ------------------------------------------------------------------ numbers

    def strength(self) -> int:
        return self.pw.get("STRENGTH", 0) + self.temp.get("STRENGTH", 0)

    def dexterity(self) -> int:
        return self.pw.get("DEXTERITY", 0) + self.temp.get("DEXTERITY", 0)

    def stance_mult(self) -> float:
        return {"WRATH": 2.0, "DIVINITY": 3.0}.get(self.stance, 1.0)

    def attack_value(self, base: float, foe: Foe | None, osty: bool = False, plain: bool = False) -> int:
        """What one hit of ours does before Block: Strength, Vigor, Weak, stance, and the target's Vulnerable (`plain`: none of them)."""
        if plain:
            return max(0, math.floor(base))
        d = base + (self.pw.get("CALCIFY", 0) if osty else self.strength() + self.pw.get("VIGOR", 0))
        if self.pw.get("WEAK") and not osty:
            d *= 0.75
        d *= 1.0 if osty else self.stance_mult()
        if foe is not None:
            if foe.powers.get("VULNERABLE"):
                d *= (1.75 if self.run.has("PAPER_PHROG") else 1.5) + (0.25 if self.pw.get("CRUELTY") else 0.0)
            if foe.powers.get("WEAK") and self.pw.get("TRACKING"):
                d *= 1.5
            if foe.powers.get("FLUTTER") or foe.powers.get("SOAR"):
                d *= 0.5
        return max(0, math.floor(d))

    def block_value(self, base: float) -> int:
        b = base + self.dexterity()
        if self.pw.get("FRAIL"):
            b *= 0.75
        return max(0, math.floor(b))

    def foe_hit(self, foe: Foe, base: int) -> int:
        """What one of its hits does to us before Block (what its intent shows)."""
        d = base + foe.powers.get("STRENGTH", 0) + foe.mem.get("spark", 0)
        if self.pw.get("SURROUNDED") and (foe.powers.get("BACK_ATTACK_LEFT") or foe.powers.get("BACK_ATTACK_RIGHT")) and foe.eid != self.facing:
            d *= 1.5
        if foe.powers.get("WEAK"):
            d *= 0.75
        if self.pw.get("VULNERABLE"):
            d *= 1.25 if self.run.has("ODD_MUSHROOM") else 1.5
        if self.stance == "WRATH":
            d *= 2
        return max(0, math.floor(d))

    # ------------------------------------------------------------------ what the bot sees

    def live(self, c: Card) -> tuple[int | None, int | None]:
        i = c.info
        base = effects.base_damage(self, c)
        dmg = self.attack_value(base, None, i.osty_attack) if base is not None and c.type == "Attack" else None
        blk = self.block_value(i.block + c.bonus) if i.block and not i.melody_block else None
        return dmg, blk

    def playable(self, c: Card) -> bool:
        cost = self.cost_of(c)
        if cost == -2 or not effects.can_play(self, c):
            return False
        if self.pw.get("ENTANGLED") and c.type == "Attack":
            return False
        if self.pw.get("SMOGGY") and c.type == "Skill" and self.skills_turn:
            return False
        if self.limit_left() == 0:
            return False
        if c.star_cost is not None and c.star_cost > self.stars:
            return False
        return cost < 0 or cost <= self.energy

    def limit_left(self) -> int | None:
        caps = [n for n in (6 if self.run.has("VELVET_CHOKER") else None, 3 if any(h.key == "NORMALITY" for h in self.hand) else None,
                            1 if self.pw.get("RINGING") else None, 3 if self.pw.get("SLOTH") else None) if n]
        return max(0, min(caps) - len(self.turn_cards)) if caps else None

    def cost_of(self, c: Card) -> int:
        cost = c.cost()
        if cost < 0:
            return cost
        if c.type == "Skill" and self.pw.get("CORRUPTION"):
            return 0
        if c.type == "Attack" and self.free_attack:
            return 0
        if self.pw.get("VOID_FREE"):
            return 0
        return cost + self.pw.get("SURCHARGE", 0)

    def state(self) -> dict:
        hand = []
        for n, c in enumerate(self.hand):
            dmg, blk = self.live(c)
            view = c.view(n, dmg, blk, playable=self.playable(c))
            view["cost"] = "X" if self.cost_of(c) == -1 else str(max(0, self.cost_of(c)))
            if not view["can_play"] and self.game.SHORT_REASON and self.cost_of(c) > self.energy and effects.can_play(self, c):
                view["unplayable_reason"] = "EnergyCostTooHigh"
            hand.append(view)
        status = [{"id": f"{k}_POWER", "amount": v, "description": ""} for k, v in {**self.pw, **self.temp_view()}.items() if v and k not in INTERNAL]
        player = {
            "character": self.run.title, "hp": self.run.hp, "max_hp": self.run.max_hp, "block": self.block, "energy": self.energy,
            "max_energy": self.run.energy, "stars": self.stars, "gold": self.run.gold, "status": status, "hand": hand,
            "draw_pile": [c.view() for c in self.draw], "discard_pile": [c.view() for c in self.discard],
            "draw_pile_count": len(self.draw), "discard_pile_count": len(self.discard), "exhaust_pile_count": len(self.exhausted),
            "potions": self.run.potion_views(), "max_potion_slots": self.run.potion_slots, "relics": self.run.relic_views(),
            "orbs": [{"id": k, "passive_val": self.orb_passive(o), "evoke_val": self.orb_evoke(o)} for o in self.orbs for k in [o[0]]],
            "orb_slots": self.orb_slots, "stance": self.stance or "Neutral",
        }
        if self.osty >= 0:
            player["pets"] = [{"name": "Osty", "hp": self.osty, "alive": self.osty > 0}]
        enemies = [self.foe_view(f) for f in self.foes if not f.gone]
        threat = {k: total / rounds for k, (total, rounds) in self.threat.items()}
        return {
            "state_type": self.kind, "run": self.run.where(), "player": player,
            "battle": {"round": self.round, "turn": "player", "is_play_phase": True, "enemies": enemies},
            "_plays_this_turn": len(self.turn_cards), "_played_names": tuple(self.turn_cards), "_dislodged": False, "_facing": self.facing,
            "_revive_pending": False, "_threat": threat, "_costs": {c.name: max(0, c.cost()) for c in self.run.deck},
        }

    def temp_view(self) -> dict:
        return {k: self.pw.get(k, 0) + v for k, v in self.temp.items() if k in ("STRENGTH", "DEXTERITY")}

    def foe_view(self, f: Foe) -> dict:
        m = f.move
        intents = []
        if self.run.has("RUNIC_DOME"):
            intents = [{"type": "Unknown", "label": ""}]
        elif m.kind:
            intents = [{"type": m.kind, "label": ""}]
        else:
            if m.dmg:
                d = self.foe_hit(f, effects.move_damage(self, f, m))
                hits = effects.move_hits(self, f, m)
                intents.append({"type": "Attack", "label": f"{d}x{hits}" if hits > 1 else str(d)})
            words = {fx[0] for fx in m.fx}
            if words & effects.DEBUFF_FX:
                intents.append({"type": "Debuff", "label": ""})
            if words & effects.BUFF_FX:
                intents.append({"type": "Buff", "label": ""})
            if m.block and not m.dmg:
                intents.append({"type": "Defend", "label": ""})
            if not intents:
                intents.append({"type": "Unknown", "label": ""})
        status = [{"id": f"{k}_POWER", "amount": v, "description": ""} for k, v in f.powers.items() if v]
        return {"entity_id": f.eid, "name": f.name, "hp": 0 if f.half_dead else f.hp, "max_hp": f.max_hp, "block": f.block,
                "status": status, "intents": intents}

    def orb_passive(self, o: list) -> int:
        base = ORB_BASE.get(o[0], (0, 0))[0]
        return base if o[0] == "PLASMA" else max(0, base + self.pw.get("FOCUS", 0))

    def orb_evoke(self, o: list) -> int:
        if o[0] == "DARK":
            return o[1]
        base = ORB_BASE.get(o[0], (0, 0))[1]
        return base if o[0] == "PLASMA" else max(0, base + self.pw.get("FOCUS", 0))

    # ------------------------------------------------------------------ hurting

    def alive(self) -> list[Foe]:
        return [f for f in self.foes if f.alive]

    def hurt(self, foe: Foe, amount: int, attack: bool = True) -> int:
        """Damage on an enemy; the HP it lost."""
        if not foe.alive or amount <= 0:
            return 0
        if foe.powers.get("INTANGIBLE"):
            amount = min(amount, 1)
        if attack and foe.powers.get("SLIPPERY"):
            foe.powers["SLIPPERY"] -= 1
            amount = min(amount, 1)
        if foe.powers.get("HARD_TO_KILL"):
            amount = min(amount, foe.block + foe.powers["HARD_TO_KILL"])
        if foe.powers.get("HARDENED_SHELL"):
            room = max(0, foe.powers["HARDENED_SHELL"] - foe.mem.get("shell", 0))
            amount = min(amount, foe.block + room)
        if attack and self.run.has("THE_BOOT") and 0 < amount < 5:
            amount = 5
        took = min(foe.block, amount)
        foe.block -= took
        rest = min(foe.hp, amount - took)
        if attack and foe.powers.get("CURL_UP") and amount > 0:
            foe.block += foe.powers.pop("CURL_UP")
        if foe.powers.get("SKITTISH") and not foe.mem.get("skittish") and amount > 0:
            foe.mem["skittish"] = True
            foe.block += foe.powers["SKITTISH"]
        if took and foe.block == 0 and foe.powers.get("BURROWED"):
            foe.powers.pop("BURROWED")
            stun = next((m for m in foe.spec.moves if foe.spec.moves[m].kind == "Stun"), None)
            if stun:
                foe.move_name = stun
        if rest > 0:
            foe.hp -= rest
            foe.mem["shell"] = foe.mem.get("shell", 0) + rest
            effects.foe_hurt(self, foe, rest, attack)
        if foe.hp <= 0:
            self.kill(foe)
        return rest

    def kill(self, foe: Foe) -> None:
        foe.hp = 0
        hook = foe.spec.hooks.get("death")
        if hook and hook(self, foe):
            return  # it is not gone (a Darkling waiting to rise, the Awakened One reborn)
        foe.gone = True
        for ally in self.foes:
            if ally.alive and ally.powers.get("CRAB_RAGE"):
                ally.powers["STRENGTH"] = ally.powers.get("STRENGTH", 0) + 6
                ally.block += 99
        if foe.mem.get("corpse"):
            for x in self.alive():
                self.hurt(x, foe.max_hp, attack=False)
        if self.run.has("GREMLIN_HORN"):
            self.energy += 1
            self.draw_cards(1)
        if not any(f.alive and not f.minion for f in self.foes):
            for f in self.foes:
                f.gone = True
            self.won = True

    def take(self, amount: int, foe: Foe | None = None, attack: bool = True) -> int:
        """Damage on us; the HP we lost."""
        if amount <= 0:
            return 0
        if self.pw.get("INTANGIBLE"):
            amount = min(amount, 1)
        took = min(self.block, amount)
        self.block -= took
        rest = amount - took
        if rest and self.osty > 0 and attack:
            soak = min(self.osty, rest)
            self.osty -= soak
            rest -= soak
        if attack and foe is not None:
            thorns = self.pw.get("THORNS", 0) + self.temp.get("FLAME_BARRIER", 0)
            if thorns:
                self.hurt(foe, thorns, attack=False)
        if rest <= 0:
            return 0
        if attack and self.run.has("TORII") and rest <= 5:
            rest = 1
        if self.run.has("TUNGSTEN_ROD"):
            rest -= 1
        if rest <= 0:
            return 0
        if self.pw.get("BUFFER"):
            self.pw["BUFFER"] -= 1
            return 0
        return self.lose_hp(rest)

    def lose_hp(self, n: int, from_card: bool = False) -> int:
        if n <= 0:
            return 0
        n = min(n, self.run.hp)
        self.run.hp -= n
        if from_card and self.pw.get("RUPTURE"):
            self.pw["STRENGTH"] = self.strength() - self.temp.get("STRENGTH", 0) + self.pw["RUPTURE"]
        if self.run.hp <= 0:
            effects.on_death(self)
        return n

    def heal(self, n: int) -> None:
        self.run.hp = min(self.run.max_hp, self.run.hp + max(0, n))

    def gain_block(self, n: int) -> None:
        if n <= 0 or self.pw.get("NO_BLOCK"):
            return
        n *= 2 if self.pw.get("SHADOWMELD") else 1
        self.block += n
        effects.block_gained(self, n)

    # ------------------------------------------------------------------ piles

    def draw_cards(self, n: int) -> None:
        if self.no_draw:
            return
        for _ in range(n):
            if len(self.hand) >= HAND_MAX:
                return
            if not self.draw:
                if not self.discard:
                    return
                self.draw, self.discard = self.discard, []
                self.rng.shuffle(self.draw)
            c = self.draw.pop()
            self.hand.append(c)
            self.pw["DRAWN_TURN"] = self.pw.get("DRAWN_TURN", 0) + 1
            effects.drawn(self, c)

    def add_card(self, c: Card, where: str = "hand") -> None:
        if where == "hand" and len(self.hand) < HAND_MAX:
            self.hand.append(c)
        elif where == "draw":
            self.draw.insert(self.rng.randrange(len(self.draw) + 1), c)
        elif where == "top":
            self.draw.append(c)
        else:
            self.discard.append(c)

    def exhaust(self, c: Card) -> None:
        self.exhausted.append(c)
        effects.exhausted(self, c)

    # ------------------------------------------------------------------ orbs and stances

    def channel(self, kind: str) -> None:
        if self.orb_slots <= 0:
            return
        if len(self.orbs) >= self.orb_slots:
            self.evoke()
        self.orbs.append([kind, 6 + self.pw.get("FOCUS", 0) if kind == "DARK" else 0])

    def evoke(self, times: int = 1, keep: bool = False) -> None:
        if not self.orbs:
            return
        o = self.orbs[0]
        for _ in range(times):
            self.orb_effect(o, self.orb_evoke(o), evoke=True)
        if not keep:
            self.orbs.pop(0)

    def orb_effect(self, o: list, amount: int, evoke: bool) -> None:
        kind = o[0]
        if kind == "FROST":
            self.gain_block(amount)
        elif kind == "PLASMA":
            self.energy += amount
        elif kind == "LIGHTNING":
            targets = self.alive() if self.pw.get("ELECTRODYNAMICS") else [self.rng.choice(self.alive())] if self.alive() else []
            for f in targets:
                self.hurt(f, amount, attack=False)
        elif kind == "DARK" and evoke and self.alive():
            self.hurt(min(self.alive(), key=lambda f: f.hp), amount, attack=False)

    def enter(self, stance: str) -> None:
        new = "" if stance == "EXIT" else stance
        if new == self.stance:
            return
        if self.stance == "CALM":
            self.energy += 2 + (1 if self.run.has("VIOLET_LOTUS") else 0)
        if new == "DIVINITY":
            self.energy += 3
        self.stance = new
        effects.stance_changed(self, new)

    def gain_mantra(self, n: int) -> None:
        self.mantra += n
        self.pw["MANTRA_GAINED"] = self.pw.get("MANTRA_GAINED", 0) + n
        if self.mantra >= 10:
            self.mantra -= 10
            self.enter("DIVINITY")

    # ------------------------------------------------------------------ playing

    def play(self, index: int, target: str | None) -> bool:
        if not 0 <= index < len(self.hand):
            return False
        c = self.hand[index]
        if not self.playable(c):
            return False
        cost = self.cost_of(c)
        x = 0
        if cost == -1:
            x = self.energy + (2 if self.run.has("CHEMICAL_X") else 0)
            self.energy = 0
        else:
            self.energy -= cost
        if c.star_cost:
            self.stars -= c.star_cost
            self.game_hook("stars", c)
        if self.pw.get("VOID_FREE") and cost >= 0:
            self.pw["VOID_FREE"] -= 1
        if c.type == "Attack" and self.free_attack:
            self.free_attack = False
        foe = next((f for f in self.foes if f.eid == target and f.alive), None)
        if c.info.needs_target and foe is None:
            alive = self.alive()
            foe = min(alive, key=lambda f: f.hp) if alive else None
        if foe is not None:
            self.facing = foe.eid
        self.hand.pop(index)
        self.turn_cards.append(c.name)
        times = 1
        if c.type == "Attack" and self.double_next:
            self.double_next, times = self.double_next - 1, 2
        elif c.type == "Skill" and self.burst:
            self.burst, times = self.burst - 1, 2
        if self.pw.get("DUPLICATION"):
            self.pw["DUPLICATION"] -= 1
            times += 1
        if self.pw.get("ECHO_FORM", 0) > len(self.turn_cards) - 1:
            times += 1
        if c.type == "Power" and self.pw.get("AMPLIFY"):
            self.pw["AMPLIFY"] -= 1
            times += 1
        times += c.info.replay
        for _ in range(times):
            if self.won or self.lost:
                break
            effects.resolve(self, c, foe if foe is None or foe.alive else (self.alive() or [None])[0], x)
        if c.type == "Attack":
            self.attacks_turn += 1
        elif c.type == "Skill":
            self.skills_turn += 1
        self.last_type = c.type
        effects.played(self, c)
        self.put_away(c)
        return True

    def put_away(self, c: Card) -> None:
        if c.type == "Power":
            return
        if c.key in effects.BACK_TO_DRAW:
            self.add_card(c, "draw")
        elif c.has("Exhaust") or (c.type == "Skill" and self.pw.get("CORRUPTION")):
            self.exhaust(c)
        else:
            self.discard.append(c)

    def use_potion(self, slot: int, target: str | None) -> bool:
        pot = self.run.potion_at(slot)
        if pot is None or not effects.potion_usable(self, pot):
            return False
        foe = next((f for f in self.foes if f.eid == target and f.alive), None) or (self.alive() or [None])[0]
        self.run.drop_potion(slot)
        self.run.log["potions"] = self.run.log.get("potions", 0) + 1
        effects.drink(self, pot, foe)
        if self.run.has("TOY_ORNITHOPTER"):
            self.heal(5)
        return True

    # ------------------------------------------------------------------ the fight

    def start(self) -> None:
        effects.fight_start(self)
        for f in self.foes:
            f.choose(self.rng)

    def game_hook(self, what: str, card=None) -> None:
        hook = getattr(self.game, "ON", None)
        if hook is not None:
            hook(self, what, card)

    def player_turn_start(self) -> None:
        self.round += 1
        for f in self.foes:
            f.mem.pop("shell", None)
            f.mem.pop("skittish", None)
        self.turn_cards, self.attacks_turn, self.skills_turn = [], 0, 0
        self.pw.pop("DISCARDED_TURN", None)
        self.no_draw = False
        if self.round > 1 and not self.pw.get("BARRICADE") and not self.pw.pop("KEEP_BLOCK", 0):
            self.block = max(0, self.block - 15) if self.run.has("CALIPERS") else 0
        if self.stance == "DIVINITY":
            self.enter("")
        self.energy = max(0, self.run.energy - self.pw.get("ENERGY_DOWN", 0)) + self.pw.pop("ENERGY_NEXT_TURN", 0)
        effects.turn_start(self)
        draw = 5 + self.pw.pop("DRAW_CARDS_NEXT_TURN", 0) + effects.extra_draw(self) - self.pw.pop("DRAW_REDUCTION", 0)
        self.draw_cards(max(0, draw))
        effects.after_draw(self)

    def player_turn_end(self) -> None:
        effects.turn_end(self)
        keep_all = self.pw.pop("KEEP_HAND", 0) or self.run.has("RUNIC_PYRAMID")
        planned = sorted((c for c in self.hand if c.type not in ("Status", "Curse") and not c.has("Retain")), key=lambda c: -c.cost())
        kept = set(map(id, planned[: self.pw.get("WELL_LAID_PLANS", 0)]))
        for c in list(self.hand):
            if c.has("Retain") or id(c) in kept or keep_all and c.type not in ("Status", "Curse") and not c.has("Ethereal"):
                c.retained += 1
                effects.retained(self, c)
                continue
            self.hand.remove(c)
            if c.has("Ethereal"):
                self.exhaust(c)
            else:
                self.discard.append(c)
        for k in ("STRENGTH", "DEXTERITY"):
            self.temp.pop(k, None)
        self.temp.pop("FLAME_BARRIER", None)
        self.pw.pop("RAGE", None)
        self.pw.pop("ENTANGLED", None)
        self.free_attack = False
        for c in self.hand + self.draw + self.discard:
            c.cost_turn = None
        for f in self.foes:
            back = f.mem.pop("str_back", 0)
            if back:
                f.powers["STRENGTH"] = f.powers.get("STRENGTH", 0) + back

    def enemies_turn(self) -> None:
        for f in list(self.foes):
            if self.won or self.lost:
                return
            if not f.alive:
                continue
            if not (f.powers.get("BARRICADE") or f.powers.get("BURROWED")):
                f.block = 0
            if f.powers.get("POISON"):
                dose = f.powers["POISON"]
                f.powers["POISON"] = dose - 1
                self.hurt(f, dose, attack=False) if not f.powers.get("INTANGIBLE") else self.hurt(f, 1, attack=False)
                if not f.alive:
                    continue
            m = f.move
            f.history.append(f.move_name)
            if m.kind == "Escape" or any(fx[0] == "escape" for fx in m.fx):
                f.gone = True
                if not any(x.alive and not x.minion for x in self.foes):
                    self.escaped = self.won = True
                continue
            effects.foe_acts(self, f, m)
            if f.powers.get("DOOM") and f.alive and f.powers["DOOM"] >= f.hp:
                self.kill(f)
        self.end_of_round()

    def end_of_round(self) -> None:
        for k in DEBUFFS:
            if self.pw.get(k):
                self.pw[k] -= 1
        for k in ("INTANGIBLE",):
            if self.pw.get(k):
                self.pw[k] -= 1
        if self.pw.get("REGEN"):
            self.heal(self.pw["REGEN"])
            self.pw["REGEN"] -= 1
        for f in self.foes:
            if not f.alive:
                continue
            for k in ("WEAK", "VULNERABLE", "INTANGIBLE"):
                if f.powers.get(k):
                    f.powers[k] -= 1
            effects.foe_round_end(self, f)
        self.game.ROUND_END(self)
        for f in self.foes:
            if f.alive:
                f.choose(self.rng)

    def note_threat(self) -> None:
        for f in self.foes:
            if f.alive:
                seen = self.threat.setdefault(f.eid, [0, 0])
                m = f.move
                seen[0] += self.foe_hit(f, effects.move_damage(self, f, m)) * effects.move_hits(self, f, m) if m.dmg and not m.kind else 0
                seen[1] += 1

    def bot_turn(self) -> None:
        """Ask the planner for one action at a time until it ends the turn."""
        for _ in range(MAX_ACTIONS):
            if self.won or self.lost or self.pw.pop("TURN_OVER", 0):
                return
            state = self.state()
            todo = combat.actions(state)
            done = False
            for action, params in todo:
                if action == "end_turn":
                    self.digest.append("E")
                    return
                if action == "play_card" and self.play(int(params.get("card_index", -1)), params.get("target")):
                    self.digest.append(f"P{params.get('card_index')}{params.get('target') or ''}")
                    self.plays += 1
                    done = True
                    break
                if action == "use_potion" and self.use_potion(int(params.get("slot", -1)), params.get("target")):
                    self.digest.append(f"Q{params.get('slot')}")
                    done = True
                    break
            if not done:
                return

    def run_fight(self) -> bool:
        """Play the fight out. True if it was won (or escaped from)."""
        self.start()
        while not (self.won or self.lost):
            if self.round >= MAX_ROUNDS:
                self.lost = True
                break
            self.player_turn_start()
            if self.won or self.lost:
                break
            if self.kind == "monster":
                self.note_threat()
            self.bot_turn()
            if self.won or self.lost:
                break
            self.player_turn_end()
            if self.won or self.lost:
                break
            if self.pw.pop("EXTRA_TURN", 0):
                continue
            self.enemies_turn()
        return self.won and not self.lost

    def hand_pick(self, prompt: str, n: int, cards: list[Card] | None = None) -> list[Card]:
        """Cards from the hand for a card's "choose a card" (the bot's hand_select handler orders them)."""
        pool = cards if cards is not None else self.hand
        if not pool or n <= 0:
            return []
        state = self.state()
        state.update(state_type="hand_select", _fight=self.kind,
                     hand_select={"prompt": prompt, "cards": [c.view(i) for i, c in enumerate(pool)], "can_confirm": False, "mode": "simple_select"})
        order = [p["card_index"] for a, p in strategy.hand_select(state, self.run.mem) if a == "combat_select_card"]
        seen: list[int] = []
        for i in order:
            if 0 <= i < len(pool) and i not in seen:
                seen.append(i)
        return [pool[i] for i in seen[:n]]

    def pile_pick(self, prompt: str, pool: list[Card], n: int = 1) -> list[Card]:
        """Cards from a pile for a "choose" screen (Headbutt, Exhume, Seek): the bot's card_select handler orders them."""
        if not pool:
            return []
        state = {"state_type": "card_select", "run": self.run.where(), "player": {"hp": self.run.hp, "max_hp": self.run.max_hp},
                 "card_select": {"screen_type": "choose", "prompt": prompt, "cards": [c.view(i) for i, c in enumerate(pool)]}}
        mem = self.run.mem
        mem.select_key = ""
        order = [p["index"] for a, p in strategy.card_select(state, mem) if a == "select_card"]
        return [pool[i] for i in order[:n] if 0 <= i < len(pool)]
