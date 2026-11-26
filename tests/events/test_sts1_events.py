"""The first game's events one by one (sts2bot/run/events/sts1.py): the choice each rule makes, and that every event of the game
is covered by a rule or was checked against the generic scorer."""

import unittest

from sts2bot import games
from sts2bot.run import events, strategy
from sts2bot.run.events import sts1

# Every event in the game's localization/eng/events.json (desktop-1.0.jar), by the id the bot sees (cards.key_of), 2026-11-26.
GAME_EVENTS = """ACCURSED_BLACKSMITH ADDICT BACK_TO_BASICS BEGGAR BIG_FISH BONFIRE_ELEMENTALS COLOSSEUM CURSED_TOME DEAD_ADVENTURER DESIGNER
DRUG_DEALER DUPLICATOR FACETRADER FALLING FORGOTTEN_ALTAR FOUNTAIN_OF_CLEANSING GHOSTS GOLDEN_IDOL GOLDEN_SHRINE GOLDEN_WING KNOWING_SKULL LAB
LIARS_GAME LIVING_WALL MASKED_BANDITS MATCH_AND_KEEP MINDBLOOM MUSHROOMS MYSTERIOUS_SPHERE NEST NOTEFORYOURSELF N_LOTH PROCEED_SCREEN PURIFIER
SCRAP_OOZE SECRETPORTAL SENSORYSTONE SHINING_LIGHT SPIRE_HEART THE_CLERIC THE_JOUST THE_LIBRARY THE_MAUSOLEUM THE_MOAI_HEAD THE_WOMAN_IN_BLUE
THINKING_WITH_PUDDLES TOMB_OF_LORD_RED_MASK TRANSMORGRIFIER UPGRADE_SHRINE VAMPIRES WEMEETAGAIN WHEEL_OF_CHANGE WINDING_HALLS WORLD_OF_GOOP""".split()

STARTER = [{"name": "Strike", "id": "Strike_R", "type": "Attack", "cost": "1", "description": "Deal 6 damage."}] * 5 + \
          [{"name": "Defend", "id": "Defend_R", "type": "Skill", "cost": "1", "description": "Gain 5 Block."}] * 4 + \
          [{"name": "Bash", "id": "Bash", "type": "Attack", "cost": "2", "description": "Deal 8 damage. Apply 2 Vulnerable."}]


class Sts1Events(unittest.TestCase):
    def setUp(self):
        games.select("sts1")
        self.addCleanup(games.select, "sts2")

    def pick(self, event_id, options, hp=60, max_hp=80, gold=100, act=1, floor=5, deck=STARTER, potions=(), relics=()):
        mem = strategy.Memory()
        mem.deck = list(deck)
        opts = [{"index": i, "title": t, "description": d} for i, (t, d) in enumerate(options)]
        state = {"state_type": "event", "run": {"act": act, "floor": floor}, "event": {"event_id": event_id, "options": opts},
                 "player": {"hp": hp, "max_hp": max_hp, "gold": gold, "potions": list(potions), "relics": [{"id": r} for r in relics], "max_potion_slots": 3}}
        index = next(a[1]["index"] for a in events.event(state, mem) if a[0] == "choose_event_option")
        return opts[index]["title"]

    def test_every_event_of_the_game_has_a_rule_or_was_checked(self):
        self.assertEqual(sorted(set(GAME_EVENTS) - sts1.COVERED), [])

    def test_back_to_basics_upgrades_every_basic_card_early(self):
        opts = [("Elegance", "Remove a card from your deck."), ("Simplicity", "Upgrade all Strikes and Defends.")]
        self.assertEqual(self.pick("BACK_TO_BASICS", opts), "Simplicity")
        few = STARTER[4:5] + STARTER[-1:] + [{"name": "Shrug It Off", "type": "Skill", "cost": "1", "description": "Gain 8 Block. Draw 1 card."}] * 8
        self.assertEqual(self.pick("BACK_TO_BASICS", opts, deck=few), "Elegance")

    def test_masked_bandits_are_paid_off_when_gold_is_short(self):
        opts = [("Fight!", ""), ("Pay", "Lose ALL of your Gold.")]
        self.assertEqual(self.pick("MASKED_BANDITS", opts, gold=15, act=2, floor=25, hp=40), "Pay")
        self.assertEqual(self.pick("MASKED_BANDITS", opts, gold=300, act=2, floor=25, hp=80), "Fight!")

    def test_n_loth_gets_none_of_our_relics(self):
        opts = [("Offer: Pure Water", "Lose this relic. Obtain a special relic."), ("Leave", "")]
        self.assertEqual(self.pick("N_LOTH", opts, act=2, relics=("PureWater",)), "Leave")

    def test_the_woman_in_blue_fills_the_belt(self):
        opts = [("Buy 1 Potion", "20 Gold."), ("Buy 2 Potions", "30 Gold."), ("Buy 3 Potions", "40 Gold."), ("Leave", "")]
        self.assertEqual(self.pick("THE_WOMAN_IN_BLUE", opts), "Buy 3 Potions")
        full = [{"name": "Fire Potion"}, {"name": "Block Potion"}]
        self.assertEqual(self.pick("THE_WOMAN_IN_BLUE", opts, potions=full), "Buy 1 Potion")  # (one free slot)

    def test_falling_loses_the_card_we_miss_least(self):
        opts = [("Land", "Lose Shrug It Off"), ("Channel", "Lose Inflame"), ("Strike", "Lose Strike")]
        self.assertEqual(self.pick("FALLING", opts), "Strike")

    def test_the_library_reads_unless_badly_hurt(self):
        opts = [("Read", "Choose 1 of 20 cards to add to your deck."), ("Sleep", "Heal 26 HP.")]
        self.assertEqual(self.pick("THE_LIBRARY", opts, hp=75), "Read")
        self.assertEqual(self.pick("THE_LIBRARY", opts, hp=25), "Sleep")

    def test_scrap_ooze_reaches_while_the_odds_pay(self):
        opts = [("Leave", ""), ("Deeper", "Lose 5 HP. 45%: Find a Relic.")]
        self.assertEqual(self.pick("SCRAP_OOZE", opts, hp=60), "Deeper")
        self.assertEqual(self.pick("SCRAP_OOZE", opts, hp=8), "Leave")

    def test_tomb_of_lord_red_mask_for_little_gold(self):
        opts = [("Leave", ""), ("Offer", "40 Gold: Lose all Gold. Obtain a Relic.")]
        self.assertEqual(self.pick("TOMB_OF_LORD_RED_MASK", opts, gold=40, act=3, floor=38), "Offer")

    def test_colosseum_victory_is_two_elites_at_once(self):
        opts = [("COWARDICE", "Escape."), ("VICTORY", "A powerful fight with many rewards.")]
        self.assertEqual(self.pick("COLOSSEUM", opts, hp=80, act=2, floor=25), "COWARDICE")

    def test_neow_s_boss_relic_over_small_gifts(self):
        opts = [("", "Max HP +7"), ("", "Lose your starting Relic Obtain a random boss Relic")]
        self.assertEqual(self.pick("NEOW_EVENT", opts, floor=0), "")
        self.assertIn("boss", [o for o in ("Max HP +7", "Lose your starting Relic Obtain a random boss Relic")
                               if events.rule_value("NEOW_EVENT", {"title": "", "description": o}, events.context({"player": {"hp": 80, "max_hp": 80}, "run": {}}, strategy.Memory()))
                               is not None][0])
