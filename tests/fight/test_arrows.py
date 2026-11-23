"""The Fool's Arrows: a Shot needs an Arrow that is not standing in an enemy, Aim goes with the first Arrow,
and Stretch with the first Block card."""

import unittest
from unittest import mock

from sts2bot.fight import battle, combat
from sts2bot.fight.fool import turn
from sts2bot.cards import parse

from ..fake_game import card
from ..helpers import DEFEND, STRIKE, combat_state, enemy

FOOLISH = lambda i, dmg=15: card(i, "FOOLISH_SHOT", "Foolish Shot", "Attack", 1, f"Shoot an Arrow to deal {dmg} damage.", "AnyEnemy")
QUICK = lambda i, dmg=10: card(i, "QUICK_SHOT", "Quick Shot", "Attack", 0, f"Shoot an Arrow to deal {dmg} damage.", "AnyEnemy")
NAIL = lambda i: card(i, "NAIL", "Nail", "Attack", 2, "If the enemy is Impaled, Deal 20 damage and apply 2 Weak and Vulnerable.", "AnyEnemy")
IMPALED = lambda n: {"id": "IMPALED_POWER", "amount": n}


class Reading(unittest.TestCase):
    def test_shots_aim_and_impaled_conditions(self):
        heavy = parse(card(0, "HEAVY_SHOT", "Heavy Shot", "Attack", 2, "Shoot an Arrow to deal 20 damage. Aim affects this card 2 times.", "AnyEnemy"))
        self.assertEqual((heavy.damage, heavy.hits, heavy.shoots, heavy.aim_times), (20, 1, True, 2))  # was read as 20 x 2
        self.assertTrue(parse(FOOLISH(0)).shoots)
        self.assertFalse(parse(STRIKE(0)).shoots)
        flor = parse(card(0, "FLORESCENCE", "Florescence", "Skill", 0, "Exhaust ALL of your cards that Shoot and create The Shot. Exhaust."))
        self.assertFalse(flor.shoots)
        nock = parse(card(0, "NOCK", "Nock", "Skill", 0, "Gain 5 Aim."))
        self.assertEqual((nock.aim, nock.known), (5, True))
        smack = parse(card(0, "BOW_SMACK", "Bow Smack", "Attack", 1, "Deal 8 damage. If the enemy is Impaled, gain [fool_energy_icon.png].", "AnyEnemy"))
        self.assertEqual((smack.damage, smack.energy_gain, smack.on_impaled), (8, 0, (1, 0)))
        hop = parse(card(0, "KI_HOP", "Ki Hop", "Attack", 1, "Deal 7 damage. If the enemy is Impaled, draw 2 cards.", "AnyEnemy"))
        self.assertEqual((hop.damage, hop.draw, hop.on_impaled), (7, 0, (0, 2)))
        nail = parse(NAIL(0))
        self.assertEqual((nail.damage, nail.impaled_only), (20, True))
        stretch = parse(card(0, "DEEP_STRETCH", "Deep Stretch", "Skill", 1, "Gain 7 Stretch. Draw 1 card."))
        self.assertEqual((stretch.stretch, stretch.draw), (7, 1))

    def test_the_enemy_reads_its_impale_and_hits_count_it_down(self):
        foe = battle.read_enemies({"battle": {"enemies": [enemy("A", 50, status=[IMPALED(3)])]}})[0]
        self.assertEqual(foe.impaled, 3)
        battle._hit(foe, 5, 2)
        self.assertEqual(foe.impaled, 1)
        battle._hit(foe, 5, 1, raw=True)  # (an orb's damage is no hit)
        self.assertEqual(foe.impaled, 1)
        self.assertEqual(turn.arrows_owned({"status": [{"id": "QUIVER_POWER", "amount": 1}]}), 2)
        self.assertEqual(turn.arrows_owned({}), 1)


