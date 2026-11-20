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
| Kaiser Crab: do not leave one claw alive for long | `combat._rage_blocks` and the Crab Rage cost in `simulate` |
| The Insatiable is a timer | the Sandpit handling in `combat.simulate` |
| Osty's HP is Block that stays | `combat._context`, `simulate` |

## Candidates to try

Ordered by how cheap they look to test. Each needs a look at the game's own text first: names and numbers below
are second-hand.

1. **Relics the bot should refuse.** `strategy.relic_select` takes the first relic offered, and the shop only
   refuses Snecko Eye. Guides call these traps, and the first is fatal to a planner that reads intents: Runic
   Dome (hides intents), Ectoplasm (no more gold), Velvet Choker (six cards a turn), Glass Cannon, Busted Crown,
   Brimstone. Rank `relic_select` and skip these in the shop.
2. **Ancient relic ratings to re-check** (`events._ANCIENT`): the bot rates Philosopher's Stone 13.5 and Sozu
   12.5; guides put Philosopher's Stone low (enemy Strength against multi-hit attacks) and Sozu as costly for boss
   fights, which is where the bot's potions go. Guides rate Silver Crucible and Arcane Scroll (7.0 here) and
   Booming Conch (8.0) among the best Neow picks. Compare with the run history before moving any.
3. **Potions that are not for a fight's bad turn.** Fruit Juice (permanent Max HP) should be drunk at once;
   Fairy in a Bottle should never be drunk (it works by itself). `potion_actions` treats both like any potion.
4. **Which card to remove.** Guides: curse, then Strike, then Defend; the Regent one Strike and one Defend
   early; no early Defend removals for the Defect and the Necrobinder; the Silent removes Strikes hard.
   `strategy._removal_rank` goes by rating only.
5. **Roles in the deck.** Count front-loaded damage, AoE, scaling, Block and draw, and add to a reward that
   fills a role the next act needs (act 2: one or two AoE cards, one source of scaling, 15-20 Block on demand;
   act 3: damage that grows over 8-12 turns). `strategy.deck_fit` only knows Stars and copies.
6. **Card ratings that disagree with `cards._TIER`** (test through `learn.py`'s record, do not just copy):
   - Ironclad: guides rate Unmovable (4.0 here), Dark Embrace (3.0), Colossus (3.0) and Evil Eye (3.5) near the
     top as an Exhaust engine, and say to skip Cinder (4.0) and to leave Demon Form (8.0) until after act 1.
   - Silent: Prepared (2.5) and Acrobatics (5.0) are the key enablers of the discard/Sly deck, rated the
     strongest; Poison is rated the weakest and needs Noxious Fumes or Accelerant.
   - Necrobinder: Reap (3.0) and Sow (5.0) are called traps; Grave Warden (6.0) the best common, as here.
   - Regent: upgrade Glow first; extra copies of Falling Star lose value; skip mediocre commons.
   - Defect: Lightning Rod (4.5) high; Focus only for the first one or two cards; damage first in act 1.
7. **Fight rules not in the planner yet.**
   - Entomancer adds Dazed for each Attack card that hits it: prefer few big attacks. (One source.)
   - Skulking Colony's Hardened Shell caps the HP it loses in a *turn*; the planner's `hit_cap` is per hit.
   - Ceremonial Beast under Ringing: the one card should be the biggest Block when it attacks. The planner sorts
     by damage plus Block (`choose_card`, few plays allowed), which already comes close.
   - Soul Fysh and Test Subject on Intangible turns: Block and set-up only. The planner prices hits at 1 HP there;
     check it also stops spending energy on them.
   - Infested Prism: guides say the first Attack each turn gives *us* an energy; the bot measured Vital Spark as
     harder hits per Skill. The bot's reading is from live fights; the guide may describe an older patch.
8. **Higher ascensions, when the bot gets there.** A6: removal costs 100 and 50 more each time, so the second
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
