import unittest
from unittest import mock

from sts2bot import learn
from sts2bot.cards import deck_value, parse
from sts2bot.fight import battle, combat
from sts2bot.run import potions, strategy

from ..fake_game import FakeGame, card
from ..helpers import DEFEND, STRIKE, enemy, make_bot
from . import aimed, combat_state, ctx, gain, outcome, pick, play_turn, potion, power, read_foes, worth

ICON, STAR = "[defect_energy_icon.png]", "[star_icon.png]"
BARRICADE = card(0, "BARRICADE", "Barricade", "Power", 3, "Block is not removed at the start of your turn.", "Self")
DISMANTLE = card(0, "DISMANTLE", "Dismantle", "Attack", 1, "Deal 8 damage. If the enemy is Vulnerable, hits twice.", "AnyEnemy")
HEMO = card(0, "HEMO", "Hemokinesis", "Attack", 1, "Lose 2 HP. Deal 15 damage.", "AnyEnemy")
BIG = card(0, "BIG", "Big", "Attack", 1, "Deal 14 damage.", "AnyEnemy")
ZAP = card(0, "ZAP", "Zap", "Skill", 1, "Channel 1 Lightning.", "Self")
DEADLY = card(0, "DEADLY_POISON", "Deadly Poison", "Skill", 1, "Apply 5 Poison.", "AnyEnemy")
STRENGTH = lambda slot: potion(slot, "Strength Potion", "Gain 2 Strength.")
FIRE = lambda slot: potion(slot, "Fire Potion", "Deal 20 damage.", "AnyEnemy")


def dummy(incoming=0, **kw):
    """A 200-HP enemy as the planner sees it."""
    return [combat.Enemy("A", 200, 0, False, False, incoming, **kw)]


class CardParsing(unittest.TestCase):
    READS = [  # a card, and what parse makes of its text
        (card(0, "X", "X", "Attack", 2, "Deal 5 damage 3 times. Apply 2 Vulnerable.", "AnyEnemy"), dict(damage=5, hits=3, vulnerable=2, cost=2, needs_target=True)),
        (DISMANTLE, dict(damage=8, hits=1, double_if_vulnerable=True)),
        (card(0, "DEBILITATE", "Debilitate", "Attack", 1, "Deal 10 damage. Vulnerable and Weak are twice as effective against the enemy for the next 2 turns.", "AnyEnemy"), dict(damage=10, hits=1)),
        (card(0, "TWIN_STRIKE", "Twin Strike", "Attack", 1, "Deal 5 damage twice.", "AnyEnemy"), dict(damage=5, hits=2)),  # ("twice" counts hits only when it is about the damage)
        (card(0, "FISTICUFFS", "Fisticuffs", "Attack", 1, "Deal 7 damage. Gain Block equal to damage dealt.", "AnyEnemy"), dict(damage=7, block=7)),
        ({"index": 1, "type": "Skill", "cost": "X", "description": "Gain 7 Block. Draw 2 cards."}, dict(is_x=True, block=7, draw=2)),
        (card(0, "DRUM_OF_BATTLE", "Drum of Battle", "Skill", 1, "Draw 2 cards. When this card is Exhausted, gain [ironclad_energy_icon.png][ironclad_energy_icon.png].", "Self"), dict(draw=2, energy_gain=0)),
        (card(0, "X", "X", "Skill", 2, "Apply 3 Poison to a random enemy 3 times."), dict(poison=9)),  # (the Silent's, Regent's, Necrobinder's and Defect's)
        (card(0, "X", "X", "Skill", 1, "Add 3 Shivs into your Hand. Exhaust."), dict(shivs=3)),
        (card(0, "X", "X", "Skill", 0, f"Gain 9 {STAR}. Exhaust."), dict(star_gain=9)),
        (card(0, "X", "X", "Skill", 1, f"Gain {STAR}{STAR}."), dict(star_gain=2)),
        (card(0, "X", "X", "Skill", 1, f"Gain 7 Block. Next turn, gain {ICON}."), dict(block=7, next_energy=1, energy_gain=0)),
        (card(0, "X", "X", "Skill", 1, "Gain 4 Block. Next turn, gain 4 Block."), dict(block=4, next_block=4)),
        (card(0, "X", "X", "Attack", 1, "Deal 6 damage. Channel 1 Frost.", "AnyEnemy"), dict(damage=6, channel=("FROST",))),
        (card(0, "X", "X", "Attack", 1, "Deal 5 damage for each Skill in your Hand. (Hits 0 times)", "AnyEnemy"), dict(damage=0)),
        (card(0, "X", "X", "Skill", 2, "Summon 4. Gain 7 Block."), dict(summon=4)),
        (card(0, "X", "X", "Skill", 1, "ALL enemies lose 6 Strength this turn. Exhaust."), dict(enemy_strength_loss=6)),
    ]

    def test_what_the_text_says(self):
        for c, want in self.READS:
            with self.subTest(c["description"]):
                self.assertEqual({k: getattr(parse(c), k) for k in want}, want)

    def test_intent_damage(self):
        self.assertEqual(combat.intent_damage({"type": "Attack", "label": "5x3"}), 15)
        self.assertEqual(combat.intent_damage({"type": "AttackDebuff", "label": "11"}), 11)
        self.assertEqual(combat.intent_damage({"type": "Buff", "label": "3"}), 0)

    def test_deck_value_ordering(self):
        strike = card(0, "STRIKE_R", "Strike", "Attack", 1, "Deal 6 damage.", rarity="Basic")
        good = card(0, "UPPERCUT", "Uppercut", "Attack", 2, "Deal 13 damage. Apply 1 Weak. Apply 1 Vulnerable.", rarity="Uncommon")
        self.assertGreater(deck_value(good), deck_value(strike))
        self.assertGreater(deck_value(strike), deck_value(card(0, "REGRET", "Regret", "Curse", 0, "Unplayable.")))


