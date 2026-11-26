"""Slay the Spire 2's events one by one (sts2bot/run/events/sts2.py): the choice each rule makes, and that every event of the game
is covered by a rule or was checked against the generic scorer."""

import unittest

from sts2bot import games
from sts2bot.run import events, strategy
from sts2bot.run.events import sts2

# Every event with options in the game's localization/eng/events.json (v0.113.1), 2026-11-26.
GAME_EVENTS = """ABYSSAL_BATHS AMALGAMATOR AROMA_OF_CHAOS BATTLEWORN_DUMMY BRAIN_LEECH BUGSLAYER BYRDONIS_NEST COLORFUL_PHILOSOPHERS COLOSSAL_FLOWER
CRYSTAL_SPHERE DENSE_VEGETATION DOLL_ROOM DOORS_OF_LIGHT_AND_DARK DROWNING_BEACON ENDLESS_CONVEYOR FIELD_OF_MAN_SIZED_HOLES GRAVE_OF_THE_FORGOTTEN
HUNGRY_FOR_MUSHROOMS INFESTED_AUTOMATON JUNGLE_MAZE_ADVENTURE LOST_WISP LUMINOUS_CHOIR MOCK_EVENT_MODEL MORPHIC_GROVE MUSEUM_OF_TORTURE POTION_COURIER
PUNCH_OFF RANWID_THE_ELDER REFLECTIONS RELIC_TRADER ROOM_FULL_OF_CHEESE ROUND_TEA_PARTY SAPPHIRE_SEED SELF_HELP_BOOK SHRINE_OF_THE_WARLORD SLIPPERY_BRIDGE
SPIRALING_WHIRLPOOL SPIRIT_GRAFTER STONE_OF_ALL_TIME SUNKEN_STATUE SUNKEN_TREASURY SYMBIOTE TABLET_OF_TRUTH TEA_MASTER THERMAE_NONUPEIPE
THE_FUTURE_OF_POTIONS THE_LANTERN_KEY THE_LEGENDS_WERE_TRUE THIS_OR_THAT TINKER_TIME TRASH_HEAP TRIAL UNREST_SITE WAR_HISTORIAN_REPY
WATERFALL_OF_OBVIOUS_SECRETS WATERLOGGED_SCRIPTORIUM WELCOME_TO_WONGOS WELLSPRING WHISPERING_HOLLOW WOOD_CARVINGS WUPPY_IN_TROUBLE ZEN_WEAVER""".split()

STRIKE = {"name": "Strike", "id": "STRIKE_IRONCLAD", "type": "Attack", "cost": "1", "description": "Deal 6 damage."}
DEFEND = {"name": "Defend", "id": "DEFEND_IRONCLAD", "type": "Skill", "cost": "1", "description": "Gain 5 Block."}
BASH = {"name": "Bash", "id": "BASH", "type": "Attack", "cost": "2", "description": "Deal 8 damage. Apply 2 Vulnerable."}
STARTER = [STRIKE] * 5 + [DEFEND] * 4 + [BASH]


def pick(event_id, options, hp=60, max_hp=80, gold=100, act=1, floor=5, deck=STARTER, potions=(), slots=2):
    """The title of the option the bot takes."""
    games.select("sts2")
    mem = strategy.Memory()
    mem.deck = list(deck)
    opts = [{"index": i, "title": t, "description": d} for i, (t, d) in enumerate(options)]
    state = {"state_type": "event", "run": {"act": act, "floor": floor}, "event": {"event_id": event_id, "options": opts},
             "player": {"hp": hp, "max_hp": max_hp, "gold": gold, "potions": list(potions), "relics": [], "max_potion_slots": slots}}
    index = next(a[1]["index"] for a in events.event(state, mem) if a[0] == "choose_event_option")
    return opts[index]["title"]


class EveryEvent(unittest.TestCase):
    def test_every_event_of_the_game_has_a_rule_or_was_checked(self):
        self.assertEqual(sorted(set(GAME_EVENTS) - sts2.COVERED), [])


