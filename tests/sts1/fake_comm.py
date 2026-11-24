"""A tiny stand-in for the first Slay the Spire as CommunicationMod shows it.

Like tests/fake_game.py it is not the game: a main menu, a short map (fight, event, shop, rest, chest,
boss), fights against one monster, rewards and game over, in CommunicationMod's message format and
taking its commands ("start", "play", "end", "choose", "proceed", "skip", "confirm", "leave", "potion").
"""

from __future__ import annotations

import random


def card(cid, name, ctype, cost, target=False, rarity="BASIC", upgrades=0):
    return {"id": cid, "name": name + ("+" if upgrades else ""), "type": ctype, "cost": cost, "has_target": target,
            "rarity": rarity, "upgrades": upgrades, "uuid": f"{cid}-{random.random()}", "exhausts": False}


STRIKE = ("Strike_R", "Strike", "ATTACK", 1, True)
DEFEND = ("Defend_R", "Defend", "SKILL", 1, False)
BASH = ("Bash", "Bash", "ATTACK", 2, True)
DAMAGE = {"Strike_R": 6, "Bash": 8, "Pommel Strike": 9, "Cleave": 8}
# The act: (x, y, symbol). One road, so every choice is the only one.
ROAD = [(0, 0, "M"), (0, 1, "?"), (0, 2, "$"), (0, 3, "R"), (0, 4, "T"), (0, 5, "E")]


