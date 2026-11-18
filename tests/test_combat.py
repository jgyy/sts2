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


class CardParsing(unittest.TestCase):
    def test_multi_hit_and_debuffs(self):
        c = parse(card(0, "X", "X", "Attack", 2, "Deal 5 damage 3 times. Apply 2 Vulnerable.", "AnyEnemy"))
        self.assertEqual((c.damage, c.hits, c.vulnerable, c.cost, c.needs_target), (5, 3, 2, 2, True))

    def test_twice_only_counts_hits_when_it_is_about_the_damage(self):
        deb = parse(card(0, "DEBILITATE", "Debilitate", "Attack", 1,
                         "Deal 10 damage. Vulnerable and Weak are twice as effective against the enemy for the next 2 turns.", "AnyEnemy"))
        self.assertEqual((deb.damage, deb.hits), (10, 1))
        twin = parse(card(0, "TWIN_STRIKE", "Twin Strike", "Attack", 1, "Deal 5 damage twice.", "AnyEnemy"))
        self.assertEqual((twin.damage, twin.hits), (5, 2))
        fists = parse(card(0, "FISTICUFFS", "Fisticuffs", "Attack", 1, "Deal 7 damage. Gain Block equal to damage dealt.", "AnyEnemy"))
        self.assertEqual((fists.damage, fists.block), (7, 7))

    def test_x_cost_and_block(self):
        c = parse({"index": 1, "type": "Skill", "cost": "X", "description": "Gain 7 Block. Draw 2 cards."})
        self.assertTrue(c.is_x)
        self.assertEqual((c.block, c.draw), (7, 2))

    def test_intent_damage(self):
        self.assertEqual(combat.intent_damage({"type": "Attack", "label": "5x3"}), 15)
        self.assertEqual(combat.intent_damage({"type": "AttackDebuff", "label": "11"}), 11)
        self.assertEqual(combat.intent_damage({"type": "Buff", "label": "3"}), 0)

    def test_deck_value_ordering(self):
        strike = card(0, "STRIKE_R", "Strike", "Attack", 1, "Deal 6 damage.", rarity="Basic")
        good = card(0, "UPPERCUT", "Uppercut", "Attack", 2, "Deal 13 damage. Apply 1 Weak. Apply 1 Vulnerable.", rarity="Uncommon")
        curse = card(0, "REGRET", "Regret", "Curse", 0, "Unplayable.")
        self.assertGreater(deck_value(good), deck_value(strike))
        self.assertGreater(deck_value(strike), deck_value(curse))


