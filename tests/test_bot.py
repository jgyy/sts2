import json
import tempfile
import unittest
from pathlib import Path

from sts2bot import combat, strategy
from sts2bot.bot import Bot, Stuck
from sts2bot.cards import deck_value, parse
from sts2bot.strategy import Memory

from .fake_game import FakeGame, card


class FakeClock:
    def __init__(self):
        self.t = 0.0

    def now(self):
        return self.t

    def sleep(self, s):
        self.t += s


def make_bot(game, **kw):
    clock = FakeClock()
    return Bot(game, clock=clock.now, sleep=clock.sleep, **kw)


def combat_state(hand, enemies, *, energy=3, hp=80, max_hp=80, block=0, kind="monster", potions=()):
    return {
        "state_type": kind,
        "run": {"act": 1, "floor": 3, "ascension": 0},
        "battle": {"round": 1, "turn": "player", "is_play_phase": True, "enemies": enemies},
        "player": {
            "hp": hp,
            "max_hp": max_hp,
            "block": block,
            "energy": energy,
            "hand": hand,
            "potions": list(potions),
            "draw_pile_count": 5,
            "discard_pile_count": 0,
            "exhaust_pile_count": 0,
        },
    }


def enemy(eid, hp, attack=None, block=0, status=()):
    intents = [{"type": "Attack", "label": str(attack)}] if attack else [{"type": "Buff", "label": ""}]
    return {"entity_id": eid, "hp": hp, "max_hp": hp, "block": block, "status": list(status), "intents": intents}


STRIKE = lambda i: card(i, "STRIKE_R", "Strike", "Attack", 1, "Deal 6 damage.", "AnyEnemy")
DEFEND = lambda i: card(i, "DEFEND_R", "Defend", "Skill", 1, "Gain 5 Block.", "Self")


class CardParsing(unittest.TestCase):
    def test_multi_hit_and_debuffs(self):
        c = parse(card(0, "X", "X", "Attack", 2, "Deal 5 damage 3 times. Apply 2 Vulnerable.", "AnyEnemy"))
        self.assertEqual((c.damage, c.hits, c.vulnerable, c.cost, c.needs_target), (5, 3, 2, 2, True))

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


