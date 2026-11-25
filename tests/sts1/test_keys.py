"""The first game: the numbers the game works out for a card in hand, its powers in the planner's words, the three keys
that open act 4 (games/sts1/keys.py), and fights the first game's own words once misled the planner in."""

from sts2bot import cards
from sts2bot.fight import battle, closing, combat
from sts2bot.fight.bossfight import BossWatch
from sts2bot.fightrecord import FightWatch
from sts2bot.games.sts1 import keys, text, tiers
from sts2bot.games.sts1.client import Sts1Client
from sts2bot.games.sts1.screens import Screens
from sts2bot.run import crowd, decks, events, strategy

from ..helpers import DEFEND, STRIKE, combat_state, enemy
from .helpers import Sts1Case


def _game(**kw) -> dict:
    return {"act": 3, "floor": 40, "current_hp": 70, "max_hp": 80, "final_act_available": True, "keys": {"ruby": False, "emerald": False, "sapphire": False}, **kw}


def screens(**game):
    """Screens reading a message of the act 3 game above, with these fields changed."""
    s = Screens()
    s.raw = {"in_game": True, "game_state": _game(**game)}
    return s


def live(i, cid, ctype, cost=1, upgrades=0, **numbers):
    """A card in hand, with the numbers the game worked out for it (damage, block and their printed base)."""
    return Screens().card({"id": cid, "name": cid, "type": ctype, "cost": cost, "upgrades": upgrades, **numbers}, i, live=True)


class LiveNumbers(Sts1Case):
    def test_a_hand_card_reads_the_numbers_the_game_worked_out(self):
        # Weak and 5 Strength: the printed 6 is not what the Strike deals; Frail and Dexterity move a Defend's Block. Without the
        # mod's numbers the printed text stands, and a Skill's own numbers are not the card's damage (Flame Barrier's 4 back).
        barrier = dict(block=15, base_block=12, damage=-1, base_damage=-1)
        for cid, ctype, numbers, says in [("Strike_R", "ATTACK", dict(damage=4, base_damage=6), "Deal 4 damage"), ("Defend_R", "SKILL", dict(block=3, base_block=5), "Gain 3 Block"),
                                          ("Strike_R", "ATTACK", {}, "Deal 6 damage"), ("Flame Barrier", "SKILL", barrier, "Gain 15 Block"),
                                          ("Flame Barrier", "SKILL", barrier, "deal 4 damage back")]:
            with self.subTest(cid, says=says):
                self.assertIn(says, live(0, cid, ctype, **numbers)["description"])
        wave = live(0, "Iron Wave", "ATTACK", upgrades=1, damage=9, base_damage=7, block=10, base_block=7)["description"]
        self.assertEqual(wave, "Gain 10 Block. Deal 9 damage.")

    def test_heavy_blade_is_one_hit(self):
        # Read as "14 damage 5 times" it was a 70-damage card for 2 energy, in hand and at every reward.
        info = cards.parse(live(0, "Heavy Blade", "ATTACK", upgrades=1, damage=24, base_damage=14))
        self.assertEqual((info.damage, info.hits), (24, 1))
        printed = cards.parse(Screens().card({"id": "Heavy Blade", "name": "Heavy Blade", "type": "ATTACK", "upgrades": 1, "cost": 2}))
        self.assertEqual((printed.damage, printed.hits), (14, 1))


