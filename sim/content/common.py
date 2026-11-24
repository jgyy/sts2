"""What both games' content modules share: the map, the events, the shop and the card offers."""

from __future__ import annotations

from sts2bot.run import strategy

from ..cards import Card

ROWS, COLS, PATHS = 15, 7, 6


def make_map(rng, weights: dict, *, elite_from: int = 5, treasure_row: int = 8, rows: int = ROWS) -> dict:
    """An act's map as the bot reads it: (col, row) -> {col, row, type, children}, rows 1..rows, the boss above them.

    Six paths climb from the bottom, one column left, right or straight at a time; rooms are drawn by weight,
    the first row is fights, `treasure_row` a chest and the last row rest sites; no elite or rest site before
    `elite_from`, none twice in a row on a path, and no rest site just below the last row."""
    edges: dict[tuple, set] = {}
    starts = []
    for p in range(PATHS):
        col = rng.randrange(COLS)
        while p < 2 and col in starts:
            col = rng.randrange(COLS)
        starts.append(col)
        for row in range(1, rows):
            nxt = min(COLS - 1, max(0, col + rng.choice((-1, 0, 1))))
            edges.setdefault((col, row), set()).add((nxt, row + 1))
            col = nxt
        edges.setdefault((col, rows), set())
    boss = (3, rows + 1)
    for k in [k for k in edges if k[1] == rows]:
        edges[k] = {boss}
    parents: dict[tuple, list] = {}
    for k, kids in edges.items():
        for c in kids:
            parents.setdefault(c, []).append(k)
    types: dict[tuple, str] = {}
    for row in range(1, rows + 1):
        for k in sorted(k for k in edges if k[1] == row):
            if row == 1:
                t = "Monster"
            elif row == treasure_row:
                t = "Treasure"
            elif row == rows:
                t = "RestSite"
            else:
                banned = {types.get(p) for p in parents.get(k, [])} & {"Elite", "Shop", "RestSite"}
                if row < elite_from:
                    banned |= {"Elite", "RestSite"}
                if row == rows - 1:
                    banned.add("RestSite")
                pool = {t: w for t, w in weights.items() if t not in banned}
                t = _draw(rng, pool)
            types[k] = t
    nodes = {k: {"col": k[0], "row": k[1], "type": types[k], "children": [list(c) for c in sorted(edges[k])]} for k in edges}
    nodes[boss] = {"col": boss[0], "row": boss[1], "type": "Boss", "children": []}
    return nodes


def _draw(rng, weights: dict) -> str:
    r = rng.random() * sum(weights.values())
    for k, w in weights.items():
        r -= w
        if r <= 0:
            return k
    return next(iter(weights))


def rarity(rng, kind: str, odds: dict) -> str:
    r = rng.random()
    for name, p in odds[kind]:
        if r < p:
            return name
        r -= p
    return odds[kind][-1][0]


def offer(run, pool: list, n: int, rare_odds: dict, kind: str, upgrade_chance: float = 0.0) -> list[Card]:
    """n different cards of a character's pool for a card reward."""
    rng = run.rng
    out: list[Card] = []
    for _ in range(n):
        want = "Rare" if kind == "boss" else rarity(rng, kind, rare_odds)
        choices = [d for d in pool if d.rarity == want and d.key not in {c.key for c in out}]
        if not choices:
            choices = [d for d in pool if d.key not in {c.key for c in out}]
        d = rng.choice(choices)
        out.append(Card(d, up=want != "Rare" and rng.random() < upgrade_chance))
    return out


def free_card(f, choices: list) -> None:
    """One of these cards at random into the hand, free to play this turn."""
    if choices:
        c = Card(f.rng.choice(choices))
        c.cost_turn = 0
        f.add_card(c)


def potion_card(f, pot) -> None:
    """Attack, Skill and Power Potions: a card of that type of the character's; the others: a colorless card."""
    kind = {"ATTACK_POTION": "Attack", "SKILL_POTION": "Skill", "POWER_POTION": "Power"}.get(pot.key)
    free_card(f, [d for d in (f.game.pool(f.run.character) if kind else f.game.colorless()) if kind is None or d.type == kind])


def upgrade_random(run, test, n: int = 2) -> None:
    """Upgrade n cards of the deck at random among those `test` lets through."""
    fresh = [c for c in run.deck if not c.up and test(c)]
    for c in run.rng.sample(fresh, min(n, len(fresh))):
        c.up = True


# ------------------------------------------------------------------ events
#
# An event here is a set of options written the way the games word them, so that the bot's event handler
# (sts2bot.run.events) can price them, each with what it does. Gold, HP and the odds scale a little by act.


def _transform(run):
    c = run.choose_from_deck("transform", "Choose a card to Transform.")
    if c is not None:
        run.remove_card(c)
        run.log["removals"] -= 1
        run.deck.append(Card(run.rng.choice(run.game.pool(run.character))))