class Strategy(unittest.TestCase):
    def base(self, kind, hp=80, gold=100, **body):
        return {"state_type": kind, "run": {"act": 1, "floor": 5}, "player": {"hp": hp, "max_hp": 80, "gold": gold, "potions": []}, **body}

    def test_map_avoids_elite_when_hurt_and_takes_it_when_healthy(self):
        m = {
            "next_options": [{"index": 0, "col": 0, "row": 1, "type": "Elite"}, {"index": 1, "col": 1, "row": 1, "type": "Monster"}],
            "nodes": [
                {"col": 0, "row": 1, "type": "Elite", "children": []},
                {"col": 1, "row": 1, "type": "Monster", "children": []},
            ],
        }
        hurt, healthy = self.base("map", hp=30, map=m), self.base("map", hp=80, map=m)
        hurt["run"]["act"] = healthy["run"]["act"] = 2  # early act 1 elites are avoided regardless
        self.assertEqual(strategy.map_screen(hurt, Memory())[0][1], {"index": 1})
        self.assertEqual(strategy.map_screen(healthy, Memory())[0][1], {"index": 0})

    def test_map_looks_ahead(self):
        m = {
            "next_options": [{"index": 0, "col": 0, "row": 1, "type": "Monster"}, {"index": 1, "col": 1, "row": 1, "type": "Monster"}],
            "nodes": [
                {"col": 0, "row": 1, "type": "Monster", "children": [[0, 2]]},
                {"col": 1, "row": 1, "type": "Monster", "children": [[1, 2]]},
                {"col": 0, "row": 2, "type": "Monster", "children": []},
                {"col": 1, "row": 2, "type": "Treasure", "children": []},
            ],
        }
        self.assertEqual(strategy.map_screen(self.base("map", map=m), Memory())[0][1], {"index": 1})

    def test_rests_before_the_boss_unless_nearly_full(self):
        opts = [{"index": 0, "id": "REST", "is_enabled": True}, {"index": 1, "id": "SMITH", "is_enabled": True}]
        s = self.base("rest_site", hp=60, rest_site={"options": opts})  # 75%
        self.assertEqual(strategy.rest_site(s, Memory())[0][1]["index"], 1)
        self.assertEqual(strategy.rest_site(s, Memory(boss_next=True))[0][1]["index"], 0)

    def test_avoids_elites_early_in_act_one_and_notes_the_boss(self):
        nodes = [{"col": 0, "row": 2, "type": "Elite", "children": [[0, 3]]}, {"col": 1, "row": 2, "type": "Monster", "children": [[0, 3]]},
                 {"col": 0, "row": 3, "type": "Boss", "children": []}]
        opts = [{"index": 0, "col": 0, "row": 2, "type": "Elite"}, {"index": 1, "col": 1, "row": 2, "type": "Monster"}]
        s = self.base("map", hp=80, map={"next_options": opts, "nodes": nodes})
        s["run"]["act"] = 1
        mem = Memory()
        self.assertEqual(strategy.map_screen(s, mem)[0][1]["index"], 1)
        self.assertTrue(mem.boss_next)
        s["run"]["act"] = 2
        self.assertEqual(strategy.map_screen(s, mem)[0][1]["index"], 0)

    def test_rest_heals_when_low_and_smiths_when_healthy(self):
        rs = {"options": [{"index": 0, "id": "rest", "is_enabled": True}, {"index": 1, "id": "smith", "is_enabled": True}]}
        self.assertEqual(strategy.rest_site(self.base("rest_site", hp=30, rest_site=rs), Memory())[0][1], {"index": 0})
        self.assertEqual(strategy.rest_site(self.base("rest_site", hp=78, rest_site=rs), Memory())[0][1], {"index": 1})

    def test_event_avoids_fatal_and_cursed_options(self):
        ev = {
            "options": [
                {"index": 0, "title": "Grab", "description": "Obtain a relic. Lose 50 HP."},
                {"index": 1, "title": "Pray", "description": "Gain 30 gold."},
                {"index": 2, "title": "Take", "description": "Obtain a relic. Gain a Curse.", "is_locked": True},
            ]
        }
        acts = strategy.event(self.base("event", hp=20, event=ev), Memory())
        self.assertEqual(acts[0][1], {"index": 1})
        self.assertNotIn(("choose_event_option", {"index": 2}), acts)

    def test_removal_picks_curse_then_basics(self):
        cs = {
            "screen_type": "select",
            "prompt": "Choose a card to Remove.",
            "cards": [
                card(0, "UPPERCUT", "Uppercut", "Attack", 2, "Deal 13 damage.", rarity="Uncommon"),
                card(1, "STRIKE_R", "Strike", "Attack", 1, "Deal 6 damage.", rarity="Basic"),
                card(2, "REGRET", "Regret", "Curse", 0, "Unplayable."),
            ],
        }
        acts = strategy.card_select(self.base("card_select", card_select=cs), Memory())
        self.assertEqual([a[1].get("index") for a in acts[:3]], [2, 1, 0])

    def test_choose_a_card_is_offered_again_after_an_ignored_click(self):
        cards = [card(0, "DISINTEGRATION", "Disintegration", "Status", 0, "At the end of your turn, take 7 damage.", "None"),
                 card(1, "SLOTH", "Sloth", "Status", 0, "You cannot play more than 3 cards each turn.", "None")]
        s = self.base("card_select", card_select={"screen_type": "choose", "prompt": "Choose a card.", "cards": cards})
        mem = Memory()
        first = strategy.card_select(s, mem)
        self.assertEqual({a for a, _ in first}, {"select_card"})  # never confirm_selection here
        mem.selected.update({0, 1})  # both were clicked on earlier, identical screens
        self.assertEqual(strategy.card_select(s, mem), first)

    def test_grid_selection_starts_over_when_clicks_did_not_register(self):
        cards = [STRIKE(0), DEFEND(1)]
        s = self.base("card_select", card_select={"screen_type": "select", "prompt": "Choose 2 cards.", "cards": cards})
        mem = Memory()
        strategy.card_select(s, mem)
        mem.selected.update({0, 1})
        self.assertIn(("select_card", {"index": 0}), strategy.card_select(s, mem))

    def test_stops_ticking_cards_once_enough_are_chosen(self):
        cards = [STRIKE(i) for i in range(5)]
        s = self.base("card_select", card_select={"screen_type": "enchant", "prompt": "Choose 3 cards to Enchant.", "cards": cards})
        mem = Memory()
        for _ in range(3):
            action, params = strategy.card_select(s, mem)[0]
            self.assertEqual(action, "select_card")
            mem.selected.add(params["index"])
        self.assertEqual(strategy.card_select(s, mem)[0], ("confirm_selection", {}))

    def test_shop_buys_removal_and_relic_but_not_weak_cards(self):
        shop = {
            "items": [
                {"index": 0, "category": "card", "price": 50, "is_stocked": True, "can_afford": True, "card_id": "STRIKE_R", "card_type": "Attack", "card_cost": "1", "card_rarity": "Basic", "card_description": "Deal 6 damage."},
                {"index": 1, "category": "relic", "price": 150, "is_stocked": True, "can_afford": True},
                {"index": 2, "category": "card_removal", "price": 75, "is_stocked": True, "can_afford": True},
                {"index": 3, "category": "relic", "price": 300, "is_stocked": True, "can_afford": False},
            ],
            "can_proceed": True,
        }
        acts = strategy.shop(self.base("shop", gold=250, shop=shop), Memory())
        self.assertEqual(acts, [("shop_purchase", {"index": 2}), ("shop_purchase", {"index": 1}), ("proceed", {})])

    def test_auto_plays_the_character_with_the_lowest_ascension(self):
        unlocked = ["IRONCLAD", "SILENT", "DEFECT"]
        pick = lambda asc: strategy.choose_character("auto", unlocked, 0, asc)
        self.assertEqual(pick({}), "IRONCLAD")
        self.assertEqual(pick({"IRONCLAD": 2, "SILENT": 1, "DEFECT": 1}), "SILENT")
        self.assertEqual(pick({"IRONCLAD": 1}), "SILENT")  # never played counts as 0
        self.assertEqual(pick({"IRONCLAD": 3, "SILENT": 3, "DEFECT": 3}), "IRONCLAD")

    def test_embarks_at_the_highest_unlocked_ascension(self):
        chars = [{"id": "IRONCLAD", "locked": False}, {"id": "SILENT", "locked": False}, {"id": "DEFECT", "locked": True}]
        s = {"state_type": "menu", "menu_screen": "character_select", "characters": chars,
             "options": [{"name": "IRONCLAD", "enabled": True}, {"name": "SILENT", "enabled": True}, {"name": "confirm", "enabled": True}],
             "ascension": 0, "max_ascension": 3}
        mem = Memory(max_ascension={"IRONCLAD": 4, "SILENT": 3})
        self.assertEqual(strategy.menu(s, mem, "auto")[0], ("menu_select", {"option": "SILENT"}))
        mem.character_selected = True
        self.assertEqual(strategy.menu(s, mem, "auto")[0], ("set_ascension", {"level": 3}))
        s["ascension"] = 3
        self.assertEqual(strategy.menu(s, mem, "auto")[0], ("menu_select", {"option": "confirm"}))
        # The levels are not on screen yet (it happens just after picking the
        # character): hold off, do not embark at ascension 0.
        s["ascension"], s["max_ascension"] = 0, 0
        mem.ascension_waits = 0
        self.assertEqual(strategy.menu(s, mem, "auto")[0], ("menu_select", {"option": "SILENT"}))
        # A mod without the patch never shows them: give up waiting and embark.
        del s["ascension"], s["max_ascension"]
        first = [strategy.menu(s, mem, "auto")[0] for _ in range(25)]
        self.assertEqual(first[-1], ("menu_select", {"option": "confirm"}))

    def test_auto_plays_towards_unlocking_every_epoch(self):
        epochs = {
            "IRONCLAD2_EPOCH": "Revealed", "IRONCLAD7_EPOCH": "NotObtained",
            "SILENT1_EPOCH": "Revealed", "SILENT2_EPOCH": "Revealed", "SILENT4_EPOCH": "NotObtained",
            "SILENT5_EPOCH": "NotObtained", "REGENT1_EPOCH": "Revealed", "REGENT2_EPOCH": "NotObtained",
            "REGENT3_EPOCH": "NotObtained", "REGENT4_EPOCH": "NotObtained", "NECROBINDER1_EPOCH": "NotObtained",
        }
        who = ["IRONCLAD", "SILENT", "REGENT"]
        pick = lambda drought=None: strategy.choose_character("auto", who, 0, {"IRONCLAD": 1}, epochs, drought)
        self.assertEqual(strategy.missing_epochs("SILENT", epochs), 2)
        self.assertEqual(pick(), "REGENT")  # three epochs to earn, the most
        # Twelve Regent runs that earned nothing: try someone else for now.
        self.assertEqual(pick({"REGENT": 12}), "SILENT")
        # Nobody has epochs left: back to evening out the ascension levels.
        done = {k: "Revealed" for k in epochs}
        self.assertEqual(strategy.choose_character("auto", who, 0, {"IRONCLAD": 1, "REGENT": 2}, done), "SILENT")

    def test_character_choice(self):
        unlocked = ["IRONCLAD", "SILENT", "REGENT"]
        self.assertEqual(strategy.choose_character("silent", unlocked, 0), "SILENT")
        self.assertEqual(strategy.choose_character("DEFECT", unlocked, 0), "IRONCLAD")
        self.assertEqual([strategy.choose_character("cycle", unlocked, n) for n in range(4)], ["IRONCLAD", "SILENT", "REGENT", "IRONCLAD"])


