import json
import tempfile
import unittest
from pathlib import Path

from sts2bot import combat, learn, strategy
from sts2bot.bot import Bot, Stuck
from sts2bot.cards import deck_value, parse
from sts2bot.strategy import Memory

from .fake_game import FakeGame, card
from .helpers import DEFEND, STRIKE, FakeClock, combat_state, enemy, make_bot  # noqa: F401


class BossMechanics(unittest.TestCase):
    """Things the bosses do that the planner got wrong (found in the fight logs of 2026-11-18)."""

    ESCAPE = staticmethod(lambda i, cost=1: card(
        i, "FRANTIC_ESCAPE", "Frantic Escape", "Status", cost,
        "Get farther away. Increase Sandpit by 1. Increase the cost of this card by 1."))

    def test_a_death_blow_is_an_attack(self):
        blow = {"type": "DeathBlow", "label": "39"}
        self.assertEqual((combat.intent_damage(blow), combat.intent_hits(blow)), (39, 1))

    def test_the_giants_last_blow_is_blocked_and_the_dead_giant_left_alone(self):
        # Killed, the Waterfall Giant stands on 999,999,999 HP until it blows up.
        giant = enemy("WATERFALL_GIANT_0", 999999999, status=[{"id": "STEAM_ERUPTION_POWER", "amount": 39}])
        giant["intents"] = [{"type": "DeathBlow", "label": "39"}]
        big = card(0, "KICK", "Kingly Kick", "Attack", 0, "Deal 35 damage.", "AnyEnemy")
        reflect = card(1, "REFLECT", "Reflect", "Skill", 1, "Gain 15 Block.", "Self")
        state = combat_state([big, reflect, DEFEND(2), DEFEND(3)], [giant], energy=3, hp=36, max_hp=75, kind="boss")
        played = []
        for _ in range(3):
            action = combat.choose_card(state)
            self.assertIsNotNone(action)
            index = action[1]["card_index"]
            played.append(index)
            state["player"]["hand"] = [c for c in state["player"]["hand"] if c["index"] != index]
            state["player"]["energy"] -= 1
            state["player"]["block"] += 15 if index == 1 else 5
        self.assertEqual(sorted(played), [1, 2, 3])  # every Block card, and not one hit on the giant
        self.assertIsNone(combat.choose_card(state))  # the free attack is not worth playing either

    def test_beckon_is_seen_despite_the_double_space(self):
        # The mod turns the line break in Beckon's text into a second space.
        beckon = card(0, "BECKON", "Beckon", "Status", 1, "At the end of your turn, if this is in your Hand,  lose 6 HP.", "Self")
        state = combat_state([beckon, DEFEND(1)], [enemy("FYSH", 200, attack=4)], energy=1, kind="boss")
        self.assertEqual(combat.hand_burns(state["player"]), ((0, 6, False),))
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0}))  # 6 saved beats 4 blocked

    def test_block_stops_cards_that_deal_damage_but_not_ones_that_take_hp(self):
        wither = card(0, "WITHER", "Wither+2", "Status", 0, "Unplayable. At the end of your turn, if this is in your Hand, take 9 damage.")
        beckon = card(0, "BECKON", "Beckon", "Status", 1, "At the end of your turn, if this is in your Hand,  lose 6 HP.")
        kw = dict(energy=0, stars=0, hp=50, max_hp=80, fight="boss")
        foe = combat.read_enemies({"battle": {"enemies": [enemy("A", 300, attack=10)]}})
        behind = lambda c, block: combat.simulate([], foe, block=block, burns=combat.hand_burns({"hand": [c]}), **kw).score
        self.assertEqual(behind(wither, 19), behind(wither, 25))  # 19 Block takes the Wither and the attack
        self.assertLess(behind(wither, 10), behind(wither, 19))
        self.assertEqual(behind(beckon, 10), behind(beckon, 25))  # the 6 HP go whatever the Block

    def test_a_wither_costs_three_for_every_level(self):
        named = lambda name: {"hand": [{"name": name, "description": "At the end of your turn, if this is in your Hand, take 3 damage."}]}
        self.assertEqual(combat.hand_burns(named("Wither"))[0][1], 3)
        self.assertEqual(combat.hand_burns(named("Wither+2"))[0][1], 9)
        self.assertEqual(combat._next_wither({"discard_pile": [{"name": "Wither+1"}]}), 6)
        self.assertEqual(combat._next_wither({}, round_no=8), 9)  # two of Aeonglass's upgrades have gone by

    def test_dead_on_paper_it_plays_the_block_it_has(self):
        # 29 coming, 22 HP, one Defend+: nothing saves it for sure, but the Block comes closest.
        power = card(0, "DEFRAGMENT", "Defragment", "Power", 1, "Gain 1 Focus.", "Self")
        guard = card(1, "DEFEND", "Defend+", "Skill", 1, "Gain 8 Block.", "Self")
        skim = card(2, "SKIM", "Skim", "Skill", 1, "Draw 3 cards.", "Self")
        state = combat_state([power, guard], [enemy("A", 300, attack=40)], energy=1, hp=22, max_hp=90, kind="boss")
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 1}))
        # With energy for both, the cards are drawn first: one of them may be the Block that is missing.
        state = combat_state([power, guard, skim], [enemy("A", 300, attack=40)], energy=2, hp=22, max_hp=90, kind="boss")
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 2}))

    def test_an_escape_is_played_before_the_margin_is_gone(self):
        # Sandpit 3 and one Escape in hand: ending on 2 needs an Escape in the very next hand.
        boss = enemy("INS", 200, attack=0, status=[{"id": "SANDPIT_POWER", "amount": 3}])
        state = combat_state([self.ESCAPE(0), STRIKE(1), STRIKE(2), STRIKE(3)], [boss], energy=3, kind="boss")
        state["player"]["draw_pile"] = [{"name": "Strike"}] * 20 + [{"name": "Frantic Escape"}] * 2
        seen = set()
        for _ in range(3):
            action = combat.choose_card(state)
            seen.add(action[1]["card_index"])
            state["player"]["hand"] = [c for c in state["player"]["hand"] if c["index"] != action[1]["card_index"]]
            state["player"]["energy"] -= 1
        self.assertIn(0, seen)
        # ... even at 2 energy, the whole of what is left.
        state = combat_state([self.ESCAPE(0, cost=2), STRIKE(1), STRIKE(2)], [boss], energy=2, kind="boss")
        state["player"]["draw_pile"] = [{"name": "Strike"}] * 20 + [{"name": "Frantic Escape"}] * 2
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0}))

    def test_escape_odds_follow_the_piles(self):
        none_left = {"draw_pile": [{"name": "Strike"}] * 10, "discard_pile": [{"name": "Frantic Escape"}] * 6, "hand": []}
        self.assertEqual(combat._escape_odds(none_left)[0], 1.0)  # the next five cards hold none for certain
        all_escapes = {"draw_pile": [{"name": "Frantic Escape"}] * 5, "discard_pile": [], "hand": []}
        self.assertEqual(combat._escape_odds(all_escapes)[0], 0.0)
        mixed = {"draw_pile": [{"name": "Strike"}] * 24 + [{"name": "Frantic Escape"}] * 6, "discard_pile": [], "hand": []}
        self.assertAlmostEqual(combat._escape_odds(mixed)[0], 0.298, places=2)

    def test_beating_a_form_of_the_test_subject_is_not_winning_the_fight(self):
        adaptable = [{"id": "ADAPTABLE_POWER", "amount": 1}]
        kw = dict(energy=2, stars=0, hp=60, max_hp=80, block=0, fight="boss")
        foes = lambda status: combat.read_enemies({"battle": {"enemies": [enemy("TEST_SUBJECT_0", 5, attack=15, status=status)]}})
        kill = [parse(STRIKE(0))]
        self.assertGreater(combat.simulate(kill, foes([]), **kw).score, combat.WIN_BONUS)
        beaten = combat.simulate(kill, foes(adaptable), **kw).score
        self.assertLess(beaten, combat.WIN_BONUS / 2)
        # ... but it is worth doing: its attack this turn does not happen.
        self.assertGreater(beaten, combat.simulate([], foes(adaptable), **kw).score + 40)
        # Once it is gone nothing more can be aimed at it this turn.
        self.assertAlmostEqual(combat.simulate(kill + [parse(STRIKE(1))], foes(adaptable), **kw).score, beaten)

    def test_set_up_cards_are_played_while_the_enemy_is_away(self):
        loop = card(0, "LOOP", "Loop", "Power", 1, "At the start of your turn, trigger the passive of your rightmost Orb.", "Self")
        zap = card(1, "ZAP", "Zap", "Skill", 1, "Channel 1 Lightning.", "Self")
        state = combat_state([loop, zap, STRIKE(2), DEFEND(3)], [], energy=2, kind="boss")
        self.assertIsNone(combat.choose_card(state))  # a fight that is simply over
        state["_revive_pending"] = True
        first = combat.choose_card(state)
        self.assertEqual(first, ("play_card", {"card_index": 0}))
        state["player"]["hand"] = [zap, STRIKE(2), DEFEND(3)]
        state["player"]["energy"] = 1
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 1}))
        state["player"]["hand"] = [STRIKE(2), DEFEND(3)]
        self.assertIsNone(combat.choose_card(state))  # no use hitting or blocking thin air

    def test_an_x_cost_summon_is_set_up_too(self):
        dirge = card(0, "DIRGE", "Dirge", "Skill", "X", "Summon 3 X times. Add X Souls into your Draw Pile. Exhaust.", "Self")
        state = combat_state([dirge, DEFEND(1)], [], energy=2, kind="boss")
        state["_revive_pending"] = True
        self.assertTrue(parse(dirge).is_x)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0}))
        state["player"]["energy"] = 0
        self.assertIsNone(combat.choose_card(state))

    def test_vital_spark_taints_by_its_own_amount(self):
        prism = enemy("PRISM", 60, attack="5x3", status=[{"id": "VITAL_SPARK_POWER", "amount": 4}])
        prism["intents"] = [{"type": "Attack", "label": "5x3"}]
        state = combat_state([DEFEND(0)], [prism], energy=1, hp=18)
        self.assertEqual(combat._context(state)["taint"], 4)
        # A 5-Block Skill that adds 4 to each of three hits lets more through, not less.
        self.assertIsNone(combat.choose_card(state))

    def test_an_escape_is_never_the_card_given_up(self):
        escape = self.ESCAPE(0)
        hand = [escape, card(1, "X", "Hyperbeam", "Attack", 2, "Deal 26 damage to ALL enemies.", rarity="Rare"), DEFEND(2)]
        state = {"state_type": "hand_select", "hand_select": {"prompt": "Choose a card to Exhaust.", "cards": hand, "can_confirm": False}}
        first = strategy.hand_select(state, Memory())[0]
        self.assertEqual(first, ("combat_select_card", {"card_index": 2}))
        order = [a[1]["card_index"] for a in strategy.hand_select(state, Memory()) if a[0] == "combat_select_card"]
        self.assertEqual(order[-1], 0)  # the Escape goes last, after even the best card


