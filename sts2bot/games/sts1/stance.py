"""The Watcher's stances along a line of cards the planner tries. Wrath: Attacks deal and take double. Calm: leaving it
gives 2 energy. Divinity: Attacks deal triple, entering it gives 3 energy. Cards and intents show the stance the turn began
in, so only a change inside the line is worked out. A turn ending in Wrath is not charged for the next (so the planner
leaves it when it can); one ending in Calm counts some of the energy that leaving it gives."""

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