class FullLoop(unittest.TestCase):
    def test_map_travel_is_not_clicked_twice(self):
        """While the travel animation plays the map still answers; wait it out."""

        class TravellingMap:
            def __init__(self):
                self.clicks = 0
                self.polls_left = 0

            def state(self):
                if self.clicks and self.polls_left <= 0:
                    return {"state_type": "event", "event": {"in_dialogue": True}}
                self.polls_left -= 1
                opts = [{"index": 0, "col": self.clicks, "row": 1, "type": "Monster"}]
                return {"state_type": "map", "map": {"next_options": opts, "nodes": []}, "player": {"hp": 50, "max_hp": 80}}

            def act(self, action, **params):
                if action == "choose_map_node":
                    self.clicks += 1
                    self.polls_left = 5
                return {"status": "ok"}

        game = TravellingMap()
        make_bot(game).step()
        self.assertEqual(game.clicks, 1)
        self.assertEqual(game.state()["state_type"], "event")

    def test_plays_several_runs_back_to_back(self):
        game = FakeGame(seed=1, floors=4)
        with tempfile.TemporaryDirectory() as d:
            bot = make_bot(game, log_dir=Path(d))
            bot.run(max_runs=3)
            lines = [json.loads(l) for l in (Path(d) / "runs.jsonl").read_text().splitlines()]
        self.assertEqual(bot.runs_finished, 3)
        self.assertEqual(game.runs_started, 3)
        self.assertEqual(game.actions[-1], ("menu_select", {"option": "main_menu"}))
        self.assertEqual(len(lines), 3)
        self.assertTrue(all(l["result"] == "win" and l["floor"] >= 4 for l in lines))
        self.assertEqual(game.screen, "main")
        names = {a for a, _ in game.actions}
        self.assertLessEqual({"play_card", "end_turn", "claim_reward", "select_card_reward", "choose_map_node", "choose_rest_option"}, names)

    def test_skipped_card_reward_does_not_loop(self):
        game = FakeGame(seed=2, floors=3)
        bot = make_bot(game)
        import sts2bot.strategy as st

        old = st.SKIP_THRESHOLD
        st.SKIP_THRESHOLD = 99.0  # skip everything
        try:
            bot.run(max_runs=1)
        finally:
            st.SKIP_THRESHOLD = old
        skips = sum(1 for a, _ in game.actions if a == "skip_card_reward")
        offered = sum(1 for a, p in game.actions if (a, p) == ("claim_reward", {"index": 1}))
        self.assertGreaterEqual(skips, 1)
        self.assertEqual(skips, offered)  # each card reward opened once, never re-opened
        self.assertEqual(len(game.deck), 10)

    def test_loses_a_run_and_starts_again(self):
        game = FakeGame(seed=3, floors=50, enemy_hp=70, enemy_attack=22)
        bot = make_bot(game)
        bot.run(max_runs=2)
        self.assertEqual((bot.runs_finished, bot.wins), (2, 0))

    def test_resumes_a_saved_run_from_the_menu(self):
        game = FakeGame(seed=4, floors=3)
        bot = make_bot(game)
        while game.screen != "combat":
            bot.step()
        game.saved_screen, game.screen = "combat", "main"  # as after a game restart
        bot.run(max_runs=1)
        self.assertEqual(game.runs_started, 1)
        self.assertIn(("menu_select", {"option": "continue"}), game.actions)

    def test_abandons_when_told_to(self):
        game = FakeGame(seed=5)
        bot = make_bot(game)
        while game.screen != "combat":
            bot.step()
        game.saved_screen, game.screen = "combat", "main"
        bot.abandon_next = True
        for _ in range(6):
            bot.step()
        self.assertFalse(bot.abandon_next)
        self.assertEqual(game.runs_started, 2)

    def test_raises_stuck_when_nothing_works(self):
        class Frozen(FakeGame):
            def state(self):
                return {"state_type": "overlay", "run": {"act": 1, "floor": 2}, "player": {"hp": 5, "max_hp": 80}, "overlay": {"screen_type": "NWeird"}}

            def act(self, action, **p):
                self.actions.append((action, p))
                return {"status": "error", "error": "nope"}

        bot = make_bot(Frozen(), stuck_timeout=120)
        with self.assertRaises(Stuck):
            for _ in range(10_000):
                bot.step()