if __name__ == "__main__":
    unittest.main()


class CardPlayTriggers(unittest.TestCase):
    """Powers that act on every card played are part of the turn's arithmetic."""

    def subject(self, hp, hand, powers, energy=3):
        foe = {"entity_id": "TEST_SUBJECT_0", "hp": hp, "max_hp": 200, "block": 0,
               "status": [{"id": "ADAPTABLE_POWER", "amount": 1}, {"id": "DOOM_POWER", "amount": 28}],
               "intents": [{"type": "Attack", "label": "10x5"}]}
        s = combat_state(hand, [foe], energy=energy, hp=10, max_hp=60, kind="boss")
        s["player"]["status"] = powers
        return s

    def test_sleight_of_flesh_finishes_what_the_attack_started(self):
        exterminate = card(0, "EXTERMINATE", "Exterminate+", "Attack", 1, "Deal 7 damage to ALL enemies 4 times.", "AllEnemies")
        bringer = lambda i: card(i, "DEATHBRINGER", "Deathbringer+", "Skill", 2, "Apply 26 Doom and 1 Weak to ALL enemies.", "AllEnemies")
        defend = lambda i: card(i, "DEFEND_N", "Defend+", "Skill", 1, "Gain 8 Block.", "Self")
        sleight = [{"id": "SLEIGHT_OF_FLESH_POWER", "amount": 13}]
        # 33 HP: the attack takes 28, the two debuffs 13 each. The form dies and its 50 damage never comes.
        s = self.subject(33, [exterminate, bringer(1), defend(2)], sleight)
        self.assertEqual(combat.choose_card(s), ("play_card", {"card_index": 0}))
        s = self.subject(5, [bringer(0), defend(1)], sleight, energy=2)
        self.assertEqual(combat.choose_card(s), ("play_card", {"card_index": 0}))
        # Without the Power there is no kill in the hand: Block is all that is left.
        s = self.subject(33, [exterminate, bringer(1), defend(2)], [])
        self.assertEqual(combat.choose_card(s), ("play_card", {"card_index": 2}))

    def test_afterimage_and_strangle_count_for_every_card(self):
        from sts2bot import triggers
        from sts2bot.battle import read_enemies
        foe = enemy("RAT_0", 5, attack=9, status=[{"id": "STRANGLE_POWER", "amount": 3}])
        s = combat_state([DEFEND(0), DEFEND(1)], [foe], energy=2, hp=30)
        s["player"]["status"] = [{"id": "AFTERIMAGE_POWER", "amount": 1}]
        t = triggers.read(s)
        self.assertEqual((t.card_block, t.strangle), (1, {"RAT_0": 3}))
        foes = read_enemies(s)
        out = combat.simulate([parse(c) for c in s["player"]["hand"]], foes, **combat._context(s))
        self.assertEqual(out.lost, 0)  # two Defends strangle it: 6 HP lost on 5
        self.assertIsNone(triggers.read(combat_state([DEFEND(0)], [enemy("A", 5)])))