class Tiers(Sts1Case):
    def test_the_first_game_s_cards_have_tiers_of_their_own(self):
        self.assertEqual([k for k in tiers.TIER if k not in text.CARDS], [])  # (every name is a card the bot can read)
        self.assertEqual(crowd.rating("UNDERHANDED_STRIKE"), tiers.C)  # (by the game's id: Sneaky Strike)
        self.assertGreater(crowd.rating("ACROBATICS"), crowd.rating("POISONED_STAB"))
        self.assertIsNone(crowd.rating("STRIKE_R"))

    def test_a_card_that_can_only_be_played_sometimes_is_not_free_damage(self):
        # Clash topped the first rewards at 9.7: 14 damage for 0 energy on paper, dead beside Defends.
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
    def test_recall_in_act_3_with_hp_to_spare_and_no_key_while_act_4_is_shut(self):
        self.assertEqual(keys.missing(_game(final_act_available=False)), set())
        # Not before the heal (40 HP), but on the last rest site (floor 49) there is no other chance.
        for game, wanted in [({"final_act_available": False}, False), ({}, True), ({"current_hp": 40}, False), ({"current_hp": 37, "floor": 49}, True), ({"current_hp": 35, "floor": 49}, False),
                             ({"act": 2}, False), ({"keys": {"ruby": True}}, False)]:
            with self.subTest(**game):
                self.assertIs(keys.wants_ruby(_game(**game)), wanted)

    def test_the_rest_site_shows_recall_only_when_it_is_wanted(self):
        s = screens(screen_type="REST", screen_state={"rest_options": ["rest", "smith", "recall"]})
        self.assertEqual([o["id"] for o in s._rest()["rest_site"]["options"]], ["recall"])
        s.raw["game_state"]["current_hp"] = 30  # (act 3 and hurt: beside the rest, which the rest site takes first)
        self.assertEqual([o["id"] for o in s._rest()["rest_site"]["options"]], ["rest", "smith", "recall"])
        s.raw["game_state"]["keys"]["ruby"] = True
        self.assertEqual([o["id"] for o in s._rest()["rest_site"]["options"]], ["rest", "smith"])
        s.raw["game_state"].update(keys={}, final_act_available=False)
        self.assertEqual([o["id"] for o in s._rest()["rest_site"]["options"]], ["rest", "smith"])

    def test_recall_before_act_3_where_an_upgrade_would_have_been(self):
        from sts2bot.run.strategy import Memory
        opts = lambda: {"options": [{"index": 0, "id": "rest"}, {"index": 1, "id": "smith"}, {"index": 2, "id": "recall"}]}
        hale = {"state_type": "rest_site", "run": {"act": 1}, "player": {"hp": 75, "max_hp": 80}, "rest_site": opts()}
        self.assertEqual(strategy.rest_site(hale, Memory())[0], ("choose_rest_option", {"index": 2}))
        hurt = {**hale, "player": {"hp": 30, "max_hp": 80}}
        self.assertEqual(strategy.rest_site(hurt, Memory())[0], ("choose_rest_option", {"index": 0}))

    def test_the_act_3_chest_gives_the_sapphire_key_over_its_relic(self):
        relic = {"id": "Anchor", "name": "Anchor"}
        rewards = [{"reward_type": "GOLD", "gold": 30}, {"reward_type": "RELIC", "relic": relic}, {"reward_type": "SAPPHIRE_KEY", "link": relic}]
        s = screens(screen_type="COMBAT_REWARD", screen_state={"rewards": rewards})
        self.assertEqual([r["type"] for r in s._combat_reward()["rewards"]["items"]], ["gold", "sapphire_key"])
        s.raw["game_state"]["act"] = 2
        self.assertEqual([r["type"] for r in s._combat_reward()["rewards"]["items"]], ["gold", "relic"])

    def test_the_burning_elite_is_worth_a_detour(self):
        s = screens(act=1, screen_type="MAP", screen_state={"next_nodes": [{"x": 0, "y": 0}, {"x": 2, "y": 0}]}, map=[
            {"x": 0, "y": 0, "symbol": "M", "children": [{"x": 0, "y": 1}]}, {"x": 2, "y": 0, "symbol": "M", "children": [{"x": 2, "y": 1}]},
            {"x": 0, "y": 1, "symbol": "E", "children": []}, {"x": 2, "y": 1, "symbol": "E", "emerald_key": True, "children": []}])
        nodes = {(n["col"], n["row"]): n["type"] for n in s._map()["map"]["nodes"]}
        self.assertEqual((nodes[(0, 2)], nodes[(2, 2)]), ("Elite", "EmeraldElite"))
        plain = strategy.path_value([("Monster", 9), ("Elite", 10)], 70, 80, 99, 1)
        burning = strategy.path_value([("Monster", 9), ("EmeraldElite", 10)], 70, 80, 99, 1)
        self.assertAlmostEqual(burning - plain, strategy.EMERALD_WORTH[1])
        s.raw["game_state"]["keys"]["emerald"] = True
        self.assertNotIn("EmeraldElite", [n["type"] for n in s._map()["map"]["nodes"]])


