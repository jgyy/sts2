# Slay the Spire 2 — character-specific strategy notes for a rule-based bot

Compiled 2026-11-20. Game state referenced by sources: early access, patches v0.99 – v0.113 (most tier data v0.104 – v0.111; a beta patch dated "November 12" changed several cards).

## 0. How to read this file (provenance and confidence)

- **Access limitation.** This session could not open web pages directly (every host except the search API was blocked: mobalytics.gg, pcgamer.com, wiki.gg, reddit.com, steamcommunity.com, untapped.gg all failed to resolve). Everything below comes from **search-engine excerpts/summaries of the listed pages**, not from full-page reads. Numbers and exact card text are therefore second-hand; treat any number as "check against in-game text".
- **Tags used.**
  - `[STS2]` = stated by at least one STS2-specific source excerpt.
  - `[STS2 x2+]` = stated consistently by two or more independent STS2 sources.
  - `[CONFLICT]` = sources disagree (both versions given).
  - `[STS1 principle]` = carried over from the first game; not confirmed for STS2 by a source.
  - `[unverified]` = single weak source, or my inference.
- **Source reliability tiers** (use this to weight anything that conflicts):
  - Tier A (player-written, high ascension, or data): slaythespire.wiki.gg, nat1gaming.com (A10 guides, tier lists updated for the Nov 12 patch), Baalorlord tier lists on sts2.untapped.gg (v0.111.0), tierspire.com / spire-codex.com / untapped.gg (win-rate data), note.com A10 write-ups (LINCE, oltre, LW, hikki, XecnaR summary), PC Gamer A10 Silent article, PCGamesN, Steam discussion threads for app 2868840.
  - Tier B (mainstream guide sites, early-access launch era, may be stale): TheGamer, Destructoid, GameRant, Keengamer, Mobalytics, IntoIndieGames, DualShockers.
  - Tier C (SEO/possibly auto-generated; contain visible errors such as "Ascension 18+", STS1 bosses, or Watcher mechanics): slaythespire-2.com, sts2guides.com, sts2front.com, stratgg.com, switchbladegaming.com, slaythespire2.space, stswiki.com and similar. Used only where nothing better existed, and tagged.
- **Card names.** Every card/relic/enemy name below appeared in a source excerpt, with four exceptions taken from the bot's own `sts2bot/cards.py` table rather than from a web source: Darkness, Tempest, Creative AI, Seeking Edge (their roles here are `[unverified]`). "Sword Polish", "Hang"/"Hangman" and "Hate Generation" come from machine-translated Japanese notes and may not be the English card names. A name appearing here is not a guarantee it exists in the current patch. Names that also exist in the bot's own `sts2bot/cards.py` table are the safest.

## 0.1 Cross-character facts (confirmed roster, ascension, encounters)

