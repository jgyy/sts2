import unittest

from sts2bot.cards import parse
from sts2bot.fight import combat, triggers
from sts2bot.run import strategy
from sts2bot.run.strategy import Memory

from ..fake_game import card
from ..fight import aimed, combat_state, ctx, gain, outcome, pick, play_turn, potion, power, read_foes, worth
from ..helpers import DEFEND, STRIKE, enemy

ESCAPE = lambda i, cost=1: card(i, "FRANTIC_ESCAPE", "Frantic Escape", "Status", cost, "Get farther away. Increase Sandpit by 1. Increase the cost of this card by 1.")
BECKON = lambda i, target="Self": card(i, "BECKON", "Beckon", "Status", 1, "At the end of your turn, if this is in your Hand,  lose 6 HP.", target)


class BossMechanics(unittest.TestCase):
    """Things the bosses do that the planner got wrong (found in the fight logs of 2026-11-18)."""

    def test_a_death_blow_is_an_attack(self):
        blow = {"type": "DeathBlow", "label": "39"}
        self.assertEqual((combat.intent_damage(blow), combat.intent_hits(blow)), (39, 1))

    def test_the_giants_last_blow_is_blocked_and_the_dead_giant_left_alone(self):
        # Killed, the Waterfall Giant stands on 999,999,999 HP until it blows up.
        giant = {**enemy("WATERFALL_GIANT_0", 999999999, status=[power("STEAM_ERUPTION", 39)]), "intents": [{"type": "DeathBlow", "label": "39"}]}
        big = card(0, "KICK", "Kingly Kick", "Attack", 0, "Deal 35 damage.", "AnyEnemy")
        reflect = card(1, "REFLECT", "Reflect", "Skill", 1, "Gain 15 Block.", "Self")
        state = combat_state([big, reflect, DEFEND(2), DEFEND(3)], [giant], energy=3, hp=36, max_hp=75, kind="boss")
        self.assertEqual(sorted(c["index"] for c in play_turn(state, block=True)), [1, 2, 3])  # every Block card, and not one hit on the giant
        self.assertIsNone(combat.choose_card(state))  # the free attack is not worth playing either

    def test_beckon_is_seen_despite_the_double_space(self):
        # The mod turns the line break in Beckon's text into a second space.
        state = combat_state([BECKON(0), DEFEND(1)], [enemy("FYSH", 200, attack=4)], energy=1, kind="boss")
        self.assertEqual(combat.hand_burns(state["player"]), ((0, 6, False),))
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0}))  # 6 saved beats 4 blocked

    def test_a_turn_only_the_fortifier_saves_is_played_for_the_fortifier(self):
        # The Soul Fysh on 6 HP, us on 4 with 36 coming, a Beckon in hand and a Fortifier on the belt: three Block cards (22,
        # tripled 66) left the Beckon's 6 to kill; the Beckon and two of them are 15, tripled 45.
        guard = lambda i, n: card(i, f"GUARD_{i}", f"Guard {n}", "Skill", 1, f"Gain {n} Block.", "Self")

        def turn(belt, kind="boss"):
            state = combat_state([guard(0, 7), guard(1, 7), BECKON(2), guard(3, 8)], [enemy("SOUL_FYSH", 6, attack=36)], energy=3, hp=4, max_hp=82, kind=kind, potions=belt)
            played = play_turn(state, limit=9, block=True, choose=lambda s: combat.actions(s)[0])
            return sorted(c["name"] for c in played), combat.actions(state)[0]

        # The Beckon and two Block cards (14 or 15: either holds once tripled), then the potion on the Block that is up.
        fortifier = potion(0, "Fortifier", "Triple your Block.")
        played, then = turn([fortifier])
        self.assertEqual((played[0], len(played), then), ("Beckon", 3, ("use_potion", {"slot": 0})))
        self.assertEqual(turn([fortifier], "monster"), (played, then))  # (in an ordinary fight too: it is the danger that counts)
        self.assertEqual(turn([])[1], ("end_turn", {}))  # (without it nothing saves the turn, whatever is played)

    def test_block_stops_cards_that_deal_damage_but_not_ones_that_take_hp(self):
        wither = card(0, "WITHER", "Wither+2", "Status", 0, "Unplayable. At the end of your turn, if this is in your Hand, take 9 damage.")
        foe = read_foes(enemy("A", 300, attack=10))
        behind = lambda c, block: combat.simulate([], foe, burns=combat.hand_burns({"hand": [c]}), **ctx(energy=0, hp=50, block=block, fight="boss")).score
        self.assertEqual(behind(wither, 19), behind(wither, 25))  # 19 Block takes the Wither and the attack
        self.assertLess(behind(wither, 10), behind(wither, 19))
        self.assertEqual(behind(BECKON(0, "None"), 10), behind(BECKON(0, "None"), 25))  # the 6 HP go whatever the Block

    def test_a_wither_costs_three_for_every_level(self):
        named = lambda name: {"hand": [{"name": name, "description": "At the end of your turn, if this is in your Hand, take 3 damage."}]}
        self.assertEqual(combat.hand_burns(named("Wither"))[0][1], 3)
        self.assertEqual(combat.hand_burns(named("Wither+2"))[0][1], 9)
        self.assertEqual(combat._next_wither({"discard_pile": [{"name": "Wither+1"}]}), 6)
        self.assertEqual(combat._next_wither({}, round_no=8), 9)  # two of Aeonglass's upgrades have gone by

    def test_the_card_that_brings_the_next_wither_is_not_held_back(self):
        # Aeonglass's count on 1 and 25 coming: the turn ended on three energy with a Block card in hand, as the Wither it would
        # bring cost more this turn than the Block saved. But the count carries over, and the Wither came all the same.
        count = lambda n: [power("WITHERING_PRESENCE", n)]
        state = combat_state([DEFEND(0)], [enemy("AEONGLASS", 410, attack=25, status=count(1))], energy=3, hp=62, max_hp=109, block=4, kind="boss", round_no=4,
                             draw_pile_count=20, discard_pile_count=9)  # a deck of thirty
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0}))
        # A card worth less than the Withers it brings nearer still stays in the hand, on any count.
        poke = card(0, "POKE", "Poke", "Attack", 0, "Deal 3 damage.", "AnyEnemy")
        for n in (1, 4):
            self.assertIsNone(combat.choose_card(combat_state([poke], [enemy("AEONGLASS", 400, status=count(n))], kind="boss")))

    def test_a_wither_made_late_in_the_fight_has_less_time_to_come_round(self):
        deck = {"hand": [{}] * 5, "draw_pile_count": 15, "discard_pile_count": 10}  # thirty cards: six turns to come round
        early, late = combat._wither_bill(deck, 2, 3), combat._wither_bill(deck, 9, 9)
        self.assertAlmostEqual(early, 5.25)  # (what a card cost on rounds 1-3 before: a 3-damage Poke is still not worth it)
        self.assertLess(late, early / 2)  # two rounds left of eleven: it will hardly be drawn again
        self.assertGreater(combat._wither_bill(deck, 14, 12), 0.0)  # a fight that runs long still pays something

    def test_dead_on_paper_it_plays_the_block_it_has(self):
        # 29 coming, 22 HP, one Defend+: nothing saves it for sure, but the Block comes closest.
        defrag = card(0, "DEFRAGMENT", "Defragment", "Power", 1, "Gain 1 Focus.", "Self")
        guard = card(1, "DEFEND", "Defend+", "Skill", 1, "Gain 8 Block.", "Self")
        turn = lambda hand, energy: combat.choose_card(combat_state(hand, [enemy("A", 300, attack=40)], energy=energy, hp=22, max_hp=90, kind="boss"))
        self.assertEqual(turn([defrag, guard], 1), ("play_card", {"card_index": 1}))
        # With energy for both, the cards are drawn first: one of them may be the Block that is missing.
        self.assertEqual(turn([defrag, guard, card(2, "SKIM", "Skim", "Skill", 1, "Draw 3 cards.", "Self")], 2), ("play_card", {"card_index": 2}))

    def test_an_escape_is_played_before_the_margin_is_gone(self):
        # Sandpit 3 and one Escape in hand: ending on 2 needs an Escape in the very next hand.
        boss = enemy("INS", 200, attack=0, status=[power("SANDPIT", 3)])
        pile = lambda: [{"name": "Strike"}] * 20 + [{"name": "Frantic Escape"}] * 2
        state = combat_state([ESCAPE(0), STRIKE(1), STRIKE(2), STRIKE(3)], [boss], energy=3, kind="boss", draw_pile=pile())
        self.assertIn(0, {c["index"] for c in play_turn(state)})
        # ... even at 2 energy, the whole of what is left.
        state = combat_state([ESCAPE(0, cost=2), STRIKE(1), STRIKE(2)], [boss], energy=2, kind="boss", draw_pile=pile())
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0}))

    def test_escape_odds_follow_the_piles(self):
        none_left = {"draw_pile": [{"name": "Strike"}] * 10, "discard_pile": [{"name": "Frantic Escape"}] * 6, "hand": []}
        self.assertEqual(combat._escape_odds(none_left)[0], 1.0)  # the next five cards hold none for certain
        self.assertEqual(combat._escape_odds({"draw_pile": [{"name": "Frantic Escape"}] * 5, "discard_pile": [], "hand": []})[0], 0.0)
        mixed = {"draw_pile": [{"name": "Strike"}] * 24 + [{"name": "Frantic Escape"}] * 6, "discard_pile": [], "hand": []}
        self.assertAlmostEqual(combat._escape_odds(mixed)[0], 0.298, places=2)

    def test_beating_a_form_of_the_test_subject_is_not_winning_the_fight(self):
        kw, kill = ctx(energy=2, fight="boss"), [parse(STRIKE(0))]
        foes = lambda status: read_foes(enemy("TEST_SUBJECT_0", 5, attack=15, status=status))
        self.assertGreater(combat.simulate(kill, foes([]), **kw).score, combat.WIN_BONUS)
        beaten = combat.simulate(kill, foes([power("ADAPTABLE")]), **kw).score
        self.assertLess(beaten, combat.WIN_BONUS / 2)
        self.assertGreater(beaten, combat.simulate([], foes([power("ADAPTABLE")]), **kw).score + 40)  # ... but it is worth doing: its attack this turn does not happen
        self.assertAlmostEqual(combat.simulate(kill + [parse(STRIKE(1))], foes([power("ADAPTABLE")]), **kw).score, beaten)  # once it is gone nothing more can be aimed at it this turn

    def test_set_up_cards_are_played_while_the_enemy_is_away(self):
        loop = card(0, "LOOP", "Loop", "Power", 1, "At the start of your turn, trigger the passive of your rightmost Orb.", "Self")
        zap = card(1, "ZAP", "Zap", "Skill", 1, "Channel 1 Lightning.", "Self")
        state = combat_state([loop, zap, STRIKE(2), DEFEND(3)], [], energy=2, kind="boss")
        self.assertIsNone(combat.choose_card(state))  # a fight that is simply over
        state["_revive_pending"] = True
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0}))
        state["player"].update(hand=[zap, STRIKE(2), DEFEND(3)], energy=1)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 1}))
        state["player"]["hand"] = [STRIKE(2), DEFEND(3)]
        self.assertIsNone(combat.choose_card(state))  # no use hitting or blocking thin air

    def test_an_x_cost_summon_is_set_up_too(self):
        dirge = card(0, "DIRGE", "Dirge", "Skill", "X", "Summon 3 X times. Add X Souls into your Draw Pile. Exhaust.", "Self")
        self.assertTrue(parse(dirge).is_x)
        state = combat_state([dirge, DEFEND(1)], [], energy=2, kind="boss", _revive_pending=True)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0}))
        state["player"]["energy"] = 0
        self.assertIsNone(combat.choose_card(state))

    def test_vital_spark_taints_by_its_own_amount(self):
        state = combat_state([DEFEND(0)], [enemy("PRISM", 60, attack="5x3", status=[power("VITAL_SPARK", 4)])], energy=1, hp=18)
        self.assertEqual(combat._context(state)["taint"], 4)
        self.assertIsNone(combat.choose_card(state))  # a 5-Block Skill that adds 4 to each of three hits lets more through, not less

    def test_an_escape_is_never_the_card_given_up(self):
        hand = [ESCAPE(0), card(1, "X", "Hyperbeam", "Attack", 2, "Deal 26 damage to ALL enemies.", rarity="Rare"), DEFEND(2)]
        state = {"state_type": "hand_select", "hand_select": {"prompt": "Choose a card to Exhaust.", "cards": hand, "can_confirm": False}}
        self.assertEqual(strategy.hand_select(state, Memory())[0], ("combat_select_card", {"card_index": 2}))
        order = [a[1]["card_index"] for a in strategy.hand_select(state, Memory()) if a[0] == "combat_select_card"]
        self.assertEqual(order[-1], 0)  # the Escape goes last, after even the best card