class CombatPlanning(unittest.TestCase):
    def test_takes_lethal_instead_of_blocking(self):
        s = combat_state([DEFEND(0), STRIKE(1), STRIKE(2)], [enemy("A_0", 12, attack=20)], hp=30)
        self.assertEqual(combat.choose_card(s)[1], {"card_index": 1, "target": "A_0"})

    def test_blocks_when_attack_would_kill(self):
        s = combat_state([STRIKE(0), DEFEND(1), DEFEND(2), DEFEND(3)], [enemy("A_0", 60, attack=14)], hp=12, max_hp=80)
        action, params = combat.choose_card(s)
        self.assertEqual(params["card_index"], 1)

    def test_attacks_when_enemy_is_not_attacking(self):
        s = combat_state([DEFEND(0), STRIKE(1)], [enemy("A_0", 60)], energy=1)
        self.assertEqual(combat.choose_card(s)[1]["card_index"], 1)

    def test_kills_the_enemy_it_can_kill(self):
        s = combat_state([STRIKE(0)], [enemy("BIG_0", 50, attack=5), enemy("SMALL_0", 6, attack=10)], energy=1)
        self.assertEqual(combat.choose_card(s)[1]["target"], "SMALL_0")

    def test_vulnerable_makes_lethal_reachable(self):
        # The real game names powers like VULNERABLE_POWER.
        vuln = [{"id": "VULNERABLE_POWER", "amount": 2}]
        s = combat_state([STRIKE(0), DEFEND(1)], [enemy("A_0", 9, attack=30, status=vuln)], energy=1, hp=10)
        self.assertEqual(combat.choose_card(s)[1]["card_index"], 0)

    def test_conditional_double_hit_needs_vulnerable(self):
        dismantle = card(0, "DISMANTLE", "Dismantle", "Attack", 1, "Deal 8 damage. If the enemy is Vulnerable, hits twice.", "AnyEnemy")
        info = parse(dismantle)
        self.assertEqual((info.damage, info.hits, info.double_if_vulnerable), (8, 1, True))
        vuln = [{"id": "VULNERABLE_POWER", "amount": 1}]
        # 8 * 1.5 * 2 = 24 kills the vulnerable one; the plain one would survive.
        s = combat_state([dismantle], [enemy("PLAIN_0", 20, attack=5), enemy("VULN_0", 24, attack=5, status=vuln)], energy=1)
        self.assertEqual(combat.choose_card(s)[1]["target"], "VULN_0")

    def test_blocks_at_full_health_when_it_cannot_kill(self):
        s = combat_state([STRIKE(0), DEFEND(1)], [enemy("A_0", 40, attack=10)], energy=1)
        self.assertEqual(combat.choose_card(s)[1]["card_index"], 1)

    def test_does_not_waste_energy_on_status_cards(self):
        slimed = card(0, "SLIMED", "Slimed", "Status", 1, "Exhaust.", "None")
        s = combat_state([slimed, STRIKE(1)], [enemy("A_0", 40)], energy=1)
        self.assertEqual(combat.choose_card(s)[1]["card_index"], 1)

    def test_slippery_is_stripped_with_small_hits_first(self):
        big = card(0, "HEMO", "Hemokinesis", "Attack", 1, "Lose 2 HP. Deal 15 damage.", "AnyEnemy")
        slip = [{"id": "SLIPPERY_POWER", "amount": 1}]
        s = combat_state([big, STRIKE(1)], [enemy("V_0", 100, status=slip)], energy=2)
        self.assertEqual(combat.choose_card(s)[1]["card_index"], 1)  # Strike eats the Slippery
        deep = [{"id": "SLIPPERY_POWER", "amount": 8}]
        s = combat_state([big, STRIKE(1)], [enemy("V_0", 100, status=deep)], energy=1)
        self.assertEqual(combat.choose_card(s)[1]["card_index"], 1)  # never pay HP for 1 damage

    def test_never_plays_a_card_that_can_kill_us(self):
        gambit = card(0, "THE_GAMBIT", "The Gambit", "Skill", 0, "Gain 50 Block. If you take unblocked attack damage this combat, die.", "Self")
        self.assertTrue(parse(gambit).deadly)
        self.assertLess(deck_value(gambit), 0)
        s = combat_state([gambit], [enemy("A_0", 60, attack=20)], energy=3)
        self.assertIsNone(combat.choose_card(s))

    def test_card_limit_spends_plays_on_the_best_cards(self):
        draw = card(0, "BT", "Battle Trance", "Skill", 0, "Draw 3 cards.", "Self")
        big = card(1, "BIG", "Big", "Attack", 1, "Deal 14 damage.", "AnyEnemy")
        s = combat_state([draw, big], [enemy("A_0", 60)], energy=3)
        s["player"]["status"] = [{"id": "RINGING_POWER", "amount": 1}]
        self.assertEqual(combat.choose_card(s)[1]["card_index"], 1)
        s["_plays_this_turn"] = 1
        self.assertIsNone(combat.choose_card(s))

    def test_thorns_cost_hp_per_hit(self):
        twin = card(0, "TWIN", "Twin Strike", "Attack", 1, "Deal 5 damage twice.", "AnyEnemy")
        big = card(1, "BIG", "Big", "Attack", 1, "Deal 10 damage.", "AnyEnemy")
        thorns = [{"id": "THORNS_POWER", "amount": 5}]
        s = combat_state([twin, big], [enemy("A_0", 60, status=thorns)], energy=1)
        self.assertEqual(combat.choose_card(s)[1]["card_index"], 1)

    def test_ignores_an_illusion_that_just_revives(self):
        illusion = [{"id": "ILLUSION_POWER", "amount": 1}, {"id": "MINION_POWER", "amount": 1}]
        foes = [enemy("EYE_0", 6, status=illusion), enemy("FOGMOG_0", 50, attack=8)]
        s = combat_state([STRIKE(0)], foes, energy=1)
        self.assertEqual(combat.choose_card(s)[1]["target"], "FOGMOG_0")

    def test_killing_the_leader_wins_even_with_minions_left(self):
        minion = [{"id": "MINION_POWER", "amount": 1}]
        foes = [enemy("MINION_0", 20, attack=30, status=minion), enemy("LEADER_0", 6)]
        s = combat_state([STRIKE(0), DEFEND(1)], foes, energy=1, hp=10)
        self.assertEqual(combat.choose_card(s), ("play_card", {"card_index": 0, "target": "LEADER_0"}))

    def test_races_to_the_stun_threshold_instead_of_blocking(self):
        plow = [{"id": "PLOW_POWER", "amount": 150}]
        s = combat_state([STRIKE(0), DEFEND(1)], [enemy("BEAST_0", 155, attack=24, status=plow)], energy=1, hp=19)
        self.assertEqual(combat.choose_card(s)[1]["card_index"], 0)  # 6 damage stuns it; 5 block would not save us

    def test_block_cards_are_useless_while_block_is_locked(self):
        s = combat_state([STRIKE(0), DEFEND(1)], [enemy("A_0", 40, attack=10)], energy=1)
        s["player"]["status"] = [{"id": "NO_BLOCK_POWER", "amount": 1}]
        self.assertEqual(combat.choose_card(s)[1]["card_index"], 0)

    def test_slow_enemy_gets_skills_first_and_the_big_hit_last(self):
        big = card(0, "BIG", "Big", "Attack", 1, "Deal 14 damage.", "AnyEnemy")
        slow = [{"id": "SLOW_POWER", "amount": 0}]
        s = combat_state([big, STRIKE(1), DEFEND(2)], [enemy("EFFIGY_0", 120, attack=10, status=slow)], energy=3)
        self.assertEqual(combat.choose_card(s)[1]["card_index"], 2)
        s = combat_state([big, STRIKE(1)], [enemy("EFFIGY_0", 120, status=slow)], energy=2)
        self.assertEqual(combat.choose_card(s)[1]["card_index"], 1)

    def test_blocks_a_hit_that_would_take_most_of_what_is_left(self):
        aoe = card(0, "BREAK", "Breakthrough", "Attack", 1, "Lose 1 HP. Deal 9 damage to ALL enemies.", "AllEnemies")
        bash = card(1, "BASH", "Bash", "Attack", 2, "Deal 8 damage. Apply 2 Vulnerable.", "AnyEnemy")
        minion = [{"id": "MINION_POWER", "amount": 1}]
        foes = [enemy("F_0", 39, attack=9, status=minion), enemy("F_1", 38, attack=8, status=minion), enemy("PRIEST_0", 98, attack=10)]
        s = combat_state([aoe, bash, DEFEND(2), DEFEND(3), STRIKE(4)], foes, energy=3, hp=37, kind="boss")
        s["player"]["max_hp"] = 80
        plays = []
        for _ in range(3):
            action = combat.choose_card(s)
            if action is None:
                break
            idx = action[1]["card_index"]
            plays.append(next(c["name"] for c in s["player"]["hand"] if c["index"] == idx))
            played = next(c for c in s["player"]["hand"] if c["index"] == idx)
            s["player"]["hand"] = [c for c in s["player"]["hand"] if c["index"] != idx]
            s["player"]["energy"] -= int(played["cost"])
            if played["name"] == "Defend":
                s["player"]["block"] += 5
        self.assertEqual(plays.count("Defend"), 2)

    def test_known_cards_are_nudged(self):
        shrug = card(0, "SHRUG_IT_OFF", "Shrug It Off", "Skill", 1, "Gain 8 Block. Draw 1 card.", "Self")
        barricade = card(0, "BARRICADE", "Barricade", "Power", 3, "Block is not removed at the start of your turn.", "Self")
        plain = card(0, "SOMETHING_NEW", "Something New", "Skill", 1, "Gain 8 Block. Draw 1 card.", "Self")
        self.assertGreater(deck_value(shrug), deck_value(plain))
        self.assertLess(deck_value(barricade), deck_value(plain))
        self.assertGreater(deck_value({**shrug, "name": "Shrug It Off+", "id": None}), deck_value(plain))

    def test_plating_counts_as_block(self):
        s = combat_state([STRIKE(0), DEFEND(1)], [enemy("A_0", 40, attack=5)], energy=1)
        self.assertEqual(combat.expected_loss(s), 5)
        s["player"]["status"] = [{"id": "PLATING_POWER", "amount": 5}]
        self.assertEqual(combat.expected_loss(s), 0)
        self.assertEqual(combat.choose_card(s)[1]["card_index"], 0)  # nothing to block: attack

    def test_weak_goes_on_the_attacker_and_saves_hp(self):
        weaken = card(0, "W", "Weaken", "Skill", 1, "Apply 2 Weak.", "AnyEnemy")
        s = combat_state([weaken], [enemy("IDLE_0", 30), enemy("HITTER_0", 60, attack=20)], energy=1, hp=30)
        self.assertEqual(combat.choose_card(s), ("play_card", {"card_index": 0, "target": "HITTER_0"}))

    def test_respects_energy_and_unplayable_cards(self):
        hand = [card(0, "BIG", "Big", "Attack", 3, "Deal 30 damage.", "AnyEnemy"), {**STRIKE(1), "can_play": False}]
        s = combat_state(hand, [enemy("A_0", 60)], energy=2)
        self.assertIsNone(combat.choose_card(s))
        self.assertEqual(combat.actions(s), [("end_turn", {})])

    def test_potions_only_when_it_matters(self):
        potion = {"slot": 1, "id": "FIRE_POTION", "can_use_in_combat": True, "target_type": "AnyEnemy"}
        calm = combat_state([STRIKE(0)], [enemy("A_0", 40, attack=5)], potions=[potion])
        self.assertEqual(combat.potion_actions(calm), [])
        boss = combat_state([STRIKE(0)], [enemy("A_0", 40, attack=5)], potions=[potion], kind="boss")
        self.assertEqual(combat.potion_actions(boss), [("use_potion", {"slot": 1, "target": "A_0"})])


