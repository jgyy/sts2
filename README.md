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

### Fights (`sts2bot/fight/`)

- `battle.py` reads a fight from the game state (enemies, intents, powers,
  cards that hurt while held); `potions.py` decides when to drink.
- `combat.py` scores every affordable set of cards in hand with a small
  simulation (damage, kills, block against enemy intents, lethal checks),
  plays the first card of the best line, then plans again. Lines that can only
  play out the same way (copies of one card, or nothing aimed at an enemy) are
  simulated once, so big hands full of Strikes and Defends stay quick.

### Between fights (`sts2bot/run/`)

- `strategy.py` handles the map, rewards, shops, rest sites and card
  selections (`decks.py`: what a card is worth in the deck that would take
  it); `events.py` rates event options. The map is planned a whole route at a
  time: every way to the boss is scored by what it gives (relics, cards,
  upgrades) against the health the bot would arrive with, because the run
  history shows the boss is rarely beaten from below 75% HP. Rest sites use the
  same sums to choose between resting and upgrading.
- `cards.py` reads card effects from the text on the card, so it keeps
  working when cards are added or rebalanced. For deck building it also has
  a hand-made rating of the cards met so far; cards it has no rating for are
  judged from their text.
- `crowd.py` rates the same cards from other players' runs: how often runs
  were won with each card and how often it was taken (tierspire.com, about
  half a million runs a character). A rating is a mix of the two tables; how
  much of it comes from this one (0.3 or 0.6) is one of the settings
  `learn.py` tunes. Refresh the tables after a game patch.

### Learning from its own runs

- `learn.py` corrects those ratings from the bot's own results: for every
  card taken from a card reward it records how much further that run got
  than runs of that character that left the same floor alive, and shifts
  the card's rating up or down accordingly (kept in `card_stats.json`). One
  run says little, so a card's own record only counts after some hundreds of
  picks. Until then a card stands where its kind does (`cards.kind`: attacks
  that cost nothing, cards that give energy, cards that cost energy and mostly
  draw), all characters together. `scripts/relearn.py` works the whole record
  out again from `runs.jsonl`.
- It also tunes a few of the bot's own settings the same way (how much HP is
  worth against damage, how eagerly to fight elites, when to upgrade instead
  of rest, how picky to be with card rewards, how much other players' results
  count, how hard a deck short of damage leans to attacks): every run plays
  with one value of each, chosen more and more often from the values whose
  runs went furthest (kept in `tuning.json`; `--no-tuning` turns this off).
- `relics.py` does the same for the relics the Ancients offer at the start
  of each act: `events.py` holds a rating for each, and every run that held
  one moves it by how much further than usual that run got from the floor
  the relic is taken on (up to 3 points, half of it after 25 runs). Nothing
  is kept for this: it is worked out from `runs.jsonl` when the bot starts.

### Keeping the game running

- `supervisor.py` starts Steam if it is not running, launches the game,
  restarts it when it stops answering or nothing changes for 5 minutes,
  abandons a run that hangs twice in the same place, and rebuilds the bridge
  mod after a game update (see below).
- After a crash the bot comes back to the room the save was made in and does
  something else there than what it had last asked for (another card, another
  option, another room, nothing bought); from the second crash in one room it
  also plays that room slowly. The run is only abandoned at the third crash
  with nothing left to do differently, or the fifth in any case.

The bot is heuristic, not an LLM: it costs nothing to run nonstop. It plays
simple, sensible Spire (kill when it can, block when it must) and will lose
plenty of runs.

Status (2026-11-21): plays the real game (v0.111) with all five characters.
30 wins in about 770 runs; every character has won on Ascension 5 (5 wins in
127 runs there) and plays on Ascension 6 now. Most runs end at the act 2 boss
or the last one.

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

With `--character auto` (the default) the bot works on the Timeline first and evens out the ascension levels second:

- While any character still has an epoch of its own to earn (beat acts 1, 2
  and 3, kill 15 elites and 15 bosses, win on Ascension 1), only those
  characters are played. The other epochs come with playing: about eighteen
  by score, and the rest by wins and by meeting the Ancients. The game has 57
  epochs that can be earned (v0.111; eleven more ids in its files are
  placeholders titled "TODO").
- Among those (or among everyone, once no such epoch is left) the character
  with the lowest ascension unlocked is played first, at its
  highest unlocked level (a win there unlocks the next one).
- Among characters on the same level, the bot draws one at random for each
  run. `--first CHARACTER` puts one character ahead of that while it has
  epochs of its own left.
- About eighteen more epochs unlock "by accumulating score". The game only
  adds a run's score in the end-of-run summary, so the bot clicks through it
  instead of going straight to the main menu (this needs the mod built by
  `scripts/install_mod.sh`).

## Logs

In `~/.local/state/sts2bot/` (change with `--log-dir`):

| File | Contents |
|---|---|
| `bot.log` | what the bot is doing (rotated at 5 MB) |
| `runs.jsonl` | one line per finished run: character, result, act, floor, HP, relics, the deck and the card rewards taken (with the floor of each) |
| `card_stats.json` | what the bot has learned about each card (see `learn.py`) |
| `tuning.json` | how runs went with each value of the settings it tunes |
| `fights.jsonl` | one line per fight: the deck, relics and potions it began with, the enemies, the rounds it took and the HP before and after (kept past the four hours `bot.log` reaches back; set aside at 64 MB) |

