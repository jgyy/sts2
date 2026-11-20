# Slay the Spire 2 — General (character-independent) strategy notes for a rule-based bot

Compiled 2026-11-20. Target: heuristic bot at Ascension 4-5, game version ~v0.11x.

## 0. How to read this file (sourcing caveats — important)

- **Access limitation:** direct page fetches were blocked in the research session (the hosts tried — sts2.untapped.gg,
  bossdown.com, switchbladegaming.com — were outside the network allowlist). Everything STS2-specific below comes from
  **web-search result excerpts/summaries** of the listed pages, not from reading full pages. The "Sources" lists are
  therefore "pages whose search excerpts were used", not pages read end to end. No Reddit thread or YouTube
  transcript was reachable.
- Tags used on each bullet:
  - **[STS2]** = STS2-specific, seen in at least one source excerpt.
  - **[STS2, multi]** = same claim seen in 2+ independent sources.
  - **[STS2, conflict]** = sources disagree; both versions given.
  - **[STS1 principle]** = well-established Slay the Spire 1 high-ascension practice, from general knowledge (not
    fetched). Use as a prior; not verified for STS2.
  - **[unverified]** = single weak source, or my inference. Check against in-game data before hard-coding.
- Many STS2 guide sites are auto-generated/SEO content and contradict each other on numbers. The most trustworthy
  data-style sources seen were **slaythespire.wiki.gg** and **spire-codex.com** (datamined-style tables). Prefer the
  bot's own observed game state (enemy intents, HP, move names) over any number here.
- All enemy/relic/card names below appeared in a source excerpt. HP and damage values drift every patch.

---

## 1. Map pathing

### Structure facts
- [STS2] 3 acts. Act 1 is **Overgrowth** or the alternate **Underdocks**; Act 2 is **Hive**; Act 3 is **Glory**.
  (spire-codex, senseos)
- [STS2] Act lengths per spire-codex: Act 1 = 15 rooms / 17 floors (3 "weak" opening fights); Act 2 = 14 rooms /
  16 floors (2 weak fights); Act 3 = 13 rooms / 15 floors (2 weak fights). Floor counts include the Ancient and the
  boss. Underdocks uses the same numbers as Overgrowth.
- [STS2] First row is always a fight; last row before the boss is always a rest site. (spire-codex)
- [STS2, conflict] Early gate: spire-codex says no elites and no rest sites in the first 5 rows; a v0.103.2 note
  (sts2-calculator) says elites and rest sites are locked until floor 7. Either way: **no elite before ~floor 6-7**.
- [STS2] Elite nodes on the map: 5 at A0, **8 at A1+** ("Swarming Elites"; other guides say "~60% more"). So at the
  bot's ascension most routes cross more elites and avoiding them all is harder.
- [STS2] Rest sites: 6-7 in Acts 1-2, 5-6 in Act 3, rolled at map generation (spire-codex). (The old A6 "Gloom"
  = one fewer rest site per act was replaced by "Inflation" in v0.102/Major Update 1, April 2026.)
- [STS2] Treasure room is guaranteed at the midpoint of each act (wiki.gg).
- [STS2] An **Ancient** sits at the start of every act (Neow in Act 1; one random Ancient for Act 2 and one for
  Act 3). It is not a path choice, it is a fixed floor. It also heals (see ascension A2).
- [STS2] Unknown ("?") rooms: base odds Monster 10%, Treasure 2%, Shop 3%, remainder Event (~85%). Each type's
  chance rises by its base amount every time it does not occur and resets when it does. Elites do not roll from "?"
  by default. Shop cannot roll directly after a shop. (wiki.gg, spire-codex; a Steam thread reports fights feeling
  more common than 10% — treat as approximate.)
  - Rule: treat a "?" as roughly 0.1-0.2 fight + 0.8 event early, with fight odds growing as "?"s are chained.
- [STS2] Underdocks: alternate Act 1 with its own enemies/elites/events/bosses; once unlocked it replaces Overgrowth
  about 50% of the time (not chosen by the player). One source says Underdocks elites have more HP and more
  defensive abilities, and Overgrowth events are slightly more generous. [unverified beyond one source]

### Elite count targets (community numbers conflict — pick conservative for a bot)
- [STS2, conflict] Act 1: "1-2 elites at most" (several guides) vs. "fight elites a lot early and snowball" (Steam
  high-ascension thread). Act 2: "2-3" vs "0-2, only with strong AoE and scaling" vs "1-2". Act 3: "0-1", and "only
  with a fully assembled deck" because Act 3 elites have boss-level stats (~300 HP).
- [STS1 principle] Strong players take 2-3 Act 1 elites because relics compound; weak/automated play profits less
  because it loses more HP per elite. A bot should scale elite count to measured HP loss per elite.
- Suggested bot defaults (synthesis, [unverified]): Act 1 = 1 elite baseline, 2 if deck has taken 3+ good attack
  cards and HP is high; Act 2 = 1 (2 only if Act 1 boss was beaten with >60% HP left); Act 3 = 0-1.

### HP thresholds for elites
- [STS2, conflict] "Take Act 1 elites above 60% HP; below that route to a rest site"; "fight at >=60%, skip at
  <40%"; "fight elites when above 70% HP with both damage and block options".
- Rule: `elite_ok = hp_pct >= 0.65 (Act 1) / 0.70 (Acts 2-3)`, relax by ~0.10 if a rest site is the node
  immediately before the elite or 2+ useful potions are held; forbid if `hp_pct < 0.40`.
- [STS1 principle] Prefer paths where the elite is preceded by a rest site and followed by one (rest -> elite ->
  rest "sandwich"), and where the path forks before the elite so it can be dodged if HP is bad.

### Ordering within an act
- [STS2, multi] Open Act 1 with several normal fights (gold + card rewards) before the first elite. The first 2-3
  fights are the "weak" pool.
- [STS2, multi] Plan the route backwards from the boss: count elites, shops, rest sites, treasure on each candidate
  route before the first click.
- [STS2] "Take a route with enough normal fights to add cards and gold, one shop or rest site to stabilize, and only
  the elites the deck can cash in on."
- [STS1 principle] Act 1: prefer fights over "?" early (the deck needs cards); Act 2-3: prefer "?" over hallway
  fights (hallway fights are dangerous and card rewards matter less).