FIRST_MOVES = [  # (the rule, a turn, what the planner does first: a hand index, a target, the params or the whole action)
    ("takes lethal instead of blocking", combat_state([DEFEND(0), STRIKE(1), STRIKE(2)], [enemy("A_0", 12, attack=20)], hp=30), {"card_index": 1, "target": "A_0"}),
    ("blocks when the attack would kill", combat_state([STRIKE(0), DEFEND(1), DEFEND(2), DEFEND(3)], [enemy("A_0", 60, attack=14)], hp=12, max_hp=80), 1),
    ("attacks an enemy that is not attacking", combat_state([DEFEND(0), STRIKE(1)], [enemy("A_0", 60)], energy=1), 1),
    ("kills the enemy it can kill", combat_state([STRIKE(0)], [enemy("BIG_0", 50, attack=5), enemy("SMALL_0", 6, attack=10)], energy=1), "SMALL_0"),
    ("Vulnerable makes lethal reachable", combat_state([STRIKE(0), DEFEND(1)], [enemy("A_0", 9, attack=30, status=[power("VULNERABLE", 2)])], energy=1, hp=10), 0),
    ("a hit doubled on the Vulnerable: 8 * 1.5 * 2 = 24 kills it", combat_state([DISMANTLE], [enemy("PLAIN_0", 20, attack=5), enemy("VULN_0", 24, attack=5, status=[power("VULNERABLE")])], energy=1), "VULN_0"),
    ("blocks at full health when it cannot kill", combat_state([STRIKE(0), DEFEND(1)], [enemy("A_0", 40, attack=10)], energy=1), 1),
    ("no energy spent on Status cards", combat_state([card(0, "SLIMED", "Slimed", "Status", 1, "Exhaust.", "None"), STRIKE(1)], [enemy("A_0", 40)], energy=1), 1),
    ("the Strike eats the Slippery", combat_state([HEMO, STRIKE(1)], [enemy("V_0", 100, status=[power("SLIPPERY")])], energy=2), 1),
    ("never HP paid for 1 damage", combat_state([HEMO, STRIKE(1)], [enemy("V_0", 100, status=[power("SLIPPERY", 8)])], energy=1), 1),
    ("Thorns cost HP per hit", combat_state([card(0, "TWIN", "Twin Strike", "Attack", 1, "Deal 5 damage twice.", "AnyEnemy"), card(1, "BIG", "Big", "Attack", 1, "Deal 10 damage.", "AnyEnemy")],
                                            [enemy("A_0", 60, status=[power("THORNS", 5)])], energy=1), 1),
    ("an Illusion that just revives is ignored", combat_state([STRIKE(0)], [enemy("EYE_0", 6, status=[power("ILLUSION"), power("MINION")]), enemy("FOGMOG_0", 50, attack=8)], energy=1), "FOGMOG_0"),
    ("the leader's death wins with minions left", combat_state([STRIKE(0), DEFEND(1)], [enemy("MINION_0", 20, attack=30, status=[power("MINION")]), enemy("LEADER_0", 6)], energy=1, hp=10), ("play_card", {"card_index": 0, "target": "LEADER_0"})),
    ("races to the stun threshold: 6 damage stuns it, 5 Block would not save us", combat_state([STRIKE(0), DEFEND(1)], [enemy("BEAST_0", 155, attack=24, status=[power("PLOW", 150)])], energy=1, hp=19), 0),
    ("Block cards are useless while Block is locked", combat_state([STRIKE(0), DEFEND(1)], [enemy("A_0", 40, attack=10)], energy=1, status=[power("NO_BLOCK")]), 0),
    ("a Slow enemy gets the Skills first", combat_state([BIG, STRIKE(1), DEFEND(2)], [enemy("EFFIGY_0", 120, attack=10, status=[power("SLOW", 0)])], energy=3), 2),
    ("... and the big hit last", combat_state([BIG, STRIKE(1)], [enemy("EFFIGY_0", 120, status=[power("SLOW", 0)])], energy=2), 1),
    ("the Slow order that kills: 6x3 then 10 (+10%) is 29, the other way 28",
     combat_state([card(0, "PHOTON_CUT", "Photon Cut", "Attack", 1, "Deal 10 damage.", "AnyEnemy"), card(1, "CELESTIAL_MIGHT", "Celestial Might", "Attack", 2, "Deal 6 damage 3 times.", "AnyEnemy"), DEFEND(2)],
                  [enemy("E", 29, attack=23, status=[power("SLOW", 0)])], energy=3, hp=40), ("play_card", {"card_index": 1, "target": "E"})),
    ("Weak goes on the attacker", combat_state([card(0, "W", "Weaken", "Skill", 1, "Apply 2 Weak.", "AnyEnemy")], [enemy("IDLE_0", 30), enemy("HITTER_0", 60, attack=20)], energy=1, hp=30), ("play_card", {"card_index": 0, "target": "HITTER_0"})),
    ("Debilitate doubles Vulnerable", combat_state([STRIKE(0), DEFEND(1)], [enemy("A", 12, attack=5, status=[power("VULNERABLE"), power("DEBILITATE", 2)])], energy=1), ("play_card", {"card_index": 0, "target": "A"})),
    ("Toxic is played: 5 saved beats 4 blocked", combat_state([card(0, "TOXIC", "Toxic", "Status", 1, "At the end of your turn, if this is in your Hand, take 5 damage. Exhaust.", "Self"), DEFEND(1)],
                                                               [enemy("A", 90, attack=4)], energy=1), ("play_card", {"card_index": 0})),
    ("a Burn's 2 and the 5 coming would kill", combat_state([{**card(0, "BURN", "Burn", "Status", 0, "Unplayable. At the end of your turn, if this is in your Hand, take 2 damage.", "Self"), "can_play": False},
                                                             STRIKE(1), DEFEND(2)], [enemy("A", 90, attack=5)], energy=1, hp=6), ("play_card", {"card_index": 2})),
    ("Lightning orbs kill a lone enemy before it acts", combat_state([DEFEND(0), DEFEND(1)], [enemy("A", 5, attack=28)], energy=2, hp=20, orbs=[{"id": "LIGHTNING_ORB", "passive_val": 3, "evoke_val": 8}] * 2, orb_slots=3), None),
    ("Enrage keeps the Skill back", combat_state([ZAP, STRIKE(1)], [enemy("A", 100, attack=10, status=[power("ENRAGE", 2)])], energy=1, kind="boss"), 1),
    ("... for when it is needed", combat_state([ZAP], [enemy("A", 100, attack=10, status=[power("ENRAGE", 2)])], energy=1, kind="boss"), None),
    ("Poison does not kill through Intangible", combat_state([DEFEND(0)], [enemy("FYSH", 6, attack=9, status=[power("POISON", 8), power("INTANGIBLE")])], energy=1, hp=29), ("play_card", {"card_index": 0})),
    ("Poison that covers its HP stops the attack", combat_state([DEADLY, DEFEND(1)], [enemy("A", 9, attack=20, status=[power("POISON", 4)])], energy=1, hp=30), ("play_card", {"card_index": 0, "target": "A"})),
]


