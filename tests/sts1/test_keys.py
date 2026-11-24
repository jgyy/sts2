"""The first game: the numbers the game works out for a card in hand, its powers in the planner's words, and the three
keys that open act 4 (games/sts1/keys.py)."""

from sts2bot import cards
from sts2bot.games.sts1 import keys
from sts2bot.games.sts1.screens import Screens
from sts2bot.run import strategy

from .helpers import Sts1Case


def _game(**kw) -> dict:
    g = {"act": 3, "floor": 40, "current_hp": 70, "max_hp": 80, "final_act_available": True,
         "keys": {"ruby": False, "emerald": False, "sapphire": False}}
    g.update(kw)
    return g


class LiveNumbers(Sts1Case):
    def card(self, cid, ctype, upgrades=0, **numbers):
        c = {"id": cid, "name": cid, "type": ctype, "upgrades": upgrades, "cost": 1, **numbers}
        return Screens().card(c, 0, live=True)

    def test_a_hand_card_reads_the_numbers_the_game_worked_out(self):
        # Weak and 5 Strength: the printed 6 is not what the Strike deals; Frail and Dexterity move a Defend's Block.
        self.assertIn("Deal 4 damage", self.card("Strike_R", "ATTACK", damage=4, base_damage=6)["description"])
        self.assertIn("Gain 3 Block", self.card("Defend_R", "SKILL", block=3, base_block=5)["description"])
        wave = self.card("Iron Wave", "ATTACK", upgrades=1, damage=9, base_damage=7, block=10, base_block=7)["description"]
        self.assertEqual(wave, "Gain 10 Block. Deal 9 damage.")

    def test_heavy_blade_is_one_hit(self):
        # Read as "14 damage 5 times" it was a 70-damage card for 2 energy, in hand and at every reward.
        blade = self.card("Heavy Blade", "ATTACK", upgrades=1, damage=24, base_damage=14)
        info = cards.parse(blade)
        self.assertEqual((info.damage, info.hits), (24, 1))
        printed = cards.parse(Screens().card({"id": "Heavy Blade", "name": "Heavy Blade", "type": "ATTACK", "upgrades": 1, "cost": 2}))
        self.assertEqual((printed.damage, printed.hits), (14, 1))

    def test_without_the_mods_numbers_the_printed_text_stands(self):
        self.assertIn("Deal 6 damage", self.card("Strike_R", "ATTACK")["description"])

    def test_a_power_s_damage_is_left_alone(self):
        # A Skill's or Power's own numbers are not the card's damage (Flame Barrier's 4 back, Combust).
        barrier = self.card("Flame Barrier", "SKILL", block=15, base_block=12, damage=-1, base_damage=-1)["description"]
        self.assertIn("Gain 15 Block", barrier)
        self.assertIn("deal 4 damage back", barrier)


class Tiers(Sts1Case):
    def test_the_first_game_s_cards_have_tiers_of_their_own(self):
        from sts2bot.games.sts1 import text, tiers
        from sts2bot.run import crowd

        self.assertEqual([k for k in tiers.TIER if k not in text.CARDS], [])  # (every name is a card the bot can read)
        self.assertEqual(crowd.rating("UNDERHANDED_STRIKE"), tiers.C)  # (by the game's id: Sneaky Strike)
        self.assertGreater(crowd.rating("ACROBATICS"), crowd.rating("POISONED_STAB"))
        self.assertIsNone(crowd.rating("STRIKE_R"))

    def test_a_card_that_can_only_be_played_sometimes_is_not_free_damage(self):
        # Clash topped the first STS1 rewards at 9.7 (14 damage for 0 energy on paper; dead beside Defends).
        clash = Screens().card({"id": "Clash", "name": "Clash", "type": "ATTACK", "rarity": "COMMON", "cost": 0})
        clap = Screens().card({"id": "Thunderclap", "name": "Thunderclap", "type": "ATTACK", "rarity": "COMMON", "cost": 1})
        self.assertLess(cards.deck_value(clash), cards.deck_value(clap) + 0.5)