- [STS1 principle] Shop value depends on gold on arrival: route a shop when the bot will have >= ~150 gold (one
  removal + a card/potion), ideally mid/late act; do not path into a shop with < 75 gold.
- [STS2] Rest sites become more valuable from A2 on because the Ancient no longer restores full HP between acts.

### Sources
- https://spire-codex.com/mechanics/map-generation
- https://spire-codex.com/mechanics/unknown-rooms
- https://slaythespire.wiki.gg/wiki/Slay_the_Spire_2:Unknown_Location
- https://slaythespire.wiki.gg/wiki/Slay_the_Spire_2:Map_Locations
- https://sts2-calculator.com/v1032-map-rule-changes/
- https://metabot.gg/en/slay-the-spire-2/guides/map-pathing-and-routing
- https://sts2guides.com/en/guides/pathing.html
- https://slaythespire-2.com/guides/map-navigation-guide
- https://sts2front.com/tips/elite-encounters-guide/
- https://senseos.gg/guides/slay-the-spire-2/acts-compared/
- https://www.sts2front.com/mechanics/alternate-acts/
- https://steamcommunity.com/app/2868840/discussions/0/839502025821692445/ (High Ascension tips thread)
- https://steamcommunity.com/app/2868840/discussions/0/839501596191702159/ (unknown-room fight odds thread)

---

## 2. Rest sites

- [STS2, multi] Default options: **Rest** = heal 30% of max HP (rounded down); **Smith** = upgrade one card. One
  option per site.
- [STS2] Extra options are added by relics/items. Confirmed in excerpts: a **Hatch** option exists (the Byrdonis
  egg from the "Byrdonis Nest" event is hatched at a rest site); relic **Miniature Tent** lets you take any number
  of options at a rest site. Other STS1-style options (Dig/Lift/etc.) were **not confirmed** for STS2. [unverified]
- [STS2] There is also an event called **Unrest Site** (Overgrowth) — an event, not a rest site.
- Thresholds seen (conflicting but in the same band):
  - [STS2] ">= 65% HP: Smith is usually correct. <= 40%: Rest is usually correct."
  - [STS2] "< 40% HP: rest almost always wins unless the upgrade is a multi-fight cornerstone. > 80%: smith almost
    always wins unless the next room is a boss."
  - [STS2] "Heal at the pre-boss rest site unless already above 80% HP."
  - [STS2] "The closer to the boss, the more Rest gains priority. If another campfire comes before the boss, smith
    now and heal later."
  - [STS2] "#1 beginner mistake is always choosing Rest"; at A10 guides say upgrade rather than rest by default.
- Suggested rule set:
  1. Pre-boss rest site: Rest if `hp_pct < 0.80`, else Smith.
  2. Otherwise: Rest if `hp_pct < 0.40`; Smith if `hp_pct >= 0.65`.
  3. In the 0.40-0.65 band: Rest if the next 2 nodes include an elite or there is no further rest site before the
     boss; else Smith.
  4. If a Hatch (egg) option is available, [unverified] take it at the first site where `hp_pct >= 0.65`.
- Upgrade priority:
  - [STS2] Upgrade the win condition / build-defining card first; do not spend a smith on a card that does not
    meaningfully change damage output or survivability.
  - [STS1 principle] Order: (1) key Power/scaling card whose upgrade changes cost or doubles effect, (2) the most
    frequently played premium attack/AoE card, (3) cards whose upgrade reduces cost to 0/1, (4) best block card.
    Never upgrade starter Strikes/Defends unless nothing else exists.
  - [STS2] A7 "Scarcity" halves how often upgraded cards appear in rewards/shops -> campfire upgrades gain value
    from A7 on.

### Sources
- https://slaythespire.wiki.gg/wiki/Slay_the_Spire_2:Rest_Sites
- https://slaythespire.wiki.gg/wiki/Slay_the_Spire_2:Unrest_Site
- https://gamestrategyhub.com/games/slay-the-spire-2/rest-smith-remove/
- https://sts2-calculator.com/tools/rest-site-optimizer/
- https://slaythespire-2.com/blog/20260409-when-to-heal
- https://allthings.how/slay-the-spire-2-healing-explained/
- https://www.thegamer.com/slay-the-spire-2-events-guide-all-events-best-choices-strategy/ (Byrdonis Nest egg/hatch)

---

## 3. Card rewards / deck building

### Skip discipline and deck size
- [STS2, conflict] Deck size targets quoted: "20-25", "15-25", "under 30", and an aggressive "12-15 with strong
  synergies". Consensus band for a generic bot: **~20-25 cards by end of Act 2, hard cap ~30**.
- [STS2] Skip test: "does the card fit what the deck is already doing? If not an immediate yes, skip", and "what bad
  draw does this card replace? If nothing, skip".
- [STS2] "Skip 60-70% of Act 3 card rewards unless they directly perfect the existing plan." Act 1: "take early
  power, skip bad rewards".
- [STS2] Vile's Steam guide (v3.0): consistency over raw power; every pick judged against the deck's current needs
  and the fights ahead; smaller decks and removal are emphasised; do not commit rigidly to an archetype.
- Rule sketch: pick-rate by act — Act 1 take ~70-80% of rewards (anything above a quality bar), Act 2 ~40-50%,
  Act 3 ~30%; raise the quality bar by one tier each time deck size exceeds 22, 26, 30.

### What each phase demands
- [STS1 principle] **Act 1 = front-loaded damage.** Starter decks cannot kill Act 1 elites fast enough; the first
  3-5 picks should be efficient attacks (roughly >= 9-10 damage per energy, or damage plus Vulnerable). Scaling
  powers picked on floors 1-5 are too slow for the first elite.
- [STS2, multi] A10-climb guides echo this: "early damage, one or two AoE cards, and block options" are the
  priorities; remove Strikes early.