class EarlySetup(unittest.TestCase):
    def first_card(self, setting, hp, attack):
        from unittest import mock

        sleight = card(0, "SLEIGHT_OF_FLESH", "Sleight of Flesh", "Power", 2, "Whenever you apply a debuff to an enemy, they take 9 damage.", "Self")
        state = combat_state([sleight, DEFEND(1), DEFEND(2), STRIKE(3)], [enemy("B", 300, attack=attack)], hp=hp, kind="boss")
        with mock.patch.dict(learn.KNOB, {"caution": 1.35, "early_setup": setting}):
            return combat.choose_card(state)[1]["card_index"]

    def test_the_setting_puts_a_power_first_in_the_opening_rounds_of_a_boss(self):
        self.assertNotEqual(self.first_card(1.0, hp=80, attack=10), 0)
        self.assertEqual(self.first_card(1.7, hp=80, attack=10), 0)
        self.assertNotEqual(self.first_card(1.7, hp=40, attack=18), 0)  # not at half HP in front of a big hit


class Disintegration(unittest.TestCase):
    def test_block_stops_the_burn_like_any_other_damage(self):
        state = combat_state([DEFEND(0), DEFEND(1)], [enemy("KNOWLEDGE_DEMON_0", 80, attack=6)], energy=2, hp=40, kind="boss")
        state["player"]["status"] = [{"id": "DISINTEGRATION_POWER", "amount": 8}]
        self.assertEqual(combat.expected_end_loss(state), 14)  # 8 and then 6, nothing in the way
        state["player"]["block"] = 10
        self.assertEqual(combat.expected_end_loss(state), 4)  # seen live: 8 a turn, 0 HP lost behind 25 Block


