//! Cards as the simulator holds them: a definition from a game's tables, and a copy in a deck or a fight.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use regex::{Captures, NoExpand, Regex};
use serde_json::{json, Map, Value};

use crate::cards::{parse, CardInfo};

/// The keywords a card can also carry in its text ("Exhaust.", "Ethereal. ..."): `_KEYWORD[k]` of the original.
#[allow(non_snake_case)]
pub fn _KEYWORD(keyword: &str) -> Option<&'static Regex> {
    // (the original's `$` also matches before a last newline: `\n?\z`)
    Some(match keyword {
        "Exhaust" => crate::re!(r"(?i)(?:^|[.!]\s*)Exhaust(?:\.|\n?\z)"),
        "Ethereal" => crate::re!(r"(?i)(?:^|[.!]\s*)Ethereal(?:\.|\n?\z)"),
        "Retain" => crate::re!(r"(?i)(?:^|[.!]\s*)Retain(?:\.|\n?\z)"),
        "Innate" => crate::re!(r"(?i)(?:^|[.!]\s*)Innate(?:\.|\n?\z)"),
        "Unplayable" => crate::re!(r"(?i)(?:^|[.!]\s*)Unplayable(?:\.|\n?\z)"),
        _ => return None,
    })
}

#[allow(non_snake_case)]
pub fn _DEAL() -> &'static Regex {
    crate::re!(r"(?i)\b(deal)\s+\d+\s+damage")
}

#[allow(non_snake_case)]
pub fn _DEAL_EQUAL() -> &'static Regex {
    crate::re!(r"(?i)\bdeal\s+damage\s+equal\s+to\s+[^.]*")
}

#[allow(non_snake_case)]
pub fn _BLOCK() -> &'static Regex {
    crate::re!(r"(?i)\b(gain)\s+\d+\s+block")
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct CardDef {
    pub key: String, // the tables' key, also the card's id: POMMEL_STRIKE
    pub name: String,
    pub r#type: String, // Attack, Skill, Power, Status, Curse
    pub cost: i64,      // -1: X; -2: unplayable
    pub rarity: String, // Basic, Common, Uncommon, Rare, Event, Status, Curse
    pub targeted: bool,
    pub text: String,
    pub text_up: String,
    pub cost_up: i64,
    pub color: String, // the character whose pool it is in, or COLORLESS, STATUS, CURSE
    pub star_cost: Option<i64>,
    pub star_cost_up: Option<i64>,
    pub keywords: Vec<String>, // (Slay the Spire 2 lists Exhaust, Ethereal, Retain, Innate, Sly ... apart from the text)
    pub keywords_up: Vec<String>,
}

/// (What tells two definitions apart nearly always; equality looks at every field.)
impl Hash for CardDef {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state);
        self.text.hash(state);
        self.text_up.len().hash(state);
        self.cost.hash(state);
        self.cost_up.hash(state);
    }
}

thread_local! {
    // info_of's results, for the base card and the upgraded one
    static INFO: RefCell<[HashMap<CardDef, Rc<CardInfo>>; 2]> = RefCell::new([HashMap::new(), HashMap::new()]);
}

/// What the card's printed text says (base numbers: the engine adds Strength and the rest itself).
pub fn info_of(d: &CardDef, up: bool) -> Rc<CardInfo> {
    if let Some(info) = INFO.with(|m| m.borrow()[up as usize].get(d).cloned()) {
        return info;
    }
    let kws = if up { &d.keywords_up } else { &d.keywords };
    let mut card = Map::new();
    card.insert("id".into(), json!(d.key));
    card.insert("name".into(), json!(d.name));
    card.insert("type".into(), json!(d.r#type));
    card.insert("cost".into(), json!(if d.cost == -1 { "X".to_string() } else { (if up { d.cost_up } else { d.cost }).max(0).to_string() }));
    card.insert("description".into(), json!(if up { &d.text_up } else { &d.text }));
    card.insert("target_type".into(), json!(target_type(d, up)));
    card.insert("star_cost".into(), json!(if up && d.star_cost_up.is_some() { d.star_cost_up } else { d.star_cost }));
    card.insert("keywords".into(), Value::Array(kws.iter().map(|k| json!({"name": k, "description": ""})).collect()));
    let info = Rc::new(parse(&Value::Object(card)));
    INFO.with(|m| m.borrow_mut()[up as usize].insert(d.clone(), info.clone()));
    info
}

pub fn target_type(d: &CardDef, up: bool) -> &'static str {
    if d.targeted {
        return "AnyEnemy";
    }
    if crate::re!(r"(?i)\ball enemies\b").is_match(if up { &d.text_up } else { &d.text }) {
        "AllEnemies"
    } else {
        "Self"
    }
}

/// `Card.view`'s keyword arguments.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewArgs {
    pub index: Option<i64>,
    pub damage: Option<i64>,
    pub block: Option<i64>,
    pub playable: bool,
}