class CardPlayTriggers(unittest.TestCase):
    """Powers that act on every card played are part of the turn's arithmetic."""

    def subject(self, hp, hand, powers, energy=3):
        foe = {"entity_id": "TEST_SUBJECT_0", "hp": hp, "max_hp": 200, "block": 0, "status": [power("ADAPTABLE"), power("DOOM", 28)], "intents": [{"type": "Attack", "label": "10x5"}]}
        return combat_state(hand, [foe], energy=energy, hp=10, max_hp=60, kind="boss", status=powers)

    def test_sleight_of_flesh_finishes_what_the_attack_started(self):
        exterminate = card(0, "EXTERMINATE", "Exterminate+", "Attack", 1, "Deal 7 damage to ALL enemies 4 times.", "AllEnemies")
        bringer = lambda i: card(i, "DEATHBRINGER", "Deathbringer+", "Skill", 2, "Apply 26 Doom and 1 Weak to ALL enemies.", "AllEnemies")
        defend = lambda i: card(i, "DEFEND_N", "Defend+", "Skill", 1, "Gain 8 Block.", "Self")
        sleight = [power("SLEIGHT_OF_FLESH", 13)]
        # 33 HP: the attack takes 28, the two debuffs 13 each. The form dies and its 50 damage never comes.
        self.assertIn(pick(self.subject(33, [exterminate, bringer(1), defend(2)], sleight)), (0, 1))  # (either first: the two together are the kill)
        self.assertEqual(combat.choose_card(self.subject(5, [bringer(0), defend(1)], sleight, energy=2)), ("play_card", {"card_index": 0}))
        # Without the Power there is no kill in the hand, and no Block that saves us either (10 HP in front of 50): the attack
        # that leaves the form on 5 HP is the best try.
        self.assertIn(pick(self.subject(33, [exterminate, bringer(1), defend(2)], [])), (0, 1))  # (the Doom card goes first now; both are played)

    def test_afterimage_and_strangle_count_for_every_card(self):
        s = combat_state([DEFEND(0), DEFEND(1)], [enemy("RAT_0", 5, attack=9, status=[power("STRANGLE", 3)])], energy=2, hp=30, status=[power("AFTERIMAGE")])
        t = triggers.read(s)
        self.assertEqual((t.card_block, t.strangle), (1, {"RAT_0": 3}))
        self.assertEqual(outcome(s, s["player"]["hand"]).lost, 0)  # two Defends strangle it: 6 HP lost on 5
        self.assertIsNone(triggers.read(combat_state([DEFEND(0)], [enemy("A", 5)])))