- [STS2] Act 1 elites specifically reward: fast kill + Weak (Byrdonis ramps Strength every turn); burst in one turn
  (Bygone Effigy); the ability to play through status cards (Phrog Parasite's Infection); multi-turn sustained
  damage rather than one burst (Skulking Colony's per-turn HP-loss cap); early block (Terror Eel 25+ early).
- [STS1 principle] **Act 2 = AoE + scaling + real block.** Multi-enemy hallway fights and elites that hit for 20-30
  per turn. Need by Act 2 start: at least 1-2 AoE sources, at least one scaling source (Strength/Dex/powers/orb or
  equivalent), and block of ~15-20 per turn on demand.
- [STS2] Hive elites confirm this: Decimillipede is multi-segment and multi-hit (total block matters, segments can
  revive -> focused sustained single-target), Entomancer hits 18-21 per turn, Infested Prism has ~200 HP.
- [STS1 principle] **Act 3 = sustained scaling + defense loop.** Bosses have 400-600 HP in STS2 (Queen 400 + minion
  199, Test Subject 600 across 3 phases, Aeonglass 512), so the deck needs damage that grows over 8-12 turns and a
  repeatable way to block 30+.
- [STS2] At A10 two Act 3 bosses are fought back to back: favour sustainable engines over glass-cannon burst, and
  check the boss icons early in Act 3 to shape picks.
- [STS2] "You need a damage clock and a defensive loop that survives while the clock works" (boss guides, general).

### Other pick rules
- [STS1 principle] Count roles, not card names: track `n_frontload`, `n_aoe`, `n_scaling`, `n_block`, `n_draw`.
  Give a bonus to the role that is below target for the upcoming act; penalise a 4th+ card of a role already full.
- [STS1 principle] Card draw/energy is worth more as the deck gets stronger; 0-cost and cantrip cards do not really
  "bloat".
- [STS1 principle] Curses/status-adding cards need a strong payoff; Powers that take 3+ turns to pay off are poor in
  hallway fights and great in boss fights — weigh by which is the current bottleneck.
- [STS2] Under A7 Scarcity rares are ~half as common: do not plan around finding specific rares; build on
  commons/uncommons.
- [STS2] A5 adds the Ascender's Bane curse to the starting deck (unplayable, Ethereal, cannot be removed per the
  excerpts): effectively +1 dead draw per cycle — slightly raises the value of draw and of thin decks.

### Sources
- https://gamestrategyhub.com/games/slay-the-spire-2/when-to-skip-cards/
- https://sts2guides.com/en/guides/deck-building.html
- https://www.spirebuilds.com/guides/understanding-card-rewards
- https://casualgameguides.com/walkthroughs/slay-the-spire-2/consistent-deck-and-skip-cards
- https://slaythespire-2.com/guides/beginner-tips
- https://steamcommunity.com/sharedfiles/filedetails/?id=3704645422 (Vile's strategy guide 3.0)
- https://gameguidesbox.com/slay-the-spire-2/ascension-9-10-climb-guide/
- https://spire-codex.com/mechanics/bosses-and-elites

---

## 4. Shops

### Prices (spire-codex merchant tables; ±small random variance)
- [STS2] Card removal: **75 gold first, +25 per use** (75/100/125/150). At **A6+ (Inflation): 100 first, +50 per
  use** (100/150/200...).
- [STS2] Cards: Common 48-52, Uncommon 71-79, Rare 142-158; colorless +15%.
- [STS2] Relics: Common 149-201, Uncommon 191-259, Rare 234-316, Shop relics 170-230 (after v0.103.2 cut all relic
  base prices by 25).
- [STS2] Potion price: one page says ~50 gold. [unverified]
- [STS2] A3 "Poverty": enemies and chests give 25% less gold -> fewer purchases per run; prioritise harder.
- [STS2] **Membership Card** = 50% off shop prices; **Smiling Mask** = removal costs a flat 50 (overrides the
  Membership Card discount for removal).
- [STS2] A "Fake Merchant" exists (spire-codex merchant page title). Details not seen. [unverified]

### Priorities
- [STS2, multi] First card removal is "the single best purchase in the game"; remove basic Strikes first (6 damage;
  almost any card is better). Curses first if present and removable. [STS1 principle] After Strikes, remove Defends
  only once the deck has better block.
- [STS2] One explicit ranking: **S-tier relic > card removal > key archetype card > potions > A-tier relic > rest**.
  Another: energy relics are the best shop purchase besides removal.
- [STS2] Do not buy potions if the gold is needed for a removal or a key card.
- Rule sketch (below A6): at a shop with gold G: (1) if G >= removal cost and deck has a Strike/curse -> remove;
  (2) if a top-tier relic is affordable -> buy (may precede removal if both cannot be afforded and relic is
  S-tier); (3) buy a card only if it fills a missing role (AoE/scaling/block) for the coming act; (4) with
  leftover >= 50 and an empty potion slot -> buy the best potion.
- At A6+: the 2nd and later removals (150+, 200+) are usually worse than a relic; cap paid removals at ~1-2 per run
  unless Smiling Mask is owned. [unverified inference]
- [STS1 principle] Gold targets: arrive at the first shop with >= 150; do not hoard past Act 3's last shop — unspent
  gold is worth nothing. Spend before the boss if a shop is on the path.

### Sources
- https://spire-codex.com/merchant
- https://spire-codex.com/mechanics/shop-inventory
- https://sts2front.com/tips/deck-thinning-strategy/
- https://sts2front.com/tips/gold-economy-guide/
- https://sts2guides.com/en/guides/gold-shop.html
- https://patched.gg/games/slay-the-spire-2/beta-patch-notes-v01020 (A6 Inflation rework)

---

## 5. Potions

- [STS2, conflict] Timing philosophy: "save potions for elites and bosses (strong decks can even skip elites and
  hold the 2 best for the boss)" vs "use common potions in hard hallway fights to save HP; never hoard". Shared
  principle: **drink when it changes the outcome** (prevents lethal / large HP loss, or makes an elite safe).
- [STS2] In-fight order: scaling potions first (Strength/Dexterity type — they gain value every turn), defensive
  potions reactively on the specific big-hit turn, burst potions at the kill window.
- [STS2] Potion-of-Doom-type effects go at the start of long boss fights; a Strength potion early vs Lagavulin
  Matriarch to end the fight before her debuffs stack.
- [STS2, multi] Highly rated: **Fairy in a Bottle** (prevents death — hold it, it triggers itself), **Entropic
  Brew**, **Fruit Juice** (permanent max HP — drink immediately), also Duplicator, Liquid Memories, Snecko Oil named.
- [STS2] A4 "Tight Belt": one fewer potion slot. Slots fill faster -> use potions more freely; a full belt wastes
  every potion drop.
