# Model runs: what 9,000 simulated runs say

**Batches:** 5,000 runs of Slay the Spire 2 at Ascension 5 (the level the live bot plays) and 4,000 of the first game
at Ascension 0, 1,000 per character, every decision made by the bot's own code. Each run draws its own value of every
setting, so each setting is compared with every other setting averaged out. A run's **score** is the floor it ended
on, plus 15 for a win (one point is about one floor).

## The short version

- **Slay the Spire 2:** play more carefully (`caution` 1.35: HP lost weighs 35% more in the combat planner; it helps
  every character) and rate cards by the bot's own table more than other players' ratings (`crowd` 0-0.3; the Ironclad
  gains most, the Necrobinder nothing). Together with Block-first card picks (`block_want` 1.5) the runs that drew all
  three went **2.7 ± 0.7 floors further and won 8.2% instead of 4.4%**.
- **First game:** the bot's defaults are already the best values tried (`crowd` 0.6, `debuff_worth` 1: together 1.6 ±
  0.6 floors, 5.9% wins instead of 4.3%); `caution` makes no measurable difference.
- **Both games are decided in act 2.** Its boss ends a third or more of all runs: the Knowledge Demon (beaten 34%) and
  the Champ (33%) above all. In Slay the Spire 2 the act 3 bosses then stop the best characters (beaten 13-21%).
- **The bot's best characters** are the Silent, Ironclad and Defect in Slay the Spire 2 (6-9% wins) and the Ironclad
  and Defect in the first game (5-8%). The Regent and Necrobinder beat the act 2 boss 29-35% of the time and the act 3
  boss 4-5%.
- **Changed:** `learn.CHOICES` now also offers `caution` 1.35, `crowd` 0, `debuff_worth` 1 and `block_want` 1.5. The
  live bot keeps a tuning table per game, so each game's runs settle on what works for it.

## Slay the Spire 2, Ascension 5 (5,000 runs)

| Character | Wins | Mean floor | Act 1 boss beaten | Act 2 boss | Act 3 boss | Runs ended by the act 2 boss |
|---|---:|---:|---:|---:|---:|---:|
| Silent | **8.7%** | 35.6 | 93% | 62% | 22% | 32% |
| Ironclad | 6.8% | 31.9 | 82% | 51% | 25% | 35% |
| Defect | 6.1% | 34.6 | 91% | 56% | 17% | 36% |
| Regent | 0.8% | 30.0 | 79% | 35% | 4% | 44% |
| Necrobinder | 0.5% | 28.5 | 80% | 29% | 5% | 44% |

**Where runs end.** Hallway fights, elites and events end few runs (0-3% in act 1, 9-15% in act 2, 5-11% in act 3).
The act 1 boss ends 7-8% of the Silent's and Defect's runs and 18-20% of the others' (the Lagavulin Matriarch and the
Waterfall Giant are the hard ones, beaten 66-92%); the act 2 boss ends 32-44%; the act 3 boss 11-32%. Bosses beaten:

| Boss | Silent | Ironclad | Defect | Regent | Necrobinder |
|---|---:|---:|---:|---:|---:|
| Kaiser Crab (Crusher + Rocket) | 69% | 47% | 70% | 37% | 43% |
| Knowledge Demon | 48% | 41% | 33% | 24% | 20% |
| The Insatiable | 69% | 66% | 64% | 44% | 24% |
| Aeonglass | 22% | 36% | 10% | 7% | 10% |
| Queen (+ Torch Head Amalgam) | 20% | 18% | 10% | 3% | 2% |
| Test Subject | 22% | 24% | 30% | 4% | 0% |

The Knowledge Demon is the hardest act 2 boss for everyone: its three Disintegrations (6, then 13, then 21 damage a
turn) make it a race the slower decks lose. In act 3 each boss punishes something different: the Aeonglass long
fights with many cards played (a Wither every 6 cards, each growing), the Queen everything at once (Chains of Binding,
then 99 turns of Weak, Frail and Vulnerable), the Test Subject Skills (Strength per Skill in its first form) and
slow damage (600 HP over three forms, Intangible every other round in the last).

