//! How good each of the first game's cards is, by tiers: the community's consensus (no run-based table exists), the second
//! opinion crowd.rs gives Slay the Spire 2's cards (on paper alone, Poisoned Stab beat Acrobatics). On crowd.rs's scale
//! (middle 5.0), mixed in by the same `crowd` share, and to be corrected by the bot's own record (learn::CardStats).

use std::sync::OnceLock;

use indexmap::IndexMap;

use super::text;

pub const S: f64 = 7.5;
pub const A: f64 = 6.5;
pub const B: f64 = 5.5;
pub const C: f64 = 4.5;
pub const D: f64 = 3.5;

// Each tier lists the Ironclad's cards, then the Silent's, the Defect's, the Watcher's and the colorless ones.
#[rustfmt::skip]
pub const _TIERS: &[(f64, &str)] = &[
    (S, "Offering, Shockwave, Feed, Immolate, Impervious, Demon Form"),
    (A, "Shrug It Off, Battle Trance, Inflame, Flame Barrier, Uppercut, Disarm, Whirlwind, Pommel Strike, Carnage, Corruption, Feel No Pain, Seeing Red, Bloodletting, Power Through, Spot Weakness, Reaper, Fiend Fire,
        Adrenaline, Wraith Form, Footwork, After Image, Malaise, Noxious Fumes, Well-Laid Plans, Piercing Wail, Acrobatics, Leg Sweep, Crippling Cloud, Tools of the Trade, Backflip, Blur, Catalyst,
        Defragment, Echo Form, Biased Cognition, Glacier, Coolheaded, Buffer, Electrodynamics, Machine Learning, Seek, Loop, Skim, Cold Snap,
        Rushdown, Talk to the Hand, Mental Fortress, Tantrum, Inner Peace, Cut Through Fate, Empty Mind, Scrawl, Ragnarok, Conclude, Wallop, Fear No Evil,
        Apotheosis, Master of Strategy, Apparition"),
    (B, "Cleave, Thunderclap, Headbutt, True Grit, Armaments, Ghostly Armor, Burning Pact, Second Wind, Dark Embrace, Bludgeon, Hemokinesis, Sentinel, Metallicize, Exhume, Rage, Pummel, Intimidate, Dropkick, Combust,
        Burst, Dash, Bouncing Flask, Calculated Gamble, Die Die Die, Glass Knife, Predator, Terror, Skewer, Heel Hook, Corpse Explosion, Storm of Steel, A Thousand Cuts, Blade Dance, Cloak and Dagger, Dagger Throw, Deadly Poison, Prepared,
        All-Out Attack, Escape Plan, Dodge and Roll,
        Ball Lightning, Compile Driver, Hologram, Sweeping Beam, Chill, Equilibrium, Force Field, Reinforced Body, Auto-Shields, Capacitor, Consume, Doom and Gloom, Fusion, Overclock, Scrape, Tempest, Double Energy, Core Surge, Hyperbeam, Multi-Cast,
        Meteor Strike, Fission, All for One, Thunder Strike, Charge Battery, Stack,
        Bowling Bash, Crescendo, Empty Body, Flying Sleeves, Follow-Up, Protect, Third Eye, Tranquility, Deceive Reality, Fasting, Foresight, Meditate, Simmering Fury, Wave of the Hand, Wheel Kick, Brilliance, Devotion, Lesson Learned, Omniscience,
        Vault, Sanctity, Battle Hymn, Wreath of Flame,
        Bite, Discovery, Dramatic Entrance, Hand of Greed, Madness, Panic Button, Secret Technique, Secret Weapon, The Bomb, Trip"),
    (C, "Anger, Twin Strike, Iron Wave, Heavy Blade, Perfected Strike, Clothesline, Sword Boomerang, Body Slam, Infernal Blade, Limit Break, Barricade, Juggernaut, Double Tap, Brutality, Berserk, Entrench, Dual Wield, Rampage, Blood for Blood,
        Searing Blow, Wild Strike,
        Expertise, Eviscerate, Accuracy, Backstab, Bane, Choke, Concentrate, Dagger Spray, Deflect, Flying Knee, Outmaneuver, Poisoned Stab, Quick Slash, Sneaky Strike, Sucker Punch, Slice, Finisher, Flechettes, Riddle with Holes, Setup, Reflex,
        Tactician, Unload, Infinite Blades, Endless Agony, Distraction, Phantasmal Killer, Nightmare, Envenom, Bullet Time, Doppelganger, Alchemize,
        Barrage, Beam Cell, Claw, Go for the Eyes, Leap, Rebound, Recursion, Steam Barrier, Streamline, Turbo, Aggregate, Blizzard, Boot Sequence, Bullseye, Chaos, Darkness, FTL, Genetic Algorithm, Heatsinks, Hello World, Melter, Recycle, Reprogram,
        Rip and Tear, Self Repair, Static Discharge, Storm, Sunder, White Noise, Amplify, Creative AI, Rainbow, Reboot,
        Consecrate, Crush Joints, Empty Fist, Evaluate, Flurry of Blows, Halt, Just Lucky, Pressure Points, Prostrate, Sash Whip, Carve Reality, Collect, Foreign Influence, Indignation, Like Water, Nirvana, Perseverance, Pray, Reach Heaven,
        Sands of Time, Signature Move, Study, Swivel, Weave, Windmill Strike, Worship, Alpha, Blasphemy, Conjure Blade, Deus Ex Machina, Deva Form, Establishment, Judgment, Master Reality, Spirit Shield, Wish,
        Bandage Up, Blind, Chrysalis, Dark Shackles, Deep Breath, Enlightenment, Finesse, Flash of Steel, Forethought, Good Instincts, Impatience, Jack of All Trades, Magnetism, Mayhem, Metamorphosis, Mind Blast, Panacea, Panache, Purity,
        Swift Strike, Thinking Ahead, Transmutation, Violence, Ritual Dagger"),
    (D, "Flex, Havoc, Warcry, Reckless Charge, Clash, Sever Soul, Rupture, Fire Breathing, Evolve, Grand Finale, Caltrops, Masterful Stab, Sadistic Nature"),
];

pub fn _key(name: &str) -> String {
    crate::re!(r"[^A-Z0-9]+").replace_all(&name.to_uppercase().replace('\'', ""), "_").trim_matches('_').to_string()
}

/// Every rated card's key and its tier's value, in the order of `_TIERS`.
#[allow(non_snake_case)]
pub fn TIER() -> &'static IndexMap<String, f64> {
    static TABLE: OnceLock<IndexMap<String, f64>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut out = IndexMap::new();
        for (value, names) in _TIERS {
            let names = names.split_whitespace().collect::<Vec<_>>().join(" ");
            for n in crate::re!(r",\s*").split(&names) {
                out.insert(_key(n), *value);
            }
        }
        out
    })
}

