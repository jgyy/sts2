import unittest

from sts2bot.fight import battle, combat
from sts2bot import learn
from sts2bot.run import strategy
from sts2bot.cards import deck_value, parse

from ..fake_game import FakeGame, card
from ..helpers import DEFEND, STRIKE, combat_state, enemy, make_bot


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

    def test_a_slippery_stack_taken_off_is_worth_a_hit(self):
        # Vantom, round 1: 8 Slippery and a 7 coming. Each stack is a later hit that lands in
        # full, so the hits go first and the Defends wait: fights that still had Slippery on
        # round 5 were lost 4 times in 8.
        twin = card(0, "TWIN_STRIKE", "Twin Strike", "Attack", 1, "Deal 5 damage twice.", "AnyEnemy")
        slip = [{"id": "SLIPPERY_POWER", "amount": 8}]
        s = combat_state([twin, DEFEND(1), STRIKE(2), DEFEND(3), STRIKE(4)], [enemy("VANTOM_0", 173, attack=7, status=slip)], energy=3, kind="boss")
        line = []
        for _ in range(3):
            index = combat.choose_card(s)[1]["card_index"]
            line.append(index)
            s["player"]["hand"] = [c for c in s["player"]["hand"] if c["index"] != index]
            s["player"]["energy"] -= 1
        self.assertEqual(sorted(line), [0, 2, 4])
        # ... but not in front of the big hit when already hurt: there the Block comes first.
        s = combat_state([twin, DEFEND(1), DEFEND(3)], [enemy("VANTOM_0", 173, attack=26, status=slip)], energy=2, hp=30, kind="boss")
        self.assertIn(combat.choose_card(s)[1]["card_index"], (1, 3))

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
        from unittest import mock


        with mock.patch.dict(learn.KNOB, {"crowd": 0.0}):  # our own table (other players' runs rate Barricade high)
            self.assertGreater(deck_value(shrug), deck_value(plain))
            self.assertLess(deck_value(barricade), deck_value(plain))
            self.assertGreater(deck_value({**shrug, "name": "Shrug It Off+", "id": None}), deck_value(plain))

    def test_an_enemy_that_grows_every_turn_is_raced(self):
        sculptor = lambda powers: enemy("DEVOTED_SCULPTOR_0", 150, attack=12, status=powers)
        hand = [STRIKE(0), STRIKE(1), DEFEND(2), DEFEND(3)]
        first = lambda powers, hp=70: combat.choose_card(combat_state(hand, [sculptor(powers)], energy=2, hp=hp, max_hp=90))[1]["card_index"]
        self.assertIn(first([]), (2, 3))  # an enemy like any other: 12 coming, two Defends
        self.assertIn(first([{"id": "RITUAL_POWER", "amount": 9}]), (0, 1))  # 9 Strength a turn: every turn it lives is dearer
        self.assertIn(first([{"id": "RITUAL_POWER", "amount": 9}], hp=14), (2, 3))  # ... but not into the grave
        self.assertEqual(combat.read_enemies(combat_state([], [sculptor([{"id": "STRENGTH_POWER", "amount": 6}])]))[0].grows, 6.0)
        late = combat_state([], [sculptor([{"id": "STRENGTH_POWER", "amount": 6}])])
        late["battle"]["round"] = 4
        self.assertEqual(combat.read_enemies(late)[0].grows, 2.0)  # 6 Strength in three turns

    def test_a_big_attack_is_not_spent_on_a_minion_that_is_not_attacking(self):
        squeeze = card(0, "SQUEEZE", "Squeeze", "Attack", 1, "Osty deals 35 damage.", "AnyEnemy")
        egg = {**enemy("TOUGH_EGG_0", 17), "status": [{"id": "MINION_POWER", "amount": 1}], "intents": [{"type": "Summon", "label": ""}]}
        copter = enemy("OVICOPTER_0", 105, attack=16)
        state = combat_state([squeeze], [egg, copter], hp=69)
        state["player"]["pets"] = [{"name": "Osty", "hp": 3, "alive": True}]
        self.assertEqual(combat.choose_card(state)[1]["target"], "OVICOPTER_0")
        # A small attack that just kills the egg is still a kill worth taking, and so is any kill of a minion about to hit us.
        poke = card(0, "STRIKE_X", "Strike", "Attack", 1, "Deal 18 damage.", "AnyEnemy")
        self.assertEqual(combat._pick_target(combat.read_enemies(combat_state([poke], [egg, copter])), 18, 1).entity_id, "TOUGH_EGG_0")
        biter = {**egg, "intents": [{"type": "Attack", "label": "6"}]}
        self.assertEqual(combat._pick_target(combat.read_enemies(combat_state([squeeze], [biter, copter])), 35, 1).entity_id, "TOUGH_EGG_0")

    def test_the_enemy_that_does_most_for_its_health_is_hit_first(self):
        bug = enemy("BOWLBUG_ROCK_0", 46, attack=15)
        beetle = {**enemy("SLUMBERING_BEETLE_0", 83), "intents": [{"type": "Attack", "label": "16"}, {"type": "Buff", "label": ""}]}
        state = combat_state([STRIKE(0)], [bug, beetle], hp=80)
        target = lambda s: combat.choose_card(s)[1]["target"]
        self.assertEqual(target(state), "BOWLBUG_ROCK_0")  # nothing known of them: the one that dies soonest
        # Four rounds in: the bug has hit every other turn, the beetle slept for three and now grows every turn.
        seen = {**state, "_threat": {"BOWLBUG_ROCK_0": 7.5, "SLUMBERING_BEETLE_0": 4.0}}
        self.assertEqual(target(seen), "SLUMBERING_BEETLE_0")
        self.assertEqual(combat.read_enemies(seen)[1].threat, 24.0)
        # ... but a bug nearly dead is finished first, and two that hit alike are taken lowest first.
        seen["battle"]["enemies"][0]["hp"] = 12
        self.assertEqual(target(seen), "BOWLBUG_ROCK_0")
        twins = combat_state([STRIKE(0)], [enemy("A", 50, attack=10), enemy("B", 40, attack=10)], hp=80)
        self.assertEqual(target({**twins, "_threat": {"A": 10.0, "B": 10.0}}), "B")
        # Boss and elite fights keep their own order (the crabs, minions and their leaders).
        boss = {**seen, "state_type": "boss"}
        boss["battle"]["enemies"][0]["hp"] = 46
        self.assertEqual(target(boss), "BOWLBUG_ROCK_0")

    def test_energy_written_as_a_number_and_an_icon_is_read(self):
        icon = "[defect_energy_icon.png]"
        boost = card(0, "SUPERCRITICAL", "Supercritical", "Skill", 0, f"Gain 4{icon}. Exhaust.", "Self")
        self.assertEqual(parse(boost).energy_gain, 4)
        self.assertEqual(parse(card(0, "TURBO", "TURBO", "Skill", 0, f"Gain {icon}{icon}. Add a Void into your Discard Pile.", "Self")).energy_gain, 2)
        # With it, the 3-cost attack that the energy in hand cannot pay for is played.
        big = card(1, "METEOR", "Meteor", "Attack", 3, "Deal 30 damage.", "AnyEnemy")
        state = combat_state([boost, big], [enemy("JAW_WORM_0", 80)], energy=1)
        state["player"]["hand"][1].update(can_play=False, unplayable_reason="EnergyCostTooHigh")
        self.assertEqual(combat.choose_card(state)[1]["card_index"], 0)

    def test_borrowed_time_makes_the_cards_after_it_dearer(self):
        icon = "[necrobinder_energy_icon.png]"
        time = card(0, "BORROWED_TIME", "Borrowed Time", "Skill", 1, f"Gain 4{icon}. Cards cost an additional {icon} this turn.", "Self")
        foe = [combat.Enemy("A", 200, 0, False, False, 0)]
        ctx = dict(energy=3, stars=0, hp=60, max_hp=60, block=0)
        strikes = [parse(STRIKE(i)) for i in range(1, 5)]
        # 3 - 1 + 4 = 6 energy at 2 a Strike: three of them, not four.
        self.assertIsNotNone(combat.simulate([parse(time), *strikes[:3]], foe, **ctx))
        self.assertIsNone(combat.simulate([parse(time), *strikes], foe, **ctx))
        self.assertIsNotNone(combat.simulate(strikes[:3], foe, **ctx))

    def test_cards_that_pay_for_or_repeat_the_next_one(self):
        foe = lambda: [combat.Enemy("A", 200, 0, False, False, 0)]
        ctx = dict(energy=3, stars=0, hp=60, max_hp=60, block=0)
        big = parse(card(1, "METEOR", "Meteor", "Attack", 3, "Deal 30 damage.", "AnyEnemy"))
        strike, guard = parse(STRIKE(2)), parse(DEFEND(3))
        value = lambda line, **kw: combat.simulate(line, foe(), **{**ctx, **kw}).score - combat.simulate([], foe(), **{**ctx, **kw}).score
        # One-Two Punch: the next Attack twice, and only the next one.
        punch = parse(card(0, "ONE_TWO_PUNCH", "One-Two Punch", "Skill", 1, "This turn, your next Attack is played an extra time.", "Self"))
        self.assertEqual(value([punch, strike, strike]), 18.0)
        self.assertEqual(value([punch, guard, strike]), 12.0)  # a Skill in between does not use it up
        # Burst: the next Skill's Block twice.
        burst = parse(card(0, "BURST", "Burst", "Skill", 1, "This turn, your next Skill is played an extra time.", "Self"))
        hit = [combat.Enemy("A", 200, 0, False, False, 20, incoming_hits=1)]
        self.assertEqual(combat.simulate([burst, guard], hit, **ctx).lost, 10)
        # Double Energy: 3 - 1 = 2, doubled, pays for the 3-cost card with one to spare.
        double = parse(card(0, "DOUBLE_ENERGY", "Double Energy", "Skill", 1, "Double your Energy. Exhaust.", "Self"))
        self.assertIsNone(combat.simulate([big, strike], foe(), **ctx))
        self.assertEqual(value([double, big, strike]), 36.0)
        # Bullet Time: what is left of the hand costs nothing.
        bullet = parse(card(0, "BULLET_TIME", "Bullet Time", "Skill", 3, "You cannot draw additional cards this turn. ALL cards in your Hand are free to play this turn.", "Self"))
        self.assertEqual(value([bullet, big, strike, strike]), 42.0)

    def test_a_power_other_players_win_with_is_worth_playing(self):
        from sts2bot.fight import battle

        barricade = card(0, "BARRICADE", "Barricade", "Power", 3, "Block is not removed at the start of your turn.", "Self")
        self.assertGreater(battle._power_worth(parse(barricade)), 12.0)  # (our own table says 4)
        unknown = card(0, "NEW_POWER", "New Power", "Power", 2, "Something nobody has rated.", "Self")
        self.assertEqual(battle._power_worth(parse(unknown)), 9.0)
        # A boss that is only buffing itself: the turn goes to Barricade, not to three Strikes.
        hand = [barricade, STRIKE(1), STRIKE(2), STRIKE(3)]
        quiet = combat_state(hand, [enemy("QUEEN_0", 400)], hp=80, kind="boss")
        self.assertEqual(combat.choose_card(quiet)[1]["card_index"], 0)
        # An ordinary fight is over before it pays: there the three Strikes go first, as they always did.
        self.assertEqual(battle._power_worth(parse(barricade), long_fight=False), 4.0)
        self.assertNotEqual(combat.choose_card({**quiet, "state_type": "monster"})[1]["card_index"], 0)

    def test_block_left_over_counts_once_it_stays(self):
        guard = card(0, "IMPERVIOUS", "Impervious", "Skill", 2, "Gain 30 Block. Exhaust.", "Self")
        state = combat_state([guard], [enemy("A", 200, attack=10)], energy=2, hp=40)
        foes = combat.read_enemies(state)
        plain = combat.simulate([parse(guard)], foes, **combat._context(state)).score
        state["player"]["status"] = [{"id": "BARRICADE_POWER", "amount": 1}]
        held = combat.simulate([parse(guard)], foes, **combat._context(state)).score
        self.assertGreater(held, plain + 15.0)  # 20 Block for next turn

    def test_an_ethereal_power_is_played_while_it_can_be(self):
        echo = card(0, "ECHO_FORM", "Echo Form", "Power", 3, "Ethereal. The first card you play each turn is played an extra time.", "Self")
        self.assertTrue(parse(echo).ethereal)
        hand = [echo, STRIKE(1), STRIKE(2), DEFEND(3)]
        boss = combat_state(hand, [enemy("AEONGLASS_0", 500, attack=22)], hp=75, max_hp=80, kind="boss")
        self.assertEqual(combat.choose_card(boss)[1]["card_index"], 0)  # 22 damage taken for the rest of the fight doubled
        dying = combat_state(hand, [enemy("AEONGLASS_0", 500, attack=22)], hp=12, max_hp=80, kind="boss")
        self.assertNotEqual(combat.choose_card(dying)[1]["card_index"], 0)

    def test_a_heal_is_worth_the_hp_it_gives_back(self):
        heal = parse(card(0, "NOT_YET", "Not Yet", "Skill", 2, "Heal 10 HP. Exhaust.", "Self"))
        self.assertEqual(heal.heal, 10)
        foe = [combat.Enemy("A", 200, 0, False, False, 0)]
        hurt = dict(energy=3, stars=0, hp=20, max_hp=60, block=0)
        self.assertGreater(combat.simulate([heal], foe, **hurt).score, combat.simulate([], foe, **hurt).score + 20)
        full = dict(energy=3, stars=0, hp=60, max_hp=60, block=0)
        self.assertLess(combat.simulate([heal], foe, **full).score, combat.simulate([], foe, **full).score + 1)
        # ... and it is there for the blow that comes this turn.
        foe = [combat.Enemy("A", 200, 0, False, False, 25, incoming_hits=1)]
        self.assertLess(combat.simulate([heal], foe, **hurt).score, 0)  # 30 HP against 25: hurt badly
        self.assertGreater(combat.simulate([heal], foe, **hurt).score, combat.simulate([], foe, **hurt).score + 100_000)  # ... not dead

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