class CombatPlanning(unittest.TestCase):
    def test_first_moves(self):
        for why, state, want in FIRST_MOVES:
            with self.subTest(why):
                act = combat.choose_card(state)
                if isinstance(want, dict):
                    act = act[1]
                elif isinstance(want, (int, str)):
                    act = act[1]["card_index" if isinstance(want, int) else "target"]
                self.assertEqual(act, want)

    def test_a_slippery_stack_taken_off_is_worth_a_hit(self):
        # Vantom on 8 Slippery: each stack is a later hit that lands in full, so the hits go before the Defends (fights still on Slippery at round 5 were lost 4 times in 8) ...
        twin = card(0, "TWIN_STRIKE", "Twin Strike", "Attack", 1, "Deal 5 damage twice.", "AnyEnemy")
        vantom = lambda attack: enemy("VANTOM_0", 173, attack=attack, status=[power("SLIPPERY", 8)])
        s = combat_state([twin, DEFEND(1), STRIKE(2), DEFEND(3), STRIKE(4)], [vantom(7)], energy=3, kind="boss")
        self.assertEqual(sorted(c["index"] for c in play_turn(s)), [0, 2, 4])
        # ... but not in front of the big hit when already hurt: there the Block comes first.
        self.assertIn(pick(combat_state([twin, DEFEND(1), DEFEND(3)], [vantom(26)], energy=2, hp=30, kind="boss")), (1, 3))

    def test_never_plays_a_card_that_can_kill_us(self):
        gambit = card(0, "THE_GAMBIT", "The Gambit", "Skill", 0, "Gain 50 Block. If you take unblocked attack damage this combat, die.", "Self")
        self.assertTrue(parse(gambit).deadly)
        self.assertLess(deck_value(gambit), 0)
        self.assertIsNone(combat.choose_card(combat_state([gambit], [enemy("A_0", 60, attack=20)], energy=3)))

    def test_card_limit_spends_plays_on_the_best_cards(self):
        hand = [card(0, "BT", "Battle Trance", "Skill", 0, "Draw 3 cards.", "Self"), card(1, "BIG", "Big", "Attack", 1, "Deal 14 damage.", "AnyEnemy")]
        s = combat_state(hand, [enemy("A_0", 60)], energy=3, status=[power("RINGING")])
        self.assertEqual(pick(s), 1)
        s["_plays_this_turn"] = 1
        self.assertIsNone(combat.choose_card(s))

    def test_blocks_a_hit_that_would_take_most_of_what_is_left(self):
        aoe = card(0, "BREAK", "Breakthrough", "Attack", 1, "Lose 1 HP. Deal 9 damage to ALL enemies.", "AllEnemies")
        bash = card(1, "BASH", "Bash", "Attack", 2, "Deal 8 damage. Apply 2 Vulnerable.", "AnyEnemy")
        foes = [enemy("F_0", 39, attack=9, status=[power("MINION")]), enemy("F_1", 38, attack=8, status=[power("MINION")]), enemy("PRIEST_0", 98, attack=10)]
        s = combat_state([aoe, bash, DEFEND(2), DEFEND(3), STRIKE(4)], foes, energy=3, hp=37, kind="boss")
        self.assertEqual([c["name"] for c in play_turn(s, block=True)].count("Defend"), 2)

    def test_known_cards_are_nudged(self):
        shrug = card(0, "SHRUG_IT_OFF", "Shrug It Off", "Skill", 1, "Gain 8 Block. Draw 1 card.", "Self")
        plain = card(0, "SOMETHING_NEW", "Something New", "Skill", 1, "Gain 8 Block. Draw 1 card.", "Self")
        with mock.patch.dict(learn.KNOB, {"crowd": 0.0}):  # our own table (other players' runs rate Barricade high)
            self.assertGreater(deck_value(shrug), deck_value(plain))
            self.assertLess(deck_value(BARRICADE), deck_value(plain))
            self.assertGreater(deck_value({**shrug, "name": "Shrug It Off+", "id": None}), deck_value(plain))

    def test_an_enemy_that_grows_every_turn_is_raced(self):
        sculptor = lambda powers: enemy("DEVOTED_SCULPTOR_0", 150, attack=12, status=powers)
        first = lambda powers, hp=70: pick(combat_state([STRIKE(0), STRIKE(1), DEFEND(2), DEFEND(3)], [sculptor(powers)], energy=2, hp=hp, max_hp=90))
        self.assertIn(first([]), (2, 3))  # an enemy like any other: 12 coming, two Defends
        self.assertIn(first([power("RITUAL", 9)]), (0, 1))  # 9 Strength a turn: every turn it lives is dearer
        self.assertIn(first([power("RITUAL", 9)], hp=14), (2, 3))  # ... but not into the grave
        self.assertEqual(read_foes(sculptor([power("STRENGTH", 6)]))[0].grows, 6.0)
        self.assertEqual(combat.read_enemies(combat_state([], [sculptor([power("STRENGTH", 6)])], round_no=4))[0].grows, 2.0)  # 6 Strength in three turns

    def test_a_big_attack_is_not_spent_on_a_minion_that_is_not_attacking(self):
        squeeze = card(0, "SQUEEZE", "Squeeze", "Attack", 1, "Osty deals 35 damage.", "AnyEnemy")
        egg = {**enemy("TOUGH_EGG_0", 17), "status": [power("MINION")], "intents": [{"type": "Summon", "label": ""}]}
        copter = enemy("OVICOPTER_0", 105, attack=16)
        self.assertEqual(aimed(combat_state([squeeze], [egg, copter], hp=69, pets=[{"name": "Osty", "hp": 3, "alive": True}])), "OVICOPTER_0")
        # A small attack (an 18-damage Strike) that just kills the egg is still a kill worth taking, and so is any kill of a minion about to hit us.
        self.assertEqual(combat._pick_target(read_foes(egg, copter), 18, 1).entity_id, "TOUGH_EGG_0")
        self.assertEqual(combat._pick_target(read_foes({**egg, "intents": [{"type": "Attack", "label": "6"}]}, copter), 35, 1).entity_id, "TOUGH_EGG_0")

    def test_the_enemy_that_does_most_for_its_health_is_hit_first(self):
        beetle = {**enemy("SLUMBERING_BEETLE_0", 83), "intents": [{"type": "Attack", "label": "16"}, {"type": "Buff", "label": ""}]}
        state = combat_state([STRIKE(0)], [enemy("BOWLBUG_ROCK_0", 46, attack=15), beetle], hp=80)
        self.assertEqual(aimed(state), "BOWLBUG_ROCK_0")  # nothing known of them: the one that dies soonest
        # Four rounds in: the bug has hit every other turn, the beetle slept for three and now grows every turn.
        seen = {**state, "_threat": {"BOWLBUG_ROCK_0": 7.5, "SLUMBERING_BEETLE_0": 4.0}}
        self.assertEqual(aimed(seen), "SLUMBERING_BEETLE_0")
        self.assertEqual(combat.read_enemies(seen)[1].threat, 24.0)
        # ... but a bug nearly dead is finished first, and two that hit alike are taken lowest first.
        seen["battle"]["enemies"][0]["hp"] = 12
        self.assertEqual(aimed(seen), "BOWLBUG_ROCK_0")
        twins = combat_state([STRIKE(0)], [enemy("A", 50, attack=10), enemy("B", 40, attack=10)], hp=80)
        self.assertEqual(aimed({**twins, "_threat": {"A": 10.0, "B": 10.0}}), "B")
        # Boss and elite fights keep their own order (the crabs, minions and their leaders).
        boss = {**seen, "state_type": "boss"}
        boss["battle"]["enemies"][0]["hp"] = 46
        self.assertEqual(aimed(boss), "BOWLBUG_ROCK_0")

    def test_energy_written_as_a_number_and_an_icon_is_read(self):
        boost = card(0, "SUPERCRITICAL", "Supercritical", "Skill", 0, f"Gain 4{ICON}. Exhaust.", "Self")
        self.assertEqual(parse(boost).energy_gain, 4)
        self.assertEqual(parse(card(0, "TURBO", "TURBO", "Skill", 0, f"Gain {ICON}{ICON}. Add a Void into your Discard Pile.", "Self")).energy_gain, 2)
        # With it, the 3-cost attack that the energy in hand cannot pay for is played.
        big = {**card(1, "METEOR", "Meteor", "Attack", 3, "Deal 30 damage.", "AnyEnemy"), "can_play": False, "unplayable_reason": "EnergyCostTooHigh"}
        self.assertEqual(pick(combat_state([boost, big], [enemy("JAW_WORM_0", 80)], energy=1)), 0)

    def test_borrowed_time_makes_the_cards_after_it_dearer(self):
        icon = "[necrobinder_energy_icon.png]"
        time = parse(card(0, "BORROWED_TIME", "Borrowed Time", "Skill", 1, f"Gain 4{icon}. Cards cost an additional {icon} this turn.", "Self"))
        strikes, kw = [parse(STRIKE(i)) for i in range(1, 5)], ctx(hp=60, max_hp=60)
        # 3 - 1 + 4 = 6 energy at 2 a Strike: three of them, not four.
        self.assertIsNotNone(combat.simulate([time, *strikes[:3]], dummy(), **kw))
        self.assertIsNone(combat.simulate([time, *strikes], dummy(), **kw))
        self.assertIsNotNone(combat.simulate(strikes[:3], dummy(), **kw))

    def test_cards_that_pay_for_or_repeat_the_next_one(self):
        kw = ctx(hp=60, max_hp=60)
        value = lambda line: gain(line, dummy(), **kw)
        skill = lambda cid, name, text, cost=1: parse(card(0, cid, name, "Skill", cost, text, "Self"))
        big, strike, guard = parse(card(1, "METEOR", "Meteor", "Attack", 3, "Deal 30 damage.", "AnyEnemy")), parse(STRIKE(2)), parse(DEFEND(3))
        punch = skill("ONE_TWO_PUNCH", "One-Two Punch", "This turn, your next Attack is played an extra time.")
        self.assertEqual(value([punch, strike, strike]), 18.0)  # One-Two Punch: the next Attack twice, and only the next one
        self.assertEqual(value([punch, guard, strike]), 12.0)  # (a Skill in between does not use it up)
        burst = skill("BURST", "Burst", "This turn, your next Skill is played an extra time.")
        self.assertEqual(combat.simulate([burst, guard], dummy(20, incoming_hits=1), **kw).lost, 10)  # Burst: the next Skill's Block twice
        double = skill("DOUBLE_ENERGY", "Double Energy", "Double your Energy. Exhaust.")
        self.assertIsNone(combat.simulate([big, strike], dummy(), **kw))
        self.assertEqual(value([double, big, strike]), 36.0)  # Double Energy: 3 - 1 = 2, doubled, pays for the 3-cost card with one to spare
        bullet = skill("BULLET_TIME", "Bullet Time", "You cannot draw additional cards this turn. ALL cards in your Hand are free to play this turn.", 3)
        self.assertEqual(value([bullet, big, strike, strike]), 42.0)  # Bullet Time: what is left of the hand costs nothing

    def test_the_attack_after_unrelenting_costs_nothing(self):
        unrelenting = card(0, "UNRELENTING", "Unrelenting", "Attack", 2, "Deal 14 damage. The next Attack you play costs 0 [ironclad_energy_icon.png].", "AnyEnemy")
        bludgeon = card(1, "BLUDGEON", "Bludgeon", "Attack", 3, "Deal 32 damage.", "AnyEnemy")
        state = combat_state([unrelenting, bludgeon], [enemy("A", 200, attack=0)], energy=2)
        self.assertIsNotNone(outcome(state, [unrelenting, bludgeon]))
        self.assertIsNone(outcome(state, [bludgeon, unrelenting]))  # Bludgeon first is 3 energy

    def test_a_power_other_players_win_with_is_worth_playing(self):
        self.assertGreater(battle._power_worth(parse(BARRICADE)), 12.0)  # (our own table says 4)
        self.assertEqual(battle._power_worth(parse(card(0, "NEW_POWER", "New Power", "Power", 2, "Something nobody has rated.", "Self"))), 9.0)
        # A boss that is only buffing itself: the turn goes to Barricade, not to three Strikes.
        quiet = combat_state([BARRICADE, STRIKE(1), STRIKE(2), STRIKE(3)], [enemy("QUEEN_0", 400)], hp=80, kind="boss")
        self.assertEqual(pick(quiet), 0)
        # An ordinary fight is over before it pays: there the three Strikes go first, as they always did.
        self.assertEqual(battle._power_worth(parse(BARRICADE), long_fight=False), 4.0)
        self.assertNotEqual(pick({**quiet, "state_type": "monster"}), 0)

    def test_block_left_over_counts_once_it_stays(self):
        guard = card(0, "IMPERVIOUS", "Impervious", "Skill", 2, "Gain 30 Block. Exhaust.", "Self")
        state = combat_state([guard], [enemy("A", 200, attack=10)], energy=2, hp=40)
        plain = outcome(state, [guard]).score
        state["player"]["status"] = [power("BARRICADE")]
        self.assertGreater(outcome(state, [guard]).score, plain + 15.0)  # 20 Block for next turn

    def test_an_ethereal_power_is_played_while_it_can_be(self):
        echo = card(0, "ECHO_FORM", "Echo Form", "Power", 3, "Ethereal. The first card you play each turn is played an extra time.", "Self")
        self.assertTrue(parse(echo).ethereal)
        boss = lambda hp: combat_state([echo, STRIKE(1), STRIKE(2), DEFEND(3)], [enemy("AEONGLASS_0", 500, attack=22)], hp=hp, max_hp=80, kind="boss")
        self.assertEqual(pick(boss(75)), 0)  # 22 damage taken for the rest of the fight doubled
        self.assertNotEqual(pick(boss(12)), 0)

    def test_a_heal_is_worth_the_hp_it_gives_back(self):
        heal = parse(card(0, "NOT_YET", "Not Yet", "Skill", 2, "Heal 10 HP. Exhaust.", "Self"))
        self.assertEqual(heal.heal, 10)
        foe, hurt, full = dummy(), ctx(hp=20, max_hp=60), ctx(hp=60, max_hp=60)
        score = lambda line, **kw: combat.simulate(line, foe, **kw).score
        self.assertGreater(score([heal], **hurt), score([], **hurt) + 20)
        self.assertLess(score([heal], **full), score([], **full) + 1)
        foe = dummy(25, incoming_hits=1)  # ... and it is there for the blow that comes this turn
        self.assertLess(score([heal], **hurt), 0)  # 30 HP against 25: hurt badly
        self.assertGreater(score([heal], **hurt), score([], **hurt) + 100_000)  # ... not dead

    def test_plating_counts_as_block(self):
        s = combat_state([STRIKE(0), DEFEND(1)], [enemy("A_0", 40, attack=5)], energy=1)
        self.assertEqual(combat.expected_loss(s), 5)
        s["player"]["status"] = [power("PLATING", 5)]
        self.assertEqual(combat.expected_loss(s), 0)
        self.assertEqual(pick(s), 0)  # nothing to block: attack

    def test_respects_energy_and_unplayable_cards(self):
        s = combat_state([card(0, "BIG", "Big", "Attack", 3, "Deal 30 damage.", "AnyEnemy"), {**STRIKE(1), "can_play": False}], [enemy("A_0", 60)], energy=2)
        self.assertIsNone(combat.choose_card(s))
        self.assertEqual(combat.actions(s), [("end_turn", {})])

    def test_potions_only_when_it_matters(self):
        fire = {"slot": 1, "id": "FIRE_POTION", "can_use_in_combat": True, "target_type": "AnyEnemy"}
        self.assertEqual(combat.potion_actions(combat_state([STRIKE(0)], [enemy("A_0", 40, attack=5)], potions=[fire])), [])
        self.assertEqual(combat.potion_actions(combat_state([STRIKE(0)], [enemy("A_0", 40, attack=5)], potions=[fire], kind="boss")), [("use_potion", {"slot": 1, "target": "A_0"})])