class Powers(Sts1Case):
    def test_first_game_powers_in_the_planner_s_words(self):
        read = Screens.powers({"powers": [{"id": "Anger", "amount": 2}, {"id": "Hex", "amount": 1}, {"id": "Flight", "amount": 3},
                                          {"id": "Sharp Hide", "amount": 3}, {"id": "Generic Strength Up Power", "amount": 3}]})
        self.assertEqual([p["id"] for p in read], ["ENRAGE_POWER", "HEX_DAZED_POWER", "FLUTTER_POWER", "THORNS_POWER", "RITUAL_POWER"])

    def test_a_darkling_comes_back_on_half_its_hp(self):
        (link,) = Screens.powers({"max_hp": 55, "powers": [{"id": "Life Link", "amount": -1}]})
        self.assertEqual((link["id"], link["amount"]), ("REATTACH_POWER", 27))


class Keys(Sts1Case):
    def test_no_key_matters_while_act_4_is_shut(self):
        self.assertEqual(keys.missing(_game(final_act_available=False)), set())
        self.assertFalse(keys.wants_ruby(_game(final_act_available=False)))

    def test_recall_in_act_3_with_hp_to_spare(self):
        self.assertTrue(keys.wants_ruby(_game()))
        self.assertFalse(keys.wants_ruby(_game(current_hp=40)))  # (the heal first)
        self.assertTrue(keys.wants_ruby(_game(current_hp=46, floor=49)))  # the last rest site: no other chance
        self.assertFalse(keys.wants_ruby(_game(act=2)))
        self.assertFalse(keys.wants_ruby(_game(keys={"ruby": True})))

    def test_the_rest_site_shows_recall_only_when_it_is_wanted(self):
        s = Screens()
        s.raw = {"in_game": True, "game_state": {**_game(), "screen_type": "REST", "screen_state": {"rest_options": ["rest", "smith", "recall"]}}}
        self.assertEqual([o["id"] for o in s._rest()["rest_site"]["options"]], ["recall"])
        s.raw["game_state"]["current_hp"] = 30
        self.assertEqual([o["id"] for o in s._rest()["rest_site"]["options"]], ["rest", "smith"])

    def test_the_act_3_chest_gives_the_sapphire_key_over_its_relic(self):
        relic = {"id": "Anchor", "name": "Anchor"}
        rewards = [{"reward_type": "GOLD", "gold": 30}, {"reward_type": "RELIC", "relic": relic}, {"reward_type": "SAPPHIRE_KEY", "link": relic}]
        s = Screens()
        s.raw = {"in_game": True, "game_state": {**_game(), "screen_type": "COMBAT_REWARD", "screen_state": {"rewards": rewards}}}
        self.assertEqual([r["type"] for r in s._combat_reward()["rewards"]["items"]], ["gold", "sapphire_key"])
        s.raw["game_state"]["act"] = 2
        self.assertEqual([r["type"] for r in s._combat_reward()["rewards"]["items"]], ["gold", "relic"])

    def test_the_burning_elite_is_worth_a_detour(self):
        s = Screens()
        game = {**_game(act=1), "screen_type": "MAP", "screen_state": {"next_nodes": [{"x": 0, "y": 0}, {"x": 2, "y": 0}]}, "map": [
            {"x": 0, "y": 0, "symbol": "M", "children": [{"x": 0, "y": 1}]}, {"x": 2, "y": 0, "symbol": "M", "children": [{"x": 2, "y": 1}]},
            {"x": 0, "y": 1, "symbol": "E", "children": []}, {"x": 2, "y": 1, "symbol": "E", "emerald_key": True, "children": []}]}
        s.raw = {"in_game": True, "game_state": game}
        nodes = {(n["col"], n["row"]): n["type"] for n in s._map()["map"]["nodes"]}
        self.assertEqual((nodes[(0, 2)], nodes[(2, 2)]), ("Elite", "EmeraldElite"))
        plain = strategy.path_value([("Monster", 9), ("Elite", 10)], 70, 80, 99, 1)
        burning = strategy.path_value([("Monster", 9), ("EmeraldElite", 10)], 70, 80, 99, 1)
        self.assertAlmostEqual(burning - plain, strategy.EMERALD_WORTH)
        game["keys"]["emerald"] = True
        self.assertNotIn("EmeraldElite", [n["type"] for n in s._map()["map"]["nodes"]])


