"""Out-of-combat decisions: map, rewards, shop, rest sites, events, selections.

Every handler returns an ordered list of candidate actions. The main loop
takes the first one that the game accepts, so a handler can list fallbacks.
"""

from __future__ import annotations

import random
import re
from dataclasses import dataclass, field

from .cards import deck_value, is_basic, is_junk

Action = tuple[str, dict]

SKIP_THRESHOLD = 1.6  # card rewards scoring below this are skipped
BIG_DECK = 24


@dataclass
class Memory:
    """The few things the game state does not tell us."""

    deck_size: int = 10
    card_skips: dict = field(default_factory=dict)  # (act, floor) -> skipped card rewards
    select_key: str = ""
    selected: set = field(default_factory=set)
    runs_finished: int = 0
    character_selected: bool = False

    def reset_run(self) -> None:
        self.deck_size = 10
        self.card_skips.clear()
        self.select_key = ""
        self.selected.clear()
        self.character_selected = False


def _where(state: dict) -> tuple:
    run = state.get("run") or {}
    return (run.get("act"), run.get("floor"))


def _hp_ratio(state: dict) -> float:
    p = state.get("player") or {}
    return int(p.get("hp") or 0) / max(1, int(p.get("max_hp") or 1))


def _option_names(options) -> list[str]:
    """Menu options come either as strings or as {name, enabled} objects."""
    out = []
    for o in options or []:
        if isinstance(o, str):
            out.append(o)
        elif isinstance(o, dict) and o.get("enabled", True) and o.get("name"):
            out.append(str(o["name"]))
    return out


# --------------------------------------------------------------------- menu


def menu(state: dict, mem: Memory, character: str) -> list[Action]:
    screen = state.get("menu_screen") or "main"
    names = _option_names(state.get("options"))
    lower = {n.lower(): n for n in names}

    def pick(*wanted: str) -> list[Action]:
        return [("menu_select", {"option": lower[w]}) for w in wanted if w in lower]

    if screen == "main":
        return pick("continue", "singleplayer")
    if screen == "singleplayer":
        return pick("standard")
    if screen == "character_select":
        chars = [c for c in state.get("characters") or [] if not c.get("locked")]
        ids = [str(c.get("id")) for c in chars]
        want = choose_character(character, ids, mem.runs_finished)
        select = [("menu_select", {"option": want})] if want else []
        embark = pick("confirm", "embark")
        return embark + select if mem.character_selected else select + embark
    if screen == "tutorial_prompt":
        return pick("no")
    if screen == "tutorial":
        return pick("advance", "proceed")
    if screen == "popup":
        return pick("yes", "confirm", "ok", "continue") or [("menu_select", {"option": n}) for n in names]
    # Timeline, settings, compendium, multiplayer screens: get out.
    return pick("back", "advance")


def choose_character(setting: str, unlocked: list[str], runs_finished: int) -> str | None:
    """`setting` is a character id, or "cycle" / "random" over the unlocked ones."""
    if not unlocked:
        return None
    by_upper = {u.upper(): u for u in unlocked}
    s = setting.strip().upper()
    if s in by_upper:
        return by_upper[s]
    if s == "CYCLE":
        return unlocked[runs_finished % len(unlocked)]
    if s == "RANDOM":
        return random.Random(runs_finished).choice(unlocked)
    return unlocked[0]


# ---------------------------------------------------------------------- map

_ROOM_BASE = {
    "Monster": 1.5,
    "Unknown": 2.2,
    "Treasure": 4.0,
    "RestSite": 3.0,
    "Shop": 1.0,
    "Elite": 2.5,
    "Boss": 0.0,
    "Ancient": 3.0,
}


def _room_value(kind: str, hp: float, gold: int) -> float:
    v = _ROOM_BASE.get(kind, 1.0)
    if kind == "Elite":
        # Elites are worth it only from a strong position; a starter deck
        # walking into one at ~80% HP came out with 18 HP.
        v = 3.0 if hp >= 0.9 else (0.5 if hp >= 0.7 else -6.0)
    elif kind == "Monster":
        v = 1.5 if hp >= 0.45 else -1.0
    elif kind == "RestSite":
        v = 3.0 + (1.0 - hp) * 5.0
    elif kind == "Shop":
        v = 3.0 if gold >= 150 else (1.5 if gold >= 80 else 0.3)
    return v


