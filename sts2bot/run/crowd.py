"""What other people's runs say about each card: tierspire.com's win rate of runs holding it and how often it was taken when
offered, per character (507,000 to 662,000 runs each). A second opinion, not the rating: the numbers cannot say how *this
bot* plays a card, nor how much of a win rate is only when the card is picked (a rare taken in act 3 is only in runs that
got there). Each entry is `Name win% pick%`."""

from __future__ import annotations

import re

from .. import games

_TABLES = {
    "IRONCLAD": """
Offering 51.9 51.9, Crimson Mantle 49.1 42.6, Unmovable 48.5 42.6, Barricade 47.7 34.2, Pyre 44.6 54.8, Impervious 47.6 32.3, Dark Embrace 49.9 20.7, Cascade 48.4 26.5, Cruelty 43.2 43.8, Stoke 48.1 23.1, Aggression 46.5 28.8,
Dominate 42.3 47.0, Battle Trance 42.5 40.6, Brand 44.0 33.4, Mangle 46.7 22.3, Demon Form 42.5 38.3, Tear Asunder 45.5 23.7, Colossus 40.6 42.9, Not Yet 43.4 31.7, One-Two Punch 42.5 31.7, Conflagration 42.3 31.1, Thrash 42.5 27.1,
Pact's End 43.6 22.8, Fiend Fire 44.7 18.4, Burning Pact 41.7 28.5, Hellraiser 39.6 34.6, Feel No Pain 43.6 18.2, Juggernaut 42.0 22.0, Bully 40.1 28.9, Vicious 42.4 19.4, Forgotten Ritual 41.7 21.3, Taunt 37.6 36.5,
Drum of Battle 42.3 13.5, Second Wind 40.6 18.1, Uppercut 37.7 26.8, Bloodletting 36.3 32.1, Pommel Strike 35.2 35.7, Evil Eye 38.6 21.9, Ashen Strike 39.0 20.3, Stone Armor 38.3 21.8, Flame Barrier 35.3 33.5, Rage 35.5 31.4,
Shrug It Off 35.2 32.5, Pillage 39.1 16.9, Feed 37.2 23.9, Expect a Fight 37.7 20.8, Rupture 37.2 22.0, Body Slam 39.3 13.4, Molten Fist 35.5 27.2, Juggling 38.8 11.2, Inflame 34.4 28.1, Stampede 35.1 24.1, Infernal Blade 35.0 24.7,
Dismantle 34.6 25.6, Tremble 34.8 23.5, Spite 36.8 14.6, True Grit 35.8 15.3, Headbutt 35.7 15.1, Primal Force 35.6 15.1, Inferno 34.6 19.0, Whirlwind 33.9 21.5, Blood Wall 33.7 18.7, Havoc 36.6 6.1, Howl from Beyond 34.0 16.3,
Unrelenting 32.7 20.4, Armaments 32.6 20.0, Bludgeon 32.5 19.8, Stomp 31.6 22.0, Fight Me! 32.1 18.0, Sword Boomerang 33.7 9.7, Thunderclap 31.8 16.8, Twin Strike 31.0 15.7, Perfected Strike 28.9 22.4, Hemokinesis 31.2 12.6,
Iron Wave 30.9 13.1, Setup Strike 30.0 16.5, Anger 29.8 16.2, Rampage 29.2 17.9, Breakthrough 28.1 14.9, Cinder 25.5 5.2
""",
    "SILENT": """
Afterimage 55.5 57.2, Adrenaline 53.6 64.5, Knife Trap 53.8 43.3, Blade of Ink 52.8 42.9, Tools of the Trade 52.0 40.9, Tracking 51.9 33.0, Abrasive 48.9 43.7, Fan of Knives 47.8 45.5, Malaise 53.7 18.7, Serpent Form 49.1 34.1,
Assassinate 48.5 32.5, Burst 47.7 34.0, Calculated Gamble 45.7 37.5, Envenom 45.3 37.5, Expose 44.2 41.0, Hidden Daggers 43.1 45.1, Storm of Steel 46.9 27.4, Acrobatics 43.4 43.1, Accuracy 44.0 39.5, Escape Plan 43.8 38.3,
Accelerant 45.1 31.2, Corrosive Wave 46.3 24.5, Nightmare 47.2 19.2, Sidestep 43.2 35.2, Footwork 42.1 38.6, Bullet Time 44.4 25.7, Master Planner 44.3 26.6, Shadow Step 45.3 21.9, Reflex 44.2 24.9, Blur 42.1 34.3,
Infinite Blades 42.7 30.9, Shadowmeld 44.8 21.0, Tactician 42.3 31.2, Phantom Blades 42.7 25.6, Prepared 38.9 40.3, Well-Laid Plans 42.7 22.7, Murder 44.4 14.9, Piercing Wail 40.6 31.6, Up My Sleeve 39.8 34.1, Expertise 44.2 11.9,
Leg Sweep 40.8 27.1, Backflip 40.1 29.9, Noxious Fumes 38.2 34.6, Blade Dance 37.6 34.2, Finisher 40.6 19.3, Mirage 40.8 16.2, Cloak and Dagger 37.6 30.3, Bubble Bubble 38.5 24.5, Haze 36.4 27.5, Leading Strike 36.2 26.1,
Backstab 35.8 28.1, Outbreak 37.7 18.6, Strangle 37.7 18.1, Speedster 39.1 11.5, Deflect 37.4 18.5, Hand Trick 37.2 19.0, The Hunt 37.7 15.6, Untouchable 36.9 17.5, Bouncing Flask 35.7 21.6, Grand Finale 38.7 6.5,
Dodge and Roll 37.2 12.2, Flechettes 36.7 10.4, Pinpoint 35.8 12.8, Memento Mori 35.9 8.1, Echoing Slash 33.8 17.7, Anticipate 35.1 11.2, Dagger Throw 33.4 18.7, Dash 33.4 16.0, Ricochet 33.4 15.0, Precise Cut 34.2 9.8,
Snakebite 32.6 14.4, Deadly Poison 31.8 17.7, Skewer 34.1 5.2, Sucker Punch 33.4 8.5, Pounce 32.6 11.5, Predator 32.5 6.0, Slice 31.8 9.3, Flick-Flack 30.3 13.9, Poisoned Stab 29.7 14.0, Dagger Spray 27.7 11.2
""",
    "DEFECT": """
Echo Form 47.2 64.4, Machine Learning 47.5 44.7, Defragment 45.6 57.3, All for One 47.8 31.5, Reboot 48.1 29.8, Buffer 47.2 31.7, Supercritical 45.1 40.3, Modded 43.1 47.3, Flak Cannon 46.2 29.6, Creative AI 43.2 42.8,
Signal Boost 44.4 34.7, Trash to Treasure 44.3 29.8, Skim 44.4 24.8, Overclock 42.3 33.2, Coolant 44.1 24.0, Voltaic 41.4 34.7, Helix Drill 44.3 19.5, Ice Lance 43.2 23.8, Spinner 40.5 35.5, Glacier 38.4 38.4, Subroutine 39.7 31.6,
Iteration 42.5 17.3, White Noise 36.2 46.4, Meteor Strike 42.9 11.9, Chill 38.1 32.5, Boot Sequence 36.9 37.9, Compact 38.0 32.3, Rocket Punch 41.2 16.0, Genetic Algorithm 38.8 26.8, Capacitor 35.3 40.6, Double Energy 37.5 28.9,
Storm 38.2 25.1, Shatter 36.6 31.1, Hologram 37.2 27.8, Fight Through 38.0 23.1, Scavenge 38.2 21.8, Feral 37.8 23.4, Bulk Up 38.3 18.9, Scrape 39.4 12.7, Adaptive Strike 37.6 20.9, FTL 36.6 24.7, Rainbow 37.8 19.0, TURBO 36.1 26.6,
Thunder 34.4 33.2, Multi-Cast 37.9 16.9, Coolheaded 35.6 25.2, Loop 34.1 31.6, Consuming Shadow 36.4 18.1, Hailstorm 36.1 19.7, Smokestack 36.3 18.1, Tempest 35.0 23.6, Compile Driver 37.9 9.8, Synchronize 35.7 16.6,
Go for the Eyes 34.5 22.1, Shadow Shield 33.6 25.9, Null 35.5 16.4, Boost Away 34.8 19.8, Charge Battery 31.5 32.0, Beam Cell 33.4 19.4, Glasswork 32.1 24.3, Darkness 32.2 23.7, Hotfix 32.5 21.4, Hyperbeam 35.1 8.6, Fusion 32.6 19.7,
Chaos 32.5 18.3, Synthesis 34.3 9.4, Tesla Coil 31.1 23.4, Lightning Rod 30.0 25.8, Claw 29.3 29.1, Cold Snap 30.5 17.0, Leap 30.1 16.1, Gunk Up 30.8 11.3, Refract 30.0 13.9, Barrage 30.5 11.5, Uproar 30.3 8.9, Ball Lightning 27.1 20.6,
Focused Strike 28.4 13.3, Momentum Strike 28.3 12.8, Sweeping Beam 28.6 11.4, Sunder 27.3 10.7
""",
    "REGENT": """
Big Bang 51.3 68.5, Decisions, Decisions 50.3 39.8, Void Form 47.4 56.8, Tyranny 51.9 24.8, Genesis 45.2 46.2, Sword Sage 46.5 38.2, Comet 47.4 33.1, Dying Star 46.4 37.0, Arsenal 47.6 30.9, GUARDS!!! 47.6 25.4, Make It So 46.8 27.9,
Bombardment 45.5 30.4, Pale Blue Dot 49.9 8.8, Monarch's Gaze 47.0 20.0, Foregone Conclusion 46.6 18.6, Bundle of Joy 46.3 19.2, Heavenly Drill 45.9 20.8, The Smith 42.6 34.5, Neutron Aegis 45.1 23.3, Orbit 42.3 34.0,
Seeking Edge 41.2 37.0, Convergence 40.8 38.7, Royal Gamble 41.3 35.6, Bulwark 39.2 45.0, Prophesize 47.1 7.4, I Am Invincible 43.2 23.4, Reflect 38.2 46.2, Child of the Stars 40.8 33.7, Glimmer 44.5 16.6, Seven Stars 41.9 26.9,
Alignment 40.9 30.3, Particle Wall 40.2 32.6, Pillar of Creation 43.5 16.4, Spectrum Shift 42.5 21.1, CHARGE!! 39.5 31.6, Gamma Blast 40.6 26.0, Summon Forth 38.8 33.7, Manifest Authority 39.8 28.0, Beat into Shape 40.6 24.4,
Heirloom Hammer 42.5 14.6, Stardust 40.5 23.6, Quasar 40.1 20.0, Glow 37.6 31.5, Radiate 39.4 21.5, Crash Landing 40.4 16.5, Conqueror 37.6 28.6, Cosmic Indifference 38.3 24.3, Black Hole 37.2 29.3, Supermassive 40.6 13.9,
Parry 38.1 22.8, Furnace 36.8 28.7, Gather Light 34.7 36.8, Royalties 38.1 20.1, Lunar Blast 40.2 9.5, Know Thy Place 36.8 23.4, Hidden Cache 35.2 30.2, Resonance 37.7 16.4, Shining Strike 35.3 27.4, Monologue 36.9 15.0,
Cloak of Stars 35.1 21.8, BEGONE! 35.7 18.5, Spoils of Battle 35.7 16.7, Patter 35.0 18.9, Hegemony 35.5 15.5, Refine Blade 33.5 22.2, Glitterstream 34.5 16.0, Terraforming 35.5 11.4, Guiding Star 35.5 11.0, Devastate 33.1 14.9,
Photon Cut 34.2 9.5, Collision Course 32.6 10.5, Crescent Spear 31.3 14.4, Solar Strike 30.9 15.6, Crush Under 30.2 14.7, Wrought in War 29.8 14.0, Kingly Kick 30.0 12.2, Kingly Punch 29.7 12.9, Astral Pulse 28.7 15.4,
Knockout Blow 29.0 11.6, Celestial Might 29.1 6.2
""",
    "NECROBINDER": """
Neurosurge 53.4 41.3, Demesne 51.7 57.3, Shared Fate 52.3 20.0, Call of the Void 47.6 35.9, Transfigure 47.5 34.3, Enfeebling Touch 44.4 45.9, Reanimate 46.8 35.0, Banshee's Cry 49.7 23.9, Debilitate 48.2 28.7, Seance 48.5 27.2,
Misery 46.1 34.1, Reaper Form 44.2 40.2, Oblivion 45.2 35.9, Eradicate 48.6 21.8, Friendship 43.7 36.5, Dredge 48.0 19.9, Spirit of Ash 47.3 22.0, Borrowed Time 44.0 32.9, Necro Mastery 43.2 35.7, Undeath 43.4 32.8, Time's Up 45.5 25.0,
Sentry Mode 43.6 31.7, Putrefy 43.6 31.0, Devour Life 42.9 31.5, Dirge 42.0 34.4, Hang 42.7 31.6, Lethality 40.8 37.2, Parse 45.9 17.1, Squeeze 43.6 24.4, End of Days 40.0 36.3, Pagestorm 45.8 11.9, Capture Spirit 39.5 32.6,
Delay 39.2 30.2, No Escape 37.4 36.7, Fetch 39.1 30.1, Soul Storm 41.3 21.8, Sleight of Flesh 40.6 23.3, Danse Macabre 43.3 12.9, Deathbringer 36.6 37.3, Cleanse 41.1 19.8, Defy 37.6 31.8, Shroud 39.9 21.7, Death's Door 38.7 26.1,
Grave Warden 35.9 34.3, Graveblast 40.8 15.0, Sacrifice 41.3 13.5, Pull from Below 41.3 12.9, Eidolon 43.4 5.5, Wisp 36.9 29.1, Spur 36.3 30.3, Invoke 36.6 27.9, Haunt 38.1 21.9, Countdown 36.0 29.0, High Five 36.2 25.3,
Right Hand Hand 37.2 20.4, Veilpiercer 37.9 16.5, Severance 37.0 18.4, The Scythe 36.6 19.5, Death March 37.7 11.7, Bury 38.3 9.5, Calcify 35.7 19.1, Rattle 34.5 18.0, Melancholy 34.9 14.3, Sic 'Em 33.3 20.2, Afterlife 34.2 17.0,
Scourge 33.4 19.8, Negative Pulse 30.8 29.6, Fear 34.7 14.7, Snap 33.9 14.2, Bone Shards 32.9 17.6, Sculpting Strike 35.0 7.8, Pull Aggro 32.0 15.6, Flatten 30.8 19.6, Poke 30.9 19.3, Reave 29.8 12.8, Drain Power 29.5 13.5,
Blight Strike 28.7 13.3, Reap 29.6 9.0, Defile 29.1 10.0, Sow 25.2 10.4
""",
    # (Colorless cards are offered rarely and late, so their win rates run high: scored among themselves, the rarest left out.)
    "COLORLESS": """
Finesse 48.2 9.3, Dark Shackles 49.2 8.2, Production 48.0 8.9, Hidden Gem 52.7 1.1, Flash of Steel 48.9 7.7, Prowess 47.0 8.5, Automation 49.2 6.7, Shockwave 48.3 6.6, Ultimate Defend 47.0 5.3, Purity 53.0 4.1, Discovery 43.9 8.3,
Prolong 46.6 5.8, Thinking Ahead 50.0 3.9, Jack of All Trades 51.4 3.4, Ultimate Strike 45.2 1.8, Fasten 39.2 9.7, Salvo 49.4 3.1, Panache 43.7 4.6, Equilibrium 43.2 4.4, Stratagem 46.0 2.8, Catastrophe 45.1 2.4, Fisticuffs 39.3 5.3,
Dramatic Entrance 36.7 6.4, Prep Time 37.8 3.7, Omnislice 36.0 4.2, Impatience 49.8 2.1, Mind Blast 47.8 1.8, Panic Button 37.2 1.3, Restlessness 45.1 2.0, Seeker Strike 49.4 2.5, The Bomb 29.7 2.7, Thrumming Hatchet 34.6 2.6,
Volley 46.0 1.8
""",
}

