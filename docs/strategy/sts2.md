# Slay the Spire 2: strategy reference

For the main branch, **v0.111.1** (Major Update 3, 2026-11-17); beta-only changes (v0.113) are marked. Numbers
come from the game files as extracted by Spire Codex and from the spire-mentor combat port; advice is community
consensus from the guides listed at the end. Where the bot has measured something itself, its number wins (the
comments in `sts2bot/` quote them), and the model runs in [simulation.md](simulation.md) are the first test of any
rule before it goes live.

## 1. The run

| Act | Map rooms | Floors (Ancient + rooms + boss) | Weak-pool fights | Bosses (one of) |
|---|---:|---:|---:|---|
| 1 Overgrowth / Underdocks (50/50 once unlocked) | 15 | 17 | 3 | Overgrowth: Ceremonial Beast, The Kin, Vantom. Underdocks: Lagavulin Matriarch, Soul Fysh, Waterfall Giant |
| 2 Hive | 14 | 16 | 2 | Kaiser Crab (Crusher + Rocket), Knowledge Demon, The Insatiable |
| 3 Glory | 13 | 15 | 2 | Queen (+ Torch Head Amalgam), Test Subject, Aeonglass |

- Bosses are on floors 17, 33 and 48. Every act opens with an **Ancient** that heals you (fully at A0-A1, 80% of
  missing HP from A2) and offers relics: Neow in act 1; Orobas, Pael or Tezcatara in act 2; Nonupeipe, Tanx or Vakuu
  in act 3; Darv (classic boss relics: Astrolabe, Black Star, Calling Bell, Empty Cage, Runic Pyramid, Snecko Eye,
  Sozu, Velvet Choker, Philosopher's Stone, Pandora's Box, Ectoplasm) in act 2 or 3. **Bosses drop no relic**: gold, a
  potion roll and a rare card reward.
- Map: 7 columns; first row fights, a treasure 7 rows from the end, the last row rest sites; no elites or rest sites
  in the first 5 rows (4 on the beta). Per act: 5 elites (8 from A1), 3 shops, 6-7 rest sites (5-6 in act 3), 9-14
  unknown rooms.
- Unknown rooms: fight 10%, shop 3%, treasure 2%, else an event; each outcome that misses grows by its base chance,
  the one that hits resets; all reset at each act.
- Card rewards: 3 cards. Normal fight 60/37/3% common/uncommon/rare, elite 50/40/10, boss all rare; a hidden offset
  starts at -5% and grows by 1% per non-rare card. Upgraded cards: act 1 0%, act 2 25%, act 3 50% (halved at A7).
- Gold: fight 10-20, elite 35-45, boss 100, chest 42-52 (×0.75 from A3). Potions: 40% after a fight (+12.5% from an
  elite), -10% after a drop and +10% after a miss; 3 slots (2 from A4).
- Shop: 5 character cards (one at half price), 2 colorless, 3 relics, 3 potions, card removal 75 +25 each time (100
  +50 from A6). Cards 50/75/150, relics 175/225/275, potions 50/75/100.
