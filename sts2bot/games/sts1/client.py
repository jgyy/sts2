"""The bot's client for the first game: the same calls as sts2bot.client.Client, over the relay.

`state()` is the game's state in the bot's words (screens.py) and `act(action, **params)` takes the
bot's actions ("play_card", "choose_map_node", "shop_purchase" ...) and sends the CommunicationMod
command that does the same ("play 2 0", "choose 1", "choose purge" ...). A few things the bot asks
for are done here without the game: the menu before a run (CommunicationMod only has "start").
"""

from __future__ import annotations

import logging
from typing import Any

from ...client import Client, GameUnavailable
from . import text
from .screens import CHARACTERS, Screens

log = logging.getLogger("sts2bot.sts1")

DEFAULT_PORT = 15527
INTENT_WAITS, INTENT_WAIT_FRAMES = 5, 10  # how often, and for how many frames each time, to wait for a fight's intents
SCRY_AWAY, SCRY_STEPS = ("STATUS", "CURSE"), 12  # what Scry discards; clicks before giving the screen back to the bot


def _intents_pending(msg: dict | None) -> bool:
    """A fight's message whose monsters have no intent yet (CommunicationMod says "DEBUG" until the game sets them)."""
    if not isinstance(msg, dict) or "wait" not in (msg.get("available_commands") or []):
        return False
    monsters = ((msg.get("game_state") or {}).get("combat_state") or {}).get("monsters") or []
    return any(str(m.get("intent")) == "DEBUG" for m in monsters if not m.get("is_gone") and not m.get("half_dead") and (m.get("current_hp") or 0) > 0)


