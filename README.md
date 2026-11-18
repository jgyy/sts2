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
    selections. The map is planned a whole route at a time: every way to the
    boss is scored by what it gives (relics, cards, upgrades) against the
    health the bot would arrive with, because the run history shows the boss
    is rarely beaten from below 75% HP. Rest sites use the same sums to
    choose between resting and upgrading.
  - `cards.py` reads card effects from the text on the card, so it keeps
    working when cards are added or rebalanced. For deck building it also has
    a hand-made rating of the cards met so far; cards it has no rating for are
    judged from their text.
  - `learn.py` corrects those ratings from the bot's own results: for every
    card taken from a card reward it records how much further that run got
    than the character usually gets from that floor, and shifts the card's
    rating up or down accordingly (kept in `card_stats.json`).
    It also tunes a few of the bot's own settings the same way (how much HP
    is worth against damage, how eagerly to fight elites, when to upgrade
    instead of rest, how picky to be with card rewards): every run plays with
    one value of each, chosen more and more often from the values whose runs
    went furthest (kept in `tuning.json`; `--no-tuning` turns this off).
  - `supervisor.py` starts Steam if it is not running, launches the game,
    restarts it when it stops answering or
    nothing changes for 5 minutes, abandons a run that hangs twice in the same
    place, and rebuilds the bridge mod after a game update (see below).

The bot is heuristic, not an LLM: it costs nothing to run nonstop. It plays
simple, sensible Spire (kill when it can, block when it must) and will lose
plenty of runs.

Status: plays the real game (v0.111). With the Ironclad it regularly reaches
the last boss and has beaten it once in about 80 runs. Other characters are
played from the text on their cards only, and do worse.

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
that file. The bot starts Steam itself when it is not running; Steam must
be able to log in without asking for a password.

Turn the service on or off with these commands (they call `systemctl --user`):

```sh
python3 -m sts2bot service status    # is it running?
python3 -m sts2bot service stop      # stop it now; it starts again at the next login
python3 -m sts2bot service disable   # stop it now and do not start it at login
python3 -m sts2bot service start     # start it now
python3 -m sts2bot service enable    # start it now and at every login
```

Do not just kill the bot's process: the service restarts it after 15 seconds.
Stopping the service does not close the game; close the game from the game
or Steam if you want it gone too.

## Game updates

Steam updates the game on its own, and an update often breaks the bridge mod:
the game then starts with "Loaded 0 mods WITH ERRORS" and the bot has nothing
to talk to. The bot handles this itself:

- Before launching the game it checks whether the game files changed since the
  mod was last built, and rebuilds the mod if so.
- If the game's log says the mod failed to load, it stops the game, rebuilds
  the mod and starts the game again.

A rebuild is `scripts/install_mod.sh`, so it needs git and the .NET SDK. When
the update changed code the mod uses, the build fails; the bot then logs the
compiler errors and tries again later (after 5 minutes, then less often, up to
hourly). Fixing that needs a change to `scripts/mod_patches/sts2bot.patch`,
which holds the bot's additions to the mod and its fixes for newer game
versions.

## Unlocks and the Timeline

The game awards unlocks ("epochs") as runs are played: ascension levels, new
characters, cards, relics. None of them take effect until they are revealed on
the Timeline screen, and until then the game may not offer Singleplayer at
all. The bot does this itself: when unlocks are waiting and no run is in
progress, it opens the Timeline from the main menu, reveals each one, confirms
the unlock screens and goes back. (This needs the mod built by
`scripts/install_mod.sh`; the stock mod refuses to reveal epochs.)

With `--character auto` (the default) the bot puts the Timeline first and
ascension levels second:

- Each character has its own epochs, and the game says what each takes:
  beating acts 1, 2 and 3, killing 15 elites and 15 bosses, and a win on
  Ascension 1. The bot plays whoever is most likely to earn one next: a
  character still short of 15 bosses always gets runs (those add up win or
  lose), and the epochs that need a win go to whoever has been getting
  furthest, with the others taking turns when it keeps coming back empty.
  `--first CHARACTER` puts one character ahead of that order.
- While epochs are left, runs are played at the easiest ascension that counts
  (0, or 1 for a character that is after its Ascension 1 epoch).
- About eighteen more epochs unlock "by accumulating score". The game only
  adds a run's score in the end-of-run summary, so the bot clicks through it
  instead of going straight to the main menu (this needs the mod built by
  `scripts/install_mod.sh`).
- With every epoch in, it plays the character with the lowest ascension at the
  highest level that character has unlocked, so all of them climb evenly.

## Logs

In `~/.local/state/sts2bot/` (change with `--log-dir`):

- `bot.log`: what the bot is doing (rotated at 5 MB).
- `runs.jsonl`: one line per finished run with character, result, act, floor,
  HP, relics, the deck and the card rewards taken (with the floor of each).
- `card_stats.json`: what the bot has learned about each card (see `learn.py`).
- `tuning.json`: how runs went with each value of the settings it tunes.

## Options

| Option | Default | Meaning |
|---|---|---|
| `--character` | `auto` | `auto` plays towards unlocking every epoch, then evens out the ascension levels (see Unlocks and the Timeline); or a character id, or `cycle` / `random` over unlocked characters. Every run uses the highest ascension that character has unlocked |
| `--runs N` | forever | Stop after N runs |
| `--first CHARACTER` | none | With `--character auto`: play this character until it has earned all of its own epochs, then carry on in the usual order |
| `--no-tuning` | off | Always play with the default settings instead of trying variations from run to run |
| `--no-launch` | off | Never start or kill the game |
| `--launch-cmd` | the game binary, else `steam -applaunch 2868840` | Command that starts the game |
| `--fast-mode` | `instant` | Game animation speed (`instant`, `fast`, `normal`), set through the mod each time the bot connects (not saved to your settings); `keep` leaves it alone |
| `--full-graphics` | off | Keep the game's normal window and graphics. By default the bot shrinks the window to 960x540 and turns anti-aliasing off for the session (not saved to your settings), which makes it faster |
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