- [STS2] At A10: enter the double boss with every slot full.
- Rule sketch:
  1. Fruit Juice-type permanent potions: use at once (any fight).
  2. If belt is full and the fight is not trivial: use the lowest-value potion on turn 1 (frees a slot for drops).
  3. Lethal prevention: if predicted unblockable incoming damage >= current HP after best block line -> use
     defensive potion (block/Weak/Ghost-type) or a kill-enabling damage potion, whichever removes lethal.
  4. Elite/boss: use scaling potions on turn 1; hold burst for a turn where it secures a kill or skips an enemy's
     big attack turn.
  5. Hallway: use a potion if expected HP loss without it exceeds ~10-15% of max HP [STS1 principle; threshold is
     my estimate].
  6. Before an Act boss, never hold back anything except Fairy in a Bottle.
- [STS1 principle] Potion drop chance rises after each fight without a drop, so spent potions get replaced; hoarding
  is the more common error than overuse.

### Sources
- https://sts2guides.com/en/guides/potions.html
- https://slaythespiretwo.com/articles/potion-guide-and-tier-list-all-63-potions-in-slay-the-spire-2
- https://gamerblurb.com/articles/slay-the-spire-2-potions-guide-full-list-and-best-potions
- https://slaythespire2.gg/potions
- https://sts2-calculator.com/potions/

---

## 6. Combat tactics

### Core math (matches STS1)
- [STS2, multi] **Vulnerable**: +50% attack damage taken. **Weak**: -25% attack damage dealt. **Frail**: -25% block
  gained from cards.
- [STS2] Formula quoted: `final = floor((base + Strength) * vuln * weak)` per hit, times hits; Strength is added
  before multipliers, so Strength counts once **per hit** (multi-hit attacks scale best with Strength, and enemy
  multi-hits scale worst for you when the enemy gains Strength).
- [STS2] One excerpt says multi-hit "subtracts enemy block only once from the total" — that is just block being a
  pool; simulate hit by hit.
- [STS2] Debuff counters tick down by 1 at end of turn (spire-codex).
- Ordering rules: apply Vulnerable before the big hits in the same turn; apply Weak before the enemy's attack turn
  (worth `0.25 * incoming`); play Strength/scaling before attacks; play draw before deciding the rest.

### Block vs damage
- [STS2] "Block first when you cannot kill this turn; if the enemy is not attacking, push offense."
- [STS2] Enemies follow fixed patterns; at higher ascension knowing the big-hit turn in advance lets the bot plan
  block for it.
- [STS1 principle] Lethal check first, every turn: if total reachable damage (with Vulnerable/Strength, potions
  included) >= remaining enemy HP + block, kill instead of blocking — dead enemies deal 0.
- [STS1 principle] Value of block = damage actually prevented; over-blocking is wasted energy. Value of damage =
  turns of enemy attacks removed. Versus scaling enemies (Strength every turn) damage is worth more; versus enemies
  with a known burst turn, hold block cards/energy for that turn.
- [STS1 principle] Leftover-energy rule: never end the turn with playable energy unless every remaining card is
  harmful to play.
- [STS1 principle] Taking chip damage to set up a Power on turn 1-2 is correct in long (elite/boss) fights and
  wrong in short hallway fights.

### Kill ordering (multi-enemy)
- [STS1 principle] Priority: (1) anything killable this turn that is attacking; (2) summoners/buffers/healers; (3)
  highest damage-per-HP enemy; (4) leave non-attacking/defending enemies for last. AoE first when it puts several
  enemies into single-target lethal range.
- [STS2] Exceptions seen in sources: enemies that punish killing in the wrong order (Kaiser Crab claws buff the
  survivor; Decimillipede segments revive; Phrog Parasite spawns Wrigglers on death) and buffers that should die
  first (Spectral Knight in the Glory knights elite; Kin Priest/Followers — sources conflict).

### Status / curse cards
- [STS2] Status cards named in sources: Dazed (Entomancer adds them when hit by Attack cards), Wound (Vantom's big
  hit), Infection (Phrog Parasite), Beckon (Soul Fysh; costs 6 unblockable damage if still in hand at end of turn —
  play/discard it), Wither (Aeonglass).
- [STS1 principle] Exhaust/discard statuses whenever it is free; in status-heavy fights the fight clock matters —
  the deck degrades every cycle, so race.
- [STS2] Vile's guide v3.0 has a dedicated section on status-heavy fights and target priority (content not
  retrievable).