class FastMode(unittest.TestCase):
    def test_sent_through_the_mod_and_tolerates_an_old_mod(self):
        from sts2bot.supervisor import set_fast_mode

        class Mod:
            def __init__(self, reply):
                self.reply, self.calls = reply, []

            def act(self, action, **params):
                self.calls.append((action, params))
                return self.reply

        new = Mod({"status": "ok", "fast_mode": "instant"})
        self.assertTrue(set_fast_mode(new, "instant"))
        self.assertEqual(new.calls, [("set_fast_mode", {"mode": "instant"})])
        with self.assertLogs("sts2bot.supervisor", level="WARNING"):
            self.assertFalse(set_fast_mode(Mod({"status": "error", "error": "Unknown action"}), "instant"))


class CrashRecovery(unittest.TestCase):
    def test_a_crashed_child_is_not_counted_as_running(self):
        import subprocess, sys, time
        from unittest import mock
        from sts2bot import supervisor

        child = subprocess.Popen([sys.executable, "-c", "pass"])
        time.sleep(0.3)  # exited, not yet collected: a zombie
        supervisor._children.append(child)
        zombie = mock.Mock(stdout="Z\n")
        with mock.patch.object(supervisor.subprocess, "run", return_value=zombie):
            self.assertFalse(supervisor.game_running())
        self.assertNotIn(child, supervisor._children)
        alive = mock.Mock(stdout="Sl\n")
        with mock.patch.object(supervisor.subprocess, "run", return_value=alive):
            self.assertTrue(supervisor.game_running())


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