class RunEnd(Sts1Case):
    def test_a_finished_room_is_not_the_end_of_the_run(self):
        # After the act 3 boss CommunicationMod says COMPLETE ("proceed" goes on to the victory room): read as game over,
        # the first win was written down as a loss on floor 50 and a win on floor 51.
        s = Screens()
        s.raw = {"in_game": True, "available_commands": ["proceed"], "game_state": {**_game(floor=50), "screen_type": "COMPLETE", "room_phase": "COMPLETE"}}
        self.assertEqual(s.translate(s.raw)["state_type"], "overlay")

    def test_a_victory_screen_does_not_lose_the_last_boss(self):
        from sts2bot.fight.bossfight import BossWatch
        from sts2bot.fightrecord import FightWatch

        boss = {"state_type": "boss", "run": {"act": 3, "floor": 50}, "battle": {"round": 5, "enemies": [{"entity_id": "DONU_0", "hp": 5, "max_hp": 250}]},
                "player": {"hp": 33, "max_hp": 87}}
        won = {"state_type": "game_over", "run": {"act": 3, "floor": 51}, "player": {"hp": 33}, "game_over": {"victory": True}}
        bosses, fights = BossWatch(), FightWatch()
        bosses.update(boss, "boss")
        fights.update(boss, "boss")
        self.assertIn(" won;", bosses.update(won, "game_over"))
        self.assertFalse(fights.update(won, "game_over")["died"])


class Scry(Sts1Case):
    def test_scry_discards_status_and_curse_cards_and_confirms(self):
        # The Watcher's first run (2026-11-24 21:52): the bot clicked Scry's three cards on and off for good.
        from sts2bot.games.sts1.client import Sts1Client

        cards = [{"name": "Defend", "uuid": "a", "type": "SKILL"}, {"name": "Dazed", "uuid": "b", "type": "STATUS"},
                 {"name": "Strike", "uuid": "c", "type": "ATTACK"}]
        picked, sent = {"a"}, []  # (a Defend already clicked on, as the stuck run had)

        def grid():
            ss = {"cards": cards, "selected_cards": [c for c in cards if c["uuid"] in picked], "any_number": True, "num_cards": 3}
            return {"in_game": True, "available_commands": ["choose", "confirm"],
                    "game_state": {"screen_type": "GRID", "current_action": "ScryAction", "room_phase": "COMBAT", "screen_state": ss}}

        class Fake(Sts1Client):
            def _message(self):
                return grid()

            def send(self, command):
                sent.append(command)
                if command == "confirm":
                    return {"status": "ok", "message": {"in_game": True, "available_commands": ["play", "end"], "game_state": {"screen_type": "NONE"}}}
                picked.symmetric_difference_update({cards[int(command.split()[1])]["uuid"]})
                return {"status": "ok", "message": grid()}

        Fake().state()
        self.assertEqual(sent, ["choose 0", "choose 1", "confirm"])
        self.assertEqual(picked, {"b"})


