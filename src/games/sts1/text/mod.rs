//! What the first game's cards and potions say: CommunicationMod sends no text, and the planner reads cards by it (cards::parse).
//! Keyed like cards::key_of spells a name; looked up by id, then name. "Deal 6(9) damage." is 6, or 9 once upgraded; "a||b" is text a, or b upgraded.

pub mod colorless;
pub mod defect;
pub mod ironclad;
pub mod potions;
pub mod silent;
pub mod watcher;

use std::sync::OnceLock;

use indexmap::IndexMap;
use serde_json::Value;

use crate::cards::key_of;
use crate::base::JsonExt;

/// An id or a name as the game sends it, read the way `key_of` reads it in the original: `str(x or "")`.
/// (A `&str`, a `String`, a JSON value, or an `Option` of one; "" and `Value::Null` stand for None.)
pub trait Raw {
    fn raw(&self) -> String;
}

impl Raw for str {
    fn raw(&self) -> String {
        self.to_string()
    }
}

impl Raw for String {
    fn raw(&self) -> String {
        self.clone()
    }
}

impl Raw for Value {
    fn raw(&self) -> String {
        self.text()
    }
}

impl<T: Raw + ?Sized> Raw for &T {
    fn raw(&self) -> String {
        (**self).raw()
    }
}

impl<T: Raw> Raw for Option<T> {
    fn raw(&self) -> String {
        self.as_ref().map(|x| x.raw()).unwrap_or_default()
    }
}

/// Every card of the first game: the Ironclad's, the Silent's, the Defect's, the Watcher's, then the colorless ones.
#[allow(non_snake_case)]
pub fn CARDS() -> &'static IndexMap<&'static str, &'static str> {
    static TABLE: OnceLock<IndexMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut out = IndexMap::new();
        for table in [ironclad::CARDS(), silent::CARDS(), defect::CARDS(), watcher::CARDS(), colorless::CARDS()] {
            for (key, text) in table {
                out.insert(*key, *text);
            }
        }
        out
    })
}

// Ids that are not the card's name in key form.
#[rustfmt::skip]
pub static ALIASES_ROWS: &[(&str, &str)] = &[
    ("UNDERHANDED_STRIKE", "SNEAKY_STRIKE"), ("CRIPPLING_POISON", "CRIPPLING_CLOUD"), ("VENOMOLOGY", "ALCHEMIZE"), ("WRAITH_FORM_V2", "WRAITH_FORM"), ("CONSERVE_BATTERY", "CHARGE_BATTERY"), ("GASH", "CLAW"), ("STEAM", "STEAM_BARRIER"), ("REDO", "RECURSION"),
    ("BOOTSEQUENCE", "BOOT_SEQUENCE"), ("UNDO", "EQUILIBRIUM"), ("LOCKON", "BULLSEYE"), ("STEAM_POWER", "OVERCLOCK"), ("GHOSTLY", "APPARITION"), ("BOWLINGBASH", "BOWLING_BASH"), ("CRUSHJOINTS", "CRUSH_JOINTS"), ("CUTTHROUGHFATE", "CUT_THROUGH_FATE"),
    ("EMPTYBODY", "EMPTY_BODY"), ("EMPTYFIST", "EMPTY_FIST"), ("EMPTYMIND", "EMPTY_MIND"), ("FLURRYOFBLOWS", "FLURRY_OF_BLOWS"), ("FLYINGSLEEVES", "FLYING_SLEEVES"), ("FOLLOWUP", "FOLLOW_UP"), ("JUSTLUCKY", "JUST_LUCKY"), ("PATHTOVICTORY", "PRESSURE_POINTS"),
    ("SASHWHIP", "SASH_WHIP"), ("CLEARTHEMIND", "TRANQUILITY"), ("THIRDEYE", "THIRD_EYE"), ("BATTLEHYMN", "BATTLE_HYMN"), ("CARVEREALITY", "CARVE_REALITY"), ("DECEIVEREALITY", "DECEIVE_REALITY"), ("FASTING2", "FASTING"), ("FEARNOEVIL", "FEAR_NO_EVIL"),
    ("FOREIGNINFLUENCE", "FOREIGN_INFLUENCE"), ("WIREHEADING", "FORESIGHT"), ("INNERPEACE", "INNER_PEACE"), ("LIKEWATER", "LIKE_WATER"), ("MENTALFORTRESS", "MENTAL_FORTRESS"), ("REACHHEAVEN", "REACH_HEAVEN"), ("ADAPTATION", "RUSHDOWN"),
    ("SANDSOFTIME", "SANDS_OF_TIME"), ("SIGNATUREMOVE", "SIGNATURE_MOVE"), ("VENGEANCE", "SIMMERING_FURY"), ("TALKTOTHEHAND", "TALK_TO_THE_HAND"), ("WAVEOFTHEHAND", "WAVE_OF_THE_HAND"), ("WHEELKICK", "WHEEL_KICK"), ("WINDMILLSTRIKE", "WINDMILL_STRIKE"),
    ("WREATHOFFLAME", "WREATH_OF_FLAME"), ("CONJUREBLADE", "CONJURE_BLADE"), ("DEUSEXMACHINA", "DEUS_EX_MACHINA"), ("DEVAFORM", "DEVA_FORM"), ("JUDGEMENT", "JUDGMENT"), ("LESSONLEARNED", "LESSON_LEARNED"), ("MASTERREALITY", "MASTER_REALITY"),
    ("SPIRITSHIELD", "SPIRIT_SHIELD"), ("HANDOFGREED", "HAND_OF_GREED"), ("RITUALDAGGER", "RITUAL_DAGGER"), ("PANICBUTTON", "PANIC_BUTTON"), ("ASCENDERSBANE", "ASCENDER_S_BANE"), ("CURSEOFTHEBELL", "CURSE_OF_THE_BELL"), ("BECOMEALMIGHTY", "BECOME_ALMIGHTY"),
    ("FAMEANDFORTUNE", "FAME_AND_FORTUNE"), ("LIVEFOREVER", "LIVE_FOREVER"), ("THROUGHVIOLENCE", "THROUGH_VIOLENCE"),
];

