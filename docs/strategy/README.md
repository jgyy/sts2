# Strategy notes

- [sts2.md](sts2.md): Slay the Spire 2 (v0.111.1): the run, ascension, principles, every elite and boss, the six
  characters, recent patches.
- [sts1.md](sts1.md): the first Slay the Spire: the run, ascension 1-20, elites and bosses, the four characters, how the
  bot plays it.
- [simulation.md](simulation.md): 1,000+ model runs per character of both games, and what they say about the bot's
  settings and card choices.

The notes are advice, not code: the bot's own numbers (quoted in `sts2bot/` comments, measured on live runs) win where
they disagree, and the model runs are where a rule is tried before it goes live.

## What the bot already does

| Advice | Where |
|---|---|
| No elite on a starter deck; elites from a healthy position | `strategy.path_value`: act 1 elites before row 8 cost 32 HP; a penalty below 75% HP; act 3 elites priced as run-enders |
| Plan the route back from the boss | `strategy.best_onward` scores every whole route by rewards against the HP it arrives with |
| Rest before the boss | `strategy.rest_site`: heal below 85% (act 1) / 95% (acts 2-3) with the boss next |
| Front-loaded damage early, then Block and scaling | `decks.deck_fit`: attacks lean while the deck deals under 5.5-6 an energy, Block and Weak cards when short |
| Removal first in a shop, then relics | `strategy.shop` (removal priced against the shelf from A6) |
| Keep potions for the boss, never sit on a full belt | `potions.potion_actions`, `potions.SAVE_FLOORS` |
| Discard Sly cards first | `strategy.hand_select` |
| Focus the enemy doing the most for its HP | `battle._pick_target`, `battle.KNOWN_THREAT` (Kin Followers, Magi Knight, Rocket, Bowlbug Nectar first) |
| Race enemies that gain Strength | `Enemy.grows` raises what damage is worth |
| Crab claws together, Decimillipede segments together | `battle._rage_blocks`, `Enemy.reattach` |
| Doom, Osty, Stars, orbs, Shivs, stances | the planner simulates each (`combat.simulate`) |
| Ratings from other players' runs and its own | `crowd.py` (tierspire.com), `learn.CardStats`, mixed by the `crowd` setting |
| ... and from the model runs | `learn.model_shift` over `run/model_cards.json` (`python3 -m sim prior`, corrected by the runs that played it with `--onto`), weighed by the `model_lean` setting |
| Never Runic Dome; avoid Coffee Dripper, Cursed Key | `games/sts1/text/potions.AVOID`, the boss relic order |

## Open questions worth testing

1. **Act 2 survival.** The act 2 boss ends a third or more of the model's runs in both games (at A7 the Knowledge Demon
   is beaten 27% of the time, 32% from full HP; the Champ 39%). A lean to AoE cards in `deck_fit` lost a floor
   (2026-11-25); routing around act 2 elites without AoE is untried.
2. **Slay the Spire 2's act 3 bosses** stop even the best decks in the model (at A7 the Queen 9%, Test Subject 18%,
   Aeonglass 14% beaten; with the corrected card table of 2026-11-26 26%, 34% and 34%, and more runs now end there than
   at the act 2 boss). The Test Subject and Aeonglass rules match the bot's logs; the Queen now has a model fight
   built from them (`scripts/bosses/queenfight.py`), not yet checked against the whole-run model's.
3. **Settings the model runs favour** are among `learn.CHOICES` (`punch` 0.9 again from 2026-11-25) and, per game, in
   `learn.GAME_DEFAULTS`; the live runs decide whether they stay. At A7 `caution` and `debuff_worth` point different ways
   from batch to batch.
4. **Cards the model runs flag** ([simulation.md](simulation.md)) are now a table the bot reads (`run/model_cards.json`,
   weighed by `model_lean`, 0, 1 or 2 in `learn.CHOICES`; Slay the Spire 2 plays 2 by default, the first game 1): in
   the model it is worth about 4 floors, 2 another floor in Slay the Spire 2, and correcting it by the runs that played
   it (`sim prior --onto`, 2026-11-26) another 2.6 in Slay the Spire 2 and 3.6 in the first game. Whether it carries
   over is for the live runs to say; correct it again (`--onto`) after the model or the bot changes. Upgrades and
   removals that lean on it gained nothing.
5. **Which card to remove.** `_removal_rank` goes by rating: guides say Strikes, then Defends only once Block is
   covered (not early for the Defect and Necrobinder).
6. **One-turn potions** (Flex, Energy, Swift, Duplicator) are drunk on round 1 of a boss like the rest; pricing each by
   the planner's score with and without it would hold them for a turn that uses them. And a quarter of the boss fights
   lost in the model began with an empty belt (7-16% of those won): keeping one potion back for the boss is untried.
7. **Unknown rooms** count a flat 0.9 on the map; they are a fight about one time in five by the act's end.