class PainfulStabs(unittest.TestCase):
    def test_each_hit_stopped_is_a_wound_saved(self):
        from sts2bot.battle import read_enemies

        guard = lambda i: card(i, "DEFEND_B", "Defend+", "Skill", 1, "Gain 10 Block.", "Self")

        def score(line, stabs):
            foe = {"entity_id": "T", "hp": 190, "max_hp": 200, "block": 0, "intents": [{"type": "Attack", "label": "10x3"}],
                   "status": [{"id": "PAINFUL_STABS_POWER", "amount": 1}] if stabs else []}
            state = combat_state([guard(0), guard(1)], [foe], energy=2, hp=80, kind="boss")
            cards = [parse(c) for c in state["player"]["hand"]]
            return combat.simulate([cards[i] for i in line], read_enemies(state), **combat._context(state)).score

        wound = score([], False) - score([], True)  # three hits through: three Wounds
        self.assertGreater(wound, 0)
        self.assertAlmostEqual(score([0, 1], False) - score([0, 1], True), wound / 3)  # two hits stopped, one Wound


class BufferCard(unittest.TestCase):
    def test_buffer_played_now_stops_the_hit_that_would_kill(self):
        buffer = card(0, "BUFFER", "Buffer", "Power", 2, "Prevent the next time you would lose HP.", "Self")
        guard = card(1, "GLACIER", "Glacier", "Skill", 2, "Gain 11 Block.", "Self")
        state = combat_state([buffer, guard], [enemy("AMALGAM_0", 56, attack=27)], energy=2, hp=6, max_hp=73, kind="boss")
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0}))