class Planning(unittest.TestCase):
    def test_a_shot_with_our_only_arrow_in_an_enemy_is_not_played(self):
        # Seen 40-odd times in the Fool's first runs: the Shot was paid for and did nothing.
        state = combat_state([FOOLISH(0), STRIKE(1)], [enemy("A", 60, attack=8, status=[IMPALED(3)])], energy=1)
        self.assertEqual(combat.choose_card(state)[1]["card_index"], 1)
        free = combat_state([FOOLISH(0), STRIKE(1)], [enemy("A", 60, attack=8)], energy=1)
        self.assertEqual(combat.choose_card(free)[1]["card_index"], 0)

    def test_the_hit_that_frees_the_arrow_goes_first(self):
        state = combat_state([FOOLISH(0), STRIKE(1)], [enemy("A", 80, attack=8, status=[IMPALED(1)])], energy=2)
        self.assertEqual(combat.choose_card(state)[1]["card_index"], 1)

    def test_a_second_shot_needs_a_second_arrow(self):
        kw = dict(energy=3, stars=0, hp=60, max_hp=80, block=0, fight="boss", arrows=1)
        foes = combat.read_enemies({"battle": {"enemies": [enemy("A", 300)]}})
        one, two = parse(FOOLISH(0)), parse(QUICK(1))
        alone = combat.simulate([one], foes, **kw).score
        self.assertAlmostEqual(combat.simulate([one, two], foes, **kw).score, alone)
        self.assertGreater(combat.simulate([one, two], foes, **{**kw, "arrows": 2}).score, alone)

    def test_three_hits_after_the_shot_bring_the_arrow_back(self):
        kw = dict(energy=6, stars=0, hp=60, max_hp=80, block=0, fight="boss", arrows=1)
        foes = combat.read_enemies({"battle": {"enemies": [enemy("A", 500)]}})
        shot, strike, quick = parse(FOOLISH(0)), parse(STRIKE(1)), parse(QUICK(2))
        two = [shot, strike, strike]
        self.assertAlmostEqual(combat.simulate(two + [quick], foes, **kw).score, combat.simulate(two, foes, **kw).score)
        three = two + [strike]
        self.assertGreater(combat.simulate(three + [quick], foes, **kw).score, combat.simulate(three, foes, **kw).score + 5)

    def test_aim_goes_with_the_first_arrow(self):
        foes = combat.read_enemies({"battle": {"enemies": [enemy("A", 300)]}})
        spent = turn.FoolTurn(arrows=2, aim=6)
        spent.aim = 0  # a Shot earlier in the line took the Aim
        quick = parse(QUICK(0, 16))  # its text counts the 6 Aim on us
        self.assertEqual(spent.before(quick, foes, None).damage, 10)
        heavy = parse(card(0, "HEAVY_SHOT", "Heavy Shot", "Attack", 2, "Shoot an Arrow to deal 32 damage. Aim affects this card 2 times.", "AnyEnemy"))
        self.assertEqual(spent.before(heavy, foes, None).damage, 20)
        nocked = turn.FoolTurn(arrows=1)
        nocked.after(parse(card(0, "NOCK", "Nock", "Skill", 0, "Gain 5 Aim.")), None)
        self.assertEqual(nocked.before(parse(QUICK(0)), foes, None).damage, 15)  # Aim gained earlier in the line

    def test_nail_goes_at_the_impaled_enemy_or_not_at_all(self):
        state = combat_state([NAIL(0), DEFEND(1)], [enemy("A", 30, attack=6), enemy("B", 60, attack=6, status=[IMPALED(2)])], energy=2)
        action = combat.choose_card(state)
        self.assertEqual(action[1], {"card_index": 0, "target": "B"})
        bare = combat_state([NAIL(0), DEFEND(1)], [enemy("A", 30, attack=6)], energy=2)
        self.assertEqual(combat.choose_card(bare)[1]["card_index"], 1)

    def test_stretch_goes_with_the_first_block_card(self):
        # At Stretch 10 every Defend reads 15; two of them make 20, not 30.
        kw = dict(energy=2, stars=0, hp=60, max_hp=80, block=0, stretch=10)
        foes = combat.read_enemies({"battle": {"enemies": [enemy("A", 100, attack=25)]}})
        d15 = parse(card(0, "DEFEND_F", "Defend", "Skill", 1, "Gain 15 Block.", "Self"))
        self.assertEqual(combat.simulate([d15, d15], foes, **kw).lost, 5)

    def test_blossom_shot_gives_stretch_for_the_damage_it_deals(self):
        blossom = parse(card(0, "BLOSSOM_SHOT", "Blossom Shot", "Attack", 1, "Shoot an Arrow to deal 11 damage and gain Stretch equal to the damage dealt.", "AnyEnemy"))
        self.assertEqual((blossom.damage, blossom.shoots, blossom.stretch_dealt), (11, True, True))
        kw = dict(energy=2, stars=0, hp=60, max_hp=80, block=0, arrows=1)
        foes = combat.read_enemies({"battle": {"enemies": [enemy("A", 100, attack=25)]}})
        d5 = parse(card(1, "DEFEND_F", "Defend", "Skill", 1, "Gain 5 Block.", "Self"))
        self.assertEqual(combat.simulate([d5], foes, **kw).lost, 20)
        self.assertEqual(combat.simulate([blossom, d5], foes, **kw).lost, 9)  # Stretch 11: the Defend reads 16
        self.assertEqual(combat.simulate([blossom, d5], foes, **{**kw, "arrows": 0}).lost, 20)  # no Arrow: no damage, no Stretch

    def test_rain_shot_splashes_the_same_damage_on_the_other_enemies(self):
        rain = parse(card(0, "RAIN_SHOT", "Rain Shot", "Attack", 2, "Shoot an Arrow to deal 12 damage. Damage ALL other enemies equal to the damage dealt.", "AnyEnemy"))
        self.assertTrue(rain.splash)
        kw = dict(energy=3, stars=0, hp=60, max_hp=80, block=0, fight="boss", arrows=1)
        alone = combat.read_enemies({"battle": {"enemies": [enemy("A", 300)]}})
        two = combat.read_enemies({"battle": {"enemies": [enemy("A", 300), enemy("B", 300)]}})
        self.assertGreater(combat.simulate([rain], two, **kw).score, combat.simulate([rain], alone, **kw).score + 5)
        self.assertLess(combat.simulate([rain], two, **{**kw, "arrows": 0}).score, combat.simulate([rain], two, **kw).score - 10)  # no Arrow, no splash