class FakeComm:
    def __init__(self, seed: int = 0, monster_hp: int = 20, attack: int = 7):
        self.rng = random.Random(seed)
        self.monster_hp, self.attack = monster_hp, attack
        self.in_game = False
        self.sent: list[str] = []
        self.runs = 0

    # ------------------------------------------------------------ the run

    def _start(self, cls, asc):
        self.in_game, self.cls, self.asc = True, cls, asc
        self.runs += 1
        self.hp = self.max_hp = 80
        self.gold, self.y, self.screen = 99, -1, "MAP"
        self.deck = [STRIKE] * 5 + [DEFEND] * 4 + [BASH]
        self.potions = [{"id": "Fire Potion", "name": "Fire Potion", "can_use": False, "can_discard": True, "requires_target": True},
                        {"id": "Potion Slot", "name": "Potion Slot"}, {"id": "Potion Slot", "name": "Potion Slot"}]
        self.room = "NeowRoom"
        self.victory, self.rested, self.chest_open, self.picked, self.grid = False, False, False, [], ""

    def _fight(self, room):
        self.room, self.screen, self.phase = room, "NONE", "COMBAT"
        self.monster = {"hp": self.monster_hp * (3 if room == "MonsterRoomBoss" else 1), "vuln": 0}
        self.max_monster = self.monster["hp"]
        self.draw = list(self.deck)
        self.rng.shuffle(self.draw)
        self.discard, self.turn = [], 0
        for p in self.potions:
            p["can_use"] = p["id"] != "Potion Slot"
        self._turn()

    def _turn(self):
        self.turn += 1
        self.energy, self.block, self.hand = 3, 0, []
        for _ in range(5):
            if not self.draw:
                self.draw, self.discard = self.discard, []
                self.rng.shuffle(self.draw)
            if self.draw:
                self.hand.append(self.draw.pop())

    def _won_fight(self):
        for p in self.potions:
            p["can_use"] = False
        if self.room == "MonsterRoomBoss":
            self.screen, self.victory = "GAME_OVER", True
            return
        self.screen, self.phase = "COMBAT_REWARD", "COMPLETE"
        self.rewards = [{"reward_type": "GOLD", "gold": 15}, {"reward_type": "CARD"}]

    # ------------------------------------------------------------ messages

    def commands(self):
        if not self.in_game:
            return ["start", "state"]
        s = self.screen
        if s == "NONE":
            return ["play", "end", "potion", "wait", "state"] if self.monster["hp"] > 0 else ["state"]
        return {
            "MAP": ["choose", "state"], "COMBAT_REWARD": ["choose", "proceed", "state"], "CARD_REWARD": ["choose", "skip", "state"],
            "EVENT": ["choose", "state"], "SHOP_ROOM": ["choose", "proceed", "state"], "SHOP_SCREEN": ["choose", "leave", "state"],
            "REST": ["choose", "proceed", "state"] if self.rested else ["choose", "state"], "GRID": ["choose", "confirm", "state"],
            "CHEST": ["choose", "proceed", "state"], "GAME_OVER": ["proceed", "state"],
        }[s]

    def _card(self, c, upgrades=0):
        return card(c[0], c[1], c[2], c[3], c[4], upgrades=upgrades)

    def message(self) -> dict:
        msg = {"available_commands": self.commands(), "ready_for_command": True, "in_game": self.in_game}
        if not self.in_game:
            return msg
        g = {
            "screen_type": self.screen, "room_type": self.room, "room_phase": getattr(self, "phase", "COMPLETE"), "current_hp": self.hp,
            "max_hp": self.max_hp, "gold": self.gold, "floor": self.y + 1, "act": 1, "class": self.cls, "ascension_level": self.asc,
            "seed": 42 + self.runs, "relics": [{"id": "Burning Blood", "name": "Burning Blood", "counter": -1}], "potions": self.potions,
            "deck": [self._card(c) for c in self.deck],
            "map": [{"x": x, "y": y, "symbol": s, "children": [{"x": 0, "y": y + 1}] if y < 5 else [{"x": 3, "y": 7}], "parents": []} for x, y, s in ROAD],
        }
        s = self.screen
        if s == "NONE":
            g["combat_state"] = {
                "turn": self.turn, "hand": [{**self._card(c), "is_playable": c[3] <= self.energy} for c in self.hand],
                "draw_pile": [self._card(c) for c in self.draw], "discard_pile": [self._card(c) for c in self.discard], "exhaust_pile": [],
                "player": {"current_hp": self.hp, "max_hp": self.max_hp, "block": self.block, "energy": self.energy, "powers": [], "orbs": []},
                "monsters": [{"id": "JawWorm", "name": "Jaw Worm", "current_hp": self.monster["hp"], "max_hp": self.max_monster, "block": 0,
                              # (the real game's first message of a fight comes before the intents are set: "DEBUG")
                              "intent": "DEBUG" if getattr(self, "intent_pending", 0) > 0 else "ATTACK",
                              "move_adjusted_damage": -1 if getattr(self, "intent_pending", 0) > 0 else self.attack, "move_hits": 1,
                              "half_dead": False, "is_gone": False,
                              "powers": [{"id": "Vulnerable", "name": "Vulnerable", "amount": self.monster["vuln"]}] if self.monster["vuln"] else []}],
            }
        elif s == "MAP":
            nxt = [] if self.y >= 5 else [{"x": 0, "y": self.y + 1, "symbol": ROAD[self.y + 1][2]}]
            g["screen_state"] = {"next_nodes": nxt, "boss_available": self.y >= 5, "current_node": {"x": 0, "y": self.y}}
            g["choice_list"] = ["boss"] if self.y >= 5 else ["x=0"]
        elif s == "COMBAT_REWARD":
            g["screen_state"] = {"rewards": self.rewards}
        elif s == "CARD_REWARD":
            g["screen_state"] = {"cards": [self._card(("Pommel Strike", "Pommel Strike", "ATTACK", 1, True), 0) | {"rarity": "COMMON"},
                                           self._card(("Clash", "Clash", "ATTACK", 0, True)) | {"rarity": "COMMON"}], "skip_available": True}
        elif s == "EVENT":
            g["screen_state"] = {"event_name": "Golden Idol", "event_id": "Golden Idol", "body_text": "An idol.",
                                 "options": [{"text": "[Take] #gObtain #gGolden #gIdol. #rTrigger #ra #rtrap.", "label": "Take", "disabled": False, "choice_index": 0},
                                             {"text": "[Leave]", "label": "Leave", "disabled": False, "choice_index": 1}]}
        elif s == "SHOP_SCREEN":
            g["screen_state"] = {"cards": [self._card(("Cleave", "Cleave", "ATTACK", 1, False)) | {"price": 50, "rarity": "COMMON"}],
                                 "relics": [{"id": "Anchor", "name": "Anchor", "price": 150}], "potions": [],
                                 "purge_available": True, "purge_cost": 75}
        elif s == "REST":
            g["screen_state"] = {"rest_options": [] if self.rested else ["rest", "smith"], "has_rested": self.rested}
        elif s == "GRID":
            g["screen_state"] = {"cards": [self._card(c) for c in self.deck], "selected_cards": [self._card(c) for c in self.picked],
                                 "num_cards": 1, "any_number": False, "confirm_up": bool(self.picked), "for_upgrade": self.grid == "upgrade",
                                 "for_purge": self.grid == "purge", "for_transform": False}
        elif s == "CHEST":
            g["screen_state"] = {"chest_type": "SmallChest", "chest_open": self.chest_open}
        elif s == "GAME_OVER":
            g["screen_state"] = {"victory": self.victory, "score": 100}
        msg["game_state"] = g
        return msg

    # ------------------------------------------------------------ commands

    def command(self, line: str) -> dict:
        """Carry out one command and answer like CommunicationMod: the new state, or an error."""
        self.sent.append(line)
        words = line.split()
        verb = words[0] if words else ""
        if verb not in self.commands():
            return {"error": f"Invalid command: {line}", "ready_for_command": True}
        try:
            ok = self._do(verb, words[1:])
        except (IndexError, ValueError, KeyError) as e:
            return {"error": f"{type(e).__name__}: {e}", "ready_for_command": True}
        if ok is False:
            return {"error": f"Invalid command: {line}", "ready_for_command": True}
        return self.message()

    def _do(self, verb, args):
        s = self.screen if self.in_game else "MENU"
        if verb == "state":
            return True
        if verb == "wait":
            self.intent_pending = max(0, getattr(self, "intent_pending", 0) - 1)
            return True
        if verb == "start":
            self._start(args[0].upper(), int(args[1]) if len(args) > 1 else 0)
            return True
        if s == "NONE" and verb == "play":
            c = self.hand[int(args[0]) - 1]
            if c[3] > self.energy or (c[4] and len(args) < 2):
                return False
            self.hand.remove(c)
            self.discard.append(c)
            self.energy -= c[3]
            if c[2] == "ATTACK":
                self.monster["hp"] -= int(DAMAGE.get(c[0], 6) * (1.5 if self.monster["vuln"] else 1))
                if c[0] == "Bash":
                    self.monster["vuln"] += 2
            else:
                self.block += 5
            if self.monster["hp"] <= 0:
                self._won_fight()
            return True
        if s == "NONE" and verb == "end":
            self.hp -= max(0, self.attack - self.block)
            self.monster["vuln"] = max(0, self.monster["vuln"] - 1)
            self.discard += self.hand
            if self.hp <= 0:
                self.hp, self.screen = 0, "GAME_OVER"
            else:
                self._turn()
            return True
        if s == "NONE" and verb == "potion":
            slot = int(args[1])
            if args[0] != "use" or self.potions[slot]["id"] == "Potion Slot" or len(args) < 3:
                return False
            self.potions[slot] = {"id": "Potion Slot", "name": "Potion Slot"}
            self.monster["hp"] -= 20
            if self.monster["hp"] <= 0:
                self._won_fight()
            return True
        if s == "MAP" and verb == "choose":
            self.y += 1
            kind = "B" if self.y > 5 else ROAD[self.y][2]
            if kind in "MEB":
                self._fight({"M": "MonsterRoom", "E": "MonsterRoomElite", "B": "MonsterRoomBoss"}[kind])
            else:
                self.phase = "EVENT"
                self.room, self.screen = {"?": ("EventRoom", "EVENT"), "$": ("ShopRoom", "SHOP_ROOM"), "R": ("RestRoom", "REST"), "T": ("TreasureRoom", "CHEST")}[kind]
                self.rested, self.chest_open, self.picked = False, False, []
            return True
        if s == "COMBAT_REWARD" and verb == "choose":
            item = self.rewards[int(args[0])]
            if item["reward_type"] == "CARD":
                self.screen = "CARD_REWARD"
            else:
                self.gold += item.get("gold", 0)
                self.rewards.remove(item)
            return True
        if s in ("COMBAT_REWARD", "CHEST") and verb == "proceed":
            self.screen = "MAP"
            return True
        if s == "CARD_REWARD" and verb == "choose":
            self.deck.append(("Pommel Strike", "Pommel Strike", "ATTACK", 1, True) if args[0] == "0" else ("Clash", "Clash", "ATTACK", 0, True))
            self.rewards = [r for r in self.rewards if r["reward_type"] != "CARD"]
            self.screen = "COMBAT_REWARD"
            return True
        if s == "CARD_REWARD" and verb == "skip":
            self.screen = "COMBAT_REWARD"
            return True
        if s == "EVENT" and verb == "choose":
            self.screen = "MAP"  # (both options end the event)
            return True
        if s == "SHOP_ROOM" and verb == "choose" and args == ["shop"]:
            self.screen = "SHOP_SCREEN"
            return True
        if s == "SHOP_ROOM" and verb == "proceed":
            self.screen = "MAP"
            return True
        if s == "SHOP_SCREEN" and verb == "choose":
            name = " ".join(args)
            if name == "purge" and self.gold >= 75:
                self.gold -= 75
                self.screen, self.grid, self.picked = "GRID", "purge", []
                return True
            return False
        if s == "SHOP_SCREEN" and verb == "leave":
            self.screen = "SHOP_ROOM"
            return True
        if s == "REST" and verb == "choose" and not self.rested:
            if int(args[0]) == 0:
                self.hp, self.rested = min(self.max_hp, self.hp + 24), True
            else:
                self.screen, self.grid, self.picked = "GRID", "upgrade", []
            return True
        if s == "REST" and verb == "proceed":
            self.screen = "MAP"
            return True
        if s == "GRID" and verb == "choose":
            self.picked = [self.deck[int(args[0])]]
            return True
        if s == "GRID" and verb == "confirm" and self.picked:
            if self.grid == "purge":
                self.deck.remove(self.picked[0])
                self.screen = "SHOP_SCREEN"
            else:
                self.screen, self.rested = "REST", True
            return True
        if s == "CHEST" and verb == "choose":
            self.chest_open = True
            self.screen, self.rewards = "COMBAT_REWARD", [{"reward_type": "RELIC", "relic": {"id": "Anchor", "name": "Anchor", "counter": -1}}]
            return True
        if s == "GAME_OVER" and verb == "proceed":
            self.in_game = False
            return True
        return False
