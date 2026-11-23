"""What the test files share: a clock that does not wait, and small builders for combat states."""


from sts2bot.bot import Bot

from .fake_game import card


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