class RunEnd(Sts1Case):
    def test_a_finished_room_is_not_the_end_of_the_run(self):
        # After the act 3 boss CommunicationMod says COMPLETE ("proceed" goes on to the victory room): read as game over,
        # the first win was written down as a loss on floor 50 and a win on floor 51.
        s = Screens()
        s.raw = {"in_game": True, "available_commands": ["proceed"], "game_state": {**_game(floor=50), "screen_type": "COMPLETE", "room_phase": "COMPLETE"}}
        self.assertEqual(s.translate(s.raw)["state_type"], "overlay")

    def test_a_victory_screen_does_not_lose_the_last_boss(self):
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
        # The Watcher's first run: the bot clicked Scry's three cards on and off for good.
        hand = [{"name": "Defend", "uuid": "a", "type": "SKILL"}, {"name": "Dazed", "uuid": "b", "type": "STATUS"}, {"name": "Strike", "uuid": "c", "type": "ATTACK"}]
        picked, sent = {"a"}, []  # (a Defend already clicked on, as the stuck run had)

        def grid():
            ss = {"cards": hand, "selected_cards": [c for c in hand if c["uuid"] in picked], "any_number": True, "num_cards": 3}
            return {"in_game": True, "available_commands": ["choose", "confirm"],
                    "game_state": {"screen_type": "GRID", "current_action": "ScryAction", "room_phase": "COMBAT", "screen_state": ss}}

        class Fake(Sts1Client):
            def _message(self):
                return grid()

            def send(self, command):
                sent.append(command)
                if command == "confirm":
                    return {"status": "ok", "message": {"in_game": True, "available_commands": ["play", "end"], "game_state": {"screen_type": "NONE"}}}
                picked.symmetric_difference_update({hand[int(command.split()[1])]["uuid"]})
                return {"status": "ok", "message": grid()}

        Fake().state()
        self.assertEqual(sent, ["choose 0", "choose 1", "confirm"])
        self.assertEqual(picked, {"b"})


class EnergyLoss(Sts1Case):
    def test_a_power_that_takes_energy_every_turn_is_priced_for_it(self):
        # Fasting read as 7.8 (3 Strength, 3 Dexterity), and the Watcher's first run held two: 1 energy a turn.
        fasting = Screens().card({"id": "Fasting2", "name": "Fasting", "type": "POWER", "rarity": "UNCOMMON", "cost": 2})
        strike = Screens().card({"id": "Strike_P", "name": "Strike", "type": "ATTACK", "rarity": "BASIC", "cost": 1})
        first = cards.deck_value(fasting) + decks.deck_fit(fasting, [strike] * 10, 1, 3)
        second = cards.deck_value(fasting) + decks.deck_fit(fasting, [strike] * 10 + [fasting], 1, 3)
        self.assertLess(first, cards.deck_value(fasting) - 3.0)
        self.assertLess(second, 0.0)

    def test_a_second_fasting_is_not_played(self):
        # Three Fasting+ left the Watcher 1 energy a turn, dead to a Chosen from full HP. One already in play: the second is an
        # energy of what is left, and the Strikes are worth more.
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
        hand = [Screens().card({"id": cid, "name": cid, "type": t, "cost": -2}, n)
                for n, (cid, t) in enumerate([("Regret", "CURSE"), ("Decay", "CURSE"), ("Burn", "STATUS"), ("Strike_R", "ATTACK")])]
        self.assertEqual(battle.hand_burns({"hand": hand}), ((0, -1, False), (1, 2, True), (2, 2, True)))


class TimeEater(Sts1Case):
    def test_the_twelfth_card_ends_the_turn(self):
        # 11 cards played, 30 HP, a 31 hit coming, 4 energy: Cold Snap was the 12th card and the last.
        s = combat_state([STRIKE(0), DEFEND(1), STRIKE(2)], [enemy("TIMEEATER_0", 183, attack=31, status=[{"id": "TIME_WARP_POWER", "amount": 11}])],
                         energy=4, hp=30, block=4, kind="boss")
        self.assertEqual(battle.cards_left_this_turn(s), 1)
        self.assertEqual(combat.choose_card(s)[1]["card_index"], 1)


