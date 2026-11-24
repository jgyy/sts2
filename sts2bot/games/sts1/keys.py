"""The three keys that open the first game's act 4 (the Heart), and when the bot goes for each.

Act 4 is there once the Ironclad, the Silent and the Defect have each won a run (CLASS_WIN in the player's prefs; the
bot's mod sets them, as the victory screen's button would: speedmod.py). From then on a run can carry the Ruby Key (a
rest site's "Recall", in place of resting or smithing), the Emerald Key (from the elite that burns on the map, one an
act until it is taken) and the Sapphire Key (in a chest, in place of its relic); with all three, beating the act 3 boss
leads on to act 4: a rest site, a shop, the Spire Shield and Spear, and the Corrupt Heart. The user's goal since
2026-11-24: unlock act 4 and play it.

The mod adds `keys` ({"ruby", "emerald", "sapphire": bool}) and `final_act_available` to CommunicationMod's game state
and `emerald_key` to the burning elite's map node; screens.py asks here which key to go for and shows the bot only that
choice (the planner and the route model know nothing of keys).
"""

from __future__ import annotations

KEYS = ("ruby", "emerald", "sapphire")
RUBY_HP = 0.70  # an act 3 rest site is given up for the Ruby Key from this share of Max HP up ...
RUBY_HP_LAST = 0.55  # ... and the last one before the boss from this share (no other rest site comes after it)
BOSS_ROW_FLOOR = 15  # floor % 17 of the row of rest sites before every boss (floors 15, 32, 49)


def missing(game: dict) -> set[str]:
    """The keys this run still needs (none while act 4 is shut: then a key is worth nothing)."""
    if not game.get("final_act_available"):
        return set()
    held = game.get("keys") or {}
    return {k for k in KEYS if not held.get(k)}


def wants_ruby(game: dict) -> bool:
    """Recall at this rest site. Only in act 3, where it is the last chance, and only with HP to spare: a key is no use to
    a run that dies at the act 3 boss for want of the heal."""
    if "ruby" not in missing(game) or game.get("act") != 3:
        return False
    hp, top = float(game.get("current_hp") or 0), float(game.get("max_hp") or 1)
    last = int(game.get("floor") or 0) % 17 == BOSS_ROW_FLOOR
    return hp >= RUBY_HP * top or (last and hp >= RUBY_HP_LAST * top)


def wants_sapphire(game: dict) -> bool:
    """The key over the chest's relic. Act 3's middle row is a treasure room on every road, so its chest is a sure
    chance; earlier chests keep their relics."""
    return "sapphire" in missing(game) and game.get("act") == 3


def wants_emerald(game: dict) -> bool:
    """The burning elite is marked for the route model (strategy.path_value: an "EmeraldElite" is an elite worth more)."""
    return "emerald" in missing(game)
