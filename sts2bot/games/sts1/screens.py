"""The first game's state, as CommunicationMod sends it, turned into the state the bot reads.

The bot (bot.py, strategy.py, the planner in fight/) was written against the STS2_MCP mod's state:
`state_type` names the screen ("monster", "map", "rewards", "card_reward", "rest_site", "shop",
"event", "card_select", "hand_select", "relic_select", "treasure", "game_over", "menu"), with the
player, the run and the screen's own block next to it. CommunicationMod sends the first game's
state in its own words (screen_type MAP, COMBAT_REWARD, GRID ...). `Screens.translate` gives the
same state in the bot's words, and remembers what the bot's actions will need to be sent back as
commands (which choice an index means, which monster an enemy id is): see client.py.

The menu is the bot's own: CommunicationMod only knows "start CLASS ASCENSION", so the main menu
and the character select screen are made up here, and the run starts when the bot confirms.
"""

from __future__ import annotations

import re

from ...cards import key_of
from . import text

CHARACTERS = ("IRONCLAD", "THE_SILENT", "DEFECT", "WATCHER")
CHARACTER_NAMES = {"IRONCLAD": "The Ironclad", "THE_SILENT": "The Silent", "DEFECT": "The Defect", "WATCHER": "The Watcher"}

ROOM_KIND = {"MonsterRoom": "monster", "MonsterRoomElite": "elite", "MonsterRoomBoss": "boss"}
MAP_SYMBOL = {"M": "Monster", "E": "Elite", "R": "RestSite", "$": "Shop", "?": "Unknown", "T": "Treasure", "B": "Boss"}
CARD_TYPE = {"ATTACK": "Attack", "SKILL": "Skill", "POWER": "Power", "STATUS": "Status", "CURSE": "Curse"}
RARITY = {"BASIC": "Basic", "COMMON": "Common", "UNCOMMON": "Uncommon", "RARE": "Rare", "SPECIAL": "Event", "CURSE": "Curse"}
# The first game's power ids where the planner knows the same thing by another name.
POWER_ID = {
    "WEAKENED": "WEAK", "PLATED_ARMOR": "PLATING", "INTANGIBLEPLAYER": "INTANGIBLE", "REGENERATION": "REGEN",
    "REGENERATE": "REGEN", "CONSTRICTED": "CONSTRICT", "DRAW_CARD": "DRAW_CARDS_NEXT_TURN", "ENERGIZED": "ENERGY_NEXT_TURN",
    "ENERGIZEDBLUE": "ENERGY_NEXT_TURN", "LOCKON": "LOCK_ON", "SURROUNDED": "SURROUNDED", "MINION": "MINION",
}
INTENT_TYPE = {"BUFF": "Buff", "DEBUFF": "Debuff", "STRONG_DEBUFF": "Debuff", "DEFEND": "Defend", "ESCAPE": "Escape",
               "SLEEP": "Sleep", "STUN": "Stun", "MAGIC": "Buff", "UNKNOWN": "Unknown", "NONE": "Unknown", "DEBUG": "Unknown"}
# What a hand selection is for, by the action that asked for it (CommunicationMod does not send the game's prompt).
HAND_PROMPT = {
    "ExhaustAction": "Choose {n} card(s) to Exhaust.", "PurityAction": "Choose up to {n} card(s) to Exhaust.",
    "RecycleAction": "Choose a card to Exhaust.", "DiscardAction": "Choose {n} card(s) to Discard.",
    "GamblingChipAction": "Choose any number of cards to Discard.", "PutOnDeckAction": "Put a card on top of your draw pile.",
    "WarcryAction": "Put a card on top of your draw pile.", "SetupAction": "Put a card on top of your draw pile.",
    "ThinkingAheadAction": "Put a card on top of your draw pile.", "ForethoughtAction": "Put a card on top of your draw pile.",
    "ArmamentsAction": "Choose a card to Upgrade.", "DualWieldAction": "Choose a card to copy.",
    "RetainCardsAction": "Choose up to {n} card(s) to Retain.", "NightmareAction": "Choose a card to copy.",
}
_COLOR = re.compile(r"#[rgbyp]|~|@|\bNL\b")


def clean(s) -> str:
    """Game text without its colour codes and line-break markers."""
    return " ".join(_COLOR.sub("", str(s or "")).split())


def upper_id(raw) -> str:
    return key_of(raw)


