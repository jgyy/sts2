"""Model runs: whole runs of either game played by the bot's own decisions, for testing strategies at scale.

The engine here is not the game. It knows the cards by their text (the same text the bot's planner reads), the
enemies by a move table per monster, and enough of the map, rewards, shops, rest sites, relics and potions to make a
run that looks like the real one. Every choice in it is made by the bot's own code: the combat planner
(sts2bot.fight.combat), the map, reward, shop, rest-site and selection handlers (sts2bot.run.strategy) and the
settings it tunes (sts2bot.learn). So a change to the bot, or a value of one of its settings, can be tried on a few
thousand runs per character in an hour instead of a week of live play.

    python3 -m sim run --game sts1 --character IRONCLAD --runs 1000 --out runs.jsonl
    python3 -m sim report runs.jsonl
    python3 -m sim trace --game sts1 --runs 3      # a digest of every decision: unchanged by a refactor that keeps behaviour

What it leaves out (see docs/strategy/simulation.md): events are a draw from a table of outcomes, not choices;
relics and potions are a common subset; enemy move tables are written from the games' rules, not read from them.
"""