class PowersThePlannerKnows(unittest.TestCase):
    def test_vigor_only_helps_the_first_attack(self):
        # With 8 Vigor each Strike reads "Deal 14 damage", but only the first one gets it:
        # 14 + 6 + 6 does not kill 30 HP, so the hit that is coming has to be blocked.
        strike = lambda i: card(i, "STRIKE_R", "Strike", "Attack", 1, "Deal 14 damage.", "AnyEnemy")
        state = combat_state([strike(0), strike(1), strike(2), DEFEND(3)], [enemy("A", 30, attack=30)], hp=20)
        state["player"]["status"] = [{"id": "VIGOR_POWER", "amount": 8}]
        line = combat.simulate([parse(c) for c in state["player"]["hand"][:3]], combat.read_enemies(state), **combat._context(state))
        self.assertLess(line.score, 0)  # the enemy lives and we do not

    def test_tender_weakens_every_later_card(self):
        state = combat_state([DEFEND(0), DEFEND(1), DEFEND(2)], [enemy("A", 90, attack=14)], hp=10)
        hand = [parse(c) for c in state["player"]["hand"]]
        plain = combat.simulate(hand, combat.read_enemies(state), **combat._context(state))
        state["player"]["status"] = [{"id": "TENDER_POWER", "amount": 0}]
        tender = combat.simulate(hand, combat.read_enemies(state), **combat._context(state))
        self.assertLess(tender.score, plain.score)  # 5 + 4 + 3 Block, not 15

    def test_slow_order_is_chosen_to_reach_the_kill(self):
        # 10 then 6x3 (+10%, rounded down) is 28; 6x3 then 10 (+10%) is 29, which kills.
        cut = card(0, "PHOTON_CUT", "Photon Cut", "Attack", 1, "Deal 10 damage.", "AnyEnemy")
        might = card(1, "CELESTIAL_MIGHT", "Celestial Might", "Attack", 2, "Deal 6 damage 3 times.", "AnyEnemy")
        foe = enemy("E", 29, attack=23, status=[{"id": "SLOW_POWER", "amount": 0}])
        state = combat_state([cut, might, DEFEND(2)], [foe], energy=3, hp=40)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 1, "target": "E"}))

    def test_stars_made_in_a_line_pay_for_a_later_card(self):
        star = card(0, "FALLING_STAR", "Falling Star", "Attack", 0, "Deal 8 damage. Apply 1 Weak. Apply 1 Vulnerable.", "AnyEnemy")
        star.update(star_cost=2, can_play=False, unplayable_reason="StarCostTooHigh")
        venerate = card(1, "VENERATE", "Venerate", "Skill", 1, "Gain [star_icon.png][star_icon.png].", "Self")
        state = combat_state([star, venerate, STRIKE(2), STRIKE(3)], [enemy("A", 28)], energy=2)
        state["player"]["stars"] = 1
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 1}))
        # ... but a card that is unplayable for another reason stays out.
        star["unplayable_reason"] = "BlockedByHook, StarCostTooHigh"
        self.assertNotEqual(combat.choose_card(state), ("play_card", {"card_index": 1}))

    def test_toxic_is_played_rather_than_sat_on(self):
        toxic = card(0, "TOXIC", "Toxic", "Status", 1, "At the end of your turn, if this is in your Hand, take 5 damage. Exhaust.", "Self")
        state = combat_state([toxic, DEFEND(1)], [enemy("A", 90, attack=4)], energy=1)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0}))  # 5 saved beats 4 blocked
        burn = card(0, "BURN", "Burn", "Status", 0, "Unplayable. At the end of your turn, if this is in your Hand, take 2 damage.", "Self")
        burn["can_play"] = False
        dying = combat_state([burn, STRIKE(1), DEFEND(2)], [enemy("A", 90, attack=5)], energy=1, hp=6)
        self.assertEqual(combat.choose_card(dying), ("play_card", {"card_index": 2}))  # 5 + 2 would kill

    def test_lightning_orbs_finish_a_lone_enemy_before_it_acts(self):
        state = combat_state([DEFEND(0), DEFEND(1)], [enemy("A", 5, attack=28)], energy=2, hp=20)
        state["player"].update(orbs=[{"id": "LIGHTNING_ORB", "passive_val": 3, "evoke_val": 8}] * 2, orb_slots=3)
        self.assertIsNone(combat.choose_card(state))  # nothing to block: it dies when the turn ends

    def test_withering_presence_makes_small_cards_not_worth_playing(self):
        poke = card(0, "POKE", "Poke", "Attack", 0, "Deal 3 damage.", "AnyEnemy")
        foe = enemy("A", 400, status=[{"id": "WITHERING_PRESENCE_POWER", "amount": 4}])
        self.assertIsNone(combat.choose_card(combat_state([poke], [foe], kind="boss")))
        self.assertIsNotNone(combat.choose_card(combat_state([poke, STRIKE(1)], [enemy("A", 400)], kind="boss")))

    def test_enrage_keeps_skills_for_when_they_are_needed(self):
        zap = card(0, "ZAP", "Zap", "Skill", 1, "Channel 1 Lightning.", "Self")
        angry = enemy("A", 100, attack=10, status=[{"id": "ENRAGE_POWER", "amount": 2}])
        self.assertEqual(combat.choose_card(combat_state([zap, STRIKE(1)], [angry], energy=1, kind="boss"))[1]["card_index"], 1)
        self.assertEqual(combat.choose_card(combat_state([zap], [angry], energy=1, kind="boss")), None)

    def test_crabs_are_brought_down_together(self):
        rage = [{"id": "CRAB_RAGE_POWER", "amount": 1}]
        crabs = [enemy("CRUSHER", 6, attack=6, status=rage), enemy("ROCKET", 120, attack=6, status=rage)]
        # Killing the Crusher now would enrage a Rocket with 120 HP left: hit the Rocket.
        self.assertEqual(combat.choose_card(combat_state([STRIKE(0)], crabs, energy=1, kind="boss"))[1]["target"], "ROCKET")
        # Both within reach: take them together.
        crabs = [enemy("CRUSHER", 6, attack=6, status=rage), enemy("ROCKET", 6, attack=6, status=rage)]
        self.assertIsNotNone(combat.choose_card(combat_state([STRIKE(0), STRIKE(1)], crabs, energy=2, kind="boss")))

    def test_drawing_is_worth_what_is_left_to_draw(self):
        skim = card(0, "SKIM", "Skim", "Skill", 1, "Draw 3 cards.", "Self")
        state = combat_state([skim, STRIKE(1)], [enemy("A", 200)], energy=1)
        wounds = [{"name": "Wound", "cost": "0", "description": "Unplayable."}] * 6
        claws = [{"name": "Claw", "cost": "0", "description": "Deal 9 damage."}] * 6
        state["player"]["draw_pile"] = wounds
        self.assertEqual(combat.choose_card(state)[1]["card_index"], 1)  # nothing worth drawing: Strike
        state["player"]["draw_pile"] = claws
        self.assertEqual(combat.choose_card(state)[1]["card_index"], 0)  # three free attacks beat one Strike

    def test_debilitate_doubles_vulnerable(self):
        foe = enemy("A", 12, attack=5, status=[{"id": "VULNERABLE_POWER", "amount": 1}, {"id": "DEBILITATE_POWER", "amount": 2}])
        self.assertEqual(combat.choose_card(combat_state([STRIKE(0), DEFEND(1)], [foe], energy=1)), ("play_card", {"card_index": 0, "target": "A"}))

    def test_deck_is_read_at_the_start_of_a_fight_only(self):
        bot = make_bot(FakeGame())
        state = combat_state([STRIKE(0), DEFEND(1)], [enemy("A", 30, attack=5)])
        state["player"].update(draw_pile_count=2, draw_pile=[{"name": "Bash", "cost": "2"}, {"name": "Strike", "cost": "1"}])
        bot._combat(state)
        self.assertEqual(bot.mem.deck_size, 4)
        # Later in the fight the piles hold Wounds and other things that are not in the deck.
        state["battle"]["round"] = 3
        state["player"].update(discard_pile_count=9, draw_pile_count=1, draw_pile=[{"name": "Wound"}])
        bot._combat(state)
        self.assertEqual(bot.mem.deck_size, 4)

    def test_star_cards_are_worth_less_without_stars_to_pay_for_them(self):
        comet = card(0, "COMET", "Comet", "Attack", 0, "Deal 33 damage.", "AnyEnemy")
        comet["star_cost"] = 5
        pulse = card(0, "ASTRAL_PULSE", "Astral Pulse", "Attack", 0, "Deal 6 damage to ALL enemies twice.", "AllEnemies")
        pulse["star_cost"] = 3
        glow = card(0, "GLOW", "Glow", "Skill", 1, "Gain [star_icon.png]. Draw 1 card.", "Self")
        deck = [STRIKE(i) for i in range(4)] + [DEFEND(i) for i in range(4)] + [pulse, pulse]
        self.assertLess(strategy.deck_fit(comet, deck), -2.0)
        self.assertGreater(strategy.deck_fit(glow, deck), 0.5)
        self.assertEqual(strategy.deck_fit(glow, [STRIKE(0)]), 0.0)