def map_screen(state: dict, mem: Memory) -> list[Action]:
    m = state.get("map") or {}
    options = m.get("next_options") or []
    if not options:
        return []
    hp = _hp_ratio(state)
    gold = int((state.get("player") or {}).get("gold") or 0)
    nodes = {(n.get("col"), n.get("row")): n for n in m.get("nodes") or []}
    memo: dict = {}

    def best_from(key) -> float:
        """Best total value of a path starting at this node."""
        if key in memo:
            return memo[key]
        memo[key] = 0.0  # cycle guard
        node = nodes.get(key)
        if node is None:
            return 0.0
        kids = [tuple(c) for c in node.get("children") or []]
        # HP is only known now; discount far-away rooms so near choices dominate.
        ahead = 0.9 * max((best_from(k) for k in kids), default=0.0)
        memo[key] = _room_value(str(node.get("type")), hp, gold) + ahead
        return memo[key]

    def value(opt: dict) -> float:
        key = (opt.get("col"), opt.get("row"))
        if key in nodes:
            return best_from(key)
        kids = opt.get("leads_to") or []
        ahead = max((_room_value(str(k.get("type")), hp, gold) for k in kids), default=0.0)
        return _room_value(str(opt.get("type")), hp, gold) + 0.9 * ahead

    ranked = sorted(options, key=value, reverse=True)
    return [("choose_map_node", {"index": int(o.get("index", 0))}) for o in ranked]


# ------------------------------------------------------------------ rewards


def rewards(state: dict, mem: Memory) -> list[Action]:
    r = state.get("rewards") or {}
    player = state.get("player") or {}
    belt_full = len(player.get("potions") or []) >= int(player.get("max_potion_slots") or 3)
    skips_left = mem.card_skips.get(_where(state), 0)
    out: list[Action] = []
    # Highest index first so earlier indices stay valid.
    for item in sorted(r.get("items") or [], key=lambda i: -int(i.get("index", 0))):
        kind = str(item.get("type"))
        if kind == "potion" and belt_full:
            continue
        if kind == "card" and skips_left > 0:
            skips_left -= 1
            continue
        out.append(("claim_reward", {"index": int(item.get("index", 0))}))
    out.append(("proceed", {}))
    return out


def card_reward(state: dict, mem: Memory) -> list[Action]:
    cr = state.get("card_reward") or {}
    cards = cr.get("cards") or []
    ranked = sorted(cards, key=deck_value, reverse=True)
    out: list[Action] = []
    threshold = SKIP_THRESHOLD + (1.5 if mem.deck_size >= BIG_DECK else 0.0)
    take = [("select_card_reward", {"card_index": int(c.get("index", 0))}) for c in ranked]
    if ranked and deck_value(ranked[0]) >= threshold:
        out += take
        if cr.get("can_skip", True):
            out.append(("skip_card_reward", {}))
    else:
        out.append(("skip_card_reward", {}))
        out += take
    return out


# ---------------------------------------------------------------- rest site


def rest_site(state: dict, mem: Memory) -> list[Action]:
    rs = state.get("rest_site") or {}
    opts = [o for o in rs.get("options") or [] if o.get("is_enabled", True)]
    hp = _hp_ratio(state)

    def rank(o: dict) -> float:
        oid = str(o.get("id") or o.get("name") or "").lower()
        if "rest" in oid or "heal" in oid:
            return 10.0 if hp < 0.6 else 1.0
        if "smith" in oid or "upgrade" in oid:
            return 5.0
        return 3.0

    out = [("choose_rest_option", {"index": int(o.get("index", 0))}) for o in sorted(opts, key=rank, reverse=True)]
    if rs.get("can_proceed"):
        out.insert(0, ("proceed", {}))
    else:
        out.append(("proceed", {}))
    return out


# --------------------------------------------------------------------- shop


def _shop_card(item: dict) -> dict:
    return {
        "id": item.get("card_id"),
        "name": item.get("card_name"),
        "type": item.get("card_type"),
        "cost": item.get("card_cost"),
        "star_cost": item.get("card_star_cost"),
        "rarity": item.get("card_rarity"),
        "description": item.get("card_description"),
    }


