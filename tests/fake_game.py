"""A tiny stand-in for the game that speaks the bridge mod's state/action shapes. It is not Slay the Spire: just enough
screens (menu, map, combat, rewards, card reward, rest site, card select, game over) to drive the bot loop for whole runs."""

from __future__ import annotations

import random


def card(index, cid, name, ctype, cost, desc, target="None", rarity="Common", **extra):
    return {"index": index, "id": cid, "name": name, "type": ctype, "cost": str(cost), "star_cost": None, "description": desc,
            "target_type": target, "can_play": True, "is_upgraded": False, "rarity": rarity, **extra}


STRIKE = ("STRIKE_R", "Strike", "Attack", 1, "Deal 6 damage.", "AnyEnemy")
DEFEND = ("DEFEND_R", "Defend", "Skill", 1, "Gain 5 Block.", "Self")
BASH = ("BASH", "Bash", "Attack", 2, "Deal 8 damage. Apply 2 Vulnerable.", "AnyEnemy")


def _options(*names, off=()):
    return [{"name": n, "enabled": n not in off} for n in names]


class FakeGame:
    def __init__(self, seed: int = 0, floors: int = 4, enemy_hp: int = 30, enemy_attack: int = 9):
        self.rng = random.Random(seed)
        self.floors, self.enemy_hp, self.enemy_attack = floors, enemy_hp, enemy_attack
        self.actions: list[tuple[str, dict]] = []
        self.runs_started, self.screen, self.character, self.has_save = 0, "main", None, False

    def _new_run(self):
        self.runs_started += 1
        self.floor, self.hp, self.max_hp, self.gold = 0, 80, 80, 99
        self.deck = [STRIKE] * 5 + [DEFEND] * 4 + [BASH]
        self.screen, self.has_save = "map", True

    def _start_combat(self):
        self.screen, self.round = "combat", 0
        self.enemy = {"hp": self.enemy_hp, "block": 0, "vuln": 0}
        self.draw = list(self.deck)
        self.rng.shuffle(self.draw)
        self.discard = []
        self._start_turn()

    def _start_turn(self):
        self.round += 1
        self.energy, self.block, self.hand = 3, 0, []
        for _ in range(5):
            if not self.draw:
                self.draw, self.discard = self.discard, []
                self.rng.shuffle(self.draw)
            if self.draw:
                self.hand.append(self.draw.pop())

    def _player(self, combat=False):
        p = {"character": "The Ironclad", "hp": self.hp, "max_hp": self.max_hp, "block": getattr(self, "block", 0), "gold": self.gold,
             "status": [], "relics": [{"id": "BURNING_BLOOD", "name": "Burning Blood"}], "potions": [], "max_potion_slots": 3}
        if combat:
            p.update(energy=self.energy, max_energy=3, hand=[{**card(i, *c), "can_play": c[3] <= self.energy} for i, c in enumerate(self.hand)],
                     draw_pile_count=len(self.draw), discard_pile_count=len(self.discard), exhaust_pile_count=0)
        return p

    def _wrap(self, kind, **body):
        return {"state_type": kind, "run": {"act": 1, "floor": self.floor, "ascension": 0}, "player": self._player(kind in ("monster", "boss")), **body}

    def state(self) -> dict:
        s = self.screen
        if s == "main":
            opts = (["continue", "abandon_run"] if self.has_save else []) + ["singleplayer", "settings", "quit"]
            return {"state_type": "menu", "menu_screen": "main", "options": opts}
        if s == "singleplayer":
            return {"state_type": "menu", "menu_screen": s, "options": _options("standard", "back")}
        if s == "popup":
            return {"state_type": "menu", "menu_screen": s, "options": _options("yes", "no")}
        if s == "character_select":
            chars = [{"id": "IRONCLAD", "name": "The Ironclad", "locked": False}, {"id": "SILENT", "name": "The Silent", "locked": True}]
            opts = _options("IRONCLAD", "SILENT", "confirm", "back", off=("SILENT",) if self.character is not None else ("SILENT", "confirm"))
            return {"state_type": "menu", "menu_screen": s, "characters": chars, "options": opts}
        if s == "map":
            kinds = ["Monster", "RestSite"] if self.floor % 2 else ["Monster", "Monster"]
            nxt = [{"index": i, "col": i, "row": self.floor + 1, "type": k, "leads_to": []} for i, k in enumerate(kinds)]
            return self._wrap("map", map={"next_options": nxt, "nodes": []})
        if s == "combat":
            e = self.enemy
            foe = {"entity_id": "JAW_WORM_0", "name": "Jaw Worm", "hp": e["hp"], "max_hp": self.enemy_hp, "block": e["block"],
                   "status": [{"id": "VULNERABLE_POWER", "amount": e["vuln"]}] if e["vuln"] else [], "intents": [{"type": "Attack", "label": str(self.enemy_attack)}]}
            return self._wrap("monster", battle={"round": self.round, "turn": "player", "is_play_phase": True, "enemies": [foe]})
        if s == "rewards":
            return self._wrap("rewards", rewards={"items": self.reward_items, "can_proceed": True})
        if s == "card_reward":
            offer = [card(0, "UPPERCUT", "Uppercut", "Attack", 2, "Deal 13 damage. Apply 1 Weak. Apply 1 Vulnerable.", "AnyEnemy", "Uncommon"),
                     card(1, "STRIKE_R", "Strike", "Attack", 1, "Deal 6 damage.", "AnyEnemy", "Basic")]
            return self._wrap("card_reward", card_reward={"cards": offer, "can_skip": True})
        if s == "rest":
            opts = [{"index": 0, "id": "rest", "name": "Rest", "is_enabled": True}, {"index": 1, "id": "smith", "name": "Smith", "is_enabled": True}]
            return self._wrap("rest_site", rest_site={"options": opts, "can_proceed": self.rested})
        if s == "upgrade":
            picked = self.upgrade_pick is not None
            return self._wrap("card_select", card_select={"screen_type": "upgrade", "prompt": "Choose a card to Upgrade.", "cards": [card(i, *c) for i, c in enumerate(self.deck)],
                                                          "preview_showing": picked, "can_confirm": picked, "can_cancel": True})
        if s == "game_over":
            return self._wrap("game_over", game_over={"options": ["main_menu"]})
        raise AssertionError(s)

    def act(self, action: str, **p) -> dict:
        self.actions.append((action, p))
        try:
            msg = self._act(action, p)
        except (KeyError, IndexError, ValueError) as e:
            return {"status": "error", "error": f"{type(e).__name__}: {e}"}
        if msg is None:
            return {"status": "error", "error": f"{action} not valid on {self.screen}"}
        return {"status": "ok", "message": msg}

    def _act(self, action, p):
        s = self.screen
        if action == "menu_select":
            opt = p["option"]
            if s == "main" and opt == "singleplayer":
                self.screen = "singleplayer"
            elif s == "main" and opt == "continue" and self.has_save:
                self.screen = self.saved_screen
            elif s == "main" and opt == "abandon_run" and self.has_save:
                self.screen = "popup"
            elif s == "popup" and opt == "yes":
                self.has_save, self.screen = False, "main"
            elif s == "singleplayer" and opt == "standard":
                self.screen, self.character = "character_select", None
            elif s == "character_select" and opt == "IRONCLAD":
                self.character = opt
            elif s == "character_select" and opt == "confirm" and self.character:
                self._new_run()
            elif s == "game_over" and opt == "main_menu":
                self.screen, self.has_save = "main", False
            else:
                return None
            return opt
        if s == "map" and action == "choose_map_node":
            kind = self.state()["map"]["next_options"][p["index"]]["type"]
            self.floor += 1
            if kind == "RestSite":
                self.screen, self.rested = "rest", False
            else:
                self._start_combat()
            return kind
        if s == "combat" and action == "play_card":
            c = self.hand[p["card_index"]]
            if c[3] > self.energy:
                raise ValueError("NotEnoughEnergy")
            if c[5] == "AnyEnemy" and p.get("target") != "JAW_WORM_0":
                raise ValueError("Card requires a target")
            self.hand.pop(p["card_index"])
            self.discard.append(c)
            self.energy -= c[3]
            if c[2] == "Attack":
                dmg = 13 if c[0] == "UPPERCUT" else int(c[4].split()[1])
                self.enemy["hp"] -= int(dmg * 1.5) if self.enemy["vuln"] else dmg
                if c[0] in ("BASH", "UPPERCUT"):
                    self.enemy["vuln"] += 2
            else:
                self.block += 5
            if self.enemy["hp"] <= 0:
                self.reward_items = [{"index": 0, "type": "gold", "gold_amount": 20}, {"index": 1, "type": "card"}]
                self.screen = "game_over" if self.floor >= self.floors else "rewards"
            return c[1]
        if s == "combat" and action == "end_turn":
            self.hp -= max(0, self.enemy_attack - self.block)
            self.enemy["vuln"] = max(0, self.enemy["vuln"] - 1)
            self.discard += self.hand
            if self.hp <= 0:
                self.hp, self.screen = 0, "game_over"
            else:
                self._start_turn()
            return "end"
        if s == "rewards" and action == "claim_reward":
            item = next(i for i in self.reward_items if i["index"] == p["index"])
            if item["type"] == "gold":
                self.gold += item["gold_amount"]
                self.reward_items.remove(item)
            else:
                self.screen = "card_reward"
            return item["type"]
        if action == "proceed" and (s == "rewards" or s == "rest" and self.rested):
            self.screen = "map"
            return "map"
        if s == "card_reward" and action == "select_card_reward":
            picked = self.state()["card_reward"]["cards"][p["card_index"]]
            self.deck.append((picked["id"], picked["name"], picked["type"], int(picked["cost"]), picked["description"], picked["target_type"]))
            self.reward_items = [i for i in self.reward_items if i["type"] != "card"]
            self.screen = "rewards"
            return picked["name"]
        if s == "card_reward" and action == "skip_card_reward":
            self.screen = "rewards"  # the card reward stays on the list, like in the game
            return "skip"
        if s == "rest" and action == "choose_rest_option" and not self.rested:
            if p["index"] == 0:
                self.hp, self.rested = min(self.max_hp, self.hp + 24), True
            else:
                self.screen, self.upgrade_pick = "upgrade", None
            return "rest"
        if s == "upgrade" and action == "select_card":
            self.upgrade_pick = p["index"]
            return "picked"
        if s == "upgrade" and action == "confirm_selection" and self.upgrade_pick is not None:
            self.screen, self.rested = "rest", True
            return "upgraded"
        return None

    def ping(self) -> bool:  # (the Client interface the bot uses)
        return True