FOUL = potion(0, "Foul Potion", "Deal 12 damage to ALL players and enemies. Can be thrown at the Merchant for 100 Gold instead.")
BLOOD = potion(0, "Blood Potion", "Heal for 20% of your Max HP.")
FIRES = [FIRE(i) for i in range(3)]
VULN = potion(0, "Vulnerable Potion", "Apply 3 Vulnerable.", "AnyEnemy")
AEONGLASS = dict(foe="AEONGLASS_0", foe_hp=500, attack=22, floor=48)
BELTS = [  # (when, the belt, Potions.boss keywords, the slots drunk)
    ("Entropic Brew is drunk at once", [potion(0, "Entropic Brew", "Fill all your empty potion slots with random potions."), STRENGTH(1)], dict(floor=5, kind="monster"), [0]),
    ("Fruit Juice in the first fight, with nothing wrong and room on the belt", [potion(0, "Fruit Juice", "Gain 5 Max HP."), STRENGTH(1)], dict(floor=5, kind="monster"), [0]),
    ("... where the Strength Potion waits", [STRENGTH(1)], dict(floor=5, kind="monster"), []),
    ("a potion that hits us too waits for Block", [FOUL, FIRE(1)], {}, [1]),
    ("... the Block that is up is for the 10 coming", [FOUL, FIRE(1)], dict(block=12), [1]),
    ("... and there is enough of it", [FOUL, FIRE(1)], dict(block=22), [0, 1]),
    ("healing waits until it is needed", [BLOOD], {}, []),
    ("... and then it goes", [BLOOD], dict(hp=30), [0]),
    ("a full belt has one to spare", FIRES, dict(floor=20, kind="monster"), [0]),
    ("... but not two rooms from the boss", FIRES, dict(floor=31, kind="monster"), []),
    ("... nor in a fight that is over anyway", FIRES, dict(floor=33, foe_hp=20), []),
    ("an elite gets the spare one", FIRES, dict(floor=22, kind="elite"), [0]),
    ("... nothing of a belt with room", FIRES[:2], dict(floor=22, kind="elite"), []),
    ("... nor with the boss two rooms on", FIRES, dict(floor=31, kind="elite"), []),
    ("... unless the fight has gone wrong", FIRES[:2], dict(floor=24, kind="elite", hp=35), [0, 1]),
    ("the boss gets all of them", FIRES[:2], dict(floor=33), [0, 1]),
    ("the Test Subject's first form: our Strength lasts, its Vulnerable would not", [VULN, STRENGTH(1)], dict(foe="TEST_SUBJECT_0", foe_hp=100, floor=48, powers=[power("ADAPTABLE"), power("ENRAGE", 2)]), [1]),
    ("... its second form", [VULN, STRENGTH(1)], dict(foe="TEST_SUBJECT_0", foe_hp=100, floor=48, powers=[power("ADAPTABLE"), power("PAINFUL_STABS")]), [0, 1]),
    ("... Intangible: 20 damage would take 1 HP", [FIRE(0)], dict(foe="TEST_SUBJECT_0", floor=3, powers=[power("INTANGIBLE")]), []),
    ("a debuff does not go into Artifact", [potion(0, "Weak Potion", "Apply 3 Weak.", "AnyEnemy"), STRENGTH(1)], dict(**AEONGLASS, powers=[power("ARTIFACT", 3)]), [1]),
    ("... without it, it does", [potion(0, "Weak Potion", "Apply 3 Weak.", "AnyEnemy"), STRENGTH(1)], dict(**AEONGLASS), [0, 1]),
    ("... and about to die nothing is being saved", [potion(0, "Weak Potion", "Apply 3 Weak.", "AnyEnemy"), STRENGTH(1)], dict(**AEONGLASS, powers=[power("ARTIFACT", 3)], hp=20), [0, 1]),
]


