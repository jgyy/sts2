"""The Fool's cards have no run statistics, only a writer's tiers (crowd.FOOL_RATING): they stand in for the crowd's part
of a rating, so that Aim and Melody cards the parser cannot read are not rated below the Shots a Fool holds one Arrow for."""

import unittest
from unittest import mock

from sts2bot import cards, learn
from sts2bot.run import crowd

from ..fake_game import card

QUICK_SHOT = ("QUICK_SHOT", "Quick Shot", "Attack", 0, "Shoot an Arrow to deal 10 damage.", "Common")


def rated(key, name, ctype, cost, text, rarity):
    return cards.deck_value(card(0, key, name, ctype, cost, text, "Self", rarity))


class FoolTiers(unittest.TestCase):
    def test_every_card_of_the_list_is_rated_and_the_tiers_are_what_was_set(self):
        self.assertEqual(len(crowd.FOOL_RATING), 79)
        self.assertEqual(sorted(set(crowd.FOOL_RATING.values())), [3.3, 4.5, 5.8, 7.2])
        self.assertEqual((crowd.rating("WAH_CHO_KI"), crowd.rating("WAH_HAMMER"), crowd.rating("HEAVY_SHOT"), crowd.rating("QUICK_SHOT")), (7.2, 5.8, 4.5, 3.3))
        self.assertEqual(crowd.rating("YAH"), 5.8)  # "Yah!" is spelt YAH as an id

    def test_the_tables_of_the_other_characters_are_left_alone(self):
        self.assertEqual(len(crowd.RATING), 5 * 80 + 33)
        self.assertTrue(set(crowd.RATING).isdisjoint(crowd.FOOL_RATING))
        self.assertIsNone(crowd.rating("NO_SUCH_CARD"))

    def test_an_aim_card_the_parser_cannot_read_beats_a_shot_that_it_can(self):
        with mock.patch.dict(learn.KNOB, {"crowd": 0.6}):
            ready, quick = rated("CHO_READY", "Cho Ready", "Skill", 1, "Gain 8 Block. Gain 3 Aim.", "Uncommon"), rated(*QUICK_SHOT)
        self.assertGreater(ready, quick)  # 3.2 against 3.5 before the tiers, a Fool's top pick in 129 of 360 offers

    def test_a_setting_of_zero_leaves_the_text_rating_alone(self):
        with mock.patch.dict(learn.KNOB, {"crowd": 0.0}):
            with_tiers = rated(*QUICK_SHOT)
            with mock.patch.dict(crowd.FOOL_RATING, {}, clear=True):
                self.assertEqual(with_tiers, rated(*QUICK_SHOT))


SORROW = card(0, "SORROWFUL_STRIKE", "Sorrowful Strike", "Attack", 1, "Deal 13 damage. Add 2 Sadness into your Hand.", "AnyEnemy", "Common")
ENNUI = card(0, "ENNUI", "Ennui", "Power", 1, "At the start of your turn, add 1 Sadness to your Hand and draw 2 additional cards.", "Self", "Rare")
PUNCH = card(0, "PUNCH_OF_JOY", "Punch of Joy", "Attack", 2, "Deal 13 damage. Transform 1 random Sadness in your Hand into Joy.", "AnyEnemy", "Uncommon")
PLAIN = [card(0, "STRIKE_FOOL", "Strike", "Attack", 1, "Deal 6 damage.", "AnyEnemy", "Basic")] * 10


class SadnessInTheDeck(unittest.TestCase):
    """Sadness stays in the hand all fight: a deck takes its makers only as far as the hands have room for them, or Joy to turn them into."""

    def test_makers_lose_rating_as_the_sadness_piles_up(self):
        from sts2bot.run import decks
        first, second = decks.deck_fit(SORROW, PLAIN, 2), decks.deck_fit(SORROW, PLAIN + [SORROW], 2)
        self.assertLess(second, first - 1.5)  # (the copy's 0.6, and a Sadness over the room)
        self.assertLess(decks.deck_fit(ENNUI, PLAIN + [SORROW], 2), decks.deck_fit(ENNUI, PLAIN, 2))
        self.assertGreater(decks.deck_fit(ENNUI, PLAIN + [SORROW, PUNCH, PUNCH], 2), decks.deck_fit(ENNUI, PLAIN + [SORROW], 2))  # (Joy takes them away)
        self.assertGreater(decks.deck_fit(PUNCH, PLAIN + [SORROW, SORROW], 2), decks.deck_fit(PUNCH, PLAIN, 2))
