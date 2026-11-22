"""Powers that pay every turn of a long fight, at a card reward: energy (decks.ENERGY_POWER; scripts/demonfight.py cards)
and damage or Doom (decks.SCALE_POWER)."""

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
COUNTDOWN = c("COUNTDOWN", "Countdown", "Power", 1, "At the start of your turn, apply 6 Doom to a random enemy.", "Uncommon")
LETHALITY = c("LETHALITY", "Lethality", "Power", 1, "Ethereal. The first Attack each turn deals 50% additional damage.", "Uncommon")
REAPER_FORM = c("REAPER_FORM", "Reaper Form", "Power", 3, "Whenever Attacks deal damage, apply that much Doom.", "Rare")
CALCIFY = c("CALCIFY", "Calcify", "Power", 1, "Osty's attacks deal 4 additional damage.", "Uncommon")
HAUNT = c("HAUNT", "Haunt", "Power", 1, "Whenever you play a Soul, a random enemy loses 7 HP.", "Uncommon")
SHARED_FATE = c("SHARED_FATE", "Shared Fate", "Skill", 0, "Lose 2 Strength. Enemy loses 2 Strength. Exhaust.", "Rare")
SOUL_STORM = c("SOUL_STORM", "Soul Storm", "Attack", 1, "Deal 9 damage. Deals 4 additional damage for each Soul in your Exhaust Pile.", "Rare")
DEATHS_DOOR = c("DEATHS_DOOR", "Death's Door", "Skill", 1, "Gain 6 Block. If you applied Doom this turn, gain Block 2 additional times.", "Uncommon")
PARSE = c("PARSE", "Parse", "Skill", 1, "Ethereal. Draw 3 cards.", "Uncommon")

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


class TurnDamage(unittest.TestCase):
    def test_what_a_power_adds_every_turn(self):
        deck = STARTER + [REAVE, POKE, GRAVE_WARDEN]  # 13 cards; hits Strike 6 x4, Unleash 6, Reave 10, Poke 6
        self.assertEqual(decks.turn_damage(COUNTDOWN, deck), 6.0)
        self.assertEqual(decks.turn_damage(dict(COUNTDOWN, description=COUNTDOWN["description"].replace("6", "9")), deck), 9.0)
        self.assertAlmostEqual(decks.turn_damage(LETHALITY, deck), 0.5 * 46 / 7)  # half the deck's average hit
        self.assertAlmostEqual(decks.turn_damage(REAPER_FORM, deck), 4 * 46 / 13)  # four plays a turn of what the deck hits for
        self.assertAlmostEqual(decks.turn_damage(CALCIFY, deck), 4 * 4 * 2 / 13)  # Unleash and Poke
        self.assertAlmostEqual(decks.turn_damage(HAUNT, deck), 7 * 4 * 2 / 13)  # Reave and Grave Warden make a Soul each

    def test_nothing_for_what_does_not_deal_damage_every_turn(self):
        for card in (NEUROSURGE, INFLAME, FRIENDSHIP, DEMESNE, SCOURGE, REAVE):  # (Neurosurge's Doom every turn is on us)
            self.assertEqual(decks.turn_damage(card, STARTER), 0.0, card["name"])

    def test_capped(self):
        heavy = [c("BIG", "Big", "Attack", 1, "Deal 40 damage.")] * 10
        self.assertAlmostEqual(decks.deck_fit(REAPER_FORM, heavy, 2) - decks.deck_fit(dict(REAPER_FORM, description="Nothing."), heavy, 2), decks.SCALE_CAP)


@mock.patch.dict(learn.KNOB, {"crowd": 0.3})
class DamageAtAReward(unittest.TestCase):
    DECK = STARTER + [REAVE, SCULPTING_STRIKE, POKE, GRAVE_WARDEN, NEGATIVE_PULSE, SCOURGE, REAVE, SCULPTING_STRIKE]

    def test_reaper_form_over_the_cards_that_beat_it(self):
        # 2026-11-22 13:53 and 16:12: "Shared Fate 4.3, Soul Storm 3.9, Reaper Form 3.7", "Soul Storm 7.1, Undeath 5.1, Reaper Form 4.9".
        self.assertEqual(chosen([SHARED_FATE, SOUL_STORM, REAPER_FORM], self.DECK, act=2), "Reaper Form")

    def test_countdown_is_no_longer_skipped(self):
        # 2026-11-22 14:07: "deck 24, need 4.5: Death's Door 4.4, Parse 3.7, Countdown 3.6" and nothing taken.
        deck = self.DECK + [DEFEND, STRIKE] * 4 + [REAVE] * 2
        self.assertEqual(chosen([DEATHS_DOOR, PARSE, COUNTDOWN], deck, act=2), "Countdown")


TIMES_UP = c("TIMES_UP", "Time's Up", "Attack", 2, "Deal damage equal to the enemy's Doom.", "Uncommon")
BLIGHT_STRIKE = c("BLIGHT_STRIKE", "Blight Strike", "Attack", 2, "Deal 8 damage. Apply Doom equal to damage dealt.")
OBLIVION = c("OBLIVION", "Oblivion", "Skill", 0, "Whenever you play a card this turn, apply 3 Doom to the enemy.", "Rare")


@mock.patch.dict(learn.KNOB, {"crowd": 0.3})
class TimesUpNeedsDoom(unittest.TestCase):
    # 2026-11-22: Time's Up is only ever held by the Necrobinder, and in 20 of 63 fights holding it the deck had none of the
    # Doom cards the logs name (fights.jsonl); it deals the Doom an enemy already has, so there it does nothing.

    def test_what_puts_doom_on_an_enemy(self):
        for card in (SCOURGE, COUNTDOWN, REAPER_FORM, BLIGHT_STRIKE, NEGATIVE_PULSE, OBLIVION):
            self.assertTrue(decks.makes_doom(card), card["name"])
        for card in (NEUROSURGE, STRIKE, REAVE, TIMES_UP):  # (Neurosurge's 3 Doom is on the Necrobinder)
            self.assertFalse(decks.makes_doom(card), card["name"])

    def test_oblivion_is_a_doom_source(self):
        plain = STARTER + [REAVE, POKE, GRAVE_WARDEN]
        self.assertGreater(decks.deck_value(TIMES_UP) + decks.deck_fit(TIMES_UP, plain + [OBLIVION], 2), 1.0)

    def test_no_doom_in_the_deck_means_no_value(self):
        deck = STARTER + [REAVE, POKE, GRAVE_WARDEN]
        for card in (TIMES_UP, dict(TIMES_UP, id="TIME_S_UP")):
            self.assertAlmostEqual(decks.deck_value(card) + decks.deck_fit(card, deck, 2), 0.0, msg=card["id"])

    def test_with_a_doom_source_it_keeps_its_rating(self):
        plain = STARTER + [REAVE, POKE, GRAVE_WARDEN]
        doomed = plain + [SCOURGE]
        self.assertGreater(decks.deck_value(TIMES_UP) + decks.deck_fit(TIMES_UP, doomed, 2), 1.0)


if __name__ == "__main__":
    unittest.main()