EVENTS = [
    # (weight, [(title, description, effect(run, act))...]): a "Leave" option is always added
    (3, [("Take", "Lose {hp} HP. Obtain a random Relic.", lambda r, a: (r.lose_hp(r.max_hp // 8 + 2 * a), r.gain_relic(r.game.random_relic(r))))]),
    (3, [("Gold", "Gain {gold} Gold.", lambda r, a: setattr(r, "gold", r.gold + 50 + 25 * a))]),
    (3, [("Purify", "Remove a card from your Deck.", lambda r, a: r.remove_one()), ("Upgrade", "Upgrade a card.", lambda r, a: r.upgrade_one())]),
    (2, [("Trade", "Lose {hp} HP. Gain {gold2} Gold.", lambda r, a: (r.lose_hp(r.max_hp // 10 + a), setattr(r, "gold", r.gold + 75 + 25 * a)))]),
    (2, [("Pray", "Become Cursed - Regret. Obtain a random Relic.", lambda r, a: (r.deck.append(r.game.random_curse(r)), r.gain_relic(r.game.random_relic(r))))]),
    (2, [("Rest", "Heal {heal} HP.", lambda r, a: setattr(r, "hp", min(r.max_hp, r.hp + r.max_hp // 3))),
         ("Grow", "Gain 5 Max HP.", lambda r, a: (setattr(r, "max_hp", r.max_hp + 5), setattr(r, "hp", r.hp + 5)))]),
    (3, [("Learn", "Choose a card to add to your Deck.", lambda r, a: r.card_reward(r.game.card_choices(r, "monster")))]),
    (1, [("Study", "Lose {mhp} Max HP. Choose a Rare card to add to your Deck.", lambda r, a: (setattr(r, "max_hp", r.max_hp - 5), setattr(r, "hp", min(r.hp, r.max_hp)), r.card_reward(r.game.card_choices(r, "boss"))))]),
    (2, [("Change", "Transform a card.", lambda r, a: _transform(r))]),
    (1, [("Buy", "Pay {cost} Gold. Obtain a random Relic.", lambda r, a: (setattr(r, "gold", r.gold - 100 - 25 * a), r.gain_relic(r.game.random_relic(r))))]),
    (2, [("Smith", "Upgrade 2 random cards.", lambda r, a: upgrade_random(r, lambda c: c.type not in ("Status", "Curse")))]),
    (2, []),  # nothing worth having (the event is passed by)
]


def event(run) -> None:
    rng, act = run.rng, run.act
    _w, opts = EVENTS[_draw(rng, {i: w for i, (w, _o) in enumerate(EVENTS)})]
    nums = {"hp": run.max_hp // 8 + 2 * act, "gold": 50 + 25 * act, "gold2": 75 + 25 * act, "heal": run.max_hp // 3, "mhp": 5, "cost": 100 + 25 * act}
    options = [(t, d.format(**nums), fn) for t, d, fn in opts]
    if any("Pay" in d for _t, d, _f in options) and run.gold < nums["cost"]:
        options = []
    options.append(("Leave", "Leave.", None))
    # (A lone option is the event's way out, "is_proceed" in the bot's words; with others beside it, it is one of them.)
    state = run.screen("event", event={"event_id": "SIM_EVENT", "event_name": "Event", "in_dialogue": False,
                                       "options": [{"index": i, "title": t, "description": d, "is_locked": False, "is_proceed": len(options) == 1}
                                                   for i, (t, d, _f) in enumerate(options)]})
    choice = len(options) - 1
    for action, params in strategy.event(state, run.mem):
        if action == "choose_event_option":
            choice = params["index"]
            break
    title, _d, fn = options[choice]
    run.digest.append(f"V{title[:2]}")
    if fn is not None:
        fn(run, act)


# ------------------------------------------------------------------ shops


def shop_stock(run, prices: dict, colorless_mult: float = 1.2) -> list[dict]:
    g = run.game
    rng = run.rng
    items = []
    if run.has("MEAL_TICKET"):
        run.hp = min(run.max_hp, run.hp + 15)

    def add(cat, price, **fields):
        items.append({"index": len(items), "category": cat, "price": int(price), "cost": int(price), **fields})

    own = g.pool(run.character)
    sale = rng.randrange(5)
    for n, kind in enumerate(("Attack", "Attack", "Skill", "Skill", "Power")):
        c = Card(rng.choice([d for d in own if d.type == kind and d.key not in {i.get("card_id") for i in items}]))
        add("card", prices[c.d.rarity] * rng.uniform(0.9, 1.1) * (0.5 if n == sale else 1.0), _card=c, **{"card_" + k: v for k, v in c.view().items()})
    for rarity in ("Uncommon", "Rare"):
        c = Card(rng.choice([d for d in g.colorless() if d.rarity == rarity]))
        add("card", prices[rarity] * colorless_mult * rng.uniform(0.9, 1.1), _card=c, **{"card_" + k: v for k, v in c.view().items()})
    for _ in range(3):
        rid = g.random_relic(run)
        if rid in {i.get("relic_id") for i in items}:
            continue
        tier = 150 if rid in g.COMMON else 250 if rid in g.UNCOMMON else 300
        add("relic", tier * rng.uniform(0.95, 1.05), _relic=rid, relic_id=rid, relic_name=rid.replace("_", " ").title(), description=g.relic_text(rid))
    for _ in range(3):
        p = g.random_potion(run)
        add("potion", rng.choice((50, 75, 100)) * rng.uniform(0.95, 1.05), _potion=p, potion_id=p.key, potion_name=p.name, description=p.text)
    add("card_removal", (50 if run.has("SMILING_MASK") else 75 + 25 * run.counters.get("removals", 0)))
    return items
