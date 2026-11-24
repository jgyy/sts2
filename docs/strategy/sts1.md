# Slay the Spire (the first game): strategy reference

The bot plays the first game through CommunicationMod (`--game sts1`, see the [README](../../README.md#slay-the-spire-1)).
This page is the game's rules in brief, the community's standing advice, and what that means for a planner that
looks one turn ahead. The model runs in [simulation.md](simulation.md) test the bot's settings on these rules.

## 1. The run

- Four acts: Exordium, The City, The Beyond (17 floors each: 15 rows of rooms, a treasure on the 9th, rest sites on
  the 15th, the boss on 16, 33 and 50), then The Ending (act 4: rest site, shop, the Spire Shield and Spear, the
  Corrupt Heart), open only with all three keys once the Ironclad, the Silent and the Defect have each won a run.
- **Keys**: Ruby (a rest site's Recall instead of resting or smithing), Emerald (the burning elite on the map),
  Sapphire (a chest's key instead of its relic). The bot goes for them (`sts2bot/games/sts1/keys.py`): Ruby only in act
  3 with 70% HP (55% at the last rest site), Emerald whenever its elite is on a reasonable route.
- **Neow** offers four blessings at the start: a card one (remove, upgrade, transform or choose a card), a resource one
  (max HP, gold, a common relic, potions), one with a drawback attached, and swapping the starter relic for a random
  boss relic.
- Map: elites and rest sites never in the first five rows; no rest site on the 14th; no elite, shop or rest site twice
  in a row on a path. Room odds per node roughly: fight 45%, unknown 22%, rest 12%, elite 8% (16% from A1), shop 5%.
- Unknown rooms: fight 10% (+10% each time none comes), shop 3%, treasure 2%, otherwise an event.
- Card rewards: 3 cards; fights 60/37/3% common/uncommon/rare (plus a rare-chance offset that grows by 1% per common),
  elites 50/40/10, bosses all rare. Non-rare rewards upgraded 0/25/50% by act (halved from A12).
- Gold: fight 10-20, elite 25-35, boss ~100. Potions: 40% after a fight, ±10% after each drop or miss; 3 slots (2
  from A11).
- Shop: 5 character cards (one at half price), 2 colorless, 3 relics, 3 potions, card removal 75 +25 each time (50
  flat with Smiling Mask).
- Bosses give a choice of 3 boss relics (energy relics with drawbacks among them) and a rare card reward. After the
  act 1 and 2 bosses you heal fully (75% of missing HP from A5).

### Ascension 1-20

A1 more elites · A2 tougher normal enemies (damage) · A3 elites · A4 bosses · A5 heal 75% after bosses · A6 start at
90% HP · A7 normal enemies' HP · A8 elites' HP · A9 bosses' HP · A10 Ascender's Bane · A11 one potion slot fewer · A12
fewer upgraded cards · A13 less boss gold · A14 lower Max HP · A15 worse events · A16 dearer shops · A17-19 harder move
sets for normal enemies, elites and bosses · A20 a second act 3 boss.

## 2. Principles

**Route.** Act 1 elites are the best relic source and the starter deck is too weak for them on floors 6-7: take two
or three ordinary fights first, then elites when HP is 70%+ with a rest site after. Act 2 is where runs die (Chosen,
Book of Stabbing, Slavers, the Champ): arrive with AoE and a block plan. Act 3 elites only with a finished deck.

**Rest or smith.** Smith above ~60% HP; rest below ~45% or before an elite/boss when below ~80%. The first upgrades go
on cost reductions and on cards that change every fight (Bash, Eruption, Defragment, Footwork, Shockwave).

**Cards.** Act 1: damage first (the starter decks deal ~3 a card), plus one AoE card for Sentries and the Gremlin
Gang. Act 2: scaling (Strength, Dexterity, Focus, Poison, stances) and draw. Act 3: only cards that fix a real gap.
Skip freely; a thin deck plays its best cards more often.

**Shop.** Removal first while Strikes and Defends remain, then a strong relic, then a card that fills a gap. Spend it:
gold left at the end is wasted.

**Boss relics.** Energy is the strongest effect for any deck, but the bot depends on seeing intents, so Runic Dome is
never taken, and Coffee Dripper and Cursed Key are avoided (`text/potions.py` ranks the rest: Black Blood, Ring of the
Serpent, Frozen Core, Holy Water, Philosopher's Stone, Mark of Pain ...).

**Combat.** Lethal first; then survival; Vulnerable and Weak before the hits they change; Strength and other scaling
on turns the enemy does not attack; block only what is coming.

## 3. Elites and bosses

| Act | Enemy | What it does | Plan |
|---|---|---|---|
| 1 | Gremlin Nob (82-86) | Enrage: +2 Strength whenever you play a Skill; Rush 14, Skull Bash 6 + Vulnerable | attacks only; kill in 3-4 turns |
| 1 | Lagavulin (109-111) | asleep 3 turns with Metallicize 8; then 18, 18, Siphon Soul (-1 Str, -1 Dex) | powers while it sleeps, then burst before the second Siphon |
| 1 | 3 Sentries (38-42, Artifact 1) | alternate Beam 9 and Bolt (2 Dazed) | AoE; otherwise kill an outer one first |
| 1 boss | Slime Boss (140) | Goop Spray (3 Slimed), prepare, Slam 35; splits at half HP into two large slimes with its HP | hit it just past half with one big turn |
| 1 boss | Hexaghost (250) | turn 2 Divider (HP/12 + 1) × 6; Sear (Burns), Tackle 5×2, Inflame, Inferno | block the Divider; race the Burns |
| 1 boss | The Guardian (240) | Mode Shift after 30 damage: Defensive Mode (20 Block, Sharp Hide 3 per attack card) | big hits, not many small ones; block Fierce Bash 32 |
| 2 | Gremlin Leader | rallies gremlins, Encourage (+3 Str, Block to all) | kill the leader: the gremlins flee |
| 2 | Book of Stabbing (160-164) | Multi-Stab 6 × (2, 3, 4 ...); Painful Stabs: a Wound per unblocked hit | block every stab; race |
| 2 | Slavers (Blue, Taskmaster, Red) | Wounds, Weak, Vulnerable, Entangle | AoE; kill the Red and Blue first |
| 2 boss | The Champ (420) | Defensive Stance, Face Slap, Taunt; at half HP Anger (clears debuffs, +6 Str) then Execute 10×2 | keep burst for the second half |
| 2 boss | The Collector (282) | Torch Heads (7 each), Fireball 18, Buff, Mega Debuff on turn 4 (3 Weak/Vuln/Frail) | AoE the torches or race the Collector |
| 2 boss | Bronze Automaton (300, Artifact 3) | 2 Bronze Orbs (Stasis steals a card), Flail 7×2, Boost, Hyper Beam 45 then Stunned | block Hyper Beam; kill the Stasis orb to get the card back |
| 3 | Giant Head (500) | Slow (+10% damage taken per card played); It Is Time 30, +5 a turn | race; few big cards |
| 3 | Nemesis (185) | Intangible every other turn; Burns; Scythe 45 | hit on the turns it is solid |
| 3 | Reptomancer | Snake Daggers explode for 25 on their second turn | kill daggers or block their blast |
| 3 boss | Awakened One (300 + 300) | Curiosity (+1 Str per Power you play) in form 1, Regen; reborn with Dark Echo 40, Sludge (Void) | powers before or after, not during form 1; kill the cultists |
| 3 boss | Time Eater (456) | every 12 cards played ends your turn (+2 Str); Haste at half HP | play fewer, bigger cards |
| 3 boss | Donu and Deca (250 each, Artifact 2) | Donu: +3 Str to both; Deca: Dazed, Block to both | kill Donu first |
| 4 | Spire Shield and Spear | Surrounded: the one behind you hits 50% harder | kill the Spear first |
| 4 | Corrupt Heart (750) | Beat of Death (1 per card), Invincible 300 per turn, Blood Shots 2×15, Echo 40+ | sustained damage at about 300 a turn, then block the multi-hits |

## 4. Characters

**Ironclad** (80 HP; 5 Strike, 4 Defend, Bash; Burning Blood heals 6 after each fight). Strength (Inflame, Spot
Weakness, Demon Form, Limit Break, Heavy Blade, Twin Strike), Exhaust (Corruption with Feel No Pain and Dark Embrace,
Second Wind, Burning Pact, Fiend Fire, True Grit), Block (Barricade, Entrench, Body Slam, Impervious, Flame Barrier).
Take: Shockwave, Offering, Feed, Reaper, Demon Form, Impervious, Battle Trance, Uppercut, Carnage, Shrug It Off, Pommel
Strike, Inflame, Whirlwind, Bludgeon. Skip: Clash, Wild Strike, Reckless Charge, Power Through (statuses), Rampage,
Searing Blow. Smith Bash (3 Vulnerable), Shockwave, Limit Break (no Exhaust), Offering.

**Silent** (70 HP; 5 Strike, 5 Defend, Neutralize, Survivor; Ring of the Snake: +2 cards on turn 1). Poison (Noxious
Fumes, Deadly Poison, Bouncing Flask, Catalyst, Crippling Cloud, Corpse Explosion), Shivs (Blade Dance, Cloak and
Dagger, Infinite Blades, Accuracy, After Image, A Thousand Cuts, Finisher), Discard (Tactician, Reflex, Calculated
Gamble, Eviscerate, Concentrate). Defense is what carries the Silent: Footwork, Leg Sweep, Piercing Wail, Backflip,
Blur, Wraith Form, Malaise. Take: Adrenaline, Footwork, Noxious Fumes, Wraith Form, Malaise, Leg Sweep, Piercing Wail,
Backflip, Dash, Catalyst (in a poison deck), Die Die Die, Terror. Act 1 damage: Dagger Spray, Dash, Predator, Poisoned
Stab, Sucker Punch. Skip: Grand Finale, Distraction, Flechettes outside a skill-heavy hand.

**Defect** (75 HP; 4 Strike, 4 Defend, Zap, Dualcast; Cracked Core channels Lightning; 3 orb slots). Focus and Frost
(Defragment, Glacier, Coolheaded, Biased Cognition, Loop, Capacitor), Lightning (Ball Lightning, Electrodynamics,
Tempest, Thunder Strike, Storm, Static Discharge), 0-cost (Claw with All for One, Scrape, FTL), Dark. Take: Echo Form,
Biased Cognition, Defragment, Glacier, Coolheaded, Hologram, Seek, Buffer, Electrodynamics, Machine Learning, Self
Repair, Fusion. Act 1 damage: Ball Lightning, Sweeping Beam, Cold Snap, Beam Cell, Compile Driver. Smith Defragment,
Glacier, Coolheaded, Echo Form (Innate).

**Watcher** (72 HP; 4 Strike, 4 Defend, Eruption, Vigilance; Pure Water adds a Miracle each fight). Stances: Wrath
(double damage dealt and taken), Calm (leaving it gives 2 energy), Divinity (triple damage, +3 energy, ends next turn;
10 Mantra enters it). The plan is to enter Wrath to kill and leave it before the enemy hits: Eruption, Tantrum, Crescendo
in; Empty Body, Empty Fist, Empty Mind, Inner Peace, Vigilance, Fear No Evil out; Rushdown and Mental Fortress pay for
the dance. Also Scry (Foresight, Third Eye, Cut Through Fate, Nirvana, Weave), Retain (Windmill Strike, Sands of Time,
Perseverance, Establishment) and Mantra (Prostrate, Worship, Devotion, Brilliance). Take: Rushdown, Mental Fortress,
Talk to the Hand, Tantrum, Inner Peace, Empty Fist, Empty Body, Wallop, Ragnarok, Lesson Learned, Wish, Scrawl, Vault,
Deva Form, Wheel Kick, Conclude, Fear No Evil. Skip: Fasting (an energy a turn), Blasphemy (the planner reads "die"),
Collect. Smith Eruption (cost 1) first. Never end a turn in Wrath in front of an attack unless it dies.

## 5. How the bot plays it

- Card text comes from `sts2bot/games/sts1/text/` (CommunicationMod sends none); the hand shows live numbers
  (Strength, Weak, Dexterity, Frail, stance) through the bot's own mod.
- The planner knows stances (`games/sts1/stance.py`): a stance change inside a turn, the energy it gives and the hit it
  doubles. It does not plan Mantra or Scry; it plays those cards for what they do outright.
- The hand-made card ratings and the crowd's win rates are Slay the Spire 2's, so here cards are rated from their text
  and corrected by `learn.py` from the bot's own runs.
- CommunicationMod cannot resume a run: a crash starts a new one.
