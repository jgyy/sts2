"""The Fool's powers that pay out on a new Melody (Song Path, Motif, Lullaby), Ki Verse, and Yah!'s energy when an Arrow
comes out of an enemy. Read live on 23 Nov 22:20 (Song Path+ and Motif+ up): the first Cho card of a turn gave 7 Block
more than its own and the Cho cards after it nothing more; Motif drew on the turn's first Wah, Ki and Cho card only;
Yah!+ gave 2 energy on the Arrow's third hit (Ki Whip) and on the death of the enemy holding it (Bow Smack)."""

import unittest

from sts2bot.cards import parse
from sts2bot.fight import combat
from sts2bot.fight.fool import turn

from ..fake_game import card
from ..helpers import DEFEND, STRIKE, combat_state, enemy, make_bot

IMPALED = lambda n: {"id": "IMPALED_POWER", "amount": n}
POWER = lambda name, n: {"id": f"{name}_POWER", "amount": n}
CHO = lambda i, block=6: card(i, "CHO_GUARD", "Cho Guard", "Skill", 1, f"Gain {block} Block.", "Self")
WAH = lambda i, dmg=6: card(i, "WAH_JAB", "Wah Jab", "Attack", 1, f"Deal {dmg} damage.", "AnyEnemy")
KI = lambda i, dmg=7: card(i, "KI_JAB", "Ki Jab", "Attack", 1, f"Deal {dmg} damage.", "AnyEnemy")
VERSE_TEXT = "The next “Wah”, “Cho”, or “Ki” card you play this turn is played an additional time."
VERSE_PLUS = "The next 2 “Wah”, “Cho”, or “Ki” cards you play this turn are played an additional time."
VERSE = lambda i, plus=False: card(i, "KI_VERSE", "Ki Verse+" if plus else "Ki Verse", "Skill", 1, VERSE_PLUS if plus else VERSE_TEXT)
KW = dict(energy=3, stars=0, hp=60, max_hp=80, block=0, fight="boss")


def foes(*specs):
    return combat.read_enemies({"battle": {"enemies": list(specs)}})


class Reading(unittest.TestCase):
    def test_ki_verse_says_how_many_cards_it_plays_again(self):
        self.assertEqual(parse(VERSE(0)).verse, 1)
        self.assertEqual(parse(VERSE(0, plus=True)).verse, 2)
        self.assertEqual(parse(STRIKE(0)).verse, 0)
        self.assertEqual(parse(VERSE(0)).melody, ("KI",))  # (a Ki card itself)

    def test_the_powers_are_read_off_us_and_yah_goes_once_an_arrow_is_out(self):
        player = {"status": [POWER("SONG_PATH", 7), POWER("MOTIF", 1), POWER("LULLABY", 20), POWER("KI_VERSE", 2), POWER("YAH", 2)]}
        self.assertEqual(turn.fool_powers(player), (7, 1, 20, 2, 2))
        self.assertEqual(turn.fool_powers(player, dislodged=True), (7, 1, 20, 2, 0))
        self.assertEqual(turn.fool_powers({"status": []}), ())
        state = combat_state([STRIKE(0)], [enemy("A", 30, attack=5)])
        state["player"]["status"] = [POWER("YAH", 1)]
        self.assertEqual(combat._context(state)["fool"], (0, 0, 0, 0, 1))
        self.assertEqual(combat._context({**state, "_dislodged": True})["fool"], ())


class NewMelodies(unittest.TestCase):
    def test_song_path_blocks_on_the_first_cho_card_of_the_turn(self):
        cho = parse(CHO(0))
        hit = lambda: foes(enemy("A", 300, attack=20))
        self.assertEqual(combat.simulate([cho], hit(), **KW).lost, 14)
        self.assertEqual(combat.simulate([cho], hit(), **KW, fool=(5, 0, 0, 0, 0)).lost, 9)
        self.assertEqual(combat.simulate([cho, parse(CHO(1))], hit(), **KW, fool=(5, 0, 0, 0, 0)).lost, 3)  # (once a turn)
        self.assertEqual(combat.simulate([cho], hit(), **KW, fool=(5, 0, 0, 0, 0), tune=frozenset({"CHO"})).lost, 14)

    def test_song_path_puts_the_cho_card_before_a_bigger_defend(self):
        hand = [card(0, "DEFEND_R", "Defend", "Skill", 1, "Gain 7 Block.", "Self"), CHO(1)]
        state = combat_state(hand, [enemy("A", 300, attack=11)], energy=1)
        self.assertEqual(combat.choose_card(state)[1]["card_index"], 0)
        state["player"]["status"] = [POWER("SONG_PATH", 5)]
        self.assertEqual(combat.choose_card(state)[1]["card_index"], 1)

    def test_motif_draws_on_each_new_melody(self):
        wah, cho, wah2 = parse(WAH(0)), parse(CHO(1)), parse(WAH(2))
        line = [wah, cho, wah2]
        plain = combat.simulate(line, foes(enemy("A", 300)), **KW).score
        drew = combat.simulate(line, foes(enemy("A", 300)), **KW, fool=(0, 1, 0, 0, 0)).score
        self.assertAlmostEqual(drew - plain, 2 * 2.0)  # Wah and Cho: two cards (the second Wah draws none)
        held = combat.simulate(line, foes(enemy("A", 300)), **KW, fool=(0, 1, 0, 0, 0), tune=frozenset({"WAH", "CHO"})).score
        self.assertAlmostEqual(held, combat.simulate(line, foes(enemy("A", 300)), **KW, tune=frozenset({"WAH", "CHO"})).score)

    def test_lullaby_blocks_once_all_three_melodies_are_in(self):
        hit = lambda: foes(enemy("A", 300, attack=30))
        two = [parse(WAH(0)), parse(CHO(1))]
        self.assertEqual(combat.simulate(two, hit(), **KW, fool=(0, 0, 20, 0, 0)).lost, 24)
        self.assertEqual(combat.simulate([*two, parse(KI(2))], hit(), **KW, fool=(0, 0, 20, 0, 0)).lost, 4)
        self.assertEqual(combat.simulate([parse(KI(2))], hit(), **KW, fool=(0, 0, 20, 0, 0), tune=frozenset({"WAH", "CHO"})).lost, 10)