class Screens:
    """Translates one message at a time, and keeps what the way back needs."""

    def __init__(self):
        self.raw: dict = {}  # the last message from the game
        self.menu_screen = "main"  # the bot's own menu, between runs
        self.character: str | None = None
        self.ascension: int | None = None
        self.max_ascension: dict[str, int] = {}  # character -> highest ascension unlocked (from the save files)
        self.locked: set[str] = set()  # characters the game would not start
        self.wins = 0  # runs won as seen by this session (plus what the save files said at the start)
        self._won_seen: tuple | None = None
        self.targets: dict[str, int] = {}  # enemy id -> monster index in the game's list
        self.shop_names: dict[int, str] = {}  # shop item index -> choice name
        self.max_energy = 3
        self._turn_seen: tuple = ()

    # ------------------------------------------------------------ helpers

    @property
    def commands(self) -> list[str]:
        return [str(c) for c in self.raw.get("available_commands") or []]

    @property
    def game(self) -> dict:
        return self.raw.get("game_state") or {}

    def _run(self) -> dict:
        g = self.game
        return {"act": g.get("act"), "floor": g.get("floor"), "ascension": g.get("ascension_level")}

    def card(self, c: dict, index: int | None = None) -> dict:
        upgraded = int(c.get("upgrades") or 0) > 0
        cost = c.get("cost")
        desc = text.card_text(c.get("id"), c.get("name"), upgraded)
        ctype = CARD_TYPE.get(str(c.get("type")), str(c.get("type") or "").title())
        target = "AnyEnemy" if c.get("has_target") else "AllEnemies" if re.search(r"\ball enemies\b", desc, re.I) else "Self"
        out = {
            "id": upper_id(c.get("id")),
            "name": str(c.get("name") or ""),
            "type": ctype,
            "cost": "X" if cost == -1 else str(max(0, int(cost))) if isinstance(cost, (int, float)) else str(cost or 0),
            "star_cost": None,
            "description": desc,
            "target_type": target,
            "can_play": bool(c.get("is_playable", True)),
            "is_upgraded": upgraded,
            "rarity": RARITY.get(str(c.get("rarity")), str(c.get("rarity") or "").title()),
        }
        if cost == -2:
            out["unplayable_reason"] = "Unplayable"
        if index is not None:
            out["index"] = index
        return out

    @staticmethod
    def powers(creature: dict) -> list[dict]:
        out = []
        for p in creature.get("powers") or []:
            pid = upper_id(p.get("id") or p.get("name"))
            out.append({"id": POWER_ID.get(pid, pid) + "_POWER", "name": p.get("name"), "amount": p.get("amount"), "description": ""})
        return out

    def potions(self) -> list[dict]:
        out = []
        for slot, p in enumerate(self.game.get("potions") or []):
            if str(p.get("id")) == "Potion Slot":
                continue
            key = text.potion_key(p.get("id"), p.get("name"))
            out.append({
                "slot": slot, "id": key, "name": p.get("name"), "description": text.potion_text(p.get("id"), p.get("name")),
                "target_type": "AnyEnemy" if p.get("requires_target") else "Self",
                "can_use_in_combat": bool(p.get("can_use")) and key not in text.potions.KEEP,
                "can_discard": bool(p.get("can_discard", True)),
            })
        return out

    def player(self, combat: bool) -> dict:
        g = self.game
        cls = str(g.get("class") or "")
        p = {
            "character": CHARACTER_NAMES.get(cls, cls.replace("_", " ").title()),
            "hp": g.get("current_hp"), "max_hp": g.get("max_hp"), "gold": g.get("gold"), "block": 0, "status": [],
            "relics": [{"id": upper_id(r.get("id")), "name": r.get("name"), "counter": r.get("counter"),
                        "description": text.relic_text(r.get("id"), r.get("name"))} for r in g.get("relics") or []],
            "potions": self.potions(), "max_potion_slots": len(g.get("potions") or []) or 3,
        }
        cs = g.get("combat_state") if combat else None
        if not cs:
            return p
        me = cs.get("player") or {}
        hand = [self.card(c, i) for i, c in enumerate(cs.get("hand") or [])]
        draw = [self.card(c) for c in cs.get("draw_pile") or []]
        orbs = [o for o in me.get("orbs") or [] if str(o.get("id")) not in ("Empty", "None")]
        turn = (g.get("floor"), cs.get("turn"))
        if turn != self._turn_seen:  # the energy a turn starts with is the most it has
            self._turn_seen = turn
            if int(cs.get("turn") or 0) == 1:
                self.max_energy = int(me.get("energy") or 3)
        p.update(
            hp=me.get("current_hp", p["hp"]), max_hp=me.get("max_hp", p["max_hp"]), block=me.get("block", 0),
            energy=me.get("energy", 0), max_energy=self.max_energy, status=self.powers(me), hand=hand,
            draw_pile=draw, draw_pile_count=len(cs.get("draw_pile") or []),
            discard_pile=[self.card(c) for c in cs.get("discard_pile") or []], discard_pile_count=len(cs.get("discard_pile") or []),
            exhaust_pile_count=len(cs.get("exhaust_pile") or []),
            orbs=[{"id": upper_id(o.get("id") or o.get("name")), "passive_val": o.get("passive_amount"), "evoke_val": o.get("evoke_amount")} for o in orbs],
            orb_slots=len(me.get("orbs") or []),
        )
        return p

    def _wrap(self, kind: str, **body) -> dict:
        return {"state_type": kind, "run": self._run(), "player": self.player(kind in ROOM_KIND.values()), **body}

    # ------------------------------------------------------------ screens

    def translate(self, msg: dict | None) -> dict:
        self.raw = msg or {}
        if not msg:
            return {"state_type": "loading"}
        if not msg.get("in_game"):
            return self.menu()
        self.menu_screen, self.character = "main", None
        g = self.game
        screen = str(g.get("screen_type") or "NONE")
        handler = getattr(self, "_" + screen.lower(), None)
        if handler is not None:
            return handler()
        if g.get("room_phase") == "COMBAT" and g.get("combat_state"):
            return self.combat()
        return self._wrap("overlay")

    def menu(self) -> dict:
        if self.menu_screen == "character_select":
            chars = [{"id": c, "name": CHARACTER_NAMES[c], "locked": c in self.locked} for c in CHARACTERS]
            top = self.max_ascension.get(self.character, 0) if self.character else None
            return {
                "state_type": "menu", "menu_screen": "character_select", "characters": chars,
                "options": [{"name": c["id"], "enabled": not c["locked"]} for c in chars]
                + [{"name": "confirm", "enabled": self.character is not None}, {"name": "back", "enabled": True}],
                "selected": self.character, "ascension": self.ascension if self.character else None, "max_ascension": top,
            }
        return {"state_type": "menu", "menu_screen": "main", "options": ["singleplayer"] if "start" in self.commands else []}

    def combat(self) -> dict:
        g = self.game
        cs = g.get("combat_state") or {}
        enemies = []
        self.targets = {}
        for i, m in enumerate(cs.get("monsters") or []):
            if m.get("is_gone"):
                continue
            eid = f"{upper_id(m.get('id') or m.get('name'))}_{i}"
            self.targets[eid] = i
            enemies.append({
                "entity_id": eid, "name": m.get("name"), "hp": 0 if m.get("half_dead") else m.get("current_hp"),
                "max_hp": m.get("max_hp"), "block": m.get("block", 0), "status": self.powers(m), "intents": self.intents(m),
            })
        playing = "play" in self.commands or "end" in self.commands
        battle = {"round": cs.get("turn"), "turn": "player" if playing else "enemy", "is_play_phase": playing, "enemies": enemies}
        return self._wrap(ROOM_KIND.get(str(g.get("room_type")), "monster"), battle=battle)

    @staticmethod
    def intents(m: dict) -> list[dict]:
        intent = str(m.get("intent") or "UNKNOWN")
        out = []
        if intent.startswith("ATTACK"):
            dmg, hits = m.get("move_adjusted_damage"), int(m.get("move_hits") or 1)
            if isinstance(dmg, (int, float)) and dmg >= 0:
                out.append({"type": "Attack", "label": f"{int(dmg)}x{hits}" if hits > 1 else str(int(dmg))})
            else:
                out.append({"type": "Attack", "label": ""})  # (Runic Dome: the numbers are hidden)
            rest = intent.removeprefix("ATTACK").strip("_")
            if rest:
                out.append({"type": INTENT_TYPE.get(rest, rest.title()), "label": ""})
        else:
            for part in intent.split("_") if intent.startswith("DEFEND_") else [intent]:
                out.append({"type": INTENT_TYPE.get(part, part.title()), "label": ""})
        return out

    def _none(self) -> dict:
        if self.game.get("room_phase") == "COMBAT" and self.game.get("combat_state"):
            return self.combat()
        return self._wrap("overlay")

    def _map(self) -> dict:
        g = self.game
        ss = g.get("screen_state") or {}
        nodes, top = {}, 0
        for n in g.get("map") or []:
            x, y = n.get("x"), n.get("y")
            top = max(top, int(y or 0))
            nodes[(x, y)] = {"col": x, "row": int(y) + 1, "type": MAP_SYMBOL.get(str(n.get("symbol")), "Unknown"),
                             "children": [[c.get("x"), int(c.get("y")) + 1] for c in n.get("children") or []]}
        boss = [3, top + 2]
        for node in nodes.values():  # the boss is not on the map: every road ends there
            node["children"] = [c if (c[0], c[1] - 1) in nodes else boss for c in node["children"]]
            if node["row"] == top + 1 and boss not in node["children"]:
                node["children"].append(boss)
        listed = list(nodes.values()) + [{"col": boss[0], "row": boss[1], "type": "Boss", "children": []}]
        options = []
        if ss.get("boss_available"):
            options.append({"index": 0, "col": boss[0], "row": boss[1], "type": "Boss", "leads_to": []})
        else:
            for i, n in enumerate(ss.get("next_nodes") or []):
                node = nodes.get((n.get("x"), n.get("y"))) or {"type": MAP_SYMBOL.get(str(n.get("symbol")), "Unknown"), "children": []}
                kids = [next((k for k in listed if [k["col"], k["row"]] == c), {"type": "Unknown"}) for c in node["children"]]
                options.append({"index": i, "col": n.get("x"), "row": int(n.get("y") or 0) + 1, "type": node["type"],
                                "leads_to": [{"type": k.get("type"), "row": k.get("row")} for k in kids]})
        return self._wrap("map", map={"next_options": options, "nodes": listed})

    def _combat_reward(self) -> dict:
        items = []
        for i, r in enumerate((self.game.get("screen_state") or {}).get("rewards") or []):
            kind = str(r.get("type") or "").upper()
            if kind == "SAPPHIRE_KEY":
                continue  # (taking it gives up the relic beside it)
            item = {"index": i, "type": {"STOLEN_GOLD": "gold"}.get(kind, kind.lower())}
            if kind in ("GOLD", "STOLEN_GOLD"):
                item["gold_amount"] = r.get("gold")
            elif kind == "POTION":
                p = r.get("potion") or {}
                item.update(potion_id=text.potion_key(p.get("id"), p.get("name")), potion_name=p.get("name"),
                            potion_description=text.potion_text(p.get("id"), p.get("name")), description=text.potion_text(p.get("id"), p.get("name")))
            elif kind == "RELIC":
                rel = r.get("relic") or {}
                item.update(relic_id=upper_id(rel.get("id")), relic_name=rel.get("name"))
            items.append(item)
        return self._wrap("rewards", rewards={"items": items, "can_proceed": "proceed" in self.commands})

    def _card_reward(self) -> dict:
        ss = self.game.get("screen_state") or {}
        cards = [self.card(c, i) for i, c in enumerate(ss.get("cards") or [])]
        state = self._wrap("card_reward", card_reward={"cards": cards, "can_skip": "skip" in self.commands or bool(ss.get("skip_available"))})
        if self.game.get("room_phase") == "COMBAT":
            # A card a card or potion offers in a fight (Discovery, Attack Potion): picked like a reward, but it is for this turn.
            state["card_reward"]["in_combat"] = True
        return state

    def _event(self) -> dict:
        ss = self.game.get("screen_state") or {}
        opts = []
        raw = ss.get("options") or []
        for o in raw:
            words = clean(o.get("text"))
            m = re.match(r"\[([^\]]*)\]\s*(.*)", words)
            title, desc = (m.group(1), m.group(2)) if m else (clean(o.get("label")) or words, words)
            locked = bool(o.get("disabled")) or o.get("choice_index") is None
            opts.append({"index": o.get("choice_index", -1), "title": title, "description": desc, "is_locked": locked,
                         "is_proceed": len(raw) == 1})
        return self._wrap("event", event={"event_id": upper_id(ss.get("event_id") or ss.get("event_name")), "event_name": ss.get("event_name"),
                                          "body": clean(ss.get("body_text")), "options": opts, "in_dialogue": False})

    def _rest(self) -> dict:
        ss = self.game.get("screen_state") or {}
        opts = [{"index": i, "id": str(o).lower(), "name": str(o).title(), "is_enabled": True} for i, o in enumerate(ss.get("rest_options") or [])]
        return self._wrap("rest_site", rest_site={"options": opts, "can_proceed": "proceed" in self.commands})

    def _shop_room(self) -> dict:
        # The merchant's room, his shelf not yet open: the client opens it once a visit (see client.py).
        return self._wrap("shop", shop={"items": [], "can_proceed": "proceed" in self.commands}, _shop_room=True)

    def _shop_screen(self) -> dict:
        ss = self.game.get("screen_state") or {}
        gold = int(self.game.get("gold") or 0)
        items, self.shop_names = [], {}

        def add(cat: str, name: str, price, **fields) -> None:
            i = len(items)
            self.shop_names[i] = name.lower()
            items.append({"index": i, "category": cat, "price": price, "cost": price, "is_stocked": True,
                          "can_afford": int(price or 0) <= gold, **fields})

        for c in ss.get("cards") or []:
            card = self.card(c)
            add("card", str(c.get("name")), c.get("price"), **{"card_" + k: v for k, v in card.items()})
        for r in ss.get("relics") or []:
            add("relic", str(r.get("name")), r.get("price"), relic_id=upper_id(r.get("id")), relic_name=r.get("name"),
                description=text.relic_text(r.get("id"), r.get("name")))
        for p in ss.get("potions") or []:
            add("potion", str(p.get("name")), p.get("price"), potion_id=text.potion_key(p.get("id"), p.get("name")),
                potion_name=p.get("name"), description=text.potion_text(p.get("id"), p.get("name")))
        if ss.get("purge_available"):
            add("card_removal", "purge", ss.get("purge_cost"))
        return self._wrap("shop", shop={"items": items, "can_proceed": True})

    def _chest(self) -> dict:
        ss = self.game.get("screen_state") or {}
        relics = [] if ss.get("chest_open") else [{"index": 0, "name": "chest", "id": upper_id(ss.get("chest_type"))}]
        return self._wrap("treasure", treasure={"relics": relics, "can_proceed": True})

    def _boss_reward(self) -> dict:
        ss = self.game.get("screen_state") or {}
        relics = [{"index": i, "id": upper_id(r.get("id")), "name": r.get("name")} for i, r in enumerate(ss.get("relics") or [])]
        relics.sort(key=text.boss_relic_rank)
        return self._wrap("relic_select", relic_select={"relics": relics, "can_skip": "skip" in self.commands})

    def _grid(self) -> dict:
        g = self.game
        ss = g.get("screen_state") or {}
        n = int(ss.get("num_cards") or 1)
        if ss.get("for_upgrade"):
            kind, verb = "upgrade", "Upgrade"
        elif ss.get("for_transform"):
            kind, verb = "transform", "Transform"
        elif ss.get("for_purge"):
            kind, verb = "remove", "Remove"
        else:
            kind, verb = "", ""
        in_fight = g.get("room_phase") == "COMBAT"
        if not kind and in_fight and n == 1 and not ss.get("any_number"):
            kind = "choose"  # (Seek, Headbutt, Exhume: one click picks and closes)
        prompt = f"Choose {n} card{'s' if n != 1 else ''} to {verb}." if verb else f"Choose {n} card{'s' if n != 1 else ''}."
        cards = [self.card(c, i) for i, c in enumerate(ss.get("cards") or [])]
        confirm = bool(ss.get("confirm_up")) or ("confirm" in self.commands and len(ss.get("selected_cards") or []) >= n)
        return self._wrap("card_select", card_select={"screen_type": kind, "prompt": prompt, "cards": cards, "preview_showing": bool(ss.get("confirm_up")),
                                                      "can_confirm": confirm, "can_cancel": "cancel" in self.commands or "return" in self.commands,
                                                      "can_skip": "skip" in self.commands})

    def _hand_select(self) -> dict:
        g = self.game
        ss = g.get("screen_state") or {}
        n = int(ss.get("max_cards") or 1)
        action = str(g.get("current_action") or "")
        prompt = HAND_PROMPT.get(action, "Choose {n} card(s).").format(n=n)
        cards = [self.card(c, i) for i, c in enumerate(ss.get("hand") or [])]
        picked = len(ss.get("selected") or [])
        ready = "confirm" in self.commands and (picked >= n or not cards or (bool(ss.get("can_pick_zero")) and picked > 0))
        state = self._wrap("hand_select", hand_select={"prompt": prompt, "cards": cards, "can_confirm": ready,
                                                       "mode": "upgrade_select" if action == "ArmamentsAction" else "simple_select"})
        if g.get("combat_state"):  # (the planner judges which card to give up from the fight it is in)
            fight = self.combat()
            state["battle"], state["player"] = fight.get("battle"), {**fight["player"], "hand": cards}
        return state

    def _game_over(self) -> dict:
        ss = self.game.get("screen_state") or {}
        if ss.get("victory"):
            seen = (self.game.get("seed"), self.game.get("floor"))
            if seen != self._won_seen:
                self._won_seen = seen
                self.wins += 1
        return self._wrap("game_over", game_over={"options": ["main_menu"], "victory": bool(ss.get("victory")), "score": ss.get("score")})

    def _complete(self) -> dict:
        return self._game_over()