**Settings** (score over the setting's average, pooled over the five characters; `*` 95%, `**` 99%):

| Setting | Values tried | Best | Effect |
|---|---|---|---|
| `caution` | 0.75 / 1 / 1.35 | 1.35 | -1.0** / +0.2 / +0.8**; 0.75 is the worst (or joint worst) for every character |
| `crowd` | 0 / 0.3 / 0.6 | 0-0.3 | +0.6* / +0.2 / -0.7**; Ironclad +1.9** at 0 |
| `block_want` | 0 / 1.5 | 1.5 | -0.4 / +0.4; for the Regent 1.5 beats 0 by 1.3 (95%) |
| `elite_worth` | 0.8 / 2.5 / 6 | 6 | -0.3 / -0.2 / +0.4 |
| `upgrade_worth` | 1.3 / 2.5 / 4 | 4 | -0.4 / 0.0 / +0.4 |
| `weak_want`, `debuff_worth`, `pickiness` | | 1, 2, 0 | +0.3 each, not significant |
| `punch`, `attack_room`, `free_lean`, `cards_first` | | | 0.2 or less pooled; Defect `punch` 0.9 (+1.6, 95%), Necrobinder `attack_room` 99 (+1.3, 95%) |

The runs that drew both `caution` 1.35 and `crowd` 0 (530 of them) went 1.9 ± 0.5 floors further than the rest and
won 6.6% against 4.3%; with `block_want` 1.5 as well (280 runs), 2.7 ± 0.7 floors and 8.2% against 4.4%. The settings
add up rather than cancel.

**Habits.** Against the runs that got as far and lost to the last boss, the won runs smithed more (3.1-4.1 times
against 2.5-3.1), rested less (3.2-3.8 against 3.8-4.4) and fought a few more elites (0.7-0.8 against 0.5-0.6), for
every character but the Necrobinder (5 wins). That is the same direction as `upgrade_worth` 4 and `elite_worth` 6,
though a healthy run can afford to smith either way.

**Cards** (score over the runs that left the same floor alive, 95% or better; picks in brackets):

| Character | Went with deeper runs | Went with shallower runs |
|---|---|---|
| Silent | Precise Cut +4.0 (76), Afterimage +2.7 (364), Accuracy +1.4 (214), Footwork +1.2 (324), Adrenaline +1.1 (384) | Reflex -2.7 (82), Blur -2.1 (200), Blade of Ink -1.8 (309), Fan of Knives -1.5 (198), Dodge and Roll -1.4 (229) |
| Ironclad | Spite +5.0 (59), Fight Me +4.8 (139), Demon Form +2.7 (251), Inflame +2.6 (320), Mangle +2.4 (97), Impervious +1.8 (305) | Tear Asunder -3.7 (45), Unrelenting -3.7 (46), Bully -3.7 (59), Unmovable -3.4 (125), Pillage -3.3 (60), Aggression -3.2 (105), Colossus -2.6 (111), Dismantle -2.2 (231) |
| Defect | Chill +2.4 (148), Ice Lance +2.0 (194), Echo Form +1.8 (291), Cold Snap +0.8 (833) | Refract -5.3 (173), Hailstorm -3.9 (158), All for One -3.2 (52), Tempest -2.6 (83), Compact -2.6 (154), Glasswork -2.6 (162), Fusion -2.2 (119), Spinner -2.0 (125) |
| Regent | Gamma Blast +2.2 (237), Shining Strike +1.6 (261), Big Bang +1.6 (261), Gather Light +1.1 (680), Celestial Might +1.0 (556), Solar Strike +0.9 (625) | Monarch's Gaze -2.9 (44), Orbit -2.7 (108), Prophesize -2.7 (285), Hidden Cache -2.3 (279), Child of the Stars -2.0 (156), Astral Pulse -1.9 (317), Crescent Spear -1.9 (219), Heavenly Drill -1.7 (112) |
| Necrobinder | Death March +4.9 (121), Danse Macabre +4.8 (48), Bone Shards +3.8 (253), Undeath +1.8 (105), No Escape +1.4 (223), Delay +1.4 (109), Deathbringer +1.0 (345) | Sleight of Flesh -2.1 (201), Pull Aggro -1.9 (143), Flatten -1.7 (219), Capture Spirit -1.6 (182), Squeeze -1.6 (122), Severance -1.4 (132), Veilpiercer -1.4 (230), Lethality -1.3 (245) |

What the lists share: for the Defect, Frost on cards that also attack or cost nothing (Chill, Ice Lance, Cold Snap) over
Glass (Refract, Glasswork, Spinner); for the Ironclad, Strength (Inflame, Demon Form, Fight Me) over Vulnerable
payoffs (Bully, Dismantle, Colossus) and Unmovable; for the Necrobinder, Doom and cheap Block (No Escape, Deathbringer,
Undeath, Danse Macabre) over Osty's attacks and Souls (Flatten, Squeeze, Capture Spirit, Severance); for the Regent,
cheap Star makers and Gamma Blast over Star-cost scaling (Child of the Stars, Crescent Spear) and slow powers (Orbit,
Monarch's Gaze); for the Silent, Powers that pay every turn (Afterimage, Footwork, Accuracy) over one-off Block (Blur,
Dodge and Roll).

## The first game, Ascension 0 (4,000 runs)

| Character | Wins | Mean floor | Act 1 boss beaten | Act 2 boss | Act 3 boss | Runs ended by the act 2 boss |
|---|---:|---:|---:|---:|---:|---:|
| Ironclad | **8.1%** | 34.9 | 96% | 50% | 33% | 40% |
| Defect | 5.2% | 34.4 | 96% | 48% | 25% | 40% |
| Watcher | 2.7% | 31.4 | 93% | 50% | 20% | 32% |
| Silent | 2.0% | 31.7 | 94% | 42% | 15% | 38% |

**Where runs end.** Act 1 is safe: its bosses fall 87-99% of the time. The act 2 boss ends a third or more of all runs,
and act 2's other rooms another 16-27% (the Watcher and Silent most: Snake Plant, Sentry + Spheric Guardian,
Centurion + Mystic). Bosses beaten:

