"""The first game's act 4: the Spire Shield and Spear (who is behind us, and turning to face a target), the Heart's Beat of
Death and Invincible in the planner's words, and the potions kept for the Heart."""

from sts2bot.fight import battle, combat, triggers
from sts2bot.games.sts1.screens import Screens
from sts2bot.run import potions

from ..helpers import DEFEND, STRIKE, combat_state, enemy
from .helpers import Sts1Case


def surrounded(shield=("ATTACK", 51, 1), spear=("BUFF", -1, 1), shield_behind=True):
    """The Shield and Spear as CommunicationMod sends them with the mod's sides: the Shield on our left, the Spear on our right.
    Each intent is (kind, damage as shown, hits); the one behind us shows its 50% more."""
    s = Screens()
    monster = lambda mid, hp, left, move: {"id": mid, "name": mid, "current_hp": hp, "max_hp": hp, "block": 0, "powers": [], "intent": move[0],
                                           "move_adjusted_damage": move[1], "move_hits": move[2], "left": left, "behind": left == shield_behind}
    s.raw = {"in_game": True, "available_commands": ["play", "end"], "game_state": {
        "act": 4, "floor": 53, "room_type": "MonsterRoomElite", "screen_type": "NONE", "room_phase": "COMBAT", "current_hp": 70, "max_hp": 80,
        "combat_state": {"turn": 3, "player": {"current_hp": 70, "max_hp": 80, "block": 0, "energy": 3, "powers": [{"id": "Surrounded", "amount": -1}]},
                         "hand": [], "draw_pile": [], "discard_pile": [], "exhaust_pile": [],
                         "monsters": [monster("SpireShield", 110, True, shield), monster("SpireSpear", 160, False, spear)]}}}
    return s


class ShieldAndSpear(Sts1Case):
    def test_each_side_is_marked_and_who_is_behind_comes_from_the_game(self):
        state = surrounded().combat()
        shield, spear = state["battle"]["enemies"]
        self.assertIn("BACK_ATTACK_LEFT_POWER", [p["id"] for p in shield["status"]])
        self.assertIn("BACK_ATTACK_RIGHT_POWER", [p["id"] for p in spear["status"]])
        read = {e.entity_id: e for e in battle.read_enemies({**state, "_facing": "SPIRESHIELD_0"})}  # (what we last aimed at does not overrule the game)
        self.assertEqual((read["SPIRESHIELD_0"].flanker, read["SPIRESHIELD_0"].behind, read["SPIRESPEAR_1"].behind), (True, True, False))

    def test_we_turn_to_face_the_one_about_to_hit_hardest(self):
        # The Shield's Smash is 34, 51 from behind, while the Spear buffs: a Strike aimed at the Shield turns us to face it. The other
        # way round (the Shield fortifies, the Spear's Skewer is 10x3 in front) the Strike goes at the Spear, though the Shield has
        # less HP: turning to the Shield would put the Skewer behind us (15x3).
        for shield, spear, target in ((("ATTACK", 51, 1), ("BUFF", -1, 1), "SPIRESHIELD_0"), (("DEFEND", -1, 1), ("ATTACK", 10, 3), "SPIRESPEAR_1")):
            with self.subTest(target):
                state = surrounded(shield, spear).combat()
                state["player"]["hand"] = [STRIKE(0)]
                action, params = combat.choose_card(state)
                self.assertEqual((action, params["target"]), ("play_card", target))

    def test_the_act_4_powers_read_as_the_planner_knows_them(self):
        read = Screens.powers({"powers": [{"id": "Invincible", "amount": 300}, {"id": "BeatOfDeath", "amount": 1}, {"id": "BackAttack", "amount": -1}]})
        self.assertEqual([p["id"] for p in read], ["HARDENED_SHELL_POWER", "BEAT_OF_DEATH_POWER", "BACK_ATTACK_POWER"])