class PaidHp(Sts1Case):
    def test_no_card_that_costs_the_last_hp_even_with_regen_to_come(self):
        # On 5 HP with Regen 3 the planner read Offering+'s 6 as 3 and played it into a stunned boss: dead.
        hand = [live(0, "Spot Weakness", "SKILL", 1, 1), live(1, "Wound", "STATUS", -2), live(2, "Defend_R", "SKILL", 1, 1, block=10, base_block=8),
                live(3, "Bash", "ATTACK", 2, 1, damage=21, base_damage=10), live(4, "Offering", "SKILL", 0, 1)]
        boss = enemy("BRONZEAUTOMATON_1", 75, status=[{"id": "ARTIFACT_POWER", "amount": 1}, {"id": "STRENGTH_POWER", "amount": 6}])
        boss["intents"] = [{"type": "Stun", "label": ""}]
        s = combat_state(hand, [boss], energy=2, hp=5, kind="boss")
        s["player"].update(draw_pile_count=10, status=[{"id": "REGEN_POWER", "amount": 3}, {"id": "STRENGTH_POWER", "amount": 11}])
        s["battle"]["round"] = 7
        self.assertNotEqual(combat.choose_card(s)[1]["card_index"], 4)

    def test_an_event_curse_in_the_first_game_s_words_costs_something(self):
        # "Become Cursed - Regret." cost nothing: Golden Shrine's Desecrate (275 Gold and Regret) beat Pray.
        value = lambda title, text: events._event_option_value({"title": title, "description": text}, 60, 80, act=1)
        self.assertLess(value("Desecrate", "Gain 275 Gold. Become Cursed - Regret."), value("Pray", "Gain 100 Gold."))
        self.assertLess(value("Rummage", "Obtain a special Relic. Become Cursed - Pain."), value("Forge", "Upgrade a card in your deck."))


class WrithingMass(Sts1Case):
    def state(self, hp):
        hand = [live(0, "Defend_P", "SKILL", 1, 1, block=8, base_block=8), live(1, "CrushJoints", "ATTACK", 0, 1, damage=10, base_damage=10),
                live(2, "Consecrate", "ATTACK", 0, 1, damage=8, base_damage=8), live(3, "Eruption", "ATTACK", 2, damage=9, base_damage=9),
                live(4, "ThirdEye", "SKILL", 1, block=7, base_block=7)]
        mass = enemy("WRITHINGMASS_0", 81, status=Screens.powers({"powers": [{"id": "Malleable", "amount": 3}, {"id": "Compulsive", "amount": -1}]}))
        mass["intents"] = [{"type": "Debuff", "label": ""}]
        s = combat_state(hand, [mass], energy=4, hp=hp, max_hp=89)
        s["battle"]["round"] = 4
        return s

    def test_a_hit_gives_it_a_new_intent(self):
        # On 12 HP, its intent a Debuff, the Watcher entered Wrath and hit it; it came round to 14 x 3.
        action = combat.choose_card(self.state(12))
        self.assertTrue(action is None or action[0] != "play_card" or action[1]["card_index"] in (0, 4))
        self.assertEqual(combat.choose_card(self.state(60))[1]["card_index"], 1)  # (with HP to spare it is still to be killed)


class AwakenedOne(Sts1Case):
    def test_its_first_death_is_not_the_end(self):
        status = Screens.powers({"powers": [{"id": "Unawakened", "amount": -1}, {"id": "Curiosity", "amount": 1}]})
        e = {"entity_id": "AWAKENEDONE_2", "hp": 20, "max_hp": 300, "block": 0, "status": status, "intents": [{"type": "Attack", "label": "16"}]}
        (boss,) = battle.read_enemies({"battle": {"enemies": [e], "round": 9}, "player": {"hp": 50}, "state_type": "boss"})
        self.assertEqual((boss.respawns, boss.curious), (1, 1))
        self.assertEqual(closing.discount(30.0, 10, 50, [boss]), 1.0)  # (20 HP is not "dying next turn": 300 more come after)