### STS2-new mechanics (verified to exist in sources)
- [STS2] **Enchantments**: permanent run-long modifiers attached to individual cards (from events/Ancient relics;
  e.g. Orobas's Electric Shrymp enchants a Skill so it auto-plays at combat start). An enchanted card is worth more
  than its base — avoid removing/transforming it.
- [STS2] **Afflictions**: temporary negative modifiers that enemies put on cards in your deck/hand; they typically
  clear after combat. E.g. the Queen's "Chains of Binding" hits 3 random cards in hand so only one of them can be
  played; Ceremonial Beast's "Ringing" limits you to one card per turn.
- [STS2] **Sly** (Silent keyword): if a Sly card is discarded from hand during your turn it is played for free.
  Bot rule: when choosing discards, discard Sly cards first (a discard is a free play).
- [STS2] **Doom** (Necrobinder): stacking debuff; the enemy dies when Doom >= its current HP. Timing per one source
  is end of the enemy's turn; another says instantly, ignoring block. [conflict] Bot rule: count
  `effective_hp = hp - doom` for lethal planning but still block the turn it resolves.
- [STS2] **Osty / Summon** (Necrobinder): Osty has its own HP bar; unblocked attack damage hits Osty first, overflow
  hits the player; Osty's HP is raised only by Summon. Treat Osty HP as a second block pool that persists across
  turns.
- [STS2] **Souls** (Necrobinder token cards that draw), **Ethereal** (exhausts if in hand at end of turn),
  **Eternal** (cannot be removed; on curses such as Ascender's Bane).
- [STS2] **Stars / Forge / Sovereign Blade** (Regent): Stars are a second resource; Forge builds up the Sovereign
  Blade over the fight (long-term scaling). Exact rules not in excerpts. [unverified details]
- [STS2] **Orbs** (Defect): Channel/Evoke as in STS1, plus a new **Glass** orb (decaying AoE, favours fast fights).
- [STS2] **Slippery** (Vantom), **Hardened Shell** (Skulking Colony: caps HP lost per turn, ~20), **Plating**
  (Lagavulin Matriarch starts with 12), **Intangible** (Soul Fysh, Test Subject: damage reduced to 1) — on
  capped/Intangible turns spend energy on block and setup, not damage.
- [STS2] **Quests**: quest-type cards exist in STS2 (card pool listings), but no rules were retrievable.
  [unverified]

### Sources
- https://spire-codex.com/mechanics/combat-mechanics
- https://www.spirebuilds.com/guides/status-effects-guide
- https://slaythespire-2.com/guides/combat-mechanics-guide
- https://sts2-calculator.com/mechanics/
- https://slaythespire2.space/mechanics/
- https://metabot.gg/en/slay-the-spire-2/guides/keywords-terms-glossary
- https://slaythespire.wiki.gg/wiki/Slay_the_Spire_2:Osty
- https://slaythespire.wiki.gg/wiki/Slay_the_Spire_2:Necrobinder
- https://spire-codex.com/guides/characters-guide
- https://www.keengamer.com/articles/guides/slay-the-spire-2-bosses-and-how-to-beat-them/

---

## 7. Ascension modifiers (10 levels, cumulative, tracked per character)

Names/order below are the post-Major-Update-1 (v0.103.2, April 2026) list, consistent across wiki.gg, spire-codex
and several guides. Some guide sites describe a 20-level ladder — that is wrong/auto-generated; ignore.

| A | Name | Effect | Adaptation |
|---|------|--------|------------|
| 1 | Swarming Elites | Elite nodes on map 5 -> 8 (~+60%) | More forced elites; pathing must actively dodge or prepare; more relic upside. |
| 2 | Weary Traveler | Ancients heal only 80% (of missing HP) between acts. [conflict: some guides add "start run at 80% HP"; one says it affects rest sites — the Ancient version is best supported] | Do not rely on the act-transition heal; pre-boss Rest more often; value rest sites higher. |
| 3 | Poverty | Enemies and chests give 25% less gold | ~1 fewer purchase per run; removal + 1 relic is the realistic budget. |
| 4 | Tight Belt | One fewer potion slot | Use potions sooner; never sit on a full belt. |
| 5 | Ascender's Bane | Start with Ascender's Bane curse (unplayable, Ethereal, Eternal/unremovable) | One dead draw per cycle; slightly favours draw and smaller decks. |
| 6 | Inflation | Card removal 100 gold, +50 per use (was 75/+25). Replaced "Gloom" (fewer rest sites) in v0.102 | Paid removal: usually just the first one or two; relics relatively better. |
| 7 | Scarcity | Rare and upgraded cards appear ~half as often (rewards and shop) | Build on commons/uncommons; Smith more. |
| 8 | Tough Enemies | Enemy HP (and per-move block) up, per-enemy values (e.g. Axebot 70-78 -> 76-86; roughly +5-10%) | Fights last ~1 turn longer; scaling worth more. |
| 9 | Deadly Enemies | Enemy attack damage up, per-enemy (e.g. Axebot hit 12 -> 14; roughly +10-15%) | More block needed; Weak worth more; HP thresholds for elites +5-10 points. |
| 10 | Double Boss | Two Act 3 bosses back to back at the end | Enter with full potions, near-full HP; sustainable engine rather than one-shot burst. |

- For the bot at A4-5: active modifiers are A1-A5. Biggest behavioural changes: elite-dense maps (A1), weaker act
  heal (A2), less gold (A3), 1 fewer potion slot (A4), permanent dead card (A5).
- [STS2] Win-rate context: Mega Crit's May 2026 newsletter gives ~16% average win rate at A0 and ~17% at A10
  (players who reach A10 are better); other trackers show 13.8-29% at A10 depending on sample.
- [STS2] Enemy-specific A8/A9 numbers change nearly every patch; do not hard-code — read intents.

### Sources
- https://slaythespire.wiki.gg/wiki/Slay_the_Spire_2:Ascension
- https://spire-codex.com/mechanics/ascension-modifiers (localized mirror seen: /spa/)
- https://spire-codex.com/mechanics/enemy-ascension-scaling (localized mirror seen: /esp/)
- https://spire-codex.com/ascensions/level_09
- https://www.pcgamesn.com/slay-the-spire-2/ascension-levels
- https://www.keengamer.com/articles/guides/slay-the-spire-2-all-ascension-levels-guide/
- https://patched.gg/games/slay-the-spire-2/beta-patch-notes-v01020
- https://senseos.gg/guides/slay-the-spire-2/ascension-levels/
- https://www.megacrit.com/news/2026-5-22-neowsletter-issue-22/
- https://gameguidesbox.com/slay-the-spire-2/ascension-9-10-climb-guide/

---

## 8. Elites and bosses per act

HP values: base (higher-ascension value in parentheses where a source gave one). Patterns are from community
guides that often disagree — flagged where so.

### Act 1 — Overgrowth
Elites: Bygone Effigy, Byrdonis, Phrog Parasite.
- **Bygone Effigy** (~127 HP): starts asleep (1 or 2 turns — conflict), then alternates Slashes with a self-buff;
  one guide says its damage grows with each card you play that turn. Plan: use sleep turns for setup (powers,
  Vulnerable), then burst; avoid long fights.
- **Byrdonis** (~81-94 HP): loop of Peck (3x3) and Swoop (~16-17); gains Strength every turn (multi-hit Peck scales
  fast). Plan: race; apply Weak on its attack turns; do not stall for setup.
- **Phrog Parasite** (~61-64 HP): shuffles Infection status cards into the deck; spawns Wrigglers on death (second
  phase). Conflict: "kill it first so Wrigglers die with it" vs "save resources for phase two". Plan: keep AoE /
  some block for the Wrigglers after the main body dies.
Bosses: Vantom, Ceremonial Beast, The Kin (Kin Priest + Kin Followers).
- **Vantom**: has Slippery (shell stripped by cheap/multi-hit attacks). One guide: 4-turn cycle — T1 attack + apply
  Slippery, T2 attack, **T3 big hit (25+) that adds Wounds**, T4 Strength buff. Other guides name moves Ink Blot,
  Inky Lance, Dismember, Prepare. [conflict] No Artifact -> Vulnerable works. Plan: hold block for the big hit
  every cycle; use small hits to strip Slippery before heavy attacks.
- **Ceremonial Beast**: 252 HP (262). Phase 1: Stamp gives itself Plow, then Plow attacks with growing Strength.
  When HP drops below ~150 it is stunned and loses the accumulated Strength. Phase 2 cycle: Beast Cry (applies
  Ringing = you may play only 1 card per turn), Stomp, Crush (+permanent Strength). Plan: set up early; cross the
  150 threshold with as much overkill as possible in one turn so the stun turn is used for a full burst; under
  Ringing play the single highest-value card (big block if it is attacking, biggest attack otherwise).
- **The Kin / Kin Priest**: Priest plus two Kin Followers. Conflict: "kill the two Followers first (lower HP, heavy
  melee)" vs "burst the Priest within ~5-6 turns before the adds overwhelm". A related tip: interrupt the Shaman
  before Dark Ritual/Strength loops make Frail and Weak unbearable. AoE is strong here. Default for a bot: Followers
  first if either dies within 2 turns of focus, else Priest. [unverified]

### Act 1 — Underdocks
Elites: Phantasmal Gardener(s), Skulking Colony, Terror Eel.
- **Skulking Colony** (~75-79 HP): Hardened Shell caps HP loss per turn (one source: 20); moves include Zoom and
  Inertia (raises its damage). Plan: do not over-invest in damage past the cap each turn — spend the remainder on
  block; still kill before Inertia stacks (needs ~4 turns minimum).
- **Terror Eel** (140 HP): moves Crash and Thrash; can deal 25+ in a single early turn. Plan: block heavily early,
  use Weak; potion-worthy at low HP.
- **Phantasmal Gardener** (~28-32 HP each; appears as a group): little strategy available. [unverified] AoE likely
  good.
Bosses: Waterfall Giant, Soul Fysh, Lagavulin Matriarch.
- **Soul Fysh** (211 HP): adds Beckon cards (6 unblockable damage if held at end of turn — always play/discard
  them); goes Intangible on some turns (one guide: turns 4 and 6); another guide: a "Consume" attack every 4 turns
  that scales. Plan: front-load damage before Intangible turns; on Intangible turns block/setup only.
- **Lagavulin Matriarch** (222 HP): asleep for 3 turns behind 12 Plating; after waking she applies Strength and
  Dexterity debuffs to the player that accumulate, and builds block/Strength. Move lists conflict (Slash,
  Disembowel, block+attack, Soul Siphon). Plan: use the 3 sleep turns for powers/scaling (do not chip pointlessly
  into Plating), then end the fight fast; scaling potions early.
- **Waterfall Giant** (~250 HP, single source): sources diverge — a "Cascade" at 50% HP adding Strength and
  removing its block pauses, vs. a death-triggered Steam Eruption that scales with fight length. Plan: balanced
  block + damage, Vulnerable for the large HP pool; keep block available on the kill turn. [unverified]

### Act 2 — Hive
Elites: Decimillipede, Entomancer, Infested Prism(s).
- **Decimillipede**: 3 segments (front/middle/back), each ~40-46 HP (46-52). Moves: Writhe (5x2), Bulk (6 (7) and
  +2 Strength), Constrict, Dead, Reattach — dead segments revive while others live. Fatal to ~11.8% of runs in one
  community tally (most dangerous Act 2 elite). Plan: block the summed multi-hit damage; kill segments close
  together in time (AoE to bring all low, then finish) so none reattaches. [kill-timing detail unverified]
- **Entomancer**: 145 HP (155). Moves: Pheromone Spit, Beeeees! (3x7), Spear (18 (20)). Adds Dazed to your deck
  when hit by Attack cards (one source). ~6.7% of runs die here. Plan: fewer, bigger attack cards rather than many
  small ones; non-Attack damage sources are ideal.
- **Infested Prism**: ~200 HP (215). "Vital Spark": first time each turn it takes Attack damage, the attacker gains
  1 Energy. Plan: open each turn with a cheap attack to bank the energy, then play the expensive card.
Bosses: Kaiser Crab, Knowledge Demon, The Insatiable.
- **Kaiser Crab**: two targets — Crusher 199 (209) and Rocket 189 (199). Crusher loop: Thrash, Enlarging Strike,
  Bug Sting, Adapt, Guarded Strike. Rocket loop: Targeting Reticle, Precision Beam, Charge Up, **Laser (the spike —
  block it)**, Recharge. Killing one claw gives the survivor Block and Strength. One source: back attacks deal +50%
  (facing matters). Plan: burst decks kill Rocket first, sustained decks Crusher first; ideally bring both low and
  kill them within 1-2 turns of each other.
- **Knowledge Demon**: presents "Demonic Choice" debuffs the player must pick between; multi-hit on turn 3; a
  turn-4 attack that heals it and adds permanent Strength. Called the most hated boss. Plan: aggressive race —
  kill before the third Demonic Choice; pick the debuff that hurts the current deck least.
- **The Insatiable** (321 HP): "Sandpit" timer starts at 4 and counts down each turn; gains permanent Strength.
  Plan: pure damage race; stalling loses.

### Act 3 — Glory
Elites: Knights (group of three including a Spectral Knight; "Knight Gang"/"Knight Trio", ~276 total HP), Mecha
Knight (~300 HP), Soul Nexus.
- **Knights**: kill the Spectral Knight (93 HP (97)) first — it buffs the other two.
- **Mecha Knight**, **Soul Nexus**: no pattern data found. [unverified] General advice: Act 3 elites have boss-level
  stats; take 0-1 and only with a complete deck.
Bosses (v0.107.1+): Queen, Test Subject, Aeonglass. (Doormaker was removed in Major Update 2 and replaced by
Aeonglass; old guides still list it.)
- **Queen**: 400 HP plus minion Torch Head Amalgam (199 HP). Turn 1 she applies Chains of Binding to 3 random cards
  in hand (only one of them can be played). Plans: kill the Amalgam first to cut incoming damage (default for
  defensive decks) or race the Queen. Weak on her is very efficient.
- **Test Subject**: 3 phases of 100 / 200 / 300 HP (600 total); one source says Intangible every other turn. Plan:
  do not overkill a phase with big cards (overkill is wasted at the phase boundary — [unverified]); save burst for
  non-Intangible turns; scaling matters, later phases have conditional attacks.
- **Aeonglass** (512 HP): 3-move cycle — heavy hit with self-block, double laser, escalation move that shuffles a
  Wither into your deck. Punishes high card-play volume; rewards ending the fight fast.
- [STS2] At A10 two of these are fought consecutively.

### Sources
- https://spire-codex.com/mechanics/bosses-and-elites
- https://spire-codex.com/monsters/entomancer
- https://spire-codex.com/monsters/decimillipede_segment
- https://spire-codex.com/monsters/ceremonial_beast
- https://senseos.gg/guides/slay-the-spire-2/bosses-and-elites/
- https://slaythespire2.space/elites/ and https://slaythespire2.space/guides/bosses/
- https://gamerblurb.com/articles/slay-the-spire-2-act-1-elites-guide-overgrowth-underdocks
- https://dood.gg/en/slay-the-spire-2/guides/maps/
- https://www.keengamer.com/articles/guides/slay-the-spire-2-bosses-and-how-to-beat-them/
- https://slaythespiretwo.com/articles/boss-guide-every-boss-in-slay-the-spire-2-and-how-to-beat-them
- https://www.sts2companion.com/bosses/lagavulin-matriarch and /bosses/ceremonial-beast
- https://spiregenius.com/monsters/lagavulin-matriarch/
- https://sts2-hub.pages.dev/guides/bosses/ceremonial-beast
- https://www.slashskill.com/slay-the-spire-2-boss-guide-every-boss-attack-patterns-and-how-to-beat-them/
- https://www.spirebuilds.com/guides/slay-the-spire-2-all-bosses-guide
- https://mobalytics.gg/slay-the-spire-2/encounters/bosses and /encounters/elites
- https://www.gamespot.com/gallery/slay-the-spire-2-bosses/2900-7563/
- https://brokenbuilds.gg/slay-the-spire-2/guides/slay-the-spire-2-act-3-boss-relic-guide
- https://sts2front.com/tips/elite-encounters-guide/

---

## 9. Events and Ancients

### Ancients (structure)
- [STS2, multi] 8 Ancients. **Neow** always at Act 1 start. Act 2: one of **Orobas, Pael, Tezcatara**. Act 3: one
  of **Nonupeipe, Tanx, Vakuu**. **Darv** can appear in either Act 2 or 3 (rarer; pool resembles STS1 boss relics,
  e.g. Pandora's Box, Runic Pyramid).
- [STS2] Each Ancient offers 3 relics from its pool and one must be picked (Orobas: one from each of 3 sub-pools).
  These replace STS1 boss relics. The Ancient also heals the player (80% at A2+).
- [STS2, conflict] Neow: older guides describe 4 blessing options (2 after an Act 1 death); newer ones say it is a
  relic offer with positive options and "curse-side" options carrying a downside.

### Neow picks seen in sources
- [STS2] Rated top by various lists: **Silver Crucible** (next 3 card rewards are upgraded; costs the first chest —
  "strongest starting relic" per two guides), **Arcane Scroll** (random Rare card — one excerpt said relic), **Lead
  Paperweight**, **Small Capsule** (top community Elo), **Booming Conch** (recommended in the high-ascension Steam
  thread), **Neow's Talisman**, **Neow's Bones**, **Golden Pearl** (150 gold, no downside).
- [STS2] Downside options: **Cursed Pearl** (333 gold + the curse Greed), **Neow's Torment** (adds the card Neow's
  Fury — build-around or dead weight), **Hefty Tablet**. Other named options: **Winged Boots** (ignore paths 3
  times), **Leafy Poultice**.
- Bot default ranking [unverified synthesis]: Silver Crucible / Booming Conch / Arcane Scroll / Lead Paperweight /
  Small Capsule > Golden Pearl > Cursed Pearl (only below A6 where removal is cheap) > Neow's Torment.

### Act 2 / Act 3 Ancient picks seen in sources
- **Pael** (Act 2): "energy relics are the most universally applicable Act 2 choice"; Pael's Tears (especially with
  Ice Cream) and Pael's Flesh favoured; also Pael's Eye, Pael's Blood, Pael's Wing (sacrifice card rewards for
  relics).
