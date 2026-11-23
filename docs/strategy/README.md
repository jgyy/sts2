# Strategy notes for the bot

Community strategy for Slay the Spire 2, collected on 2026-11-20 for the bot to consider. Nothing here is wired
into the code yet: these are candidates to try, one at a time, against the bot's own run history.

- [`general.md`](general.md): map, rest sites, card rewards, shops, potions, combat, the ten ascension levels,
  every act's elites and bosses, Ancients, events, relics.
- [`characters.md`](characters.md): each character's kit, archetypes, cards to take and to skip, removal,
  tactics and hard fights.

## How far to trust it

- The research session could not open web pages: both files are built from **search-result excerpts** of the
  pages they list, not from full reads. No Reddit thread or video was reachable.
- Every bullet is tagged (`[STS2]`, seen in several sources, conflicting, STS1 principle, unverified). Many STS2
  guide sites are auto-generated and disagree on numbers; `characters.md` ranks the sources by reliability.
- Where a note disagrees with a number the bot measured itself (the comments in `sts2bot/` quote them), the
  bot's number wins until a test says otherwise. The ratings in `cards.py` are also deliberately about how *this
  bot* plays a card, not how a person would.

## What the bot already does

| Advice | Where the bot has it |
|---|---|
| No elite on a starter deck; elites only from a healthy position (guides: 60-70% HP) | `strategy.path_value`: act 1 elites before row 8 cost 32 HP; a penalty below 75% HP |
| Plan the route back from the boss | `strategy.best_onward` scores every whole route |
| Rest before the boss (guides: unless above 80%) | `strategy.rest_site`: rests below 95% with the boss next |
| Front-loaded damage in act 1 | `strategy.card_reward`: +0.8 for Attacks while the act 1 deck is under 18 cards |
| Card removal first in a shop, then relics | `strategy.shop` |
| Do not sit on a full potion belt; keep potions for the boss | `potions.potion_actions` |
| Defensive potions on the turn something big is coming | `potions._DEFENSIVE_POTION` |
| Discard Sly cards first | `strategy.hand_select` |
| Use a sleeping enemy's turns for set-up | `combat.choose_card` (Asleep) |
| Kaiser Crab: do not leave one claw alive for long | `battle._rage_blocks` and the Crab Rage cost in `combat.simulate` |
| The Insatiable is a timer | the Sandpit handling in `combat.simulate` |
| Osty's HP is Block that stays, and worth keeping; Summon before Unleash | `combat._context`, `simulate` (every HP he keeps is priced), `cards.order_key` |
| Card ratings from run data | `crowd.py` (tierspire.com win and pick rates), mixed into `cards.deck_value` by the `crowd` setting |
| A deck needs damage before anything else | `strategy.deck_fit`: attacks are worth more while the deck does under 5 to 6 damage an energy |
| Fruit Juice at once | `potions.potion_actions` |
| Focus the enemy that does the most for its health | `combat._pick_target` in ordinary fights (`Bot._threat` remembers what each enemy has done) |
| Race an enemy that gains Strength every turn (Devoted Sculptor, cultists) | `Enemy.grows` raises what damage is worth in `combat.simulate` |
| Entomancer: few big attacks, since every Attack that hits it adds Dazed | `Enemy.hive`, priced per hit in `combat.simulate` |
| Decimillipede: bring the segments down together (a dead one is back in two turns while another lives) | `Enemy.reattach`, `combat._pick_target`: the healthiest segment is hit and none finished until the rest have 40 HP or less |
| The Regent's Star cards need Stars | `strategy.deck_fit`: Star cards lose and makers gain by what the deck is short (tightened 2026-11-21) |
| Arrive at the last boss on full HP | `strategy.boss_odds`: in act 3 a route is worth full marks only from 95% HP |
| Powers and Ethereal cards played while they can be, in long fights | `battle._power_worth` (not below what other players' results say), 1.6 times for Ethereal Powers |

## Candidates to try

Ordered by how cheap they look to test. Each needs a look at the game's own text first: names and numbers below
are second-hand.

1. **Relics the bot should refuse.** `strategy.relic_select` takes the first relic offered, and the shop only
   refuses Snecko Eye. Guides call these traps, and the first is fatal to a planner that reads intents: Runic
   Dome (hides intents), Ectoplasm (no more gold), Velvet Choker (six cards a turn), Glass Cannon, Busted Crown,
   Brimstone. Rank `relic_select` and skip these in the shop.
2. **Ancient relic ratings** (`events._ANCIENT`): done on 2026-11-20 for the second and third Ancients, from the
   bot's own runs and tierspire.com's win rates (cards every turn before energy with a drawback: Snecko Eye and
   Pael's Blood up; Philosopher's Stone, Sozu, Ectoplasm, Blessed Antler, Spiked Gauntlets and Looming Fruit
   down). Neow's offers were re-rated from the run history the same day; the crowd's numbers for them are too
   close together to add anything.
3. **Potions that are not for a fight's bad turn.** Fruit Juice is drunk at once now; Fairy in a Bottle is never
   drunk (the game does not offer it). Still open: one-turn potions (Flex, Energy, Swift, Gigantification,
   Duplicator) go on round 1 of a boss fight like the rest; pricing each by the planner's score with and without
   it would hold them for a turn that uses them.
4. **Which card to remove.** Guides: curse, then Strike, then Defend; the Regent one Strike and one Defend
   early; no early Defend removals for the Defect and the Necrobinder; the Silent removes Strikes hard.
   `strategy._removal_rank` goes by rating only.
5. **Roles in the deck.** Count front-loaded damage, AoE, scaling, Block and draw, and add to a reward that
   fills a role the next act needs (act 2: one or two AoE cards, one source of scaling, 15-20 Block on demand;
   act 3: damage that grows over 8-12 turns). `strategy.deck_fit` knows Stars, copies and, since 2026-11-20,
   damage an energy; AoE, scaling and draw are not counted yet.
6. **Card ratings that disagree with `cards._TIER`**: now measured through the `crowd` setting (see `crowd.py`)
   instead of by hand. The guides' points, for reference:
   - Ironclad: guides rate Unmovable (4.0 here), Dark Embrace (3.0), Colossus (3.0) and Evil Eye (3.5) near the
     top as an Exhaust engine, and say to skip Cinder (4.0) and to leave Demon Form (8.0) until after act 1.
   - Silent: Prepared (2.5) and Acrobatics (5.0) are the key enablers of the discard/Sly deck, rated the
     strongest; Poison is rated the weakest and needs Noxious Fumes or Accelerant.
   - Necrobinder: Reap (3.0) and Sow (5.0) are called traps; Grave Warden (6.0) the best common, as here.
   - Regent: upgrade Glow first; extra copies of Falling Star lose value; skip mediocre commons.
   - Defect: Lightning Rod (4.5) high; Focus only for the first one or two cards; damage first in act 1.
7. **Fight rules not in the planner yet.**
   - Entomancer adds Dazed for each Attack card that hits it: done on 2026-11-20 (the game's text confirms it).
   - Skulking Colony's Hardened Shell caps the HP it loses in a *turn*; the planner's `hit_cap` is per hit.
   - Ceremonial Beast under Ringing: the one card should be the biggest Block when it attacks. The planner sorts
     by damage plus Block (`choose_card`, few plays allowed), which already comes close.
   - Soul Fysh and Test Subject on Intangible turns: Block and set-up only. The planner prices hits at 1 HP there;
     check it also stops spending energy on them.
   - Infested Prism: guides say the first Attack each turn gives *us* an energy; the bot measured Vital Spark as
     harder hits per Skill. The bot's reading is from live fights; the guide may describe an older patch.
8. **Higher ascensions.** (A6, done 2026-11-21: `strategy.shop` no longer puts a removal above 125 gold before
   every relic.) A6: removal costs 100 and 50 more each time, so the second
   one is usually worse than a relic. A7: rares and upgraded cards half as common, so upgrades at rest sites are
   worth more (`upgrade_worth`). A8/A9: about a tenth more enemy HP and damage, so `FIGHT_COST` needs measuring
   again. A10: two act 3 bosses in a row, so arrive with a full belt and a deck that lasts.
9. **Unknown rooms** are about 85% events at first, with the odds of a fight growing each time none comes
   (`path_value` counts a flat 0.9 and no HP).

## Keeping this up to date

The game changes with every patch (Doormaker was replaced by Aeonglass in v0.107.1). With web access that can
open pages, the sources worth reading in full are spire-codex.com (monster and mechanics tables),
slaythespire.wiki.gg (`Slay_the_Spire_2:` pages), the tier lists on sts2.untapped.gg and the Steam guides for
app 2868840.