class OtherCharacters(unittest.TestCase):
    """What the Silent, Regent, Necrobinder and Defect do that the Ironclad does not."""

    def test_reads_their_effects(self):
        icon = "[defect_energy_icon.png]"
        self.assertEqual(parse(card(0, "X", "X", "Skill", 2, "Apply 3 Poison to a random enemy 3 times.")).poison, 9)
        self.assertEqual(parse(card(0, "X", "X", "Skill", 1, "Add 3 Shivs into your Hand. Exhaust.")).shivs, 3)
        self.assertEqual(parse(card(0, "X", "X", "Skill", 0, "Gain 9 [star_icon.png]. Exhaust.")).star_gain, 9)
        self.assertEqual(parse(card(0, "X", "X", "Skill", 1, "Gain [star_icon.png][star_icon.png].")).star_gain, 2)
        c = parse(card(0, "X", "X", "Skill", 1, f"Gain 7 Block. Next turn, gain {icon}."))
        self.assertEqual((c.block, c.next_energy, c.energy_gain), (7, 1, 0))
        c = parse(card(0, "X", "X", "Skill", 1, "Gain 4 Block. Next turn, gain 4 Block."))
        self.assertEqual((c.block, c.next_block), (4, 4))
        c = parse(card(0, "X", "X", "Attack", 1, "Deal 6 damage. Channel 1 Frost.", "AnyEnemy"))
        self.assertEqual((c.damage, c.channel), (6, ("FROST",)))
        c = parse(card(0, "X", "X", "Attack", 1, "Deal 5 damage for each Skill in your Hand. (Hits 0 times)", "AnyEnemy"))
        self.assertEqual(c.damage, 0)
        self.assertEqual(parse(card(0, "X", "X", "Skill", 2, "Summon 4. Gain 7 Block.")).summon, 4)
        self.assertEqual(parse(card(0, "X", "X", "Skill", 1, "ALL enemies lose 6 Strength this turn. Exhaust.")).enemy_strength_loss, 6)

    def test_poison_is_worth_more_on_a_stack_and_in_a_long_fight(self):
        self.assertEqual(combat._poison_total(5, 7), 15)  # 5 + 4 + 3 + 2 + 1
        self.assertEqual(combat._poison_total(15, 7) - combat._poison_total(10, 7), 35)  # 5 more for seven turns
        poison = parse(card(0, "DEADLY_POISON", "Deadly Poison", "Skill", 1, "Apply 5 Poison.", "AnyEnemy"))
        kw = dict(energy=1, stars=0, hp=60, max_hp=80, block=0)

        def gain(stack, fight):
            status = [{"id": "POISON_POWER", "amount": stack}] if stack else []
            foes = combat.read_enemies({"battle": {"enemies": [enemy("A", 300, status=status)]}})
            return combat.simulate([poison], foes, fight=fight, **kw).score - combat.simulate([], foes, fight=fight, **kw).score

        self.assertGreater(gain(10, "boss"), 2 * gain(0, "boss"))
        self.assertGreater(gain(0, "boss"), gain(0, "monster"))

    def test_poison_that_covers_its_hp_stops_the_attack(self):
        poison = card(0, "DEADLY_POISON", "Deadly Poison", "Skill", 1, "Apply 5 Poison.", "AnyEnemy")
        foe = enemy("A", 9, attack=20, status=[{"id": "POISON_POWER", "amount": 4}])
        state = combat_state([poison, DEFEND(1)], [foe], energy=1, hp=30)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0, "target": "A"}))

    def test_frost_orbs_count_as_block(self):
        foe = enemy("A", 100, attack=7)
        state = combat_state([DEFEND(0), STRIKE(1)], [foe], energy=1, hp=10)
        self.assertEqual(combat.choose_card(state)[1]["card_index"], 0)  # must block: 7 would hurt
        frost = {"id": "FROST_ORB", "name": "Frost", "passive_val": 2, "evoke_val": 5}
        state["player"]["orbs"] = [frost, frost, frost, frost]
        state["player"]["orb_slots"] = 4
        self.assertEqual(combat.choose_card(state)[1]["card_index"], 1)  # the orbs block it; attack

    def test_evoke_takes_the_orb_channeled_first(self):
        # Orbs come oldest first, and "your rightmost Orb" is that oldest one.
        dualcast = card(0, "DUALCAST", "Dualcast", "Skill", 0, "Evoke your rightmost Orb twice.", "Self")
        self.assertEqual(parse(dualcast).evoke, 2)
        self.assertEqual(parse(card(0, "QUADCAST", "Quadcast", "Skill", 1, "Evoke your rightmost Orb 4 times.")).evoke, 4)
        frost = ("FROST", 2, 5)
        glass = ("GLASS", 4, 8)
        foes = combat.read_enemies(combat_state([], [enemy("A", 100, attack=20)]))
        kw = dict(energy=0, stars=0, hp=19, max_hp=100, block=6, fight="boss", orb_slots=3)
        frost_first = combat.simulate([parse(dualcast)], foes, orbs=(frost, glass, glass), **kw).score
        glass_first = combat.simulate([parse(dualcast)], foes, orbs=(glass, glass, frost), **kw).score
        self.assertGreater(frost_first, glass_first + 20)  # 10 Block that keeps us alive against 16 damage

    def test_a_kill_by_orb_is_a_kill(self):
        dualcast = card(0, "DUALCAST", "Dualcast", "Skill", 1, "Evoke your rightmost Orb twice.", "Self")
        lightning = {"id": "LIGHTNING_ORB", "name": "Lightning", "passive_val": 3, "evoke_val": 8}
        state = combat_state([dualcast, DEFEND(1)], [enemy("A", 14, attack=30)], energy=1, hp=20)
        state["player"].update(orbs=[lightning], orb_slots=3)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0}))  # 16 kills it: no need to block
        # Vulnerable does nothing for orbs: 17 HP is out of reach, so it blocks.
        tough = enemy("A", 17, attack=30, status=[{"id": "VULNERABLE_POWER", "amount": 2}])
        state = combat_state([dualcast, DEFEND(1)], [tough], energy=1, hp=20)
        state["player"].update(orbs=[lightning], orb_slots=3)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 1}))

    def test_focus_strengthens_the_orbs_a_card_channels(self):
        zap = parse(card(0, "ZAP", "Zap", "Skill", 1, "Channel 1 Lightning.", "Self"))
        foes = lambda: combat.read_enemies(combat_state([], [enemy("A", 100)]))
        kw = dict(energy=1, stars=0, hp=50, max_hp=80, block=0, orb_slots=3)
        plain = combat.simulate([zap], foes(), **kw).score
        focused = combat.simulate([zap], foes(), focus_power=3, **kw).score
        self.assertGreater(focused, plain)

    def test_osty_takes_the_hit(self):
        foe = enemy("A", 100, attack=8)
        state = combat_state([DEFEND(0), STRIKE(1)], [foe], energy=1, hp=10)
        state["player"]["pets"] = [{"id": "OSTY", "alive": True, "hp": 9, "max_hp": 9}]
        self.assertEqual(combat.choose_card(state)[1]["card_index"], 1)

    def test_stars_pay_for_star_cards(self):
        venerate = card(0, "VENERATE", "Venerate", "Skill", 1, "Gain [star_icon.png][star_icon.png].")
        falling = card(1, "FALLING_STAR", "Falling Star", "Attack", 0, "Deal 8 damage. Apply 1 Weak. Apply 1 Vulnerable.", "AnyEnemy", star_cost="2")
        foes = combat.read_enemies(combat_state([], [enemy("A", 100)]))
        ctx = dict(energy=1, stars=0, hp=80, max_hp=80, block=0)
        self.assertIsNone(combat.simulate([parse(falling)], foes, **ctx))  # no Stars yet
        self.assertIsNotNone(combat.simulate([parse(venerate), parse(falling)], foes, **ctx))

    def test_their_key_cards_are_rated(self):
        fumes = card(0, "NOXIOUS_FUMES", "Noxious Fumes", "Power", 1, "At the start of your turn, apply 2 Poison to ALL enemies.", rarity="Uncommon")
        slice_ = card(1, "SLICE", "Slice", "Attack", 0, "Deal 6 damage.", "AnyEnemy")
        self.assertGreater(deck_value(fumes), 5.0)
        self.assertGreater(deck_value(fumes), deck_value(slice_))