class Sts2Events(unittest.TestCase):
    def test_battleworn_dummy_is_fought_where_it_can_be_killed(self):
        # 0 of 22 logged fights killed the 300 HP dummy (125 dealt on average): nothing was ever won.
        opts = [("Setting 3", "Fight a 300 HP dummy. Obtain a random Relic."), ("Setting 2", "Fight a 150 HP dummy. Upgrade 2 random cards."),
                ("Setting 1", "Fight a 75 HP dummy. Procure 1 random Potion.")]
        self.assertEqual(pick("BATTLEWORN_DUMMY", opts), "Setting 1")
        big = [{"name": "Bludgeon", "type": "Attack", "cost": "1", "description": "Deal 40 damage."}] * 12
        self.assertEqual(pick("BATTLEWORN_DUMMY", opts, deck=big), "Setting 3")  # (120 a turn)

    def test_waterfall_beat_up_s_relic_beats_be_nice_s_card(self):
        opts = [("Be Nice", "Obtain Dingy Dagger."), ("Beat Up", "Obtain a random relic. Add 2 Guilty to your Deck.")]
        self.assertEqual(pick("WATERFALL_OF_OBVIOUS_SECRETS", opts), "Beat Up")

    def test_amalgamator_combines_strikes(self):
        opts = [("Combine Defends", "Remove 2 Defends. Add Ultimate Defend to your Deck."), ("Combine Strikes", "Remove 2 Strikes. Add Ultimate Strike to your Deck.")]
        self.assertEqual(pick("AMALGAMATOR", opts), "Combine Strikes")

    def test_colorful_philosophers_takes_cards_that_work_alone(self):
        opts = [("Pink", "Obtain 3 Necrobinder cards."), ("Orange", "Obtain 3 Regent cards."), ("Green", "Obtain 3 Silent cards.")]
        self.assertEqual(pick("COLORFUL_PHILOSOPHERS", opts), "Green")

    def test_colossal_flower_goes_down_for_its_relic_with_hp_to_spare(self):
        opts = [("Extract Nectar", "Gain 35 Gold."), ("Reach Deeper", "Enter deeper. Lose 5 HP.")]
        self.assertEqual(pick("COLOSSAL_FLOWER", opts, hp=75), "Reach Deeper")
        self.assertEqual(pick("COLOSSAL_FLOWER", opts, hp=20), "Extract Nectar")
        self.assertEqual(pick("COLOSSAL_FLOWER", [("Extract Nectar", "Gain 140 Gold."), ("Enter the Center", "Lose 7 HP. Obtain Pollinous Core.")], hp=60), "Enter the Center")

    def test_doors_a_curse_is_better_gone_than_a_card_upgraded(self):
        opts = [("Light Door", "Upgrade a card."), ("Dark Door", "Remove 1 card from your Deck.")]
        self.assertEqual(pick("DOORS_OF_LIGHT_AND_DARK", opts, deck=STARTER + [{"name": "Regret", "type": "Curse"}]), "Dark Door")

    def test_losing_all_gold_is_not_the_end_of_the_run(self):
        opts = [("Loner", "Gain 5 Max HP."), ("Group", "Lose ALL of your Gold. Transform 2 cards.")]
        self.assertEqual(pick("MORPHIC_GROVE", opts, gold=10), "Group")
        self.assertEqual(pick("MORPHIC_GROVE", opts, gold=400), "Loner")
        self.assertGreater(events._event_option_value({"title": "Group", "description": "Lose ALL of your Gold. Transform 2 cards."}, 60, 80), -100)

    def test_a_curse_named_curse_is_charged_once(self):
        take = events._event_option_value({"title": "Take It", "description": "Obtain Thumbscrew. Add a random Curse to your Deck."}, 60, 80)
        self.assertGreater(take, -1.0)  # (was -6.5: the curse twice)

    def test_sunken_statue_s_sword_wants_five_elites(self):
        opts = [("Grab the Sword", "Obtain the Sword of Stone."), ("Dive into the Water", "Gain 105 Gold. Lose 7 HP.")]
        self.assertEqual(pick("SUNKEN_STATUE", opts, hp=40), "Dive into the Water")

    def test_wuppy_soaks_more_for_an_injury(self):
        opts = [("Be Cautious", "Lose 2 HP. Obtain Wuppy with 30 HP."), ("Help Immediately", "Add an Injury to your Deck. Obtain Wuppy with 60 HP."),
                ("Ignore", "Procure a random potion.")]
        self.assertEqual(pick("WUPPY_IN_TROUBLE", opts), "Help Immediately")

    def test_round_tea_party_royal_poison_only_near_the_end(self):
        opts = [("Pick a Fight", "Lose 11 HP. Obtain a random Relic."), ("Enjoy Your Tea", "Obtain Royal Poison. Heal to full HP.")]
        self.assertEqual(pick("ROUND_TEA_PARTY", opts, hp=60, floor=20, act=2), "Pick a Fight")
        self.assertEqual(pick("ROUND_TEA_PARTY", opts, hp=20, floor=45, act=3), "Enjoy Your Tea")

    def test_the_spoils_map_is_nothing_in_act_3(self):
        opts = [("Nab the Map", "Receive the Spoils Map."), ("Slowly Find an Exit", "Lose 8 HP. Procure 1 random Potion.")]
        self.assertEqual(pick("THE_LEGENDS_WERE_TRUE", opts, act=1), "Nab the Map")
        self.assertEqual(pick("THE_LEGENDS_WERE_TRUE", opts, act=3, floor=40, hp=70), "Slowly Find an Exit")

    def test_wood_carvings_slither_only_for_dear_cards(self):
        opts = [("Snake", "Enchant 1 card with Slither."), ("Bird", "Choose 1 starter card to Transform into Peck.")]
        self.assertEqual(pick("WOOD_CARVINGS", opts), "Bird")

    def test_endless_conveyor_free_gold_is_taken(self):
        opts = [("Observe the Chef", "Upgrade a random card in your Deck."), ("Grab Golden Fysh off the Belt", "LUCKY WINNER! Gain 150 Gold.")]
        self.assertEqual(pick("ENDLESS_CONVEYOR", opts), "Grab Golden Fysh off the Belt")
        opts[1] = ("Grab Seapunk Salad off the Belt", "Pay 40 Gold. Add Feeding Frenzy to your Deck. Continue feasting!")
        self.assertEqual(pick("ENDLESS_CONVEYOR", opts), "Observe the Chef")

    def test_trial_never_doubles_down(self):
        self.assertEqual(pick("TRIAL", [("Accept", "Give in. Serve as today's Decider."), ("Double Down", "Face lethal repercussions.")]), "Accept")
        merchant = [("DECIDE: Guilty", "Add Regret to your Deck. Obtain 2 random Relics."), ("DECIDE: Innocent", "Add Shame to your Deck. Upgrade 2 cards.")]
        self.assertEqual(pick("TRIAL", merchant), "DECIDE: Guilty")

    def test_the_lantern_key_is_returned_in_act_3(self):
        opts = [("Return the Key", "Gain 100 Gold."), ("Keep the Key", "Fight to obtain the Key.")]
        self.assertEqual(pick("THE_LANTERN_KEY", opts, act=3, floor=38, hp=80), "Return the Key")

    def test_unrest_site_poor_sleep_holds_a_place_in_every_hand(self):
        opts = [("Rest Anyways", "Heal to full HP. Receive Poor Sleep."), ("Kill the Trees", "Lose 8 Max HP. Obtain a random Relic.")]
        self.assertEqual(pick("UNREST_SITE", opts, hp=70), "Kill the Trees")

    def test_luminous_choir_two_removals_for_spore_mind(self):
        opts = [("Offer Tribute", "Pay 144 Gold. Obtain a random Relic."), ("Reach into the Flesh", "Remove 2 cards from your Deck. Add Spore Mind to your Deck.")]
        self.assertEqual(pick("LUMINOUS_CHOIR", opts, gold=150), "Reach into the Flesh")

    def test_a_rule_that_fails_leaves_the_option_to_the_generic_scorer(self):
        ctx = events.context({"player": {"hp": 50, "max_hp": 80}, "run": {"act": 1, "floor": 3}}, strategy.Memory())
        self.assertIsNone(events.rule_value("NOT_AN_EVENT", {"title": "x"}, ctx))
        self.assertIsNone(events.rule_value("ABYSSAL_BATHS", {"title": None, "description": None}, ctx))