class Melodies(unittest.TestCase):
    BLAST = card(0, "WAH_BLAST", "Wah Blast", "Attack", 2, "Deal 14 damage to ALL enemies. If you have a Cho Melody or Ki Melody, hits twice.", "AllEnemies")
    RHYTHM = card(1, "RHYTHM_STRIKE", "Rhythm Strike", "Attack", 0, "Deal 4 damage. Hits an additional time for each Melody you have. (Hits 1 time)", "AnyEnemy")
    CHO = card(2, "CHO_READY", "Cho Ready", "Skill", 1, "Gain 8 Block.  Gain 3 Aim.", "Self")

    def test_a_card_gives_the_melodies_its_name_says(self):
        self.assertEqual(parse(card(0, "WAH_CHO_KI", "Wah Cho Ki+", "Attack", 1, "Gain 12 Block. Deal 12 damage.", "AnyEnemy")).melody, ("WAH", "CHO", "KI"))
        self.assertEqual(parse(card(0, "KI_HOP", "Ki Hop", "Attack", 1, "Deal 7 damage.", "AnyEnemy")).melody, ("KI",))
        self.assertEqual(parse(card(0, "KILLING", "Killing Blow", "Attack", 1, "Deal 7 damage.", "AnyEnemy")).melody, ())
        self.assertEqual(turn.played_melodies(["Cho Ready", "Wah Sweep+", "Strike"]), frozenset({"CHO", "WAH"}))
        blast = parse(self.BLAST)
        self.assertEqual((blast.hits, blast.melody_twice), (1, ("CHO", "KI")))
        shield = parse(card(0, "HARMONIC_SHIELD", "Harmonic Shield", "Skill", 1, "Gain 5 Block for each Melody you have. (Gain 0 Block)", "Self"))
        self.assertEqual((shield.block, shield.melody_block), (0, 5))

    def test_melody_cards_go_before_the_cards_that_count_them(self):
        kw = dict(energy=3, stars=0, hp=60, max_hp=80, block=0, fight="boss")
        foes = combat.read_enemies({"battle": {"enemies": [enemy("A", 300)]}})
        blast, cho = parse(self.BLAST), parse(self.CHO)
        self.assertGreater(combat.simulate([cho, blast], foes, **kw).score, combat.simulate([blast, cho], foes, **kw).score)
        self.assertGreater(combat.simulate([blast], foes, **kw, tune=frozenset({"KI"})).score, combat.simulate([blast], foes, **kw).score)
        rhythm = parse(self.RHYTHM)
        self.assertGreater(combat.simulate([cho, rhythm], foes, **kw).score, combat.simulate([rhythm, cho], foes, **kw).score)

    def test_violent_chord_goes_off_on_the_first_wah_of_the_turn(self):
        kw = dict(energy=3, stars=0, hp=60, max_hp=80, block=0)
        foes = combat.read_enemies({"battle": {"enemies": [enemy("A", 300), enemy("B", 300)]}})
        sweep = parse(card(0, "WAH_SWEEP", "Wah Sweep", "Attack", 1, "Deal 8 damage to ALL enemies.", "AllEnemies"))
        plain = combat.simulate([sweep], foes, **kw).score
        self.assertGreater(combat.simulate([sweep], foes, **kw, chord=9).score, plain + 10)
        self.assertAlmostEqual(combat.simulate([sweep], foes, **kw, chord=9, tune=frozenset({"WAH"})).score, plain)

    def test_explosive_arrows_go_off_when_a_shot_impales(self):
        kw = dict(energy=3, stars=0, hp=60, max_hp=80, block=0, arrows=1)
        shot = parse(FOOLISH(0))
        foes = combat.read_enemies({"battle": {"enemies": [enemy("A", 300), enemy("B", 300)]}})
        self.assertGreater(combat.simulate([shot], foes, **kw, boom=8).score, combat.simulate([shot], foes, **kw).score + 12)
        stuck = combat.read_enemies({"battle": {"enemies": [enemy("A", 300, status=[IMPALED(2)]), enemy("B", 300)]}})
        self.assertAlmostEqual(combat.simulate([shot], stuck, **kw, boom=8).score, combat.simulate([shot], stuck, **kw).score)

    def test_the_bot_hands_the_planner_the_cards_played_this_turn(self):
        state = combat_state([STRIKE(0)], [enemy("A", 30, attack=5)])
        state["_played_names"] = ("Ki Hop", "Defend")
        self.assertEqual(combat._context(state)["tune"], frozenset({"KI"}))