class EnergyLoss(Sts1Case):
    def test_a_power_that_takes_energy_every_turn_is_priced_for_it(self):
        # Fasting read as 7.8 (3 Strength, 3 Dexterity) and the Watcher's first run held two: 1 energy a turn.
        from sts2bot.run import decks

        fasting = Screens().card({"id": "Fasting2", "name": "Fasting", "type": "POWER", "rarity": "UNCOMMON", "cost": 2})
        strike = Screens().card({"id": "Strike_P", "name": "Strike", "type": "ATTACK", "rarity": "BASIC", "cost": 1})
        first = cards.deck_value(fasting) + decks.deck_fit(fasting, [strike] * 10, 1, 3)
        second = cards.deck_value(fasting) + decks.deck_fit(fasting, [strike] * 10 + [fasting], 1, 3)
        self.assertLess(first, cards.deck_value(fasting) - 3.0)
        self.assertLess(second, 0.0)

    def test_a_second_fasting_is_not_played(self):
        # 2026-11-24 21:54: two Fasting+ on round 1 and a third on round 3 left the Watcher 1 energy a turn, dead to a Chosen
        # from 79 of 79 HP. One already in play: the second is an energy of what is left, and the Strikes are worth more.
        from sts2bot.fight import combat
        from tests.helpers import STRIKE, combat_state, enemy

        fasting = Screens().card({"id": "Fasting2", "name": "Fasting", "type": "POWER", "rarity": "UNCOMMON", "cost": 2, "upgrades": 1}, 0)
        self.assertEqual(cards.parse(fasting).energy_drain, 1)
        s = combat_state([fasting, STRIKE(1), STRIKE(2)], [enemy("CHOSEN_0", 98, attack="5x2")], energy=2, kind="elite")
        s["player"]["status"] = Screens.powers({"powers": [{"id": "EnergyDownPower", "amount": 1}, {"id": "Strength", "amount": 4}]})
        self.assertEqual(s["player"]["status"][0]["id"], "ENERGY_DOWN_POWER")
        self.assertNotEqual(combat.choose_card(s)[1]["card_index"], 0)


class Curses(Sts1Case):
    def test_curses_that_hurt_at_the_end_of_the_turn_are_seen(self):
        # In the first game's words ("At the end of your turn, lose HP equal to ...") Regret cost 3-5 HP a turn the plans
        # counted as nothing; Decay and Burn too.
        from sts2bot.fight import battle

        hand = [Screens().card({"id": cid, "name": cid, "type": t, "cost": -2}, n)
                for n, (cid, t) in enumerate([("Regret", "CURSE"), ("Decay", "CURSE"), ("Burn", "STATUS"), ("Strike_R", "ATTACK")])]
        self.assertEqual(battle.hand_burns({"hand": hand}), ((0, -1, False), (1, 2, True), (2, 2, True)))


class TimeEater(Sts1Case):
    def test_the_twelfth_card_ends_the_turn(self):
        # 21:26 on 2026-11-24: 11 cards played, 30 HP, a 31 hit coming, 4 energy: Cold Snap was the 12th card and the last.
        from sts2bot.fight import battle, combat
        from tests.helpers import DEFEND, STRIKE, combat_state, enemy

        s = combat_state([STRIKE(0), DEFEND(1), STRIKE(2)], [enemy("TIMEEATER_0", 183, attack=31, status=[{"id": "TIME_WARP_POWER", "amount": 11}])],
                         energy=4, hp=30, block=4, kind="boss")
        self.assertEqual(battle.cards_left_this_turn(s), 1)
        self.assertEqual(combat.choose_card(s)[1]["card_index"], 1)