def shop(state: dict, mem: Memory, key: str = "shop") -> list[Action]:
    s = state.get(key) or {}
    if key == "fake_merchant":
        s = s.get("shop") or {}
    if s.get("error"):
        return []
    player = state.get("player") or {}
    gold = int(player.get("gold") or 0)
    belt_full = len(player.get("potions") or []) >= int(player.get("max_potion_slots") or 3)

    scored: list[tuple[float, dict]] = []
    for item in s.get("items") or []:
        price = int(item.get("price", item.get("cost", 0)) or 0)
        if not item.get("is_stocked", True) or not item.get("can_afford", price <= gold):
            continue
        cat = str(item.get("category"))
        if cat == "card_removal":
            want = 9.0
        elif cat == "relic":
            want = 8.0 - price / 200.0
        elif cat == "card":
            v = deck_value(_shop_card(item))
            want = v if v >= 3.0 and mem.deck_size < BIG_DECK else -1.0
        elif cat == "potion":
            want = 2.0 if not belt_full and gold - price >= 100 else -1.0
        else:
            want = -1.0
        if want > 0:
            scored.append((want, item))
    scored.sort(key=lambda t: -t[0])
    out: list[Action] = [("shop_purchase", {"index": int(i.get("index", 0))}) for _, i in scored]
    out.append(("proceed", {}))
    return out


# ------------------------------------------------------------------- events

_GOOD = [
    (r"\brelic\b", 6.0),
    (r"\bremove\b", 4.0),
    (r"\bupgrade\b", 4.0),
    (r"\bmax hp\b", 3.0),
    (r"\bheal\b", 2.5),
    (r"\bgold\b", 2.0),
    (r"\btransform\b", 2.0),
    (r"\bpotion\b", 1.5),
    (r"\bcard\b", 1.0),
    (r"\benchant\b", 3.0),
]
_BAD = [(r"\bcurse\b", 6.0), (r"\blose all\b", 6.0), (r"\bfight\b", 2.0), (r"\blose \d+ max hp\b", 4.0)]


def _event_option_value(opt: dict, hp: int, max_hp: int) -> float:
    text = f"{opt.get('title') or ''}. {opt.get('description') or ''}".lower()
    score = 0.5
    if opt.get("relic_name"):
        score += 6.0
    for pat, w in _GOOD:
        if re.search(pat, text) and not re.search(r"lose[^.]*" + pat, text):
            score += w
    for pat, w in _BAD:
        if re.search(pat, text):
            score -= w
    m = re.search(r"(?:lose|take)\s+(\d+)\s+(?:hp|damage)", text)
    if m:
        loss = int(m.group(1))
        if loss >= hp:
            score -= 1000.0
        else:
            score -= 12.0 * loss / max(1, hp)
    if re.search(r"heal", text) and hp >= 0.9 * max_hp:
        score -= 2.0
    return score


def event(state: dict, mem: Memory) -> list[Action]:
    ev = state.get("event") or {}
    if ev.get("in_dialogue"):
        return [("advance_dialogue", {})]
    opts = [o for o in ev.get("options") or [] if not o.get("is_locked")]
    if not opts:
        return [("advance_dialogue", {}), ("proceed", {})]
    player = state.get("player") or {}
    hp, max_hp = int(player.get("hp") or 1), int(player.get("max_hp") or 1)
    proceeds = [o for o in opts if o.get("is_proceed")]
    rest = [o for o in opts if not o.get("is_proceed")]
    rest.sort(key=lambda o: _event_option_value(o, hp, max_hp), reverse=True)
    ordered = proceeds + rest if proceeds else rest
    return [("choose_event_option", {"index": int(o.get("index", 0))}) for o in ordered]


# --------------------------------------------------------------- selections

_WORST_FIRST = re.compile(r"remove|transform|exhaust|discard|lose|purge|sacrifice|destroy", re.I)


def _removal_rank(card: dict) -> float:
    """Lower = get rid of it first."""
    if is_junk(card):
        return -100.0
    v = deck_value(card)
    if is_basic(card):
        v -= 5.0
    return v


def _pick_order(cards: list[dict], prompt: str, screen_type: str = "") -> list[dict]:
    if _WORST_FIRST.search(prompt) or screen_type == "transform":
        return sorted(cards, key=_removal_rank)
    if "upgrade" in prompt.lower() or screen_type == "upgrade":
        fresh = [c for c in cards if not c.get("is_upgraded")]
        return sorted(fresh or cards, key=deck_value, reverse=True)
    return sorted(cards, key=deck_value, reverse=True)