class Necrobinder(unittest.TestCase):
    POKE = staticmethod(lambda i: card(i, "POKE", "Poke", "Attack", 0, "Osty deals 6 damage.", "AnyEnemy"))
    SHARDS = staticmethod(lambda i: card(
        i, "BONE_SHARDS", "Bone Shards", "Attack", 1, "If Osty is alive, he deals 9 damage to ALL enemies and you gain 9 Block. Osty dies.", "AllEnemies"))

    def state(self, hand, osty, foe_hp=40, attack=12, doom=0, energy=3):
        foe = enemy("SLUG_0", foe_hp, attack=attack, status=[{"id": "DOOM_POWER", "amount": doom}] if doom else [])
        s = combat_state(hand, [foe], energy=energy, hp=40)
        s["player"]["pets"] = [{"name": "Osty", "hp": osty, "alive": osty > 0}]
        return s

    def test_doom_on_ourselves_counts_as_hp_already_gone(self):
        surge = card(0, "NEUROSURGE", "Neurosurge", "Power", 0, "Gain 3 energy. Draw 2 cards. At the start of your turn, apply 3 Doom to yourself.", "Self")
        boss = combat_state([surge, STRIKE(1)], [enemy("A", 300, attack=10)], hp=60, kind="boss")
        self.assertEqual(combat.choose_card(boss)[1]["card_index"], 1)  # a long fight: never
        small = combat_state([surge], [enemy("A", 30, attack=10)], hp=60)
        self.assertEqual(combat.choose_card(small), ("play_card", {"card_index": 0}))
        # 75 HP with 66 Doom and 3 more on the way is 6 HP as far as the plan goes.
        hurt = combat_state([STRIKE(0)], [enemy("A", 300, attack=10)], hp=75, kind="boss")
        self.assertEqual(combat._context(hurt)["hp"], 75)
        hurt["player"]["status"] = [{"id": "DOOM_POWER", "amount": 66}, {"id": "NEUROSURGE_POWER", "amount": 3}]
        self.assertEqual(combat._context(hurt)["hp"], 6)

    def test_ostys_attacks_do_nothing_while_he_is_dead(self):
        self.assertEqual(combat.choose_card(self.state([self.POKE(0)], osty=3)), ("play_card", {"card_index": 0, "target": "SLUG_0"}))
        self.assertIsNone(combat.choose_card(self.state([self.POKE(0)], osty=0)))
        # Bone Shards kills him: a Poke after it is wasted, so the Poke goes first.
        both = self.state([self.SHARDS(0), self.POKE(1)], osty=1)
        self.assertEqual(combat.choose_card(both)[1]["card_index"], 1)
        # Without the pet in the state nothing is assumed about him.
        bare = self.state([self.POKE(0)], osty=0)
        del bare["player"]["pets"]
        self.assertIsNotNone(combat.choose_card(bare))

    def test_bone_shards_gives_up_the_hp_osty_stood_in_front_with(self):
        s = self.state([self.SHARDS(0)], osty=10, attack=12)
        cards = [parse(c) for c in s["player"]["hand"]]
        from sts2bot.battle import read_enemies
        kept = combat.simulate([], read_enemies(s), **combat._context(s)).lost  # Osty soaks 10 of 12
        after = combat.simulate(cards, read_enemies(s), **combat._context(s)).lost  # 9 Block instead of his 10 HP
        self.assertEqual((kept, after), (2, 3))

    def test_end_of_days_kills_the_doomed_at_once(self):
        days = card(0, "END_OF_DAYS", "End of Days", "Skill", 3, "Apply 29 Doom to ALL enemies. Kill enemies with at least as much Doom as HP.", "AllEnemies")
        guard = card(1, "DEFEND_N", "Defend", "Skill", 1, "Gain 5 Block.", "Self")
        # 40 HP, 15 Doom already: 44 covers it. The enemy dies before its 30 damage, which three Defends would not stop.
        s = self.state([days, guard, guard | {"index": 2}, guard | {"index": 3}], osty=0, foe_hp=40, attack=30, doom=15)
        self.assertEqual(combat.choose_card(s), ("play_card", {"card_index": 0}))
        self.assertEqual(combat.expected_end_loss(self.state([days], osty=0, foe_hp=40, attack=30, doom=15)), 30)