/// `ALIASES`, in the order of its rows.
#[allow(non_snake_case)]
pub fn ALIASES() -> &'static IndexMap<&'static str, &'static str> {
    static TABLE: OnceLock<IndexMap<&'static str, &'static str>> = OnceLock::new();
    TABLE.get_or_init(|| ALIASES_ROWS.iter().copied().collect())
}

// Cards whose upgrade lowers the cost: (cost, cost upgraded).
#[rustfmt::skip]
pub static UPGRADE_COST_ROWS: &[(&str, (i64, i64))] = &[
    ("BODY_SLAM", (1, 0)), ("HAVOC", (1, 0)), ("BLOOD_FOR_BLOOD", (4, 3)), ("EXHUME", (1, 0)), ("BARRICADE", (3, 2)), ("CORRUPTION", (3, 2)), ("ENTRENCH", (2, 1)), ("INFERNAL_BLADE", (1, 0)), ("SEEING_RED", (1, 0)), ("DARK_EMBRACE", (2, 1)), ("BULLET_TIME", (3, 2)),
    ("DISTRACTION", (1, 0)), ("NIGHTMARE", (3, 2)), ("PHANTASMAL_KILLER", (1, 0)), ("TERROR", (1, 0)), ("SETUP", (1, 0)), ("ALCHEMIZE", (1, 0)), ("RECYCLE", (1, 0)), ("DOUBLE_ENERGY", (1, 0)), ("CREATIVE_AI", (3, 2)), ("FUSION", (2, 1)), ("ZAP", (1, 0)),
    ("WHITE_NOISE", (1, 0)), ("ERUPTION", (2, 1)), ("TRANQUILITY", (1, 0)), ("CRESCENDO", (1, 0)), ("OMNISCIENCE", (4, 3)), ("VAULT", (3, 2)), ("SCRAWL", (1, 0)), ("STUDY", (2, 1)), ("RUSHDOWN", (1, 0)), ("APOTHEOSIS", (2, 1)), ("MADNESS", (1, 0)),
    ("MASTER_REALITY", (1, 0)), ("MAGNETISM", (2, 1)), ("MAYHEM", (2, 1)), ("MIND_BLAST", (2, 1)),
];

