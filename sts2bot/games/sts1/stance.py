"""The Watcher's stances along a line of cards the planner tries. Wrath: Attacks deal and take double. Calm: leaving it
gives 2 energy. Divinity: Attacks deal triple, entering it gives 3 energy. Cards and intents show the stance the turn began
in, so only a change inside the line is worked out. A turn ending in Wrath is charged for the next when the next hand may
hold no way out and the doubled blows would kill her (`stuck_odds`, `carry_cost`); one ending in Calm counts some of the
energy that leaving it gives."""

from __future__ import annotations

import re
from math import comb

from ...cards import parse

DEALT = {"WRATH": 2.0, "DIVINITY": 3.0}  # an Attack's damage in each stance, against no stance
TAKEN = {"WRATH": 2.0}  # damage taken from attacks
CALM_EXIT, DIVINITY_ENTRY = 2, 3  # energy for leaving Calm, for entering Divinity
CALM_LATER = 1  # energy next turn counted for a turn ended in Calm (it comes only when she leaves it)
MANTRA_DIVINITY = 10  # Mantra that puts her in Divinity, and is used up doing it
MANTRA_LATER = 2.0  # points a Mantra short of that is worth (Divinity: 3 energy and Attacks tripled for a turn, about 45, some turns away)
NEXT_HAND = 5  # cards a new hand draws
# A turn ended in Wrath whose doubled blows next turn would kill her if no way out is drawn: points for that chance (the
# planner's near-kill lines are worth 400). 2026-11-25 17:54: the Watcher ended a Boost turn in Wrath on 31 HP with no
# Calm card in the deck's next five; the Bronze Automaton's Hyper Beam came to 104.
WRATH_DEATH = 400.0
STUCK_BLOCK = 10  # Block a hand with no way out of Wrath still puts up


def read(stance) -> str:
    """WRATH, CALM, DIVINITY, or "" (no stance: "Neutral", or a game without stances)."""
    s = str(stance or "").upper()
    return s if s in DEALT or s == "CALM" else ""


class Stance:
    def __init__(self, start) -> None:
        self.start = self.now = read(start)

    def deal(self, per: int) -> int:
        """An Attack's damage a hit, from what its text shows (in the stance the turn began in) to the stance now."""
        return round(per * DEALT.get(self.now, 1.0) / DEALT.get(self.start, 1.0))

    def enter(self, to: str) -> int:
        """Change to `to` (a card's CardInfo.stance: WRATH, CALM, DIVINITY or EXIT); the energy that gives."""
        new = "" if to == "EXIT" else to
        if not to or new == self.now:
            return 0
        gained = (CALM_EXIT if self.now == "CALM" else 0) + (DIVINITY_ENTRY if new == "DIVINITY" else 0)
        self.now = new
        return gained

    def taken(self) -> float:
        """What the enemies' attacks (as their intents show them, in the stance the turn began in) come to now."""
        return TAKEN.get(self.now, 1.0) / TAKEN.get(self.start, 1.0)

    def later_energy(self) -> int:
        return CALM_LATER if self.now == "CALM" else 0


def leaves_wrath(card: dict) -> bool:
    """Played, it takes her out of Wrath: into Calm or Divinity, or out of any stance (Inner Peace's "otherwise Enter Calm" too;
    not Blasphemy: she dies next turn)."""
    info = parse(card)
    words = str(card.get("description") or "")
    return (info.stance in ("CALM", "DIVINITY", "EXIT") or bool(re.search(r"\benter\s+(?:calm|divinity)\b|\bexit\s+your\s+stance\b", words, re.I))) and not info.deadly


def stuck_odds(player: dict) -> float:
    """The chance that next turn's hand holds no card that leaves Wrath (none when one is Retained in the hand now). The
    draw comes off the draw pile; what it lacks comes from the discard pile, this turn's hand in it, shuffled."""
    hand = player.get("hand") or []
    if any(leaves_wrath(c) and re.search(r"\bretain\b", str(c.get("description") or ""), re.I) for c in hand):
        return 0.0
    draw = player.get("draw_pile") or []
    need, pool = NEXT_HAND, draw
    if len(draw) < need:
        if any(map(leaves_wrath, draw)):
            return 0.0
        need, pool = need - len(draw), [*(player.get("discard_pile") or []), *hand]
    size, outs = len(pool), sum(map(leaves_wrath, pool))
    if not size:
        return 1.0
    need = min(need, size)
    return comb(size - outs, need) / comb(size, need)


def carry_cost(odds: float, blow: float, hp_left: int, attacks: float = 1.0) -> float:
    """What ending the turn in Wrath costs: the chance of no way out next turn and of an attack then (`attacks`: how often the
    enemies have attacked), if their usual blows (`blow`, as they are outside Wrath) doubled would get through a hand's Block
    (`STUCK_BLOCK`) and kill her. Not the HP a doubled blow takes short of that: in the model runs charging it too lost 1.1
    floors a run (act 1 bosses 57 deaths in 533 runs against 37: fights went on longer without Wrath)."""
    return odds * attacks * WRATH_DEATH if odds > 0 and blow > 0 and 0 < hp_left <= 2 * blow - STUCK_BLOCK else 0.0