## Analysis scripts

`scripts/fightlog.py COMMAND` reads the logs fight by fight (the bot has to run
with `-v`). Run it without arguments, or read its docstring, for every option.

| Command | Shows |
|---|---|
| `bosses` | the record against each boss |
| `costs` | what each ordinary encounter costs in HP |
| `show 'YYYY-MM-DD HH:MM'` | one fight round by round, with the cards in hand and what was played |
| `knobs` | how runs went under each value of each tuned setting |
| `deaths` | the fight every run ended in |
| `pace` | rounds and damage a round in ordinary fights, by act |
| `rooms SETTING` | what ordinary fights cost under each value of one setting |
| `bosspace` | HP a boss lost a round (what separates won from lost), under each setting |
| `reads` | what the planner read on each card against what playing it did |
| `unplayed` | cards left in hand on turns that could have paid for them |
| `surprises` | turns that cost more HP than the plan expected, by enemy |
| `idle` | turns ended with energy and a playable card while HP was about to be lost |
| `powers` | powers no module of the bot names, with their rules text |
| `events` | every event met, with how often each option was taken |
| `pit`, `crabs`, `artifact` | the Insatiable, Crusher and Rocket, and Artifact fights round by round |
| `ancients` | the Ancients' relics: rating, how runs that held them went, where that moves the rating |
| `wiki` | saves the game's text for every discovered card and relic to `wiki.json` |

`scripts/decklog.py` reads `fights.jsonl`: what an ordinary fight costs against
how many cards of each sort the deck held, with the character taken out, then
the cards and relics themselves; `decklog.py bosses` does the same for boss
fights won and lost.

Model fights play a boss or room with the bot's real planner, so a change can
be tried on a few thousand fights in a minute. Most take `NAME=VALUE` to try a
planner constant or tuned setting.

| Script | Fight |
|---|---|
| `scripts/demonfight.py` | the Knowledge Demon, next to the best play there is (`regret`: the turns the planner gives fights away) |
| `scripts/crabfight.py` | Crusher and Rocket |
| `scripts/kinfight.py` | the Kin (act 1 boss) |
| `scripts/knightfight.py` | the act 3 knights, with and without the kill order in `battle.KNOWN_THREAT` (`flips`: fights two arms disagree on) |
| `scripts/roomfight.py` | an ordinary act 2 fight: HP lost next to the least any play could lose |
| `scripts/bosses/bowlfight.py` | Bowlbug Rock and Nectar |
| `scripts/bosses/subjectfight.py` | the Test Subject |
| `scripts/demon/*.py` | relics, afflictions and Powers at the Knowledge Demon, priced by perfect play |
| `scripts/audit/*.py` | cards played for nothing, and card text `cards.parse` misread |

## Options

| Option | Default | Meaning |
|---|---|---|
| `--character` | `auto` | `auto` (and `random`) plays a character with the lowest ascension, drawn at random among those on that level; or a character id, or `cycle` over unlocked characters. Every run uses the highest ascension that character has unlocked |
| `--runs N` | forever | Stop after N runs |
| `--first CHARACTER` | none | With `--character auto`: play this character until it has earned all of its own epochs, then carry on in the usual order |
| `--no-tuning` | off | Always play with the default settings instead of trying variations from run to run |
| `--no-launch` | off | Never start or kill the game |
| `--launch-cmd` | the game binary, else `steam -applaunch 2868840` | Command that starts the game |
| `--fast-mode` | `instant` | Game animation speed (`instant`, `fast`, `normal`), set through the mod each time the bot connects (not saved to your settings); `keep` leaves it alone |
| `--time-scale` | `3` | How many times faster the game's own clock runs (timers, pauses between enemy moves), set through the mod for the session; `1` leaves it alone |
| `--full-graphics` | off | Keep the game's normal window and graphics. By default the bot maximises the window and turns anti-aliasing and the frame cap off for the session (not saved to your settings) |
| `--small-window` | off | Shrink the game window to 960x540 instead of maximising it (a little faster on a slow machine) |
| `--stuck-timeout` | `300` | Seconds without change before the game is restarted |
| `--host`, `--port` | `127.0.0.1`, `15526` | Where the bridge mod listens |
| `-v` | off | Log every action |

## Development

```sh
python3 -m unittest discover -s tests -t .   # the tests
ruff check .                                  # lint (settings in pyproject.toml)
```

The tests run the bot against a small simulated game (`tests/fake_game.py`)
that speaks the same state and action format as the mod. They do not need the
real game. `tests/test_size.py` also keeps the code base in shape:

- no script is longer than 1000 lines (split it);
- no directory holds more than 10 files (move the rest into a new directory);
- no table in `sts2bot/` lists the same key twice.

## Strategy notes

`docs/strategy/` holds community strategy collected for the bot to consider (general play, and each character),
with a list of what the bot already does and what is worth trying next. The notes themselves are not wired into
the code; the card and relic win rates in `sts2bot/run/crowd.py` and `sts2bot/run/events.py` are.

## Uninstall

```sh
scripts/install_mod.sh --remove
```
