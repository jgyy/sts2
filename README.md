# sts2

A bot that plays **Slay the Spire 2** around the clock: it starts a run, plays it to the end, starts the next one,
and restarts the game when it crashes or freezes. The same bot plays the first **Slay the Spire** (`--game sts1`,
through CommunicationMod) or both games at once (`--game both`).

It is heuristic, not an LLM, so it costs nothing to run nonstop. Status (2026-11-21, live): plays v0.111 with all
five characters; 30 wins in about 770 runs, every character has won on Ascension 5; most runs end at the act 2 boss
or the last one.

## How it works

```
Slay the Spire 2  <-- STS2_MCP mod (HTTP, localhost:15526) -->  sts2bot
Slay the Spire    <-- CommunicationMod (stdin/stdout) --> relay.py <-- HTTP, localhost:15527 -->  sts2bot --game sts1
```

| Part | What it does |
|---|---|
| [STS2_MCP](https://github.com/Gennadiyev/STS2MCP) (MIT) | runs inside the game and serves its state and actions; `scripts/install_mod.sh` builds it at a pinned commit with the bot's patch |
| `sts2bot/fight/` | the combat planner: `battle.py` reads the fight (enemies, intents, powers), `combat.py` scores every affordable line of cards with a small simulation (damage, kills, Block against intents, lethal checks), plays its first card and plans again |
| `sts2bot/run/` | everything between fights: whole-route map planning against the HP the bot would reach the boss with, rewards, shops, rest sites, events, potions; `decks.py` rates a card for the deck that would take it |
| `sts2bot/cards.py` | reads what a card does from its text (so new and rebalanced cards work), plus hand-made ratings |
| `sts2bot/run/crowd.py` | other players' win and pick rates per card (tierspire.com), mixed into the ratings |
| `sts2bot/learn.py` | learns from its own runs: shifts card ratings by how far runs that took them got, and tunes a dozen settings by Thompson sampling (`tuning.json`; `--no-tuning` turns it off) |
| `sts2bot/supervisor.py` | starts Steam and the game, restarts it when it hangs, rebuilds the mod after a game update, and after a crash does something different in the room it crashed in |
| `sts2bot/games/sts1/` | the first game: translates CommunicationMod's state into the bot's and the bot's actions back, holds every card's text (CommunicationMod sends none), the Watcher's stances and the act 4 keys |
| `sim/` | model runs of both games played by the bot's own code (see [Model runs](#model-runs)) |

## Setup (Slay the Spire 2)

Needs Linux, Python 3.11+, git, the .NET SDK and the game from Steam.

```sh
scripts/install_mod.sh              # build and install the bridge mod (STS2_GAME_DIR if the game is elsewhere)
# start the game once, accept the mod dialog (or Settings -> Mods), restart it, then check:
curl -s http://localhost:15526/
python3 -m sts2bot                  # plays forever, launching the game if needed
python3 -m sts2bot --runs 1 -v      # one run, logging every action
```

With mods enabled the game uses a separate save profile (a fresh one has only the first character). Stop with
Ctrl+C; the game saves the run and the bot resumes it next time.

**Game updates** often break the mod ("Loaded 0 mods WITH ERRORS"). The bot rebuilds it before launching when the
game files changed, and when the game's log says the mod failed; if the build fails it logs the compiler errors and
retries later (5 minutes, then up to hourly). Fixing such a break means updating `scripts/mod_patches/sts2bot.patch`.

**Unlocks.** Epochs only take effect once revealed on the Timeline, so the bot reveals them from the main menu. With
`--character auto` (the default) it plays characters that still have epochs of their own to earn, then the one with
the lowest ascension (rotating among equals), always at its highest unlocked ascension. It clicks through the
end-of-run summary because that is where score epochs are earned.

## Slay the Spire 1

```sh
python3 -m sts2bot sts1-setup                 # once: point CommunicationMod at the bot's relay
python3 -m sts2bot --game sts1                # launches ModTheSpire with BaseMod and CommunicationMod
python3 -m sts2bot --game sts1 --character WATCHER --runs 1 -v
```

Subscribe to ModTheSpire, BaseMod and CommunicationMod on the Workshop first (or set `STS1_GAME_DIR`, `STS1_MTS_JAR`
or `--launch-cmd`). Characters are `IRONCLAD`, `THE_SILENT`, `DEFECT`, `WATCHER`; each plays at its highest unlocked
ascension (read from the save's `preferences/`, `STS1_PREFS_DIR` for another).

- The bot's own ModTheSpire mod turns Fast Mode on, runs the clock `--sts1-time-scale` times faster (default 8) and
  adds what CommunicationMod leaves out: live card numbers, the Watcher's stance, the act 4 keys.
- The card ratings and crowd tables are Slay the Spire 2's, so here cards are judged from their text and corrected
  by `learn.py`. Logs and learning live in `~/.local/state/sts2bot/sts1/`.
- The planner knows stances and Mantra; it does not plan Scry. Once the first three characters have each won, it goes
  for the three keys and act 4 (`games/sts1/keys.py`).
- CommunicationMod cannot continue a saved run: after a crash the bot starts a new one.

## Both games at once

```sh
python3 -m sts2bot --game both [-v] [--runs N]
```

One bot process per game; every other option is passed to both (`--port`, `--log-dir` and `--launch-cmd` are refused
because each game needs its own). Each has its own port, logs and supervisor, and a bot that exits with an error is
restarted after 15 seconds. Terminal lines name their game. The first game is only launched while CommunicationMod
is set up for the relay (`sts1-setup` fixes it).

## Running 24/7

`systemd/sts2bot.service` is a user service that restarts the bot and blocks idle sleep (install steps are at the top
of the file; Steam must log in without a password). Add `--game both` to `ExecStart` to run both games.

```sh
python3 -m sts2bot service status|start|stop|enable|disable
```

`stop` lasts until the next login, `disable` until you `enable` it again. Killing the process is not enough: the
service restarts it.

## Options

| Option | Default | Meaning |
|---|---|---|
| `--game` | `sts2` | `sts2`, `sts1` or `both` |
| `--character` | `auto` | `auto`/`random` (lowest ascension first), a character id, or `cycle` |
| `--first CHARACTER` | - | with `auto`: this character until its own epochs are earned |
| `--runs N` | forever | stop after N runs (per game with `both`) |
| `--no-tuning` | off | always play the default settings |
| `--no-launch` | off | never start or kill the game |
| `--launch-cmd` | the game binary, else `steam -applaunch 2868840` | how to start the game |
| `--fast-mode` | `instant` | animation speed (`instant`, `fast`, `normal`, `keep`) |
| `--time-scale` / `--sts1-time-scale` | `3` / `8` | how much faster the game clock runs (`1`: leave it) |
| `--full-graphics` | off | keep the normal window and graphics (default: maximised, no AA, uncapped fps) |
| `--small-window` / `--windowed` | off | a 960x540 window / a maximised window instead of full screen |
| `--stuck-timeout` | `300` | seconds without change before the game is restarted |
| `--host`, `--port` | `127.0.0.1`, `15526` (`15527` for sts1) | where the mod or relay listens |
| `--log-dir` | `~/.local/state/sts2bot` (`sts1/` below it) | logs and what is learned |
| `-v` | off | log every action (the analysis scripts need it) |

## Logs and analysis

`bot.log` (rotated at 5 MB), `runs.jsonl` (one line per run: character, result, floor, relics, deck, picks, settings),
`fights.jsonl` (every fight: deck, enemies, rounds, HP before and after), `card_stats.json`, `tuning.json`.

| Script | Shows |
|---|---|
| `scripts/fightlog.py COMMAND` | read `bot.log` fight by fight: `bosses`, `costs`, `show 'YYYY-MM-DD HH:MM'`, `knobs`, `deaths`, `pace`, `rooms SETTING`, `bosspace`, `reads`, `unplayed`, `surprises`, `idle`, `powers`, `events`, `pit`, `crabs`, `artifact`, `ancients`, `wiki` |
| `scripts/decklog.py [bosses]` | what a fight costs against what the deck holds (`fights.jsonl`) |
| `scripts/relearn.py` | rebuild `card_stats.json` from `runs.jsonl` |
| `scripts/demonfight.py`, `crabfight.py`, `kinfight.py`, `knightfight.py`, `roomfight.py`, `bosses/*.py` | model fights: one boss or room played by the real planner thousands of times (most take `NAME=VALUE` to try a constant); `demonfight.py` also solves the Knowledge Demon exactly |
| `scripts/demon/*.py`, `scripts/audit/*.py` | relic and affliction prices at the Demon; cards played for nothing and text the parser misreads |

## Model runs

`sim/` plays whole runs of either game with every decision made by the bot's own code: the combat planner, the
map, reward, shop, rest-site, event and selection handlers, and the tuned settings. The engine knows cards by their
text (the first game's from the bot's tables, Slay the Spire 2's from `sim/content/sts2_data.json`, extracted from
the game by Spire Codex), monsters by their move tables, and a common subset of relics and potions.

```sh
python3 -m sim run --game sts1 --character all --runs 1000 --vary --out runs.jsonl   # --vary: random settings per run
python3 -m sim run --game sts2 --character NECROBINDER --runs 200 --ascension 5
python3 -m sim report runs.jsonl                                                     # by character, setting, boss, card
python3 -m sim trace                                                                 # one hash per character
```

`trace` hashes every decision of seeded runs: a change that should not alter behaviour must leave the hashes as they
were. Results and limits: [docs/strategy/simulation.md](docs/strategy/simulation.md).

## Development

```sh
python3 -m unittest discover -s tests -t .   # against fake games: the bot loop, the StS1 relay, the planner
ruff check .
PYTHONHASHSEED=0 python3 -m sim trace         # unchanged hashes = unchanged decisions
```

`tests/test_size.py` keeps files under 1000 lines, directories under 10 files and dict literals free of repeated keys.
Strategy notes for both games are in [docs/strategy/](docs/strategy/).

## Uninstall

```sh
scripts/install_mod.sh --remove
```
