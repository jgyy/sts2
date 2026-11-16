# sts2

A bot that plays Slay the Spire 2 around the clock: it starts a run, plays it
to the end, returns to the menu and starts the next one. If the game crashes or
freezes, it restarts the game and carries on.

## How it works

```
Slay the Spire 2  <-- STS2_MCP mod (HTTP on localhost:15526) -->  sts2bot (Python)
```

- **Bridge mod.** [STS2_MCP](https://github.com/Gennadiyev/STS2MCP) (MIT) runs
  inside the game and exposes the game state and actions on localhost.
  `scripts/install_mod.sh` builds it from source at a pinned commit.
- **Bot** (`sts2bot/`, standard library only). Polls the state, decides, acts.
  - `combat.py` scores every affordable set of cards in hand with a small
    simulation (damage, kills, block against enemy intents, lethal checks),
    plays the first card of the best line, then plans again.
  - `strategy.py` handles the map, rewards, shops, rest sites, events and card
    selections.
  - `cards.py` reads card effects from the text on the card, so there is no
    card database to keep up to date.
  - `supervisor.py` launches the game through Steam, restarts it when it stops
    answering or nothing changes for 5 minutes, and abandons a run that hangs
    twice in the same place.

The bot is heuristic, not an LLM: it costs nothing to run nonstop. It plays
simple, sensible Spire (kill when it can, block when it must) and will lose
plenty of runs.

Status: tested against a simulated game only (see Tests). It has not yet been
run against the real game, so expect to tune it after the first live session.

## Setup

Needs Linux, Python 3.11+, git, the .NET SDK, and the game installed via Steam.

1. Build and install the bridge mod:

   ```sh
   scripts/install_mod.sh
   ```

   Set `STS2_GAME_DIR` if the game is not in the default Steam library.

2. Start the game once, accept the mod consent dialog (or enable mods under
   Settings → Mods), and restart the game. Then check the mod answers:

   ```sh
   curl -s http://localhost:15526/
   ```

   With mods enabled the game uses a separate modded save profile, so the bot
   does not touch your normal progress. A fresh profile starts with only the
   first character unlocked.

3. Run the bot:

   ```sh
   python3 -m sts2bot                 # plays forever, launches the game if needed
   python3 -m sts2bot --runs 1 -v     # one run, logging every action
   python3 -m sts2bot --no-launch     # only play; never start or kill the game
   python3 -m sts2bot --character cycle
   ```

Stop it with Ctrl+C. The game keeps running; the run in progress is saved by
the game and the bot resumes it next time.

## Running 24/7

`systemd/sts2bot.service` is a user service that restarts the bot if it exits
and blocks idle sleep while it plays. Install instructions are at the top of
that file. Steam must be running and logged in.

## Logs

In `~/.local/state/sts2bot/` (change with `--log-dir`):

- `bot.log`: what the bot is doing (rotated at 5 MB).
- `runs.jsonl`: one line per finished run with character, result, act, floor,
  HP, relics and deck size.

## Options

| Option | Default | Meaning |
|---|---|---|
| `--character` | `IRONCLAD` | Character id, or `cycle` / `random` over unlocked characters |
| `--runs N` | forever | Stop after N runs |
| `--no-launch` | off | Never start or kill the game |
| `--launch-cmd` | the game binary, else `steam -applaunch 2868840` | Command that starts the game |
| `--stuck-timeout` | `300` | Seconds without change before the game is restarted |
| `--host`, `--port` | `127.0.0.1`, `15526` | Where the bridge mod listens |
| `-v` | off | Log every action |

## Tests

```sh
python3 -m unittest discover -s tests -t .
```

The tests run the bot against a small simulated game (`tests/fake_game.py`)
that speaks the same state and action format as the mod. They do not need the
real game.

## Uninstall

```sh
scripts/install_mod.sh --remove
```
