"""The keys to the first game's act 4 (open once three characters have won: speedmod.py sets CLASS_WIN), and when the bot
goes for each: Ruby (a rest site's Recall), Emerald (the burning elite), Sapphire (a chest, instead of its relic). The mod
adds them to CommunicationMod's state; screens.py shows the bot only the choice wanted."""

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
    """Recall at this rest site: only in act 3 (the last chance), and only with HP to spare for the act 3 boss."""
    if "ruby" not in missing(game) or game.get("act") != 3:
        return False
    hp, top = float(game.get("current_hp") or 0), float(game.get("max_hp") or 1)
    last = int(game.get("floor") or 0) % 17 == BOSS_ROW_FLOOR
    return hp >= RUBY_HP * top or (last and hp >= RUBY_HP_LAST * top)


def wants_sapphire(game: dict) -> bool:
    """The key over the chest's relic, in act 3 only (its middle row is a treasure room on every road: a sure chance)."""
    return "sapphire" in missing(game) and game.get("act") == 3


def wants_emerald(game: dict) -> bool:
    """The burning elite is marked for the route model (strategy.path_value: an "EmeraldElite" is an elite worth more)."""
    return "emerald" in missing(game)