class CrabRage(unittest.TestCase):
    def crabs(self, crusher_hp=6, rocket_hp=120):
        return [
            combat.Enemy(entity_id="CRUSHER_0", hp=crusher_hp, block=0, vulnerable=False, weak=False, incoming=6, rage=True),
            combat.Enemy(entity_id="ROCKET_0", hp=rocket_hp, block=0, vulnerable=False, weak=False, incoming=6, rage=True),
        ]

    def test_a_surviving_crab_gets_block_once_per_ally_that_died(self):
        foes = self.crabs(crusher_hp=0)
        known = set()
        combat._rage_blocks(foes, known)
        combat._rage_blocks(foes, known)  # the same death is not counted twice
        self.assertEqual(foes[1].block, combat.RAGE_BLOCK)
        self.assertEqual(known, {"CRUSHER_0"})

    def test_hitting_the_second_crab_after_the_first_dies_is_worth_less(self):
        line = [parse(STRIKE(0)), parse(STRIKE(1))]  # kill the Crusher, then hit the Rocket
        kw = dict(energy=2, stars=0, hp=80, max_hp=80, block=0, fight="boss")
        with_rage = combat.simulate(line, self.crabs(), **kw).score
        saved, combat.RAGE_BLOCK = combat.RAGE_BLOCK, 0
        try:
            without = combat.simulate(line, self.crabs(), **kw).score
        finally:
            combat.RAGE_BLOCK = saved
        self.assertLess(with_rage, without)