class PersonalHive(unittest.TestCase):
    """The Entomancer: every Attack that hits it puts Dazed into our draw pile."""

    def first(self, hive, hand, junk=0, energy=1):
        dazed = card(9, "DAZED", "Dazed", "Status", 0, "Unplayable. Ethereal.")
        s = combat_state(hand, [enemy("ENTOMANCER_0", 140, status=[power("PERSONAL_HIVE", hive)])], energy=energy, hp=60, kind="elite",
                         draw_pile=[STRIKE(9)] * (12 - junk) + [dazed] * junk, discard_pile=[])
        act = combat.choose_card(s)
        return None if act is None else act[1]["card_index"]

    def test_small_attacks_stop_as_the_hive_grows(self):
        big = card(1, "BLUDGEON", "Bludgeon", "Attack", 1, "Deal 32 damage.", "AnyEnemy")
        doom = card(2, "NO_ESCAPE", "No Escape", "Skill", 1, "Apply 15 Doom.", "AnyEnemy")
        self.assertEqual(self.first(1, [card(0, "DRAIN_POWER", "Drain Power", "Attack", 1, "Deal 12 damage.", "AnyEnemy")]), 0)  # one Dazed for 12 damage: worth it
        self.assertEqual(self.first(1, [STRIKE(0)]), 0)  # ... and for 6: the hand has a card to spare for one Dazed
        self.assertIsNone(self.first(3, [STRIKE(0)]))  # three of them for 6 is a bad trade
        self.assertEqual(self.first(3, [STRIKE(0), big]), 1)  # a big one is not
        self.assertEqual(self.first(3, [STRIKE(0), doom]), 2)  # and what is not an Attack costs nothing
        self.assertEqual(self.first(3, [STRIKE(0)], junk=8), 0)  # (in a deck already full of them a draw is worth little)
        self.assertEqual(read_foes(enemy("E", 9, status=[power("PERSONAL_HIVE", 2)]))[0].hive, 2)

    def test_the_hive_never_stops_every_attack(self):
        # (Nothing was played with 4 energy, three attacks in hand and 4x7 coming, at hive 2.)
        unleash = card(0, "UNLEASH", "Unleash", "Attack", 1, "Deal 10 damage.", "AnyEnemy")
        storm = card(1, "SOUL_STORM", "Soul Storm", "Attack", 1, "Deal 9 damage.", "AnyEnemy")
        hive = lambda hp, n: enemy("ENTOMANCER_0", hp, attack="4x7", status=[power("PERSONAL_HIVE", n)])
        s = combat_state([unleash, storm, STRIKE(2)], [hive(88, 2)], energy=4, hp=65, max_hp=76, kind="elite", draw_pile=[STRIKE(9)] * 20, discard_pile=[])
        self.assertEqual(pick(s), 0)  # the biggest one goes
        s["player"]["hand"] = [STRIKE(2)]
        self.assertEqual(pick(s), 2)  # ... a Strike too, when it is the only one
        # After it, each further attack is two whole cards later on: only one that is worth them.
        foes, kw = combat.read_enemies(s), ctx(energy=4, hp=65, max_hp=76, fight="elite", deck=(21, 0))
        self.assertLess(combat.simulate([parse(unleash), parse(storm)], foes, **kw).score, combat.simulate([parse(unleash)], foes, **kw).score)
        # With the kill in the hand nothing is drawn afterwards: everything goes.
        foe = hive(24, 3)

        def hit(c, _):
            foe["hp"] -= {"Unleash": 10, "Soul Storm": 9, "Strike": 6}[c["name"]]
            return foe["hp"] <= 0

        self.assertEqual(len(play_turn(combat_state([unleash, storm, STRIKE(2)], [foe], energy=4, hp=65, max_hp=76, kind="elite"), limit=4, after=hit)), 3)


class Disintegration(unittest.TestCase):
    def test_block_stops_constrict_too(self):
        state = combat_state([DEFEND(0), DEFEND(1), STRIKE(2)], [enemy("SLITHERING_STRANGLER_0", 40)], energy=2, hp=40, status=[power("CONSTRICT", 6)])
        self.assertEqual(combat.expected_end_loss(state), 6)  # no attack coming: the squeeze alone
        # One Defend takes 5 of the 6; the Strike is worth more than the last point.
        self.assertEqual(sorted(c["name"] for c in play_turn(state, block=True)), ["Defend", "Strike"])
        self.assertEqual(combat.expected_end_loss(state), 1)

    def test_block_stops_the_burn_like_any_other_damage(self):
        state = combat_state([DEFEND(0), DEFEND(1)], [enemy("KNOWLEDGE_DEMON_0", 80, attack=6)], energy=2, hp=40, kind="boss", status=[power("DISINTEGRATION", 8)])
        self.assertEqual(combat.expected_end_loss(state), 14)  # 8 and then 6, nothing in the way
        state["player"]["block"] = 10
        self.assertEqual(combat.expected_end_loss(state), 4)  # seen live: 8 a turn, 0 HP lost behind 25 Block


