//! What a relic on a Merchant's shelf is worth to this bot next to an ordinary one (0; +1 is a good card more, -3 a relic that
//! does nothing for the bot as it plays), added to the shop's price rule, which alone bought the cheapest relic whatever it did.
//! Set by what each text does, checked against what the relics held did to the cost of fights; energy, worth most at bosses,
//! follows events::_ANCIENT less each relic's drawback. Deck-bound relics are rated by the deck (`_DECK`).

use std::collections::HashSet;
use std::sync::LazyLock;

use indexmap::IndexMap;
use serde_json::Value;

use crate::cards::parse as _parse;
use crate::base::{self, JsonExt};

/// relic id -> worth next to an ordinary shop relic (0). Anything not listed is taken as ordinary.
pub static WORTH: LazyLock<IndexMap<&'static str, f64>> = LazyLock::new(|| {
    #[rustfmt::skip]
    let rows: &[(&str, f64)] = &[
        // --- HP into the boss, and HP a boss cannot take
        ("PANTOGRAPH", 1.5), ("LIZARD_TAIL", 1.5), ("BEATING_REMNANT", 1.2), ("TUNGSTEN_ROD", 0.8), ("MANGO", 0.5), ("PEAR", 0.2), ("STRAWBERRY", 0.0), ("MEAT_ON_THE_BONE", 0.0), ("BLOOD_VIAL", -0.2),
        // --- energy
        ("CANDELABRA", 1.0), ("CHANDELIER", 1.0), ("HAPPY_FLOWER", 0.8), ("BREAD", 0.6), ("LANTERN", 0.4), ("ICE_CREAM", 0.2), ("ART_OF_WAR", 0.1), ("VENERABLE_TEA_SET", -0.3), ("NUNCHAKU", -0.3), ("IVORY_TILE", -1.0),
        // energy every turn (the Ancient table's rating, less each one's drawback)
        ("PAELS_FLESH", 2.0), ("PRISMATIC_GEM", 2.0), ("WHISPERING_EARRING", 2.0), ("SPIKED_GAUNTLETS", 1.0), ("PUMPKIN_CANDLE", 1.0), ("BLESSED_ANTLER", 0.5), ("PAELS_TEARS", 0.5), ("SEAL_OF_GOLD", 0.5), ("VERY_HOT_COCOA", 1.5), ("BLOOD_SOAKED_ROSE", 0.0),
        // --- Block every fight
        ("ORICHALCUM", 0.8), ("CLOAK_CLASP", 0.6), ("GORGET", 0.6), ("ANCHOR", 0.5), ("CAPTAINS_WHEEL", 0.5), ("STURDY_CLAMP", 0.5), ("HORN_CLEAT", 0.4), ("ODDLY_SMOOTH_STONE", 0.5), ("VAMBRACE", 0.2), ("SPARKLING_ROUGE", 0.2), ("RAINBOW_RING", 0.2),
        ("BRONZE_SCALES", 0.3), ("RIPPLE_BASIN", -0.1), ("SELF_FORMING_CLAY", 0.0), ("THE_ABACUS", -0.2), ("PARRYING_SHIELD", -0.2), ("INTIMIDATING_HELMET", -0.2), ("ORNAMENTAL_FAN", -0.1), ("KUNAI", 0.0), ("TUNING_FORK", -0.4), ("PERMAFROST", -0.4),
        // --- damage and debuffs every fight
        ("MERCURY_HOURGLASS", 0.6), ("SHURIKEN", 0.6), ("STONE_CALENDAR", 0.5), ("NINJA_SCROLL", 0.5), ("RED_MASK", 0.5), ("BAG_OF_MARBLES", 0.5), ("FESTIVE_POPPER", 0.4), ("VAJRA", 0.4), ("TWISTED_FUNNEL", 0.4), ("BRIMSTONE", 0.3), ("AKABEKO", 0.3),
        ("KUSARIGAMA", 0.2), ("GREMLIN_HORN", 0.2), ("LETTER_OPENER", -0.1), ("PEN_NIB", -0.1), ("UNSETTLING_LAMP", 0.0), ("PAPER_KRANE", -0.1), ("PAPER_PHROG", -0.2), ("SLING_OF_COURAGE", -0.2), ("RED_SKULL", -0.2), ("MINIATURE_CANNON", -0.3),
        ("CHARONS_ASHES", -0.3), ("REPTILE_TRINKET", -0.4), ("SCREAMING_FLAGON", -0.6),
        // --- cards: drawn, made, upgraded
        ("BAG_OF_PREPARATION", 0.3), ("VEXING_PUZZLEBOX", 0.2), ("RAZOR_TOOTH", 0.2), ("STONE_CRACKER", 0.0), ("BELLOWS", 0.0), ("ORANGE_DOUGH", 0.0), ("WHETSTONE", 0.0), ("WAR_PAINT", -0.2), ("PENDULUM", -0.2), ("CENTENNIAL_PUZZLE", -0.2), ("POCKETWATCH", -0.1),
        ("DOLLYS_MIRROR", -0.1), ("BIG_HAT", -0.2), ("MUMMIFIED_HAND", -0.2), ("TOOLBOX", -0.2), ("GNARLED_HAMMER", -0.2), ("GAME_PIECE", -0.3), ("UNCEASING_TOP", -0.3), ("GAMBLING_CHIP", -0.3), ("POWER_CELL", -0.4), ("FROZEN_EGG", -0.3), ("MOLTEN_EGG", -0.2),
        ("TOXIC_EGG", -0.2), ("KIFUDA", -0.5), ("PUNCH_DAGGER", -0.6), ("ROYAL_STAMP", -0.6), ("ORRERY", -0.6), ("RINGING_TRIANGLE", -0.6), ("BURNING_STICKS", -0.8), ("JOSS_PAPER", -0.8), ("GHOST_SEED", -0.8), ("LASTING_CANDY", -0.8), ("WING_CHARM", -0.8),
        ("LAVA_LAMP", -0.8), ("DINGY_RUG", -1.0), ("BOOKMARK", -1.2), ("WHITE_STAR", -0.5), ("PRAYER_WHEEL", -0.5),
        // --- potions
        ("WHITE_BEAST_STATUE", 0.4), ("POTION_BELT", 0.4), ("TINY_MAILBOX", 0.3), ("CAULDRON", 0.0), ("PETRIFIED_TOAD", 0.0),
        // --- healing and gold outside fights
        ("REGAL_PILLOW", 0.0), ("MEAL_TICKET", 0.0), ("BOOK_OF_FIVE_RINGS", 0.0), ("ETERNAL_FEATHER", -0.1), ("PLANISPHERE", -0.2), ("DRAGON_FRUIT", -0.1), ("OLD_COIN", -0.2), ("MEMBERSHIP_CARD", -0.3), ("THE_COURIER", -0.5), ("JUZU_BRACELET", -0.5),
        ("AMETHYST_AUBERGINE", -0.6), ("BOWLER_HAT", -0.8), ("LUCKY_FYSH", -0.8),
        // --- what the bot never uses as it plays
        ("GIRYA", 0.5), ("SHOVEL", 0.5), ("MINIATURE_TENT", -3.0), ("MYSTIC_LIGHTER", -2.5), ("BELT_BUCKLE", -2.0),
        // --- the characters' own (offered only to them)
        ("DATA_DISK", 0.4), ("FUNERARY_MASK", 0.0), ("FENCING_MANUAL", 0.0), ("GOLD_PLATED_CABLES", 0.0), ("SYMBIOTIC_VIRUS", 0.0), ("RUNIC_CAPACITOR", 0.0), ("LUNAR_PASTRY", 0.0), ("MINI_REGENT", 0.0), ("BONE_FLUTE", -0.2), ("UNDYING_SIGIL", -0.2),
        ("REGALITE", -0.2), ("GALACTIC_DUST", -0.3), ("EMOTION_CHIP", -0.3), ("BOOK_REPAIR_KNIFE", -0.5), ("METRONOME", -0.5), ("VITRUVIAN_MINION", -0.5),
    ];
    rows.iter().copied().collect()
});
/// Worth that comes over the rest of the run outside fights, or by changing rewards still to come: less of it is left in a later act.
pub static LONG_RUN: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    HashSet::from([
        "MEMBERSHIP_CARD", "THE_COURIER", "AMETHYST_AUBERGINE", "BOWLER_HAT", "LUCKY_FYSH", "DRAGON_FRUIT", "OLD_COIN", "JUZU_BRACELET", "MEAL_TICKET", "REGAL_PILLOW", "ETERNAL_FEATHER", "TINY_MAILBOX", "PLANISPHERE", "BOOK_OF_FIVE_RINGS",
        "FROZEN_EGG", "MOLTEN_EGG", "TOXIC_EGG", "DINGY_RUG", "LASTING_CANDY", "WING_CHARM", "PRAYER_WHEEL", "WHITE_STAR", "VENERABLE_TEA_SET", "LAVA_LAMP", "WHITE_BEAST_STATUE", "PUMPKIN_CANDLE",
    ])
});
pub const LATER: [(i64, f64); 3] = [(1, 0.0), (2, 0.3), (3, 0.8)]; // act -> taken off those