class PaidHp(Sts1Case):
    def test_no_card_that_costs_the_last_hp_even_with_regen_to_come(self):
        # 2026-11-24 21:43: on 5 HP with Regen 3 the planner read Offering+'s 6 as 3 and played it into a stunned boss: dead.
        from sts2bot.fight import combat
        from tests.helpers import combat_state, enemy

        def card(i, cid, ctype, cost, upgrades=0, **numbers):
            return Screens().card({"id": cid, "name": cid, "type": ctype, "cost": cost, "upgrades": upgrades, **numbers}, i, live=True)

        hand = [card(0, "Spot Weakness", "SKILL", 1, 1), card(1, "Wound", "STATUS", -2), card(2, "Defend_R", "SKILL", 1, 1, block=10, base_block=8),
                card(3, "Bash", "ATTACK", 2, 1, damage=21, base_damage=10), card(4, "Offering", "SKILL", 0, 1)]
        boss = enemy("BRONZEAUTOMATON_1", 75, status=[{"id": "ARTIFACT_POWER", "amount": 1}, {"id": "STRENGTH_POWER", "amount": 6}])
        boss["intents"] = [{"type": "Stun", "label": ""}]
        s = combat_state(hand, [boss], energy=2, hp=5, kind="boss")
        s["player"].update(draw_pile_count=10, status=[{"id": "REGEN_POWER", "amount": 3}, {"id": "STRENGTH_POWER", "amount": 11}])
        s["battle"]["round"] = 7
        self.assertNotEqual(combat.choose_card(s)[1]["card_index"], 4)

    def test_an_event_curse_in_the_first_game_s_words_costs_something(self):
        # "Become Cursed - Regret." cost nothing until 2026-11-24: Golden Shrine's Desecrate (275 Gold and Regret) beat Pray.
        from sts2bot.run import events

        def value(title, description):
            return events._event_option_value({"title": title, "description": description}, 60, 80, act=1)

        self.assertLess(value("Desecrate", "Gain 275 Gold. Become Cursed - Regret."), value("Pray", "Gain 100 Gold."))
        self.assertLess(value("Rummage", "Obtain a special Relic. Become Cursed - Pain."), value("Forge", "Upgrade a card in your deck."))


class WrithingMass(Sts1Case):
    def state(self, hp):
        from tests.helpers import combat_state, enemy

        def card(i, cid, ctype, cost, upgrades=0, **numbers):
            return Screens().card({"id": cid, "name": cid, "type": ctype, "cost": cost, "upgrades": upgrades, **numbers}, i, live=True)

        hand = [card(0, "Defend_P", "SKILL", 1, 1, block=8, base_block=8), card(1, "CrushJoints", "ATTACK", 0, 1, damage=10, base_damage=10),
                card(2, "Consecrate", "ATTACK", 0, 1, damage=8, base_damage=8), card(3, "Eruption", "ATTACK", 2, damage=9, base_damage=9),
                card(4, "ThirdEye", "SKILL", 1, block=7, base_block=7)]
        mass = enemy("WRITHINGMASS_0", 81, status=Screens.powers({"powers": [{"id": "Malleable", "amount": 3}, {"id": "Compulsive", "amount": -1}]}))
        mass["intents"] = [{"type": "Debuff", "label": ""}]
        s = combat_state(hand, [mass], energy=4, hp=hp, max_hp=89)
        s["battle"]["round"] = 4
        return s

    def test_a_hit_gives_it_a_new_intent(self):
        # 2026-11-24 22:07: on 12 HP, its intent a Debuff, the Watcher entered Wrath and hit it; it came round to 14 x 3.
        from sts2bot.fight import combat

        action = combat.choose_card(self.state(12))
        self.assertTrue(action is None or action[0] != "play_card" or action[1]["card_index"] in (0, 4))
        self.assertEqual(combat.choose_card(self.state(60))[1]["card_index"], 1)  # (with HP to spare it is still to be killed)


class AwakenedOne(Sts1Case):
    def test_its_first_death_is_not_the_end(self):
        from sts2bot.fight import battle, closing

        status = Screens.powers({"powers": [{"id": "Unawakened", "amount": -1}, {"id": "Curiosity", "amount": 1}]})
        e = {"entity_id": "AWAKENEDONE_2", "hp": 20, "max_hp": 300, "block": 0, "status": status, "intents": [{"type": "Attack", "label": "16"}]}
        (boss,) = battle.read_enemies({"battle": {"enemies": [e], "round": 9}, "player": {"hp": 50}, "state_type": "boss"})
        self.assertEqual((boss.respawns, boss.curious), (1, 1))
        self.assertEqual(closing.discount(30.0, 10, 50, [boss]), 1.0)  # (20 HP is not "dying next turn": 300 more come after)