class PainfulStabs(unittest.TestCase):
    def test_each_hit_stopped_is_a_wound_saved(self):
        guard = lambda i: card(i, "DEFEND_B", "Defend+", "Skill", 1, "Gain 10 Block.", "Self")

        def score(line, stabs):
            foe = {"entity_id": "T", "hp": 190, "max_hp": 200, "block": 0, "intents": [{"type": "Attack", "label": "10x3"}], "status": [power("PAINFUL_STABS")] if stabs else []}
            state = combat_state([guard(0), guard(1)], [foe], energy=2, hp=80, kind="boss")
            return outcome(state, [state["player"]["hand"][i] for i in line]).score

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
    SHARDS = staticmethod(lambda i: card(i, "BONE_SHARDS", "Bone Shards", "Attack", 1, "If Osty is alive, he deals 9 damage to ALL enemies and you gain 9 Block. Osty dies.", "AllEnemies"))
    PULSE = staticmethod(lambda i: card(i, "NEGATIVE_PULSE", "Negative Pulse", "Skill", 1, "Gain 5 Block. Apply 7 Doom to ALL enemies.", "AllEnemies"))

    def state(self, hand, osty, foe_hp=40, attack=12, doom=0, energy=3):
        foe = enemy("SLUG_0", foe_hp, attack=attack, status=[power("DOOM", doom)] if doom else [])
        return combat_state(hand, [foe], energy=energy, hp=40, pets=[{"name": "Osty", "hp": osty, "alive": osty > 0}])

    def test_a_summon_goes_before_unleash(self):
        unleash = card(0, "UNLEASH", "Unleash", "Attack", 1, "Osty deals 7 damage. Deals additional damage equal to Osty's current HP.", "AnyEnemy")
        guard = card(1, "BODYGUARD", "Bodyguard", "Skill", 1, "Summon 5.", "Self")
        s = self.state([unleash, guard], osty=1, attack=0)
        self.assertEqual(pick(s), 1)
        self.assertEqual(outcome(s, [guard, unleash]).score - outcome(s, [unleash, guard]).score, 5.0)  # 12 damage, not 7
        # After Bone Shards he is dead, whatever was Summoned before it.
        self.assertLess(outcome(s, [self.SHARDS(2), unleash]).score, outcome(s, [unleash, self.SHARDS(2)]).score)

    def test_times_up_hits_for_the_doom_on_the_enemy(self):
        times = card(0, "TIMES_UP", "Time's Up", "Attack", 2, "Deal damage equal to the enemy's Doom.", "AnyEnemy")
        s = self.state([times, self.PULSE(1)], osty=1, foe_hp=200, attack=0, doom=30)
        self.assertEqual(worth(s, [times]), 30.0)
        # ... and the Doom applied first counts: the pulse goes before it.
        self.assertEqual(pick(s), 1)
        self.assertGreater(outcome(s, [self.PULSE(1), times]).score, outcome(s, [times, self.PULSE(1)]).score + 6.0)

    def test_no_escape_grows_with_the_doom_already_there(self):
        escape = card(0, "NO_ESCAPE", "No Escape", "Skill", 1, "Apply 10 Doom, plus an additional 5 Doom for every 10 Doom already on this enemy.", "AnyEnemy")
        scourge = card(1, "SCOURGE", "Scourge", "Skill", 1, "Apply 13 Doom. Draw 1 card.", "AnyEnemy")
        s = self.state([escape, scourge], osty=1, foe_hp=300, attack=0, doom=67)
        self.assertEqual(worth(s, [escape]), 0.8 * (10 + 5 * 6))  # 67 Doom: six tens
        # ... so it goes after the turn's other Doom: 80 Doom is eight tens.
        self.assertEqual(worth(s, [scourge, escape]) - worth(s, [escape, scourge]), 0.8 * 10)
        self.assertEqual(pick(s), 1)

    def test_deaths_door_blocks_three_times_after_doom(self):
        door = card(0, "DEATHS_DOOR", "Death's Door", "Skill", 1, "Gain 6 Block. If you applied Doom this turn, gain Block 2 additional times.", "Self")
        s = self.state([door, self.PULSE(1)], osty=0, foe_hp=200, attack=30)
        self.assertEqual(outcome(s, [door, self.PULSE(1)]).lost, 19)  # 6 + 5
        self.assertEqual(outcome(s, [self.PULSE(1), door]).lost, 7)  # 5 + 18
        self.assertEqual(pick(s), 1)

    def test_sacrifice_takes_osty_with_it(self):
        sacrifice = card(0, "SACRIFICE", "Sacrifice", "Skill", 1, "Retain. If Osty is alive, he dies and you gain Block equal to triple his Max HP. (Gain 15 Block)", "Self")
        self.assertTrue(parse(sacrifice).osty_dies)
        self.assertEqual(outcome(self.state([sacrifice], osty=5, attack=24), [sacrifice]).lost, 9)  # 15 Block against 24: his 5 HP are gone with him
        self.assertEqual(outcome(self.state([sacrifice], osty=0, attack=24), [sacrifice]).lost, 24)

    def test_under_necro_mastery_what_osty_takes_the_enemy_takes(self):
        s = self.state([DEFEND(0)], osty=10, attack=10, energy=1)
        plain = outcome(s).score
        s["player"]["status"] = [power("NECRO_MASTERY")]
        self.assertEqual(outcome(s).score - plain, 10.0)

    def test_hang_hits_twice_as_hard_each_time(self):
        hang = lambda i: card(i, "HANG", "Hang", "Attack", 1, "Deal 10 damage. Double the damage ALL Hang cards deal to this enemy.", "AnyEnemy")
        s = combat_state([hang(0), hang(1)], [enemy("SLUG_0", 100, status=[power("HANG", 4)])], energy=2)
        self.assertEqual(combat.read_enemies(s)[0].hang, 4)
        self.assertEqual(worth(s, [hang(0)]), 40.0)  # 10 x 4
        self.assertGreater(worth(s, [hang(0), hang(1)]), combat.WIN_BONUS)  # 40 + 80: the enemy is dead
        self.assertEqual(gain([parse(hang(0))], read_foes(enemy("SLUG_0", 100)), **combat._context(s)), 10.0)

    def test_block_that_keeps_osty_standing_is_worth_something(self):
        s = self.state([DEFEND(0)], osty=10, attack=10, energy=1)
        self.assertEqual(combat.expected_end_loss(s), 0)  # he takes it all
        self.assertEqual(combat.choose_card(s), ("play_card", {"card_index": 0}))  # ... and with 5 Block keeps 5 HP
        self.assertIsNone(combat.choose_card(self.state([DEFEND(0)], osty=10, attack=0, energy=1)))  # nothing coming: nothing to protect him from

    def test_an_osty_attack_goes_before_flatten_and_rattle(self):
        flatten = card(0, "FLATTEN", "Flatten", "Attack", 2, "Osty deals 12 damage. This card costs 0 [necrobinder_energy_icon.png] if Osty has attacked this turn.", "AnyEnemy")
        rattle = card(0, "RATTLE", "Rattle", "Attack", 1, "Osty deals 7 damage. Hits an additional time for each other time he has attacked this turn. (Hits 1 times)", "AnyEnemy")
        # 2 energy: Flatten first leaves nothing for the Strike; after the Poke it is free. Rattle after the Poke hits twice.
        self.assertEqual(pick(self.state([flatten, self.POKE(1), STRIKE(2), STRIKE(3)], osty=3, foe_hp=90, attack=0, energy=2)), 1)
        self.assertEqual(pick(self.state([rattle, self.POKE(1)], osty=3, foe_hp=90, attack=0, energy=1)), 1)

    def test_doom_on_ourselves_counts_as_hp_already_gone(self):
        surge = card(0, "NEUROSURGE", "Neurosurge", "Power", 0, "Gain 3 energy. Draw 2 cards. At the start of your turn, apply 3 Doom to yourself.", "Self")
        self.assertEqual(pick(combat_state([surge, STRIKE(1)], [enemy("A", 300, attack=10)], hp=60, kind="boss")), 1)  # a long fight ahead: not yet
        self.assertEqual(combat.choose_card(combat_state([surge], [enemy("A", 30, attack=10)], hp=60)), ("play_card", {"card_index": 0}))
        # Round 8, the boss on 90 of 300 at 30 a round: three turns and one to spare is 12 Doom, on 60 HP.
        late = combat_state([surge, STRIKE(1)], [{**enemy("A", 90, attack=10), "max_hp": 300}], hp=60, kind="boss", round_no=8)
        self.assertEqual(pick(late), 0)
        # Round 5 of the Insatiable on 67 of 76: six turns to go is 18 Doom, and the boss will have taken half our HP by then.
        self.assertEqual(pick(combat_state([surge, STRIKE(1)], [{**enemy("A", 175, attack=20), "max_hp": 321}], hp=67, max_hp=76, kind="boss", round_no=5)), 1)
        late["player"]["hp"] = 25  # ... which is too much of 25
        self.assertEqual(pick(late), 1)
        late["player"]["hp"] = 60
        late["player"]["status"] = [power("DOOM", 15), power("NEUROSURGE", 3)]  # a second one on top
        self.assertEqual(pick(late), 1)
        # 75 HP with 66 Doom and 3 more on the way is 6 HP as far as the plan goes.
        hurt = combat_state([STRIKE(0)], [enemy("A", 300, attack=10)], hp=75, kind="boss")
        self.assertEqual(combat._context(hurt)["hp"], 75)
        hurt["player"]["status"] = [power("DOOM", 66), power("NEUROSURGE", 3)]
        self.assertEqual(combat._context(hurt)["hp"], 6)

    def test_ostys_attacks_do_nothing_while_he_is_dead(self):
        self.assertEqual(combat.choose_card(self.state([self.POKE(0)], osty=3)), ("play_card", {"card_index": 0, "target": "SLUG_0"}))
        self.assertIsNone(combat.choose_card(self.state([self.POKE(0)], osty=0)))
        self.assertEqual(pick(self.state([self.SHARDS(0), self.POKE(1)], osty=1)), 1)  # Bone Shards kills him: a Poke after it is wasted, so the Poke goes first
        bare = self.state([self.POKE(0)], osty=0)
        del bare["player"]["pets"]  # without the pet in the state nothing is assumed about him
        self.assertIsNotNone(combat.choose_card(bare))

    def test_bone_shards_gives_up_the_hp_osty_stood_in_front_with(self):
        s = self.state([self.SHARDS(0)], osty=10, attack=12)
        self.assertEqual((outcome(s).lost, outcome(s, s["player"]["hand"]).lost), (2, 3))  # Osty soaks 10 of 12; after it, 9 Block instead of his 10 HP

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
        foe = {"entity_id": "TEST_SUBJECT_0", "hp": 97, "max_hp": 200, "block": 0, "intents": [{"type": "Attack", "label": "10x6"}], "status": [power("PAINFUL_STABS")]}
        state = combat_state([wail], [foe], energy=1, hp=33, max_hp=128, block=26, kind="boss")
        self.assertEqual(combat.expected_end_loss(state), 34)  # dead as it stands
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0}))  # six hits of 4: all behind the Block

    def test_an_attack_on_all_that_saps_them_counts_the_sap(self):
        crush = card(0, "CRUSH_UNDER", "Crush Under", "Attack", 1, "Deal 8 damage to ALL enemies. All enemies lose 1 Strength this turn.", "AllEnemies")
        state = combat_state([crush], [enemy("A", 50, attack="3x4"), enemy("B", 50, attack=10)], energy=1, hp=40)
        self.assertEqual(outcome(state, [crush]).lost, 2 * 4 + 9)