/// A card's tier on crowd.rs's scale, by the game's id in key form; None for the starter cards, curses and the like.
pub fn rating(key: &str) -> Option<f64> {
    TIER().get(text::ALIASES().get(key).copied().unwrap_or(key)).copied()
}

#[cfg(test)]
mod tests {
    //! Replays tests/golden/sts1_text.json (recorded from tiers.rs).
    use super::*;
    use serde_json::{json, Value};

    fn golden() -> Value {
        serde_json::from_str(include_str!("../../../tests/golden/sts1_text.json")).unwrap()
    }

    #[test]
    fn the_tiers_are_the_recorded() {
        let g = golden();
        assert_eq!(json!([S, A, B, C, D]), json!(g["keys_consts"].as_array().unwrap()[4..9]));
        assert_eq!(json!(_TIERS.iter().map(|(v, s)| json!([v, s])).collect::<Vec<_>>()), g["_TIERS"]);
        assert_eq!(json!(TIER().iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>()), g["TIER"]);
        assert!(TIER().keys().all(|k| text::CARDS().contains_key(k.as_str()))); // (every name is a card the bot can read)
        for c in g["_key"].as_array().unwrap() {
            assert_eq!(_key(c[0].as_str().unwrap()), c[1].as_str().unwrap(), "{}", c[0]);
        }
    }

    #[test]
    fn every_card_rates_as_recorded() {
        let g = golden();
        let cases = g["rating"].as_array().unwrap();
        assert!(cases.len() > text::CARDS().len());
        for c in cases {
            assert_eq!(rating(c[0].as_str().unwrap()), c[1].as_f64(), "{}", c[0]);
        }
        assert_eq!(rating("UNDERHANDED_STRIKE"), Some(C)); // (by the game's id: Sneaky Strike)
        assert_eq!(rating("STRIKE_R"), None);
    }
}

#[cfg(test)]
mod first_game_ratings {
    //! What the cards and crowd ports could not check before this module was there: `cards::deck_value` and
    //! `run::crowd::rating` while playing the first game, over every first-game card of tests/golden/cards.json
    //! (tests/golden/cards_sts1.json, written by the first version).
    use serde_json::Value;

    use crate::run::crowd;
    use crate::{cards, games, learn};

    #[test]
    fn every_first_game_card_is_worth_what_the_record_says() {
        let g: Value = serde_json::from_str(include_str!("../../../tests/golden/cards_sts1.json")).unwrap();
        let all: Value = serde_json::from_str(include_str!("../../../tests/golden/cards.json")).unwrap();
        let cases = all["cases"].as_array().unwrap();
        let _game = games::playing("sts1");
        let rows = g["deck_value"].as_array().unwrap();
        assert_eq!(rows.len(), cases.iter().filter(|c| c["g"] == "sts1").count());
        assert!(rows.len() > 700);
        for row in rows {
            let case = &cases[row[0].as_u64().unwrap() as usize];
            assert_eq!(case["g"], "sts1");
            let card = &case["card"];
            for (share, want) in [0.0, 0.3, 0.6].iter().zip(row[1].as_array().unwrap()) {
                let _knob = learn::with_knobs(&[("crowd", *share)]);
                assert_eq!(cards::deck_value(card), want.as_f64().unwrap(), "deck_value at crowd {share}: {card}");
            }
            assert_eq!(crowd::rating(&cards::_card_key(card)), row[2].as_f64(), "crowd::rating of {card}");
        }
        let keys = g["rating"].as_array().unwrap();
        assert!(keys.len() > 400);
        for row in keys {
            assert_eq!(crowd::rating(row[0].as_str().unwrap()), row[1].as_f64(), "crowd::rating({})", row[0]);
            assert_eq!(crowd::rating(row[0].as_str().unwrap()), super::rating(row[0].as_str().unwrap()));
        }
    }
}