class Dislodging(unittest.TestCase):
    def test_uproot_takes_the_arrow_back_for_the_next_shot(self):
        uproot = card(0, "UPROOT", "Uproot", "Skill", 1, "If the enemy is Impaled, Dislodge and apply 3 Vulnerable.", "AnyEnemy")
        info = parse(uproot)
        self.assertEqual((info.impaled_only, info.dislodge, info.vulnerable), (True, True, 3))
        kw = dict(energy=2, stars=0, hp=60, max_hp=80, block=0, fight="boss", arrows=1)
        foes = combat.read_enemies({"battle": {"enemies": [enemy("A", 300, status=[IMPALED(3)])]}})
        shot = parse(FOOLISH(1))
        self.assertGreater(combat.simulate([info, shot], foes, **kw).score, combat.simulate([shot, info], foes, **kw).score + 15)

    def test_wrest_draws_only_from_an_impaled_enemy(self):
        # Live 20:41-21:35: 69 of 106 Wrests drew nothing, most played before the Shot that would have let them draw.
        wrest = parse(card(0, "WREST", "Wrest", "Skill", 0, "If the enemy is Impaled, Dislodge and draw 2 cards.", "AnyEnemy"))
        self.assertEqual((wrest.impaled_only, wrest.dislodge, wrest.draw), (True, True, 2))
        kw = dict(energy=1, stars=0, hp=60, max_hp=80, block=0, fight="boss", arrows=1)
        foes = lambda: combat.read_enemies({"battle": {"enemies": [enemy("A", 300)]}})
        self.assertEqual(combat.simulate([wrest], foes(), **kw).score, combat.simulate([], foes(), **kw).score)
        shot = parse(FOOLISH(1))
        self.assertGreater(combat.simulate([shot, wrest], foes(), **kw).score, combat.simulate([wrest, shot], foes(), **kw).score)
        state = combat_state([card(0, "WREST", "Wrest", "Skill", 0, "If the enemy is Impaled, Dislodge and draw 2 cards.", "AnyEnemy"), FOOLISH(1)], [enemy("A", 300, attack=5)], energy=1)
        self.assertEqual(combat.choose_card(state)[1]["card_index"], 1)  # the Shot first

    def test_encore_leaves_one_more_card(self):
        # Live 21:04 and 21:32: Encore went first, then one Wrest, and the turn ended on 3-4 energy (a Soul Fysh and a
        # Lagavulin Matriarch lost from 69 and 82 HP).
        text = "Gain [fool_energy_icon.png]. Draw 2 cards. You can only play 1 more card this turn."
        encore = parse(card(0, "ENCORE", "Encore", "Skill", 0, text))
        self.assertEqual((encore.play_limit, encore.energy_gain, encore.draw), (1, 1, 2))
        self.assertEqual(parse(STRIKE(0)).play_limit, -1)
        kw = dict(energy=3, stars=0, hp=60, max_hp=80, block=0, fight="boss")
        foes = lambda: combat.read_enemies({"battle": {"enemies": [enemy("A", 300, attack=10)]}})
        strikes = [parse(STRIKE(i)) for i in (1, 2, 3)]
        self.assertIsNone(combat.simulate([encore, *strikes[:2]], foes(), **kw))
        self.assertIsNotNone(combat.simulate([*strikes[:2], encore], foes(), **kw))
        state = combat_state([card(0, "ENCORE", "Encore", "Skill", 0, text), STRIKE(1), STRIKE(2), STRIKE(3)], [enemy("A", 300, attack=5)], energy=3)
        self.assertNotEqual(combat.choose_card(state)[1]["card_index"], 0)  # three Strikes first, Encore last if at all
        state["player"]["status"] = [{"id": "ENCORE_POWER", "amount": 1}]
        self.assertEqual(battle.cards_left_this_turn(state), 1)
        state["player"]["status"] = [{"id": "ENCORE_POWER", "amount": 0}]
        self.assertEqual(battle.cards_left_this_turn(state), 0)

    def test_cho_limbs_gains_its_block_twice(self):
        limbs = parse(card(0, "CHO_LIMBS", "Cho Limbs", "Skill", 2, "Gain 5 Block twice. Costs 2 less [fool_energy_icon.png] if you have a Wah Melody or Ki Melody.", "Self"))
        plus = parse(card(0, "CHO_LIMBS", "Cho Limbs+", "Skill", 2, "Gain 7 Block twice. Costs 2 less [fool_energy_icon.png] if you have a Wah Melody or Ki Melody.", "Self"))
        self.assertEqual((limbs.block, plus.block), (10, 14))  # read as 5 and 7

    def test_cho_firm_blocks_twice_while_an_arrow_is_to_hand(self):
        firm = parse(card(0, "CHO_FIRM", "Cho Firm", "Skill", 1, "Gain 6 Block. If you have an Arrow, gain Block an additional time.", "Self"))
        self.assertTrue(firm.arrow_block)
        kw = dict(energy=1, stars=0, hp=60, max_hp=80, block=0, arrows=1)
        free = combat.read_enemies({"battle": {"enemies": [enemy("A", 100, attack=20)]}})
        stuck = combat.read_enemies({"battle": {"enemies": [enemy("A", 100, attack=20, status=[IMPALED(2)])]}})
        self.assertEqual(combat.simulate([firm], free, **kw).lost, 8)
        self.assertEqual(combat.simulate([firm], stuck, **kw).lost, 14)


