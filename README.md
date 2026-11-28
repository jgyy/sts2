# sts2

A bot that plays **Slay the Spire 2** around the clock: it starts a run, plays it to the end, starts the next one,
and restarts the game when it crashes or freezes. The same bot plays the first **Slay the Spire** (`--game sts1`,
through CommunicationMod) or both games at once (`--game both`).

It is heuristic, not an LLM, so it costs nothing to run nonstop. Status (2026-11-21, live): plays v0.111 with all
five characters; 30 wins in about 770 runs, every character has won on Ascension 5; most runs end at the act 2 boss
or the last one.

## Quick start: the commands you actually need

Run everything from this folder (`cd ~/Documents/repo/sts2`).

| I want to... | Type this |
|---|---|
| **build the bot** (once, and again after any code change) | `cargo build --release` |
| **play BOTH games at once** | `target/release/sts2bot --game both -v` |
| play only Slay the Spire 2 | `target/release/sts2bot -v` |
| play only the first Slay the Spire | `target/release/sts2bot --game sts1 -v` |
| stop the bot | press `Ctrl+C` in its terminal |
| run both games in the background instead (keeps going after you close the terminal) | `target/release/sts2bot service start` |
| stop the background bot | `target/release/sts2bot service stop` |
| see what the background bot is doing | `journalctl --user -u sts2bot -f` |
| run the tests | `cargo test --release` |

Things that trip people up:

- **"Both games" does nothing for the first game?** Run `target/release/sts2bot sts1-setup` once. It tells
  CommunicationMod to start this bot. Run it again if the bot's folder moves.
- **`-v`** only makes the log more detailed (every action). Leave it on: the analysis scripts need it.
- **Do not start it twice.** If the background service is running, do not also start the bot in a terminal
  (check with `target/release/sts2bot service status`).
- The bot starts Steam and the games by itself. You do not need to open them first.
- Logs are in `~/.local/state/sts2bot/bot.log` (Slay the Spire 2) and `~/.local/state/sts2bot/sts1/bot.log`.