class Potions(unittest.TestCase):
    def boss(self, belt, hp=75, block=0, foe_hp=300, floor=33, kind="boss", foe="A", attack=10, powers=()):
        state = combat_state([STRIKE(0)], [enemy(foe, foe_hp, attack=attack, status=powers)], hp=hp, max_hp=75, block=block, kind=kind, potions=belt, floor=floor)
        return [a[1]["slot"] for a in combat.potion_actions(state)]

    def test_which_potions_are_drunk(self):
        for why, belt, kw, slots in BELTS:
            with self.subTest(why):
                self.assertEqual(self.boss(belt, **kw), slots)

    def test_a_belt_that_refills_every_fight_is_emptied_every_fight(self):
        state = combat_state([STRIKE(0)], [enemy("A", 60, attack=10)], hp=75, max_hp=75, potions=[STRENGTH(0), FIRE(1)], floor=40, max_potion_slots=3)
        self.assertEqual(combat.potion_actions(state), [])  # an ordinary fight, nothing wrong
        state["player"]["relics"] = [{"id": "DELICATE_FROND", "description": "At the start of each combat, fill all empty potion slots with random Potions."}]
        self.assertEqual(sorted(a[1]["slot"] for a in combat.potion_actions(state)), [0, 1])

    def test_a_big_belt_keeps_three_for_the_boss_and_drinks_the_rest_on_the_way(self):
        # Six slots and five held was never "full", and a Regent lost 37 HP in two fights holding all five. The two a boss needs least go on the way; near the boss none.
        belt = [STRENGTH(0), potion(1, "Swift Potion", "Draw 3 cards."), FIRE(2), potion(3, "Attack Potion", "Add 1 of 3 random Attack cards into your Hand."), potion(4, "Focus Potion", "Gain 2 Focus.")]
        state = combat_state([STRIKE(0)], [enemy("A", 60, attack=10)], hp=60, max_hp=75, potions=belt, floor=21, max_potion_slots=6)
        self.assertEqual(sorted(a[1]["slot"] for a in combat.potion_actions(state)), [1, 3])
        state["run"]["floor"] = 30  # three floors before the boss: all five are for it
        self.assertEqual(combat.potion_actions(state), [])

    def test_the_potion_drunk_on_the_way_is_the_one_a_boss_needs_least(self):
        # A full belt cannot take the next potion, so one goes in an ordinary fight: not the Strength Potion.
        strength = STRENGTH(0)
        state = combat_state([STRIKE(0)], [enemy("A", 60, attack=10)], hp=75, max_hp=75, potions=[strength, potion(1, "Swift Potion", "Draw 3 cards.")], floor=22, max_potion_slots=2)
        self.assertEqual([a[1]["slot"] for a in combat.potion_actions(state)], [1])
        # ... and none at all once the boss is near (nine floors in act 2): a slot emptied there often stays empty.
        state["run"]["floor"] = 24
        self.assertEqual(combat.potion_actions(state), [])
        state["run"]["floor"] = 23
        self.assertEqual(len(combat.potion_actions(state)), 1)
        self.assertGreater(potions.boss_worth(strength), potions.boss_worth(potion(0, "Fire Potion", "Deal 20 damage.")))
        self.assertEqual(potions.boss_worth({"potion_name": "Odd Vial", "potion_description": "Deal 31 damage."}), 31.0)  # (unlisted: from its text)
        self.assertGreater(potions.boss_worth({"name": "Odd Tonic", "description": "Gain 3 Strength."}), 20.0)

    def test_no_potion_is_poured_out_at_a_sleeping_boss(self):
        energy = potion(0, "Energy Potion", "Gain [defect_energy_icon.png][defect_energy_icon.png].")
        sleeper = enemy("LAGAVULIN_MATRIARCH_0", 222, status=[power("ASLEEP", 3)])
        state = combat_state([STRIKE(i) for i in range(5)], [sleeper], kind="boss", potions=[energy, STRENGTH(1)])
        self.assertEqual(combat.potion_actions(state), [])
        state["battle"]["enemies"][0]["status"][0]["amount"] = 1  # the turn before she wakes: now they count
        plan: dict = {}
        combat.choose_card(state, plan)
        self.assertEqual(sorted(a[1]["slot"] for a in combat.potion_actions(state, plan["lost"], plan["line"])), [0, 1])

    def test_an_energy_potion_waits_for_a_hand_that_can_spend_it(self):
        energy = potion(0, "Energy Potion", "Gain [ironclad_energy_icon.png][ironclad_energy_icon.png].")
        first = lambda hand, foe=enemy("A", 300): combat.actions(combat_state(hand, [foe], kind="boss", potions=[energy]))[0]
        self.assertEqual(first([STRIKE(0), STRIKE(1), DEFEND(2), DEFEND(3), DEFEND(4)])[0], "play_card")  # (a boss that only buffs: no use for the Defends)
        big = card(3, "BLUDGEON", "Bludgeon", "Attack", 3, "Deal 32 damage.", "AnyEnemy")
        self.assertEqual(first([STRIKE(0), STRIKE(1), DEFEND(2), big]), ("use_potion", {"slot": 0}))  # ... an attack three energy cannot pay for: drunk
        self.assertEqual(first([DEFEND(i) for i in range(5)], enemy("A", 300, attack=30)), ("use_potion", {"slot": 0}))  # ... and for Block when the plan still loses HP

    def test_no_potion_when_the_hand_already_ends_the_danger(self):
        belt = [STRENGTH(0), potion(1, "Foul Potion", "Deal 12 damage to ALL players and enemies.")]
        drunk = lambda hand, foe, **kw: [a for a in combat.actions(combat_state(hand, [foe], max_hp=90, kind="monster", potions=belt, **kw)) if a[0] == "use_potion"]
        self.assertEqual(drunk([STRIKE(0), STRIKE(1)], enemy("A", 12, attack=20), hp=31, floor=30), [])  # two Strikes kill it: nothing is coming
        self.assertEqual(drunk([STRIKE(0)], enemy("A", 60, attack=12), energy=0, hp=20, block=17, floor=24), [])  # out of energy behind enough Block: nothing is coming either
        self.assertTrue(drunk([STRIKE(0), STRIKE(1)], enemy("A", 60, attack=20), hp=31, floor=30))  # 20 of 31 HP is coming and the hand cannot stop it

    def test_a_potion_waits_for_the_moment_it_does_something(self):
        def used(drink, hand, line=None, block=0, attack=30, discard=0):
            state = combat_state(hand, [enemy("A", 300, attack=attack)], hp=75, max_hp=75, block=block, kind="boss", potions=[drink], floor=33, discard_pile_count=discard)
            return bool(combat.potion_actions(state, None, line))
        # "Triple your Block": 11 of 14 tripled nothing. It goes once the turn's Block is up and is still short.
        fortifier = potion(0, "Fortifier", "Triple your Block.")
        self.assertFalse(used(fortifier, [DEFEND(0)], line=[parse(DEFEND(0))]))
        self.assertFalse(used(fortifier, [], line=[], block=0))
        self.assertTrue(used(fortifier, [], line=[], block=10))
        self.assertFalse(used(fortifier, [], line=[], block=10, attack=8))
        # "Put a card from your Discard Pile into your Hand": not on round 1, when there is none.
        memories = potion(0, "Liquid Memories", "Put a card from your Discard Pile into your Hand. It's free to play this turn.")
        self.assertFalse(used(memories, [STRIKE(0)]))
        self.assertTrue(used(memories, [STRIKE(0)], discard=6))
        # Strength for one turn is for a turn of attacks (it was kept for the turns we Block).
        flex = potion(0, "Flex Potion", "Gain 5 Strength. At the end of your turn, lose 5 Strength.")
        self.assertFalse(used(flex, [STRIKE(0), DEFEND(1), DEFEND(2)], line=[parse(DEFEND(1)), parse(DEFEND(2))]))
        self.assertTrue(used(flex, [STRIKE(0), STRIKE(1), STRIKE(2)], line=[parse(STRIKE(i)) for i in range(3)]))
        # "Exhaust your Hand. Draw 10 cards": for a hand of dead cards, not for the opening one.
        glow = potion(0, "Glowwater Potion", "Exhaust your Hand. Draw 10 cards.")
        wound = lambda i: card(i, "WOUND", "Wound", "Status", 0, "Unplayable.")
        self.assertFalse(used(glow, [STRIKE(0), DEFEND(1), STRIKE(2)]))
        self.assertTrue(used(glow, [STRIKE(0), wound(1), wound(2)]))
        # Snecko Oil is drunk like any other (the costs it rolls are in the state).
        self.assertTrue(used(potion(0, "Snecko Oil", "Draw 7 cards. Randomize the cost of cards in your Hand this turn."), [STRIKE(0)]))
        # "Your next card is played an extra time": kept for a card worth playing twice, and that card is the one played next.
        twice = potion(0, "Duplicator", "This turn, your next card is played an extra time.")
        heavy = card(1, "BLUDGEON", "Bludgeon", "Attack", 2, "Deal 14 damage.", "AnyEnemy")
        self.assertFalse(used(twice, [STRIKE(0)], line=[parse(STRIKE(0))]))
        self.assertTrue(used(twice, [STRIKE(0), heavy], line=[parse(STRIKE(0)), parse(heavy)]))
        self.assertEqual(pick(combat_state([STRIKE(0), heavy], [enemy("A", 300)], energy=3, kind="boss", status=[power("DUPLICATION")])), 1)


