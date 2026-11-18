"""Main loop: read state, decide, act, repeat. One Bot plays run after run."""

from __future__ import annotations

import hashlib
import json
import logging
import time
from collections import defaultdict
from pathlib import Path

from . import cards, combat, learn, strategy
from .bossfight import BossWatch
from .client import Client
from .strategy import Memory

log = logging.getLogger("sts2bot")

COMBAT_STATES = ("monster", "elite", "boss")
MAX_STRIKES = 2  # failed attempts before an action is shelved for the current state
MAX_PLAYS_PER_TURN = 40
_VOLATILE = {"is_hovered", "is_highlighted"}
# Cards a relic or an enemy shuffles in when a fight starts (the piles do not say what type a card is).
NOT_OURS = {"Dazed", "Wound", "Burn", "Slimed", "Soot", "Debris", "Infection", "Toxic", "Void", "Wither", "Frantic Escape"}


class Stuck(Exception):
    """Nothing the bot tries changes the game any more."""


def _scrub(obj):
    if isinstance(obj, dict):
        return {k: _scrub(v) for k, v in obj.items() if k not in _VOLATILE}
    if isinstance(obj, list):
        return [_scrub(v) for v in obj]
    return obj


def fingerprint(state: dict) -> str:
    raw = json.dumps(_scrub(state), sort_keys=True, default=str)
    return hashlib.sha1(raw.encode()).hexdigest()