First time on a new machine? Follow [Setup](#setup-slay-the-spire-2) and [Slay the Spire 1](#slay-the-spire-1) below first.

## How it works

```
Slay the Spire 2  <-- STS2_MCP mod (HTTP, localhost:15526) -->  sts2bot
Slay the Spire    <-- CommunicationMod (stdin/stdout) --> sts2bot relay <-- HTTP, localhost:15527 -->  sts2bot --game sts1
```

| Part | What it does |
|---|---|
| [STS2_MCP](https://github.com/Gennadiyev/STS2MCP) (MIT) | runs inside the game and serves its state and actions; `scripts/install_mod.sh` builds it at a pinned commit with the bot's patch |
| `src/fight/` | the combat planner: `battle.rs` reads the fight (enemies, intents, powers), `combat.rs` scores every affordable line of cards with a small simulation (damage, kills, Block against intents, lethal checks), plays its first card and plans again |
| `src/run/` | everything between fights: whole-route map planning against the HP the bot would reach the boss with, rewards, shops, rest sites, events, potions; `decks.rs` rates a card for the deck that would take it |
| `src/cards.rs` | reads what a card does from its text (so new and rebalanced cards work), plus hand-made ratings |
| `src/run/crowd.rs` | other players' win and pick rates per card (tierspire.com), mixed into the ratings |
| `src/learn.rs` | learns from its own runs: shifts card ratings by how far runs that took them got, and tunes a dozen settings by Thompson sampling (`tuning.json`; `--no-tuning` turns it off); one of them weighs in what the model runs say of each card (`run/model_cards.json`) |
| `src/supervisor.rs` | starts Steam and the game, restarts it when it hangs, rebuilds the mod after a game update, and after a crash does something different in the room it crashed in |
| `src/games/sts1/` | the first game: translates CommunicationMod's state into the bot's and the bot's actions back, holds every card's text (CommunicationMod sends none), the Watcher's stances and the act 4 keys |
| `src/sim/` | model runs of both games played by the bot's own code (see [Model runs](#model-runs)) |

## Setup (Slay the Spire 2)

Needs Linux, Rust (cargo, 1.80 or newer), git, the .NET SDK and the game from Steam.

```sh
cargo build --release               # builds target/release/sts2bot (the bot), sim (model runs) and scripts (analysis)
scripts/install_mod.sh              # build and install the bridge mod (STS2_GAME_DIR if the game is elsewhere)
# start the game once, accept the mod dialog (or Settings -> Mods), restart it, then check:
curl -s http://localhost:15526/
target/release/sts2bot              # plays forever, launching the game if needed
target/release/sts2bot --runs 1 -v  # one run, logging every action
```

The commands below are written `sts2bot`, `sim` and `scripts`: the binaries in `target/release/` (put that directory
on your `PATH`, or run them as `cargo run --release --bin sim -- ...`).

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
sts2bot sts1-setup                 # once: point CommunicationMod at the bot's relay
sts2bot --game sts1                # launches ModTheSpire with BaseMod and CommunicationMod
sts2bot --game sts1 --character WATCHER --runs 1 -v
```

Subscribe to ModTheSpire, BaseMod and CommunicationMod on the Workshop first (or set `STS1_GAME_DIR`, `STS1_MTS_JAR`
or `--launch-cmd`). Characters are `IRONCLAD`, `THE_SILENT`, `DEFECT`, `WATCHER`; each plays at its highest unlocked
ascension (read from the save's `preferences/`, `STS1_PREFS_DIR` for another).

- The bot's own ModTheSpire mod turns Fast Mode on, runs the clock `--sts1-time-scale` times faster (default 8) and
  adds what CommunicationMod leaves out: live card numbers, the Watcher's stance, the act 4 keys.
- The card ratings and crowd tables are Slay the Spire 2's, so here cards are judged from their text and corrected
  by `learn.rs`. Logs and learning live in `~/.local/state/sts2bot/sts1/`.
- The planner knows stances and Mantra; it does not plan Scry. Once the first three characters have each won, it goes
  for the three keys and act 4 (`games/sts1/keys.rs`).
- CommunicationMod cannot continue a saved run: after a crash the bot starts a new one.

## Both games at once

```sh
sts2bot --game both [-v] [--runs N]
```

One bot process per game; every other option is passed to both (`--port`, `--log-dir` and `--launch-cmd` are refused
because each game needs its own). Each has its own port, logs and supervisor, and a bot that exits with an error is
restarted after 15 seconds. Terminal lines name their game. The first game is only launched while CommunicationMod
is set up for the relay (`sts1-setup` fixes it).

## Running 24/7

`systemd/sts2bot.service` is a user service that restarts the bot and blocks idle sleep (install steps are at the top
of the file; Steam must log in without a password). Add `--game both` to `ExecStart` to run both games.

```sh
sts2bot service status|start|stop|enable|disable
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
| `--no-tuning` | off | always play the default settings (each game's best in the model runs, `learn.GAME_DEFAULTS`) |
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

`bot.log` (rotated at 20 MB), `runs.jsonl` (one line per run: character, result, floor, relics, deck, picks, settings),
`fights.jsonl` (every fight: deck, enemies, rounds, HP before and after), `card_stats.json`, `tuning.json`.

| Script | Shows |
|---|---|
| `scripts fightlog COMMAND` | read `bot.log` fight by fight: `bosses`, `costs`, `show 'YYYY-MM-DD HH:MM'`, `knobs`, `deaths`, `pace`, `rooms SETTING`, `bosspace`, `reads`, `unplayed`, `surprises`, `idle`, `powers`, `events`, `pit`, `crabs`, `artifact`, `ancients`, `wiki` |
| `scripts decklog [bosses]` | what a fight costs against what the deck holds (`fights.jsonl`) |
| `scripts relearn` | rebuild `card_stats.json` from `runs.jsonl` |
| `scripts demonfight`, `crabfight`, `kinfight`, `knightfight`, `roomfight`, `bosses/NAME` (`aeonfight`, `bowlfight`, `giantfight`, `matriarchfight`, `queenfight`, `sadfight`, `subjectfight`) | model fights: one boss or room played by the real planner thousands of times (most take `NAME=VALUE` to try a constant); `demonfight` also solves the Knowledge Demon exactly |
| `scripts demon/*`, `scripts audit/*` | relic and affliction prices at the Demon; cards played for nothing and text the parser misreads; what the model runs do with each card's text (`audit/simcards`) |

## Model runs

`src/sim/` plays whole runs of either game with every decision made by the bot's own code: the combat planner, the
map, reward, shop, rest-site, event and selection handlers, and the tuned settings. The engine knows cards by their
text (the first game's from the bot's tables, Slay the Spire 2's from `src/sim/content/sts2_data.json`, extracted from
the game by Spire Codex), monsters by their move tables, and a common subset of relics and potions.

```sh
sim run --game sts1 --character all --runs 1000 --vary --out runs.jsonl   # --vary: random settings per run
sim run --game sts2 --character NECROBINDER --runs 200 --ascension 5
sim report runs.jsonl                                                     # by character, setting, boss, card
sim trace                                                                 # one hash per character
sim prior sts2.jsonl sts1.jsonl --out src/run/model_cards.json         # the card table the bot reads
sim prior runs.jsonl --onto src/run/model_cards.json --out new.json     # ... corrected by runs that played it
sim run --game sts2 --runs 400 --vary --only model_table,caution --alt-cards new.json   # vary only these; two tables (more: a,b)
sim run --game sts1 --runs 400 --vary --only caution,belt_spare --grid caution=1,1.35      # caution drawn from these values only
sim run --game sts2 --runs 300 --ascension 9 --set ERUPTION=0                              # a name in capitals: a constant of the planner
```

`trace` hashes every decision of seeded runs: a change that should not alter behaviour must leave the hashes as they
were. Results and limits: [docs/strategy/simulation.md](docs/strategy/simulation.md).

## Development

```sh
cargo test                    # against fake games: the bot loop, the StS1 relay, the planner, the model runs
cargo test --release -- --ignored   # the long ones: hundreds more seeded model runs
sim trace                     # unchanged hashes = unchanged decisions
```

`tests/all/test_size.rs` keeps source files under 2000 lines, directories under 20 files and `tests/golden/` under
100 MB in all (53 MB on 2026-11-28).

`tests/golden/` holds recorded answers (card readings, planner choices, whole bot sessions, the decisions of seeded
model runs): the tests replay them and compare exactly. After a change that is meant to alter decisions, write the
recorded runs again with `cargo test --release -- --ignored rewrite_the_golden_runs` and read the diff.
Strategy notes for both games are in [docs/strategy/](docs/strategy/).

## Uninstall

```sh
scripts/install_mod.sh --remove
```
