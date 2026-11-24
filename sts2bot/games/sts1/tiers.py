"""How good each of the first game's cards is, by tiers: the second opinion that crowd.py gives Slay the Spire 2's cards.

There is no run-based table for the first game to read any more (Spirelogs, the site that had one, is gone), and the tier
lists on the web found on 2026-11-24 are partial or odd (one put Barrage in its top tier). So these tiers are written from
the community's long-standing consensus on the cards (A0-A20, all four characters): an opinion, not runs, like the Fool's
table in crowd.py, and to be replaced or corrected by the bot's own record (learn.CardStats) as STS1 runs pile up.

Why it is needed: the bot's own rating reads only what a card does on paper (cards.deck_value), and on paper Poisoned
Stab (6.8) beat Acrobatics (3.9), Piercing Wail (2.2) and Noxious Fumes (3.0) at the first STS1 rewards. The tiers sit on
crowd.py's scale (middle 5.0) and mix in by the same `crowd` share.
"""

from __future__ import annotations

import re

from . import text

S, A, B, C, D = 7.5, 6.5, 5.5, 4.5, 3.5

_TIERS = {
    # The Ironclad.
    S: "Offering, Shockwave, Feed, Immolate, Impervious, Demon Form",
    A: """Shrug It Off, Battle Trance, Inflame, Flame Barrier, Uppercut, Disarm, Whirlwind, Pommel Strike, Carnage, Corruption,
        Feel No Pain, Seeing Red, Bloodletting, Power Through, Spot Weakness, Reaper, Fiend Fire,
        Adrenaline, Wraith Form, Footwork, After Image, Malaise, Noxious Fumes, Well-Laid Plans, Piercing Wail, Acrobatics,
        Leg Sweep, Crippling Cloud, Tools of the Trade, Backflip, Blur, Catalyst,
        Defragment, Echo Form, Biased Cognition, Glacier, Coolheaded, Buffer, Electrodynamics, Machine Learning, Seek, Loop,
        Skim, Cold Snap,
        Rushdown, Talk to the Hand, Mental Fortress, Tantrum, Inner Peace, Cut Through Fate, Empty Mind, Scrawl, Ragnarok,
        Conclude, Wallop, Fear No Evil,
        Apotheosis, Master of Strategy, Apparition""",
    B: """Cleave, Thunderclap, Headbutt, True Grit, Armaments, Ghostly Armor, Burning Pact, Second Wind, Dark Embrace, Bludgeon,
        Hemokinesis, Sentinel, Metallicize, Exhume, Rage, Pummel, Intimidate, Dropkick, Combust,
        Burst, Dash, Bouncing Flask, Calculated Gamble, Die Die Die, Glass Knife, Predator, Terror, Skewer, Heel Hook,
        Corpse Explosion, Storm of Steel, A Thousand Cuts, Blade Dance, Cloak and Dagger, Dagger Throw, Deadly Poison,
        Prepared, All-Out Attack, Escape Plan, Dodge and Roll,
        Ball Lightning, Compile Driver, Hologram, Sweeping Beam, Chill, Equilibrium, Force Field, Reinforced Body,
        Auto-Shields, Capacitor, Consume, Doom and Gloom, Fusion, Overclock, Scrape, Tempest, Double Energy, Core Surge,
        Hyperbeam, Multi-Cast, Meteor Strike, Fission, All for One, Thunder Strike, Charge Battery, Stack,
        Bowling Bash, Crescendo, Empty Body, Flying Sleeves, Follow-Up, Protect, Third Eye, Tranquility, Deceive Reality,
        Fasting, Foresight, Meditate, Simmering Fury, Wave of the Hand, Wheel Kick, Brilliance, Devotion, Lesson Learned,
        Omniscience, Vault, Sanctity, Battle Hymn, Wreath of Flame,
        Bite, Discovery, Dramatic Entrance, Hand of Greed, Madness, Panic Button, Secret Technique, Secret Weapon, The Bomb,
        Trip""",
    C: """Anger, Twin Strike, Iron Wave, Heavy Blade, Perfected Strike, Clothesline, Sword Boomerang, Body Slam, Infernal Blade,
        Limit Break, Barricade, Juggernaut, Double Tap, Brutality, Berserk, Entrench, Dual Wield, Rampage, Blood for Blood,
        Searing Blow, Wild Strike,
        Expertise, Eviscerate, Accuracy, Backstab, Bane, Choke, Concentrate, Dagger Spray, Deflect, Flying Knee, Outmaneuver,
        Poisoned Stab, Quick Slash, Sneaky Strike, Sucker Punch, Slice, Finisher, Flechettes, Riddle with Holes, Setup,
        Reflex, Tactician, Unload, Infinite Blades, Endless Agony, Distraction, Phantasmal Killer, Nightmare, Envenom,
        Bullet Time, Doppelganger, Alchemize,
        Barrage, Beam Cell, Claw, Go for the Eyes, Leap, Rebound, Recursion, Steam Barrier, Streamline, Turbo, Aggregate,
        Blizzard, Boot Sequence, Bullseye, Chaos, Darkness, FTL, Genetic Algorithm, Heatsinks, Hello World, Melter, Recycle,
        Reprogram, Rip and Tear, Self Repair, Static Discharge, Storm, Sunder, White Noise, Amplify, Creative AI, Rainbow,
        Reboot,
        Consecrate, Crush Joints, Empty Fist, Evaluate, Flurry of Blows, Halt, Just Lucky, Pressure Points, Prostrate,
        Sash Whip, Carve Reality, Collect, Foreign Influence, Indignation, Like Water, Nirvana, Perseverance, Pray,
        Reach Heaven, Sands of Time, Signature Move, Study, Swivel, Weave, Windmill Strike, Worship, Alpha, Blasphemy,
        Conjure Blade, Deus Ex Machina, Deva Form, Establishment, Judgment, Master Reality, Spirit Shield, Wish,
        Bandage Up, Blind, Chrysalis, Dark Shackles, Deep Breath, Enlightenment, Finesse, Flash of Steel, Forethought,
        Good Instincts, Impatience, Jack of All Trades, Magnetism, Mayhem, Metamorphosis, Mind Blast, Panacea, Panache,
        Purity, Swift Strike, Thinking Ahead, Transmutation, Violence, Ritual Dagger""",
    D: """Flex, Havoc, Warcry, Reckless Charge, Clash, Sever Soul, Rupture, Fire Breathing, Evolve,
        Grand Finale, Caltrops, Masterful Stab, Sadistic Nature""",
}


def _key(name: str) -> str:
    return re.sub(r"[^A-Z0-9]+", "_", name.upper().replace("'", "")).strip("_")


TIER = {_key(n): value for value, names in _TIERS.items() for n in re.split(r",\s*", " ".join(names.split()))}


def rating(key: str) -> float | None:
    """A card's tier on crowd.py's scale, by the game's id in key form ("UNDERHANDED_STRIKE" is Sneaky Strike); None for
    cards with none (the starter cards, curses, cards made in a fight)."""
    return TIER.get(text.ALIASES.get(key, key))