SPREAD = 1.5  # rating points for one standard deviation of a character's cards
MIDDLE = 5.0  # the rating of a card average on both counts: commons then keep the level our own table gives them


def _key(name: str) -> str:
    """A card's name as the game spells its id: TIMES_UP for "Time's Up"."""
    return re.sub(r"[^A-Z0-9]+", "_", name.upper().replace("'", "")).strip("_")


def _ratings() -> dict:
    out: dict = {}
    for table in _TABLES.values():
        rows = [(m.group(1).strip(), float(m.group(2)), float(m.group(3))) for m in re.finditer(r"\s*(.+?) (\d+\.\d) (\d+\.\d)(?:,|\s*$)", table, re.S)]
        zs = []
        for col in (1, 2):
            mean = sum(r[col] for r in rows) / len(rows)
            sd = (sum((r[col] - mean) ** 2 for r in rows) / len(rows)) ** 0.5 or 1.0
            zs.append([(r[col] - mean) / sd for r in rows])
        for (name, _win, _pick), won, taken in zip(rows, *zs):
            # (The site's own tiers weigh the two the same way.)
            out[_key(name)] = max(1.5, min(8.0, MIDDLE + SPREAD * (0.7 * won + 0.3 * taken)))
    return out


RATING = _ratings()
# The Fool has no table on tierspire.com: one writer's tiers (gamerblurb.com), drawn in a little, for what cards.parse cannot read (Aim, Melodies).
_FOOL_TIERS = {
    7.2: "Wah Cho Ki, Motif, Song Path, Wrest, Cho Ready, Wah Cho, Quiver, Violent Chord, Mythic Form",
    5.8: """Archery, Bow Smack, Cho Firm, Cho Ki, Coda, Explosive Arrows, Harmonic Shield, Hidden Roots, Ki Hop, Ki Rest, Ki Verse, Ki Whip, Lullaby, Melodic Shot, Rhythm Strike, Rummage, Serrated Arrows, Sky Shot, Vitriol, Wah Blast,
        Wah Hammer, Wah Ki, Wah Sweep, Wah Symphony, Yah!""",
    4.5: """All Is Well, Anguish, Arrow Stab, Blossom Shot, Bounce, Brace, Cho Barbs, Cho Chorus, Cho Limbs, Deep Stretch, Dislike You, Ecstasy, Encore, Ennui, Growth, Heavy Shot, Hobbling Shot, Honing, Ki Chant, Nail, Nock,
        Punch of Joy, Rain Shot, Rapid Shot, Recall, Repeat Shot, Show Off, Steady Cadence, Uproot, Wah Touch, Whistling Arrows""",
    3.3: "Blot out the Sky, Brighten Up, Epic Retrieval, Florescence, Jest, Locked In, Mourning Blow, Quick Shot, Quietus, Simpleton, Sorrowful Strike, Timber, Trick Shot, Tune",
}
FOOL_RATING = {_key(name): value for value, names in _FOOL_TIERS.items() for name in re.split(r",\s*", " ".join(names.split()))}