class KiVerse(unittest.TestCase):
    def test_a_waiting_ki_verse_plays_the_next_melody_card_twice(self):
        # Ki Jab (7) into an enemy on 14 that is about to hit for 10: a kill only when it is played twice.
        hit = lambda: foes(enemy("A", 14, attack=10))
        ki = parse(KI(0))
        self.assertEqual(combat.simulate([ki], hit(), **KW).lost, 10)
        self.assertEqual(combat.simulate([ki], hit(), **KW, fool=(0, 0, 0, 1, 0)).lost, 0)
        self.assertEqual(combat.simulate([parse(STRIKE(1))], foes(enemy("A", 12, attack=10)), **KW, fool=(0, 0, 0, 1, 0)).lost, 10)  # (not a Strike)

    def test_ki_verse_goes_before_the_cards_it_plays_twice_and_not_last(self):
        verse, ki = parse(VERSE(0)), parse(KI(1))
        self.assertGreater(combat.simulate([verse, ki], foes(enemy("A", 300)), **KW).score, combat.simulate([ki, verse], foes(enemy("A", 300)), **KW).score + 5)
        state = combat_state([VERSE(0), KI(1), STRIKE(2)], [enemy("A", 300, attack=5)], energy=3)
        self.assertEqual(combat.choose_card(state)[1]["card_index"], 0)
        alone = combat_state([VERSE(0), STRIKE(1)], [enemy("A", 300, attack=5)], energy=2)
        plan: dict = {}
        combat.choose_card(alone, plan)
        self.assertNotIn("KI_VERSE", [c.key for c in plan["line"]])  # nothing to play twice: it was played last, for nothing


class Yah(unittest.TestCase):
    def test_the_arrows_third_hit_pays_for_the_next_card(self):
        strike, defend = parse(STRIKE(0)), parse(DEFEND(1))
        stuck = lambda: foes(enemy("A", 300, attack=10, status=[IMPALED(1)]))
        one = dict(KW, energy=1)
        self.assertIsNone(combat.simulate([strike, defend], stuck(), **one))
        self.assertIsNotNone(combat.simulate([strike, defend], stuck(), **one, fool=(0, 0, 0, 0, 1)))
        self.assertIsNone(combat.simulate([strike, defend], foes(enemy("A", 300, attack=10, status=[IMPALED(2)])), **one, fool=(0, 0, 0, 0, 1)))
        self.assertIsNone(combat.simulate([defend, strike], stuck(), **one, fool=(0, 0, 0, 0, 1)))  # (the hit must come first)

    def test_the_death_of_the_enemy_holding_the_arrow_counts_and_only_once_a_turn(self):
        strike = lambda i: parse(STRIKE(i))
        two = lambda: foes(enemy("A", 6, attack=10, status=[IMPALED(3)]), enemy("B", 300, attack=10, status=[IMPALED(1)]))
        line = [strike(0), strike(1), strike(2)]
        self.assertIsNotNone(combat.simulate(line[:2], two(), **dict(KW, energy=1), fool=(0, 0, 0, 0, 1)))
        self.assertIsNone(combat.simulate(line, two(), **dict(KW, energy=1), fool=(0, 0, 0, 0, 1)))  # (the second Arrow out pays nothing)

    def test_the_bot_notes_an_arrow_coming_out_this_turn(self):
        bot = make_bot(None)
        first = combat_state([STRIKE(0), STRIKE(1)], [enemy("A", 300, attack=5, status=[IMPALED(1)])], energy=2)
        bot._combat(first)
        self.assertFalse(bot._dislodged)
        out = combat_state([STRIKE(1)], [enemy("A", 294, attack=5)], energy=1)
        bot._combat(out)
        self.assertTrue(bot._dislodged)
        bot._combat(out)
        self.assertTrue(bot._dislodged)  # (for the rest of the turn)
        later = combat_state([STRIKE(0)], [enemy("A", 294, attack=5, status=[IMPALED(3)])], energy=3)
        later["battle"]["round"] = 2
        bot._combat(later)
        self.assertFalse(bot._dislodged)


if __name__ == "__main__":
    unittest.main()