class Malaise(unittest.TestCase):
    def test_x_amounts_are_read_per_energy(self):
        malaise = card(0, "MALAISE", "Malaise", "Skill", "X", "Enemy loses X Strength. Apply X Weak. Exhaust.", "AnyEnemy")
        info = parse(malaise)
        self.assertEqual((info.is_x, info.enemy_strength_loss, info.weak, info.known), (True, 1, 1, True))
        # Three energy into a 5x6 attack: 2x6, and a quarter off that for Weak.
        self.assertEqual(combat.choose_card(combat_state([malaise], [enemy("A", 80, attack="5x6")], energy=3, hp=30)), ("play_card", {"card_index": 0, "target": "A"}))


class DeadOnPaper(unittest.TestCase):
    """Aeonglass on 51 HP about to hit for 40, the bot on 26 behind 11 Block with a Wither in hand."""

    def state(self, boss_hp, hail=True):
        wither = card(2, "WITHER", "Wither+4", "Status", 0, "Unplayable. At the end of your turn, if this is in your Hand, take 15 damage.", "Self")
        hand = [card(0, "COOLHEADED", "Coolheaded", "Skill", 1, "Channel 1 Frost. Draw 1 card.", "Self"), card(1, "MOMENTUM_STRIKE", "Momentum Strike+", "Attack", 0, "Deal 19 damage.", "AnyEnemy"),
                {**wither, "can_play": False, "unplayable_reason": "HasUnplayableKeyword"}, card(3, "DEFEND_B", "Defend", "Skill", 1, "Gain 8 Block.", "Self"),
                card(4, "FOCUSED_STRIKE", "Focused Strike+", "Attack", 1, "Deal 15 damage.", "AnyEnemy")]
        orbs = [{"id": f"{k}_ORB", "passive_val": p, "evoke_val": 8} for k, p in (("FROST", 2), ("GLASS", 4), ("LIGHTNING", 3))]
        boss = enemy("AEONGLASS_0", boss_hp, 40, status=[power("WITHERING_PRESENCE")])
        return combat_state(hand, [boss], energy=3, hp=26, max_hp=89, block=11, kind="boss", round_no=13, orbs=orbs, orb_slots=3, status=[power("HAILSTORM", 12)] if hail else [])

    def test_hailstorm_and_the_orbs_finish_the_boss_before_it_hits(self):
        # 19 + 15 from the cards, 12 from Hailstorm, 4 and 3 from the orbs: 53 on a boss with 51.
        plan: dict = {}
        self.assertIsNotNone(combat.choose_card(self.state(51), plan))
        self.assertLess(plan["lost"], 26)

    def test_far_past_saving_the_bot_still_swings(self):
        # Every card brings the next Wither, so playing nothing was "least dead": the turn ended with two attacks paid for.
        state = self.state(200, hail=False)
        state["player"]["hp"] = 20  # 24 short with nothing played: no Block in this hand mends that
        state["player"]["hand"] = state["player"]["hand"][1:]  # no card that draws
        self.assertIn(pick(state), (1, 4))


class LostOnPaper(unittest.TestCase):
    """When every line ends in our death, Block that leaves us short is worth nothing: the kill is."""

    def state(self, hp):
        swing = lambda i: card(i, "SWING", "Swing", "Attack", 1, "Deal 12 damage.", "AnyEnemy")
        guard = card(2, "GUARD", "Guard", "Skill", 1, "Gain 8 Block.", "Self")
        return combat_state([swing(0), swing(1), guard], [enemy("QUEEN_0", 30, attack=35)], energy=2, hp=hp, max_hp=80, kind="boss")

    def test_the_bot_goes_for_the_kill_it_cannot_quite_count(self):
        # 15 HP in front of 35: one attack and the Block lose 27, both attacks lose 35 and leave her on 6. Dead either way by
        # the count; only the second can turn out a kill.
        state = self.state(15)
        self.assertEqual(pick(state), 0)
        state["player"]["hand"] = [dict(c, index=n) for n, c in enumerate(state["player"]["hand"][1:])]
        state["player"]["energy"] = 1
        state["battle"]["enemies"][0]["hp"] = 18
        self.assertEqual(pick(state), 0)  # the second attack, not the Block

    def test_block_that_saves_us_is_still_played(self):
        plan: dict = {}
        combat.choose_card(self.state(30), plan)  # 30 HP: an attack and the Block lose 27
        self.assertEqual(plan["lost"], 27)

    def test_reach_is_what_has_to_die_for_the_blow_not_to_land(self):
        foes = read_foes(enemy("QUEEN_0", 300), enemy("AMALGAM_0", 12, attack=40, block=3, status=[power("MINION"), power("POISON", 5)]))
        self.assertEqual(combat.kill_reach(foes, True), 10)  # the minion that attacks: 12 less 5 Poison, and 3 Block
        self.assertEqual(combat.kill_reach(foes, False), 300)  # something else kills us too: only the fight's end helps
        self.assertEqual(combat.kill_reach(read_foes(enemy("FYSH_0", 9, attack=20, status=[power("INTANGIBLE")])), True), float("inf"))