def card_select(state: dict, mem: Memory) -> list[Action]:
    cs = state.get("card_select") or {}
    prompt = str(cs.get("prompt") or "")
    kind = str(cs.get("screen_type") or "")
    cards = cs.get("cards") or []
    key = f"{_where(state)}|{kind}|{prompt}|{len(cards)}"
    if key != mem.select_key:
        mem.select_key, mem.selected = key, set()

    ordered = _pick_order(cards, prompt, kind)
    if kind == "choose":
        # One click picks and closes the screen; there is nothing to confirm
        # and nothing to remember. A click that lands while the screen is
        # still opening is ignored by the game, so always offer it again.
        out = [("select_card", {"index": int(c.get("index", 0))}) for c in ordered]
        if cs.get("can_skip") or cs.get("can_cancel"):
            out.append(("cancel_selection", {}))
        return out

    confirmable = cs.get("preview_showing") or cs.get("can_confirm")
    indices = [int(c.get("index", 0)) for c in ordered]
    if not confirmable and all(i in mem.selected for i in indices):
        mem.selected = set()  # our clicks did not register; start over
    out: list[Action] = []
    if confirmable:
        out.append(("confirm_selection", {}))
    out += [("select_card", {"index": i}) for i in indices if i not in mem.selected]
    if cs.get("can_skip") or cs.get("can_cancel"):
        out.append(("cancel_selection", {}))
    out.append(("confirm_selection", {}))
    return out


def hand_select(state: dict, mem: Memory) -> list[Action]:
    hs = state.get("hand_select") or {}
    prompt = str(hs.get("prompt") or "")
    kind = "upgrade" if hs.get("mode") == "upgrade_select" else ""
    out: list[Action] = []
    if hs.get("can_confirm"):
        out.append(("combat_confirm_selection", {}))
    for c in _pick_order(hs.get("cards") or [], prompt, kind):
        out.append(("combat_select_card", {"card_index": int(c.get("index", 0))}))
    out.append(("combat_confirm_selection", {}))
    return out


def bundle_select(state: dict, mem: Memory) -> list[Action]:
    bs = state.get("bundle_select") or {}
    out: list[Action] = []
    if bs.get("preview_showing") or bs.get("can_confirm"):
        out.append(("confirm_bundle_selection", {}))
    bundles = sorted(
        bs.get("bundles") or [],
        key=lambda b: sum(deck_value(c) for c in b.get("cards") or []),
        reverse=True,
    )
    out += [("select_bundle", {"index": int(b.get("index", 0))}) for b in bundles]
    out.append(("confirm_bundle_selection", {}))
    return out


def relic_select(state: dict, mem: Memory) -> list[Action]:
    rs = state.get("relic_select") or {}
    out: list[Action] = [("select_relic", {"index": int(r.get("index", 0))}) for r in rs.get("relics") or []]
    if rs.get("can_skip"):
        out.append(("skip_relic_selection", {}))
    return out


def treasure(state: dict, mem: Memory) -> list[Action]:
    t = state.get("treasure") or {}
    relics = t.get("relics") or []
    if not relics and not t.get("can_proceed"):
        return []  # chest still opening
    out: list[Action] = [("claim_treasure_relic", {"index": int(r.get("index", 0))}) for r in relics]
    out.append(("proceed", {}))
    return out


def crystal_sphere(state: dict, mem: Memory) -> list[Action]:
    cs = state.get("crystal_sphere") or {}
    if cs.get("can_proceed"):
        return [("crystal_sphere_proceed", {})]
    cells = cs.get("clickable_cells") or []
    w, h = int(cs.get("grid_width") or 9), int(cs.get("grid_height") or 11)
    # Start from the middle: big tools reveal the most there.
    cells = sorted(cells, key=lambda c: abs(c.get("x", 0) - w // 2) + abs(c.get("y", 0) - h // 2))
    out: list[Action] = [("crystal_sphere_click_cell", {"x": c.get("x"), "y": c.get("y")}) for c in cells[:6]]
    out.append(("crystal_sphere_proceed", {}))
    return out


def game_over(state: dict, mem: Memory) -> list[Action]:
    return [("menu_select", {"option": "main_menu"})]


def overlay(state: dict, mem: Memory) -> list[Action]:
    return [("proceed", {}), ("confirm_selection", {}), ("cancel_selection", {})]