| Boss | Ironclad | Defect | Watcher | Silent |
|---|---:|---:|---:|---:|
| The Champ | 34% | 33% | 42% | 24% |
| Bronze Automaton | 50% | 60% | 48% | 52% |
| The Collector | 65% | 51% | 58% | 52% |
| Awakened One | 26% | 41% | 19% | 20% |
| Donu and Deca | 31% | 19% | 22% | 17% |
| Time Eater | 42% | 19% | 19% | 9% |

The Champ is the wall for everyone. The Time Eater punishes many cheap cards (every 12th card played ends the turn):
the Silent beats it 9% of the time, the Ironclad 42%.

**Settings.** `crowd` 0.6 is the best value (+0.7 pooled, 95%; +1.7 for the Silent, 99%) and `debuff_worth` 1 beats 2
(+0.5 against -0.6, 95%); `elite_worth` 6 (+0.4) and `block_want` 1.5 (+0.2) lean the same way as in Slay the Spire 2.
`caution` does nothing clear here (1.35 is 0.4 worse, not significant). The runs that drew both `crowd` 0.6 and
`debuff_worth` 1 went 1.6 ± 0.6 floors further than the rest and won 5.9% against 4.3%.

**Cards:**

| Character | Went with deeper runs | Went with shallower runs |
|---|---|---|
| Ironclad | Uppercut +2.3 (246), Shockwave +2.3 (425), Carnage +1.8 (373), Battle Trance +1.7 (364), Inflame +1.6 (421), Ghostly Armor +1.2 (316) | Reaper -2.2 (153), Fiend Fire -1.9 (157), Bloodletting -1.7 (319), Intimidate -1.5 (173), Clash -1.5 (351) |
| Defect | Reprogram +2.6 (314), Rainbow +2.2 (500), Fission +2.0 (356), Go for the Eyes +1.7 (173), Boot Sequence +1.4 (345), Melter +1.3 (359), Doom and Gloom +1.1 (443) | Aggregate -2.5 (115), Hyperbeam -2.0 (537), Overclock -1.8 (291), Bullseye -1.7 (293), Reboot -1.2 (385) |
| Watcher | Tantrum +2.6 (383), Brilliance +1.3 (503) | Inner Peace -3.7 (78), Master Reality -2.3 (85), Talk to the Hand -1.6 (133); Foresight -5.2 (37) is the model's (no Scry choices) |
| Silent | Masterful Stab +1.6 (382), Adrenaline +1.4 (334), Crippling Cloud +1.3 (354), Backflip +1.0 (515) | Concentrate -4.3 (260), Grand Finale -2.6 (176), Unload -2.4 (367), Heel Hook -2.0 (251) |

