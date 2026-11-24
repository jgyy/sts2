"""Cards that pay off only on what other cards make (Poison, Shivs, Souls, cards Exhausted), and the cards that make it."""

import unittest
from unittest import mock

from sts2bot.cards import deck_value
from sts2bot.run import decks, strategy


class Payoffs(unittest.TestCase):
    def test_a_payoff_is_worth_little_until_the_deck_holds_what_feeds_it(self):
        silent = lambda cid, name, ctype, cost, text: {"id": cid, "name": name, "type": ctype, "cost": cost, "rarity": "Uncommon", "description": text}
        bubble = silent("BUBBLE_BUBBLE", "Bubble Bubble", "Skill", 1, "If the enemy has Poison, apply 9 Poison.")
        accuracy = silent("ACCURACY", "Accuracy", "Power", 1, "Shivs deal 4 additional damage.")
        poison = silent("DEADLY_POISON", "Deadly Poison", "Skill", 1, "Apply 5 Poison.")
        fumes = silent("NOXIOUS_FUMES", "Noxious Fumes", "Power", 1, "At the start of your turn, apply 2 Poison to ALL enemies.")
        dance = silent("BLADE_DANCE", "Blade Dance", "Skill", 1, "Add 3 Shivs into your Hand. Exhaust.")
        starter = [{"id": "STRIKE_SILENT", "name": "Strike", "type": "Attack", "cost": 1, "description": "Deal 6 damage."}] * 5
        starter += [{"id": "DEFEND_SILENT", "name": "Defend", "type": "Skill", "cost": 1, "description": "Gain 5 Block."}] * 5
        worth = lambda c, deck: deck_value(c) + strategy.deck_fit(c, deck, 2)
        # Nothing to feed it: Bubble Bubble does nothing (and its 9 Poison is no hit for the lean to damage).
        self.assertLess(worth(bubble, starter), 0.4 * worth(bubble, starter + [fumes]))
        self.assertLess(worth(bubble, starter), worth(bubble, starter + [poison]))
        self.assertLess(worth(bubble, starter + [poison]), worth(bubble, starter + [poison, poison]))
        for fed in ([fumes], [poison, poison]):  # fed in full: nothing taken off (a Power feeds it every turn, so one is enough)
            with mock.patch.object(decks, "FEED_KEEP", 1.0):  # (the rule off)
                whole = worth(bubble, starter + fed)
            self.assertAlmostEqual(worth(bubble, starter + fed), whole)
        self.assertLess(worth(accuracy, starter), 0.4 * worth(accuracy, starter + [dance, dance]))
        self.assertAlmostEqual(strategy.deck_fit(accuracy, starter + [dance, dance], 2), 0.0)  # fed: its rating, no more
        # ... and once one is held, what feeds it is wanted, until there is enough of it.
        self.assertGreater(strategy.deck_fit(dance, starter + [accuracy], 2), strategy.deck_fit(dance, starter, 2) + 0.9)
        self.assertGreater(strategy.deck_fit(dance, starter + [accuracy], 2), strategy.deck_fit(dance, starter + [accuracy, dance], 2))
        self.assertAlmostEqual(strategy.deck_fit(poison, starter + [accuracy], 2), strategy.deck_fit(poison, starter, 2))  # not Accuracy's
        # A card that does something of its own is no payoff (Soul Storm deals its 9 whatever the Souls).
        storm = silent("SOUL_STORM", "Soul Storm", "Attack", 1, "Deal 9 damage. Deals 4 additional damage for each Soul in your Exhaust Pile.")
        self.assertIsNone(decks.pays_off(storm))


if __name__ == "__main__":
    unittest.main()
