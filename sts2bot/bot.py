"""Main loop: read state, decide, act, repeat. One Bot plays run after run."""

from __future__ import annotations

import hashlib
import json
import logging
import time
from collections import defaultdict
from pathlib import Path

from . import cards, learn
from .fight import combat
from .run import events, relics, strategy
from .fight.bossfight import BossWatch
from .fightrecord import FightWatch
from .client import Client
from .run.strategy import Memory

log = logging.getLogger("sts2bot")

COMBAT_STATES = ("monster", "elite", "boss")
RECENT_RUNS = 40  # runs whose characters the rotation looks back over (strategy.choose_character)
# Screens strategy.py has a handler for, under the screen's own name (the map's is map_screen).
SCREENS = {"hand_select", "rewards", "card_reward", "map", "event", "rest_site", "shop", "treasure", "card_select", "bundle_select", "relic_select", "crystal_sphere", "game_over", "overlay"}
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
    def __init__(self, client: Client, *, character: str = "auto", log_dir: Path | None = None, poll: float = 0.4, settle: float = 3.0,
                 stuck_timeout: float = 300.0, tune: bool = False, clock=time.monotonic, sleep=time.sleep):
        """`tune`: try different settings from run to run and learn which work (learn.Knobs)."""
        self.client, self.character, self.log_dir, self.poll = client, character, log_dir, poll
        # Pauses after leaving a room or buying, and between looks while a room loads: faster crashed StS2 (StS1's are ~0: __main__).
        self.room_pause, self.room_poll = 0.4, 0.15
        self.settle, self.travel_settle, self.quiet_settle, self.choose_settle = settle, 8.0, 0.3, 0.5
        self.careful_until, self.careful_at = 0.0, None  # after two crashes in one place (a progress()): act slowly there until then
        self._crashed: set = set()  # (act, floor, action, params) asked last before a crash: the game reloads that room, so avoid it there
        self._last_sent: tuple | None = None  # the last such request since the game was (re)started
        self.time_scale, self._scale_at = 0.0, 0.0  # the game clock's speed-up (0: none), re-sent every few seconds: boss fights reset it
        self.stuck_timeout, self.clock, self.sleep = stuck_timeout, clock, sleep

        runs = log_dir / "runs.jsonl" if log_dir else None
        self.mem = Memory()
        self.card_stats = self.mem.card_stats = learn.CardStats(log_dir / "card_stats.json" if log_dir else None)
        if runs:
            learn.seed_from_history(self.card_stats, runs)
            self.mem.recent = [str(r.get("character") or "") for r in learn.history(runs)][-RECENT_RUNS:]  # (the rotation goes on across restarts)
        relics.load(runs, events._ANCIENT)  # what the Ancients' relics did in our runs
        self._upgrades: dict = {}  # card id -> what its upgrade adds (None: the game could not say)
        self.mem.upgrade_gain = self._upgrade_gain
        self.knobs = learn.Knobs(log_dir / "tuning.json" if log_dir else None)
        self._knobs_whole_run = False  # the settings in force were there from the run's first floor
        self.tune = tune
        if tune:
            self.knobs.draw()
        self.max_runs: int | None = None
        self.abandon_next = False  # set by the supervisor to drop a run that keeps hanging
        self.runs_finished = self.wins = 0
        self._strikes: dict = defaultdict(int)
        self._dead_buys: dict = defaultdict(int)  # purchases accepted that changed nothing, by (act, floor, item): never forgiven
        self._last_fp, self._last_change, self._last_retry = "", clock(), clock()
        self._over_logged, self._prev_kind = False, None  # (the state before this one: a run that ends straight out of a fight was lost)
        self._boss_watch, self._fight_watch = BossWatch(), FightWatch(log_dir / "fights.jsonl" if log_dir else None)  # (every fight, kept past the logs)
        # This turn: its key, plays, cards played (a potion is not one of the "3 cards each turn") and their names (the Fool's Melodies).
        self._turn_key, self._turn_plays, self._turn_cards, self._turn_names = (), 0, 0, []
        self._stuck, self._dislodged = set(), False  # enemies holding a Fool's Arrow, and whether one came out this turn (Yah!'s energy is spent then)
        self._fight_state: dict | None = None  # the combat state as the planner last saw it (for a potion's offer of cards)
        self._turn_energy, self._turns = 0, []  # energy when we last looked at this turn; (cards played, energy left) for each turn ended in this fight
        self._last_where, self._seen_powers = (), set()
        self._wins_seen, self._wins_asked = None, False  # the game's win counter when we last looked; asked once for a run already under way
        self._timeline_idle = 0  # Timeline clicks in a row that changed nothing
        self._facing, self._facing_fight = None, ()  # enemy we last aimed at, in this fight
        self._threat, self._threat_fight = {}, ()  # enemy id -> [damage it has aimed at us in this fight, rounds seen, its blows, the turns it attacked]
        self._revive_fight = ()  # the fight in which the enemy last seen revives when beaten
        self._end_check, self._played_from = (), ()  # the hand we last thought of ending the turn on, and the one we last played from
        self._fight_kind, self._played_at = "monster", 0.0  # monster, elite or boss: the fight we were last in

    def progress(self) -> tuple:
        """Changes whenever the bot gets somewhere new (used to spot repeat hangs)."""
        return (self.runs_finished, *self._last_where)

    def _careful(self) -> bool:
        return self.clock() < self.careful_until and self.careful_at in (None, self.progress())

    def note_crash(self) -> bool:
        """The game just died: never ask its last request again in that room. True if that is something new to avoid."""
        sent, self._last_sent = self._last_sent, None
        if sent is None or sent in self._crashed:
            return False
        self._crashed.add(sent)
        log.warning("the game died after %s %s at act %s floor %s: that will not be tried there again", sent[2], sent[3], sent[0], sent[1])
        return True

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
                # No run to abandon only once the menu offers a new one (just after a launch it lists neither for a moment).
                if "singleplayer" not in names:
                    return []
                self.abandon_next = False
            return strategy.menu(state, mem, self.character)
        if kind in COMBAT_STATES:
            return self._combat(state)
        if kind == "fake_merchant":
            return strategy.shop(state, mem, key="fake_merchant")
        if kind == "hand_select":
            state = {**state, "_fight": self._fight_kind}  # the screen does not say what kind of fight it is in
        in_fight = kind == "card_select" or (kind == "card_reward" and bool((state.get("card_reward") or {}).get("in_combat")))
        if in_fight and self._turn_key[:2] == strategy._where(state):
            state = {**state, "_turns": [*self._turns, (self._turn_cards, self._turn_energy)]}  # ... nor how the fight has gone
            state["_fight_state"] = self._fight_state  # ... nor the enemies, the hand or the energy (a potion's offer is for this turn)
        handler = getattr(strategy, "map_screen" if kind == "map" else str(kind), None) if kind in SCREENS else None
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

    def _profile(self) -> dict | None:
        """The game's own records (None if they cannot be asked for: an old mod or a hiccup must not stop the bot)."""
        try:
            profile = self.client.profile()
            return profile if isinstance(profile, dict) else None
        except Exception as e:
            log.debug("could not read the profile: %s", e)
            return None

    @staticmethod
    def _epochs(profile: dict) -> dict:
        return {str(e.get("id")): str(e.get("state")) for e in profile.get("epochs") or [] if e.get("id")}

    def _refresh_ascensions(self) -> None:
        """Ask the game how far each character has climbed (used to pick who plays next)."""
        profile = self._profile()
        if profile is None:
            return
        if isinstance(profile.get("total_wins"), (int, float)):
            self._wins_seen = int(profile["total_wins"])
        try:
            self.mem.max_ascension = {str(c.get("id")): int(c.get("max_ascension") or 0) for c in profile.get("characters") or [] if c.get("id")}
            self.mem.epochs = self._epochs(profile)
        except (AttributeError, TypeError, ValueError) as e:
            log.debug("could not read the profile: %s", e)

    def _combat(self, state: dict) -> list[tuple[str, dict]]:
        battle, player = state.get("battle"), state.get("player") or {}
        if not battle or "hand" not in player:
            return []  # combat is starting or just ended
        if battle.get("turn") != "player" or not battle.get("is_play_phase"):
            return []
        self._fight_kind = str(state.get("state_type"))
        self._note_deck(state)
        key = (*strategy._where(state), battle.get("round"))
        stuck = {str(e.get("entity_id")) for e in battle.get("enemies") or [] if int(e.get("hp") or 0) > 0 and combat.power_amount(e, "IMPALED") > 0}
        self._dislodged, self._stuck = key == self._turn_key and (self._dislodged or bool(self._stuck - stuck)), stuck
        if key != self._turn_key:
            # (What each turn of the fight came to: the Knowledge Demon's offers are judged by it, see afflictions.py.)
            self._turns = [*self._turns, (self._turn_cards, self._turn_energy)] if key[:2] == self._turn_key[:2] else []
            self._turn_key, self._turn_plays, self._turn_cards, self._turn_names = key, 0, 0, []
            # Each enemy's damage over the fight: one intent says little, the average says who is doing the damage.
            if key[:2] != self._threat_fight:
                self._threat_fight, self._threat = key[:2], {}
            calm = combat.TAKEN.get(combat.read_stance(player.get("stance")), 1.0)  # (the first game's intents show Wrath's doubling)
            for e in battle.get("enemies") or []:
                seen = self._threat.setdefault(str(e.get("entity_id")), [0, 0, 0.0, 0])
                hit = sum(combat.intent_damage(i) for i in e.get("intents") or [])
                seen[0] += hit
                seen[1] += 1
                if hit:  # its blows, as they would be outside Wrath (what ending a turn in Wrath risks)
                    seen[2], seen[3] = seen[2] + hit / calm, seen[3] + 1
        self._turn_energy = int(player.get("energy") or 0)
        if key[:2] != self._facing_fight:
            self._facing_fight, self._facing = key[:2], None  # a new fight
        if self._turn_plays >= MAX_PLAYS_PER_TURN:
            return [("end_turn", {})]
        # A card just played lingers in the hand: sending it again (and again) left the game hanging at the end of turn.
        if self._hand_sig(state) == self._played_from and self.clock() - self._played_at < 2.0:
            return []
        # An enemy that revives stronger (Adaptable) leaves the field for the rest of the turn it is beaten in: it is coming back.
        standing = [e for e in battle.get("enemies") or [] if int(e.get("hp") or 0) > 0]
        if standing:
            self._revive_fight = key[:2] if any(combat.power_amount(e, "ADAPTABLE") for e in standing) else ()
        revive = not standing and self._revive_fight == key[:2]
        threat = {k: total / rounds for k, (total, rounds, _b, _n) in self._threat.items()}
        blow = {k: (blows / attacks, attacks / rounds) for k, (_t, rounds, blows, attacks) in self._threat.items() if attacks}  # (its blow, how often it attacks)
        costs = {str(c.get("name")): cards._cost(c.get("cost")) for c in self.mem.deck}  # (as the fight began: Void Form hides them)
        self._fight_state = {**state, "_plays_this_turn": self._turn_cards, "_played_names": tuple(self._turn_names), "_dislodged": self._dislodged, "_facing": self._facing, "_revive_pending": revive, "_threat": threat, "_blow": blow, "_costs": costs}
        wanted = combat.actions(self._fight_state)
        # Drawn cards (or Reboot's new hand) show up a moment late: look once more before ending a turn with energy left.
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
        """Remember the deck as a fight begins: later the piles also hold what the fight added (Wounds, Shivs, Souls)."""
        player, battle = state.get("player") or {}, state.get("battle") or {}
        fresh = int(battle.get("round") or 0) == 1 and not int(player.get("discard_pile_count") or 0) and not int(player.get("exhaust_pile_count") or 0)
        if fresh and int(player.get("max_energy") or 0) > 0:
            self.mem.max_energy = int(player["max_energy"])
        if not fresh:
            if self.mem.deck_size <= 0:
                self.mem.deck_size = len(player.get("hand") or []) + int(player.get("draw_pile_count") or 0)
            return
        keep = ("id", "name", "type", "cost", "star_cost", "description", "rarity", "is_upgraded")
        pile = player.get("draw_pile")
        whole = isinstance(pile, list) and len(pile) == int(player.get("draw_pile_count") or 0)
        cards = [{k: c.get(k) for k in keep if c.get(k) is not None} for c in (player.get("hand") or []) + (pile if whole else [])]
        if whole:
            self.mem.deck = [c for c in cards if str(c.get("type")) != "Status" and str(c.get("name")) not in NOT_OURS]
            self.mem.deck_size = len(self.mem.deck)
        else:  # a mod that does not show the piles: the count has to do
            self.mem.deck_size = len(cards) + int(player.get("draw_pile_count") or 0)

    def _after_success(self, state: dict, action: str, params: dict) -> None:
        kind = state.get("state_type")
        if action == "select_card":
            self.mem.selected.add(params.get("index"))
        elif action == "select_card_reward" and not (state.get("card_reward") or {}).get("in_combat"):  # (a card for this fight only is no pick)
            for c in (state.get("card_reward") or {}).get("cards") or []:
                if c.get("index") == params.get("card_index"):
                    self.mem.deck.append({k: v for k, v in c.items() if k not in ("index", "keywords")})
                    self.mem.deck_size += 1
                    self.mem.picks.append([(state.get("run") or {}).get("floor"), str(c.get("name"))])
        elif action in ("skip_card_reward", "discard_potion"):
            table, where = self.mem.card_skips if action == "skip_card_reward" else self.mem.potion_swaps, strategy._where(state)
            table[where] = table.get(where, 0) + 1
        elif action in ("play_card", "use_potion"):
            self._turn_plays += 1
            if action == "play_card":
                self._turn_cards += 1  # (apart from potions: under Sloth a potion and two cards are not three cards)
                self._played_from, self._played_at = self._hand_sig(state), self.clock()
                hand = (state.get("player") or {}).get("hand") or []
                self._turn_names += [str(c.get("name")) for c in hand if c.get("index") == params.get("card_index")]
            if params.get("target"):
                self._facing = params["target"]  # we turn towards whatever we aim at
        elif action == "menu_select" and kind == "menu":
            option = str(params.get("option"))
            if state.get("menu_screen") == "character_select" and option not in ("confirm", "embark"):
                self.mem.character_selected = True
            elif option == "abandon_run":
                log.warning("abandoning the current run")
                self.careful_until = 0.0
                self._crashed.clear()
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
            # "Done" must hold for a few clicks (between animations the Timeline looks finished), or nothing happen for long.
            self._timeline_idle = 0 if progressed else self._timeline_idle + (3 if result.get("done") else 1)
            if self._timeline_idle >= (9 if result.get("done") else 45):
                self._timeline_idle = 0
                self.mem.timeline_done = True
                self.mem.timeline_stuck = set(result.get("pending_epoch_ids") or [])  # cannot be revealed yet: do not come back for it
        self.sleep(0.6)  # let the reveal animation run before the next click

    def _note_powers(self, state: dict) -> None:
        """Log the text of each power, odd card, keyword and potion the first time it is met (to notice what the planner lacks)."""
        def once(seen: str, message: str, *args) -> None:
            if seen not in self._seen_powers:
                self._seen_powers.add(seen)
                log.info(message, *args)

        creatures = [state.get("player") or {}, *((state.get("battle") or {}).get("enemies") or [])]
        for c in creatures:
            for p in c.get("status") or []:
                once(str(p.get("id")), "power %s (on %s): %s", p.get("id"), c.get("name") or c.get("character") or "player", p.get("description"))
        for c in (state.get("player") or {}).get("hand") or []:
            if str(c.get("type")) in ("Status", "Curse"):
                once(f"card:{c.get('name')}", "card %s (%s, cost %s): %s", c.get("name"), c.get("type"), c.get("cost"), c.get("description"))
            for k in c.get("keywords") or []:
                once(f"kw:{k.get('name')}", "keyword %s (on %s): %s", k.get("name"), c.get("name"), k.get("description"))
        for p in (state.get("player") or {}).get("potions") or []:  # (the mod's wiki has no potions)
            once(f"potion:{p.get('name')}", "potion %s (%s): %s", p.get("name"), p.get("target_type"), p.get("description"))

    def _note_state(self, state: dict) -> None:
        kind = state.get("state_type")
        prev, self._prev_kind = self._prev_kind, kind
        ended = self._boss_watch.update(state, kind)
        if ended:
            log.info(ended)
        if self._boss_watch.brought:
            log.info(self._boss_watch.brought)
            self._boss_watch.brought = None
        self._fight_watch.update(state, kind)
        if kind in COMBAT_STATES:
            self._note_powers(state)
        if state.get("run"):
            if self._wins_seen is None and kind != "game_over" and not self._wins_asked:
                # Started mid-run, the bot has not read the win counter in the menu (else the run's end falls back on HP).
                self._wins_asked = True
                self._wins_seen = self._profile_wins()
            where = strategy._where(state)
            if where != self._last_where:
                self._last_where = where
                p = state.get("player") or {}
                log.info("act %s floor %s  %s  hp %s/%s  gold %s", *where, kind, p.get("hp"), p.get("max_hp"), p.get("gold"))
        if kind == "game_over":
            if not self._over_logged:
                self._over_logged = True
                # A death: straight out of a fight, or a boss fight the boss watch saw lost (a won last boss leaves its fight first).
                self._record_run(state, died=prev in COMBAT_STATES or bool(ended and " lost;" in ended))
        elif self._over_logged:
            self._over_logged = False
            self.mem.reset_run()
            self._crashed.clear()
            if self.tune:
                log.info("settings for the next run: %s", self.knobs.draw())
                self._knobs_whole_run = True
            self.mem.runs_finished = self.runs_finished

    def _profile_wins(self) -> int | None:
        """Total wins according to the game's own records (None if it cannot be asked)."""
        wins = (self._profile() or {}).get("total_wins")
        return int(wins) if isinstance(wins, (int, float)) else None

    def _note_epochs_earned(self) -> None:
        """Log the Timeline epochs the run just finished has earned, and remember where the Timeline stands."""
        profile = self._profile()
        if profile is None:
            return
        epochs = self._epochs(profile)
        earned = lambda table: {k for k, v in table.items() if v in strategy._EARNED}
        new = earned(epochs) - earned(self.mem.epochs)
        if new:
            log.info("epochs earned this run: %s", ", ".join(sorted(new)))
        self.mem.epochs = epochs

    def _record_run(self, state: dict, died: bool = False) -> None:
        p, run = state.get("player") or {}, state.get("run") or {}
        # The end screen looks the same for a win and a loss, and its HP is unreliable; the game's win counter goes up on a win.
        before, now = self._wins_seen, self._profile_wins()
        if before is not None and now == before and int(run.get("act") or 0) >= 3 and not died:
            # The game records a win a moment after the end screen comes up (not awaited after a death: no win is coming).
            for _ in range(8):
                self.sleep(0.5)
                now = self._profile_wins()
                if now is None or now > before:
                    break
        self._note_epochs_earned()
        if now is not None:
            self._wins_seen = now
        won = now > before if before is not None and now is not None else int(p.get("hp") or 0) > 0
        self.runs_finished += 1
        self.wins += won
        self.mem.last_result = "win" if won else "loss"
        self.mem.recent = (self.mem.recent + [str(p.get("character") or "")])[-RECENT_RUNS:]
        entry = {
            "time": time.strftime("%Y-%m-%dT%H:%M:%S%z"), "character": p.get("character"), "result": "win" if won else "loss",
            "act": run.get("act"), "floor": run.get("floor"), "ascension": run.get("ascension"), "hp": p.get("hp"), "max_hp": p.get("max_hp"),
            "gold": p.get("gold"), "relics": [r.get("id") for r in p.get("relics") or []], "deck_size": self.mem.deck_size,
            "deck": sorted(str(c.get("name")) for c in self.mem.deck), "picks": self.mem.picks,
        }
        who = str(entry["character"] or "")
        usual = self.card_stats.usual(who, 0)
        if self._knobs_whole_run and usual is not None and isinstance(entry["floor"], int):
            self.knobs.record(entry["floor"] + (learn.WIN_BONUS if won else 0) - usual)
        entry["knobs"] = dict(learn.KNOB) if self._knobs_whole_run else None
        self.card_stats.record(who, entry["floor"], bool(won), self.mem.picks)
        relics.note(entry)
        log.info("run %d over: %s on act %s floor %s (%d wins so far)", self.runs_finished, entry["result"], entry["act"], entry["floor"], self.wins)
        if self.log_dir:
            self.log_dir.mkdir(parents=True, exist_ok=True)
            with open(self.log_dir / "runs.jsonl", "a") as f:
                f.write(json.dumps(entry) + "\n")

    @staticmethod
    def _is_quiet(state: dict, action: str, params: dict) -> bool:
        """Actions that work without the state changing (ticking a grid card, picking the highlighted character): no settle wait."""
        if action == "select_card":
            return (state.get("card_select") or {}).get("screen_type") != "choose"
        if action == "menu_select" and state.get("menu_screen") == "character_select":
            return str(params.get("option")) not in ("confirm", "embark")
        if action == "menu_select" and state.get("menu_screen") == "timeline":
            return True  # the Timeline animates between clicks; the state says nothing about it
        return False

    @staticmethod
    def _is_choose(state: dict, action: str) -> bool:
        """Picking one card on a "choose 1 of 3" screen: ignored while the screen slides in, so re-sent after a short wait."""
        return action == "select_card" and (state.get("card_select") or {}).get("screen_type") == "choose"

    def _wait_for_change(self, fp: str, leave: str | None = None, quiet: bool = False, settle: float | None = None, slow: bool = False) -> bool:
        """Wait for the action to show; True if the state changed. With `leave`, until that screen is gone (the map answers during travel)."""
        deadline = self.clock() + (settle if settle is not None else self.travel_settle if leave else self.quiet_settle if quiet else self.settle)
        changed = False
        # Ask less often while a room loads (a state request walks the scene being rebuilt; rapid polling crashed the game).
        pause = min(self.room_poll if leave or slow else 0.03, self.poll)
        if self._careful():
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
        now = self.clock()
        if self.time_scale and now - self._scale_at > 3.0:
            self._scale_at = now
            self.client.act("set_time_scale", scale=self.time_scale)
        state = self.client.state()
        fp = fingerprint(state)
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
        todo = self.candidates(state)
        run = state.get("run") or {}
        suspect = lambda a, p: (run.get("act"), run.get("floor"), a, json.dumps(p, sort_keys=True)) in self._crashed
        if self._crashed and any(not suspect(a, p) for a, p in todo):
            todo = [(a, p) for a, p in todo if not suspect(a, p)]  # (when it is all there is to do, it is done again)
        for action, params in todo:
            key = (action, json.dumps(params, sort_keys=True))
            if self._strikes[key] >= MAX_STRIKES:
                continue
            buy = (run.get("act"), run.get("floor"), key[1]) if action == "shop_purchase" else None
            if buy and self._dead_buys[buy] >= MAX_STRIKES:
                continue
            if action != "menu_select" and state.get("state_type") not in COMBAT_STATES:
                self._last_sent = (run.get("act"), run.get("floor"), *key)
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
            quiet, choose = self._is_quiet(state, action, params), self._is_choose(state, action)
            # Leaving a room, buying and embarking rebuild the scene (rapid polling or clicks then crashed the game): pause, then poll less.
            fragile = action in ("proceed", "shop_purchase")
            embarking = action == "menu_select" and state.get("menu_screen") == "character_select" and str(params.get("option")) in ("confirm", "embark")
            if (fragile or embarking) and self.room_pause:
                self.sleep(self.room_pause)
            if self._careful():
                self.sleep(1.0)
            leave = "map" if action == "choose_map_node" else "menu" if embarking else None
            changed = self._wait_for_change(fp, leave=leave, quiet=quiet, settle=self.choose_settle if choose else None, slow=fragile)
            if not quiet and not choose and not changed:
                self._strikes[key] += 1  # accepted, but nothing happened
                if buy:
                    self._dead_buys[buy] += 1
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