class Bot:
    def __init__(
        self,
        client: Client,
        *,
        character: str = "auto",
        log_dir: Path | None = None,
        poll: float = 0.4,
        settle: float = 3.0,
        stuck_timeout: float = 300.0,
        tune: bool = False,  # try different settings from run to run and learn which work (see learn.Knobs)
        clock=time.monotonic,
        sleep=time.sleep,
    ):
        self.client = client
        self.character = character
        self.log_dir = log_dir
        self.poll = poll
        self.settle = settle
        self.travel_settle = 8.0
        self.quiet_settle = 0.3
        self.choose_settle = 0.5
        # Set by the supervisor after the game crashed twice in the same place:
        # until this time, act slowly and ask for the state rarely.
        self.careful_until = 0.0
        self.stuck_timeout = stuck_timeout
        self.clock = clock
        self.sleep = sleep

        self.mem = Memory()
        self.card_stats = learn.CardStats(log_dir / "card_stats.json" if log_dir else None)
        if log_dir:
            learn.seed_from_history(self.card_stats, log_dir / "runs.jsonl")
        self.mem.card_stats = self.card_stats
        self._upgrades: dict = {}  # card id -> what its upgrade adds (None: the game could not say)
        self.mem.upgrade_gain = self._upgrade_gain
        self.knobs = learn.Knobs(log_dir / "tuning.json" if log_dir else None)
        self._knobs_whole_run = False  # the settings in force were there from the run's first floor
        self.tune = tune
        if tune:
            self.knobs.draw()
        self.max_runs: int | None = None
        self.abandon_next = False  # set by the supervisor to drop a run that keeps hanging
        self.runs_finished = 0
        self.wins = 0
        self._strikes: dict = defaultdict(int)
        self._last_fp = ""
        self._last_change = clock()
        self._last_retry = clock()
        self._over_logged = False
        self._boss_watch = BossWatch()
        self._turn_key: tuple = ()
        self._turn_plays = 0
        self._last_where: tuple = ()
        self._seen_powers: set = set()
        self._wins_seen: int | None = None  # the game's win counter when we last looked
        self._wins_asked = False  # looked it up once for a run already under way
        self._timeline_idle = 0  # Timeline clicks in a row that changed nothing
        self._facing: str | None = None  # enemy we last aimed at in this fight
        self._facing_fight: tuple = ()
        self._revive_fight: tuple = ()  # the fight in which the enemy last seen revives when beaten
        self._end_check: tuple = ()  # the hand we last thought about ending the turn on
        self._played_from: tuple = ()  # the hand as it was when we last played a card from it
        self._played_at = 0.0

    # ------------------------------------------------------------ deciding

    def progress(self) -> tuple:
        """Changes whenever the bot gets somewhere new (used to spot repeat hangs)."""
        return (self.runs_finished, *self._last_where)

    def candidates(self, state: dict) -> list[tuple[str, dict]]:
        kind = state.get("state_type")
        mem = self.mem
        if kind == "menu":
            if state.get("menu_screen") == "character_select" and not mem.character_selected:
                self._refresh_ascensions()
            if self.abandon_next and (state.get("menu_screen") or "main") == "main":
                names = [o if isinstance(o, str) else o.get("name") for o in state.get("options") or []]
                if "abandon_run" in names:
                    return [("menu_select", {"option": "abandon_run"})]
                # No run to abandon only once the menu offers a new one. Just
                # after a launch the menu lists neither for a few seconds:
                # deciding then dropped the order, the bot pressed Continue,
                # and a run that crashed the game on loading did so nine times.
                if "singleplayer" not in names:
                    return []
                self.abandon_next = False
            return strategy.menu(state, mem, self.character)
        if kind in COMBAT_STATES:
            return self._combat(state)
        handler = {
            "hand_select": strategy.hand_select,
            "rewards": strategy.rewards,
            "card_reward": strategy.card_reward,
            "map": strategy.map_screen,
            "event": strategy.event,
            "rest_site": strategy.rest_site,
            "shop": strategy.shop,
            "treasure": strategy.treasure,
            "card_select": strategy.card_select,
            "bundle_select": strategy.bundle_select,
            "relic_select": strategy.relic_select,
            "crystal_sphere": strategy.crystal_sphere,
            "game_over": strategy.game_over,
            "overlay": strategy.overlay,
        }.get(kind)
        if kind == "fake_merchant":
            return strategy.shop(state, mem, key="fake_merchant")
        return handler(state, mem) if handler else []

    def _upgrade_gain(self, card: dict) -> float | None:
        """What upgrading this card adds, from the game's own text for both versions."""
        cid = str(card.get("id") or "")
        lookup = getattr(self.client, "card", None)
        if not cid or lookup is None:
            return None
        if cid not in self._upgrades:
            gain = None
            try:
                entry = lookup(cid)
                if entry and entry.get("base") and entry.get("upgraded"):
                    gain = cards.upgrade_gain(entry["base"], entry["upgraded"])
            except Exception as e:  # a hiccup must not hold up the choice
                log.debug("could not look up %s: %s", cid, e)
                return None
            self._upgrades[cid] = gain
        return self._upgrades[cid]

    def _refresh_ascensions(self) -> None:
        """Ask the game how far each character has climbed (used to pick who plays next)."""
        fetch = getattr(self.client, "profile", None)
        if fetch is None:
            return
        try:
            profile = fetch()
            if isinstance(profile.get("total_wins"), (int, float)):
                self._wins_seen = int(profile["total_wins"])
            self.mem.epochs = {str(e.get("id")): str(e.get("state")) for e in profile.get("epochs") or [] if e.get("id")}
            chars = profile.get("characters") or []
            self.mem.max_ascension = {str(c.get("id")): int(c.get("max_ascension") or 0) for c in chars if c.get("id")}
            kills: dict = {str(c.get("id")).upper(): 0 for c in chars if c.get("id")}
            for enc in profile.get("encounter_stats") or []:
                if str(enc.get("id")).upper().endswith("_BOSS"):
                    for row in enc.get("by_character") or []:
                        who = str(row.get("character")).upper()
                        kills[who] = kills.get(who, 0) + int(row.get("wins") or 0)
            self.mem.boss_kills = kills
            # How far each character has been getting lately ("The Defect" in our records is DEFECT here).
            for name, finals in self.card_stats.finals.items():
                recent = finals[-15:]
                for cid in kills:
                    if cid in name.upper().replace(" ", "") and recent:
                        self.mem.strength[cid] = min(1.0, sum(recent) / len(recent) / 48.0)
        except Exception as e:  # an old mod or a hiccup must not stop the run from starting
            log.debug("could not read the profile: %s", e)

    def _combat(self, state: dict) -> list[tuple[str, dict]]:
        battle = state.get("battle")
        player = state.get("player") or {}
        if not battle or "hand" not in player:
            return []  # combat is starting or just ended
        if battle.get("turn") != "player" or not battle.get("is_play_phase"):
            return []
        self._note_deck(state)
        key = (*self._where(state), battle.get("round"))
        if key != self._turn_key:
            self._turn_key, self._turn_plays = key, 0
        if key[:2] != self._facing_fight:
            self._facing_fight, self._facing = key[:2], None  # a new fight
        if self._turn_plays >= MAX_PLAYS_PER_TURN:
            return [("end_turn", {})]
        # A card we just played stays in the hand for a moment. Seen again
        # unchanged, the same card was sent a second and third time, and the
        # end of turn that followed left the game waiting for ever.
        if self._hand_sig(state) == self._played_from and self.clock() - self._played_at < 2.0:
            return []
        # An enemy that revives stronger (Adaptable) leaves the field for the
        # rest of the turn it is beaten in: remember that it is coming back.
        standing = [e for e in battle.get("enemies") or [] if int(e.get("hp") or 0) > 0]
        if standing:
            self._revive_fight = key[:2] if any(combat.power_amount(e, "ADAPTABLE") for e in standing) else ()
        revive = not standing and self._revive_fight == key[:2]
        wanted = combat.actions({**state, "_plays_this_turn": self._turn_plays, "_facing": self._facing, "_revive_pending": revive})
        # Cards that draw (or reshuffle the hand, like Reboot) show up a moment
        # after the card that brought them: straight after Reboot the hand read
        # as empty, and the bot ended a turn with 4 energy and a kill in reach.
        # So before ending a turn with energy left, look once more.
        if wanted and wanted[0][0] == "end_turn" and self._turn_plays and int(player.get("energy") or 0) > 0:
            check = (key, self._turn_plays, tuple(str(c.get("name")) for c in player.get("hand") or []))
            if check != self._end_check:
                self._end_check = check
                return []
        if wanted and wanted[0][0] == "end_turn" and log.isEnabledFor(logging.DEBUG):
            expect = combat.expected_end_loss(state)
            if expect is not None:
                log.debug("ending r%s on %s HP, expecting to lose %d", battle.get("round"), player.get("hp"), expect)
        return wanted

    @staticmethod
    def _hand_sig(state: dict) -> tuple:
        """The hand and what we have to spend: unchanged until a played card has left it."""
        player = state.get("player") or {}
        hand = tuple((c.get("index"), c.get("name")) for c in player.get("hand") or [])
        return ((state.get("battle") or {}).get("round"), hand, player.get("energy"), player.get("stars"))

    def _note_deck(self, state: dict) -> None:
        """Remember the deck as it stands when a fight begins.

        Only then: later in a fight the piles also hold what the fight put
        there (Wounds, Shivs, Souls). Counting those made a 17-card deck look
        like 26, and the bot turned down good cards for being "too big".
        """
        player = state.get("player") or {}
        battle = state.get("battle") or {}
        fresh = (
            int(battle.get("round") or 0) == 1
            and not int(player.get("discard_pile_count") or 0)
            and not int(player.get("exhaust_pile_count") or 0)
        )
        if not fresh:
            if self.mem.deck_size <= 0:
                self.mem.deck_size = len(player.get("hand") or []) + int(player.get("draw_pile_count") or 0)
            return
        keep = ("id", "name", "type", "cost", "star_cost", "description", "rarity", "is_upgraded")
        cards = [{k: c.get(k) for k in keep if c.get(k) is not None} for c in player.get("hand") or []]
        pile = player.get("draw_pile")
        if isinstance(pile, list) and len(pile) == int(player.get("draw_pile_count") or 0):
            cards += [{k: c.get(k) for k in keep if c.get(k) is not None} for c in pile]
            self.mem.deck = [c for c in cards if str(c.get("type")) != "Status" and str(c.get("name")) not in NOT_OURS]
            self.mem.deck_size = len(self.mem.deck)
        else:  # a mod that does not show the piles: the count has to do
            self.mem.deck_size = len(cards) + int(player.get("draw_pile_count") or 0)

    @staticmethod
    def _where(state: dict) -> tuple:
        run = state.get("run") or {}
        return (run.get("act"), run.get("floor"))

    # -------------------------------------------------------------- acting

    def _after_success(self, state: dict, action: str, params: dict) -> None:
        kind = state.get("state_type")
        if action == "select_card":
            self.mem.selected.add(params.get("index"))
        elif action == "select_card_reward":
            for c in (state.get("card_reward") or {}).get("cards") or []:
                if c.get("index") == params.get("card_index"):
                    self.mem.deck.append({k: v for k, v in c.items() if k not in ("index", "keywords")})
                    self.mem.deck_size += 1
                    self.mem.picks.append([(state.get("run") or {}).get("floor"), str(c.get("name"))])
        elif action == "skip_card_reward":
            where = self._where(state)
            self.mem.card_skips[where] = self.mem.card_skips.get(where, 0) + 1
        elif action in ("play_card", "use_potion"):
            self._turn_plays += 1
            if action == "play_card":
                self._played_from, self._played_at = self._hand_sig(state), self.clock()
            if params.get("target"):
                self._facing = params["target"]  # we turn towards whatever we aim at
        elif action == "menu_select" and kind == "menu":
            option = str(params.get("option"))
            if state.get("menu_screen") == "character_select" and option not in ("confirm", "embark"):
                self.mem.character_selected = True
                self.mem.current_character = option
            elif option == "abandon_run":
                log.warning("abandoning the current run")
            elif option in ("yes", "confirm") and self.abandon_next and state.get("menu_screen") == "popup":
                self.abandon_next = False
                self.mem.reset_run()

    def _after_timeline_click(self, params: dict, result: dict) -> None:
        """Follow the Timeline: the mod reveals one thing per click and says when nothing is left."""
        message = str(result.get("message") or "")
        if message.startswith("Revealing epoch") or message.startswith("Confirmed"):
            log.info("timeline: %s", message)
        if str(params.get("option")) == "advance":
            progressed = message.startswith(("Revealing", "Confirmed", "Closed", "Clicked", "Opening"))
            # "Done" has to hold for a few clicks in a row: between two
            # animations the Timeline briefly looks finished when it is not.
            self._timeline_idle = 0 if progressed else self._timeline_idle + (3 if result.get("done") else 1)
            # ... and nothing at all happening for half a minute means the same.
            if self._timeline_idle >= (9 if result.get("done") else 45):
                self._timeline_idle = 0
                self.mem.timeline_done = True
                # Whatever is still pending cannot be revealed yet; do not come back for it.
                self.mem.timeline_stuck = set(result.get("pending_epoch_ids") or [])
        self.sleep(0.6)  # let the reveal animation run before the next click

    def _note_powers(self, state: dict) -> None:
        """Log each power's rules text the first time it is met (the planner only
        knows the ones it was taught; this is how the rest get noticed)."""
        creatures = [state.get("player") or {}, *((state.get("battle") or {}).get("enemies") or [])]
        for c in creatures:
            for p in c.get("status") or []:
                pid = str(p.get("id"))
                if pid not in self._seen_powers:
                    self._seen_powers.add(pid)
                    log.info("power %s (on %s): %s", pid, c.get("name") or c.get("character") or "player", p.get("description"))
        # Same for keywords on our cards (enchantments, afflictions a boss puts on them, ...),
        # and for the cards a fight puts in our hand (Wither, Beckon, Frantic Escape, ...).
        for c in (state.get("player") or {}).get("hand") or []:
            if str(c.get("type")) in ("Status", "Curse"):
                name = "card:" + str(c.get("name"))
                if name not in self._seen_powers:
                    self._seen_powers.add(name)
                    log.info("card %s (%s, cost %s): %s", c.get("name"), c.get("type"), c.get("cost"), c.get("description"))
            for k in c.get("keywords") or []:
                name = "kw:" + str(k.get("name"))
                if name not in self._seen_powers:
                    self._seen_powers.add(name)
                    log.info("keyword %s (on %s): %s", k.get("name"), c.get("name"), k.get("description"))

    def _note_state(self, state: dict) -> None:
        kind = state.get("state_type")
        ended = self._boss_watch.update(state, kind)
        if ended:
            log.info(ended)
        if kind in COMBAT_STATES:
            self._note_powers(state)
        if state.get("run"):
            if self._wins_seen is None and kind != "game_over" and not self._wins_asked:
                # A bot started in the middle of a run has not been through
                # the menu, where the win counter is first read. Without it
                # the end of that run fell back on the end screen's HP, and two
                # wins (the Regent's and the Silent's first) went down as losses.
                self._wins_asked = True
                self._wins_seen = self._profile_wins()
            where = self._where(state)
            if where != self._last_where:
                self._last_where = where
                p = state.get("player") or {}
                log.info("act %s floor %s  %s  hp %s/%s  gold %s", *where, kind, p.get("hp"), p.get("max_hp"), p.get("gold"))
        if kind == "game_over":
            if not self._over_logged:
                self._over_logged = True
                self._record_run(state)
        elif self._over_logged:
            self._over_logged = False
            self.mem.reset_run()
            if self.tune:
                log.info("settings for the next run: %s", self.knobs.draw())
                self._knobs_whole_run = True
            self.mem.runs_finished = self.runs_finished

    def _profile_wins(self) -> int | None:
        """Total wins according to the game's own records (None if it cannot be asked)."""
        fetch = getattr(self.client, "profile", None)
        if fetch is None:
            return None
        try:
            wins = fetch().get("total_wins")
        except Exception as e:
            log.debug("could not read the profile: %s", e)
            return None
        return int(wins) if isinstance(wins, (int, float)) else None

    def _note_epochs_earned(self) -> None:
        """Keep count of runs that earned a character nothing, so "auto" can move on from it."""
        who = self.mem.current_character.upper()
        fetch = getattr(self.client, "profile", None)
        if not who or fetch is None:
            return
        try:
            epochs = {str(e.get("id")): str(e.get("state")) for e in fetch().get("epochs") or [] if e.get("id")}
        except Exception as e:
            log.debug("could not read the profile: %s", e)
            return
        earned = lambda table: {k for k, v in table.items() if v in strategy._EARNED}
        new = earned(epochs) - earned(self.mem.epochs)
        if new:
            log.info("epochs earned this run: %s", ", ".join(sorted(new)))
            self.mem.epoch_drought[who] = 0
        else:
            self.mem.epoch_drought[who] = self.mem.epoch_drought.get(who, 0) + 1
        self.mem.epochs = epochs

    def _record_run(self, state: dict) -> None:
        p = state.get("player") or {}
        run = state.get("run") or {}
        # The end screen looks the same for a win and a loss, and the HP it
        # reports is not reliable (the first win was logged as a loss). The
        # game's own win counter is: it goes up by one when a run is won.
        before, now = self._wins_seen, self._profile_wins()
        if before is not None and now == before and int(run.get("act") or 0) >= 3:
            # The game writes a win to the profile a moment after the end
            # screen comes up: asked 0.3 s after the last click, the counter
            # had not moved and the Regent's first win went down as a loss
            # (and taught the card ratings and settings the wrong thing).
            for _ in range(8):
                self.sleep(0.5)
                now = self._profile_wins()
                if now is None or now > before:
                    break
        self._note_epochs_earned()
        if now is not None:
            self._wins_seen = now
        if before is not None and now is not None:
            won = now > before
        else:
            won = int(p.get("hp") or 0) > 0
        self.runs_finished += 1
        self.wins += won
        entry = {
            "time": time.strftime("%Y-%m-%dT%H:%M:%S%z"),
            "character": p.get("character"),
            "result": "win" if won else "loss",
            "act": run.get("act"),
            "floor": run.get("floor"),
            "ascension": run.get("ascension"),
            "hp": p.get("hp"),
            "max_hp": p.get("max_hp"),
            "gold": p.get("gold"),
            "relics": [r.get("id") for r in p.get("relics") or []],
            "deck_size": self.mem.deck_size,
            "deck": sorted(str(c.get("name")) for c in self.mem.deck),
            "picks": self.mem.picks,
        }
        who = str(entry["character"] or "")
        usual = self.card_stats.usual(who, 0)
        if self._knobs_whole_run and usual is not None and isinstance(entry["floor"], int):
            self.knobs.record(entry["floor"] + (learn.WIN_BONUS if won else 0) - usual)
        entry["knobs"] = dict(learn.KNOB) if self._knobs_whole_run else None
        self.card_stats.record(who, entry["floor"], bool(won), self.mem.picks)
        log.info("run %d over: %s on act %s floor %s (%d wins so far)", self.runs_finished, entry["result"], entry["act"], entry["floor"], self.wins)
        if self.log_dir:
            self.log_dir.mkdir(parents=True, exist_ok=True)
            with open(self.log_dir / "runs.jsonl", "a") as f:
                f.write(json.dumps(entry) + "\n")

    @staticmethod
    def _is_quiet(state: dict, action: str, params: dict) -> bool:
        """Actions that work without the reported state changing.

        Ticking a card in a selection grid and picking the character that is
        already highlighted are invisible in the state, so waiting for a change
        only burns the whole settle time.
        """
        if action == "select_card":
            return (state.get("card_select") or {}).get("screen_type") != "choose"
        if action == "menu_select" and state.get("menu_screen") == "character_select":
            return str(params.get("option")) not in ("confirm", "embark")
        if action == "menu_select" and state.get("menu_screen") == "timeline":
            return True  # the Timeline animates between clicks; the state says nothing about it
        return False

    @staticmethod
    def _is_choose(state: dict, action: str) -> bool:
        """Picking one card on a "choose 1 of 3" screen.

        The game takes the click but ignores it while the screen is still
        sliding in. Waiting the full settle time before trying again cost three
        seconds on every such card, so these are re-sent after a short wait.
        """
        return action == "select_card" and (state.get("card_select") or {}).get("screen_type") == "choose"

    def _wait_for_change(
        self, fp: str, leave: str | None = None, quiet: bool = False, settle: float | None = None, slow: bool = False
    ) -> bool:
        """Wait for the action to show. With `leave`, wait until that screen is gone.

        The map keeps answering with fresh options while the travel animation
        plays; acting on those would click a second node.

        Returns True if the game state changed. Every state request costs a
        game frame or more, so the answer is reused instead of asking again.
        """
        deadline = self.clock() + (
            settle if settle is not None else self.travel_settle if leave else self.quiet_settle if quiet else self.settle
        )
        changed = False
        # While a room is loading the game is rebuilding its scene; ask less
        # often then (a state request walks that scene, and the game once
        # crashed mid-travel under rapid polling).
        pause = min(0.15 if leave or slow else 0.03, self.poll)
        if self.clock() < self.careful_until:
            pause = max(pause, 0.5)
        while self.clock() < deadline:
            self.sleep(pause)
            state = self.client.state()
            changed = changed or fingerprint(state) != fp
            if leave:
                if state.get("state_type") != leave:
                    return True
            elif changed:
                return True
        return changed

    def step(self) -> bool:
        """One read-decide-act cycle. Returns True if an action was sent."""
        state = self.client.state()
        fp = fingerprint(state)
        now = self.clock()
        if fp != self._last_fp:
            self._last_fp, self._last_change, self._last_retry = fp, now, now
            self._strikes.clear()
        elif now - self._last_change > self.stuck_timeout:
            raise Stuck(f"no change for {self.stuck_timeout:.0f}s in state {state.get('state_type')!r}")
        elif now - self._last_retry > 8:  # clicks sent mid-animation are ignored; try again soon
            self._last_retry = now
            self._strikes.clear()  # give shelved actions another go

        self._note_state(state)
        if self._done():
            return False
        for action, params in self.candidates(state):
            key = (action, json.dumps(params, sort_keys=True))
            if self._strikes[key] >= MAX_STRIKES:
                continue
            result = self.client.act(action, **params)
            if result.get("status") == "error" or "error" in result:
                self._strikes[key] += 1
                log.debug("%s %s rejected: %s", action, params, result.get("error"))
                self.sleep(min(0.15, self.poll))
                return True
            log.debug("%s %s -> %s", action, params, result.get("message"))
            self._after_success(state, action, params)
            if action == "menu_select" and state.get("menu_screen") == "timeline":
                self._after_timeline_click(params, result)
            quiet = self._is_quiet(state, action, params)
            choose = self._is_choose(state, action)
            # Leaving a room and buying something both rebuild part of the
            # scene. Asking for the state every 30 ms through that crashed the
            # game (reliably, when a card was bought and the shop left at once),
            # so give it a moment first and then ask less often.
            fragile = action in ("proceed", "shop_purchase")
            # Embarking loads the whole run; clicking the character again
            # while it does crashed the game. Wait for the menu to go away.
            embarking = (
                action == "menu_select"
                and state.get("menu_screen") == "character_select"
                and str(params.get("option")) in ("confirm", "embark")
            )
            if fragile or embarking:
                self.sleep(0.4)
            if self.clock() < self.careful_until:
                self.sleep(1.0)
            changed = self._wait_for_change(
                fp,
                leave="map" if action == "choose_map_node" else "menu" if embarking else None,
                quiet=quiet,
                settle=self.choose_settle if choose else None,
                slow=fragile,
            )
            if not quiet and not choose and not changed:
                self._strikes[key] += 1  # accepted, but nothing happened
            return True
        self.sleep(self.poll)
        return False

    def _done(self) -> bool:
        """True once the requested number of runs is over and its end screen is dismissed."""
        return self.max_runs is not None and self.runs_finished >= self.max_runs and not self._over_logged

    def run(self, max_runs: int | None = None) -> None:
        """Play until `max_runs` runs have finished (forever if None)."""
        self.max_runs = max_runs
        self._last_change = self.clock()
        while not self._done():
            self.step()
