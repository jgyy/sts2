"""Powers that give energy every turn, at a card reward (decks.ENERGY_POWER; scripts/demonfight.py cards)."""

import unittest
from unittest import mock

from sts2bot.run import decks, strategy
from sts2bot import learn
from sts2bot.run.strategy import Memory


def c(cid, name, ctype, cost, text, rarity="Common"):
    return {"id": cid, "name": name, "type": ctype, "cost": str(cost), "rarity": rarity, "description": text}


E = "[necrobinder_energy_icon.png]"
STRIKE = c("STRIKE_NECROBINDER", "Strike", "Attack", 1, "Deal 6 damage.", "Basic")
DEFEND = c("DEFEND_NECROBINDER", "Defend", "Skill", 1, "Gain 5 Block.", "Basic")
BODYGUARD = c("BODYGUARD", "Bodyguard", "Skill", 1, "Summon 5.", "Basic")
UNLEASH = c("UNLEASH", "Unleash", "Attack", 1, "Osty deals 6 damage. Deals additional damage equal to Osty's current HP.", "Basic")
POKE = c("POKE", "Poke", "Attack", 0, "Osty deals 6 damage.")
REAVE = c("REAVE", "Reave", "Attack", 1, "Deal 10 damage. Add a Soul into your Draw Pile.")
GRAVE_WARDEN = c("GRAVE_WARDEN", "Grave Warden", "Skill", 1, "Gain 8 Block. Add a Soul into your Draw Pile.")
SCOURGE = c("SCOURGE", "Scourge", "Skill", 1, "Apply 13 Doom. Draw 1 card.")
NEGATIVE_PULSE = c("NEGATIVE_PULSE", "Negative Pulse", "Skill", 1, "Gain 5 Block. Apply 7 Doom to ALL enemies.")
SCULPTING_STRIKE = c("SCULPTING_STRIKE", "Sculpting Strike", "Attack", 1, "Deal 9 damage. Add Ethereal to a card in your Hand.")
BORROWED_TIME = c("BORROWED_TIME", "Borrowed Time", "Skill", 1, f"Gain 4{E}. Cards cost an additional {E} this turn.", "Uncommon")
FRIENDSHIP = c("FRIENDSHIP", "Friendship", "Power", 1, f"Lose 2 Strength. Gain {E} at the start of each turn.", "Uncommon")
DEMESNE = c("DEMESNE", "Demesne", "Power", 3, f"Ethereal. At the start of your turn, gain {E} and draw 1 additional card.", "Rare")
PYRE = c("PYRE", "Pyre", "Power", 2, "Gain [ironclad_energy_icon.png] at the start of each turn.", "Rare")
ORBIT = c("ORBIT", "Orbit", "Power", 2, "Every 4[regent_energy_icon.png] you spend, gain [regent_energy_icon.png].", "Uncommon")
AUTOMATION = c("AUTOMATION", "Automation", "Power", 1, f"Every 10 cards you draw, gain {E}.", "Uncommon")
NEUROSURGE = c("NEUROSURGE", "Neurosurge", "Power", 0, f"Gain {E}{E}{E}. Draw 2 cards. At the start of your turn, apply 3 Doom to yourself.", "Rare")
WISP = c("WISP", "Wisp", "Skill", 0, f"Gain {E}. Exhaust.")
INFLAME = c("INFLAME", "Inflame", "Power", 1, "Gain 2 Strength.", "Uncommon")

STARTER = [STRIKE] * 4 + [DEFEND] * 4 + [BODYGUARD, UNLEASH]


def chosen(offer, deck, act=1):
    """The card a reward is answered with (None: skipped)."""
    mem = Memory()
    mem.deck, mem.deck_size = list(deck), len(deck)
    state = {"state_type": "card_reward", "run": {"act": act}, "player": {"character": "The Necrobinder"}, "card_reward": {"cards": [dict(x, index=i) for i, x in enumerate(offer)]}}
    first = strategy.card_reward(state, mem)[0]
    return None if first[0] == "skip_card_reward" else offer[first[1]["card_index"]]["name"]


class TurnEnergy(unittest.TestCase):
    def test_what_a_power_gives_every_turn(self):
        self.assertEqual(decks.turn_energy(FRIENDSHIP), 1.0)
        self.assertEqual(decks.turn_energy(DEMESNE), 1.0)
        self.assertEqual(decks.turn_energy(PYRE), 1.0)
        self.assertAlmostEqual(decks.turn_energy(ORBIT), 0.75)
        self.assertAlmostEqual(decks.turn_energy(AUTOMATION), 0.55)

    def test_energy_once_is_not_energy_every_turn(self):
        for card in (NEUROSURGE, WISP, BORROWED_TIME, INFLAME, STRIKE):
            self.assertEqual(decks.turn_energy(card), 0.0, card["name"])


@mock.patch.dict(learn.KNOB, {"crowd": 0.3})  # (one of the two live settings; at it our own table says Friendship 4.5, Grave Warden 6.0)
class EnergyAtAReward(unittest.TestCase):
    def test_friendship_over_the_cards_that_used_to_beat_it(self):
        # 2026-11-22 11:48 and 12:30: Grave Warden 6.0 and Scourge 7.3 against Friendship 5.0-5.1, deck of 16.
        deck = STARTER + [REAVE, SCULPTING_STRIKE, POKE, GRAVE_WARDEN, NEGATIVE_PULSE, SCOURGE]
        self.assertEqual(chosen([GRAVE_WARDEN, FRIENDSHIP, BORROWED_TIME], deck), "Friendship")
        self.assertEqual(chosen([SCOURGE, FRIENDSHIP, NEGATIVE_PULSE], deck), "Friendship")

    def test_demesne_is_no_longer_skipped(self):
        # 2026-11-22 12:31: "deck 22, need 5.0: Demesne 4.1, Negative Pulse 4.0, Sculpting Strike 2.5" and nothing taken.
        deck = STARTER + [REAVE, SCULPTING_STRIKE, POKE, GRAVE_WARDEN, NEGATIVE_PULSE, SCOURGE] * 2
        self.assertEqual(chosen([DEMESNE, NEGATIVE_PULSE, SCULPTING_STRIKE], deck, act=2), "Demesne")

    def test_the_second_is_worth_less_and_more_energy_less_again(self):
        deck = STARTER + [REAVE, POKE, GRAVE_WARDEN]
        first = decks.deck_fit(DEMESNE, deck, 2)
        self.assertAlmostEqual(first, decks.ENERGY_POWER)
        self.assertAlmostEqual(decks.deck_fit(DEMESNE, deck + [PYRE], 2), first / 2)
        self.assertAlmostEqual(decks.deck_fit(DEMESNE, deck, 2, energy=4), first / 2)  # a relic's energy counts too

    def test_friendship_costs_more_where_every_hit_takes_strength(self):
        osty = STARTER[4:] + [POKE] * 4  # Defends, Bodyguard, Unleash and Pokes: nothing that takes Strength
        hitters = STARTER + [REAVE] * 4
        with mock.patch.object(decks, "_hitters", return_value=99):  # (the lean to attacks is not what is tested)
            self.assertGreater(decks.deck_fit(FRIENDSHIP, osty, 2), decks.deck_fit(FRIENDSHIP, hitters, 2) + 1.0)
        self.assertLess(decks.deck_fit(FRIENDSHIP, hitters + [FRIENDSHIP], 2), 0.0)  # a second one: less Strength still


if __name__ == "__main__":
    unittest.main()
