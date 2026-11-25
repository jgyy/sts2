"""The Watcher's Wrath carried into the next turn: what the planner charges for ending a turn in it."""

from sts2bot.fight import combat
from sts2bot.games.sts1 import stance
from sts2bot.games.sts1.screens import Screens

from .helpers import Sts1Case

CARDS = {"Eruption": ("ATTACK", 1, True), "Strike_P": ("ATTACK", 1, True), "Defend_P": ("SKILL", 1, False), "Vigilance": ("SKILL", 2, False),
         "ClearTheMind": ("SKILL", 1, False), "Blasphemy": ("SKILL", 1, False)}
NAMES = {"Strike_P": "Strike", "Defend_P": "Defend", "ClearTheMind": "Tranquility"}


def card(cid: str, index: int | None = None) -> dict:
    kind, cost, aimed = CARDS[cid]
    return Screens().card({"id": cid, "name": NAMES.get(cid, cid), "type": kind, "cost": cost, "has_target": aimed}, index, live=index is not None)


def watcher(hand, draw, *, hp=31, blow=40, stance_now="Neutral", intent=None):
    enemy = {"entity_id": "BRONZEAUTOMATON_0", "hp": 200, "max_hp": 300, "block": 0, "status": [],
             "intents": [{"type": "Attack", "label": str(intent)}] if intent else [{"type": "Defend", "label": ""}, {"type": "Buff", "label": ""}]}
    player = {"hp": hp, "max_hp": 72, "block": 0, "energy": 2, "hand": [card(c, n) for n, c in enumerate(hand)], "draw_pile": [card(c) for c in draw],
              "discard_pile": [], "draw_pile_count": len(draw), "discard_pile_count": 0, "exhaust_pile_count": 0, "potions": [], "stance": stance_now, "orbs": []}
    return {"state_type": "boss", "run": {"act": 2, "floor": 33, "ascension": 0}, "player": player, "_blow": {"BRONZEAUTOMATON_0": (blow, 0.6)},
            "battle": {"round": 5, "turn": "player", "is_play_phase": True, "enemies": [enemy]}}


class WrathCarry(Sts1Case):
    def test_the_odds_of_no_way_out_come_from_the_next_draw(self):
        player = watcher([], ["Strike_P"] * 5 + ["Defend_P"] * 5)["player"]
        self.assertEqual(stance.stuck_odds(player), 1.0)
        player = watcher([], ["Strike_P"] * 4 + ["Vigilance"])["player"]
        self.assertEqual(stance.stuck_odds(player), 0.0)  # (all five are drawn)
        player = watcher([], ["Strike_P"] * 9 + ["Vigilance"])["player"]
        self.assertAlmostEqual(stance.stuck_odds(player), 0.5)
        player = watcher(["ClearTheMind"], ["Strike_P"] * 10)["player"]
        self.assertEqual(stance.stuck_odds(player), 0.0)  # (Tranquility is Retained)
        player = watcher([], ["Strike_P"] * 9 + ["Blasphemy"])["player"]
        self.assertEqual(stance.stuck_odds(player), 1.0)  # (Divinity, then death)

    def test_no_wrath_before_a_blow_she_could_not_leave_it_for(self):
        # A Boost turn, 31 HP, a 40 blow seen already and no Calm card in the next five: Eruption would leave her in Wrath for it.
        state = watcher(["Eruption", "Strike_P", "Strike_P"], ["Strike_P"] * 5 + ["Defend_P"] * 5)
        self.assertEqual(combat.actions(state)[0], ("play_card", {"card_index": 1, "target": "BRONZEAUTOMATON_0"}))

    def test_wrath_when_the_next_hand_holds_a_way_out(self):
        state = watcher(["Eruption", "Strike_P", "Strike_P"], ["Strike_P"] * 4 + ["Vigilance"])
        self.assertEqual(combat.actions(state)[0], ("play_card", {"card_index": 0, "target": "BRONZEAUTOMATON_0"}))

    def test_wrath_when_the_blows_are_small(self):
        state = watcher(["Eruption", "Strike_P", "Strike_P"], ["Strike_P"] * 5 + ["Defend_P"] * 5, hp=72, blow=4)
        self.assertEqual(combat.actions(state)[0], ("play_card", {"card_index": 0, "target": "BRONZEAUTOMATON_0"}))

    def test_the_stance_potion_is_a_choice_the_planner_weighs(self):
        from sts2bot.run import strategy
        s = Screens()
        s.raw = {"available_commands": ["choose"], "game_state": {"screen_type": "CARD_REWARD", "room_phase": "COMBAT", "screen_state": {"cards": [
            {"id": "Wrath", "name": "Wrath", "type": "POWER", "cost": -2}, {"id": "Calm", "name": "Calm", "type": "POWER", "cost": -2}]}}}
        offer = s._card_reward()
        self.assertEqual([c["description"] for c in offer["card_reward"]["cards"]], ["Enter Wrath.", "Enter Calm."])
        # On 31 HP with a 40 blow seen and no way out of Wrath in the next five: Calm.
        fight = watcher(["Strike_P", "Strike_P"], ["Strike_P"] * 5 + ["Defend_P"] * 5)
        state = {**offer, "state_type": "card_reward", "_fight_state": fight}
        self.assertEqual(strategy.card_reward(state, strategy.Memory())[0], ("select_card_reward", {"card_index": 1}))


class Mantra(Sts1Case):
    """Mantra: at 10 she enters Divinity (Attacks tripled, 3 energy) at once."""

    def test_mantra_that_reaches_ten_is_divinity_now(self):
        prostrate = Screens().card({"id": "Prostrate", "name": "Prostrate", "type": "SKILL", "cost": 0}, 0, live=True)
        state = watcher(["Strike_P", "Strike_P"], ["Defend_P"] * 5, hp=72, blow=4)
        state["player"]["hand"] = [prostrate, card("Strike_P", 1), card("Strike_P", 2)]
        state["player"]["status"] = [{"id": "MANTRA_POWER", "amount": 8}]
        state["battle"]["enemies"][0]["hp"] = 30  # (two Strikes tripled kill it: 6 x 3 x 2 = 36)
        from sts2bot.cards import parse
        self.assertEqual(parse(prostrate).mantra, 2)
        ctx, foes, line = combat._context(state), combat.read_enemies(state), [parse(c) for c in state["player"]["hand"]]
        self.assertEqual(ctx["mantra0"], 8)
        self.assertGreater(combat.simulate(line, foes, **ctx).score, combat.WIN_BONUS)  # (Prostrate first: Divinity, then the kill)
        self.assertLess(combat.simulate(line[1:] + line[:1], foes, **ctx).score, combat.WIN_BONUS)  # (Prostrate last: too late)