- **Orobas** (Act 2): Driftwood (card-reward rerolls), Electric Shrymp (enchant a Skill to auto-play at combat
  start), Glass Eye, Alchemical Coffer, Black Star, Sealed Throne, Forbidden Grimoire, Touch of Orobas all named
  as strong by at least one guide.
- **Tezcatara** (Act 2): called the weakest Act 2 Ancient; Very Hot Cocoa (extra starting energy) and Storybook
  are the suggested picks; Pumpkin Candle gives energy but only during Act 2.
- **Nonupeipe** (Act 3): Fur Coat ("seven free combat wins") rated top by one guide. **Tanx**: attack-themed pool
  (e.g. Tri-Boomerang, needs 3+ enchantable Attacks). **Vakuu**: no pick data found.
- General rule [STS1 principle + STS2 guides]: unconditional energy-per-turn relic > draw/consistency relic >
  situational; avoid picks whose downside disables a system the bot relies on (potions, gold, intents).

### Events
- [STS2] ~56 events. Generic rules from guides: free card removal is almost always the best option; HP-for-relic
  trades are fine above ~50% HP; transform > upgrade late (Act 3) if Strikes/Defends remain.
- Specific picks seen:
  - **Byrdonis Nest**: take the egg early in the run (hatch it at a rest site); take the max HP late if no rest
    sites remain.
  - **Self-Help Book**: "Read the Entire Book" (39.3% win rate vs 27.3% event average over 5,945 choices).
  - **Grave of the Forgotten**: accept Forgotten Soul (free relic, no downside); "Confront Truth" only with exhaust
    synergy.
  - **Aroma of Chaos**: transform if Strikes/Defends remain, upgrade if key cards are unupgraded.
  - **Amalgamator**: first option (merges/replaces Strikes) is generally better.
  - **Doll Room**: "Examine Each and Make the Best Choice" (more HP cost, 1 of 3 relics; 46.7% vs 40.6% win rate).
  - **Crystal Sphere**: guides favour the Debt option (divine 6 times) over paying 56 gold (3 times).
  - "Fight" options for a relic in events are generally worth it with HP above the elite threshold.
