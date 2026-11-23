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


class Sts1Client(Client):
    def __init__(self, host: str = "127.0.0.1", port: int = DEFAULT_PORT, timeout: float = 30.0, prefs: dict | None = None):
        super().__init__(host, port, timeout)
        self.screens = Screens()
        self._shops_opened: set = set()
        prefs = prefs or {}
        self.screens.max_ascension = {k: int(v) for k, v in (prefs.get("max_ascension") or {}).items()}
        self.screens.wins = int(prefs.get("wins") or 0)

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
        st = self.screens.translate(self._message())
        if st.get("_shop_room"):
            # The merchant's room comes before his shelf: open it once a visit, the bot only shops on the shelf.
            where = (self.screens.game.get("seed"), self.screens.game.get("floor"))
            if where not in self._shops_opened and "choose" in self.screens.commands:
                self._shops_opened.add(where)
                answer = self.send("choose shop")
                if answer.get("status") == "ok" and isinstance(answer.get("message"), dict):
                    st = self.screens.translate(answer["message"])
        return st

    def profile(self) -> dict[str, Any]:
        """What the save files said, and the wins seen since: enough for the bot to pick a character."""
        return {
            "total_wins": self.screens.wins,
            "characters": [{"id": c, "max_ascension": self.screens.max_ascension.get(c, 0)} for c in CHARACTERS],
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