class LiveGameLessons(unittest.TestCase):
    """Things the first sessions against the real game got wrong."""

    ICON = "[ironclad_energy_icon.png]"

    def test_energy_is_written_as_icons(self):
        c = parse(card(0, "OFFERING", "Offering", "Skill", 0, f"Lose 6 HP. Gain {self.ICON}{self.ICON}. Draw 3 cards. Exhaust."))
        self.assertEqual((c.energy_gain, c.draw, c.hp_loss), (2, 3, 6))

    def test_later_effects_are_not_read_as_immediate(self):
        jug = parse(card(0, "JUGGERNAUT", "Juggernaut", "Power", 2, "Whenever you gain Block, deal 6 damage to a random enemy."))
        self.assertEqual(jug.damage, 0)
        barrier = parse(card(0, "FLAME_BARRIER", "Flame Barrier", "Skill", 2, "Gain 12 Block. Whenever you are attacked this turn, deal 4 damage back."))
        self.assertEqual((barrier.block, barrier.damage), (12, 0))
        bomb = parse(card(0, "THE_BOMB", "The Bomb", "Skill", 2, "At the end of 3 turns, deal 40 damage to ALL enemies."))
        self.assertEqual(bomb.damage, 0)

    def test_weak_and_vulnerable_together(self):
        c = parse(card(0, "SHOCKWAVE", "Shockwave", "Skill", 2, "Apply 3 Weak and Vulnerable to ALL enemies. Exhaust."))
        self.assertEqual((c.weak, c.vulnerable, c.aoe), (3, 3, True))

    def test_strength_this_turn_adds_to_later_attacks(self):
        setup = card(0, "SETUP_STRIKE", "Setup Strike", "Attack", 1, "Deal 7 damage. Gain 3 Strength this turn.", "AnyEnemy")
        hand = [parse(setup), parse(STRIKE(1)), parse(STRIKE(2))]
        foes = combat.read_enemies(combat_state([], [enemy("A", 100)]))
        ctx = dict(energy=3, stars=0, hp=80, max_hp=80, block=0)
        with_setup = combat.simulate(hand, foes, **ctx).score
        plain = combat.simulate([parse(STRIKE(0)), parse(STRIKE(1)), parse(STRIKE(2))], foes, **ctx).score
        self.assertAlmostEqual(with_setup - plain, 1 + 3 + 3)

    def test_demon_form_is_played_early_in_a_boss_fight(self):
        demon = card(0, "DEMON_FORM", "Demon Form", "Power", 3, "At the start of your turn, gain 3 Strength.")
        state = combat_state([demon, STRIKE(1), STRIKE(2), STRIKE(3)], [enemy("BOSS", 250)], kind="boss")
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0}))

    def test_sapping_strength_counts_as_defence(self):
        shackles = card(0, "DARK_SHACKLES", "Dark Shackles", "Skill", 0, "Enemy loses 9 Strength this turn. Exhaust.", "AnyEnemy")
        foe = enemy("A", 100)
        foe["intents"] = [{"type": "Attack", "label": "10x3"}]
        state = combat_state([shackles], [foe], energy=0, hp=20)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0, "target": "A"}))

    def test_never_trades_max_hp_away(self):
        def best(*options):
            opts = [{"index": i, "title": t, "description": d} for i, (t, d) in enumerate(options)]
            state = {"state_type": "event", "event": {"options": opts}, "player": {"hp": 70, "max_hp": 80}}
            return strategy.event(state, Memory())[0][1]["index"]

        self.assertEqual(best(("Lose Everything", "???"), ("Give Up", "Stop reading the text and leave.")), 1)
        self.assertEqual(best(("Continue", "Lose 24 Max HP. Upgrade 2 random cards."), ("Give Up", "Leave.")), 1)
        self.assertEqual(best(("Decipher", "Lose 3 Max HP. Upgrade a random card."), ("Give Up", "Leave.")), 0)

    def test_bridge_event_gives_up_a_starter_card_not_a_good_one(self):
        def value(title, text, hp=60):
            return strategy._event_option_value({"title": title, "description": text}, hp, 80)

        hold = value("Hold On", "Lose 5 HP. The card in the above option is randomized.")
        self.assertGreater(value("Overcome", "Defend is removed from your Deck."), hold)
        self.assertLess(value("Overcome", "Demon Form+ is removed from your Deck."), hold)

    def test_picks_the_mildest_boss_affliction(self):
        cards_ = [
            card(0, "DISINTEGRATION", "Disintegration", "Status", 0, "At the end of your turn, take 6 damage."),
            card(1, "SLOTH", "Sloth", "Status", 0, "You cannot play more than 3 cards each turn."),
            card(2, "WASTE_AWAY", "Waste Away", "Status", 0, "Gain 1 less energy per turn."),
        ]
        state = {"state_type": "card_select", "card_select": {"screen_type": "choose", "prompt": "Choose a card.", "cards": cards_}}
        self.assertEqual(strategy.card_select(state, Memory())[0], ("select_card", {"index": 1}))

    def test_unreadable_cards_are_not_taken_for_their_rarity(self):
        odd = card(0, "NEW_THING", "New Thing", "Skill", 1, "Do something the bot has never seen.", rarity="Rare")
        state = {"state_type": "card_reward", "run": {"act": 2}, "card_reward": {"cards": [odd], "can_skip": True}}
        self.assertEqual(strategy.card_reward(state, Memory())[0][0], "skip_card_reward")

    def test_a_burrowed_enemy_is_dug_out_instead_of_blocked_forever(self):
        burrowed = [{"id": "BURROWED_POWER", "amount": 1}]
        bash = card(3, "BASH", "Bash", "Attack", 2, "Deal 11 damage. Apply 2 Vulnerable.", "AnyEnemy")
        hand = [DEFEND(0), DEFEND(1), DEFEND(2), bash]
        state = combat_state(hand, [enemy("T", 62, attack=24, block=20, status=burrowed)], hp=57)
        state["battle"]["round"] = 25
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 3, "target": "T"}))
        # Breaking the last of its Block stuns it, so that comes before blocking.
        state = combat_state(hand, [enemy("T", 62, attack=24, block=9, status=burrowed)], hp=57)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 3, "target": "T"}))

    def test_escapes_the_sandpit_before_it_closes(self):
        escape = card(0, "FRANTIC_ESCAPE", "Frantic Escape", "Status", 1, "Get farther away. Increase Sandpit by 1. Increase the cost of this card by 1.")
        boss = enemy("INS", 200, attack=20, status=[{"id": "SANDPIT_POWER", "amount": 1}])
        state = combat_state([escape, DEFEND(1), DEFEND(2), DEFEND(3)], [boss], kind="boss")
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0}))

    def test_kills_the_minion_that_does_all_the_damage(self):
        minion = enemy("TORCH", 199, attack=28, status=[{"id": "MINION_POWER", "amount": 1}])
        queen = enemy("QUEEN", 400)
        state = combat_state([STRIKE(0)], [minion, queen], kind="boss", energy=1)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0, "target": "TORCH"}))
        # Small fry beside a leader that fights for itself are still ignored.
        weak = enemy("FOLLOWER", 59, attack=5, status=[{"id": "MINION_POWER", "amount": 1}])
        state = combat_state([STRIKE(0)], [weak, enemy("PRIEST", 190, attack=8)], kind="boss", energy=1)
        self.assertEqual(combat.choose_card(state)[1]["target"], "PRIEST")

    def test_turns_to_face_the_enemy_about_to_hit_hard(self):
        """Surrounded: the enemy behind us hits 50% harder; aiming a card at it turns us round."""
        crusher = enemy("CRUSHER", 200, attack=6, status=[{"id": "BACK_ATTACK_LEFT_POWER", "amount": 1}])
        rocket = enemy("ROCKET", 150, attack=27, status=[{"id": "BACK_ATTACK_RIGHT_POWER", "amount": 1}])
        state = combat_state([STRIKE(0)], [crusher, rocket], kind="boss", energy=1, hp=40)
        state["player"]["status"] = [{"id": "SURROUNDED_POWER", "amount": 1}]
        state["_facing"] = "CRUSHER"  # so Rocket's 27 is its attack from behind
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 0, "target": "ROCKET"}))
        foes = combat.read_enemies(state)
        self.assertEqual([(e.entity_id, e.behind) for e in foes], [("CRUSHER", False), ("ROCKET", True)])

    def test_hard_to_kill_caps_each_hit(self):
        shell = [{"id": "HARD_TO_KILL_POWER", "amount": 9}]
        big = card(0, "BLUDGEON", "Bludgeon", "Attack", 3, "Deal 32 damage.", "AnyEnemy")
        twin = card(1, "TWIN_STRIKE", "Twin Strike", "Attack", 1, "Deal 5 damage twice.", "AnyEnemy")
        state = combat_state([big, twin, STRIKE(2), STRIKE(3)], [enemy("EXO", 28, status=shell)])
        self.assertNotEqual(combat.choose_card(state)[1]["card_index"], 0)

    def test_block_potion_waits_for_a_big_hit(self):
        potion = {"slot": 0, "id": "BLOCK_POTION", "name": "Block Potion", "description": "Gain 12 Block.", "target_type": "Self"}
        calm = combat_state([STRIKE(0)], [enemy("BOSS", 300)], kind="boss", potions=[potion])
        self.assertNotIn("use_potion", [a for a, _ in combat.actions(calm)])
        storm = combat_state([STRIKE(0)], [enemy("BOSS", 300, attack=30)], kind="boss", potions=[potion])
        self.assertEqual(combat.actions(storm)[0], ("use_potion", {"slot": 0}))

    def test_only_one_bound_card_is_planned_per_turn(self):
        bound = {"keywords": [{"name": "Bound", "description": "Only 1 Bound card can be played each turn."}]}
        hand = [
            card(0, "DEFEND_R", "Defend", "Skill", 1, "Gain 5 Block.", "Self", **bound),
            card(1, "DEFEND_R", "Defend", "Skill", 1, "Gain 5 Block.", "Self", **bound),
            card(2, "IMPERVIOUS", "Impervious", "Skill", 2, "Gain 30 Block. Exhaust.", "Self", **bound),
        ]
        # Without the rule the plan would be Defend + Impervious; the first
        # Defend played would then lock Impervious out and the hit would land.
        state = combat_state(hand, [enemy("A", 200, attack=30)], hp=40)
        self.assertEqual(combat.choose_card(state), ("play_card", {"card_index": 2}))

    def test_two_cards_together_kill_the_reviving_attacker(self):
        ghost = enemy("PARAFRIGHT", 21, attack=16, status=[{"id": "ILLUSION_POWER", "amount": 1}, {"id": "MINION_POWER", "amount": 1}])
        leader = enemy("OBSCURA", 101, attack=10)
        bash = card(0, "BASH", "Bash", "Attack", 2, "Deal 8 damage. Apply 2 Vulnerable.", "AnyEnemy")
        twin = card(1, "TWIN_STRIKE", "Twin Strike+", "Attack", 1, "Deal 7 damage twice.", "AnyEnemy")
        state = combat_state([bash, twin, DEFEND(2)], [ghost, leader], hp=18)
        action, params = combat.choose_card(state)
        self.assertEqual((action, params.get("target")), ("play_card", "PARAFRIGHT"))

    def test_a_win_is_read_from_the_games_own_counter(self):
        class Game:
            wins = 4

            def profile(self):
                return {"total_wins": self.wins, "characters": []}

        game = Game()
        bot = Bot(game, clock=FakeClock().now, sleep=lambda s: None)
        bot._refresh_ascensions()  # run start: remembers the counter
        over = {"state_type": "game_over", "run": {"act": 3, "floor": 48}, "player": {"hp": 0, "max_hp": 80}}
        game.wins = 5
        with self.assertLogs("sts2bot", level="INFO") as logs:
            bot._record_run(over)
        self.assertIn("win", logs.output[0])
        self.assertEqual(bot.wins, 1)
        with self.assertLogs("sts2bot", level="INFO") as logs:
            bot._record_run({**over, "player": {"hp": 12, "max_hp": 80}})  # counter unchanged: a loss
        self.assertIn("loss", logs.output[0])

    def test_reveals_unlocks_on_the_timeline_before_starting_a_run(self):
        mem = Memory()
        main = {"state_type": "menu", "menu_screen": "main", "options": ["timeline", "settings", "quit"],
                "pending_epoch_ids": ["NEOW_EPOCH"]}
        self.assertEqual(strategy.menu(main, mem, "auto")[0], ("menu_select", {"option": "timeline"}))
        timeline = {"state_type": "menu", "menu_screen": "timeline",
                    "options": [{"name": "advance", "enabled": True}, {"name": "back", "enabled": True}]}
        self.assertEqual(strategy.menu(timeline, mem, "auto")[0], ("menu_select", {"option": "advance"}))

        class Game:
            def state(self):
                return timeline

            def act(self, action, **params):
                return {"status": "ok", "message": "No more epochs to advance", "done": True}

        clock = FakeClock()
        bot = Bot(Game(), clock=clock.now, sleep=clock.sleep)
        for _ in range(3):
            bot.step()  # the mod says nothing is left, three times running ...
        self.assertEqual(strategy.menu(timeline, bot.mem, "auto")[0], ("menu_select", {"option": "back"}))
        # ... so back at the menu the bot plays instead of going round again.
        main = {"state_type": "menu", "menu_screen": "main", "options": ["singleplayer", "timeline", "quit"]}
        self.assertEqual(strategy.menu(main, bot.mem, "auto")[0], ("menu_select", {"option": "singleplayer"}))
        # A run in progress comes first; the Timeline can wait until it is over.
        busy = {"state_type": "menu", "menu_screen": "main", "options": ["continue", "abandon_run", "timeline"],
                "pending_epoch_ids": ["SILENT2_EPOCH"]}
        self.assertEqual(strategy.menu(busy, Memory(), "auto")[0], ("menu_select", {"option": "continue"}))

    def test_choose_screen_click_is_retried_quickly(self):
        """The first click on a "choose 1 of 3" screen is often ignored by the game."""

        class Game:
            def __init__(self):
                self.clicks = 0

            def state(self):
                done = self.clicks >= 2
                cs = {"screen_type": "choose", "prompt": "Choose a card.", "cards": [] if done else [STRIKE(0)]}
                return {"state_type": "map" if done else "card_select", "card_select": cs}

            def act(self, action, **params):
                self.clicks += 1
                return {"status": "ok", "message": "Choosing card"}

        game = Game()
        clock = FakeClock()
        bot = Bot(game, clock=clock.now, sleep=clock.sleep)
        bot.step()
        bot.step()
        self.assertEqual(game.clicks, 2)
        self.assertLess(clock.t, 1.5)