class StockRespawn(unittest.TestCase):
    def axebot(self, hp=5, respawns=2):
        return combat.Enemy(entity_id="AXEBOT_0", hp=hp, block=0, vulnerable=False, weak=False, incoming=15,
                            respawns=respawns, max_hp=85)

    def test_a_stock_enemy_comes_back_at_full_hp_while_it_has_stock(self):
        foes = [self.axebot(hp=0, respawns=1)]
        combat._respawns(foes)  # first death: back at full HP, one stack used
        self.assertEqual((foes[0].alive, foes[0].hp, foes[0].respawns), (True, 85, 0))
        foes[0].hp = 0
        combat._respawns(foes)  # second death, no stock left: really dead
        self.assertFalse(foes[0].alive)

    def test_killing_a_stock_enemy_is_not_a_win(self):
        kw = dict(energy=1, stars=0, hp=80, max_hp=80, block=0, fight="boss")
        line = [parse(STRIKE(0))]  # 6 damage on a 5-HP Axebot
        with_stock = combat.simulate(line, [self.axebot(respawns=2)], **kw).score
        without = combat.simulate(line, [self.axebot(respawns=0)], **kw).score
        self.assertLess(with_stock, without - combat.WIN_BONUS / 2)

    def test_stock_is_read_from_the_state(self):
        state = {"battle": {"enemies": [{"entity_id": "AXEBOT_0", "hp": 5, "max_hp": 85, "block": 0,
                                         "status": [{"id": "STOCK_POWER", "amount": 2}], "intents": []}]},
                 "player": {}}
        (foe,) = combat.read_enemies(state)
        self.assertEqual((foe.respawns, foe.max_hp), (2, 85))


if __name__ == "__main__":
    unittest.main()