def rating(key: str) -> float | None:
    """What the crowd's numbers make a card worth on the scale of cards._TIER (None: none); the first game's: games/sts1/tiers.py."""
    if not games.uses_sts2_tables():
        from ..games.sts1 import tiers  # (only when playing the first game)
        return tiers.rating(key)
    got = RATING.get(key.replace("_S_", "S_") if key not in RATING else key)
    return FOOL_RATING.get(key) if got is None else got


# Which cards win together: the site's groups of runs with alike final decks that won more than their character's runs (share won, cards).
_BUILDS = [
    (59.5, "Maul, Tear Asunder, Not Yet"), (56.8, "Feel No Pain, Dark Embrace, Stoke"), (47.4, "Mad Science, Juggernaut, Body Slam"), (67.6, "Maul, Ultimate Strike, Mad Science"), (63.6, "Phantom Blades, Blade of Ink, Accuracy"),
    (51.9, "Nightmare, Grand Finale, Corrosive Wave"), (47.4, "Knife Trap, Fan of Knives, Blade Dance"), (45.6, "Accelerant, Bubble Bubble, Mirage"), (45.1, "Ricochet, Reflex, Speedster"),
    (74.7, "Monarch's Gaze, Big Bang, Decisions Decisions"), (60.8, "The Sealed Throne, Black Hole, Mad Science"), (53.7, "Ultimate Strike, Mad Science, Tyranny"), (46.0, "Maul, Mad Science, Big Bang"),
    (63.8, "Forbidden Grimoire, Oblivion, Time's Up"), (57.9, "Ultimate Strike, Mad Science, Banshee's Cry"), (55.0, "Shroud, Death's Door, Sleight of Flesh"), (50.1, "Squeeze, Rattle, Calcify"),
    (73.7, "Meteor Strike, Helix Drill, Trash to Treasure"), (54.1, "Rocket Punch, Smokestack, Flak Cannon"), (52.4, "Mad Science, Storm, Thunder"), (51.1, "Maul, Ultimate Strike, Mad Science"),
]
BUILDS = [(won, frozenset(_key(n) for n in names.split(", "))) for won, names in _BUILDS]
PARTNER_MOST = 1.5


def partner(key: str, held: set) -> float:
    """What a card is worth on top of its own rating for going with cards the deck already holds (Dark Embrace with Feel
    No Pain): from 0.3 for a group won 45% of the time to 1.5 at 63% and beyond."""
    best = 0.0
    for won, cards in BUILDS:
        if key in cards and (cards - {key}) & held:
            best = max(best, min(PARTNER_MOST, (won - 40.0) / 15.0))
    return best