/// `UPGRADE_COST`, in the order of its rows.
#[allow(non_snake_case)]
pub fn UPGRADE_COST() -> &'static IndexMap<&'static str, (i64, i64)> {
    static TABLE: OnceLock<IndexMap<&'static str, (i64, i64)>> = OnceLock::new();
    TABLE.get_or_init(|| UPGRADE_COST_ROWS.iter().copied().collect())
}

#[allow(non_snake_case)]
pub fn _UPGRADE() -> &'static regex::Regex {
    crate::re!(r"(\d+)\((\d+)\)")
}

#[allow(non_snake_case)]
pub fn _ENERGY() -> &'static regex::Regex {
    crate::re!(r"(?i)\bgain\s+(\d+)\s+energy\b")
}

/// One version of a table entry, energy written the way the parser reads it ("Gain 2[energy_icon.png]").
pub fn expand(text: &str, upgraded: bool) -> String {
    let mut text = text;
    if text.contains("||") {
        text = text.splitn(2, "||").nth(if upgraded { 1 } else { 0 }).unwrap_or("");
    }
    let text = _UPGRADE().replace_all(text, |m: &regex::Captures| m[if upgraded { 2 } else { 1 }].to_string());
    _ENERGY().replace_all(&text, |m: &regex::Captures| format!("Gain {}[energy_icon.png]", &m[1])).into_owned()
}

#[allow(non_snake_case)]
pub fn _DEAL() -> &'static regex::Regex {
    crate::re!(r"(?i)\b(deal)\s+\d+\s+damage")
}

#[allow(non_snake_case)]
pub fn _GAIN_BLOCK() -> &'static regex::Regex {
    crate::re!(r"(?i)\b(gain)\s+\d+\s+block")
}

/// Sentences whose effect the game's own damage number already holds.
#[allow(non_snake_case)]
pub fn _FOLDED_IN() -> &'static regex::Regex {
    crate::re!(r"(?i)\s*(?:strength\s+affects\s+this\s+card\s+\d+\s+times|deals?\s+\d+\s+additional\s+damage\s+for\s+all\s+your\s+cards\s+containing\s+strike)\.?")
}

/// `isinstance(x, int)` (a bool is one in the original), and the way an f-string prints `max(0, x)`.
fn _int(v: &Value) -> Option<(i64, String)> {
    match v {
        Value::Bool(b) => Some((*b as i64, if *b { "True".to_string() } else { "0".to_string() })),
        Value::Number(n) if n.is_i64() || n.is_u64() => {
            let x = n.as_i64().unwrap_or(i64::MAX);
            Some((x, if x > 0 { n.to_string() } else { "0".to_string() }))
        }
        _ => None,
    }
}

/// A hand card's text with the damage and Block the game worked out for it (the bot's mod sends them), as StS2 prints them.
pub fn live_numbers(desc: &str, ctype: &str, card: &Value) -> String {
    let mut desc = desc.to_string();
    let (dmg, base_dmg) = (_int(&card["damage"]), _int(&card["base_damage"]));
    if let (true, Some((_, n)), Some((base_dmg, _))) = (ctype == "Attack", dmg, base_dmg) {
        if base_dmg >= 0 {
            desc = _FOLDED_IN().replace_all(&desc, "").into_owned();
            desc = crate::re!(r"(?i)\bdeal\s+damage\s+equal\s+to\s+[^.]*").replace_all(&desc, regex::NoExpand(&format!("Deal {n} damage"))).into_owned(); // (Body Slam, Mind Blast)
            desc = _DEAL().replace_all(&desc, |m: &regex::Captures| format!("{} {} damage", &m[1], n)).into_owned();
        }
    }
    let (blk, base_blk) = (_int(&card["block"]), _int(&card["base_block"]));
    if let (true, Some((_, n)), Some((base_blk, _))) = (ctype == "Attack" || ctype == "Skill", blk, base_blk) {
        if base_blk >= 0 {
            desc = _GAIN_BLOCK().replace_all(&desc, |m: &regex::Captures| format!("{} {} Block", &m[1], n)).into_owned();
        }
    }
    desc
}