class PowersThePlannerKnows(unittest.TestCase):
    def test_vigor_only_helps_the_first_attack(self):
        # With 8 Vigor each Strike reads "Deal 14 damage", but only the first one gets it: 14 + 6 + 6 does not kill 30 HP.
        strike = lambda i: card(i, "STRIKE_R", "Strike", "Attack", 1, "Deal 14 damage.", "AnyEnemy")
        state = combat_state([strike(0), strike(1), strike(2), DEFEND(3)], [enemy("A", 30, attack=30)], hp=20, status=[power("VIGOR", 8)])
        self.assertLess(outcome(state, state["player"]["hand"][:3]).score, 0)  # the enemy lives and we do not

    def test_tender_weakens_every_later_card(self):
        state = combat_state([DEFEND(0), DEFEND(1), DEFEND(2)], [enemy("A", 90, attack=14)], hp=10)
        plain = outcome(state, state["player"]["hand"])
        state["player"]["status"] = [power("TENDER", 0)]
        self.assertLess(outcome(state, state["player"]["hand"]).score, plain.score)  # 5 + 4 + 3 Block, not 15

    def test_stars_made_in_a_line_pay_for_a_later_card(self):
        star = card(0, "FALLING_STAR", "Falling Star", "Attack", 0, "Deal 8 damage. Apply 1 Weak. Apply 1 Vulnerable.", "AnyEnemy")
        star.update(star_cost=2, can_play=False, unplayable_reason="StarCostTooHigh")
        venerate = card(1, "VENERATE", "Venerate", "Skill", 1, f"Gain {STAR}{STAR}.", "Self")
        state = combat_state([star, venerate, STRIKE(2), STRIKE(3)], [enemy("A", 28)], energy=2, stars=1)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 1}))
        star["unplayable_reason"] = "BlockedByHook, StarCostTooHigh"  # ... but a card that is unplayable for another reason stays out
        self.assertNotEqual(combat.choose_card(state), ("play_card", {"card_index": 1}))

    def test_withering_presence_makes_small_cards_not_worth_playing(self):
        poke = card(0, "POKE", "Poke", "Attack", 0, "Deal 3 damage.", "AnyEnemy")
        self.assertIsNone(combat.choose_card(combat_state([poke], [enemy("A", 400, status=[power("WITHERING_PRESENCE", 4)])], kind="boss")))
        self.assertIsNotNone(combat.choose_card(combat_state([poke, STRIKE(1)], [enemy("A", 400)], kind="boss")))

    def test_crabs_are_brought_down_together(self):
        crabs = lambda crusher, rocket: [enemy("CRUSHER", crusher, attack=6, status=[power("CRAB_RAGE")]), enemy("ROCKET", rocket, attack=6, status=[power("CRAB_RAGE")])]
        # Killing the Crusher now would enrage a Rocket with 120 HP left: hit the Rocket. Both within reach: take them together.
        self.assertEqual(aimed(combat_state([STRIKE(0)], crabs(6, 120), energy=1, kind="boss")), "ROCKET")
        self.assertIsNotNone(combat.choose_card(combat_state([STRIKE(0), STRIKE(1)], crabs(6, 6), energy=2, kind="boss")))

    def test_drawing_is_worth_what_is_left_to_draw(self):
        state = combat_state([card(0, "SKIM", "Skim", "Skill", 1, "Draw 3 cards.", "Self"), STRIKE(1)], [enemy("A", 200)], energy=1)
        state["player"]["draw_pile"] = [{"name": "Wound", "cost": "0", "description": "Unplayable."}] * 6
        self.assertEqual(pick(state), 1)  # nothing worth drawing: Strike
        state["player"]["draw_pile"] = [{"name": "Claw", "cost": "0", "description": "Deal 9 damage."}] * 6
        self.assertEqual(pick(state), 0)  # three free attacks beat one Strike

    def test_orichalcum_and_regen_are_counted(self):
        state = combat_state([], [enemy("A", 100, attack=18)], hp=60, max_hp=80)
        self.assertEqual(combat.expected_end_loss(state), 18)
        state["player"]["relics"] = [{"id": "ORICHALCUM", "description": "If you end your turn without Block, gain 6 Block."}]
        self.assertEqual(combat.expected_end_loss(state), 12)
        state["player"]["status"] = [power("REGEN", 5)]
        self.assertEqual(combat.expected_end_loss(state), 7)
        state["player"]["block"] = 3  # some Block already: the relic stays out of it
        self.assertEqual(combat.expected_end_loss(state), 10)

    def test_deck_is_read_at_the_start_of_a_fight_only(self):
        bot = make_bot(FakeGame())
        state = combat_state([STRIKE(0), DEFEND(1)], [enemy("A", 30, attack=5)], draw_pile_count=2, draw_pile=[{"name": "Bash", "cost": "2"}, {"name": "Strike", "cost": "1"}])
        bot._combat(state)
        self.assertEqual(bot.mem.deck_size, 4)
        # Later in the fight the piles hold Wounds and other things that are not in the deck.
        state["battle"]["round"] = 3
        state["player"].update(discard_pile_count=9, draw_pile_count=1, draw_pile=[{"name": "Wound"}])
        bot._combat(state)
        self.assertEqual(bot.mem.deck_size, 4)

    def test_star_cards_are_worth_less_without_stars_to_pay_for_them(self):
        comet = {**card(0, "COMET", "Comet", "Attack", 0, "Deal 33 damage.", "AnyEnemy"), "star_cost": 5}
        pulse = {**card(0, "ASTRAL_PULSE", "Astral Pulse", "Attack", 0, "Deal 6 damage to ALL enemies twice.", "AllEnemies"), "star_cost": 3}
        glow = card(0, "GLOW", "Glow", "Skill", 1, f"Gain {STAR}. Draw 1 card.", "Self")
        deck = [STRIKE(i) for i in range(4)] + [DEFEND(i) for i in range(4)] + [pulse, pulse]
        self.assertLess(strategy.deck_fit(comet, deck), -2.0)
        self.assertGreater(strategy.deck_fit(glow, deck), 0.5)
        self.assertEqual(strategy.deck_fit(glow, [STRIKE(0)]), 0.0)

    def test_the_stars_a_fight_starts_with_do_not_pay_for_every_cycle(self):
        # The starter deck (Falling Star 2, Venerate +2) with one Astral Pulse: a third star card would ask for 8 Stars a cycle of a deck that makes 2 (and has 3 once, when the fight starts).
        star = lambda cid, name, n: {**card(0, cid, name, "Attack", 0, "Deal 8 damage.", "AnyEnemy"), "star_cost": n}
        venerate = card(0, "VENERATE", "Venerate", "Skill", 1, f"Gain {STAR}{STAR}.", "Self")
        deck = [STRIKE(i) for i in range(4)] + [DEFEND(i) for i in range(4)] + [star("FALLING_STAR", "Falling Star", 2), venerate, star("ASTRAL_PULSE", "Astral Pulse", 3)]
        self.assertLess(strategy.deck_fit(star("GAMMA_BLAST", "Gamma Blast", 3), deck), -2.0)
        genesis = card(0, "GENESIS", "Genesis", "Power", 2, f"At the start of your turn, gain {STAR}{STAR}.", "Self")
        self.assertGreater(strategy.deck_fit(star("GAMMA_BLAST", "Gamma Blast", 3), deck + [genesis]), -0.1)  # Genesis pays for it