class Sts1Client(Client):
    def __init__(self, host: str = "127.0.0.1", port: int = DEFAULT_PORT, timeout: float = 30.0, prefs=None):
        """`prefs`: what the save files say (host.read_prefs's dict), or a function that reads them again: then each
        profile() asks it afresh, so a win's next ascension and a newly unlocked character are seen without a restart."""
        super().__init__(host, port, timeout)
        self.screens = Screens()
        self._shops_opened: set = set()
        self._read_prefs = prefs if callable(prefs) else None
        self._refused: set = set()  # characters the game would not start
        self._take_prefs(self._read_prefs() if self._read_prefs else (prefs or {}))

    def _take_prefs(self, prefs: dict) -> None:
        self.screens.max_ascension = {k: int(v) for k, v in (prefs.get("max_ascension") or {}).items()}
        self.screens.wins = max(self.screens.wins, int(prefs.get("wins") or 0))
        self.screens.locked = set(prefs.get("locked") or ()) | self._refused

    # ------------------------------------------------------------ transport

    def ping(self) -> bool:
        try:
            return bool(self._request(self.base + "/").get("connected"))
        except GameUnavailable:
            return False

    def _message(self) -> dict | None:
        msg = self._request(self.base + "/state").get("message")
        return msg if isinstance(msg, dict) else None

    def send(self, command: str) -> dict[str, Any]:
        """One CommunicationMod command, answered with the game's next state (or its error)."""
        return self._request(self.base + "/command", {"command": command})

    # ------------------------------------------------------------ the Client calls the bot makes

    def state(self) -> dict[str, Any]:
        msg = self._message()
        for _ in range(INTENT_WAITS):
            # The first message of a fight comes before the monsters' intents are set (they read "DEBUG"), and the
            # planner then played its first card blind (Strike into a Jaw Worm's 11, the first live runs). A few
            # frames later they are there.
            if not _intents_pending(msg):
                break
            answer = self.send(f"wait {INTENT_WAIT_FRAMES}")
            if answer.get("status") != "ok" or not isinstance(answer.get("message"), dict):
                break
            msg = answer["message"]
        msg = self._scry(msg)
        st = self.screens.translate(msg)
        if st.get("_shop_room"):
            # The merchant's room comes before his shelf: open it once a visit, the bot only shops on the shelf.
            where = (self.screens.game.get("seed"), self.screens.game.get("floor"))
            if where not in self._shops_opened and "choose" in self.screens.commands:
                self._shops_opened.add(where)
                answer = self.send("choose shop")
                if answer.get("status") == "ok" and isinstance(answer.get("message"), dict):
                    st = self.screens.translate(answer["message"])
        return st

    def _scry(self, msg: dict | None) -> dict | None:
        """The Watcher's Scry: the top cards of the draw pile in a grid, any number of them to discard. Done here: Status and
        Curse cards go, the rest stay, then confirm. (As a card select the bot clicked the three cards on and off for good:
        the grid's confirm never showed as ready, 2026-11-24 21:52, the Watcher's first run.)"""
        for _ in range(SCRY_STEPS):
            g = (msg or {}).get("game_state") or {}
            if str(g.get("screen_type")) != "GRID" or str(g.get("current_action")) != "ScryAction":
                return msg
            ss = g.get("screen_state") or {}
            picked = {c.get("uuid") for c in ss.get("selected_cards") or []}
            wrong = [i for i, c in enumerate(ss.get("cards") or []) if (str(c.get("type")) in SCRY_AWAY) != (c.get("uuid") in picked)]
            answer = self.send(f"choose {wrong[0]}" if wrong else "confirm")
            if answer.get("status") != "ok" or not isinstance(answer.get("message"), dict):
                return msg
            msg = answer["message"]
        return msg

    def profile(self) -> dict[str, Any]:
        """What the save files say, and the wins seen since: enough for the bot to pick a character."""
        if self._read_prefs and not self.screens.raw.get("in_game"):
            self._take_prefs(self._read_prefs())
        return {
            "total_wins": self.screens.wins,
            "characters": [{"id": c, "max_ascension": self.screens.max_ascension.get(c, 0)} for c in CHARACTERS if c not in self.screens.locked],
        }

    def card(self, card_id: str) -> dict[str, Any] | None:
        key = text.card_key(card_id)
        if key is None:
            return None
        cost = text.UPGRADE_COST.get(key)
        base = {"id": key, "description": text.card_text(key), **({"cost": str(cost[0])} if cost else {})}
        upgraded = {"id": key, "description": text.card_text(key, upgraded=True), **({"cost": str(cost[1])} if cost else {})}
        return {"id": key, "base": base, "upgraded": upgraded}

    def act(self, action: str, **params: Any) -> dict[str, Any]:
        local = self._local(action, params)
        if local is not None:
            return local
        command = self.command(action, params)
        if command is None:
            return {"status": "error", "error": f"{action} {params} is not possible here (commands: {', '.join(self.screens.commands)})"}
        answer = self.send(command)
        if answer.get("status") != "ok":
            if command.startswith("start "):
                who = command.split()[1]
                log.warning("the game would not start %s (%s)", who, answer.get("error"))
                self._refused.add(who)
                self.screens.locked.add(who)
            return {"status": "error", "error": str(answer.get("error") or "the game refused the command")}
        if isinstance(answer.get("message"), dict):
            self.screens.translate(answer["message"])
        return {"status": "ok", "message": command}

    # ------------------------------------------------------------ the way back

    def _local(self, action: str, params: dict) -> dict | None:
        """Menu clicks: the menu before a run is the bot's own (screens.Screens.menu)."""
        sc = self.screens
        if self.screens.raw.get("in_game") or action not in ("menu_select", "set_ascension"):
            return None
        ok = lambda what: {"status": "ok", "message": what}
        if action == "set_ascension":
            sc.ascension = max(0, min(20, int(params.get("level") or 0)))
            return ok(f"ascension {sc.ascension}")
        option = str(params.get("option") or "")
        if option == "singleplayer" and "start" in sc.commands:
            sc.menu_screen, sc.character = "character_select", None
            return ok(option)
        if option == "back":
            sc.menu_screen, sc.character = "main", None
            return ok(option)
        if sc.menu_screen == "character_select" and option.upper() in CHARACTERS and option.upper() not in sc.locked:
            sc.character = option.upper()
            sc.ascension = sc.max_ascension.get(sc.character, 0)
            return ok(option)
        if sc.menu_screen == "character_select" and option in ("confirm", "embark") and sc.character:
            return None  # the game starts the run: see command()
        return {"status": "error", "error": f"no menu option {option!r} here"}

    def command(self, action: str, p: dict) -> str | None:
        """The CommunicationMod command for one of the bot's actions (None: there is none on this screen)."""
        sc = self.screens
        cmds = set(sc.commands)
        screen = str(sc.game.get("screen_type") or "")
        choose = lambda i: f"choose {int(i)}" if "choose" in cmds else None
        first = lambda *names: next((n for n in names if n in cmds), None)

        if action == "menu_select":
            option = str(p.get("option") or "")
            if option in ("confirm", "embark") and sc.character and "start" in cmds:
                return f"start {sc.character} {int(sc.ascension or 0)}"
            if option in ("main_menu", "force_main_menu", "continue"):
                return first("proceed", "confirm")
            return None
        if action == "play_card":
            if "play" not in cmds:
                return None
            target = p.get("target")
            if target is not None and str(target) in sc.targets:
                return f"play {int(p['card_index']) + 1} {sc.targets[str(target)]}"
            return f"play {int(p['card_index']) + 1}"
        if action == "end_turn":
            return "end" if "end" in cmds else None
        if action in ("use_potion", "discard_potion"):
            if "potion" not in cmds:
                return None
            verb = "use" if action == "use_potion" else "discard"
            target = p.get("target")
            tail = f" {sc.targets[str(target)]}" if verb == "use" and target is not None and str(target) in sc.targets else ""
            return f"potion {verb} {int(p.get('slot', 0))}{tail}"
        if action in ("choose_map_node", "claim_reward", "select_card_reward", "choose_rest_option", "choose_event_option",
                      "select_card", "combat_select_card", "select_relic"):
            return choose(p.get("index", p.get("card_index", 0)))
        if action == "claim_treasure_relic":
            return choose(0)
        if action == "advance_dialogue":
            return choose(0)
        if action == "shop_purchase":
            name = sc.shop_names.get(int(p.get("index", -1)))
            return f"choose {name}" if name and "choose" in cmds else None
        if action in ("skip_card_reward", "skip_relic_selection"):
            return first("skip", "cancel", "return")
        if action in ("confirm_selection", "combat_confirm_selection", "confirm_bundle_selection"):
            return first("confirm", "proceed")
        if action == "cancel_selection":
            return first("cancel", "return", "skip")
        if action == "proceed":
            if screen == "SHOP_SCREEN":
                return first("leave", "return", "cancel")
            return first("proceed", "confirm", "leave", "return", "skip")
        return None