/// (the original skips a card its test cannot read, one that is no dict: such a card passes no test here either.)
pub fn _count(deck: &[Value], test: fn(&Value) -> bool) -> i64 {
    let mut n = 0;
    for c in deck {
        n += test(c) as i64;
    }
    n
}

fn _discards(c: &Value) -> bool {
    crate::re!(r"(?i)\bdiscard").is_match(&c["description"].text())
}

/// Relics that only work with certain cards: (worth with none of them, worth each such card adds, most it can reach, the test of such a card).
pub static _DECK: LazyLock<IndexMap<&'static str, (f64, f64, f64, fn(&Value) -> bool)>> = LazyLock::new(|| {
    let rows: [(&str, (f64, f64, f64, fn(&Value) -> bool)); 6] = [
        ("CHEMICAL_X", (-3.0, 3.0, 0.8, |c| c.is_object() && _parse(c).is_x())),
        ("SNECKO_SKULL", (-1.5, 0.5, 0.5, |c| c.is_object() && _parse(c).poison > 0)),
        ("HELICAL_DART", (-1.5, 0.5, 0.5, |c| c.is_object() && (_parse(c).shivs > 0 || c["name"].text().starts_with("Shiv")))),
        ("STRIKE_DUMMY", (-0.8, 0.25, 0.6, |c| c["name"].text().to_lowercase().contains("strike"))),
        ("TINGSHA", (-1.0, 0.3, 0.3, _discards)),
        ("TOUGH_BANDAGES", (-1.0, 0.3, 0.3, _discards)),
    ];
    rows.into_iter().collect()
});