## Method

`sim/` (see the [README](../../README.md#model-runs)) plays whole runs with every decision made by the bot's own code:
the combat planner (`sts2bot/fight/combat.py`), the map, reward, shop, rest-site, event and selection handlers
(`sts2bot/run/strategy.py`), and the settings `learn.py` tunes. Only the game is modelled:

| Part | First game | Slay the Spire 2 |
|---|---|---|
| Cards | every card's text from `sts2bot/games/sts1/text/`, costs and rarities from `sim/content/sts1_cards.py` | 638 cards of v0.111.1 (Spire Codex), keywords included |
| Effects | what the planner's parser reads, plus handlers for ~150 cards whose text says more (conditions, piles, powers) | the same, and Powers read from their own text ("At the start of your turn, ...", "Whenever you play ...") |
| Enemies | ~60 monsters, elites and bosses written from the game's rules, Ascension 0 numbers | 115 monsters with their move state machines (spire-mentor) and A8/A9 numbers, their powers acted out |
| Run | 3 acts (no act 4), Neow, map rules, unknown-room odds, rewards, shops, rest sites, boss relics | 3 acts, Ancients (heal and a relic choice), the same, boss rewards without relics |
| Simplified | events (a table of typical outcomes, chosen by the bot's event handler), a common subset of relics and potions, no Scry choices | the same; no enchantments, quests or the Fool; Dampen, Galvanic, Infested, Possess, Ravenous and Surprise do nothing |

Each run draws its own value of every setting (`--vary`, the values in `sim/__main__.GRID`: the bot's choices and one
on either side), so a setting's effect is the difference between the runs that drew each of its values. With 1,000
runs a character, a value's effect has a standard error of about 0.55 for one character and 0.27 pooled.

```sh
python3 -m sim run --game sts2 --character all --runs 1000 --vary --seed 2000 --ascension 5 --out sts2.jsonl
python3 -m sim run --game sts1 --character all --runs 1000 --vary --seed 1000 --out sts1.jsonl
python3 -m sim report sts2.jsonl sts1.jsonl
```

## How far to trust it

The model is easier than the live game in places (events never ambush, relics and potions are a common subset,
monsters do exactly what their tables say) and harder in others (no Scry choices, no Neow swap). So the absolute
win rates are the model's, not the game's; what transfers is the **comparison** between settings and between cards
under the bot's own play, and where in a run the bot loses. A finding is a candidate for `learn.CHOICES` (the live
bot then tests it on real runs), not a fact about the game.

The bosses were checked against the model fights built from the bot's own logs (`scripts/bosses/`, `demonfight.py`):
the Knowledge Demon matched, and the Test Subject (Strength reset between forms, Multi-Claw growing a hit a round) and
the Aeonglass (Withering Presence, Withers growing with each buff) were corrected to match before these batches. So
were debuffs that enemies put on us (they lasted a turn too few, so Weak 1 from a monster did nothing), Slay the Spire
2's Plating (it never wore off), and the first game's Awakened One (it never rose for its second form).

Three cautions when reading the tables:

- **Many comparisons.** The settings table holds about 30 values per game for each of 4-5 characters: at 95%, a few
  cells are marked by chance alone. The pooled column and settings that point the same way for every character are
  the ones to act on.
- **Extremes of a list.** The card lists are the marked cards among the top and bottom of 60-100 per character; their
  sizes are inflated by being picked as extremes. Cards with many picks are the trustworthy ones.
- **What the model leaves out favours what it models.** A card whose strength is something the model simplifies
  (an event, a relic synergy, a Scry choice) looks worse here than it is. That matters most for `crowd`: other
  players' ratings credit effects the model does not have, so its preference for the bot's own table may be too strong.