/// The table key for a card, or None if it is not in the tables. (`name`: "" for none.)
pub fn card_key<A: Raw + ?Sized, B: Raw + ?Sized>(card_id: &A, name: &B) -> Option<String> {
    for raw in [card_id.raw(), name.raw()] {
        if raw.is_empty() {
            continue;
        }
        let key = key_of(&raw);
        let key = ALIASES().get(key.as_str()).map(|k| k.to_string()).unwrap_or(key);
        if CARDS().contains_key(key.as_str()) {
            return Some(key);
        }
    }
    None
}

/// What the card does ("" for one not in the tables). (`name`: "" for none; `upgraded` is false in the original when left out.)
pub fn card_text<A: Raw + ?Sized, B: Raw + ?Sized>(card_id: &A, name: &B, upgraded: bool) -> String {
    match card_key(card_id, name) {
        Some(key) => expand(CARDS()[key.as_str()], upgraded),
        None => String::new(),
    }
}

pub fn potion_key<A: Raw + ?Sized, B: Raw + ?Sized>(potion_id: &A, name: &B) -> String {
    let (potion_id, name) = (potion_id.raw(), name.raw());
    for raw in [&name, &potion_id] {
        let key = key_of(raw);
        if potions::POTIONS().contains_key(key.as_str()) {
            return key;
        }
    }
    key_of(if name.is_empty() { &potion_id } else { &name })
}

pub fn potion_text<A: Raw + ?Sized, B: Raw + ?Sized>(potion_id: &A, name: &B) -> String {
    potions::POTIONS().get(potion_key(potion_id, name).as_str()).copied().unwrap_or("").to_string()
}

pub fn relic_text<A: Raw + ?Sized, B: Raw + ?Sized>(relic_id: &A, name: &B) -> String {
    let (relic_id, name) = (relic_id.raw(), name.raw());
    let by_name = potions::RELICS().get(key_of(if name.is_empty() { &relic_id } else { &name }).as_str()).copied().unwrap_or("");
    if !by_name.is_empty() {
        return by_name.to_string();
    }
    potions::RELICS().get(key_of(&relic_id).as_str()).copied().unwrap_or("").to_string()
}

/// Lower is better (the order the boss's relics are offered to the bot in).
pub fn boss_relic_rank(relic: &Value) -> i64 {
    let key = key_of(&relic["name"].or(&relic["id"]).text());
    if let Some(i) = potions::AVOID.iter().position(|k| *k == key) {
        return 1000 + i as i64;
    }
    potions::BOSS_RELICS.iter().position(|k| *k == key).map(|i| i as i64).unwrap_or(500)
}

#[cfg(test)]
mod tests {
    //! Replays tests/golden/sts1_text.json (recorded from the original tables and functions).
    use super::*;
    use std::collections::HashMap;

    fn golden() -> Value {
        serde_json::from_str(include_str!("../../../../tests/golden/sts1_text.json")).unwrap()
    }

    fn rows(table: &IndexMap<&'static str, &'static str>) -> Value {
        Value::Array(table.iter().map(|(k, v)| serde_json::json!([k, v])).collect())
    }

    #[test]
    fn the_tables_hold_the_recorded_rows_in_the_recorded_order() {
        let g = golden();
        let t = &g["tables"];
        assert_eq!(t.as_object().unwrap().len(), 13);
        assert_eq!(rows(CARDS()), t["CARDS"]);
        assert_eq!(rows(ALIASES()), t["ALIASES"]);
        assert_eq!(rows(ironclad::CARDS()), t["ironclad.CARDS"]);
        assert_eq!(rows(silent::CARDS()), t["silent.CARDS"]);
        assert_eq!(rows(defect::CARDS()), t["defect.CARDS"]);
        assert_eq!(rows(watcher::CARDS()), t["watcher.CARDS"]);
        assert_eq!(rows(colorless::CARDS()), t["colorless.CARDS"]);
        assert_eq!(rows(potions::POTIONS()), t["potions.POTIONS"]);
        assert_eq!(rows(potions::RELICS()), t["potions.RELICS"]);
        let costs: Vec<Value> = UPGRADE_COST().iter().map(|(k, (a, b))| serde_json::json!([k, [a, b]])).collect();
        assert_eq!(Value::Array(costs), t["UPGRADE_COST"]);
        let mut keep = potions::KEEP.to_vec();
        keep.sort();
        assert_eq!(serde_json::json!(keep), t["potions.KEEP"]);
        assert_eq!(serde_json::json!(potions::BOSS_RELICS), t["potions.BOSS_RELICS"]);
        assert_eq!(serde_json::json!(potions::AVOID), t["potions.AVOID"]);
        // (no row said twice within a table: the row lists and the maps are the same thing)
        assert_eq!(ALIASES_ROWS.len(), ALIASES().len());
        assert_eq!(UPGRADE_COST_ROWS.len(), UPGRADE_COST().len());
    }