- **Roster `[STS2 x2+]`:** five playable characters — Ironclad, Silent, Defect (returning, reworked), Regent, Necrobinder (new). No sixth character found in any source. Ironclad needs no unlock.
- **Ascension `[STS2 x2+]`:** 10 levels in early access, cumulative. Names/effects per Spire Codex and others:
  - A1 Swarming Elites (more elites on map); A2 Weary Traveler (Ancients heal only 80%); A3 Poverty (less gold); A4 Tight Belt (one fewer potion slot); A5 Ascender's Bane (start with a curse); A6 Inflation (card removal costs more — one source: starts 100 gold, +50 per removal instead of 75 / +25); A7 Scarcity (rare and upgraded cards about half as common in rewards, slower pity); A8 Tough Enemies (more enemy HP); A9 Deadly Enemies (more enemy damage); A10 Double Boss (two bosses at the end of the final act).
  - `[CONFLICT]` Some Tier C sites describe 20 levels or "A15+/A18+" advice; ignore those. One source also claims starter relics upgrade and starting HP drops at "Ascension 2+" (Ring of the Drake, Ironclad 64 HP) — unverified, contradicts the modifier list above.
  - Bot implication at A4–A5 (the current level): one fewer potion slot (use potions sooner rather than hoard), a starting curse (removal priority #1 if it is a real curse card), less gold, weaker Ancient healing.
- **Act structure / enemy names `[STS2 x2+]` (wiki + Mobalytics + Spire Codex):**
  - Act 1 Overgrowth — bosses: Vantom, Ceremonial Beast, The Kin; elites: Bygone Effigy, Byrdonis, Phrog Parasite.
  - Act 1 Underdocks — bosses: Waterfall Giant, Soul Fysh, Lagavulin Matriarch; elites: Phantasmal Gardener (four Gardeners), Skulking Colony, Terror Eel.
  - Act 2 Hive — bosses: The Insatiable, Knowledge Demon, Kaiser Crab; elites: Decimillipede, Entomancer, Infested Prism.
  - Act 3 Glory — bosses: Queen, Test Subject, Aeonglass (replaced the older Doormaker); elites: Knight gang (Flail Knight, Spectral Knight, Magi Knight; ~276 total HP), Mecha Knight (~300 HP), Soul Nexus (~234 HP).
- **Generic encounter rules `[STS2]` (mostly Tier B/C boss guides; consistent with each other):**
  - Bygone Effigy: asleep turn 1 (free setup turn: play powers/draw), then kill within 1–2 turns of waking.
  - Byrdonis: Strength-stacking damage race; apply Weak, burst it.
  - Phrog Parasite: adds status cards; AoE helps.
  - Skulking Colony: "Hardened Shell" ability makes it one of the trickier Act 1 elites (mechanic not described in excerpts).
  - Vantom: has Slippery — use cheap or multi-hit attacks to strip it; hold block for its turn-3 hit; push damage while it buffs (turn 4).
  - Ceremonial Beast: phase change at 150 HP gives a stun turn — only cross the threshold when the hand can exploit the stun as a burst turn.
  - The Kin: kill the two Kin Followers first (one source says treat as DPS race).
  - Waterfall Giant: long fight (~10 turns of block needed); scaling block/powers beat one-shot setups.
  - The Insatiable: Sandpit countdown ticks every turn = hard turn timer; blocking instead of killing loses; plan single-digit-turn kill, concentrate Vulnerable/Strength into burst turns.
  - Knowledge Demon: deadliest Act 2 boss in death data (22% of attempts; Defect 26%); kill speed beats passive defense; stack Vulnerable, spend potions.
  - Kaiser Crab: ends ~20% of attempts, worst for Ironclad; has a Block problem that Poison bypasses; in phase 1 attack-heavy turns are better because skill-heavy turns "feed the boss".
  - Queen: minions plus boss; punishes front-loaded decks (Ironclad and Defect die most); race only if minion pressure is handled (AoE).
  - Test Subject: longest boss fight (avg 9.3 turns), multi-phase pure scaling check.
  - Aeonglass: described as a "Focus breakpoint"/burst check; at A10 there is a 2/3 chance of facing it (two of three Act 3 bosses). Mechanics not found in excerpts.
  - General: bosses run a fixed repeating cycle; save premium block for confirmed heavy-hit turns and attack/setup on buff turns. Act 1 elites commonly hit 20+ on turn 2.
- **Generic deck rules `[STS2]` unless noted:**
  - Act 1 is the hardest stretch at high ascension; take front-loaded damage early, fight often, and do not lock into an archetype before the end of Act 1 (nat1 / Steam / PC Gamer agree).
  - PC Gamer A10 priority order (written for Silent but stated as general): (1) early damage plus 1–2 AoE cards, (2) good block, (3) card draw / discard. "Three-energy powers are way too costly at Ascension 10."
  - Elites: 1–2 in Act 1 for most characters, fight at >= ~60% HP with at least one good damage card; a campfire directly before an elite is an upgrade opportunity. Some A9+ players prefer elites over hard-pool hallway fights.
  - Rest sites: upgrading generally produces more value than healing; rest only when the next fight is likely lethal (one guide: Ironclad below ~30 HP before an elite).
  - Skipping a card reward is often correct, especially after Act 1. Deck-size targets quoted: ~20–25 cards at the Act 1 boss (Tier C), thinner for combo characters.
  - Removal: curses first, then Strikes before Defends in most cases `[STS2 + STS1 principle]`. Character exceptions are listed per character below.
  - Colorless cards rated top by data/expert lists: Hidden Gem (nat1: best colorless), Master of Strategy, Mayhem, Jackpot, Nostalgia, Apotheosis, Secret Technique, Shockwave, Extend, Pugilism.
  - Potions: Fairy in a Bottle (heal 30% max HP instead of dying) is the universal top potion. Use potions on elites that would otherwise end the run; otherwise hold for the boss.
  - Debuff values (Vulnerable +50%, Weak -25%) are `[STS1 principle]`; Paper Phrog raising Vulnerable to 75% is `[CONFLICT]` (another source says 100%).

---

## 1. Ironclad

### 1.1 Kit
- Start: 80 HP, 3 energy `[STS2 x2+]`. Deck (10): 5x Strike (6 dmg), 4x Defend (5 block), 1x Bash (damage + Vulnerable; one guide says upgraded Bash gives 3 turns) `[STS2 x2+]`.
- Starter relic: Burning Blood — heal 6 HP at end of combat `[STS2 x2+]`. Can be replaced by Black Blood (heal 12) through a relic-swap event `[STS2]`.
- Core mechanics: Strength scaling, Vulnerable, Exhaust synergies, and paying HP for value ("self-damage") `[STS2 x2+]`. Weaknesses noted by Mobalytics: almost no Weak access apart from Uppercut, no Retain.
- The healing relic means HP is a resource: Ironclad can fight more often and upgrade instead of resting in Act 1 `[STS2]`.

### 1.2 Archetypes
- **Exhaust engine (rated best by most Tier A sources).**
  - Enablers: Burning Pact, Stoke (exhaust hand, then draw that many — reworked in v0.103.2), Second Wind, Fiend Fire (exhaust hand, 7 dmg per card), True Grit, Havoc (play top card of draw pile and exhaust it), Brand (thins Strikes).
  - Payoffs: Dark Embrace (draw on exhaust), Feel No Pain (block on exhaust; "most reliable block engine if you have enough exhaust cards"), Evil Eye (8 block, +8 more if a card was exhausted this turn), Ashen Strike (6 dmg +3 per card in exhaust pile), Pyre (`[CONFLICT]`: energy each turn vs. 3 energy if you exhausted this turn), Corruption.
  - Energy: Bloodletting (0 cost, lose 3 HP, gain energy; a common, "the first good energy generation you see"), Offering.
  - Tier C sources describe "Burning Pact + Bloodletting" infinite loops; a bot should not try to pilot infinites, but the same cards are good individually.
- **Strength.**
  - Enablers: Inflame (+2 Strength, cheap, best early scaling), Demon Form (slow; data says it performs much better when added in Act 2 than in Act 1), Dominate (1 energy, Exhaust, gain Strength equal to target's Vulnerable stacks), Rupture (gain 1 Strength whenever you lose HP on your turn), Limit Break (Tier C mention only).
  - Payoffs: multi-hit attacks — Twin Strike, Sword Boomerang, Whirlwind, Thrash (4 dmg x2, can exhaust an attack in hand to add its damage), Heavy Blade (Tier C mention).
  - Relics: Vajra (+1 Strength per combat), Ruined Helmet (doubles the first Strength gain each combat), Sword of Jade (3 Strength), Sling of Courage.
- **Vulnerable.** Bash, Thunderclap (AoE), Taunt (block + Vulnerable; "the only block card that applies Vulnerable"), Molten Fist (10 dmg, doubles target's Vulnerable), Uppercut, Colossus (block + halve damage taken from Vulnerable enemies this turn). Relics: Paper Phrog, Red Mask (Vulnerable turn 1), Bag of Marbles.
- **Block / self-damage.** Crimson Mantle (rare power: lose 1 HP per turn, gain 8 (10) block per turn; top pick-lift card in MetaBot data), Unmovable (rare power, 2 cost / 1 upgraded: first block gained from a card each turn is doubled; does not double relic/power block), Barricade, Impervious, Shrug It Off (block + draw 1), Blood Wall (2 cost, lose 2 HP, 16 block), Body Slam, Juggernaut. Community-data build: Bloodletting/Offering/Pyre for energy with a block core of Colossus, Shrug It Off, Body Slam.

### 1.3 Card tiers
- **Top across data and expert lists `[STS2 x2+]`:** Offering (highest score on Spire Codex; picked ~66% when offered), Unmovable, Crimson Mantle, Dark Embrace, Impervious, Stoke, Colossus, Evil Eye, Dominate, Fiend Fire, Battle Trance, Feel No Pain, Corruption, Demon Form (Act 2+), Barricade.
- **Baalorlord S tier (v0.111.0):** Offering, Unmovable, Break, Colossus, Evil Eye, Dominate, Dark Embrace (Fiend Fire, Stoke, Thrash near the top; Feel No Pain A).
- **Good Act 1 picks `[STS2 x2+]`:** Pommel Strike, Headbutt, Shrug It Off, Uppercut, Whirlwind, Thunderclap, Molten Fist, Break, Breakthrough (lose 1 HP, 9 dmg to ALL), Inflame, Taunt, Twin Strike, Bloodletting, Tremble ("powerful early pickup that scales").
- **Block picks worth taking (be selective):** Shrug It Off, Taunt, Colossus, Evil Eye; skip most other plain block cards early (Steam A10 advice).
- **Traps / usually skip:** Cinder and (per Spire Codex win rate) Break at ~31% win rate `[CONFLICT with Baalorlord rating Break S]`; Havoc (4% pick rate), Body Slam (8% pick rate; PCGamesN nevertheless rates it S — only good with a real block engine) `[CONFLICT]`; SpireForge flags Anger, Sword Boomerang, Tank as low. Tank is a co-op card (allies take half damage): do not take solo. Demon Form in Act 1 and any 3-cost power at high ascension.
- **Rule:** weight Weak sources higher than usual (Ironclad has few) — the note.com A10 author values Weak against most Act 1 elites and recommends Intimidate as a Strength-scaling pick.
- **Upgrade priority:** Bash first is the common advice (Tier C, but matches `[STS1 principle]`); then key engine pieces — Unmovable (cost 2 -> 1), Crimson Mantle (8 -> 10 block), Inflame/Dominate, Shrug It Off, Whirlwind/AoE. `[unverified ordering]`.

### 1.4 Removal and deck size
- Strikes first `[STS2]`; Brand and exhaust effects thin them in combat. One Tier C source says keep Strikes in Strength decks because they scale — lower weight; `[STS1 principle]` still says Strikes go first.
- Compact deck with "fast Act 1 damage, enough block to survive setup, and one engine" (MetaBot guide). No explicit size given; `[STS1 principle]` 20–28 cards.

### 1.5 Combat tactics
- Turn 1 Bash on the highest-HP target, then spend attacks into Vulnerable `[STS1 principle]`. Play Molten Fist after Vulnerable is applied; play Dominate after stacking Vulnerable and before the attack burst.
- Play scaling powers on enemy non-attack turns; with Anchor/Horn Cleat-style relics the first turns are free setup `[STS2]`.
- HP-cost cards (Bloodletting, Offering, Breakthrough, Blood Wall, Crimson Mantle) are safe when the fight ends soon because Burning Blood refunds 6; forbid them when incoming damage plus the self-loss would be lethal. Self-damage also triggers Rupture and relics such as Centennial Puzzle and Self-Forming Clay.
- With Evil Eye in hand, exhaust something first. With Unmovable, play the largest block card first each turn.
- Colossus only matters when the attacker is Vulnerable — apply Vulnerable before blocking with it.

### 1.6 Relics and potions
- Relics: Self-Forming Clay and Ruined Helmet top Spire Codex data (~64% win rate); Fiddle and Beautiful Bracelet top MetaBot's list; Vajra, Bag of Marbles, Paper Phrog, Red Mask, Anchor, Horn Cleat; Joss Paper and Charon's Ashes for exhaust; Kunai is near the bottom for Ironclad. Energy relics matter most because key cards are expensive (Demon Form, Impervious, Offering).
- Potions: Strength Potion, Cultist Potion, burst damage `[STS2, Tier C]`.

### 1.7 Hard matchups
- Kaiser Crab (Act 2) is Ironclad's worst boss by death share; needs attack-dense turns in phase 1 and block-piercing damage.
- The Insatiable punishes slow Strength setups (turn timer).
- Queen punishes front-loaded decks with no scaling.
- A note.com A10 boss tier list rates Ironclad's hardest bosses as ones answered by "draw, retain, loop"; the same author rates Kaiser Crab as the top damage dealer overall. Byrdonis and other Act 1 elites are much easier with Weak.

### 1.8 Higher ascension
- A10 players report that 3+ Act 1 elites often kills them; shops are a gamble early. Front-load damage, be selective with block, do not force an archetype.
- A7 Scarcity: do not plan around rares; commons/uncommons (Bloodletting, Shrug It Off, Taunt, Evil Eye, Colossus, Inflame) carry.

### Sources (search excerpts)
- https://slaythespire.wiki.gg/wiki/Slay_the_Spire_2:Ironclad , https://slaythespire.wiki.gg/wiki/Slay_the_Spire_2:Unmovable , https://slaythespire.wiki.gg/wiki/Slay_the_Spire_2:Crimson_Mantle , https://slaythespire.wiki.gg/wiki/Slay_the_Spire_2:Colossus
- https://sts2.untapped.gg/en/tier-list/004de170-026a-4dd4-a280-3b904be0b5d6 (Baalorlord Ironclad v0.111.0)
- https://spire-codex.com/guides/ironclad-tier-list , https://spire-codex.com/tier-list/cards
- https://nat1gaming.com/sts2/tier-list/ironclad-card-tier-list/ , https://nat1gaming.com/sts2/character-guide/the-ultimate-guide-to-ironclad-in-slay-the-spire-2/
- https://metabot.gg/en/slay-the-spire-2/guides/ironclad-deep-dive-cards-relics-archetypes , https://metabot.gg/en/slay-the-spire-2/relics/tier-list/ironclad
- https://www.pcgamesn.com/slay-the-spire-2/ironclad
- https://www.thegamer.com/slay-the-spire-2-best-builds-relics-cards-the-ironclad-strength-vulnerable-guide/
- https://www.destructoid.com/best-ironclad-build-in-slay-the-spire-2-cards-strategy-and-more/
- https://intoindiegames.com/walkthroughs/tips-tricks/slay-the-spire-2-ironclad-ultimate-build-guide/
- https://note.com/e1427207/n/n08ce7564de7b , https://note.com/spire_picklab/n/n8866e43069b3 , https://note.com/spire_picklab/n/ndc05f995d99e
- https://steamcommunity.com/app/2868840/discussions/0/839501596191705877/ , https://steamcommunity.com/app/2868840/discussions/0/806845754928990077/
- https://www.stratgg.com/guides/ironclad-tier-list/ , https://spireforge.app/tier-lists/ironclad , https://www.keengamer.com/articles/guides/the-complete-guide-to-the-ironclad-in-slay-the-spire-2/ (Tier B/C)

---

## 2. Silent

### 2.1 Kit
- Deck (12): 5x Strike, 5x Defend, 1x Neutralize (damage + 1 Weak), 1x Survivor (block + discard a card; the only Sly enabler in the starter deck) `[STS2 x2+]`. One Tier C source says 4 Defends — majority and Steam say 5.
- HP: not confirmed in excerpts (`[STS1 principle]` 70) `[unverified]`.
- Starter relic: Ring of the Snake — draw 2 extra cards `[CONFLICT]` on turn 1 of each combat (most sources, matches STS1) vs "each combat start" (same thing phrased differently).
- **Sly `[STS2 x2+]`:** a card with Sly can be played normally for its cost, or, when it is discarded from hand during your turn, its effect triggers automatically for 0 energy. Some cards have Sly printed (Tactician, Reflex, Flick-Flack, Haze, Untouchable, Ricochet); others can gain it (Master Planner: skills gain Sly).
- **Shivs `[STS2 + STS1 principle]`:** 0-cost 4-damage Exhaust attacks generated by cards. Accuracy: Shivs deal +4 (+6) damage.
- **Poison `[STS1 principle, mechanics unchanged in excerpts]`:** target loses HP equal to Poison at its turn start, then Poison drops by 1; ignores Block. Catalyst appears to be absent in STS2 (a Steam thread asks for it back) — poison lacks a multiplier, which is why sources call it weaker than in STS1.

### 2.2 Archetypes
- **Discard / Sly (rated strongest: LINCE A10 guide says "by far the strongest"; StratGG/nat1 agree it is dominant).**
  - Enablers (discard outlets): Prepared (0 cost: draw 1, discard 1 — "easy to get, upgrade it"), Acrobatics (1 cost: draw 3 (4), discard 1 — "premiere draw engine"), Survivor, Dagger Throw (damage + draw/discard), Calculated Gamble (discard hand, draw that many; now Exhaust), Tools of the Trade (each turn draw 1 and discard 1), Hand Trick, Expertise, Well-Laid Plans.
  - Payoffs: Tactician (energy when discarded), Reflex (draw 2 when discarded), Untouchable (9 block when Slyed), Flick-Flack (AoE-ish damage), Ricochet, Haze (`[CONFLICT]`: reworked to 2-cost Poison + Weak), Master Planner (non-Sly skills become Sly), Abrasive (Dexterity + 4 Thorns for effectively 0), Speedster.
  - Requirement: thin deck; remove Strikes aggressively. Relic: Tough Bandages (block on discard).
- **Shivs.** Generators: Cloak and Dagger, Infinite Blades (a Shiv each turn), Blade Dance (now Exhausts — hurts it), Leading Strike, Hidden Daggers ("sleeper pick"), Knife Trap (plays the Shivs in your exhaust pile at the enemy; top win-rate card on Tierspire). Payoffs: Accuracy, Finisher (6 dmg per attack played this turn), Fan of Knives (Shivs hit all enemies), Phantom Blades, Envenom (Shiv hits apply Poison), Afterimage (block per card played), Serpent Form, Blade of Ink. Relic: Ninja Scroll.
- **Poison (weakest of the three).** Noxious Fumes (the premier enabler), Deadly Poison, Poisoned Stab, Accelerant ("mandatory if building Poison"), Bubble Bubble (only with lots of Poison already), Outbreak (reworked: rare skill, 3 cost, 9 (12) Poison to ALL, Poison triggers immediately), Corrosive Wave (apply 2–3 Poison to ALL per card drawn this turn — pairs with draw engines), Mirage (block scaling with Poison; now Exhausts unless upgraded), Snakebite (rated "worst card" by one guide, break-even by another data source) `[CONFLICT]`.

### 2.3 Card tiers
- **Take in any deck `[STS2 x2+]`:** Adrenaline (most trusted card in the pool: ~55% win rate, 65% pick rate), Acrobatics, Backflip (block + draw 2), Piercing Wail, Footwork, Wraith Form ("always take over any other rare" at top ascension), Afterimage (PC Gamer: chip damage is the enemy at A10), Leg Sweep, Malaise (underrated: 54.8% win rate at 18.8% pick rate), Calculated Gamble, Prepared, Tactician, Reflex.
- **nat1 S tier (Nov 12 list):** Adrenaline, Wraith Form, Flanking. A tier includes Bullet Time, Tactician, Reflex, Strangle, Shadow Step, Untouchable, Flick-Flack, Burst (best in discard and shiv decks).
- **Tierspire top win rates (v0.111.0):** Knife Trap, Adrenaline, Afterimage, Blade of Ink, Serpent Form, Fan of Knives.
- **Act 1 picks:** one AoE card early — Echoing Slash, Flick-Flack or Dagger Spray (PC Gamer); Dagger Throw (better than a Strike, plus draw and Sly trigger); Dash (good early on raw value); Backstab (strong early); Flechettes (~10 dmg for 1 energy on an average hand); Leading Strike, Assassinate, Blade Dance hold up best when added in Act 1 (MetaBot data). Accuracy or Tactician early can rescue a weak start (nat1).
- **Traps / skip:** Impatience, Envenom (36% win rate; only for committed Shiv decks), Dagger Spray per Spire Codex (33% win rate) `[CONFLICT with PC Gamer naming it an acceptable early AoE]`, Snakebite, Assassinate late, Poison cards without Noxious Fumes/Accelerant, any 3-cost power at A10. Abrasive is low in MetaBot data but A tier elsewhere `[CONFLICT]`.
- **Upgrade priority:** Prepared (LINCE: upgrade it early for the discard plan), Acrobatics, Neutralize `[STS1 principle]`, Footwork/Accuracy, Adrenaline, Backflip. Exact nat1 list not retrievable `[unverified ordering]`.

### 2.4 Removal and deck size
- Remove Strikes first, "as many as possible" for Sly decks `[STS2 x2+]`. Silent is the "most defensively slanted" starter (5 Defends), so early block cards are worth less and early damage worth more.
- Thin decks: Sly/discard decks must be "extremely thin". `[STS1 principle]` 20 or fewer effective cards with heavy draw.

### 2.5 Combat tactics
- Goal is "zero damage taken" per fight (LINCE): prioritise full blocks and Weak (Neutralize on the attacker's attack turn) over racing.
- Discard sequencing rule: when a discard effect asks for a card, discard in this order — Sly payoff cards (Reflex/Tactician first, since they refund cards/energy that can be used this turn) > status/curse > surplus Strikes/Defends. Never pay energy for a Sly card when a discard outlet is in hand.
- Play draw cards (Acrobatics, Backflip, Prepared, Adrenaline) before committing energy, so discards and Sly triggers are known.
- Ring of the Snake gives a 7-card turn 1: play powers (Footwork, Accuracy, Noxious Fumes, Afterimage) on turn 1 if the enemy is not attacking for lethal-ish damage.
- Finisher last in the turn after all attacks/Shivs. Accuracy before Shiv generators. Poison first, block after (poison needs time).
- Piercing Wail on multi-hit or multi-enemy attack turns (it reduces each hit).

### 2.6 Relics and potions
- Relics: Tough Bandages (discard), Ninja Scroll (Shivs), anything adding draw or energy; Kunai/Shuriken-type relics are `[STS1 principle]`.
- Potions: Dexterity Potion and defensive tools; Duplication, Ghost, Cultist rated high in one (Tier C) list.

### 2.7 Hard matchups
- Act 1 elites when the deck lacks damage or AoE — the usual reason Silent elite fights go badly; Weak sources help most.
- Lagavulin Matriarch: Shiv decks without scaling struggle (one guide).
- Kaiser Crab: Poison is the recommended answer (bypasses Block).
- Vantom: Shivs/Sly multi-hits strip Slippery well.
- Multi-enemy fights before an AoE card is found — hence "one AoE card early".

### 2.8 Higher ascension
- A7 Scarcity halves rare odds: do not plan on Wraith Form/Adrenaline; build from commons/uncommons (Prepared, Acrobatics, Backflip, Dagger Throw, Cloak and Dagger, Infinite Blades, Footwork).
- Chip damage matters more: Afterimage, Footwork, Piercing Wail, Leg Sweep rise in value; expensive powers fall.

### Sources (search excerpts)
- https://www.pcgamer.com/games/roguelike/slay-the-spire-2-best-silent-cards-build-ascension-10/
- https://note.com/l1nc3/n/na787f022cae9 (LINCE, A10 Silent, v0.104.0) , https://note.com/baroque_core/n/na81b21b2ed68
- https://nat1gaming.com/sts2/tier-list/silent-card-tier-list/ , https://nat1gaming.com/sts2/character-guide/the-ultimate-guide-to-silent/
- https://www.pcgamesn.com/slay-the-spire-2/silent
- https://spire-codex.com/guides/silent-tier-list , https://tierspire.com/slay-the-spire-2/ , https://metabot.gg/en/slay-the-spire-2/cards/tier-list/silent , https://metabot.gg/en/slay-the-spire-2/guides/silent-deep-dive-cards-relics-archetypes
- https://kotaku.com/slay-the-spire-2-patch-mega-crit-silent-card-buff-nerf-2000720771 , https://slaythespire2.net/changelog/v0.110.1
- https://steamcommunity.com/app/2868840/discussions/0/662737213850905602/ , https://steamcommunity.com/app/2868840/discussions/0/806845754928855457/ , https://steamcommunity.com/app/2868840/discussions/0/845131730817943068/
- https://slaythespire2.wiki.fextralife.com/The_Silent , https://www.thegamer.com/slay-the-spire-2-the-silent-best-build-cards-relics-shiv-poison-strategy/ , https://www.keengamer.com/articles/guides/slay-the-spire-2-silent-guide-best-strategy-cards-unlocks/ , https://www.destructoid.com/best-silent-build-in-slay-the-spire-2/ , https://www.gamebrief.net/blog/slay-the-spire-2-silent-solo-build-guide-2026 (Tier B/C)

---

## 3. Defect

### 3.1 Kit
- Deck (10): 4x Strike, 4x Defend, 1x Zap (1 energy: channel 1 Lightning), 1x Dualcast (1 energy: evoke the rightmost orb twice) `[STS2 x2+]`.
- HP: not confirmed (`[STS1 principle]` 75) `[unverified]`. Starter relic: Cracked Core — channel 1 Lightning at combat start `[STS2 x2+]`.
- **Orbs `[STS2 x2+]`:** 3 orb slots at start (more via Capacitor, Modded). Channeling into a full row evokes the oldest orb `[STS1 principle]`. Passive triggers at end of turn.
  - Lightning: passive 3 damage to a random enemy; evoke 8.
  - Frost: passive 2 block; evoke 5.
  - Dark: gains 6 (+Focus) stored damage per turn; evoke deals the stored total (`[STS1 principle]`: to the lowest-HP enemy).
  - Plasma: passive 1 energy per turn; evoke 2 energy. Not affected by Focus.
  - Glass (new): passive 2 (+Focus) damage to ALL enemies; evoke value not found.
- **Focus:** adds to passive and evoke values of every orb except Plasma; can go negative but never below 0 output. Defragment = +1 (+2) Focus per copy (a rare in STS2); Biased Cognition = fast Focus with a drawback; Synchronize nerfed to 1 (2) Focus in the Nov 12 beta; Data Disk relic = start with 1 Focus.
- `[STS2]` (oltre A10 guide): maximum practical Focus in the current game is about 3, so pure Focus-stacking is no longer a viable primary plan — win through orb count, slots and evoke payoffs instead.

### 3.2 Archetypes
- Share of winning A10 decks (oltre): 40–50% Lightning, 20–30% Dark, remainder status/other. Frost is the defensive layer inside all of them ("orb slots + Frost + potent attacks" is the most reliable shell per a high-win-rate Steam player).
- **Lightning.** 2–3 Lightning generators are enough: Lightning Rod (1 energy: 4 block, channel Lightning at the start of each of the next two turns — "almost always take"), Ball Lightning (1 energy: 7 damage + channel Lightning; do not over-draft it), Storm (channel Lightning when you play a Power), Tempest, Voltaic / Thunder (payoffs for a critical mass of Lightning), Shatter (nat1 S: high damage for 1 energy).
- **Dark.** Darkness and other Dark channelers plus Capacitor ("if pursuing Dark, make sure to take Capacitor") so Dark orbs can sit and grow; Dualcast/evoke effects as the finisher.
- **Frost / block.** Glacier (10 block + channel 2 Frost even after a nerf), Coolheaded (channel Frost + draw 1), Chill (channel 1 Frost per enemy, Exhaust), Cold Snap, Coolant (2 block per unique orb type each turn), Charge Battery.
- **Status / "fuel".** Defect can deliberately create status cards (Slimes, Burns, Voids) for payoffs: Turbo (energy now, status later), Overclock (`[CONFLICT]`: draw 2 at 0 cost vs "generates statuses"), Compact, Flak Cannon (a Steam player: "probably the best build", Overclock + Turbo). PCGamesN says Status and Claw fall off at higher ascension.
- **Claw / 0-cost.** Claw (3 dmg, +2 to all Claws each play), Scrape, FTL, Go for the Eyes, Beam Cell, Feral, Hologram, All for One (mandatory payoff; one of the top win-rate Defect cards). Do not start Claw without All for One in sight.
- **Engine cards for every build:** Echo Form (S everywhere; hard to play without extra energy), Machine Learning (power: +1 draw each turn), Hologram (1 energy: 3 block, return a discard-pile card to hand, draw 1), Skim (1 energy: draw 3), Supercritical (energy; needs draw), Buffer, Reboot, Capacitor, Creative AI.

### 3.3 Card tiers
- **Win-rate S tier (Tierspire v0.113.1, ~514k Defect runs):** Echo Form, Machine Learning, Defragment. Spire Codex top: Buffer, All for One, Echo Form, Machine Learning, Reboot. MetaBot: Supercritical strongest.
- **nat1 (Nov 12):** S = Echo Form, Biased Cognition, Shatter; A = Supercritical, Turbo; B = Coolheaded, Compact; Claw rated high in practice.
- **PCGamesN:** S = Hologram, Echo Form, Synchronize, Capacitor, Defragment, Voltaic, Glacier; A = Buffer. D = Gunk Up, Boost Away, Feral, Beam Cell, Chaos, Compact, Helix Drill.
- **Act 1 rule (strong agreement):** take damage first — "any damage cards to survive"; target about 30 damage per turn for the Act 1 boss, about 1.5x that for Act 2 and 2x for Act 3 (oltre). Defect commons give enough block; block that only arrives next turn is too slow (LW).
- **Good commons named by A10 players:** Ball Lightning (first copy), Coolheaded, Cold Snap, Lightning Rod, Gunk Up (as an attack common, `[CONFLICT]` with PCGamesN D), Fight Through and Boost Away as block (`[CONFLICT]` with PCGamesN D), Charge Battery, Compact (situational).
- **Act 2 mistake to avoid:** drafting more Ball Lightnings when the deck needs Skim, Coolheaded, Hologram, Overclock (draw/consistency).
- **Traps / skip:** Meteor Strike, Bulk Up, Helix Drill, Chaos, Momentum Strike (no synergy), Feral/Beam Cell/Claw without All for One, extra Focus cards beyond the first couple.
- **Upgrade priority (nat1 excerpt):** cost-reduction and energy first — Dualcast, Zap, Turbo — then powers (Capacitor, Feral in Claw). LW: upgrade early; at least one key card loses Exhaust when upgraded. Also Glacier and Charge Battery for defense.

### 3.4 Removal and deck size
- Remove Strikes first `[STS2 x2+]` (also makes Ruckus stronger per LW). nat1 warns that aggressive thinning via Precarious Shears is risky because Defect cannot avoid early damage — do not remove Defends early.
- Deck size not stated; `[STS1 principle]` medium decks (25–30) are fine because powers leave the deck and orbs do work off-card.

### 3.5 Combat tactics
- Fill slots early: channel before attacking so passives tick every turn. Do not channel into a full row if it would evoke a growing Dark orb prematurely, unless that evoke kills.
- Dualcast evokes the rightmost orb twice: aim it at a Dark orb that has charged for several turns (lethal check), at Plasma for burst energy, or at Lightning (16 base) for early burst. Dualcast on Frost (10 block) is a fallback when incoming damage is otherwise unblocked.
- Frost as steady-state block: with N Frost orbs passive block is N x (2 + Focus) per turn; count this before spending energy on Defends.
- Play Focus (Defragment, Biased Cognition) before the end-of-turn passive trigger and before evoke cards. Play Powers before Storm-less turns end; with Storm, each Power also channels Lightning.
- Lightning passives hit random enemies: against a single dangerous target prefer evokes/targeted attacks; Glass and Lightning are good against multi-enemy fights.
- Plasma: keep it in a slot (1 energy per turn) rather than evoking, unless a 2-energy burst wins the fight this turn.

### 3.6 Relics and potions
- Relics: Data Disk (1 Focus), Runic Capacitor (extra slots; "transformative"), Nuclear Battery (for orb-light decks), Unceasing Top (0-cost/Claw decks). Energy relics for Echo Form.
- Potions: Focus Potion (+2 Focus roughly doubles base orb output) is the best Defect potion; block/"Ghost"-type potions for Act 1 survival.

### 3.7 Hard matchups
- Lowest win rate of the five characters in every tracker (25–30% community, 2.4% at A10 in one data set). Dies most to Knowledge Demon (26% of attempts) and Queen; Aeonglass is a burst/breakpoint check.
- Act 1 elites before orbs are online — the starter deck is weak at burst. Take damage cards and fight elites late in the act.
- The Insatiable (turn timer) versus slow Frost/Dark setups: need evoke burst.

### 3.8 Higher ascension
- Top players reach 35%+ (some claim 70%+) at A10, so the character works but punishes sequencing errors.
- A7 Scarcity makes Defragment/Echo Form (rares) unreliable: build on Lightning Rod, Coolheaded, Glacier, Capacitor, Hologram, Skim; treat Focus as a bonus.

### Sources (search excerpts)
- https://slaythespire.wiki.gg/wiki/Slay_the_Spire_2:Defect
- https://note.com/oltre_game/n/ne5aa9f2f59cf , https://note.com/oltre_game/n/ne594fb763f5c , https://note.com/oltre_game/n/ndbc325ed6bdc (oltre, A10 Defect)
- https://note.com/lw_gusto/n/na48055effad6 , https://note.com/lw_gusto/n/nacd19241f734 (LW, A10 Defect)
- https://nat1gaming.com/sts2/tier-list/defect-card-tier-list/ , https://nat1gaming.com/sts2/character-guide/the-ultimate-guide-to-defect/
- https://tierspire.com/slay-the-spire-2/defect/ , https://spire-codex.com/tier-list/cards , https://metabot.gg/en/slay-the-spire-2/guides/defect-deep-dive-cards-relics-archetypes , https://spire2stats.com/characters/defect/
- https://www.pcgamesn.com/slay-the-spire-2/defect , https://gamerant.com/slay-the-spire-2-best-defect-cards-builds-relics/ , https://gamerblurb.com/articles/slay-the-spire-2-defect-tier-list-best-defect-cards
- https://steamcommunity.com/app/2868840/discussions/0/798966745989837796/ , https://steamcommunity.com/app/2868840/discussions/0/839501827476106232/ , https://steamcommunity.com/app/2868840/discussions/0/839501343505873692/
- https://slaythespire2.wiki.fextralife.com/The_Defect , https://dood.gg/en/slay-the-spire-2/defect/cards/ , https://sts2db.com/characters/defect/ , https://sts2guides.com/en/guides/defect-complete-guide.html , https://slaythespire-2.com/characters/defect (Tier B/C)

---

## 4. Regent

### 4.1 Kit
- HP `[CONFLICT]`: 75 (better supported) vs 80. 3 energy.
- Deck (10): 4x Strike, 4x Defend, 1x Falling Star (0 energy, costs 2 Stars: damage [plus a debuff per one source]), 1x Venerate (gain Stars) `[STS2 x2+]`. One source claims Sovereign Blade is in the opening hand — not supported by the others.
- Starter relic: Divine Right — gain 3 Stars at the start of each combat `[STS2 x2+]` (Mobalytics calls it "Crown of Stars" — likely an old name). Upgrade: Divine Destiny (6 Stars) via the Ancient relic Touch of Orobas; note Spire Codex data shows Divine Destiny as the only Regent relic below baseline (48%) — do not pay much for it.
- **Stars `[STS2 x2+]`:** second resource shown beside energy. Stars do NOT reset each turn — they persist within a combat until spent, and are not regenerated automatically. Many cards cost Stars instead of / in addition to energy. One Tier C source gives a cap of 24 `[unverified]`.
- **Forge `[STS2 x2+]`:** the first "Forge X" played in a combat creates Sovereign Blade in hand; every later Forge X adds X damage to that blade for the rest of combat (no extra copies). Sovereign Blade: colorless attack, 2 energy (1 upgraded per one source), 10 base damage, Retain. Bonuses reset between combats.
- Structural weaknesses (GGG 100-hour analysis): energy rarely reaches 4, block does not carry over and defensive cards only help on the turn drawn, draw is mediocre — weak cards in the deck hurt Regent more than other characters.

### 4.2 Archetypes
- **Stars ("mana") — most consistent per A10 writers ("build around Stars while adding blades"; "a Stars-free Regent deck is quite weak").**
  - Generators: Glow (gain 1 (2) Stars, draw 1 (2), next turn draw 1 — "best star generator, one of the best cards in the game"), Venerate, Gather Light (1 energy: 7 block + 1 Star), Solar Strike (1 energy: 8 dmg + 1 Star), Shining Strike (8 dmg + 2 Stars, returns to top of draw pile), Genesis (2-cost power: +2 Stars each turn), Radiance (Stars; "secure early"), Star Child / Child of the Stars (see payoffs), Alignment (Stars -> energy).
  - Payoffs: Child of the Stars (power: gain 2 block per Star spent — the survival engine; hikki rates Star Child top), Black Hole (power: 3 damage to ALL whenever you gain or spend Stars), Seven Stars (2 energy: 7 dmg to ALL x7), Particle Wall (0 energy / 2 Stars: 9 block, returns to hand — "best defensive common"), Cloak of Stars (0 energy / 1 Star: 7 block), Gamma Blast (0-energy attack), Falling Star, Comet (0-energy attack, S on nat1), Reflect (1-energy skill, S on nat1/PCGamesN), Big Bang (0-energy skill; top Regent card by win rate, 53.8% at 68.7% pick), Meteor Shower (0 energy, Ancient rarity), Stardust (X-cost damage from Stars), Astral Pulse (scales with Strength), Astral Wave (XecnaR: highly recommended), Heavenly Drill.
- **Forge / Sovereign Blade.** Forge cards: Wrought in War (solid early damage common that also Forges), Bulwark, Refine Blade, Sword Polish (`[unverified name, from a translated note]`), Parry (hikki: top Forge card), Conqueror (small Forge + doubles Sovereign Blade damage the turn it is played), Summon Forth (fetch Sovereign Blade from anywhere), Seeking Edge. Plan: 2–3 turns of ramp, then one huge hit; needs survival tools meanwhile. Tier C sources call Forge the "safest" plan; A10 writers treat it as a secondary damage source added to a Stars deck.
- **Card creation / colorless.** Pillar of Creation (hikki top tier; nat1 C tier — needs a lot of generation) `[CONFLICT]`, Hidden Cache cards, Make It So (strong in skill-dense decks, 52.4% win rate), Decisions, Decisions.
- **Energy cheats:** Void Form (first two cards each turn cost 0; cost given as 3 — fails the "no 3-cost powers at A10" rule unless energy relics), Tyranny (2nd on Spire Codex), Autocracy (GGG: lowers "accident rate" and scales long fights, does nothing the turn drawn), Pale Blue Dot, Cosmic Indifference (take once the deck is bigger, around Act 3).

### 4.3 Card tiers
- **Pick policy (XecnaR A10 guide, the most cited Regent guide):** pick only the few cards with abnormally high stats and skip mediocre cards even in early Act 1. Named strong: Astral Wave, Reflect, Ramparts, Meteor Shower, Star Child, Pillar of Creation.
- **Consensus top `[STS2 x2+]`:** Big Bang, Reflect, Meteor Shower, Comet, Glow, Child of the Stars, Particle Wall, Tyranny, Genesis, Black Hole, Seven Stars, Void Form (energy permitting).
- **Act 1 picks:** Glow (upgrade early: upgraded Glow leaves 3 Stars free for Falling Star, Reflect, Gamma Blast), Wrought in War, Solar Strike, Gather Light, Particle Wall, Cloak of Stars, Shining Strike, one AoE (Seven Stars / Black Hole).
- **Deck-thinning cards:** CHARGE!! and BEGONE! (remove/transform starter Strikes and Defends in combat per one guide) `[STS2, Tier B]`.
- **Traps / skip:** Photon Cut (XecnaR: weak); extra Falling Star copies (F tier by win rate: 13–30%, yet picked a third of the time); Prophesize `[CONFLICT]`; Pillar of Creation without generation; Star payoffs with no generators (or the reverse); Divine Destiny swap.
- **Upgrade priority:** Glow first; then Forge cards and the damage-plus-Forge common; many Regent cards "need smithing", so late campfires favour upgrades over new cards; upgrading Strikes is the lowest value.

### 4.4 Removal and deck size
- XecnaR: "always remove 1 Strike + 1 Defend" (an exception to Strikes-first, because Regent's Defends are also weak once Star block is online).
- Compact deck: 15–18 cards quoted for Stars decks (Tier B); GGG also says Regent performs better compact. Bot rule: stricter skip threshold for Regent than for other characters.

### 4.5 Combat tactics
- Stars persist: do not spend Stars on low-value plays just because they are available. Bank them for (a) a block card on an attack turn (Particle Wall, Cloak of Stars), (b) lethal, or (c) a turn with Child of the Stars / Black Hole in play so each Star spent also yields block/AoE.
- Order within a turn: play Star generators first, then Star spenders; play Child of the Stars / Black Hole before any Star movement; with Black Hole both gaining and spending trigger damage.
- Forge: play Forge cards every turn they are drawn, hold Sovereign Blade (Retain) until it kills or until Conqueror is in hand for the doubled turn; with spare energy and no better play, swing the blade — it is not consumed (unverified whether it returns; treat as a normal Retain attack).
- Regent block is turn-local: on enemy non-attack turns spend everything on generators, Forge and powers.
- Route (XecnaR): do not dive into too many Act 1 hallway fights — the weak starter wears down. nat1 recommends paths with 3+ campfires, many fights and one or two late elites `[mild CONFLICT]`; common ground: elites late in the act, campfires for upgrades.

### 4.6 Relics and potions
- Any relic that generates or interacts with Stars is top priority; energy relics (for Void Form, Genesis, Sovereign Blade); draw relics.
- Potions: nothing Regent-specific confirmed. (One Tier C site recommends "Ambrosia for Divinity" — that is an STS1 Watcher mechanic and is almost certainly wrong for STS2; ignore.)

### 4.7 Hard matchups
- Act 2 multi-attack enemies without Child of the Stars (survival check).
- Fights with many enemies: single-target Sovereign Blade decks stall — keep an AoE source (Seven Stars, Black Hole, Big Bang-style turns).
- Long scaling checks (Test Subject) for decks whose damage tops out at one blade hit; The Insatiable's timer versus slow Forge ramp.

### 4.8 Higher ascension
- A10 writers report ~50% win rates with disciplined picking; the character is high-variance (rated S by some, last by others).
- A7 Scarcity hurts because Regent leans on a few rares (Big Bang, Void Form, Genesis); prioritise uncommon/common engines: Glow, Particle Wall, Child of the Stars, Reflect, Wrought in War.

### Sources (search excerpts)
- https://slaythespire.wiki.gg/wiki/Slay_the_Spire_2:Regent
- https://note.com/toku123/n/nab86f10a69af (summary of XecnaR's A10 Regent guide) , https://note.com/munaz/n/nc4fca01cce30 , https://note.com/mmgrr/n/n9e64a4ec354f
- https://note.com/hikki0000/n/nfee829c1740d , https://note.com/hikki0000/n/ncb7857cc7248 (hikki, all Regent + colorless cards, v0.105.1)
- https://note.com/ggg6190/n/n328e633f189c , https://note.com/clean_mink8969/n/ndac777ad395b , https://note.com/mizz325/n/n2e5a61587e6b
- https://nat1gaming.com/sts2/tier-list/regent-card-tier-list/ , https://nat1gaming.com/sts2/character-guide/the-ultimate-guide-to-regent/ , https://nat1gaming.com/sts2/tier-list/colorless-card-tier-list/
- https://spire-codex.com/guides/regent-tier-list , https://spire-codex.com/tier-list/cards?color=regent , https://metabot.gg/en/slay-the-spire-2/cards/tier-list/regent , https://sts2.untapped.gg/en/tier-list/0e6c1e23-bec6-4887-a9e0-dbf49ede974d (Baalorlord Regent v0.111.0)
- https://www.pcgamesn.com/slay-the-spire-2/regent , https://mobalytics.gg/slay-the-spire-2/characters/regent-guide , https://progameguides.com/slay-the-spire-2/slay-the-spire-2-regent-guide-best-build-cards-strategy-2026/ , https://www.destructoid.com/best-regent-build-in-slay-the-spire-2-cards-strategy-and-more/ , https://www.keengamer.com/articles/guides/slay-the-spire-2-best-regent-builds/
- https://slaythespire2.wiki.fextralife.com/The_Regent , https://www.slaythespire2.website/classes/regent-guide , https://slaythespire-2.com/characters/regent , https://sts2front.com/mechanics/forge/ (Tier B/C)

---

## 5. Necrobinder

### 5.1 Kit
- HP 66 (lowest of the roster) `[STS2 x2+]`. Because max HP is low, resting heals little — value block cards higher and upgrades over rests (LINCE).
- Deck (10): 4x Strike, 4x Defend, 1x Bodyguard (Summon: add 5 HP to Osty), 1x Unleash (Osty attacks: 6 damage plus Osty's current HP) `[STS2 x2+]`.
- Starter relic: Bound Phylactery `[CONFLICT on text]`: "at the start of your turn, Summon 1" (most likely; also described as "free summons every turn") vs "summons Osty at combat start".
- **Osty (companion) `[STS2 x2+]`:** starts each combat at 1 HP. Unblocked attack damage aimed at you hits Osty first; only damage beyond Osty's current HP spills over to you. At 0 HP Osty dies and returns at 1 HP at the start of your next turn. "Summon X" = add X to Osty's HP. Osty attack cards deal damage through Osty (several scale with Osty's HP), and some effects need Osty alive.
- **Doom `[STS2 x2+]` (wiki):** stacking debuff. At the end of an enemy's turn, if its Doom >= its current HP, it dies. Doom does not reduce HP, so it ignores Block, Slippery and Intangible. Some cards apply Doom to yourself (Neurosurge: free energy/cards but self-Doom each turn) — the same death rule applies to the player.
- **Soul `[STS2 x2+]` (wiki):** colorless 0-cost token skill — draw 2 (3 upgraded) cards, Exhaust. Generated into draw pile/hand/discard by many cards; "playing a Soul" is a trigger for payoffs.
- **Ethereal** theme: several cards and payoffs care about Ethereal cards (Call of the Void).

### 5.2 Archetypes
- nat1: many Necrobinder cards are good in every archetype — take strong cards rather than archetype-specific ones. Steam A10 thread: winning decks usually combine two archetypes.
- **Souls (most consistent; best at high ascension per several sources).**
  - Generators: Grave Warden (1 energy: 8 block + a Soul into draw pile — best common), Capture Spirit (1 energy: enemy loses 3 HP, add 3 Souls to draw pile), Severance (2 energy: 13 dmg, Soul to hand, draw pile and discard), Dirge (X cost: Summon 3 X times and add X Souls; upgraded better; "defines the character" for some), Glimpse Beyond (top win-rate card, 62%), Seance.
  - Payoffs: Haunt (1-cost power: whenever you play a Soul, a random enemy loses 6 HP — ignores Block), Devour Life (1 energy: whenever you play a Soul, Summon 1), Soul Storm, Death March (11 dmg +3 per card drawn this turn), Spirit of Ash.
  - Energy: Borrowed Time (0-cost, does not exhaust; unusual energy source — S on PCGamesN), Friendship (nat1 S: energy power; costs Strength — a trap in Osty attack decks) `[CONFLICT]`, Prayer (LINCE: "2 energy for 1", an inferior Adrenaline).
  - Rule from PCGamesN: have finishers by the end of Act 2 or Act 3 is very hard.
- **Osty / Summon (simplest).**
  - Growth: Bodyguard, Pull Aggro (2 energy: Summon 4 + 7 block), Spur (1 energy: Retain a card, Summon 3, Osty heals 5), Reanimate (3 energy: Summon 20), Dirge, Legion of Bone, Calcify (Plated-Armor-like, unverified), Wisp (LINCE/nat1: weak unless upgraded).
  - Attacks: Unleash, Poke (0 cost: Osty deals 6), Fetch (0 cost: Osty deals 3, draw 1), Flatten (free 12 dmg once Osty has attacked this turn), Sic 'Em (1 energy: 5 dmg; Osty grows by 2 each time he attacks this turn), Rattle (7 dmg, extra hits per Osty attack this turn), High Five (11 dmg to ALL + Vulnerable), Right Hand Hand, Squeeze (highest raw ceiling per one source).
  - Payoffs: Necro Mastery (2-cost power: whenever Osty loses HP, all enemies lose that much HP), Sacrifice / Bone Shards (kill Osty for AoE equal to his HP; he returns at 1 HP next turn), Protector. Relic: Bone Flute (gain 2 block whenever Osty attacks; best common relic for the class, 65% win rate).
- **Doom.**
  - Appliers: Scourge (common, efficient Doom), Negative Pulse (AoE Doom while defending; LINCE S), No Escape, Time's Up, Deathbringer, Blight Strike, End of Days (29 (37) Doom to ALL, then kills enemies whose Doom >= HP immediately), Banshee's Cry.
  - Support: Death's Door (large Doom-conditional block; LINCE rates its block highly), Debilitate, Oblivion, Shared Fate. Relic: Book Repair Knife.
- **Chunky attacks as a supplement (Steam A10):** Reap, Bury, Eradicate, Squeeze, Hang(man) supply damage while you block — `[CONFLICT]`: Reap, Sow, Bury are near the bottom by win rate.

### 5.3 Card tiers
- **Win-rate top (Tierspire v0.111 / Spire Codex):** Neurosurge, Demesne, Debilitate, Shared Fate, Banshee's Cry, Eradicate, Glimpse Beyond, Legion of Bone; A tier: Seance, Oblivion, Borrowed Time. Shop buy-rate leaders: Glimpse Beyond, Lethality, Oblivion.
- **Expert lists:** LINCE S = Call of the Void (fetches Ethereal cards to fill gaps), Death's Door, Negative Pulse; A = Prayer, Pull Aggro-style block ("Hate Generation" in translation); firepower leaders Hang and Eradicate. nat1: Friendship S, Call of the Void A, Glimpse Beyond and Borrowed Time praised, Sleight of Flesh and Pagestorm B, Delay C, Parse and Wisp D (Necrobinder already draws plenty). PCGamesN S: Graveblast, Borrowed Time, Dirge, Dredge, Haunt, Devour Life, Protector, Soul Storm; A: Grave Warden.
- **Act 1 picks:** Grave Warden (take and upgrade), Scourge, Poke/Fetch/Flatten package, Sic 'Em, High Five (AoE + Vulnerable), Pull Aggro, Capture Spirit, Defy, Borrowed Time, Blight Strike, any efficient block.
- **Traps / skip:** Afterlife (18–23%: worst by every metric), Reap, Sow, Bury, Undeath, Parse, Wisp unupgraded, Delay, Eidolon and Call of the Void per PCGamesN (`[CONFLICT]` with LINCE/nat1 on Call of the Void), Graveblast `[CONFLICT]` (PCGamesN S vs SpireForge trap), pure draw cards (the class has the most draw in the game — "draw without energy is useless"), Friendship in Osty-attack decks.
- **Upgrade priority:** Grave Warden, Dirge (4x when upgraded), Wisp if kept, Bodyguard/Unleash early `[unverified ordering]`, key powers (Haunt, Devour Life, Necro Mastery).

### 5.4 Removal and deck size
- Strikes first: remove two Strikes, then a third at the next shop (nat1 / Steam). Osty attack cards replace Strike damage; Defends keep Osty alive.
- Thin deck: "no need to pick a lot of cards"; Soul decks cycle fast and want few dead cards. In-combat thinning via Seance, Eidolon, Cleanse.

### 5.5 Combat tactics
- Damage budget each enemy turn: incoming minus your Block is absorbed by Osty up to his HP; the remainder hits you. So block needed to take zero = incoming - Osty HP, but letting Osty die costs his accumulated HP (and Unleash/Sacrifice/Necro Mastery damage). Rule: block enough to keep Osty alive when Osty HP is large (>= ~8) or an Osty-HP payoff is in the deck; when Osty is at 1–3 HP, treat him as a free 1–3 point shield.
- A10 framework: "stall and block, then burn" — Osty is the damage source and only survives if you block.
- Attack ordering: play any cheap Osty attack (Poke/Fetch) first to enable Flatten; play Sic 'Em before other Osty attacks (growth per attack), Rattle last (hits scale with attacks made). Summon before Unleash / Sacrifice (damage scales with Osty HP). Sacrifice/Bone Shards on the turn Osty would die anyway.
- Souls: play Souls first each turn (0-cost draw 2) unless hand size would overflow; play Haunt/Devour Life before any Soul.
- Doom lethal check: at the point of ending the turn, an enemy dies after its own turn if Doom >= HP. So compare (current Doom + Doom you can add) against (HP - damage you can deal); once Doom >= HP stop attacking that enemy and just block its last action. End of Days kills immediately.
- Self-Doom (Neurosurge): track own Doom versus own HP; "do not fear it at A10" but a bot must keep a hard safety margin (stop when self-Doom approaches current HP).

### 5.6 Relics and potions
- Relics: Bone Flute, Ivory Tile and Big Hat (top by win rate, 66%), Funerary Mask (Tier C), Book Repair Knife (Doom), sustain relics (low HP class), energy relics (lots of draw, little energy).
- Potions: nothing class-specific confirmed; scaling/defensive potions favoured because fights run long (Tier C).

### 5.7 Hard matchups
- Bosses that punish pure "defend and grow Osty": Infested Prism (Act 2 elite), Aeonglass, Test Subject phase 1 — need real burst (chunky attacks, Doom, Haunt).
- Multi-hit attackers and AoE that strip Osty quickly; early Act 1 elites before Summon cards (66 HP leaves little margin).
- The Insatiable turn timer versus slow setups; "Inkblot Phantasm" is mentioned by one Tier C guide as a problem fight (name unverified against the wiki list).

### 5.8 Higher ascension
- Rated the strongest or second-strongest class by several lists (30.9% win rate, A tier on Tierspire). Soul builds scale best.
- A9 Deadly Enemies raises the block needed to keep Osty alive; A8 Tough Enemies raises Doom thresholds — Doom needs more appliers or a damage supplement. A4 potion-slot loss matters more for a 66-HP class: use potions early in elite fights.

### Sources (search excerpts)
- https://slaythespire.wiki.gg/wiki/Slay_the_Spire_2:Doom , https://slaythespire.wiki.gg/wiki/Slay_the_Spire_2:Soul , https://slaythespire.wiki.gg/wiki/Slay_the_Spire_2:Bound_Phylactery , https://slaythespire.wiki.gg/wiki/Slay_the_Spire_2:Keywords
- https://note.com/l1nc3/n/n225c8084342b , https://note.com/l1nc3/n/n38e09eff86ed (LINCE Necrobinder tier lists) , https://note.com/gahlink/n/nd6f537b73aba
- https://nat1gaming.com/sts2/tier-list/necrobinder-card-tier-list/ , https://nat1gaming.com/sts2/character-guide/the-ultimate-guide-to-necrobinder/
- https://tierspire.com/slay-the-spire-2/necrobinder/ , https://spire-codex.com/guides/necrobinder-tier-list , https://sts2.untapped.gg/en/tier-list/cards/necrobinder , https://metabot.gg/en/slay-the-spire-2/relics/tier-list/necrobinder , https://spireforge.app/tier-lists/necrobinder
- https://www.pcgamesn.com/slay-the-spire-2/necrobinder , https://www.thegamer.com/slay-the-spire-2-best-build-cards-relics-the-necrobinder-souls-guide/ , https://gamerant.com/slay-the-spire-2-sts2-best-necrobinder-cards/ , https://www.destructoid.com/best-necrobinder-build-in-slay-the-spire-2-best-cards-strategy-and-more/ , https://www.keengamer.com/articles/guides/best-necrobinder-build-in-slay-the-spire-2/ , https://www.dualshockers.com/slay-the-spire-2-how-to-play-necrobinder/
- https://steamcommunity.com/app/2868840/discussions/0/592938395265825061/ , https://steamcommunity.com/app/2868840/discussions/0/798965065596422917/
- https://slaythespire2.wiki.fextralife.com/The_Necrobinder , https://www.screenhype.co.uk/best-osty-deck-builds-in-slay-the-spire-2/ , https://www.stratgg.com/characters/necrobinder/ , https://www.sts2front.com/tips/necrobinder-a10-strategy/ , https://slaythespire-2.com/characters/necrobinder (Tier B/C)

---

## 6. Rule candidates for the bot (distilled; each traces to a section above)

- **Global**
  - Act 1 reward scoring: bonus for front-loaded damage and for the first AoE card; penalty for 3-cost powers and for slow scaling picked before the Act 1 boss (exception: cheap scalers such as Inflame, Glow, Accuracy).
  - After Act 1, raise the skip threshold; raise it further for Regent and for Sly Silent.
  - Removal order default: curse > Strike > Defend. Regent: one Strike and one Defend early. Defect/Necrobinder: no Defend removals early.
  - Campfire: upgrade unless HP is too low for the next elite/boss.
  - Boss turn logic: full block on telegraphed heavy turns, setup/damage on buff turns; shorten fights against The Insatiable and Knowledge Demon (prefer damage over block when both are possible and survival is not at stake).
- **Ironclad:** treat HP as spendable while Burning Blood is held; bonus for exhaust payoffs once two or more exhaust enablers are owned (and vice versa); Vulnerable before burst; extra value on Weak sources; never take Tank solo.
- **Silent:** count discard outlets and Sly cards and score each by the other count; never pay energy for a Sly card if an outlet is playable; draw first; Poison cards need Noxious Fumes or Accelerant to score above average; target zero damage taken.
- **Defect:** Act 1 = damage; cap Ball Lightning copies; Lightning Rod high; Capacitor high when Dark orbs are in the deck; Focus cards valued for the first one or two only; Dualcast targets a matured Dark orb or lethal; Claw cards only with All for One.
- **Regent:** Stars persist — hold for block or payoff turns; generators before spenders; first Forge creates the blade, hold the blade for lethal or Conqueror; Glow is the first upgrade; skip mediocre commons.
- **Necrobinder:** model Osty HP as a damage sponge with a keep-alive bonus proportional to his HP; cheap Osty attack before Flatten, Sic 'Em early, Rattle last, Summon before Unleash; Doom lethal check before spending damage; Souls first; block cards valued above rests; self-Doom safety margin.

## 7. Thinly sourced or unresolved items (verify in game before encoding)

- Exact card text and numbers for most cards (excerpts only; several cards changed in v0.103–v0.113 and the Nov 12 beta: Stoke, Expect a Fight, Outbreak, Mirage, Haze, Synchronize, Glacier, Blade Dance, Calculated Gamble).
- Starting HP of Silent and Defect; Regent 75 vs 80.
- Bound Phylactery text; Ring of the Snake timing; Star cap; whether Sovereign Blade's cost changes on upgrade.
- Per-boss/elite mechanics for Soul Fysh, Lagavulin Matriarch, Terror Eel, Phantasmal Gardener, Skulking Colony, Decimillipede, Entomancer, Infested Prism, Aeonglass, Test Subject (names confirmed, mechanics not).
- Upgrade-priority and removal sections of the nat1gaming "Ultimate Guide" pages and the full PC Gamer / LINCE / XecnaR texts (only fragments surfaced).
- Potion synergies per character (only Tier C sources; one contained a clear STS1 Watcher error).
- No Reddit r/slaythespire thread content could be retrieved at all.