class SmallReads(unittest.TestCase):
    def test_debilitate_doubles_vulnerable_for_the_cards_after_it(self):
        debilitate = card(0, "DEBILITATE", "Debilitate", "Attack", 1, "Deal 10 damage. Vulnerable and Weak are twice as effective against the enemy for the next 2 turns.", "AnyEnemy")
        slam = card(1, "SLAM", "Slam", "Attack", 1, "Deal 13 damage.", "AnyEnemy")
        # 15 for the Debilitate (Vulnerable), then 26 and not 19 for the Slam: 41 on 40 HP.
        plan: dict = {}
        state = combat_state([debilitate, slam], [enemy("A", 40, attack=30, status=[power("VULNERABLE", 2)])], energy=2, hp=50)
        self.assertEqual(combat.choose_card(state, plan), ("play_card", {"card_index": 0, "target": "A"}))
        self.assertEqual(plan["lost"], 0)

    def test_energy_for_next_turn_is_worth_a_small_attack_now(self):
        invoke = card(0, "INVOKE", "Invoke+", "Skill", 1, "Next turn, Summon 3 and gain [necrobinder_energy_icon.png][necrobinder_energy_icon.png][necrobinder_energy_icon.png].", "Self")
        drain = card(1, "DRAIN_POWER", "Drain Power", "Attack", 1, "Deal 7 damage.", "AnyEnemy")
        queen = enemy("QUEEN_0", 150, status=[power("VULNERABLE", 4)])  # she only buffs this turn
        self.assertEqual(combat.choose_card(combat_state([invoke, drain], [queen], energy=1, hp=35, max_hp=97, kind="boss", round_no=9)), ("play_card", {"card_index": 0}))

    def test_a_bolt_at_random_is_not_let_loose_next_to_a_crab_it_could_kill(self):
        rage = [power("CRAB_RAGE")]
        cool = card(0, "COOLHEADED", "Coolheaded", "Skill", 1, "Channel 1 Frost. Draw 1 card.", "Self")
        strike = card(1, "ULTIMATE_STRIKE", "Ultimate Strike", "Attack", 1, "Deal 12 damage.", "AnyEnemy")
        ball = card(2, "BALL_LIGHTNING", "Ball Lightning+", "Attack", 1, "Deal 9 damage. Channel 1 Lightning.", "AnyEnemy")
        bolt = {"id": "LIGHTNING_ORB", "name": "Lightning", "passive_val": 3, "evoke_val": 8}
        state = combat_state([cool, strike, ball], [enemy("CRUSHER_0", 3, status=rage), enemy("ROCKET_0", 20, attack=52, status=rage)], energy=4, hp=3, max_hp=85, block=7, kind="boss",
                             orbs=[bolt, bolt, bolt], orb_slots=3)
        # Coolheaded first pushes a Lightning out at one of the two: on the Crusher it kills, and the Rocket takes 99 Block with
        # its 52 still coming. The two attacks kill the Rocket first.
        first = combat.choose_card(state)
        self.assertEqual((first[1]["card_index"], first[1].get("target")), (1, "ROCKET_0"))

    def test_hits_on_all_enemies_stop_counting_once_one_crab_dies_of_them(self):
        rage = [power("CRAB_RAGE")]
        hand = [card(0, "EXTERMINATE", "Exterminate+", "Attack", 1, "Deal 10 damage to ALL enemies 4 times.", "AllEnemies"), card(1, "BEAT_INTO_SHAPE", "Beat into Shape", "Attack", 1, "Deal 11 damage.", "AnyEnemy"),
                card(2, "SOLAR_STRIKE", "Solar Strike", "Attack", 1, "Deal 15 damage.", "AnyEnemy"), card(3, "CRUSH_UNDER", "Crush Under", "Attack", 1, "Deal 14 damage.", "AnyEnemy")]
        # 15 on the Rocket and four rounds of 10 looks like both dead; the Rocket dies of the first round and the Crusher keeps
        # 29 HP behind 99 Block. 15 + 14 + 11 on the Crusher kills what is about to hit us.
        plan: dict = {}
        first = combat.choose_card(combat_state(hand, [enemy("CRUSHER_0", 39, attack=24, status=rage), enemy("ROCKET_0", 22, status=rage)], energy=3, hp=4, max_hp=91, kind="boss"), plan)
        self.assertEqual(first[1].get("target"), "CRUSHER_0")
        self.assertNotEqual(first[1]["card_index"], 0)
        self.assertEqual(plan["lost"], 0)


class Asleep(unittest.TestCase):
    """The Lagavulin Matriarch sleeps for three turns unless she loses HP: those turns are for setting up."""

    def state(self, turns, round_no=1):
        sleeper = {"entity_id": "LAGAVULIN_MATRIARCH_0", "hp": 222, "max_hp": 222, "block": 12, "status": [power("PLATING", 12), power("ASLEEP", turns)], "intents": [{"type": "Sleep", "label": ""}]}
        inflame = card(1, "INFLAME", "Inflame", "Power", 1, "Gain 2 Strength.", "Self")
        return combat_state([STRIKE(0), inflame, DEFEND(2), STRIKE(3)], [sleeper], energy=3, kind="boss", round_no=round_no)

    def test_she_is_left_asleep_while_the_powers_go_down(self):
        state = self.state(3)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 1}))
        state["player"]["hand"] = [STRIKE(0), DEFEND(1), STRIKE(2)]
        self.assertIsNone(combat.choose_card(state))  # nothing that lasts: the turn ends with the energy unspent

    def test_the_last_turn_of_her_sleep_is_for_hitting(self):
        state = self.state(1, round_no=3)
        state["player"]["hand"] = [STRIKE(0), DEFEND(1), STRIKE(2)]
        self.assertEqual(pick(state), 0)
        self.assertIsNotNone(combat.choose_card(dict(self.state(3, round_no=3))))  # ... and so is round 3 if the count does not come down by itself

    def test_nothing_is_thrown_at_a_sleeper(self):
        state = self.state(3)
        state["player"]["potions"] = [potion(0, "Fire Potion", "Deal 20 damage to an enemy.", "AnyEnemy"), potion(1, "Flex Potion", "Gain 5 Strength this turn.")]
        # (Nor drunk ourselves: Strength for this turn, with nothing to hit until she wakes, is poured out.)
        self.assertEqual(combat.potion_actions(state, 0), [])
        self.assertEqual(sorted(a[1]["slot"] for a in combat.potion_actions(self.state(1, round_no=3) | {"player": state["player"]}, 0)), [0, 1])


class SlipperyPotions(unittest.TestCase):
    def test_strength_is_not_drunk_while_every_hit_takes_one_hp(self):
        vantom = enemy("VANTOM_0", 172, attack=7, status=[power("SLIPPERY", 7)])
        state = combat_state([STRIKE(0)], [vantom], kind="boss", potions=[potion(0, "Flex Potion", "Gain 5 Strength this turn."), potion(1, "Swift Potion", "Draw 3 cards.")])
        self.assertEqual([a[1]["slot"] for a in combat.potion_actions(state, 0)], [1])
        vantom["status"] = []
        self.assertEqual([a[1]["slot"] for a in combat.potion_actions(state, 0)], [0, 1])


class Debris(unittest.TestCase):
    """Crash Landing fills the hand with Debris (1 energy: Exhaust), which stays in the deck for the fight."""

    CRASH = staticmethod(lambda i: card(i, "CRASH_LANDING", "Crash Landing", "Attack", 1, "Deal 21 damage to ALL enemies. Fill your Hand with Debris.", "AllEnemies"))

    def line(self, enemies, kind="boss", round_no=2):
        s = combat_state([STRIKE(0), self.CRASH(1), DEFEND(2), DEFEND(3), STRIKE(4)], enemies, energy=3, hp=70, max_hp=75, kind=kind, round_no=round_no, draw_pile_count=9, discard_pile_count=5)
        return [c["index"] for c in play_turn(s)]

    def test_it_is_read_as_filling_the_hand(self):
        self.assertEqual(parse(self.CRASH(0)).debris, -1)
        self.assertEqual(parse(card(0, "COLLISION_COURSE", "Collision Course", "Attack", 0, "Deal 10 damage. Add a Debris into your Hand.", "AnyEnemy")).debris, 1)

    def test_early_in_a_boss_fight_it_stays_in_the_hand(self):
        # Six dead cards in a deck of nineteen, for eight more rounds: Regent bosses with Crash Landing in the deck were won
        # 1 time in 6 (22 in 34 without).
        self.assertNotIn(1, self.line([enemy("LAGAVULIN_MATRIARCH_0", 200, attack=14)]))

    def test_it_finishes_a_fight_and_clears_a_room(self):
        self.assertEqual(self.line([enemy("LAGAVULIN_MATRIARCH_0", 20, attack=14)])[0], 1)  # the kill: no later to pay for
        self.assertIn(1, self.line([enemy("LAGAVULIN_MATRIARCH_0", 60, attack=14)], round_no=9))  # the fight is nearly done
        self.assertEqual(self.line([enemy("A_0", 30, attack=8), enemy("B_0", 28, attack=7), enemy("C_0", 25, attack=6)], kind="monster", round_no=1)[0], 1)

    def test_debris_is_cleared_with_energy_left_over(self):
        s = combat_state([STRIKE(0), card(1, "DEBRIS", "Debris", "Status", 1, "Exhaust.", "None")], [enemy("LAGAVULIN_MATRIARCH_0", 200)], energy=2, kind="boss")
        self.assertEqual([c["index"] for c in play_turn(s)], [0, 1])