class WhatSoftensTheBlow(unittest.TestCase):
    def test_orichalcum_and_regen_are_counted(self):
        state = combat_state([], [enemy("A", 100, attack=18)], hp=60, max_hp=80)
        self.assertEqual(combat.expected_end_loss(state), 18)
        state["player"]["relics"] = [{"id": "ORICHALCUM", "description": "If you end your turn without Block, gain 6 Block."}]
        self.assertEqual(combat.expected_end_loss(state), 12)
        state["player"]["status"] = [{"id": "REGEN_POWER", "amount": 5}]
        self.assertEqual(combat.expected_end_loss(state), 7)
        state["player"]["block"] = 3  # some Block already: the relic stays out of it
        self.assertEqual(combat.expected_end_loss(state), 10)


class Potions(unittest.TestCase):
    @staticmethod
    def potion(slot, name, text, target="Self"):
        return {"slot": slot, "id": name.upper().replace(" ", "_"), "name": name, "description": text, "target_type": target, "can_use_in_combat": True}

    def boss(self, potions, hp=75, block=0, foe_hp=300, floor=33, kind="boss"):
        state = combat_state([STRIKE(0)], [enemy("A", foe_hp, attack=10)], hp=hp, max_hp=75, block=block, kind=kind, potions=potions)
        state["run"]["floor"] = floor
        return [a[1]["slot"] for a in combat.potion_actions(state)]

    def test_a_belt_that_refills_every_fight_is_emptied_every_fight(self):
        strength = self.potion(0, "Strength Potion", "Gain 2 Strength.")
        fire = self.potion(1, "Fire Potion", "Deal 20 damage.", "AnyEnemy")
        state = combat_state([STRIKE(0)], [enemy("A", 60, attack=10)], hp=75, max_hp=75, potions=[strength, fire])
        state["run"]["floor"] = 40
        state["player"]["max_potion_slots"] = 3
        self.assertEqual(combat.potion_actions(state), [])  # an ordinary fight, nothing wrong
        state["player"]["relics"] = [{"id": "DELICATE_FROND", "description": "At the start of each combat, fill all empty potion slots with random Potions."}]
        self.assertEqual(sorted(a[1]["slot"] for a in combat.potion_actions(state)), [0, 1])

    def test_a_big_belt_keeps_three_for_the_boss_and_drinks_the_rest_on_the_way(self):
        # Six slots, five held: never "full", so nothing was spare, and a Regent went from 53 HP to 16 in
        # two ordinary fights holding all five. The two a boss needs least go on the way; near the boss none.
        belt = [self.potion(0, "Strength Potion", "Gain 2 Strength."), self.potion(1, "Swift Potion", "Draw 3 cards."),
                self.potion(2, "Fire Potion", "Deal 20 damage.", "AnyEnemy"), self.potion(3, "Attack Potion", "Add 1 of 3 random Attack cards into your Hand."),
                self.potion(4, "Focus Potion", "Gain 2 Focus.")]
        state = combat_state([STRIKE(0)], [enemy("A", 60, attack=10)], hp=60, max_hp=75, potions=belt)
        state["run"]["floor"] = 21
        state["player"]["max_potion_slots"] = 6
        self.assertEqual(sorted(a[1]["slot"] for a in combat.potion_actions(state)), [1, 3])
        state["run"]["floor"] = 30  # three floors before the boss: all five are for it
        self.assertEqual(combat.potion_actions(state), [])

    def test_the_potion_drunk_on_the_way_is_the_one_a_boss_needs_least(self):
        # A full belt cannot take the next potion, so one goes in an ordinary fight: not the Strength Potion.
        strength = self.potion(0, "Strength Potion", "Gain 2 Strength.")
        swift = self.potion(1, "Swift Potion", "Draw 3 cards.")
        state = combat_state([STRIKE(0)], [enemy("A", 60, attack=10)], hp=75, max_hp=75, potions=[strength, swift])
        state["run"]["floor"] = 22
        state["player"]["max_potion_slots"] = 2
        self.assertEqual([a[1]["slot"] for a in combat.potion_actions(state)], [1])
        # ... and none at all once the boss is near (nine floors in act 2): a slot emptied there often stays empty.
        state["run"]["floor"] = 24
        self.assertEqual(combat.potion_actions(state), [])
        state["run"]["floor"] = 23
        self.assertEqual(len(combat.potion_actions(state)), 1)
        from sts2bot.run import potions
        self.assertGreater(potions.boss_worth(strength), potions.boss_worth(self.potion(0, "Fire Potion", "Deal 20 damage.")))
        self.assertEqual(potions.boss_worth({"potion_name": "Odd Vial", "potion_description": "Deal 31 damage."}), 31.0)  # (unlisted: from its text)
        self.assertGreater(potions.boss_worth({"name": "Odd Tonic", "description": "Gain 3 Strength."}), 20.0)

    def test_no_potion_is_poured_out_at_a_sleeping_boss(self):
        energy = self.potion(0, "Energy Potion", "Gain [defect_energy_icon.png][defect_energy_icon.png].")
        strength = self.potion(1, "Strength Potion", "Gain 2 Strength.")
        hand = [STRIKE(0), STRIKE(1), STRIKE(2), STRIKE(3), STRIKE(4)]
        state = combat_state(hand, [enemy("LAGAVULIN_MATRIARCH_0", 222, status=[{"id": "ASLEEP_POWER", "amount": 3}])], kind="boss", potions=[energy, strength])
        self.assertEqual(combat.potion_actions(state), [])
        state["battle"]["enemies"][0]["status"][0]["amount"] = 1  # the turn before she wakes: now they count
        plan: dict = {}
        combat.choose_card(state, plan)
        self.assertEqual(sorted(a[1]["slot"] for a in combat.potion_actions(state, plan["lost"], plan["line"])), [0, 1])

    def test_an_energy_potion_waits_for_a_hand_that_can_spend_it(self):
        energy = self.potion(0, "Energy Potion", "Gain [ironclad_energy_icon.png][ironclad_energy_icon.png].")
        big = card(3, "BLUDGEON", "Bludgeon", "Attack", 3, "Deal 32 damage.", "AnyEnemy")
        idle = enemy("A", 300)  # (round 1 of a boss that only buffs: the Defends in hand are no use for energy)
        state = combat_state([STRIKE(0), STRIKE(1), DEFEND(2), DEFEND(3), DEFEND(4)], [idle], kind="boss", potions=[energy])
        self.assertEqual(combat.actions(state)[0][0], "play_card")
        # ... with an attack the three energy cannot pay for, it is drunk.
        state = combat_state([STRIKE(0), STRIKE(1), DEFEND(2), big], [idle], kind="boss", potions=[energy])
        self.assertEqual(combat.actions(state)[0], ("use_potion", {"slot": 0}))
        # ... and for Block when the plan still loses HP.
        state = combat_state([DEFEND(0), DEFEND(1), DEFEND(2), DEFEND(3), DEFEND(4)], [enemy("A", 300, attack=30)], kind="boss", potions=[energy])
        self.assertEqual(combat.actions(state)[0], ("use_potion", {"slot": 0}))

    def test_entropic_brew_is_drunk_at_once(self):
        brew = self.potion(0, "Entropic Brew", "Fill all your empty potion slots with random potions.")
        strength = self.potion(1, "Strength Potion", "Gain 2 Strength.")
        self.assertEqual(self.boss([brew, strength], floor=5, kind="monster"), [0])

    def test_fruit_juice_is_drunk_in_the_first_fight(self):
        juice = self.potion(0, "Fruit Juice", "Gain 5 Max HP.")
        strength = self.potion(1, "Strength Potion", "Gain 2 Strength.")
        # An ordinary fight, nothing wrong, a belt with room: the Strength Potion waits, the juice does not.
        self.assertEqual(self.boss([juice, strength], floor=5, kind="monster"), [0])
        self.assertEqual(self.boss([strength], floor=5, kind="monster"), [])

    def test_potions_thrown_at_the_enemy_wait_out_the_test_subjects_first_form(self):
        vuln = self.potion(0, "Vulnerable Potion", "Apply 3 Vulnerable.", "AnyEnemy")
        strength = self.potion(1, "Strength Potion", "Gain 2 Strength.")

        def used(powers):
            foe = enemy("TEST_SUBJECT_0", 100, attack=10, status=powers)
            state = combat_state([STRIKE(0)], [foe], hp=75, max_hp=75, kind="boss", potions=[vuln, strength])
            state["run"]["floor"] = 48
            return [a[1]["slot"] for a in combat.potion_actions(state)]

        first = [{"id": "ADAPTABLE_POWER", "amount": 1}, {"id": "ENRAGE_POWER", "amount": 2}]
        self.assertEqual(used(first), [1])  # our own Strength lasts; its Vulnerable would not
        second = [{"id": "ADAPTABLE_POWER", "amount": 1}, {"id": "PAINFUL_STABS_POWER", "amount": 1}]
        self.assertEqual(used(second), [0, 1])
        fire = self.potion(0, "Fire Potion", "Deal 20 damage.", "AnyEnemy")
        foe = enemy("TEST_SUBJECT_0", 300, attack=10, status=[{"id": "INTANGIBLE_POWER", "amount": 1}])
        state = combat_state([STRIKE(0)], [foe], hp=75, max_hp=75, kind="boss", potions=[fire])
        self.assertEqual(combat.potion_actions(state), [])  # 20 damage would take 1 HP this turn

    def test_a_potion_does_not_go_into_artifact(self):
        """Seen 2026-11-21: a Weak Potion on round 1 against Aeonglass, Artifact 3."""
        weak = self.potion(0, "Weak Potion", "Apply 3 Weak.", "AnyEnemy")
        strength = self.potion(1, "Strength Potion", "Gain 2 Strength.")

        def used(powers, hp=75):
            foe = enemy("AEONGLASS_0", 500, attack=22, status=powers)
            state = combat_state([STRIKE(0)], [foe], hp=hp, max_hp=75, kind="boss", potions=[weak, strength])
            state["run"]["floor"] = 48
            return [a[1]["slot"] for a in combat.potion_actions(state)]

        self.assertEqual(used([{"id": "ARTIFACT_POWER", "amount": 3}]), [1])
        self.assertEqual(used([]), [0, 1])
        self.assertEqual(used([{"id": "ARTIFACT_POWER", "amount": 3}], hp=20), [0, 1])  # about to die: nothing is being saved

    def test_a_potion_that_hits_us_too_waits_for_block(self):
        foul = self.potion(0, "Foul Potion", "Deal 12 damage to ALL players and enemies. Can be thrown at the Merchant for 100 Gold instead.")
        fire = self.potion(1, "Fire Potion", "Deal 20 damage.", "AnyEnemy")
        self.assertEqual(self.boss([foul, fire]), [1])
        self.assertEqual(self.boss([foul, fire], block=12), [1])  # that Block is for the 10 coming
        self.assertEqual(self.boss([foul, fire], block=22), [0, 1])

    def test_no_potion_when_the_hand_already_ends_the_danger(self):
        belt = [self.potion(0, "Strength Potion", "Gain 2 Strength."), self.potion(1, "Foul Potion", "Deal 12 damage to ALL players and enemies.")]

        def used(foe_hp):
            state = combat_state([STRIKE(0), STRIKE(1)], [enemy("A", foe_hp, attack=20)], hp=31, max_hp=90, kind="monster", potions=belt)
            state["run"]["floor"] = 30
            return [a for a in combat.actions(state) if a[0] == "use_potion"]

        self.assertEqual(used(12), [])  # two Strikes kill it: nothing is coming
        # Out of energy behind enough Block, on low HP: nothing is coming either.
        state = combat_state([STRIKE(0)], [enemy("A", 60, attack=12)], energy=0, hp=20, max_hp=90, block=17, kind="monster", potions=belt)
        state["run"]["floor"] = 24
        self.assertEqual([a for a in combat.actions(state) if a[0] == "use_potion"], [])
        self.assertTrue(used(60))  # 20 of 31 HP is coming and the hand cannot stop it

    def test_a_potion_waits_for_the_moment_it_does_something(self):
        def used(potion, hand, line=None, block=0, attack=30, discard=0, hp=75):
            state = combat_state(hand, [enemy("A", 300, attack=attack)], hp=hp, max_hp=75, block=block, kind="boss", potions=[potion])
            state["run"]["floor"] = 33
            state["player"]["discard_pile_count"] = discard
            return bool(combat.potion_actions(state, None, line))

        # "Triple your Block": 11 of 14 tripled nothing. It goes once the turn's Block is up and is still short.
        fortifier = self.potion(0, "Fortifier", "Triple your Block.")
        self.assertFalse(used(fortifier, [DEFEND(0)], line=[parse(DEFEND(0))]))
        self.assertFalse(used(fortifier, [], line=[], block=0))
        self.assertTrue(used(fortifier, [], line=[], block=10))
        self.assertFalse(used(fortifier, [], line=[], block=10, attack=8))
        # "Put a card from your Discard Pile into your Hand": not on round 1, when there is none.
        memories = self.potion(0, "Liquid Memories", "Put a card from your Discard Pile into your Hand. It's free to play this turn.")
        self.assertFalse(used(memories, [STRIKE(0)]))
        self.assertTrue(used(memories, [STRIKE(0)], discard=6))
        # Strength for one turn is for a turn of attacks (it was kept for the turns we Block).
        flex = self.potion(0, "Flex Potion", "Gain 5 Strength. At the end of your turn, lose 5 Strength.")
        strikes = [parse(STRIKE(i)) for i in range(3)]
        self.assertFalse(used(flex, [STRIKE(0), DEFEND(1), DEFEND(2)], line=[parse(DEFEND(1)), parse(DEFEND(2))]))
        self.assertTrue(used(flex, [STRIKE(0), STRIKE(1), STRIKE(2)], line=strikes))
        # "Exhaust your Hand. Draw 10 cards": for a hand of dead cards, not for the opening one.
        glow = self.potion(0, "Glowwater Potion", "Exhaust your Hand. Draw 10 cards.")
        wound = lambda i: card(i, "WOUND", "Wound", "Status", 0, "Unplayable.")
        self.assertFalse(used(glow, [STRIKE(0), DEFEND(1), STRIKE(2)]))
        self.assertTrue(used(glow, [STRIKE(0), wound(1), wound(2)]))
        # Snecko Oil is drunk like any other (the costs it rolls are in the state).
        self.assertTrue(used(self.potion(0, "Snecko Oil", "Draw 7 cards. Randomize the cost of cards in your Hand this turn."), [STRIKE(0)]))
        # "Your next card is played an extra time": kept for a card worth playing twice ...
        twice = self.potion(0, "Duplicator", "This turn, your next card is played an extra time.")
        heavy = card(1, "BLUDGEON", "Bludgeon", "Attack", 2, "Deal 14 damage.", "AnyEnemy")
        self.assertFalse(used(twice, [STRIKE(0)], line=[parse(STRIKE(0))]))
        self.assertTrue(used(twice, [STRIKE(0), heavy], line=[parse(STRIKE(0)), parse(heavy)]))
        # ... and that card is the one played next.
        state = combat_state([STRIKE(0), heavy], [enemy("A", 300)], energy=3, kind="boss")
        state["player"]["status"] = [{"id": "DUPLICATION_POWER", "amount": 1}]
        self.assertEqual(combat.choose_card(state)[1]["card_index"], 1)

    def test_healing_waits_until_it_is_needed(self):
        blood = self.potion(0, "Blood Potion", "Heal for 20% of your Max HP.")
        self.assertEqual(self.boss([blood]), [])
        self.assertEqual(self.boss([blood], hp=30), [0])

    def test_potions_are_kept_for_the_boss_next_door(self):
        belt = [self.potion(i, "Fire Potion", "Deal 20 damage.", "AnyEnemy") for i in range(3)]
        self.assertEqual(self.boss(belt, floor=20, kind="monster"), [0])  # a full belt: one is free to use
        self.assertEqual(self.boss(belt, floor=31, kind="monster"), [])  # two rooms from the boss
        self.assertEqual(self.boss(belt, floor=33, foe_hp=20), [])  # the fight is over anyway

    def test_an_elite_gets_the_spare_potion_and_the_rest_wait_for_the_boss(self):
        belt = [self.potion(i, "Fire Potion", "Deal 20 damage.", "AnyEnemy") for i in range(3)]
        self.assertEqual(self.boss(belt, floor=22, kind="elite"), [0])  # a full belt: one is free
        self.assertEqual(self.boss(belt[:2], floor=22, kind="elite"), [])
        self.assertEqual(self.boss(belt, floor=31, kind="elite"), [])  # the boss is two rooms on
        self.assertEqual(self.boss(belt[:2], floor=24, kind="elite", hp=35), [0, 1])  # the fight has gone wrong
        self.assertEqual(self.boss(belt[:2], floor=33), [0, 1])  # the boss gets all of them


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

    def test_the_stars_a_fight_starts_with_do_not_pay_for_every_cycle(self):
        # The starter deck (Falling Star 2, Venerate +2) with one Astral Pulse: a third star card
        # would ask for 8 Stars a cycle of a deck that makes 2 (and has 3 once, when the fight starts).
        star = lambda cid, name, n: {**card(0, cid, name, "Attack", 0, "Deal 8 damage.", "AnyEnemy"), "star_cost": n}
        venerate = card(0, "VENERATE", "Venerate", "Skill", 1, "Gain [star_icon.png][star_icon.png].", "Self")
        deck = [STRIKE(i) for i in range(4)] + [DEFEND(i) for i in range(4)] + [star("FALLING_STAR", "Falling Star", 2), venerate, star("ASTRAL_PULSE", "Astral Pulse", 3)]
        self.assertLess(strategy.deck_fit(star("GAMMA_BLAST", "Gamma Blast", 3), deck), -2.0)
        genesis = card(0, "GENESIS", "Genesis", "Power", 2, "At the start of your turn, gain [star_icon.png][star_icon.png].", "Self")
        self.assertGreater(strategy.deck_fit(star("GAMMA_BLAST", "Gamma Blast", 3), deck + [genesis]), -0.1)  # Genesis pays for it


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

    def test_poison_does_not_kill_through_intangible(self):
        foe = enemy("FYSH", 6, attack=9, status=[{"id": "POISON_POWER", "amount": 8}, {"id": "INTANGIBLE_POWER", "amount": 1}])
        state = combat_state([DEFEND(0)], [foe], energy=1, hp=29)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0}))

    def test_poison_that_covers_its_hp_stops_the_attack(self):
        poison = card(0, "DEADLY_POISON", "Deadly Poison", "Skill", 1, "Apply 5 Poison.", "AnyEnemy")
        foe = enemy("A", 9, attack=20, status=[{"id": "POISON_POWER", "amount": 4}])
        state = combat_state([poison, DEFEND(1)], [foe], energy=1, hp=30)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0, "target": "A"}))

    def test_a_plasma_orb_is_evoked_to_pay_for_more_cards(self):
        # Seen against the Insatiable: 1 energy, a Plasma orb in front, and the bot
        # played Ball Lightning alone. Dualcast first is 4 energy: both attacks.
        hand = [
            card(0, "BALL_LIGHTNING", "Ball Lightning+", "Attack", 1, "Deal 10 damage. Channel 1 Lightning.", "AnyEnemy"),
            card(1, "DUALCAST", "Dualcast", "Skill", 1, "Evoke your rightmost Orb twice.", "Self"),
            card(2, "EXTERMINATE", "Exterminate", "Attack", 1, "Deal 4 damage to ALL enemies 4 times.", "AllEnemies"),
            card(3, "STRIKE_D", "Strike", "Attack", 1, "Deal 6 damage.", "AnyEnemy"),
        ]
        state = combat_state(hand, [enemy("THE_INSATIABLE_0", 161, "10x2")], energy=1, hp=35, max_hp=75, kind="boss")
        state["player"].update(orbs=[{"id": "PLASMA_ORB", "passive_val": 1, "evoke_val": 2}], orb_slots=3)
        self.assertEqual(combat.choose_card(state)[1]["card_index"], 1)
        # With nothing to spend it on, the orb and its energy every turn are kept.
        state["player"]["hand"] = hand[:2]
        self.assertEqual(combat.choose_card(state)[1]["card_index"], 0)

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

    def test_shatter_sets_off_every_orb(self):
        shatter = parse(card(0, "SHATTER", "Shatter", "Attack", 1, "Deal 7 damage to ALL enemies. Evoke all of your Orbs twice. Exhaust.", "AllEnemies"))
        self.assertEqual(shatter.evoke_all, 2)
        foes = lambda: combat.read_enemies(combat_state([], [enemy("A", 200, attack=30)]))
        kw = dict(energy=1, stars=0, hp=40, max_hp=80, block=0, orb_slots=3)
        bare = combat.simulate([shatter], foes(), **kw).score
        loaded = combat.simulate([shatter], foes(), orbs=(("LIGHTNING", 3, 8), ("FROST", 2, 5)), **kw).score
        self.assertGreater(loaded, bare + 30)  # 16 damage and 10 Block on top

    def test_a_kill_by_orb_is_a_kill(self):
        dualcast = card(0, "DUALCAST", "Dualcast", "Skill", 1, "Evoke your rightmost Orb twice.", "Self")
        lightning = {"id": "LIGHTNING_ORB", "name": "Lightning", "passive_val": 3, "evoke_val": 8}
        state = combat_state([dualcast, DEFEND(1)], [enemy("A", 14, attack=30)], energy=1, hp=20)
        state["player"].update(orbs=[lightning], orb_slots=3)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0}))  # 16 kills it: no need to block
        # Vulnerable does nothing for orbs: 17 HP is out of reach, so it blocks (and lives, on 26 HP).
        tough = enemy("A", 17, attack=30, status=[{"id": "VULNERABLE_POWER", "amount": 2}])
        state = combat_state([dualcast, DEFEND(1)], [tough], energy=1, hp=26)
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
        state = combat_state([DEFEND(0), STRIKE(1)], [foe], energy=1, hp=80)
        state["player"]["pets"] = [{"id": "OSTY", "alive": True, "hp": 9, "max_hp": 9}]
        self.assertEqual(combat.expected_end_loss(state), 0)
        self.assertEqual(combat.choose_card(state)[1]["card_index"], 1)
        # ... but what he soaks now he cannot soak later: badly hurt, the Defend keeps him standing.
        state["player"]["hp"] = 10
        self.assertEqual(combat.choose_card(state)[1]["card_index"], 0)

    def test_regret_costs_a_point_for_every_card_still_held(self):
        regret = card(0, "REGRET", "Regret", "Curse", 0, "Unplayable. At the end of your turn, if this is in your Hand, lose 1 HP for each card in your Hand.")
        regret["can_play"] = False
        state = combat_state([regret, DEFEND(1), DEFEND(2), STRIKE(3)], [enemy("A", 100)], energy=0, hp=30)
        self.assertEqual(combat.expected_end_loss(state), 4)  # (it was read as "lose 1 HP")
        state["player"]["energy"] = 1
        self.assertIsNotNone(combat.choose_card(state))  # a card played is a point saved

    def test_osty_does_not_stop_what_burns_in_the_hand(self):
        """Seen 2026-11-21: Osty on 20, two Toxics in hand, "expecting to lose 0", and 10 HP gone."""
        toxic = lambda i: card(i, "TOXIC", "Toxic", "Status", 1, "At the end of your turn, if this is in your Hand, take 5 damage. Exhaust.")
        state = combat_state([toxic(0), toxic(1)], [enemy("A", 100, attack=6)], energy=0, hp=21, max_hp=75)
        state["player"]["pets"] = [{"id": "OSTY", "alive": True, "hp": 20, "max_hp": 20}]
        self.assertEqual(combat.expected_end_loss(state), 10)  # he takes the attack; the Toxics are ours
        state["player"]["block"] = 7
        self.assertEqual(combat.expected_end_loss(state), 3)  # our own Block does stop them
        state["player"]["pets"][0]["hp"] = 2
        self.assertEqual(combat.expected_end_loss(state), 7)  # ... and then there is none left for the attack he cannot take whole

    def test_strength_given_up_for_good_is_counted(self):
        """Seen 2026-11-21: two Friendships down on sight, Strikes hitting for 2, 7 energy and nothing to spend it on."""
        friendship = lambda i: card(i, "FRIENDSHIP", "Friendship", "Power", 1, "Lose 2 Strength. Gain [necrobinder_energy_icon.png] at the start of each turn.")
        info = parse(friendship(0))
        self.assertEqual((info.strength, info.strength_lasts), (-2, True))
        flex = parse(card(0, "FLEX", "Flex", "Skill", 0, "Gain 2 Strength. At the end of your turn, lose 2 Strength."))
        self.assertEqual(flex.strength, 2)  # (given back at the end of the turn is not a price paid)
        foes = lambda: combat.read_enemies(combat_state([], [enemy("A", 300)]))
        kw = dict(energy=3, stars=0, hp=70, max_hp=80, block=0, fight="boss")
        nothing = combat.simulate([], foes(), **kw).score
        first = combat.simulate([info], foes(), **kw).score
        second = combat.simulate([info], foes(), friends=1, **kw).score
        both = combat.simulate([info, parse(friendship(1))], foes(), **kw).score
        self.assertGreater(first, nothing)  # an energy every turn of a boss fight is worth 2 Strength
        self.assertLess(second, nothing)  # ... a second one is not
        self.assertLess(both, first)
        # In the same line the Strength is gone for the attacks after it.
        strike = parse(STRIKE(2))
        after = combat.simulate([info, strike], foes(), **kw).score - first
        before = combat.simulate([strike], foes(), **kw).score - nothing
        self.assertLess(after, before)

    def test_strength_taken_off_an_enemy_for_good_is_for_the_one_that_hits_often(self):
        fate = card(0, "SHARED_FATE", "Shared Fate", "Skill", 0, "Lose 2 Strength. Enemy loses 2 Strength. Exhaust.", "AnyEnemy")
        info = parse(fate)
        self.assertEqual((info.strength, info.enemy_strength_loss, info.enemy_strength_lasts), (-2, 2, True))
        wail = parse(card(0, "PIERCING_WAIL", "Piercing Wail", "Skill", 1, "ALL enemies lose 6 Strength this turn. Exhaust."))
        self.assertFalse(wail.enemy_strength_lasts)
        kw = dict(energy=3, stars=0, hp=70, max_hp=80, block=30, fight="boss")  # (behind Block: only what lasts is being weighed)
        gain = lambda attack: combat.simulate([info], combat.read_enemies(combat_state([], [enemy("A", 300, attack=attack)])), **kw).score - combat.simulate(
            [], combat.read_enemies(combat_state([], [enemy("A", 300, attack=attack)])), **kw).score
        self.assertGreater(gain("8x3"), 0)  # 6 off every such turn for our 2 Strength
        self.assertLess(gain("20"), 0)  # 2 off a single hit is not worth them
        self.assertGreater(gain("8x3"), gain("20"))

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
        saved, battle.RAGE_BLOCK = battle.RAGE_BLOCK, 0
        try:
            without = combat.simulate(line, self.crabs(), **kw).score
        finally:
            battle.RAGE_BLOCK = saved
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

    def test_killing_a_stock_enemy_takes_its_attack_away_for_the_turn(self):
        # Axebot on 32 about to hit for 17: the Hammer and the Crush Under kill it, and the new one only
        # Defends and Buffs this turn. That beats 10 damage and 8 Block in front of the 17.
        stock = [{"id": "STOCK_POWER", "amount": 1}]
        hammer = card(0, "HEIRLOOM_HAMMER", "Heirloom Hammer+", "Attack", 2, "Deal 22 damage.", "AnyEnemy")
        crush = card(1, "CRUSH_UNDER", "Crush Under+", "Attack", 1, "Deal 10 damage.", "AnyEnemy")
        wall = card(2, "GLITTERSTREAM", "Glitterstream", "Skill", 2, "Gain 8 Block.", "Self")
        bot = {**enemy("AXEBOT_0", 32, attack=17, status=stock), "max_hp": 87}
        plan: dict = {}
        first = combat.choose_card(combat_state([hammer, crush, wall], [bot], energy=3, hp=82, max_hp=139), plan)
        self.assertIn(first[1]["card_index"], (0, 1))
        self.assertEqual(plan["lost"], 0)

    def test_stock_is_read_from_the_state(self):
        state = {"battle": {"enemies": [{"entity_id": "AXEBOT_0", "hp": 5, "max_hp": 85, "block": 0,
                                         "status": [{"id": "STOCK_POWER", "amount": 2}], "intents": []}]},
                 "player": {}}
        (foe,) = combat.read_enemies(state)
        self.assertEqual((foe.respawns, foe.max_hp), (2, 85))


