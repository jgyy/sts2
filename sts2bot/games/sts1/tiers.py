"""How good each of the first game's cards is, by tiers: the community's consensus (no run-based table exists), the second
opinion crowd.py gives Slay the Spire 2's cards (on paper alone, Poisoned Stab beat Acrobatics). On crowd.py's scale
(middle 5.0), mixed in by the same `crowd` share, and to be corrected by the bot's own record (learn.CardStats)."""

from __future__ import annotations

import re

from . import text

S, A, B, C, D = 7.5, 6.5, 5.5, 4.5, 3.5

# Each tier lists the Ironclad's cards, then the Silent's, the Defect's, the Watcher's and the colorless ones.
_TIERS = {
    S: "Offering, Shockwave, Feed, Immolate, Impervious, Demon Form",
    A: """Shrug It Off, Battle Trance, Inflame, Flame Barrier, Uppercut, Disarm, Whirlwind, Pommel Strike, Carnage, Corruption, Feel No Pain, Seeing Red, Bloodletting, Power Through, Spot Weakness, Reaper, Fiend Fire,
        Adrenaline, Wraith Form, Footwork, After Image, Malaise, Noxious Fumes, Well-Laid Plans, Piercing Wail, Acrobatics, Leg Sweep, Crippling Cloud, Tools of the Trade, Backflip, Blur, Catalyst,
        Defragment, Echo Form, Biased Cognition, Glacier, Coolheaded, Buffer, Electrodynamics, Machine Learning, Seek, Loop, Skim, Cold Snap,
        Rushdown, Talk to the Hand, Mental Fortress, Tantrum, Inner Peace, Cut Through Fate, Empty Mind, Scrawl, Ragnarok, Conclude, Wallop, Fear No Evil,
        Apotheosis, Master of Strategy, Apparition""",
    B: """Cleave, Thunderclap, Headbutt, True Grit, Armaments, Ghostly Armor, Burning Pact, Second Wind, Dark Embrace, Bludgeon, Hemokinesis, Sentinel, Metallicize, Exhume, Rage, Pummel, Intimidate, Dropkick, Combust,
        Burst, Dash, Bouncing Flask, Calculated Gamble, Die Die Die, Glass Knife, Predator, Terror, Skewer, Heel Hook, Corpse Explosion, Storm of Steel, A Thousand Cuts, Blade Dance, Cloak and Dagger, Dagger Throw, Deadly Poison, Prepared,
        All-Out Attack, Escape Plan, Dodge and Roll,
        Ball Lightning, Compile Driver, Hologram, Sweeping Beam, Chill, Equilibrium, Force Field, Reinforced Body, Auto-Shields, Capacitor, Consume, Doom and Gloom, Fusion, Overclock, Scrape, Tempest, Double Energy, Core Surge, Hyperbeam, Multi-Cast,
        Meteor Strike, Fission, All for One, Thunder Strike, Charge Battery, Stack,
        Bowling Bash, Crescendo, Empty Body, Flying Sleeves, Follow-Up, Protect, Third Eye, Tranquility, Deceive Reality, Fasting, Foresight, Meditate, Simmering Fury, Wave of the Hand, Wheel Kick, Brilliance, Devotion, Lesson Learned, Omniscience,
        Vault, Sanctity, Battle Hymn, Wreath of Flame,
        Bite, Discovery, Dramatic Entrance, Hand of Greed, Madness, Panic Button, Secret Technique, Secret Weapon, The Bomb, Trip""",
    C: """Anger, Twin Strike, Iron Wave, Heavy Blade, Perfected Strike, Clothesline, Sword Boomerang, Body Slam, Infernal Blade, Limit Break, Barricade, Juggernaut, Double Tap, Brutality, Berserk, Entrench, Dual Wield, Rampage, Blood for Blood,
        Searing Blow, Wild Strike,
        Expertise, Eviscerate, Accuracy, Backstab, Bane, Choke, Concentrate, Dagger Spray, Deflect, Flying Knee, Outmaneuver, Poisoned Stab, Quick Slash, Sneaky Strike, Sucker Punch, Slice, Finisher, Flechettes, Riddle with Holes, Setup, Reflex,
        Tactician, Unload, Infinite Blades, Endless Agony, Distraction, Phantasmal Killer, Nightmare, Envenom, Bullet Time, Doppelganger, Alchemize,
        Barrage, Beam Cell, Claw, Go for the Eyes, Leap, Rebound, Recursion, Steam Barrier, Streamline, Turbo, Aggregate, Blizzard, Boot Sequence, Bullseye, Chaos, Darkness, FTL, Genetic Algorithm, Heatsinks, Hello World, Melter, Recycle, Reprogram,
        Rip and Tear, Self Repair, Static Discharge, Storm, Sunder, White Noise, Amplify, Creative AI, Rainbow, Reboot,
        Consecrate, Crush Joints, Empty Fist, Evaluate, Flurry of Blows, Halt, Just Lucky, Pressure Points, Prostrate, Sash Whip, Carve Reality, Collect, Foreign Influence, Indignation, Like Water, Nirvana, Perseverance, Pray, Reach Heaven,
        Sands of Time, Signature Move, Study, Swivel, Weave, Windmill Strike, Worship, Alpha, Blasphemy, Conjure Blade, Deus Ex Machina, Deva Form, Establishment, Judgment, Master Reality, Spirit Shield, Wish,
        Bandage Up, Blind, Chrysalis, Dark Shackles, Deep Breath, Enlightenment, Finesse, Flash of Steel, Forethought, Good Instincts, Impatience, Jack of All Trades, Magnetism, Mayhem, Metamorphosis, Mind Blast, Panacea, Panache, Purity,
        Swift Strike, Thinking Ahead, Transmutation, Violence, Ritual Dagger""",
    D: "Flex, Havoc, Warcry, Reckless Charge, Clash, Sever Soul, Rupture, Fire Breathing, Evolve, Grand Finale, Caltrops, Masterful Stab, Sadistic Nature",
}


def _key(name: str) -> str:
    return re.sub(r"[^A-Z0-9]+", "_", name.upper().replace("'", "")).strip("_")


TIER = {_key(n): value for value, names in _TIERS.items() for n in re.split(r",\s*", " ".join(names.split()))}


def rating(key: str) -> float | None:
    """A card's tier on crowd.py's scale, by the game's id in key form; None for the starter cards, curses and the like."""
    return TIER.get(text.ALIASES.get(key, key))