- [STS1 principle] Default event policy for a bot: never take a curse unless the reward is a relic or >= 150 gold
  equivalent; value max HP at ~ (act-dependent) 8-10 gold per point; value a removal at ~100 gold; decline HP costs
  above 15% of max HP when `hp_pct < 0.5`.

### Sources
- https://sts2wiki.org/ancients
- https://gamestier.com/slay-the-spire-2-ancients-guide/
- https://mobalytics.gg/slay-the-spire-2/guides/ancients
- https://selphie1999gaming.com/game-guides/slay-the-spire-2/the-best-ancient-relics-to-get-in-slay-the-spire-2/
- https://slaythespire2.net/ancient
- https://slaythespire.net/ancient-relics/
- https://www.neonlightsmedia.com/blog/slay-the-spire-2-all-ancients-relics-guide
- https://brokenbuilds.gg/slay-the-spire-2/guides/slay-the-spire-2-act-2-boss-relic-guide
- https://sts2.gg/neow-relics and https://sts2.gg/guides/neow-relic-guide
- https://spire-codex.com/guides/neow-relic-offer
- https://sts2-calculator.com/tools/neow-relic-evaluator/
- https://www.thegamer.com/slay-the-spire-2-events-guide-all-events-best-choices-strategy/
- https://metabot.gg/en/slay-the-spire-2/events
- https://sts2guides.com/en/guides/events.html
- https://www.destructoid.com/all-events-and-choices-in-slay-the-spire-2/
- https://senseos.gg/guides/slay-the-spire-2/events-and-choices/