- Rest site: rest (30% of Max HP) or smith; relics add Lift (Girya), Dig (Shovel), Cook (Meat Cleaver), Kindle
  (Pumpkin Candle), Clone (Pael's Growth), Hatch (the Byrdonis egg).

### Ascension

| A | Effect | What it changes for the bot |
|---|---|---|
| 1 | 8 elites per act instead of 5 | elites are harder to route around; more relics on offer |
| 2 | Ancients heal 80% of missing HP | the act transition no longer resets HP; pre-boss rests matter more |
| 3 | 25% less gold | about one purchase fewer per run |
| 4 | 2 potion slots | drink sooner; a full belt wastes drops |
| 5 | Start with Ascender's Bane (Unplayable, Ethereal, Eternal) | one dead draw per cycle |
| 6 | Card removal 100 +50 | the second removal is usually worse than a relic |
| 7 | Rares and upgraded cards half as common | build on commons and uncommons; smith more |
| 8 | Enemy HP up (`hp_a8`) | fights last about a turn longer |
| 9 | Enemy damage up (`damage_a9`) | more Block needed; Weak worth more |
| 10 | Two act 3 bosses back to back | arrive with full HP and a full belt; sustained damage beats burst |

## 2. Principles

**Route.** Plan the whole act backwards from the boss. Act 1: several ordinary fights first (the deck needs cards),
then elites late in the act from a healthy position (65%+ HP), ideally with a rest site after them. Act 2: elites
only with AoE and some scaling. Act 3: elites rarely (boss-level stats, little time left to use the relic). Shops
are worth a detour with 150+ gold; gold left at the end of the run is worth nothing.

**Rest or smith.** Smith from about 65% HP, rest below 40%, and in between rest when an elite or the boss comes
before the next rest site. Before the boss rest unless above about 80-90% (act 2 and 3 bosses punish every missing
point). Upgrade the card that changes a fight most: a cost reduction, the scaling power, the most played attack.

**Card rewards.** Act 1 wants front-loaded damage (the starter decks deal about 3 a card), then one or two AoE cards
and a reliable block card. Act 2 wants scaling and 15-20 Block on demand; act 3 sustained damage over 8-12 turns. Take
fewer cards as the deck grows (skip most act 3 rewards); thin decks find their best cards more often.

**Shop.** Card removal first (Strikes, then Defends once the deck blocks well; curses always), then a strong relic,
then a card that fills a missing role, then potions with the boss near. From A6 the second removal rarely beats a
relic.

**Potions.** Drink permanent ones at once (Fruit Juice), scaling ones on turn 1 of elites and bosses, defensive ones
on the turn they prevent the most damage. Never die holding a potion; never sit on a full belt. Keep the best two for
the act boss (bosses met with two potions were beaten about twice as often as with one).

**Combat.** Lethal first; then don't die; then Vulnerable/Weak before the hits they modify; block only what is
coming. Against enemies that scale (Strength every turn, Ritual) damage now is worth more than Block. On Intangible
or capped turns (Soul Fysh, Skulking Colony, Test Subject) spend energy on Block and set-up.

### Slay the Spire 2 mechanics

- **Sly** (Silent): a Sly card discarded from the hand during your turn is played for free.
- **Doom** (Necrobinder): at the end of an enemy's turn, if its Doom is at least its HP it dies. Doom ignores Block,
  Slippery and Intangible. End of Days kills at once. Self-Doom (Neurosurge) kills you the same way.
- **Osty** (Necrobinder): Summon N adds N HP to Osty; attacks hit Osty after your Block and before your HP; at 0 HP
  he dies and returns at 1 HP next turn (Bound Phylactery: Summon 1 every turn). Osty attacks do nothing while he is
  dead.
- **Souls**: 0-cost token, draw 2 (3), Exhaust.
- **Stars and Forge** (Regent): Stars persist through a fight; Forge N creates the Sovereign Blade (2 energy, 10
  damage, Retain) or adds N damage to it for the fight.
- **Orbs** (Defect): Lightning, Frost, Dark, Plasma as in the first game, plus Glass (damage to all enemies).
- **Enchantments** stay on a card for the run (don't remove an enchanted card); **afflictions** are put on cards by
  enemies for a fight (the Queen's Chains of Binding, the Ceremonial Beast's Ringing).
- **Plating**: Block at the end of each turn, 1 less each turn; **Slippery**: the next N hits do 1 damage each;
  **Hardened Shell**: HP lost per turn is capped.

## 3. Elites and bosses

| Act | Enemy | HP (A8+) | What it does | Plan |
|---|---|---|---|---|
| 1 | Bygone Effigy | ~127 | Slow; wakes, then slashes and buffs | use the sleep turns for set-up, then burst |
| 1 | Byrdonis | 81-94 | Territorial: gains Strength every turn; Peck 3×3, Swoop | race it; Weak on its attack turns |
| 1 | Phrog Parasite | 61-64 | Shuffles Infections into the deck; Wrigglers when it dies | keep AoE and Block for the Wrigglers |
| 1 | Skulking Colony | 75-79 | Hardened Shell caps HP lost per turn | spread damage over turns, block the rest |
| 1 | Terror Eel | 140 | 25+ early; Shriek stuns it at a threshold | block early, Weak; push it past the threshold |
| 1 | Phantasmal Gardeners | 28-32 each | Skittish: Block the first time hit each turn | AoE |
| 1 boss | Ceremonial Beast | 252 (262) | Stamp gives Plow 150: Plow 18 +2 Str every turn; below 150 HP it is stunned and loses its Strength, then Beast Cry (Ringing: one card a turn), Stomp 15, Crush 17 +3 Str | set up, then cross 150 HP in one big turn; under Ringing play the single best card |
| 1 boss | The Kin | Priest 190 (199) + 2 Followers | Priest: 8 + Frail, 8 + Weak, 3×3, +2 Str. Followers buff and hit | AoE; the bot's model fights say Followers first (92% vs 31% won) |
| 1 boss | Vantom | 173 (183) | Slippery 8 (9); Ink Blot 7, Inky Lance 6×2, Dismember 26 + 3 Wounds, +2 Str | strip Slippery with small hits, block Dismember |
| 1 boss | Lagavulin Matriarch | 222 (233) | Asleep 3 turns behind 12 Plating (1 less each turn); Slash 19, Disembowel 9×2, 12 + 12 Block, +2 Str | powers while she sleeps, then end it fast |
| 1 boss | Soul Fysh | 211 (221) | Beckon cards (6 HP if held), De-Gas 16, Gaze 7, Fade (Intangible 2), Scream 13 + 3 Vulnerable | play or discard Beckons; block on Intangible turns |
| 1 boss | Waterfall Giant | 240 (250) | Builds Steam Eruption every turn; when killed it stands one more turn and explodes for it | keep Block for the turn after the kill |
| 2 | Decimillipede | 3 segments ~40-52 | Segments Reattach two turns after dying while another lives | bring all low, kill them together |
| 2 | Entomancer | 145 (165) | Personal Hive: a Dazed into the draw pile per Attack that hits it; Beeeees! 3×7, Spear 18 | few big attacks, non-Attack damage |
| 2 | Infested Prisms | ~200 | Vital Spark: each Skill you play makes its hits stronger this turn | attacks over skills on its attack turns |
| 2 boss | Kaiser Crab | Crusher 209 (219), Rocket 199 (209) | Back Attack: the one behind you hits 50% harder; Crab Rage: when one dies the other gains 6 Str and 99 Block; Rocket's Laser 31 | bring both low, kill them close together; Rocket's Laser is the hit to block |
| 2 boss | Knowledge Demon | 379 (399) | Curse of Knowledge three times (an affliction pick: Disintegration or Mind Rot / Sloth / Waste Away), Slap 17, 8×3, Ponder 11 + heal 30 + Str | race; take the affliction the deck minds least |
| 2 boss | The Insatiable | 321 (341) | Sandpit 4: you are eaten when it runs out; Frantic Escape cards push it back; Thrash 8×2, Lunging Bite 28 | a damage race; play Escapes when the timer is short |
| 3 | Knights (Flail, Magi, Spectral) | ~276 total | Magi Knight downgrades cards and hits 35 every third turn; Spectral makes cards Ethereal | Magi Knight first (the bot's model: 20-45 HP saved) |
| 3 | Mecha Knight | ~300 | Flamethrower: 4 Burns into the hand and 8 (12) damage | block and race |
| 3 | Soul Nexus | — | summons | AoE |
| 3 boss | Queen | 400 (419) + Amalgam 199 | Puppet Strings (Chains of Binding), You Are Mine (99 Weak/Frail/Vulnerable), then a 3×5 / 15 / +2 Str cycle | kill the Amalgam first in defensive decks |
| 3 boss | Test Subject | three forms, ~100 / 200 / 300 | Form 1: Enrage (Str per Skill), Bite 20, Skull Bash 14 + Vulnerable. Form 2: Painful Stabs, Multi-Claw 10 × 3 hits, one more each round. Form 3: Nemesis (Intangible on its first round and every other), Lacerate 10×3, Big Pounce 45, Burns + Str. Strength resets between forms; a form killed does not attack | few Skills in form 1; Block for form 2's growing claws; damage on the turns it is not Intangible |
| 3 boss | Aeonglass | 512 (535) | Artifact 3; Withering Presence: a Wither into the hand every 6 cards played (3 damage at end of turn, 3 more for each buff); Ebb 22 + 33 Block, Eye Lasers 11×2, Increasing Intensity (a Wither, every Wither a level up, Str 3 then 4, 5 ...) | end it fast; fewer cards played means fewer Withers |

## 4. Characters

In the bot's model runs ([simulation.md](simulation.md)) the Silent wins most (8.7% at A5), then the Ironclad and
Defect (6-7%); the Regent and Necrobinder rarely get past the act 2 boss. The cards that went with deeper runs for each
character are listed there.

**Ironclad** (80 HP; 5 Strike, 4 Defend, Bash; Burning Blood heals 6 after each fight). The healing relic makes HP a
resource: fight more, smith more. Archetypes: Exhaust (Burning Pact, Stoke, True Grit, Fiend Fire, Havoc with Dark
Embrace, Feel No Pain, Corruption, Evil Eye, Ashen Strike), Strength (Inflame, Demon Form 3 (4) since v0.111, Rupture,
Dominate with multi-hits), Vulnerable (Bash, Thunderclap, Taunt, Molten Fist, Uppercut, Colossus), Block (Crimson
Mantle, Unmovable, Barricade, Impervious, Shrug It Off). Take: Offering, Unmovable, Crimson Mantle, Dark Embrace,
Impervious, Colossus, Evil Eye, Dominate, Pommel Strike, Shrug It Off, Uppercut, Thunderclap, Bloodletting (now
uncommon), Inflame. Skip: Cinder, Havoc, Body Slam without a block engine, Demon Form in act 1. Few Weak sources: value
Uppercut and Intimidate. Hard fights: Kaiser Crab, The Insatiable, the Queen.

**Silent** (70 HP; 5 Strike, 5 Defend, Neutralize, Survivor; Ring of the Snake draws 2 more on turn 1). Discard/Sly is
the strongest plan (Prepared, Acrobatics, Survivor, Dagger Throw, Calculated Gamble, Tools of the Trade, Expertise
with Tactician, Reflex, Untouchable, Flick-Flack, Master Planner); Shivs (Cloak and Dagger, Infinite Blades, Blade
Dance, Knife Trap with Accuracy, Finisher, Fan of Knives, Afterimage, Serpent Form); Poison is weakest (Noxious Fumes,
Deadly Poison, Accelerant, Outbreak, now a 3-cost rare that triggers at once). Take: Adrenaline, Acrobatics, Backflip,
Piercing Wail, Footwork, Wraith Form, Afterimage, Leg Sweep, Malaise, Prepared, one early AoE (Echoing Slash,
Flick-Flack, Dagger Spray). Skip: Impatience, Envenom outside a Shiv deck, Snakebite, Poison without Fumes. Remove
Strikes hard; discard Sly cards first. v0.111: Expertise draws 2 (3) with Retain, Well-Laid Plans is a rare power that
keeps the hand, Tracking +50%.

**Defect** (75 HP; 4 Strike, 4 Defend, Zap, Dualcast; Cracked Core channels a Lightning; 3 orb slots). Lightning (2-3
generators: Lightning Rod, Ball Lightning, Storm, Tempest, Thunder, Voltaic, Shatter), Dark (Darkness with Capacitor),
Frost as the defensive layer (Glacier, Coolheaded, Chill, Cold Snap, Coolant), 0-cost Claw only with All for One.
Engines: Echo Form, Machine Learning, Defragment, Biased Cognition (5 (6) Focus since v0.111), Hologram, Skim, Buffer,
Capacitor. Take damage first in act 1 (aim for 30 a turn at the act 1 boss). Skip: Meteor Strike, Chaos, Bulk Up, Claw
without payoff. Don't remove Defends early. The lowest win rate of the roster; dies most to the Knowledge Demon and the
Queen.

**Necrobinder** (66 HP; 4 Strike, 4 Defend, Bodyguard, Unleash; Bound Phylactery summons 1 every turn). Souls (Grave
Warden, Capture Spirit, Severance, Dirge with Haunt, Devour Life, Soul Storm, Death March), Osty (Bodyguard, Pull
Aggro, Spur, Reanimate; Poke, Flatten, Sic 'Em, Rattle, Unleash; Necro Mastery, Sacrifice), Doom (Scourge, Negative
Pulse, No Escape, Deathbringer, End of Days, Time's Up). Take: Grave Warden, Scourge, Debilitate, Borrowed Time,
Friendship (not in Osty-attack decks: it costs Strength), Glimpse Beyond, Eradicate, Death's Door. Skip: Afterlife,
Reap, Sow, Bury, Parse, pure draw. Play cheap Osty attacks before Flatten, Sic 'Em before the other Osty attacks,
Summon before Unleash, Souls first. Low Max HP: rests heal little, value Block.

**Regent** (75 HP; 4 Strike, 4 Defend, Falling Star, Venerate; Divine Right gives 3 Stars each fight). Stars (Glow,
Venerate, Gather Light, Solar Strike, Genesis, Alignment and Resonance now 2 Stars; payoffs Child of the Stars, Black
Hole, Seven Stars, Particle Wall, Cloak of Stars, Gamma Blast, Comet) is the consistent plan; Forge (Wrought in War,
Bulwark, Refine Blade, Spoils of Battle, Parry, Summon Forth, Seeking Edge) ramps the Sovereign Blade; Void Form and
Tyranny cheat energy. Pick few, strong cards; keep Star makers and Star spenders in balance (the bot measured that
decks short of Stars lose); upgrade Glow first; remove one Strike and one Defend. Skip: extra Falling Stars, Photon
Cut, Pillar of Creation without card creation.

**The Fool** (beta v0.113 only; 77 HP; 5 Strike, 4 Defend, Foolish Shot; Song Bow gives 6 Aim each fight): Arrows
that impale enemies, Aim and Stretch, and the Wah/Cho/Ki Melodies. Sadness ("Unplayable. Retain.") from Sorrowful Strike,
Mourning Blow, Jest, Vitriol, Rummage and Ennui stays in the hand all fight and the hand holds 10: past five or so, every
one is a card not drawn each turn, until Punch of Joy or Ecstasy turns it into Joy (a boss was lost on a hand of ten
Sadness). Take the makers only with Joy to match; play Ennui in short fights. The bot plays it (`sts2bot/fight/fool/`,
Sadness in `sadness.py`); the model runs do not yet.

## 5. Recent changes

**Major Update 3 (v0.108-v0.111, main since 2026-11-17).** Neow adds Neow's Sacrifice and Dowsing Rod; Darv offers
energy relics more often. Orb passives and Regen now resolve before Doom. Ironclad: Demon Form 3 (4), Mangle 20 (26),
Howl From Beyond 18 (24), Expect a Fight reworked, Colossus and Crimson Mantle nerfed, Taunt now common. Silent:
Outbreak, Well-Laid Plans, Expertise, Haze and Sidestep reworked; Tracking nerfed; Accelerant and Echoing Slash
uncommon. Regent: Alignment and Resonance cost 2 Stars, Devastate 35 (45), Guiding Star 1 Star; Pillar of Creation,
Collision Course and Refine Blade nerfed. Necrobinder: Eidolon reworked, Haunt 7 (9), Sacrifice triple Max HP, Time's
Up no longer Exhausts. Defect: Hyperbeam 24 (30) and Focus loss for the turn only, Biased Cognition 5 (6), Sunder 26
(34), Thunder 8 (11); Null and Compact nerfed. Colorless Rend 1 cost 10 (12). Enemies: Aeonglass's Ebb 22 (26), Axebot
hits harder and gains Max HP on respawn, Mecha Knight's Flamethrower deals damage.

**Beta v0.113.** The Fool (sixth character), Blitz mode (a shorter map, its own 5-level ascension) and Dread mode
(every modifier stacked, boss "Dominion" effects), rest sites one row earlier.

## 6. Sources

Spire Codex (game data, github.com/ptrlrd/spire-codex) · spire-mentor (combat port, github.com/tfriedel/spire-mentor) ·
slaythespire.wiki.gg · Steam patch notes for app 2868840 · tier lists and guides: Tierspire, nat1gaming, Mobalytics,
PCGamesN, Untapped, MetaBot, LINCE and oltre (note.com, A10), XecnaR's Regent guide, Vile's Steam guide.