class Imbalanced(unittest.TestCase):
    def test_the_rocks_attack_is_blocked_whole(self):
        # Bowlbug (Rock): "If its attacks are fully blocked, it becomes Stunned." Three Defends stop its 15 and take its next
        # turn away; two of them and an attack only save 10 HP.
        big = card(0, "BLUDGEON", "Bludgeon", "Attack", 1, "Deal 16 damage.", "AnyEnemy")
        rock = lambda status: combat_state([big, DEFEND(1), DEFEND(2), DEFEND(3)], [enemy("BOWLBUG_ROCK_0", 60, attack=15, status=status)], energy=3, hp=70, max_hp=75)
        self.assertEqual(sorted(c["index"] for c in play_turn(rock([power("IMBALANCED")]), block=True)), [1, 2, 3])
        self.assertEqual(sorted(c["index"] for c in play_turn(rock([]), block=True)), [0, 1, 2])


class Skittish(unittest.TestCase):
    def test_each_gardener_is_hit_once_a_turn(self):
        # "The first time it is hit each turn, it gains 6 Block": a second Strike on the same one does nothing.
        foes = [enemy("PHANTASMAL_GARDENER_0", 29, status=[power("SKITTISH", 6)]), enemy("PHANTASMAL_GARDENER_1", 30, status=[power("SKITTISH", 6)])]
        state = combat_state([STRIKE(0), STRIKE(1)], foes, energy=2)
        first = combat.choose_card(state)[1]
        foes[int(first["target"][-1])].update(hp=23, block=6)  # hit once: 6 HP gone, 6 Block up
        state["player"]["hand"], state["player"]["energy"] = [STRIKE(1)], 1
        self.assertNotEqual(aimed(state), first["target"])


class Reattach(unittest.TestCase):
    def segments(self, front, middle, back):
        names = ("DECIMILLIPEDE_SEGMENT_FRONT_0", "DECIMILLIPEDE_SEGMENT_MIDDLE_0", "DECIMILLIPEDE_SEGMENT_BACK_0")
        return combat_state([STRIKE(0)], [enemy(n, hp, attack=6, status=[power("REATTACH", 25)]) for n, hp in zip(names, (front, middle, back)) if hp], energy=1, kind="elite")

    def test_a_segment_is_not_killed_while_the_others_are_far_from_dead(self):
        # "If other segments are still alive, revives in 2 turns with 25 HP": the Strike that would kill the middle one (7 HP)
        # goes into the healthiest instead, until all three can die within two turns.
        self.assertEqual(aimed(self.segments(46, 7, 44)), "DECIMILLIPEDE_SEGMENT_FRONT_0")

    def test_the_segments_are_finished_once_the_rest_is_within_reach(self):
        self.assertEqual(aimed(self.segments(20, 5, 18)), "DECIMILLIPEDE_SEGMENT_MIDDLE_0")
        self.assertEqual(aimed(self.segments(0, 5, 0)), "DECIMILLIPEDE_SEGMENT_MIDDLE_0")  # the last one standing: the fight


class BeatingRemnant(unittest.TestCase):
    def test_a_turn_costs_no_more_than_the_relic_allows(self):
        # "You cannot lose more than 20 HP in a single turn": 5 Block in front of 40 saves nothing, the Strike counts.
        state = combat_state([STRIKE(0), DEFEND(1)], [enemy("A", 60, attack=40)], energy=1, hp=70)
        self.assertEqual(pick(state), 1)
        state["player"]["relics"] = [{"id": "BEATING_REMNANT", "description": "You cannot lose more than 20 HP in a single turn."}]
        plan: dict = {}
        self.assertEqual(pick(state, plan), 0)
        self.assertEqual(plan["lost"], 20)


class VoidForm(unittest.TestCase):
    FORM = lambda self, i: card(i, "VOID_FORM", "Void Form", "Power", 3, "Ethereal. End your turn. The first 2 cards you play each turn are free to play.")

    def test_the_card_that_ends_the_turn_is_played_last(self):
        # Played first, as Powers are, it ended round 1 of the Queen with two potions drunk and 62 damage in hand.
        state = combat_state([self.FORM(0), STRIKE(1), DEFEND(2)], [enemy("QUEEN_0", 400, attack=10)], energy=5, kind="boss")
        self.assertNotEqual(pick(state), 0)
        state["player"]["hand"], state["player"]["energy"] = [self.FORM(0)], 3
        self.assertEqual(pick(state), 0)

    def test_its_free_plays_go_to_the_dearest_cards(self):
        # With free plays left the hand shows every cost as 0: the two free ones are the 3 and the 2, not two Strikes.
        free = lambda c: {**c, "cost": "0"}
        blow = card(2, "KNOCKOUT_BLOW", "Knockout Blow", "Attack", 3, "Deal 30 damage.", "AnyEnemy")
        blade = card(3, "SOVEREIGN_BLADE", "Sovereign Blade", "Attack", 2, "Deal 18 damage.", "AnyEnemy")
        state = combat_state([free(STRIKE(0)), free(STRIKE(1)), free(blow), free(blade)], [enemy("A", 300)], energy=2, kind="boss", status=[power("VOID_FORM", 2)],
                             _costs={"Strike": 1, "Knockout Blow": 3, "Sovereign Blade": 2})
        self.assertEqual(combat.free_plays(state), 2)
        self.assertIn(pick(state), (2, 3))
        # Once they are used up the costs show again, and nothing is free.
        state["_plays_this_turn"], state["player"]["hand"] = 2, [STRIKE(0), STRIKE(1), blow]
        self.assertEqual(combat.free_plays(state), 0)
        self.assertIn(pick(state), (0, 1))
        self.assertEqual(combat.free_plays({**state, "_plays_this_turn": 0}), 0)  # (a hand that shows a cost says so, whatever the bot has counted)


class ThievingHopper(unittest.TestCase):
    def hopper(self, flutter, attack=21):
        return enemy("THIEVING_HOPPER_0", 40, attack=attack, status=[power("ESCAPE_ARTIST", 3), power("SWIPE"), power("FLUTTER", flutter)])

    def test_the_last_flutter_hit_stuns_it(self):
        # "Deal attack damage 5 times to Stun it": two hits from that, the Strikes are this turn's Block and more.
        turn = lambda flutter: combat_state([DEFEND(0), DEFEND(1), DEFEND(2), STRIKE(3), STRIKE(4)], [self.hopper(flutter)], energy=3, hp=50)
        plan: dict = {}
        self.assertIn(pick(turn(2), plan), (3, 4))
        self.assertEqual(plan["lost"], 0)
        combat.choose_card(turn(5), plan)  # five hits away with two attacks in hand, the 21 has to be blocked as before
        self.assertGreater(plan["lost"], 0)

    def test_attacks_do_half_until_then_and_what_is_not_an_attack_does_not(self):
        foe = read_foes(self.hopper(2))[0]
        self.assertEqual(combat._hit(foe, 10, 1), 5)
        self.assertEqual(combat._hit(foe, 10, 1, raw=True), 10)  # (an orb, a Power's damage: not an Attack, and it does not count)
        self.assertEqual((foe.flutter, foe.incoming), (1, 21))
        self.assertEqual(combat._hit(foe, 10, 2), 5 + 10)  # the hit that Stuns it is the last one halved
        self.assertEqual((foe.flutter, foe.incoming, foe.damage_mult), (0, 0, 1.0))

    def test_the_one_holding_our_card_is_worth_killing(self):
        foe = read_foes(self.hopper(5))[0]
        self.assertTrue(foe.thief)
        self.assertGreater(foe.worth, 1.0)

    def test_curl_up_is_block_after_the_first_hit(self):
        foe = read_foes(enemy("LOUSE_PROGENITOR_0", 60, status=[power("CURL_UP", 14)]))[0]
        self.assertEqual(combat._hit(foe, 6, 2), 6)  # the second Strike of the turn meets the 14 Block
        self.assertEqual(foe.block, 8)