---

## 10. Relics

### Consistently top-rated, character-independent
- [STS2, multi] **Ice Cream** (unspent energy carries over), **Gambling Chip**, **Paper Phrog**, **Tungsten Rod**,
  **Bag of Preparation** — "rarely skip on any character".
- [STS2] **Prismatic Gem** (energy relic with little downside), **Membership Card** (50% shop discount — buy early),
  **Smiling Mask** (flat 50 removal; much better at A6+), **Miniature Tent** (multiple rest-site options).
- [STS2] Win-rate tracker outliers: History Course (~85% win rate, low pick rate), Sword of Jade, Iron Club.
  [single source]
- [STS2] sts2.gg S-tier (Ancient/Neow-type): Booming Conch, Blessed Antler, Blood-Soaked Rose, Pumpkin Candle,
  Runic Pyramid, Whispering Earring, Spiked Gauntlets, Very Hot Cocoa, Music Box, the Pael set.
- [STS2] Golden Compass: 2 guaranteed chests mid-act instead of 1.

### Trap / high-risk relics (matter a lot for a bot)
- [STS2] **Runic Dome** (+1 energy, hides enemy intents): never take — a rule-based bot depends on intents.
- [STS2] **Ectoplasm** (+1 energy, no more gold): skip (one guide: skip even at zero gold), especially early.
- [STS2] **Sozu** (+1 energy, cannot obtain potions): significant cost for boss fights; acceptable only late and if
  the bot's potion use is weak anyway.
- [STS2] **Velvet Choker** (+1 energy, max 6 cards played per turn): bad for decks that play many cheap cards.
- [STS2] **Philosopher's Stone** (+1 energy, enemies +1 Strength): harsh versus the many multi-hit enemies; C tier.
- [STS2] **Glass Cannon** (doubles damage taken — "most punishing trap"), **Busted Crown**, **Brimstone** (gives
  enemies the same Strength it gives you): avoid by default.
- [STS2] **Pandora's Box** (Darv): high variance; only when many starter cards remain. [STS1 principle]
- Rule: classify relics as (a) unconditional stat/energy/draw, (b) conditional on deck tags, (c) downside relics.
  Auto-take (a); take (b) only when the tag count matches; take (c) only from an explicit allowlist.

### Sources
- https://www.keengamer.com/articles/guides/slay-the-spire-2-relics-tier-list/
- https://progameguides.com/slay-the-spire-2/slay-the-spire-2-relics-tier-list/
- https://nat1gaming.com/sts2/tier-list/relic-tier-list/
- https://metabot.gg/en/slay-the-spire-2/relics/tier-list
- https://www.stratgg.com/relics/
- https://sts2guides.com/en/guides/best-relics.html
- https://www.neonlightsmedia.com/blog/slay-the-spire-2-relics-tier-list
- https://sts2.gg/neow-relics
- https://spire-codex.com/merchant

---

## Appendix A — Quick rule table for the bot (synthesis; thresholds are starting points to tune)

| Decision | Rule |
|---|---|
| Elite, Act 1 | allow if hp >= 65% and >= 3 non-starter attack/damage cards; max 2 |
| Elite, Act 2 | allow if hp >= 70% and deck has AoE + scaling; default 1 |
| Elite, Act 3 | allow only if hp >= 75% and last boss was won with > 50% HP; max 1 |
| Rest vs Smith | <40% rest; >=65% smith; between: rest if elite within 2 nodes or no later campfire |
| Pre-boss campfire | rest unless hp >= 80% |
| Card reward | Act 1 take most damage cards; from Act 2 require role-fit; tighten at 22/26/30 cards |
| Shop | removal (75 +25; A6+: 100 +50) > top relic > role-filling card > potion (~50) |
| Potions | permanent ones at once; scaling T1 of elite/boss; defensive on spike turn or to stop lethal |
| Combat | lethal check > avoid lethal on self > Vulnerable/Weak before hits > block only what is incoming |
| Capped/Intangible enemy turn | spend on block and setup, not damage |
| Trap relics | never Runic Dome; avoid Ectoplasm, Glass Cannon, Busted Crown, Brimstone |

## Appendix B — Things not verifiable in this session (re-check when page access is available)
- Exact A2 wording (start HP 80%?), exact A8/A9 per-enemy numbers.
- Whether any relic adds further rest-site options beyond Hatch / Miniature Tent.
- Quest-card rules; Stars/Forge exact rules; Doom timing.
- Move tables for Vantom, Waterfall Giant, Knowledge Demon, Test Subject, Mecha Knight, Soul Nexus, Phantasmal
  Gardener; Kin kill order.
- Best single data sources to scrape once allowed: spire-codex.com/monsters/<name>, slaythespire.wiki.gg
  (Slay_the_Spire_2: namespace), sts2.untapped.gg guides, Vile's Steam guide (id=3704645422).