class CrashLoop(unittest.TestCase):
    def test_repeated_crashes_in_one_place_slow_the_bot_then_abandon_the_run(self):
        from unittest import mock
        from sts2bot import supervisor

        bot = mock.Mock()
        bot.progress.return_value = (3, 1, 6)
        bot.clock.return_value = 100.0
        bot.abandon_next = False
        bot.careful_until = 0.0
        sup = supervisor.Supervisor(bot, mock.Mock(), launch_cmd="game")
        sup._note_crash()  # a first crash can be chance
        self.assertEqual((bot.careful_until, bot.abandon_next), (0.0, False))
        with self.assertLogs("sts2bot.supervisor", level="WARNING"):
            sup._note_crash()
        self.assertEqual((bot.careful_until, bot.abandon_next), (220.0, False))
        with self.assertLogs("sts2bot.supervisor", level="WARNING"):
            sup._note_crash()
            sup._note_crash()
        self.assertTrue(bot.abandon_next)
        # Getting somewhere new starts the count again.
        bot.abandon_next = False
        bot.progress.return_value = (3, 1, 7)
        sup._note_crash()
        self.assertFalse(bot.abandon_next)


class ModUpkeep(unittest.TestCase):
    """A game update breaks the bridge mod; the supervisor rebuilds it by itself."""

    def test_load_failure_is_read_from_the_game_log(self):
        import os, time
        from unittest import mock
        from sts2bot import supervisor

        with tempfile.TemporaryDirectory() as d:
            logf = Path(d) / "godot.log"
            with mock.patch.object(supervisor, "GAME_LOG", logf):
                self.assertIsNone(supervisor.mod_load_error())
                logf.write_text("[INFO] Loading assembly DLL mods/STS2_MCP.dll\n[INFO]  --- RUNNING MODDED! --- Loaded 1 mods\n")
                self.assertIsNone(supervisor.mod_load_error())
                logf.write_text(
                    "[ERROR] Exception thrown while loading mod STS2_MCP: System.Reflection.ReflectionTypeLoadException: x\n"
                    "   at System.Reflection.RuntimeModule.GetTypes()\n"
                    "System.TypeLoadException: Could not load type 'A.LobbyPlayer' from assembly 'sts2'.\n"
                )
                self.assertIn("LobbyPlayer", supervisor.mod_load_error())
                # A log from before this launch says nothing about it.
                os.utime(logf, (time.time() - 100, time.time() - 100))
                self.assertIsNone(supervisor.mod_load_error(since=time.time() - 10))

    def test_mod_is_stale_when_the_game_files_change(self):
        from unittest import mock
        from sts2bot import supervisor

        with tempfile.TemporaryDirectory() as d:
            stamp = Path(d) / "stamp"
            with mock.patch.object(supervisor, "MOD_STAMP", stamp), mock.patch.object(
                supervisor, "game_fingerprint", return_value="v1"
            ):
                self.assertTrue(supervisor.mod_is_stale())  # never built by the bot
                stamp.write_text("v1\n")
                self.assertFalse(supervisor.mod_is_stale())
            with mock.patch.object(supervisor, "MOD_STAMP", stamp), mock.patch.object(
                supervisor, "game_fingerprint", return_value="v2"
            ):
                self.assertTrue(supervisor.mod_is_stale())

    def test_rebuilds_and_relaunches_when_the_game_rejects_the_mod(self):
        from unittest import mock
        from sts2bot import supervisor

        class Api:
            up = False

            def ping(self):
                return self.up

        api = Api()
        events = []
        state = {"error": "Could not load type X"}

        def rebuild():
            events.append("rebuild")
            state["error"] = None
            return True

        def launch(cmd):
            events.append("launch")
            api.up = state["error"] is None

        sup = supervisor.Supervisor(mock.Mock(), api, launch_cmd="game", boot_timeout=0.05)
        with mock.patch.multiple(
            supervisor,
            game_running=lambda: False,
            mod_is_stale=lambda: False,
            mod_load_error=lambda since=0.0: state["error"],
            rebuild_mod=rebuild,
            launch_game=launch,
            kill_game=lambda grace=10.0: events.append("kill"),
        ), mock.patch.object(supervisor.time, "sleep"), self.assertLogs("sts2bot.supervisor", level="ERROR"):
            sup.ensure_game()
        self.assertEqual(events, ["launch", "kill", "rebuild", "launch"])

    def test_rebuilds_before_launch_after_a_game_update(self):
        from unittest import mock
        from sts2bot import supervisor

        api = mock.Mock()
        api.ping.side_effect = [False, True]
        events = []
        sup = supervisor.Supervisor(mock.Mock(), api, launch_cmd="game")
        with mock.patch.multiple(
            supervisor,
            game_running=lambda: False,
            mod_is_stale=lambda: True,
            mod_load_error=lambda since=0.0: None,
            rebuild_mod=lambda: events.append("rebuild") or True,
            launch_game=lambda cmd: events.append("launch"),
        ), self.assertLogs("sts2bot.supervisor", level="WARNING"):
            sup.ensure_game()
        self.assertEqual(events, ["rebuild", "launch"])