KI_REST = lambda i: card(i, "KI_REST", "Ki Rest+", "Skill", 2, "Next turn, gain 6[fool_energy_icon.png]. If you have a Wah Melody or Cho Melody, Retain your Hand this turn.")
ONES = [{"name": "Strike", "cost": "1", "description": "Deal 6 damage."} for _ in range(10)]


class NextTurnEnergy(unittest.TestCase):
    """Energy next turn is worth what the next hand can spend (live: after Ki Rest+ the turn began on 9.4 and ended on 4.6)."""

    def test_retain_your_hand_is_read_with_the_melodies_it_needs(self):
        self.assertEqual(parse(KI_REST(0)).keeps_hand, ("WAH", "CHO"))
        self.assertEqual(parse(KI_REST(0)).next_energy, 6)
        convergence = card(0, "CONVERGENCE", "Convergence", "Skill", 1, "Next turn, gain [regent_energy_icon.png] and [star_icon.png]. Retain your Hand this turn.")
        self.assertEqual(parse(convergence).keeps_hand, ("ANY",))
        self.assertEqual(parse(STRIKE(0)).keeps_hand, ())

    def test_the_room_is_the_next_hands_cost_less_the_energy_it_will_have(self):
        state = combat_state([], [enemy("A", 100, attack=10)])
        self.assertEqual(battle.next_room(state), (99.0, 1.0))  # (no piles: every energy counts, as before)
        state["player"].update(draw_pile=ONES, max_energy=3)
        self.assertEqual(battle.next_room(state), (2.0, 1.0))  # five cards at 1, three energy of its own
        state["player"]["status"] = [{"id": "ENERGY_NEXT_TURN_POWER", "amount": 3}]
        self.assertEqual(battle.next_room(state), (-1.0, 1.0))
        state["player"]["status"] = [{"id": "SLOTH_POWER", "amount": 0}, {"id": "MIND_ROT_POWER", "amount": 1}]
        self.assertEqual(battle.next_room(state), (0.0, 1.0))  # three cards a turn
        self.assertEqual(battle.next_energy_worth(6, (2.0, 1.0)), 6.0 * 2 + 1.0 * 4)
        self.assertEqual(battle.next_energy_worth(6, (2.0, 1.0), kept=3), 6.0 * 5 + 1.0 * 1)
        self.assertEqual(battle.next_energy_worth(2, (99.0, 1.0)), 12.0)  # (the old flat 6)
        beast = {**enemy("CEREMONIAL_BEAST_0", 200), "intents": [{"type": "Debuff", "label": ""}]}
        self.assertEqual(battle.next_room(combat_state([], [beast]))[0], -99.0)  # Ringing next turn: one card

    def test_no_ki_rest_before_a_ringing_turn(self):
        # Live 21:28: Ki Rest+ before each of the Ceremonial Beast's four Ringing turns, 9 energy spent on one card each time.
        beast = {**enemy("CEREMONIAL_BEAST_0", 150), "intents": [{"type": "Debuff", "label": ""}]}
        epic = card(1, "EPIC_RETRIEVAL", "Epic Retrieval", "Attack", 2, "Deal 15 damage.", "AnyEnemy")
        state = combat_state([KI_REST(0), epic, STRIKE(2)], [beast], energy=3, kind="boss")
        state["player"].update(draw_pile=ONES, max_energy=3)
        self.assertEqual(combat.choose_card(state)[1]["card_index"], 1)
        with mock.patch.object(combat, "next_room", lambda s: (99.0, 1.0)):
            self.assertNotEqual(combat.choose_card(state)[1]["card_index"], 1)  # (what it did before: Strike, then Ki Rest+)

    def test_rewards_count_about_two_energy_next_turn(self):
        from sts2bot import cards

        gain = lambda n: {**card(0, "X", "X", "Skill", 1, "Next turn, gain " + "[fool_energy_icon.png]" * n + "."), "rarity": "Common"}
        with mock.patch.object(cards, "_crowd_share", lambda: 0.0):
            self.assertAlmostEqual(cards.deck_value(gain(2)) - cards.deck_value(gain(1)), 0.8)
            self.assertAlmostEqual(cards.deck_value(gain(6)) - cards.deck_value(gain(2)), 0.8)  # (Ki Rest+: 4.8 for its 6 before)


