"""The Watcher's stances (the first game), as a line of cards the planner tries plays out.

Wrath: Attacks deal double damage and she takes double from attacks. Calm: leaving it gives 2 energy. Divinity: Attacks
deal triple, entering it gives 3 energy (it ends when her next turn starts). The bot's mod puts her stance in the state
(speedmod.py); a card in hand already shows its damage in the stance she is in, and the enemies' intents what they will
deal to her in it, so only a change of stance inside the line needs working out here: the Attacks after it, the energy,
and the hit she takes when the turn ends. (Until 2026-11-24 the planner knew nothing of stances: the Watcher entered
Wrath for a card's own damage and took double the next hit.)

What a turn ending in Wrath does for the next one (double damage again) is not counted, so the planner leaves Wrath when
it can; a turn ending in Calm counts some of the energy that leaving it next turn gives.
"""

from __future__ import annotations

DEALT = {"WRATH": 2.0, "DIVINITY": 3.0}  # an Attack's damage in each stance, against no stance
TAKEN = {"WRATH": 2.0}  # damage taken from attacks
CALM_EXIT, DIVINITY_ENTRY = 2, 3  # energy for leaving Calm, for entering Divinity
CALM_LATER = 1  # energy next turn counted for a turn ended in Calm (it comes only when she leaves it)


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