class StunsAndShells(unittest.TestCase):
    def test_the_terror_eel_is_stunned_as_its_hp_crosses_the_shriek(self):
        # "The first time Terror Eel's HP reaches 70 or below, it becomes Stunned": the Strike is this turn's Block.
        plan: dict = {}
        state = combat_state([DEFEND(0), STRIKE(1)], [enemy("TERROR_EEL_0", 75, attack=22, status=[power("SHRIEK", 70)])], energy=1, hp=40, kind="elite")
        self.assertEqual(pick(state, plan), 1)
        self.assertEqual(plan["lost"], 0)

    def test_a_hardened_shell_lets_through_what_it_has_left_this_turn(self):
        colony = lambda left: enemy("SKULKING_COLONY_0", 60, attack=14, status=[power("HARDENED_SHELL", left)])
        foe = read_foes(colony(8))[0]
        self.assertEqual(combat._hit(foe, 6, 2), 8)
        self.assertEqual(combat._hit(foe, 6, 1), 0)
        # With nothing left to take off it this turn, the energy goes to Block.
        self.assertEqual(pick(combat_state([STRIKE(0), DEFEND(1)], [colony(0)], energy=1, hp=40, kind="elite")), 1)
        self.assertEqual(read_foes(enemy("A", 60))[0].shell, -1)


class PlayedAgain(unittest.TestCase):
    BLADE = lambda self, i: card(i, "SOVEREIGN_BLADE", "Sovereign Blade", "Attack", 1, "Retain. Deal 30 damage. Replay 1.", "AnyEnemy")

    def test_replay_is_read_from_the_card_and_not_from_one_that_hands_it_out(self):
        self.assertEqual(parse(self.BLADE(0)).replay, 1)
        sage = card(0, "SWORD_SAGE", "Sword Sage", "Power", 2, "Sovereign Blade gains Replay 1.")
        self.assertEqual(parse({**sage, "keywords": [{"name": "Replay", "description": "Plays this card an additional time."}]}).replay, 0)
        self.assertEqual(parse({**STRIKE(0), "keywords": [{"name": "Replay", "description": "Plays this card an additional 2 times."}]}).replay, 2)

    def test_a_card_played_twice_is_two_of_the_three_sloth_allows(self):
        # Against the Knowledge Demon the Blade took Sloth from 0 to 2, the plan's third card could not be played, and 41 HP went on 42.
        foes = read_foes(enemy("KNOWLEDGE_DEMON_0", 160, attack=36))
        blade, strike, guard = parse(self.BLADE(0)), parse(STRIKE(1)), parse(DEFEND(2))
        kw = ctx(energy=4, hp=42, max_hp=108, fight="boss", card_limit=3)
        self.assertIsNone(combat.simulate([blade, strike, guard], foes, **kw))
        self.assertIsNotNone(combat.simulate([blade, guard], foes, **kw))
        self.assertIsNotNone(combat.simulate([strike, guard, blade], foes, **kw))  # (as the third it still goes)
        self.assertEqual(gain([blade], foes, **kw) > 60 * 1.5 - 1, True)
        # The count is the game's own, the number on the power.
        state = combat_state([self.BLADE(0), STRIKE(1), DEFEND(2)], [enemy("KNOWLEDGE_DEMON_0", 160, attack=36)], energy=4, hp=42, kind="boss", status=[power("SLOTH", 2)])
        self.assertEqual(combat.cards_left_this_turn(state), 1)
        self.assertEqual(combat.cards_left_this_turn({**state, "_plays_this_turn": 1}), 1)

    def test_the_last_card_sloth_allows_is_not_one_that_only_draws(self):
        # Sweeping Gaze, Soul and Soul+ were played with five energy in hand, 10 damage dealt and 13 coming.
        soul = lambda i: card(i, "SOUL", "Soul", "Skill", 0, "Draw 2 cards. Exhaust.", "Self")
        state = combat_state([soul(0), soul(1), STRIKE(2)], [enemy("KNOWLEDGE_DEMON_0", 190, attack=13)], energy=5, hp=32, kind="boss", status=[power("SLOTH", 2)])
        self.assertEqual(pick(state), 2)  # one play left: the Strike
        state["player"]["status"] = [power("SLOTH", 3)]
        self.assertIsNone(combat.choose_card(state))
        foes, kw, one, two = combat.read_enemies(state), ctx(energy=5, hp=32, fight="boss"), parse(soul(0)), parse(soul(1))
        free = combat.simulate([one, two], foes, **kw).score - combat.simulate([one], foes, **kw).score
        last = combat.simulate([one, two], foes, **kw, card_limit=2).score - combat.simulate([one], foes, **kw, card_limit=2).score
        self.assertGreater(free, last + 1.5)  # the second Soul's cards can no longer be played

    def test_under_echo_form_the_first_card_of_a_turn_is_the_one_worth_two(self):
        heavy = card(1, "BLUDGEON", "Bludgeon", "Attack", 2, "Deal 14 damage.", "AnyEnemy")
        state = combat_state([STRIKE(0), heavy], [enemy("A", 300)], energy=3, kind="boss", status=[power("ECHO_FORM")])
        plan: dict = {}
        self.assertEqual(pick(state, plan), 1)
        twice = plan["score"]
        combat.choose_card({**state, "_plays_this_turn": 1}, plan)
        self.assertGreater(twice, plan["score"] + 14)  # (the second card of the turn is played once)


class DoomAndAttacksTogether(unittest.TestCase):
    """Doom is health already gone: it and the attacks go on the same enemy, and not on one it has finished."""

    SCOURGE = staticmethod(lambda i: card(i, "SCOURGE", "Scourge", "Skill", 1, "Apply 13 Doom. Draw 1 card.", "AnyEnemy"))

    def plays(self, hand, foes, energy=3):
        """(card name, target) of every card of the turn, the enemies taking what each card did (near enough for the next choice)."""
        out = []

        def did(c, params):
            out.append((c["name"], params.get("target")))
            for e in foes:
                if e["entity_id"] == params.get("target") and c["name"] == "Scourge":
                    e["status"] = [power("DOOM", 13 + sum(p["amount"] for p in e["status"]))]
                elif e["entity_id"] == params.get("target"):
                    e["hp"] -= 6

        play_turn(combat_state(hand, foes, energy=energy, hp=49, max_hp=66), limit=99, after=did)
        return out

    def test_scourge_and_the_strikes_go_on_one_nibbit(self):
        # The Doom went on the Nibbit with 43 HP, the attacks on the one with 42.
        foes = [enemy("NIBBIT_0", 42, attack=6), enemy("NIBBIT_1", 43)]
        self.assertEqual(len({t for _name, t in self.plays([self.SCOURGE(0), STRIKE(1), STRIKE(2)], foes)}), 1)

    def test_an_enemy_doom_has_finished_is_left_alone(self):
        # 21 HP under 26 Doom dies when its turn ends; two Strikes do not kill it sooner, and the other one is still to do.
        foes = [enemy("NIBBIT_0", 21, attack=10, status=[power("DOOM", 26)]), enemy("NIBBIT_1", 28, attack=10)]
        self.assertEqual({t for _name, t in self.plays([STRIKE(0), STRIKE(1)], foes, energy=2)}, {"NIBBIT_1"})
        # ... but an attack that kills it now stops its hit.
        foes = [enemy("NIBBIT_0", 6, attack=10, status=[power("DOOM", 26)]), enemy("NIBBIT_1", 28, attack=10)]
        self.assertEqual(self.plays([STRIKE(0)], foes, energy=1), [("Strike", "NIBBIT_0")])

    def test_more_doom_goes_on_the_one_that_still_needs_it(self):
        foes = [enemy("NIBBIT_0", 21, attack=10, status=[power("DOOM", 26)]), enemy("NIBBIT_1", 40, attack=10)]
        self.assertEqual(self.plays([self.SCOURGE(0)], foes, energy=1), [("Scourge", "NIBBIT_1")])