    #[test]
    fn expand_reads_every_text_both_ways() {
        let g = golden();
        let cases = g["expand"].list();
        assert!(cases.len() >= CARDS().len());
        for c in cases {
            assert_eq!(expand(c[0].s(), false), c[1].s(), "{}", c[0]);
            assert_eq!(expand(c[0].s(), true), c[2].s(), "{} (upgraded)", c[0]);
        }
    }

    #[test]
    fn ids_and_names_find_their_rows() {
        let g = golden();
        let cases = g["names"].list();
        assert!(cases.len() > 3000);
        for c in cases {
            let (id, name) = (&c[0], &c[1]);
            assert_eq!(card_key(id, name).map(Value::from).unwrap_or(Value::Null), c[2], "card_key {id} {name}");
            assert_eq!(card_text(id, name, false), c[3].s(), "card_text {id} {name}");
            assert_eq!(card_text(id, name, true), c[4].s(), "card_text upgraded {id} {name}");
            assert_eq!(potion_key(id, name), c[5].s(), "potion_key {id} {name}");
            assert_eq!(potion_text(id, name), c[6].s(), "potion_text {id} {name}");
            assert_eq!(relic_text(id, name), c[7].s(), "relic_text {id} {name}");
            // (a &str, a String and an Option read like the JSON string)
            if let (Some(i), Some(n)) = (id.as_str(), name.as_str()) {
                assert_eq!(card_key(i, n).map(Value::from).unwrap_or(Value::Null), c[2]);
                assert_eq!(card_text(&i.to_string(), &Some(n), true), c[4].s());
                assert_eq!(potion_key(i, &n.to_string()), c[5].s());
            }
            if let (Some(i), true) = (id.as_str(), name.is_null()) {
                assert_eq!(card_key(i, "").map(Value::from).unwrap_or(Value::Null), c[2]);
                assert_eq!(potion_text(i, &None::<&str>), c[6].s());
                assert_eq!(relic_text(i, ""), c[7].s());
            }
        }
    }

    #[test]
    fn live_numbers_as_the_record_writes_them() {
        let g = golden();
        let numbers = g["live_numbers_cards"].list();
        let changed: HashMap<(String, bool, String, usize), &str> =
            g["live_numbers"].list().iter().map(|c| ((c[0].s().to_string(), c[1].truthy(), c[2].s().to_string(), c[3].int() as usize), c[4].s())).collect();
        assert!(changed.len() > 2000);
        let mut seen = 0;
        for (key, raw) in CARDS() {
            for up in [false, true] {
                let desc = expand(raw, up);
                for ctype in ["Attack", "Skill", "Power"] {
                    for (n, card) in numbers.iter().enumerate() {
                        let got = live_numbers(&desc, ctype, card);
                        match changed.get(&(key.to_string(), up, ctype.to_string(), n)) {
                            Some(want) => {
                                seen += 1;
                                assert_eq!(&got, want, "{key} {up} {ctype} {card}")
                            }
                            None => assert_eq!(got, desc, "{key} {up} {ctype} {card} (unchanged in the original)"),
                        }
                    }
                }
            }
        }
        assert_eq!(seen, changed.len());
        let free = g["live_numbers_free"].list();
        assert!(free.len() >= 500);
        for c in free {
            assert_eq!(live_numbers(c[0].s(), c[1].s(), &c[2]), c[3].s(), "{} {} {}", c[0], c[1], c[2]);
        }
    }

    #[test]
    fn boss_relics_rank_as_recorded() {
        let g = golden();
        let cases = g["boss_relic_rank"].list();
        assert!(cases.len() > 60);
        for c in cases {
            assert_eq!(boss_relic_rank(&c[0]), c[1].as_i64().unwrap(), "{}", c[0]);
        }
    }
}
