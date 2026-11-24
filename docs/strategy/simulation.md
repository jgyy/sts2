# Model runs: what 9,000 simulated runs say

_Results are filled in from the batches described below; see "Results"._

## Method

`sim/` (see the [README](../../README.md#model-runs)) plays whole runs with every decision made by the bot's own code:
the combat planner (`sts2bot/fight/combat.py`), the map, reward, shop, rest-site, event and selection handlers
(`sts2bot/run/strategy.py`), and the settings `learn.py` tunes. Only the game is modelled:

| Part | First game | Slay the Spire 2 |
|---|---|---|
| Cards | every card's text from `sts2bot/games/sts1/text/`, costs and rarities from `sim/content/sts1_cards.py` | 638 cards of v0.111.1 (Spire Codex), keywords included |
| Effects | what the planner's parser reads, plus handlers for ~150 cards whose text says more (conditions, piles, powers) | the same, and Powers read from their own text ("At the start of your turn, ...", "Whenever you play ...") |
| Enemies | ~60 monsters, elites and bosses written from the game's rules, Ascension 0 numbers | 115 monsters with their move state machines (spire-mentor) and A8/A9 numbers |
| Run | 3 acts (no act 4), Neow, map rules, unknown-room odds, rewards, shops, rest sites, boss relics | 3 acts, Ancients (heal and a relic choice), the same, boss rewards without relics |
| Simplified | events (a table of typical outcomes, chosen by the bot's event handler), a common subset of relics and potions, no Scry choices | the same; no enchantments, quests or the Fool |

Batches: **4,000 runs of the first game at Ascension 0** and **5,000 of Slay the Spire 2 at Ascension 5** (the
level the live bot plays), 1,000 per character. Each run draws its own value of every setting (`--vary`, the values
in `sim/__main__.GRID`: the bot's current choices and one on either side), so each setting's effect is the difference
between the runs that drew each of its values, every other setting averaged out. A run's **score** is the floor it
ended on, plus 15 for a win (the bot's own measure, `learn.WIN_BONUS`).

```sh
python3 -m sim run --game sts1 --character all --runs 1000 --vary --seed 1000 --out sts1.jsonl
python3 -m sim run --game sts2 --character all --runs 1000 --vary --seed 2000 --ascension 5 --out sts2.jsonl
python3 -m sim report sts1.jsonl sts2.jsonl
```

## How far to trust it

The model is easier than the live game in places (events never ambush, relics and potions are a common subset,
monsters do exactly what their tables say) and harder in others (no Scry choices, no Neow swap). So the absolute
win rates are the model's, not the game's; what transfers is the **comparison** between settings and between cards
under the bot's own play, and where in a run the bot loses. A finding is a candidate for `learn.CHOICES` (the live
bot then tests it on real runs), not a fact about the game.