/// A relic's id from its name as the game prints it ("Lee's Waffle" -> LEES_WAFFLE; true of all but The Chosen Cheese).
pub fn id_of(name: &str) -> String {
    let bare = crate::re!(r"['’]").replace_all(name, "").to_uppercase();
    crate::re!(r"[^A-Z0-9]+").replace_all(&bare, "_").trim_matches('_').to_string()
}

/// Energy-every-turn relics against cards-every-turn ones: on the Knowledge Demon model energy is worth most to a deck on 3 a turn, cards on 4+.
/// relic -> (over its table rating for a deck on 3 energy, for one on 4 or more)
pub static TURN_LEAN: LazyLock<IndexMap<&'static str, (f64, f64)>> = LazyLock::new(|| {
    #[rustfmt::skip]
    let rows: &[(&str, (f64, f64))] = &[
        ("VELVET_CHOKER", (2.5, -1.5)), ("PRISMATIC_GEM", (2.5, -1.5)), ("SOZU", (2.5, -1.5)), ("ECTOPLASM", (2.5, -1.5)), ("SPIKED_GAUNTLETS", (2.5, -1.5)), ("WHISPERING_EARRING", (2.5, -1.5)), ("PAELS_FLESH", (2.0, -1.5)), ("SEAL_OF_GOLD", (2.0, -1.5)),
        ("PHILOSOPHERS_STONE", (2.0, -3.0)), ("BLESSED_ANTLER", (2.0, -3.0)), ("BLOOD_SOAKED_ROSE", (2.0, -3.0)), ("PUMPKIN_CANDLE", (1.25, -0.75)), ("PAELS_BLOOD", (-1.5, 1.0)), ("SNECKO_EYE", (0.0, 1.0)), ("FIDDLE", (0.0, 1.0)), ("IRON_CLUB", (-0.75, 0.5)),
    ];
    rows.iter().copied().collect()
});
/// The share of an energy a turn each gives once held (Pael's Flesh from the third turn, Seal of Gold while the gold lasts).
pub static TURN_ENERGY: LazyLock<IndexMap<&'static str, f64>> = LazyLock::new(|| {
    let mut out: IndexMap<&'static str, f64> = TURN_LEAN.iter().filter(|(_, (on3, _on4))| *on3 > 0.0).map(|(rid, _)| (*rid, 1.0)).collect();
    for (rid, share) in [("PAELS_FLESH", 0.85), ("SEAL_OF_GOLD", 0.8), ("PUMPKIN_CANDLE", 0.5)] {
        out.insert(rid, share);
    }
    out
});

/// Energy a turn beyond the 3 every character starts with: as the last fight began (`energy`), or from relics held
/// whose energy comes later in a fight (Pael's Flesh), and half of each energy-a-turn Power in the deck.
/// (`held`, `deck`: empty for the original's None.)
pub fn spare_energy(energy: i64, held: &[Value], deck: &[Value]) -> f64 {
    use super::decks::turn_energy; // (decks reads cards and crowd only, but keep the import where it is needed)
    let relics_held = base::fsum(held.iter().filter(|r| r.is_object()).map(|r| TURN_ENERGY.get(r["id"].text().as_str()).copied().unwrap_or(0.0)));
    let powers = 0.5 * base::fsum(deck.iter().filter(|c| c.is_object()).map(turn_energy));
    let spare = energy as f64 - 3.0;
    (if relics_held > spare { relics_held } else { spare }) + powers
}