class DeckBuilding(unittest.TestCase):
    def test_shots_beyond_the_arrows_are_worth_less(self):
        from sts2bot.run import decks

        quick = QUICK(0)
        deck = [STRIKE(0) for _ in range(4)]
        self.assertEqual(decks.deck_fit(quick, deck, act=1), 0.0)  # the first Shot
        second = decks.deck_fit(quick, deck + [FOOLISH(0)], act=1)
        third = decks.deck_fit(quick, deck + [FOOLISH(0), FOOLISH(0)], act=1)
        self.assertLess(third, second)
        self.assertLess(second, 0.0)
        quiver = card(0, "QUIVER", "Quiver", "Power", 1, "Gain 1 additional Arrow.")
        self.assertEqual(decks.deck_fit(quick, deck + [FOOLISH(0), quiver], act=1), 0.0)  # a second Arrow for it

    def test_encore_and_impaled_only_cards_are_rated_for_what_they_do(self):
        from sts2bot import cards

        encore = {**card(0, "ENCORE", "Encore", "Skill", 0, "Gain [fool_energy_icon.png]. Draw 2 cards. You can only play 1 more card this turn."), "rarity": "Rare"}
        self.assertLessEqual(cards.deck_value(encore), 2.0)  # was 7.4, above every other Fool card offered
        self.assertEqual(cards.kind(encore), "")
        text = "If the enemy is Impaled, Dislodge and draw 2 cards."
        wrest = {**card(0, "WREST", "Wrest", "Skill", 0, text, "AnyEnemy"), "rarity": "Uncommon"}
        plain = {**card(0, "DRAWER", "Drawer", "Skill", 0, "Draw 2 cards.", "AnyEnemy"), "rarity": "Uncommon"}
        with mock.patch.object(cards, "_crowd_share", lambda: 0.0):
            self.assertAlmostEqual(cards.deck_value(plain) - cards.deck_value(wrest), 0.8)  # half of its 2 cards' 1.6

    def test_a_card_the_crowd_has_no_numbers_for_is_mixed_with_its_rarity(self):
        # The Fool skipped 29% of card rewards on the bare text rating where the others, mixed with crowd.py, skipped 0-11%.
        from sts2bot import cards

        nock = {**card(0, "STEADY_AIM", "Steady Aim", "Skill", 0, "Gain 5 Aim."), "rarity": "Common"}  # (no tier either: Nock has one since crowd.FOOL_RATING)
        with mock.patch.object(cards, "_crowd_share", lambda: 0.0):
            bare = cards.deck_value(nock)
        with mock.patch.object(cards, "_crowd_share", lambda: 0.6):
            self.assertAlmostEqual(cards.deck_value(nock), 0.4 * bare + 0.6 * cards._RARITY_CROWD["Common"])
            strike = {**STRIKE(0), "rarity": "Basic"}
            self.assertEqual(cards.deck_value(strike), -2.5)  # (no mean for a Basic card: left alone)


if __name__ == "__main__":
    unittest.main()