class StrengthSappedFromAll(unittest.TestCase):
    def test_piercing_wail_is_played_in_front_of_a_many_hit_attack(self):
        wail = card(0, "PIERCING_WAIL", "Piercing Wail", "Skill", 1, "ALL enemies lose 6 Strength this turn. Exhaust.", "AllEnemies")
        foe = {"entity_id": "TEST_SUBJECT_0", "hp": 97, "max_hp": 200, "block": 0, "intents": [{"type": "Attack", "label": "10x6"}],
               "status": [{"id": "PAINFUL_STABS_POWER", "amount": 1}]}
        state = combat_state([wail], [foe], energy=1, hp=33, max_hp=128, block=26, kind="boss")
        self.assertEqual(combat.expected_end_loss(state), 34)  # dead as it stands
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0}))  # six hits of 4: all behind the Block

    def test_an_attack_on_all_that_saps_them_counts_the_sap(self):
        from sts2bot.battle import read_enemies

        crush = card(0, "CRUSH_UNDER", "Crush Under", "Attack", 1, "Deal 8 damage to ALL enemies. All enemies lose 1 Strength this turn.", "AllEnemies")
        foes = [enemy("A", 50, attack="3x4"), enemy("B", 50, attack=10)]
        foes[0]["intents"] = [{"type": "Attack", "label": "3x4"}]
        state = combat_state([crush], foes, energy=1, hp=40)
        out = combat.simulate([parse(crush)], read_enemies(state), **combat._context(state))
        self.assertEqual(out.lost, 2 * 4 + 9)


class Malaise(unittest.TestCase):
    def test_x_amounts_are_read_per_energy(self):
        malaise = card(0, "MALAISE", "Malaise", "Skill", "X", "Enemy loses X Strength. Apply X Weak. Exhaust.", "AnyEnemy")
        info = parse(malaise)
        self.assertEqual((info.is_x, info.enemy_strength_loss, info.weak, info.known), (True, 1, 1, True))
        # Three energy into a 5x6 attack: 2x6, and a quarter off that for Weak.
        foe = enemy("A", 80, attack="5x6")
        foe["intents"] = [{"type": "Attack", "label": "5x6"}]
        state = combat_state([malaise], [foe], energy=3, hp=30)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0, "target": "A"}))