/// What an energy-a-turn or cards-a-turn relic is worth over its table rating for this deck (0 when the energy is not known).
pub fn energy_lean(relic_id: &str, energy: Option<i64>, held: &[Value], deck: &[Value]) -> f64 {
    let (Some(energy), Some(lean)) = (energy, TURN_LEAN.get(relic_id)) else { return 0.0 };
    if spare_energy(energy, held, deck) < 0.5 {
        lean.0
    } else {
        lean.1
    }
}

/// What this relic is worth next to an ordinary shop relic, for this deck, act and HP.
/// (`deck`: empty for the original's None; `act`: None for anything that is not an int; `hp`, `max_hp`: 0 when left out.)
pub fn worth(relic_id: &str, deck: &[Value], act: Option<i64>, hp: i64, max_hp: i64) -> f64 {
    let rid = relic_id;
    let mut w = if let Some((base, each, top, test)) = _DECK.get(rid) {
        let reached = base + each * _count(deck, *test) as f64;
        if reached < *top {
            reached
        } else {
            *top
        }
    } else if rid == "LEES_WAFFLE" {
        // "Raise your Max HP by 7 and heal all of your HP": as much as the HP it gives back.
        if max_hp > 0 {
            -0.4 + 2.0 * (1.0 - hp as f64 / max_hp as f64)
        } else {
            0.0
        }
    } else {
        WORTH.get(rid).copied().unwrap_or(0.0)
    };
    let a = act.unwrap_or(1);
    if LONG_RUN.contains(rid) {
        w -= LATER.iter().find(|(k, _)| *k == a).map(|(_, v)| *v).unwrap_or(LATER[2].1);
    } else if rid == "PANTOGRAPH" && a >= 3 {
        w -= 0.3; // one boss left to heal for, but the one that ends most runs
    }
    w
}

#[cfg(test)]
mod tests {
    //! Replays tests/golden/relicworth.json: calls recorded from the original tests and model runs, every relic of the tables
    //! in each act, and the tables themselves (recorded from relicworth.rs).
    use super::*;
    use crate::run::decks::tests::{resolve, same};
    use crate::games;
    use serde_json::json;

    #[test]
    fn golden() {
        let g: Value = serde_json::from_str(include_str!("../../tests/golden/relicworth.json")).unwrap();
        let all: Value = serde_json::from_str(include_str!("../../tests/golden/cards.json")).unwrap();
        let cards = all["cases"].as_array().unwrap();
        let mut checked = 0;
        for (name, rows) in g["cases"].as_object().unwrap() {
            for (n, row) in rows.as_array().unwrap().iter().enumerate() {
                let a = resolve(&row[0], &g, cards);
                let _game = games::playing(row[2].as_str().unwrap());
                let got = match name.as_str() {
                    "id_of" => json!(id_of(a[0].as_str().unwrap())),
                    "spare_energy" => json!(spare_energy(a[0].as_i64().unwrap(), a[1].list(), a[2].list())),
                    "energy_lean" => json!(energy_lean(a[0].as_str().unwrap(), a[1].as_i64(), a[2].list(), a[3].list())),
                    "worth" => json!(worth(a[0].as_str().unwrap(), a[1].list(), a[2].as_i64(), a[3].as_i64().unwrap(), a[4].as_i64().unwrap())),
                    other => panic!("no replay for {other}"),
                };
                assert!(same(&got, &row[3]), "{name} case {n}: {} gives {got}, the original {}", base::json_dumps(&a), row[3]);
                checked += 1;
            }
        }
        let t = &g["tables"];
        assert_eq!(json!(WORTH.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>()), t["WORTH"]);
        let mut long_run: Vec<&str> = LONG_RUN.iter().copied().collect();
        long_run.sort();
        assert_eq!(json!(long_run), t["LONG_RUN"]);
        assert_eq!(json!(LATER.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>()), t["LATER"]);
        assert_eq!(json!(_DECK.iter().map(|(k, v)| json!([k, [v.0, v.1, v.2]])).collect::<Vec<_>>()), t["_DECK"]);
        assert_eq!(json!(TURN_LEAN.iter().map(|(k, v)| json!([k, [v.0, v.1]])).collect::<Vec<_>>()), t["TURN_LEAN"]);
        assert_eq!(json!(TURN_ENERGY.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>()), t["TURN_ENERGY"]);
        assert!(checked > 2000, "{checked} cases");
    }
}