class OtherCharacters(unittest.TestCase):
    """What the Silent, Regent, Necrobinder and Defect do that the Ironclad does not."""

    def test_poison_is_worth_more_on_a_stack_and_in_a_long_fight(self):
        self.assertEqual(combat._poison_total(5, 7), 15)  # 5 + 4 + 3 + 2 + 1
        self.assertEqual(combat._poison_total(15, 7) - combat._poison_total(10, 7), 35)  # 5 more for seven turns
        stack = lambda n, fight: gain([parse(DEADLY)], read_foes(enemy("A", 300, status=[power("POISON", n)] if n else [])), **ctx(energy=1, fight=fight))
        self.assertGreater(stack(10, "boss"), 2 * stack(0, "boss"))
        self.assertGreater(stack(0, "boss"), stack(0, "monster"))

    def test_a_plasma_orb_is_evoked_to_pay_for_more_cards(self):
        # Seen against the Insatiable: 1 energy, a Plasma orb in front, and Ball Lightning played alone. Dualcast first is 4 energy: both attacks.
        hand = [card(0, "BALL_LIGHTNING", "Ball Lightning+", "Attack", 1, "Deal 10 damage. Channel 1 Lightning.", "AnyEnemy"), card(1, "DUALCAST", "Dualcast", "Skill", 1, "Evoke your rightmost Orb twice.", "Self"),
                card(2, "EXTERMINATE", "Exterminate", "Attack", 1, "Deal 4 damage to ALL enemies 4 times.", "AllEnemies"), card(3, "STRIKE_D", "Strike", "Attack", 1, "Deal 6 damage.", "AnyEnemy")]
        state = combat_state(hand, [enemy("THE_INSATIABLE_0", 161, "10x2")], energy=1, hp=35, max_hp=75, kind="boss", orbs=[{"id": "PLASMA_ORB", "passive_val": 1, "evoke_val": 2}], orb_slots=3)
        self.assertEqual(pick(state), 1)
        state["player"]["hand"] = hand[:2]  # with nothing to spend it on, the orb and its energy every turn are kept
        self.assertEqual(pick(state), 0)

    def test_frost_orbs_count_as_block(self):
        state = combat_state([DEFEND(0), STRIKE(1)], [enemy("A", 100, attack=7)], energy=1, hp=10)
        self.assertEqual(pick(state), 0)  # must block: 7 would hurt
        state["player"].update(orbs=[{"id": "FROST_ORB", "name": "Frost", "passive_val": 2, "evoke_val": 5}] * 4, orb_slots=4)
        self.assertEqual(pick(state), 1)  # the orbs block it; attack

    def test_evoke_takes_the_orb_channeled_first(self):
        # Orbs come oldest first, and "your rightmost Orb" is that oldest one.
        dualcast = parse(card(0, "DUALCAST", "Dualcast", "Skill", 0, "Evoke your rightmost Orb twice.", "Self"))
        self.assertEqual(dualcast.evoke, 2)
        self.assertEqual(parse(card(0, "QUADCAST", "Quadcast", "Skill", 1, "Evoke your rightmost Orb 4 times.")).evoke, 4)
        frost, glass = ("FROST", 2, 5), ("GLASS", 4, 8)
        foes, kw = read_foes(enemy("A", 100, attack=20)), ctx(energy=0, hp=19, max_hp=100, block=6, fight="boss", orb_slots=3)
        frost_first = combat.simulate([dualcast], foes, orbs=(frost, glass, glass), **kw).score
        self.assertGreater(frost_first, combat.simulate([dualcast], foes, orbs=(glass, glass, frost), **kw).score + 20)  # 10 Block that keeps us alive against 16 damage

    def test_shatter_sets_off_every_orb(self):
        shatter = parse(card(0, "SHATTER", "Shatter", "Attack", 1, "Deal 7 damage to ALL enemies. Evoke all of your Orbs twice. Exhaust.", "AllEnemies"))
        self.assertEqual(shatter.evoke_all, 2)
        foes, kw = read_foes(enemy("A", 200, attack=30)), ctx(energy=1, hp=40, orb_slots=3)
        loaded = combat.simulate([shatter], foes, orbs=(("LIGHTNING", 3, 8), ("FROST", 2, 5)), **kw).score
        self.assertGreater(loaded, combat.simulate([shatter], foes, **kw).score + 30)  # 16 damage and 10 Block on top

    def test_a_kill_by_orb_is_a_kill(self):
        dualcast = card(0, "DUALCAST", "Dualcast", "Skill", 1, "Evoke your rightmost Orb twice.", "Self")
        orbs = dict(orbs=[{"id": "LIGHTNING_ORB", "name": "Lightning", "passive_val": 3, "evoke_val": 8}], orb_slots=3)
        state = combat_state([dualcast, DEFEND(1)], [enemy("A", 14, attack=30)], energy=1, hp=20, **orbs)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0}))  # 16 kills it: no need to block
        # Vulnerable does nothing for orbs: 17 HP is out of reach, so it blocks (and lives, on 26 HP).
        state = combat_state([dualcast, DEFEND(1)], [enemy("A", 17, attack=30, status=[power("VULNERABLE", 2)])], energy=1, hp=26, **orbs)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 1}))

    def test_focus_strengthens_the_orbs_a_card_channels(self):
        zap, foes, kw = parse(ZAP), read_foes(enemy("A", 100)), ctx(energy=1, hp=50, orb_slots=3)
        self.assertGreater(combat.simulate([zap], foes, focus_power=3, **kw).score, combat.simulate([zap], foes, **kw).score)

    def test_osty_takes_the_hit(self):
        state = combat_state([DEFEND(0), STRIKE(1)], [enemy("A", 100, attack=8)], energy=1, hp=80, pets=[{"id": "OSTY", "alive": True, "hp": 9, "max_hp": 9}])
        self.assertEqual(combat.expected_end_loss(state), 0)
        self.assertEqual(pick(state), 1)
        state["player"]["hp"] = 10  # ... but what he soaks now he cannot soak later: badly hurt, the Defend keeps him standing
        self.assertEqual(pick(state), 0)

    def test_regret_costs_a_point_for_every_card_still_held(self):
        regret = {**card(0, "REGRET", "Regret", "Curse", 0, "Unplayable. At the end of your turn, if this is in your Hand, lose 1 HP for each card in your Hand."), "can_play": False}
        state = combat_state([regret, DEFEND(1), DEFEND(2), STRIKE(3)], [enemy("A", 100)], energy=0, hp=30)
        self.assertEqual(combat.expected_end_loss(state), 4)  # (it was read as "lose 1 HP")
        state["player"]["energy"] = 1
        self.assertIsNotNone(combat.choose_card(state))  # a card played is a point saved

    def test_osty_does_not_stop_what_burns_in_the_hand(self):
        toxic = lambda i: card(i, "TOXIC", "Toxic", "Status", 1, "At the end of your turn, if this is in your Hand, take 5 damage. Exhaust.")
        state = combat_state([toxic(0), toxic(1)], [enemy("A", 100, attack=6)], energy=0, hp=21, max_hp=75, pets=[{"id": "OSTY", "alive": True, "hp": 20, "max_hp": 20}])
        self.assertEqual(combat.expected_end_loss(state), 10)  # he takes the attack; the Toxics are ours
        state["player"]["block"] = 7
        self.assertEqual(combat.expected_end_loss(state), 3)  # our own Block does stop them
        state["player"]["pets"][0]["hp"] = 2
        self.assertEqual(combat.expected_end_loss(state), 7)  # ... and then there is none left for the attack he cannot take whole

    def test_strength_given_up_for_good_is_counted(self):
        # Two Friendships went down on sight, and Strikes hit for 2 with 7 energy and nothing to spend it on.
        friendship = lambda i: card(i, "FRIENDSHIP", "Friendship", "Power", 1, "Lose 2 Strength. Gain [necrobinder_energy_icon.png] at the start of each turn.")
        info = parse(friendship(0))
        self.assertEqual((info.strength, info.strength_lasts), (-2, True))
        flex = parse(card(0, "FLEX", "Flex", "Skill", 0, "Gain 2 Strength. At the end of your turn, lose 2 Strength."))
        self.assertEqual(flex.strength, 2)  # (given back at the end of the turn is not a price paid)
        foes, kw = read_foes(enemy("A", 300)), ctx(hp=70, fight="boss")
        score = lambda line, **more: combat.simulate(line, foes, **kw, **more).score
        nothing, first = score([]), score([info])
        self.assertGreater(first, nothing)  # an energy every turn of a boss fight is worth 2 Strength
        self.assertLess(score([info], friends=1), nothing)  # ... a second one is not
        self.assertLess(score([info, parse(friendship(1))]), first)
        strike = parse(STRIKE(2))  # in the same line the Strength is gone for the attacks after it
        self.assertLess(score([info, strike]) - first, score([strike]) - nothing)

    def test_strength_taken_off_an_enemy_for_good_is_for_the_one_that_hits_often(self):
        info = parse(card(0, "SHARED_FATE", "Shared Fate", "Skill", 0, "Lose 2 Strength. Enemy loses 2 Strength. Exhaust.", "AnyEnemy"))
        self.assertEqual((info.strength, info.enemy_strength_loss, info.enemy_strength_lasts), (-2, 2, True))
        self.assertFalse(parse(card(0, "PIERCING_WAIL", "Piercing Wail", "Skill", 1, "ALL enemies lose 6 Strength this turn. Exhaust.")).enemy_strength_lasts)
        kw = ctx(hp=70, block=30, fight="boss")  # (behind Block: only what lasts is being weighed)
        against = lambda attack: gain([info], read_foes(enemy("A", 300, attack=attack)), **kw)
        self.assertGreater(against("8x3"), 0)  # 6 off every such turn for our 2 Strength
        self.assertLess(against("20"), 0)  # 2 off a single hit is not worth them
        self.assertGreater(against("8x3"), against("20"))

    def test_stars_pay_for_star_cards(self):
        venerate = card(0, "VENERATE", "Venerate", "Skill", 1, f"Gain {STAR}{STAR}.")
        falling = card(1, "FALLING_STAR", "Falling Star", "Attack", 0, "Deal 8 damage. Apply 1 Weak. Apply 1 Vulnerable.", "AnyEnemy", star_cost="2")
        foes, kw = read_foes(enemy("A", 100)), ctx(energy=1, hp=80)
        self.assertIsNone(combat.simulate([parse(falling)], foes, **kw))  # no Stars yet
        self.assertIsNotNone(combat.simulate([parse(venerate), parse(falling)], foes, **kw))

    def test_more_stars_are_worth_little_once_plenty_are_held(self):
        venerate = card(0, "VENERATE", "Venerate", "Skill", 1, f"Gain {STAR}{STAR}.", "Self")
        held = lambda stars: worth(combat_state([venerate], [enemy("A", 60, attack=0)], energy=1, stars=stars), [venerate])
        self.assertGreater(held(0), 4.0)
        self.assertLess(held(12), 1.0)

    def test_their_key_cards_are_rated(self):
        fumes = card(0, "NOXIOUS_FUMES", "Noxious Fumes", "Power", 1, "At the start of your turn, apply 2 Poison to ALL enemies.", rarity="Uncommon")
        self.assertGreater(deck_value(fumes), 5.0)
        self.assertGreater(deck_value(fumes), deck_value(card(1, "SLICE", "Slice", "Attack", 0, "Deal 6 damage.", "AnyEnemy")))