class HttpClient(unittest.TestCase):
    """The real Client against a local HTTP server that fronts the fake game."""

    def test_round_trip_and_errors(self):
        import threading
        from http.server import BaseHTTPRequestHandler, HTTPServer

        from sts2bot.client import Client, GameUnavailable

        game = FakeGame(seed=6, floors=2)

        class Handler(BaseHTTPRequestHandler):
            def _send(self, code, body):
                raw = json.dumps(body).encode()
                self.send_response(code)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)

            def do_GET(self):
                self._send(200, game.state() if self.path.startswith("/api") else {"status": "ok"})

            def do_POST(self):
                body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
                result = game.act(body.pop("action"), **body)
                self._send(200 if result["status"] == "ok" else 400, result)

            def log_message(self, *a):
                pass

        server = HTTPServer(("127.0.0.1", 0), Handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        try:
            client = Client(port=server.server_address[1])
            self.assertTrue(client.ping())
            self.assertEqual(client.state()["state_type"], "menu")
            self.assertEqual(client.act("end_turn")["status"], "error")
            bot = Bot(client, poll=0.0, settle=0.05)
            bot.run(max_runs=1)
            self.assertEqual(bot.runs_finished, 1)
        finally:
            server.shutdown()
            server.server_close()
        with self.assertRaises(GameUnavailable):
            client.state()
        self.assertFalse(client.ping())


if __name__ == "__main__":
    unittest.main()
