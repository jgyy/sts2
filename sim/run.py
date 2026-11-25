"""One run: the map, the rooms, the rewards, the shops and the rest sites, every choice made by the bot's own code."""

from __future__ import annotations

import random
import time

from sts2bot import cards as bot_cards, learn
from sts2bot.run import strategy

from .cards import Card
from .engine import Fight


class Potion:
    def __init__(self, key: str, name: str, text: str, rarity: str = "Common"):
        self.key, self.name, self.text, self.rarity = key, name, text, rarity

    def view(self, slot: int, keep: set) -> dict:
        low = self.text.lower()
        aimed = "all enemies" not in low and any(w in low for w in ("deal ", "apply "))
        return {"slot": slot, "id": self.key, "name": self.name, "description": self.text, "target_type": "AnyEnemy" if aimed else "Self",
                "can_use_in_combat": self.key not in keep, "can_discard": True}


class Run:
    def __init__(self, game, character: str, seed: int, knobs: dict | None = None, ascension: int = 0):
        self.game, self.character, self.seed = game, character, seed
        self.rng = random.Random(seed)
        self.asc = ascension
        start = game.CHARACTERS[character]
        self.title = start["title"]
        self.max_hp = start["hp"] - game.max_hp_penalty(ascension, character)
        self.hp = self.max_hp if ascension < 6 else int(self.max_hp * 0.9)
        self.gold = 99
        self.energy = 3
        self.orb_slots = start.get("orbs", 0)
        self.deck: list[Card] = [game.make(k) for k in start["deck"]]
        self.relics: list[str] = []
        self.potions: list = [None] * 3
        self.counters: dict = {}
        self.act, self.floor = 1, 0
        self.mem = strategy.Memory()
        self.mem.card_stats = None
        self.mem.upgrade_gain = self._upgrade_gain
        self.knobs = dict(knobs or {})
        self.log = {"fights": [], "picks": [], "elites": 0, "rests": 0, "smiths": 0, "removals": 0, "shops": 0, "events": 0, "bought": [], "skips": 0}
        self.digest: list[str] = []
        self.unknown_odds = {"Monster": 0.1, "Shop": 0.03, "Treasure": 0.02}
        self.recent: list[str] = []
        self.potion_chance = 0.4
        for rid in start["relics"]:
            self.gain_relic(rid)
        game.setup_run(self)

    # ------------------------------------------------------------------ what the bot sees of us

    @property
    def potion_slots(self) -> int:
        return len(self.potions)

    def has(self, relic: str) -> bool:
        return relic in self.relics

    def where(self) -> dict:
        return {"act": self.act, "floor": self.floor, "ascension": self.asc}

    def relic_views(self) -> list[dict]:
        return [{"id": r, "name": r.replace("_", " ").title(), "description": self.game.relic_text(r), "counter": self.counters.get(r, -1)} for r in self.relics]

    def potion_views(self) -> list[dict]:
        return [p.view(i, self.game.KEEP_POTIONS) for i, p in enumerate(self.potions) if p is not None]

    def potion_at(self, slot: int):
        return self.potions[slot] if 0 <= slot < len(self.potions) else None

    def drop_potion(self, slot: int) -> None:
        if 0 <= slot < len(self.potions):
            self.potions[slot] = None

    def add_potion(self, p) -> bool:
        if self.has("SOZU") or None not in self.potions:
            return False
        self.potions[self.potions.index(None)] = p
        return True

    def player(self) -> dict:
        return {"character": self.title, "hp": self.hp, "max_hp": self.max_hp, "gold": self.gold, "block": 0, "status": [],
                "relics": self.relic_views(), "potions": self.potion_views(), "max_potion_slots": self.potion_slots}

    def screen(self, kind: str, **body) -> dict:
        return {"state_type": kind, "run": self.where(), "player": self.player(), **body}

    def _upgrade_gain(self, card: dict):
        d = self.game.CARDS.get(str(card.get("id") or ""))
        if d is None:
            return None
        return bot_cards.upgrade_gain(Card(d).view(), Card(d, True).view())

    def note_deck(self) -> None:
        """What the bot remembers of its deck as a fight begins (bot.Bot._note_deck)."""
        self.mem.deck = [{k: v for k, v in c.view().items() if k in ("id", "name", "type", "cost", "star_cost", "description", "rarity", "is_upgraded")}
                         for c in self.deck if c.type != "Status"]
        self.mem.deck_size = len(self.mem.deck)
        self.mem.max_energy = self.energy

    # ------------------------------------------------------------------ gains

    def gain_relic(self, rid: str) -> None:
        if rid in self.relics:
            return
        self.relics.append(rid)
        self.game.relic_pickup(self, rid)

    def add_card(self, c: Card) -> None:
        if self.has({"Attack": "MOLTEN_EGG", "Skill": "TOXIC_EGG", "Power": "FROZEN_EGG"}.get(c.type, "")):
            c.up = True
        self.deck.append(c)

    def remove_card(self, c: Card) -> None:
        if c in self.deck:
            self.deck.remove(c)
            self.log["removals"] += 1

    def lose_hp(self, n: int) -> None:
        self.hp = max(1, self.hp - n)

    # ------------------------------------------------------------------ the bot's choices

    def choose_from_deck(self, screen_type: str, prompt: str, pool: list[Card] | None = None) -> Card | None:
        pool = list(self.deck if pool is None else pool)
        if not pool:
            return None
        state = self.screen("card_select", card_select={"screen_type": screen_type, "prompt": prompt, "cards": [c.view(i) for i, c in enumerate(pool)],
                                                         "preview_showing": False, "can_confirm": False, "can_cancel": False})
        self.mem.select_key, self.mem.selected = "", set()
        for action, params in strategy.card_select(state, self.mem):
            if action == "select_card" and 0 <= params.get("index", -1) < len(pool):
                return pool[params["index"]]
        return None

    def upgrade_one(self) -> Card | None:
        fresh = [c for c in self.deck if not c.up and c.type not in ("Status", "Curse")]
        c = self.choose_from_deck("upgrade", "Choose a card to Upgrade.", fresh)
        if c is not None:
            c.up = True
            self.digest.append(f"U{c.key}")
        return c

    def remove_one(self) -> Card | None:
        pool = [c for c in self.deck if c.key not in ("ASCENDER_S_BANE", "CURSE_OF_THE_BELL", "NECRONOMICURSE")]
        c = self.choose_from_deck("remove", "Choose a card to Remove.", pool)
        if c is not None:
            self.remove_card(c)
            self.digest.append(f"R{c.key}")
        return c

    def card_reward(self, offered: list[Card], can_skip: bool = True) -> Card | None:
        if not offered:
            return None
        state = self.screen("card_reward", card_reward={"cards": [c.view(i) for i, c in enumerate(offered)], "can_skip": can_skip})
        for action, params in strategy.card_reward(state, self.mem):
            if action == "skip_card_reward":
                self.log["skips"] += 1
                self.digest.append("S")
                return None
            if action == "select_card_reward":
                c = offered[params["card_index"]]
                self.add_card(c)
                self.mem.deck.append({k: v for k, v in c.view().items() if k not in ("index",)})
                self.mem.deck_size += 1
                self.mem.picks.append([self.floor, c.name])
                self.log["picks"].append([self.floor, c.key])
                self.digest.append(f"C{c.key}")
                return c
        return None

    # ------------------------------------------------------------------ the run

    def play(self) -> dict:
        t0 = time.perf_counter()
        learn.KNOB.clear()
        learn.KNOB.update({**learn.DEFAULTS, **self.knobs})
        self.mem.reset_run()
        self.mem.card_stats = None
        self.game.run_start(self)
        won = False
        for act in range(1, self.game.ACTS + 1):
            self.act = act
            self.game.act_start(self)
            if not self.play_act():
                break
            if act == self.game.ACTS:
                won = True
            else:
                self.hp += int((self.max_hp - self.hp) * (1.0 if self.asc < 5 else 0.75))
        return {
            "game": self.game.KEY, "character": self.character, "seed": self.seed, "ascension": self.asc, "knobs": self.knobs,
            "won": won, "act": self.act, "floor": self.floor, "hp": self.hp, "max_hp": self.max_hp, "gold": self.gold,
            "deck": sorted(c.name for c in self.deck), "relics": self.relics, "secs": round(time.perf_counter() - t0, 2), **self.log,
        }

    def play_act(self) -> bool:
        nodes = self.game.make_map(self)
        at = None
        while True:
            options = self.next_options(nodes, at)
            if not options:
                return True
            pick = self.choose_node(nodes, options)
            self.floor += 1
            at = (pick["col"], pick["row"])
            kind = pick["type"]
            self.digest.append(f"M{kind[:2]}{at[0]}")
            if not self.room(kind):
                return False
            if kind == "Boss":
                self.game.boss_won(self)
                return True

    def next_options(self, nodes: dict, at) -> list[dict]:
        if at is None:
            keys = sorted(k for k in nodes if k[1] == 1)
        else:
            keys = [tuple(k) for k in nodes[at]["children"]]
        out = []
        for i, k in enumerate(keys):
            n = nodes[k]
            kids = [nodes[tuple(c)] for c in n["children"] if tuple(c) in nodes]
            out.append({"index": i, "col": k[0], "row": k[1], "type": n["type"], "leads_to": [{"type": c["type"], "row": c["row"]} for c in kids]})
        return out

    def choose_node(self, nodes: dict, options: list[dict]) -> dict:
        state = self.screen("map", map={"next_options": options, "nodes": list(nodes.values())})
        for action, params in strategy.map_screen(state, self.mem):
            if action == "choose_map_node":
                return options[params["index"]]
        return options[0]

    def room(self, kind: str) -> bool:
        if kind == "Unknown":
            kind = self.unknown_room()
        if kind in ("Monster", "Elite", "Boss"):
            return self.fight(kind.lower())
        {"RestSite": self.rest_site, "Shop": self.shop, "Treasure": self.treasure, "Event": self.event}.get(kind, lambda: None)()
        return kind != "Event" or self.hp > 0

    def unknown_room(self) -> str:
        r, odds, base, pick = self.rng.random(), self.unknown_odds, self.game.UNKNOWN_ODDS, "Event"
        for kind in ("Monster", "Shop", "Treasure"):
            if r < odds[kind]:
                pick = kind
                break
            r -= odds[kind]
        for k in odds:  # (the odds of the room met go back to the start, the others grow)
            odds[k] = base[k] if k == pick else odds[k] + base[k]
        return pick

    # ------------------------------------------------------------------ fights

    def fight(self, kind: str) -> bool:
        foes = self.game.encounter(self, kind)
        self.note_deck()
        self.mem.boss_next = False
        hp0, met = self.hp, sorted({x.key for x in foes})  # (the encounter as it starts: splits and summons join the list)
        f = Fight(self, foes, kind)
        won = f.run_fight()
        self.digest.append("F" + "".join(f.digest))
        self.log["fights"].append({"act": self.act, "floor": self.floor, "kind": kind, "foes": met, "hp0": hp0,
                                   "hp1": max(0, self.hp), "max_hp": self.max_hp, "rounds": f.round, "won": won})
        self.log["elites"] += kind == "elite"
        if won:
            self.game.fight_won(self, f)
            if not f.escaped:
                self.rewards(kind, f)
        return won

    def rewards(self, kind: str, f: Fight) -> None:
        g = self.game
        items = []
        gold = g.fight_gold(self, kind)
        if gold:
            items.append({"type": "gold", "gold_amount": gold})
        if kind != "boss" and self.rng.random() < self.potion_chance or self.has("WHITE_BEAST_STATUE") and kind != "boss":
            self.potion_chance = max(0.0, self.potion_chance - 0.1)
            p = g.random_potion(self)
            items.append({"type": "potion", "potion_id": p.key, "potion_name": p.name, "description": p.text, "_potion": p})
        elif kind != "boss":
            self.potion_chance = min(1.0, self.potion_chance + 0.1)
        if kind == "elite":
            items.append({"type": "relic", "_relic": g.random_relic(self)})
        items.append({"type": "card"})
        if self.has("PRAYER_WHEEL") and kind == "monster":
            items.append({"type": "card"})
        for n, it in enumerate(items):
            it["index"] = n
        taken: set[int] = set()
        for _ in range(12):
            left = [i for i in items if i["index"] not in taken]
            state = self.screen("rewards", rewards={"items": [{k: v for k, v in i.items() if not k.startswith("_")} for i in left], "can_proceed": True})
            for action, params in strategy.rewards(state, self.mem):
                if action == "proceed":
                    return
                if action == "discard_potion":
                    self.drop_potion(int(params["slot"]))
                    break
                if action == "claim_reward":
                    it = items[params["index"]]
                    taken.add(it["index"])
                    if it["type"] == "gold":
                        self.gold += it["gold_amount"]
                    elif it["type"] == "potion":
                        if not self.add_potion(it["_potion"]):
                            continue
                    elif it["type"] == "relic":
                        self.gain_relic(it["_relic"])
                    elif it["type"] == "card":
                        self.card_reward(g.card_choices(self, kind))
                    break
            else:
                return

    # ------------------------------------------------------------------ other rooms

    def rest_site(self) -> None:
        opts = self.game.rest_options(self)
        if not opts:
            return
        state = self.screen("rest_site", rest_site={"options": opts, "can_proceed": False})
        for action, params in strategy.rest_site(state, self.mem):
            if action != "choose_rest_option":
                continue
            o = next((o for o in opts if o["index"] == params["index"]), None)
            if o is None:
                continue
            what = o["id"]
            if what == "rest":
                self.hp = min(self.max_hp, self.hp + int(self.max_hp * 0.3) + (15 if self.has("REGAL_PILLOW") else 0))
                self.log["rests"] += 1
                if self.has("DREAM_CATCHER"):
                    self.card_reward(self.game.card_choices(self, "monster"))
            elif what == "smith":
                if self.upgrade_one() is None:
                    continue
                self.log["smiths"] += 1
            elif what == "lift":
                self.counters["GIRYA"] = self.counters.get("GIRYA", 0) + 1
            elif what == "toke":
                self.remove_one()
            elif what == "dig":
                self.gain_relic(self.game.random_relic(self))
            self.digest.append(f"r{what}")
            return

    def treasure(self) -> None:
        self.gain_relic(self.game.random_relic(self, chest=True))
        if self.rng.random() < 0.5:
            self.gold += self.rng.randint(20, 60)
        if self.has("CURSED_KEY"):
            self.deck.append(self.game.random_curse(self))

    def event(self) -> None:
        self.log["events"] += 1
        self.game.event(self)

    def shop(self) -> None:
        self.log["shops"] += 1
        stock = self.game.shop_stock(self)
        if self.has("MEMBERSHIP_CARD"):
            for it in stock:
                it["price"] = int(it["price"] * 0.5)
        for _ in range(10):
            shown = [{k: v for k, v in it.items() if not k.startswith("_")} | {"can_afford": it["price"] <= self.gold, "is_stocked": True}
                     for it in stock]
            state = self.screen("shop", shop={"items": shown, "can_proceed": True})
            for action, params in strategy.shop(state, self.mem):
                if action == "proceed":
                    return
                if action != "shop_purchase":
                    continue
                it = next((x for x in stock if x["index"] == params["index"]), None)
                if it is None or it["price"] > self.gold:
                    continue
                if it["category"] == "potion" and not self.add_potion(it["_potion"]):
                    continue
                if it["category"] == "card_removal":
                    if self.remove_one() is None:
                        continue
                    self.counters["removals"] = self.counters.get("removals", 0) + 1
                elif it["category"] == "card":
                    c = it["_card"]
                    self.add_card(c)
                    self.mem.deck.append({k: v for k, v in c.view().items()})
                    self.mem.deck_size += 1
                elif it["category"] == "relic":
                    self.gain_relic(it["_relic"])
                self.gold -= it["price"]
                if self.has("MAW_BANK"):
                    self.relics.remove("MAW_BANK")
                stock.remove(it)
                self.log["bought"].append(it["category"])
                self.digest.append(f"B{it['category'][:2]}")
                break
            else:
                return