def heart(invincible=300, beat=1, attack=None):
    status = [{"id": "HARDENED_SHELL_POWER", "amount": invincible}] + ([{"id": "BEAT_OF_DEATH_POWER", "amount": beat}] if beat else [])
    return enemy("CORRUPTHEART_0", 750, attack=attack, status=status)


class TheHeart(Sts1Case):
    def test_every_card_played_costs_its_beat_once_block_is_gone(self):
        state = combat_state([STRIKE(0), DEFEND(1)], [heart(beat=2)], kind="boss")
        ctx, foes = combat._context(state), battle.read_enemies(state)
        strike, defend = (combat.parse(c) for c in state["player"]["hand"])
        self.assertEqual(combat.simulate([strike], foes, **ctx).lost, 2)
        self.assertEqual(combat.simulate([defend, strike], foes, **ctx).lost, 0)  # (5 Block takes both beats)
        self.assertEqual(combat.simulate([strike, defend], foes, **ctx).lost, 2)  # (the first beat comes before any Block)

    def test_no_card_is_played_into_a_heart_that_can_lose_no_more_this_turn(self):
        # Invincible spent (300 lost this turn): a Strike takes nothing off it and its beat takes 1 HP off us.
        self.assertIsNone(combat.choose_card(combat_state([STRIKE(0)], [heart(invincible=0)], kind="boss")))
        self.assertIsNotNone(combat.choose_card(combat_state([STRIKE(0)], [heart(invincible=300)], kind="boss")))

    def test_damage_beyond_what_it_can_still_lose_counts_for_nothing(self):
        state = combat_state([STRIKE(0), STRIKE(1)], [heart(invincible=6, beat=0)], kind="boss")
        ctx, foes = combat._context(state), battle.read_enemies(state)
        one, two = combat.simulate([combat.parse(STRIKE(0))], foes, **ctx), combat.simulate([combat.parse(STRIKE(0)), combat.parse(STRIKE(1))], foes, **ctx)
        self.assertAlmostEqual(one.score, two.score)

    def test_the_way_through_act_4_keeps_the_belt_for_the_heart(self):
        self.assertEqual([potions.saving(f) for f in (50, 51, 52, 53, 54)], [False, True, True, True, False])


class SnakePlant(Sts1Case):
    def test_each_hit_that_hurts_it_buys_it_block_for_the_next_card(self):
        # Malleable 3 on 15 HP: three Strikes deal 6, then 6 - 3, then 6 - 4 (11, not 18), so the Chomp still comes.
        plant = enemy("SNAKEPLANT_0", 15, attack="7x3", status=[{"id": "MALLEABLE_POWER", "amount": 3}])
        plain = enemy("SNAKEPLANT_0", 15, attack="7x3")
        strikes = [STRIKE(0), STRIKE(1), STRIKE(2)]
        line = [combat.parse(c) for c in strikes]
        for foe, lost in ((plain, 0), (plant, 21)):
            state = combat_state(strikes, [foe])
            with self.subTest(malleable=foe is plant):
                self.assertEqual(combat.simulate(line, battle.read_enemies(state), **combat._context(state)).lost, lost)


class BlueCandle(Sts1Case):
    def test_necronomicurse_is_never_played_and_other_curses_cost_their_hp(self):
        # 2026-11-25 20:47: under Blue Candle it came back to the hand at every play, 1 HP each, energy unchanged: a loop.
        curse = Screens().card({"id": "Necronomicurse", "name": "Necronomicurse", "type": "CURSE", "cost": 0, "is_playable": True}, 1, live=True)
        self.assertIn("Lose 1 HP", curse["description"])
        state = combat_state([DEFEND(0), curse], [enemy("BYRD_0", 25, attack="2x5")], energy=3)
        action = combat.choose_card(state)
        self.assertNotEqual(action and action[1].get("card_index"), 1)
        self.assertTrue(triggers.shunned(combat.parse(curse), state))  # (whatever it would seem to offer)
        pain = Screens().card({"id": "Pain", "name": "Pain", "type": "CURSE", "cost": 0, "is_playable": True}, 0, live=True)
        self.assertEqual(combat.parse(pain).hp_loss, 1)