impl Default for ViewArgs {
    fn default() -> Self {
        ViewArgs { index: None, damage: None, block: None, playable: true }
    }
}

thread_local! {
    static UID: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// A copy of a card. `cost_fight`/`cost_turn` override the printed cost (Madness, Infernal Blade, Swivel).
///
/// the original tells cards apart by identity (`c in f.hand`, `f.hand.remove(c)`, `h is not c`, `id(c)`): here every card made
/// (`Card::new`, `copy`) gets its own `uid`, and `clone()` is another handle on the SAME card (the same `uid`).
#[derive(Clone, Debug)]
pub struct Card {
    pub uid: u64,
    pub d: Rc<CardDef>,
    pub up: bool,
    pub cost_fight: Option<i64>,
    pub cost_turn: Option<i64>,
    pub bonus: i64, // damage or Block a card has grown by this fight (Rampage, Windmill Strike, Perseverance)
    pub retained: i64,
}

impl Card {
    pub fn new(d: &Rc<CardDef>, up: bool) -> Card {
        let uid = UID.with(|u| {
            u.set(u.get() + 1);
            u.get()
        });
        Card { uid, d: d.clone(), up, cost_fight: None, cost_turn: None, bonus: 0, retained: 0 }
    }

    pub fn key(&self) -> &str {
        &self.d.key
    }

    pub fn r#type(&self) -> &str {
        &self.d.r#type
    }

    pub fn name(&self) -> String {
        if self.up {
            format!("{}+", self.d.name)
        } else {
            self.d.name.clone()
        }
    }

    pub fn text(&self) -> &str {
        if self.up {
            &self.d.text_up
        } else {
            &self.d.text
        }
    }

    pub fn info(&self) -> Rc<CardInfo> {
        info_of(&self.d, self.up)
    }

    pub fn printed_cost(&self) -> i64 {
        if self.up {
            self.d.cost_up
        } else {
            self.d.cost
        }
    }

    pub fn cost(&self) -> i64 {
        let base = self.printed_cost();
        if base < 0 {
            base
        } else {
            self.cost_turn.or(self.cost_fight).unwrap_or(base)
        }
    }

    pub fn star_cost(&self) -> Option<i64> {
        if self.up && self.d.star_cost_up.is_some() {
            self.d.star_cost_up
        } else {
            self.d.star_cost
        }
    }

    pub fn keywords(&self) -> &[String] {
        if self.up {
            &self.d.keywords_up
        } else {
            &self.d.keywords
        }
    }

    pub fn has(&self, keyword: &str) -> bool {
        self.keywords().iter().any(|k| k == keyword) || _KEYWORD(keyword).is_some_and(|r| r.is_match(self.text()))
    }

    pub fn copy(&self) -> Card {
        let mut c = Card::new(&self.d, self.up);
        c.cost_fight = self.cost_fight;
        c.bonus = self.bonus;
        c
    }

    /// The card as the bot's state lists it; `damage`/`block`: the numbers the game shows for this moment of the fight.
    pub fn view(&self, args: ViewArgs) -> Value {
        let ViewArgs { index, damage, block, playable } = args;
        let cost = self.cost();
        let mut desc = self.text().to_string();
        if let (Some(damage), "Attack") = (damage, self.r#type()) {
            let n = damage.max(0);
            let equal = _DEAL_EQUAL().replace_all(&desc, NoExpand(&format!("Deal {n} damage"))).into_owned();
            desc = _DEAL().replace_all(&equal, |m: &Captures| format!("{} {n} damage", &m[1])).into_owned();
        }
        if let Some(block) = block {
            let n = block.max(0);
            desc = _BLOCK().replace_all(&desc, |m: &Captures| format!("{} {n} Block", &m[1])).into_owned();
        }
        let mut out = Map::new();
        out.insert("id".into(), json!(self.d.key));
        out.insert("name".into(), json!(self.name()));
        out.insert("type".into(), json!(self.r#type()));
        out.insert("cost".into(), json!(if cost == -1 { "X".to_string() } else { cost.max(0).to_string() }));
        out.insert("star_cost".into(), json!(self.star_cost()));
        out.insert("description".into(), json!(desc));
        out.insert("target_type".into(), json!(target_type(&self.d, self.up)));
        out.insert("can_play".into(), json!(playable && cost != -2));
        out.insert("is_upgraded".into(), json!(self.up));
        out.insert("rarity".into(), json!(self.d.rarity));
        if cost == -2 {
            out.insert("unplayable_reason".into(), json!("Unplayable"));
        }
        if !self.keywords().is_empty() {
            out.insert("keywords".into(), Value::Array(self.keywords().iter().map(|k| json!({"name": k, "description": ""})).collect()));
        }
        if let Some(index) = index {
            out.insert("index".into(), json!(index));
        }
        Value::Object(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games;
    use crate::base::{self, JsonExt};

    fn def_of(v: &Value) -> CardDef {
        let words = |x: &Value| x.list().iter().map(|k| k.s().to_string()).collect::<Vec<_>>();
        CardDef {
            key: v["key"].s().into(),
            name: v["name"].s().into(),
            r#type: v["type"].s().into(),
            cost: v["cost"].as_i64().unwrap(),
            rarity: v["rarity"].s().into(),
            targeted: v["targeted"].as_bool().unwrap(),
            text: v["text"].s().into(),
            text_up: v["text_up"].s().into(),
            cost_up: v["cost_up"].as_i64().unwrap(),
            color: v["color"].s().into(),
            star_cost: v["star_cost"].as_i64(),
            star_cost_up: v["star_cost_up"].as_i64(),
            keywords: words(&v["keywords"]),
            keywords_up: words(&v["keywords_up"]),
        }
    }

    /// The fields of `info` that are not as `CardInfo::default()` has them (as tests/golden/cards.json keeps them).
    fn diff(info: &CardInfo) -> Value {
        let base = CardInfo::default().to_json();
        Value::Object(info.to_json().as_object().unwrap().iter().filter(|(k, v)| base[k.as_str()] != **v).map(|(k, v)| (k.clone(), v.clone())).collect())
    }

    fn opt(v: &Value) -> Option<i64> {
        v.as_i64()
    }

    /// A view against the golden file's: the whole dict as the original prints it, or the start of the SHA-1 of that text.
    fn same(got: &Value, want: &Value, why: &str) {
        use sha1::{Digest, Sha1};
        let text = base::json_dumps(got);
        match want.as_str() {
            Some(hash) => {
                let digest: String = Sha1::digest(text.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
                assert_eq!(&digest[..10], hash, "{why}: {text}");
            }
            None => assert_eq!(text, base::json_dumps(want), "{why}"),
        }
    }

    #[test]
    fn golden() {
        let g: Value = serde_json::from_str(include_str!("../../tests/golden/sim_cards.json")).unwrap();
        let mut checked = 0;
        for case in g["cards"].list() {
            let _game = games::playing(case["g"].s());
            let d = Rc::new(def_of(&case["d"]));
            let why = format!("{} {}", case["g"].s(), d.key);
            for (n, up) in [false, true].into_iter().enumerate() {
                let want = &case["ups"][n];
                assert_eq!(target_type(&d, up), want["target"].s(), "{why}");
                let info = info_of(&d, up);
                assert!(Rc::ptr_eq(&info, &info_of(&d, up)), "{why}");
                assert_eq!(diff(&info), want["info"], "{why} up {up}");
                let mut c = Card::new(&d, up);
                assert_eq!(*c.info(), *info);
                assert_eq!(json!([c.key(), c.r#type(), c.name(), c.printed_cost(), c.cost(), c.star_cost(), c.keywords()]), want["props"], "{why}");
                let has: Vec<bool> = ["Exhaust", "Ethereal", "Retain", "Innate", "Unplayable", "Sly", "Eternal"].iter().map(|k| c.has(k)).collect();
                assert_eq!(json!(has), want["has"], "{why}");
                same(&c.view(ViewArgs::default()), &want["view"], &why);
                assert_eq!(c.view(ViewArgs::default())["description"].s(), c.text(), "{why}");
                for v in want["views"].list() {
                    let a = &v["args"];
                    c.cost_fight = opt(&a[4]);
                    c.cost_turn = opt(&a[5]);
                    let got = c.view(ViewArgs { index: opt(&a[0]), damage: opt(&a[1]), block: opt(&a[2]), playable: a[3].as_bool().unwrap() });
                    same(&got, &v["out"], &format!("{why} {a}"));
                    assert_eq!(c.cost(), v["cost"].as_i64().unwrap(), "{why} {a}");
                    checked += 1;
                }
                c.bonus = 3;
                c.retained = 2;
                let copy = c.copy();
                assert_eq!((copy.cost_fight, copy.cost_turn, copy.bonus, copy.retained, copy.up), (c.cost_fight, None, 3, 0, up));
                checked += 5;
            }
        }
        assert!(checked > 5000, "{checked} golden cases");
    }
}