if __name__ == "__main__":
    unittest.main()


class FreeNextCard(unittest.TestCase):
    def test_the_attack_after_unrelenting_costs_nothing(self):
        from sts2bot.fight.battle import read_enemies

        unrelenting = card(0, "UNRELENTING", "Unrelenting", "Attack", 2, "Deal 14 damage. The next Attack you play costs 0 [ironclad_energy_icon.png].", "AnyEnemy")
        bludgeon = card(1, "BLUDGEON", "Bludgeon", "Attack", 3, "Deal 32 damage.", "AnyEnemy")
        state = combat_state([unrelenting, bludgeon], [enemy("A", 200, attack=0)], energy=2)
        line = [parse(unrelenting), parse(bludgeon)]
        ctx = combat._context(state)
        self.assertIsNotNone(combat.simulate(line, read_enemies(state), **ctx))
        self.assertIsNone(combat.simulate(line[::-1], read_enemies(state), **ctx))  # Bludgeon first is 3 energy

    def test_drum_of_battle_gives_no_energy_when_played(self):
        drum = card(0, "DRUM_OF_BATTLE", "Drum of Battle", "Skill", 1, "Draw 2 cards. When this card is Exhausted, gain [ironclad_energy_icon.png][ironclad_energy_icon.png].", "Self")
        self.assertEqual((parse(drum).draw, parse(drum).energy_gain), (2, 0))


class StarsInTheBank(unittest.TestCase):
    def test_more_stars_are_worth_little_once_plenty_are_held(self):
        from sts2bot.fight.battle import read_enemies

        venerate = card(0, "VENERATE", "Venerate", "Skill", 1, "Gain [star_icon.png][star_icon.png].", "Self")

        def worth(stars):
            state = combat_state([venerate], [enemy("A", 60, attack=0)], energy=1)
            state["player"]["stars"] = stars
            ctx = combat._context(state)
            return combat.simulate([parse(venerate)], read_enemies(state), **ctx).score - combat.simulate([], read_enemies(state), **ctx).score

        self.assertGreater(worth(0), 4.0)
        self.assertLess(worth(12), 1.0)