class RageAndRespawn(unittest.TestCase):
    def crabs(self, crusher_hp=6, rocket_hp=120):
        crab = lambda eid, hp: combat.Enemy(entity_id=eid, hp=hp, block=0, vulnerable=False, weak=False, incoming=6, rage=True)
        return [crab("CRUSHER_0", crusher_hp), crab("ROCKET_0", rocket_hp)]

    def test_a_surviving_crab_gets_block_once_per_ally_that_died(self):
        foes, known = self.crabs(crusher_hp=0), set()
        combat._rage_blocks(foes, known)
        combat._rage_blocks(foes, known)  # the same death is not counted twice
        self.assertEqual(foes[1].block, combat.RAGE_BLOCK)
        self.assertEqual(known, {"CRUSHER_0"})

    def test_hitting_the_second_crab_after_the_first_dies_is_worth_less(self):
        line, kw = [parse(STRIKE(0)), parse(STRIKE(1))], ctx(energy=2, hp=80, fight="boss")  # kill the Crusher, then hit the Rocket
        with_rage = combat.simulate(line, self.crabs(), **kw).score
        with mock.patch.object(battle, "RAGE_BLOCK", 0):
            self.assertLess(with_rage, combat.simulate(line, self.crabs(), **kw).score)

    def axebot(self, hp=5, respawns=2):
        return combat.Enemy(entity_id="AXEBOT_0", hp=hp, block=0, vulnerable=False, weak=False, incoming=15, respawns=respawns, max_hp=85)

    def test_a_stock_enemy_comes_back_at_full_hp_while_it_has_stock(self):
        foes = [self.axebot(hp=0, respawns=1)]
        combat._respawns(foes)  # first death: back at full HP, one stack used
        self.assertEqual((foes[0].alive, foes[0].hp, foes[0].respawns), (True, 85, 0))
        foes[0].hp = 0
        combat._respawns(foes)  # second death, no stock left: really dead
        self.assertFalse(foes[0].alive)

    def test_killing_a_stock_enemy_is_not_a_win(self):
        line, kw = [parse(STRIKE(0))], ctx(energy=1, hp=80, fight="boss")  # 6 damage on a 5-HP Axebot
        with_stock = combat.simulate(line, [self.axebot(respawns=2)], **kw).score
        self.assertLess(with_stock, combat.simulate(line, [self.axebot(respawns=0)], **kw).score - combat.WIN_BONUS / 2)

    def test_killing_a_stock_enemy_takes_its_attack_away_for_the_turn(self):
        # Axebot on 32 about to hit for 17: the Hammer and the Crush Under kill it, and the new one only Defends and Buffs this turn. That beats 10 damage and 8 Block in front of the 17.
        hammer = card(0, "HEIRLOOM_HAMMER", "Heirloom Hammer+", "Attack", 2, "Deal 22 damage.", "AnyEnemy")
        crush = card(1, "CRUSH_UNDER", "Crush Under+", "Attack", 1, "Deal 10 damage.", "AnyEnemy")
        wall = card(2, "GLITTERSTREAM", "Glitterstream", "Skill", 2, "Gain 8 Block.", "Self")
        plan: dict = {}
        self.assertIn(pick(combat_state([hammer, crush, wall], [{**enemy("AXEBOT_0", 32, attack=17, status=[power("STOCK")]), "max_hp": 87}], energy=3, hp=82, max_hp=139), plan), (0, 1))
        self.assertEqual(plan["lost"], 0)

    def test_stock_is_read_from_the_state(self):
        (foe,) = read_foes({"entity_id": "AXEBOT_0", "hp": 5, "max_hp": 85, "block": 0, "status": [power("STOCK", 2)], "intents": []})
        self.assertEqual((foe.respawns, foe.max_hp), (2, 85))
