# Model runs: what the simulator says

**Latest round (2026-11-26, round 5):** 9,900 runs (Slay the Spire 2 at **Ascension 8**, where the live bot now plays
every character; the first game at Ascension 0): 2,700 of the bot as it was, 3,600 drawing a corrected card table and
three new rules against the old ones, and 3,600 of the bot before and after on seeds nothing was chosen on. The main
finding was about **the model itself**: it was wrong about two dozen cards, and the card table had learned its mistakes.
A run's **score** is the floor it ended on, plus 15 for a win (one point is about one floor). [Round 4](#round-4-2026-11-26)
follows, then [round 3](#round-3-2026-11-26), the batches of [2026-11-25](#batches-of-2026-11-25) and the A5 batches
[further down](#earlier-9000-runs-at-ascension-5-and-0).

## Round 5 (2026-11-26)

- **The model was wrong about two dozen cards, and the card table had learned it.** The shipped table (round 4's) had
  the Silent's **Grand Finale** at its +3 cap: no "Can only be played if" held in Slay the Spire 2's model, so it was 60
  damage to all enemies for nothing whenever drawn (+7.5 floors a pick at A8). At `model_lean` 2 that is 6 rating points,
  and on the corrected model 29% of the Silent's decks took it and it beat the act 3 boss 10% of the time (2% and 25%
  with the corrected table). An audit that plays every card with a drawback or a pile clause in a model fight
  (`scripts/audit/simcards.py`) found the rest: **Bone Shards** left Osty alive and **Fight Me** gave the enemy no
  Strength (both at +3); statuses cards put in our piles were never added (Fight Through's two Wounds, +3; Boost Away,
  Gunk Up, Overclock, Turbo), nor copies (Adaptive Strike, Undeath), Colorless cards (Bundle of Joy, Manifest Authority,
  Quasar), Stoke's new hand or cards moved between piles (Dredge, Graveblast, All for One, Secret Technique, Cosmic
  Indifference, Thrash, Foregone Conclusion, Nightmare); this turn's Focus never counted (Hotfix, Hyperbeam). In the first
  game Reprogram gave its Strength and Dexterity twice (+3 in the table too). The Ancients' Philosopher's Stone and
  Pandora's Box were titled so that the bot's own relic table never knew them. All fixed; those 25 cards start again
  from 0 in the table, and what the runs on the corrected model say of them is theirs.
- **The planner was blind to two of them in the live game as well:** an enemy's Strength gained (Fight Me) and the
  statuses a card puts in our piles (Wild Strike, Reckless Charge, Power Through and Immolate in the first game; Fight
  Through, Boost Away, Gunk Up, Overclock and Turbo) now cost what a lasting Strength loss gains and what
  `battle.junk_cost` prices a dead card at, as Debris already did.
- **The bot before and after**, 200 runs a character on seeds nothing was chosen on (54000 and 55000), the corrected
  model, settings fixed: "before" is the bot as it was (commit 28bddab: the round-4 table, every setting as it was),
  "after" the bot now (the table rebuilt below, `belt_spare` 0 in Slay the Spire 2, the planner's two new prices).

| | Slay the Spire 2, A8 (1,000 pairs) | First game, A0 (800 pairs) |
|---|---|---|
| **The bot after** | **+1.83 ± 0.97 floors** (95%); wins 5.7% → 9.5% | **+2.43 ± 1.19**; wins 17.0% → 24.8% |

  In Slay the Spire 2 the Defect gains most (+5.2 ± 2.5, wins 5.5% → 13.0%), then the Silent (+2.2), Necrobinder
  (+1.3) and Regent (+1.2, each ± 1.9-2.1; the Ironclad -0.6 ± 2.3), and every act 2 and 3 boss but the Queen falls more
  often: **the Knowledge Demon 35% → 53%**, the Kaiser Crab 67% → 74%, the Insatiable 60% → 67%, the Test Subject 16% →
  27%, the Aeonglass 22% → 29% (the Queen 16% → 17%); the act 2 boss ends 27% of runs, not 35%. In the first game every
  character goes further (the Defect +3.1, Watcher +2.35, Ironclad +2.3, Silent +1.9, each ± 2.0-2.6; wins 22% → 31.5%,
  5% → 9.5%, 24.5% → 33.5%, 16.5% → 24.5%), and the Time Eater is beaten 43% → 61%, Donu and Deca 42% → 50%, the
  Awakened One 50% → 56%, the Champ 62% → 69%.

- **The card table, rebuilt on the corrected model.** C: the shipped table plus what the 2,700 runs of the bot as it was
  say (`sim prior --onto`; those runs were on the old model, so the 25 cards were reset). D, shipped: C plus what the
  1,025 + 800 runs that played C on the corrected model say (`--table 1`). In the varied batch C against the shipped
  table: Slay the Spire 2 +0.42 ± 0.56 floors, all of it the Silent's (+3.5 ± 1.2, 99%; wins 3.6% → 11.2%: Grand Finale);
  the first game **+3.0 ± 0.65** (99%; wins 15.9% → 22.6%), every character ahead (the Silent +5.2, Defect +3.4, Ironclad
  +2.8, Watcher +0.5). What moved most from the old table to D: Fight Through +3.0 → -0.6, Hyperbeam +1.7 → -1.2, Go for
  the Eyes -0.6 → +2.4 and Adaptive Strike -1.6 → +0.7 for the Defect; Undeath +3.0 → +0.4, Bone Shards +3.0 → +1.5 and
  Graveblast -0.6 → -3.0 for the Necrobinder; Fight Me +3.0 → +2.4, Inflame, Impervious and Battle Trance to +3.0, Bully
  +1.6 → -1.0 for the Ironclad; Orbit +0.3 → -2.6 for the Regent; Grand Finale +3.0 → 0, Dash to +3.0 for the Silent; in
  the first game Reprogram +3.0 → -0.4, Echo Form and Reinforced Body to +3.0, Barrage +1.3 → -2.1, Anger +0.4 → +2.9,
  True Grit 0 → -2.8, Backstab and Choke to +3.0.
- **A full belt is no reason to drink in a Slay the Spire 2 hallway fight** (`belt_spare` 0, Slay the Spire 2's
  `learn.GAME_DEFAULTS`): **+1.23 ± 0.57 floors** (95%) in the varied batch, ahead for four characters of five. Hallway
  fights drank half as many potions (0.11 → 0.06 a fight) for no more HP lost (9.6% of Max HP a fight either way), and
  the act 2 boss was beaten 63% of the time instead of 54%. In the first game the same rule went **1.50 ± 0.65 floors
  short** (95%; the Ironclad 4.4): three slots at A0, and act 2 and 3 hallways that end a quarter of its runs. So it is
  Slay the Spire 2's default only, and not among the live choices (the two games want opposite values).
- **Tried and dropped** (each a `--vary` setting, then removed):

| Rule | StS2 A8 (2,000) | First game A0 (1,600) |
|---|---|---|
| An elite away from the boss gets the whole belt, as a boss does (`elite_potions`) | +0.5 ± 0.55 | +0.2 ± 0.65 |
| What arriving hurt costs at a boss halved in the route planner, and the rest before the boss with it (`hp_fear` 0.5) | -0.3 ± 0.55 | +0.9 ± 0.65 |

- **Settings** in the same batches (score against the default, one standard error): nothing reaches 95% in either game.
  `caution` 1.35: +0.4 / -0.4; `block_want` 1.5: -0.7 / -0.5; `upgrade_worth` 2.5 and 4 against the default 1.3: +0.2
  and -0.1 / -0.2 and -0.9 (StS2 / the first game).
- **A8 against A7** on the same 60 seeds: 7 wins against 8, mean score 39.4 against 38.5. Enemy HP 5% up (`hp_a8`) changes
  little; the bosses are what they were.
- **Arriving hurt at the act 3 boss costs less than the route planner assumes** (the bot as it was, 1,500 runs at A8):
  from 80-90% of Max HP it was won 0.66 times as often as from 90%+, from 70-80% 0.76 times (the planner's curve: 0.53
  and 0.25). Those runs are also the weaker ones, so the true cost is lower still; but halving it in the planner
  (`hp_fear` above) changed nothing.

- **Where the bot stands now** (the after arm above): Slay the Spire 2 at A8 wins Defect 13%, Necrobinder 11%, Ironclad
  9.5%, Silent 8%, Regent 6%; the act 2 boss ends 27% of runs and the act 3 boss 29%. The first game at A0 wins Ironclad
  33.5%, Defect 31.5%, Silent 24.5%, Watcher 9.5%; act 3's hallways end 17% of runs. These are lower than round 4's
  because the model is harder than it was where it had been wrong: the bot as it was won 10.7% of Slay the Spire 2 runs at
  A8 on the old model and 5.7% on the corrected one (the Silent 16% → 5%, the Defect 14% → 5.5%), and 20.8% → 17% of the
  first game's (the Defect 36% → 22%, Reprogram).

```sh
python3 -m sim run --game sts2 --character all --runs 300 --seed 50000 --ascension 8 --out base_sts2.jsonl   # the bot as it was
python3 -m sim run --game sts1 --character all --runs 300 --seed 51000 --out base_sts1.jsonl
python3 -m sim prior base_sts2.jsonl base_sts1.jsonl --onto sts2bot/run/model_cards.json --out C.json         # (the 25 cards then reset to 0)
python3 -m sim run --game sts2 --character all --runs 400 --seed 52000 --ascension 8 --vary --alt-cards C.json \
  --only model_table,belt_spare,elite_potions,hp_fear,caution,block_want,upgrade_worth --grid caution=1,1.35 --out var_sts2.jsonl
python3 -m sim prior var_sts2.jsonl var_sts1.jsonl --onto C.json --table 1 --out D.json                        # (the first game alike, seed 53000; shipped)
python3 -m sim run --game sts2 --character all --runs 200 --seed 54000 --ascension 8 --out after.jsonl          # ("before": the bot of 28bddab on this model)
python3 scripts/audit/simcards.py sts2                                                                       # what the model does with each card's text
```

## Round 4 (2026-11-26)

- **The card table now also learns from the runs that play it.** It was built only from runs that did not lean on it,
  so it described cards for a bot that ignores it. `python3 -m sim prior --onto TABLE` keeps a table and adds what the
  runs that played it (at the game's own `model_lean`) still say of each card: a card whose picks still went with
  deeper runs than the runs that left the same floor alive moves up, one whose picks went with shallower runs moves
  down (the same measure as before, shrunk by 40 picks, no card past ±3). Built from 1,000 + 800 runs of the bot as
  it was (seeds 40000-40199 and 41000-41199) and set against the old table on fresh seeds, each run drawing the table
  and every other setting under test (`--vary --only`, `--alt-cards`): Slay the Spire 2 **+1.9 ± 0.55 floors** (one
  standard error; 99%), every character ahead (the Necrobinder +3.9, Defect +2.5, Regent +2.1, Silent +0.7, Ironclad
  +0.6); the first game +0.9 ± 0.6 (the Defect +4.6, Ironclad -1.4).
- **The bot before and after**, 200 runs a character on seeds nothing was chosen on (44000 and 45000), settings fixed.
  Two tables were tried: **A**, the old table plus what all 2,015 + 1,612 runs that played it say (403 a character),
  and **B**, a second pass: the table above plus what the 985 + 788 runs that played *it* say (197 a character).

| | Slay the Spire 2, A7 (1,000 pairs) | First game, A0 (800 pairs) |
|---|---|---|
| **B, shipped** (`run/model_cards.json`) | **+2.6 ± 0.95 floors** (95%); wins 10.5% → 16.4% | **+3.6 ± 1.2**; wins 11.6% → 19.0% |
| A | +2.1 ± 0.9; wins 13.9% | +3.3 ± 1.15; wins 19.4% |
| B against A | +0.6 ± 1.0 | +0.3 ± 1.2 |

  B is ahead of A in both games, though not by a clear margin; both are well ahead of the old table. With B every
  character of Slay the Spire 2 but the Silent goes further (the Necrobinder +3.8, Defect +3.7, Ironclad +3.4, Regent
  +2.6, each ± 1.9-2.3; the Silent -0.4), and every boss from act 2 on falls more often: the Knowledge Demon 48% →
  62%, the Kaiser Crab 68% → 77%, the Insatiable 69% → 76%, the Test Subject 25% → 34%, the Aeonglass 28% → 34%, the
  Queen 21% → 26%. In the first game the Defect gains most (+7.5, wins 14% → 30.5%), then the Ironclad (+3.8),
  Watcher (+2.4) and Silent (+0.7); the Champ is beaten 49% → 63%, the Time Eater 34% → 50%, the Bronze Automaton
  60% → 78%. What moved most (old → new rating shift, before `model_lean`): the Silent's Afterimage +1.2 → +3.0 and
  Untouchable +0.5 → -1.1, the Defect's Scrape +0.7 → +3.0 and Adaptive Strike +0.6 → -1.6, the Ironclad's Fiend Fire
  +0.1 → +2.5 and Drum of Battle +0.1 → -2.4, the Necrobinder's Undeath +1.0 → +3.0 and Snap +1.1 → -1.9, the Regent's
  Knockout Blow +1.2 → +3.0 and Reflect +0.8 → -1.1; in the first game the Watcher's Tantrum and Brilliance, the
  Ironclad's Immolate and the Silent's Masterful Stab to +3.0, Bloodletting +0.1 → -3.0, Hologram +1.4 → -1.4 and
  Worship +0.7 → -1.9. The old table's own rule stands: correct it again when the model or the bot's card play changes.

- **Two act 3 monsters of Slay the Spire 2 were harder in the model than in the game** (`sim/content/sts2_foes.py`).
  The Owl Magistrate's Soar ("Receives 50% less attack damage until it lands", Spire Codex) lasted the rest of the
  fight once Judicial Flight had raised it; it now ends with Verdict, the swoop that follows. A new Axebot that
  replaces a killed one (Stock) kept the old one's Strength (Boot Up gives 3 a time) and debuffs; it now starts
  afresh. On the bot as it was (1,000 runs before and after the fix, other seeds), an Owl fight went from 26% of Max
  HP to 20% (the Ironclad lost 12 of 31, then 5 of 31), an Axebot fight from 30% to 22%.
- **Tried and dropped** (each a `--vary` setting, then removed):

| Rule | StS2 A7 (2,000) | First game (1,600) |
|---|---|---|
| Upgrades lean to the cards the table rates: its shift added to a card's worth at the smith (`model_upgrade`) | -0.9 ± 0.55 | -0.2 ± 0.6 |
| Card removal does the same for cards beyond the starter deck (`model_removal`) | 0.0 ± 0.55 | +0.3 ± 0.6 |

- **Settings** in the same batches (score against each game's default, one standard error; `*` 95%): nothing moves
  in Slay the Spire 2; in the first game `caution` 1.35 is +1.8 ± 0.75* (the 4,000 runs at A0 of 2026-11-25 had it
  0.4 behind, so not adopted) and `block_want` 1.5 +1.0 ± 0.6. `debuff_worth` 3 is 1.3 ± 0.7 behind 1 in Slay the
  Spire 2, `punch` 2.7 1.15 ± 0.65 behind 0.9 in the first game: the defaults stand.

| Setting | StS2 A7 | First game A0 |
|---|---|---|
| `caution` 0.75 / 1.35 (against 1) | -0.3 / +0.2 | +0.5 / +1.8* |
| `block_want` 1.5 (against 0) | +0.4 | +1.0 |
| `debuff_worth` 2 / 3 (against 1) | -0.8 / -1.3 | -1.0 / +0.5 |
| `upgrade_worth` 2.5 / 4 (against 1.3) | -1.0 / 0.0 | -0.4 / +1.35 |
| `punch` 2.7 (against 0.9) | -0.5 | -1.15 |
| `attack_room` (StS2: 99 against 3; first game: 3 against 99) | -0.2 | +0.3 |
| `elite_worth` (StS2: 0.8 / 6 against 2.5; first game: 0.8 / 2.5 against 6) | -0.3 / -0.2 | +0.7 / +0.1 |

- **Where the bot stands now** (the shipped table, the corrected model; 1,000 runs of Slay the Spire 2 at A7, 800 of
  the first game at A0): Slay the Spire 2 wins Defect 22%, Silent 21.5%, Ironclad 17.5%, Necrobinder 16%, Regent 5%;
  runs end at the act 2 boss 25% of the time (was 32%) and at the act 3 boss 36% (was 32%): **the act 3 bosses are
  now the wall** (the Queen beaten 26%, Test Subject 34%, Aeonglass 34%), and the Regent the weakest character (the
  act 3 bosses 6-18%). The first game wins Defect 30.5%, Ironclad 26.5%, Silent 14.5%, Watcher 4.5%; the act 2 boss
  ends 23% of runs (was 33%), act 3's other rooms 21%.
- **Potions at bosses** (fights now log the potions drunk and those left, `drunk` and `left`): a lost boss fight almost
  never leaves a potion unused (under 1.5%), but a quarter of them (23-26% in both games) began with none at all,
  against 7-16% of the boss fights won from act 2 on. Arriving with a potion is a lead worth testing (keeping one
  back on the way, buying one before the boss), not yet a rule.

```sh
python3 -m sim run --game sts2 --character all --runs 200 --seed 40000 --ascension 7 --out base_sts2.jsonl   # the bot as it was
python3 -m sim run --game sts1 --character all --runs 200 --seed 41000 --out base_sts1.jsonl
python3 -m sim prior base_sts2.jsonl base_sts1.jsonl --onto sts2bot/run/model_cards.json --out T2.json
python3 -m sim run --game sts2 --character all --runs 400 --seed 42000 --ascension 7 --vary --alt-cards T2.json \
  --only model_table,model_upgrade,model_removal,upgrade_worth,elite_worth,caution,debuff_worth,punch,block_want,attack_room --out p2_sts2.jsonl
python3 -m sim prior base_sts2.jsonl p2_sts2.jsonl --onto sts2bot/run/model_cards.json --out A.json      # (the first game alike, seeds 43000)
python3 -m sim prior p2_sts2.jsonl --onto T2.json --table 1 --out B.json                                  # (shipped)
python3 -m sim run --game sts2 --character all --runs 200 --seed 44000 --ascension 7 --out before.jsonl
python3 -m sim run --game sts2 --character all --runs 200 --seed 44000 --ascension 7 --alt-cards B.json --set model_table=1 --out after.jsonl
```

## Round 3 (2026-11-26)

- **Slay the Spire 2 now leans on the model card table twice as hard** (`learn.GAME_DEFAULTS["sts2"]["model_lean"]`
  2, was 1). Same seeds (32000-32199, 200 runs a character), settings fixed: **+0.93 ± 0.42 floors** (95%), wins
  8.2% → 9.8%; the Necrobinder +2.6, Ironclad +1.8, Regent +0.7, Silent +0.4, Defect -0.8 (each ± 0.9). The varied
  batch said the same (+1.05 ± 0.72). Lean 3 went back 0.6 ± 0.4 from 2: 2 is the top.
- **The first game stays at 1.** Lean 2 there: +0.20 ± 0.55 on the same seeds (33000-33199), +1.4 ± 0.9 in the varied
  batch; lean 3 +1.1 ± 0.6 on the 644 runs it got through. Not enough to move the default; the live bot now draws
  `model_lean` 0, 1 or 2 (`learn.CHOICES`) in both games, and its own runs decide.
- **Four route and rest rules tried and dropped** (each a `--vary` setting, then removed; no effect in either game):

| Rule | StS2 A7 (1,500) | First game (1,200) |
|---|---|---|
| Getting past the boss worth 24 to the route planner, not 16 (`BOSS_WORTH`) | -0.4 ± 0.6 | -0.2 ± 0.7 |
| A rest site always heals below 65% HP, not 50% | -0.4 ± 0.6 | -0.6 ± 0.7 |
| Routes judged 70% by how they go when fights cost 1.7 times the average, not 40% | -0.3 ± 0.6 | +0.9 ± 0.7 |
| An act 1 upgrade worth 6 against resting, not 4 (`UPGRADE_EARLY`) | +0.5 ± 0.6 | -0.3 ± 0.7 |

- **Also checked, not adopted:** Block-first picks (`block_want` 1.5) with no cap on attacks (`attack_room` 99) on top
  of lean 2: +0.34 ± 0.36 (the Defect and Silent +1.5, the Necrobinder and Regent -0.8). In the varied batch
  `attack_room` 99 was +1.4 ± 0.6 over 3, where three batches of 2026-11-25 had it 0.7-1.3 behind: no clear direction.
  Without the table (`model_lean` 0) runs lose 4.4 ± 0.7 floors in Slay the Spire 2 and 3.1 ± 0.9 in the first game,
  as before.
- **Where the bot stands now** (Slay the Spire 2 at A7, the new defaults, 1,000 runs): wins Silent 13.5%, Defect 11%,
  Ironclad 11%, Necrobinder 8.5%, Regent 5%; bosses beaten: Kaiser Crab 70%, Insatiable 69%, **Knowledge Demon 47%**,
  Test Subject 25%, Aeonglass 26%, **Queen 19%**. The first game at A0 (800 runs): wins Defect 17%, Ironclad 12%,
  Silent 10.5%, Watcher 4%; the Champ 52%, Donu and Deca 28%, Time Eater 42%. The act 2 boss and the act 3 bosses are
  still where runs end.

```sh
python3 -m sim run --game sts2 --character all --runs 300 --vary --seed 30000 --ascension 7 --out r3_sts2.jsonl
python3 -m sim run --game sts1 --character all --runs 300 --vary --seed 31000 --out r3_sts1.jsonl
python3 -m sim run --game sts2 --character all --runs 200 --seed 32000 --ascension 7 --set model_lean=2 --out lean2.jsonl  # and =1, =3
python3 -m sim run --game sts1 --character all --runs 200 --seed 33000 --set model_lean=2 --out lean2_sts1.jsonl
```

## Batches of 2026-11-25

At the levels the live bot plays, every decision made by the bot's own code. Settings are compared with `--vary`
(each run draws its own value of every setting); the bot before and after the changes on the same seeds without it.

### The short version

- **Ascension 7 is hard on the bot:** 0-5% wins per character in the model (the Ironclad, Silent and Defect 4-5%, the
  Regent and Necrobinder next to none). The act 2 boss ends 33-46% of runs; the **Knowledge Demon** is beaten 27% of
  the time and only 32% even from full HP (the Kaiser Crab and the Insatiable go from about 55% to 65-70% at full HP).
  The act 3 bosses are beaten 9-18%.
- **The first game at A0** is where it was: 2-8% wins, the Champ the wall (39%), act 2's hallways ending 15-25% of runs.
- **The bot before and after, on 1,800 seeds none of it was chosen on:** Slay the Spire 2 at A7 **+5.3 ± 0.4 floors**
  (wins 2.6% → 6.6%; every character better, the Defect +7.7, Regent +7.0, Necrobinder +6.3, Silent +3.5, Ironclad
  +2.1; the act 2 bosses beaten 48% → 65% the Kaiser Crab, 34% → 45% the Knowledge Demon, 48% → 66% the Insatiable),
  the first game at A0 **+3.9 ± 0.6** (wins 5.4% → 13.5%; the Silent +6.9, Defect +4.7, Ironclad +2.6, Watcher +1.5;
  the Champ 42% → 56%).
- **Most of it comes from rating cards by what the model runs say of them** (`model_lean`, `sts2bot/run/model_cards.json`,
  built by `python3 -m sim prior`). On seeds the table had not learned from, Slay the Spire 2 runs with it went
  **4.0 floors further** (99%), better for all five characters, and won 6.6% of runs instead of 2.9%; first-game runs
  went **3.4 floors further** (99%) and won 10.6% instead of 4.1%.
- **Per-game defaults** (`learn.GAME_DEFAULTS`: played with `--no-tuning` and by the model): Slay the Spire 2 takes
  `crowd` 0.3 (0.6, the old default, was the worst of three in every batch) and `attack_room` 3 (0.7-1.3 floors ahead
  in all three), the first game `elite_worth` 6 and `cards_first` 1 (1.4-1.9 floors ahead in both its batches), and
  both `model_lean` 1.
- **The live bot's choices** (`learn.CHOICES`) now offer `model_lean` 0/1 and `punch` 0.9 again (it only ever played 2.7;
  0.9 was 0.2-1.3 floors ahead in all three A7 batches): its own runs decide.
- **Tried and dropped** (no gain, or a loss): act 3 card rewards needing 1.5 or 3 more to be taken (+0.9 on one batch,
  -1.1 and -0.9 on fresh seeds of each game), a lean to AoE cards while the deck has fewer than two (-1.1 floors, 99%),
  a lean to Powers that scale every turn (-0.3), lasting effects counted for 4.5 turns of a boss fight instead of 3
  (+0.2), card removal priced higher at the Merchant (-0.2), damage worth 25% more in boss fights (+0.1).
- **The model now matches the game at A6+:** Slay the Spire 2 runs no longer start at 90% HP (the first game's A6),
  no longer heal after a boss on top of the next Ancient's heal (at A5+ they healed 95% of missing HP, not 80%), and at
  A7 rare cards are half as common.

### What changed

| | Slay the Spire 2, A7 | First game, A0 |
|---|---|---|
| **The bot before and after** (same seeds, no `--vary`, 200 runs a character) | **+5.3 ± 0.4 floors** (99%); wins 2.6% → 6.6% | **+3.9 ± 0.6 floors** (99%); wins 5.4% → 13.5% |
| `model_lean` 1 against 0 (round 2: fresh seeds, `--vary`) | **+4.0 ± 0.5 floors** (99%); wins 2.9% → 6.6% | **+3.4 ± 0.7** (99%); wins 4.1% → 10.6% |
| Defaults (`learn.GAME_DEFAULTS`) | `crowd` 0.3, `attack_room` 3, `model_lean` 1 | `elite_worth` 6, `cards_first` 1, `model_lean` 1 |
| Live choices (`learn.CHOICES`) | `model_lean` [0, 1] added; `punch` [0.9, 2.7] (was [2.7]) | the same table |

"Before" is the bot as it was, with its defaults (what `--no-tuning` played, and the model's own default); "after" is
the bot now, with each game's defaults and the card table (seeds 9000-9199 and 10000-10199, which no table or setting
was chosen on). The live bot plays neither exactly: it draws each setting from `learn.CHOICES`.

**The card table.** `python3 -m sim prior` reads run files and writes, per game and character, the floors each card's
picks gained over the runs that left the same floor alive (the measure of the card lists below and of
`learn.CardStats`), less the character's average pick, shrunk by 40 picks and capped at ±3 rating points. With
`model_lean` 1 that is added to the card's rating at rewards and on the Merchant's shelf. Slay the Spire 2's was built
from the 3,000 runs of round 1 (seeds 5000-5299) and tested on seeds 7000-7399: every character gained (Necrobinder
+5.5, Defect +5.0, Regent +3.3, Ironclad +3.1, Silent +1.8), and every act 2 boss fell more often (the Knowledge Demon
29% → 38%, the Kaiser Crab 52% → 64%, the Insatiable 54% → 63%). The first game's, built from its 1,200 base runs and
tested on seeds 8000-8299, did the same for all four characters (+1.1 to +5.2). What it says agrees with the A5
batch's card lists below, played on other seeds at another ascension: for the Defect Frost over Glass (Refract,
Glasswork, Hailstorm low; Cold Snap, Ice Lance, Echo Form high), Bone Shards and Death March for the Necrobinder, Gamma
Blast and Shining Strike over Hidden Cache and Child of the Stars for the Regent. The shipped table was rebuilt from
every run that did not lean on it (3,970 of Slay the Spire 2, 1,836 of the first game); it never learns from runs that
used it (`model_lean` is recorded per run). **Rebuild it** when the model or the bot's card play changes, or it
describes a bot that no longer exists.

**Why it may not carry over in full.** Part of the gain may be the table steering away from cards whose strength is
what the model leaves out (events, relic synergies, Scry choices), which would not be a gain in the game. So the live
bot draws `model_lean` 0 or 1 like any other setting, and its own runs decide.

**Settings, three batches of Slay the Spire 2 at A7** (base, round 1 and round 2 with `--vary`; score difference from
a regression on every setting at once, `*` 95%, `**` 99%):

| Setting | Base (1,500) | Round 1 (1,500) | Round 2 (2,000) | Taken |
|---|---|---|---|---|
| `crowd` 0.6 against 0 | -1.4* | -1.3* | -1.5* | 0.3 (0 and 0.3 trade places) |
| `attack_room` 99 against 3 | -1.1* | -1.3* | -0.7 | 3 |
| `punch` 2.7 against 0.9 | -1.3* | -0.8 | -0.2 | 0.9 offered to the live bot again |
| `elite_worth` 6 against 2.5 | -0.1 | +0.5 | -0.4 | unchanged (0.8 is worst) |
| `cards_first` 1 against 0 | +0.6 | +0.9 | -0.6 | unchanged |
| `caution` 1.35 against 1 | -0.7 | -0.5 | +1.0 | unchanged |
| `debuff_worth` 2 against 1 | +0.1 | -0.1 | +1.7** | unchanged |

`caution` 1.35, best at A5, is no better at A7; `debuff_worth` 2 and `weak_want` 0 stand out in one batch only (with
some 20 comparisons a batch, one or two marked cells are chance).

**Tried and dropped** (each a setting in `--vary`, then removed):

| Rule | Effect | |
|---|---|---|
| Act 3 card rewards need 1.5 (or 3) more to be taken | +0.9* on round 1's seeds; -1.1 and -0.9 on fresh ones (both games) | the round 1 result was chance |
| +1.5 for an AoE card while the deck holds fewer than two | -1.1** | worse for the Ironclad and Silent above all |
| +1.5 for a Power that pays every turn (Inflame, Demon Form, Footwork, Afterimage, Echo Form) while fewer than two | -0.3 | their ratings already carry it |
| Lasting effects count 4.5 turns of a boss fight, not 3 | +0.2 | |
| Card removal +1.5 at the Merchant | -0.2 | |
| Damage worth 25% more in boss fights | +0.1 | |

### Slay the Spire 2, Ascension 7 (1,500 runs, before the changes)

| Character | Wins | Mean floor | Act 1 boss beaten | Act 2 boss | Act 3 boss | Runs ended by the act 2 boss |
|---|---:|---:|---:|---:|---:|---:|
| Silent | 4.3% | 35.0 | 95% | 60% | 14% | 33% |
| Defect | 4.3% | 33.4 | 92% | 54% | 14% | 35% |
| Ironclad | **5.3%** | 31.3 | 84% | 46% | 26% | 39% |
| Regent | 0.3% | 29.7 | 80% | 33% | 2% | 46% |
| Necrobinder | 0.0% | 27.9 | 76% | 31% | 0% | 40% |

**Where runs end.** The act 1 boss ends 5-8% of the Silent's and Defect's runs and 16-24% of the others' (the
Lagavulin Matriarch beats the Necrobinder 42% of the time, the Waterfall Giant the Ironclad, Regent and Necrobinder about
30%); the act 2 boss 33-46%; act 2's other rooms 11-16%; the act 3 boss 10-28%. Bosses beaten:

| Boss | Silent | Defect | Ironclad | Regent | Necrobinder | All |
|---|---:|---:|---:|---:|---:|---:|
| Kaiser Crab (Crusher + Rocket) | 72% | 63% | 53% | 36% | 45% | 55% |
| Knowledge Demon | 39% | 29% | 35% | 13% | 14% | 27% |
| The Insatiable | 69% | 67% | 48% | 47% | 34% | 54% |
| Aeonglass | 23% | 9% | 20% | 0% | 0% | 14% |
| Queen (+ Torch Head Amalgam) | 11% | 7% | 28% | 0% | 0% | 9% |
| Test Subject | 7% | 35% | 30% | 6% | 0% | 18% |

Arriving HP matters at the Kaiser Crab and the Insatiable (53-56% beaten from 80-99% HP, 65-70% from full) more than at
the Knowledge Demon (27%, 32%): that one is lost on the deck, not the way there.

**Habits.** As at A5, the won runs fought more elites than the runs that lost to the last boss (0.6-2.0 against
0.3-0.5) and, but for the Ironclad's, smithed more (2.9-4.0 against 2.5-2.8).

**Cards** (score over the runs that left the same floor alive, `*` 95%, `**` 99%; picks in brackets):

| Character | Went with deeper runs | Went with shallower runs |
|---|---|---|
| Silent | Noxious Fumes +1.8* (119), Footwork +1.5 (106), Afterimage +1.4 (112), Leg Sweep +1.1 (105) | Deflect -4.2** (47), Fan of Knives -2.2* (49), Echoing Slash -1.6 (89), Flick-Flack -1.4 (141) |
| Defect | Null +2.5* (125), Echo Form +2.0 (68), Glacier +1.3 (119), Cold Snap +1.2 (257) | Refract -5.2** (60), Glasswork -4.4** (49), Hologram -2.7* (44), Focused Strike -1.9** (167), Lightning Rod -1.9* (79) |
| Ironclad | Demon Form +2.8* (77), Fight Me +2.6 (38), Rampage +2.0 (76), Taunt +1.5 (118) | Armaments -3.6** (42), Whirlwind -2.8* (46), Howl from Beyond -2.8* (63), Blood Wall -1.6 (98) |
| Regent | Hegemony +3.3** (35), Shining Strike +3.2** (76), Bulwark +3.1* (50), Big Bang +1.7* (67), Gather Light +1.4** (212) | Hidden Cache -2.6** (91), Crescent Spear -2.4* (71), Child of the Stars -2.2* (54), Heirloom Hammer -2.0* (50) |
| Necrobinder | Death March +4.2** (39), Bone Shards +3.9** (76), Misery +2.1* (53), Grave Warden +1.4** (212) | Pull Aggro -3.9** (38), Severance -3.4** (39), Drain Power -1.8** (151), Capture Spirit -1.9* (54), Lethality -1.7* (81) |

### The first game, Ascension 0 (1,200 runs, before the changes)

| Character | Wins | Mean floor | Act 1 boss beaten | Act 2 boss | Act 3 boss | Runs ended by the act 2 boss |
|---|---:|---:|---:|---:|---:|---:|
| Ironclad | **8.0%** | 34.7 | 93% | 53% | 31% | 36% |
| Defect | 5.3% | 34.0 | 95% | 48% | 28% | 39% |
| Watcher | 3.3% | 31.8 | 90% | 56% | 26% | 27% |
| Silent | 2.0% | 32.0 | 92% | 46% | 14% | 35% |

The Champ is beaten 39% of the time (the Defect and Silent 30-32%), the Bronze Automaton 59%, the Collector 55%; in act 3
the Awakened One 30%, Donu and Deca 21%, the Time Eater 27%. Act 2's hallways end 15-25% of runs (the Silent and Watcher
most). Settings: `elite_worth` 6 (+2.0 over 0.8, 95%) and `cards_first` 1 (+1.4, 95%) ahead; `crowd` 0.6 the best of
three, as before. Cards: Steam Barrier +4.5**, Go for the Eyes +4.1**, Uppercut +3.4**, Shockwave +2.8**, Adrenaline
+2.9** with the deeper runs; Aggregate -4.0**, Overclock -4.0**, Multi-Cast -3.1**, Burning Pact -4.4**, Grand
Finale -4.3**, Unload -3.5**, Rushdown -5.0** with the shallower.

## Earlier: 9,000 runs at Ascension 5 and 0

5,000 runs of Slay the Spire 2 at Ascension 5 and 4,000 of the first game at Ascension 0, 1,000 per character (2026-11-25,
before the A6+ corrections to the model above). Their short version:

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

### Slay the Spire 2, Ascension 5 (5,000 runs)

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

### The first game, Ascension 0 (4,000 runs)

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
| Run | 3 acts (no act 4), Neow, map rules, unknown-room odds, rewards, shops, rest sites, boss relics; ascension rules (A6: start at 90% HP, A5: heal 75% after a boss) | 3 acts, Ancients (heal and a relic choice), the same, boss rewards without relics; the A1-A10 rules (more elites, 80% Ancient heal, less gold, 2 potion slots, Ascender's Bane, dearer removal, half the rares and upgrades, then the monsters' A8-A9 numbers and two act 3 bosses) |
| Simplified | events (a table of typical outcomes, chosen by the bot's event handler), a common subset of relics and potions, no Scry choices | the same; no enchantments, quests or the Fool; Dampen, Galvanic, Infested, Possess, Ravenous and Surprise do nothing |

Each run draws its own value of every setting (`--vary`, the values in `sim/__main__.GRID`: the bot's choices and one
on either side), so a setting's effect is the difference between the runs that drew each of its values; a regression
on every setting at once (and the character) sharpens it a little. With 300 runs a character (1,500 a game) a
setting's effect has a standard error of about 0.5 floors pooled. A rule under test is a setting in `GRID` whose
default is today's behaviour, so `python3 -m sim trace` shows the bot unchanged until it is adopted.

```sh
python3 -m sim run --game sts2 --character all --runs 300 --vary --seed 5000 --ascension 7 --out sts2.jsonl   # base and round 1
python3 -m sim run --game sts1 --character all --runs 300 --vary --seed 6000 --out sts1.jsonl
python3 -m sim prior sts2.jsonl sts1.jsonl --out sts2bot/run/model_cards.json                                # the card table
python3 -m sim run --game sts2 --character all --runs 400 --vary --seed 7000 --ascension 7 --out round2.jsonl # fresh seeds
python3 -m sim run --game sts2 --character all --runs 200 --seed 9000 --ascension 7 --out after.jsonl         # once per version of the bot
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
  It matters as much for the card table (`model_lean`), which is that preference written down card by card: its 4
  floors are the model's, and the live runs, which draw it at 0 or 1, are the test that counts.
