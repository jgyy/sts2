//! What the things an event gives and takes are worth, on the scale its options are scored on (a relic is 6, an upgrade or a
//! card removed 4): the shared prices the per-event rules (sts2.rs, sts1.rs) are built from, in one place so that every event
//! weighs gold, HP, curses and cards alike.

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::LazyLock;

use indexmap::IndexMap;
use serde_json::{json, Value};

use crate::cards::{_card_key, deck_value, is_basic, parse};
use crate::base::{self, JsonExt};

pub const RELIC: f64 = 6.0;
pub const POTION: f64 = 1.5;
pub const CARD_REWARD: f64 = 1.2; // a card reward of three, the best one taken (or none)
pub const UPGRADE: f64 = 4.0; // an upgrade of our choosing (the best card for it)
pub const RANDOM_UPGRADE: f64 = 2.0; // one at random (as often a card we hardly play)
pub const ACT3_HP: f64 = 1.6; // a point of HP given up in act 3 is dearer (the last boss is won from full HP)
/// points a gold: shops left to spend it in (a relic is about 200 gold, a removal 100)
pub static GOLD_RATE: LazyLock<IndexMap<i64, f64>> = LazyLock::new(|| IndexMap::from([(1, 1.0 / 40.0), (2, 1.0 / 45.0), (3, 1.0 / 70.0)]));
pub const GOLD_FULL: i64 = 300; // gold past this is worth half as much: there is only so much on a shelf
pub const FIGHTS_PER_FLOOR: f64 = 0.55; // combats a floor, on the way to the last boss
pub static LAST_FLOOR: LazyLock<IndexMap<&'static str, i64>> = LazyLock::new(|| IndexMap::from([("sts2", 48), ("sts1", 50)]));

/// Curses and the like by what they cost in the deck (the generic scorer's table, events._CURSES, uses the same numbers).
#[rustfmt::skip]
pub static CURSE: LazyLock<IndexMap<&'static str, f64>> = LazyLock::new(|| IndexMap::from([
    ("ascenders bane", 1.5), ("clumsy", 1.5), ("guilty", 1.0), ("poor sleep", 6.0), ("folly", 3.0), ("spore mind", 2.5), ("injury", 3.0), ("debt", 4.0), ("decay", 4.5),
    ("doubt", 4.5), ("shame", 4.5), ("greed", 5.0), ("curse of the bell", 5.0), ("regret", 5.0), ("writhe", 5.0), ("normality", 8.0), ("enthralled", 8.0),
    ("bad luck", 10.0), ("pain", 5.0), ("parasite", 4.0), ("necronomicurse", 6.0), ("pride", 4.0), ("wound", 2.0), ("dazed", 1.5), ("burn", 2.5), ("void", 3.0),
    ("slimed", 1.0), ("soot", 3.0), ("infection", 3.5),
]));
/// never removed
pub static ETERNAL: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| HashSet::from(["ascenders bane", "greed", "curse of the bell", "folly", "bad luck", "enthralled", "necronomicurse"]));
pub const RANDOM_CURSE: f64 = 4.5; // "Add a random Curse": the mean of what can come

crate::defaults! {
    /// The run as an event sees it.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Ctx {
        pub hp: i64,
        pub max_hp: i64,
        pub gold: i64,
        pub act: i64 = 1,
        pub floor: i64 = 1,
        pub deck: Vec<Value>, // card dicts (strategy.Memory.deck)
        pub relics: Vec<Value>,
        pub potions: Vec<Value>,
        pub slots: i64 = 3, // potion slots
        pub character: String,
        pub game: String = "sts2".to_string(),
        pub bled: i64,                // HP this event has taken already
        pub keys: Option<Vec<Value>>, // the first game's keys, once act 4 is open
    }
}

/// (`hp` and `max_hp` have no default in the original: every caller gives them.)

impl Ctx {
    pub fn free_slots(&self) -> i64 {
        (self.slots - self.potions.len() as i64).max(0)
    }

    pub fn fights_left(&self) -> f64 {
        ((LAST_FLOOR.get(self.game.as_str()).copied().unwrap_or(48) - self.floor) as f64 * FIGHTS_PER_FLOOR).max(1.0)
    }
}

/// A card as the helpers take it: its dict, or only its name (the original's `card: dict | str`).
pub trait CardOrName {
    /// The name, when this is a name and not a card's dict.
    fn as_name(&self) -> Option<&str>;
    /// The card's dict (`{"name": card}` for a name).
    fn as_card(&self) -> Cow<'_, Value>;
}

impl CardOrName for str {
    fn as_name(&self) -> Option<&str> {
        Some(self)
    }
    fn as_card(&self) -> Cow<'_, Value> {
        Cow::Owned(json!({"name": self}))
    }
}

impl CardOrName for String {
    fn as_name(&self) -> Option<&str> {
        Some(self)
    }
    fn as_card(&self) -> Cow<'_, Value> {
        Cow::Owned(json!({"name": self}))
    }
}

impl CardOrName for Value {
    fn as_name(&self) -> Option<&str> {
        self.as_str()
    }
    fn as_card(&self) -> Cow<'_, Value> {
        match self {
            Value::String(s) => Cow::Owned(json!({"name": s})),
            _ => Cow::Borrowed(self),
        }
    }
}

impl<T: CardOrName + ?Sized> CardOrName for &T {
    fn as_name(&self) -> Option<&str> {
        (**self).as_name()
    }
    fn as_card(&self) -> Cow<'_, Value> {
        (**self).as_card()
    }
}

pub fn num(pattern: &str, text: &str, default: i64) -> i64 {
    match base::re::cached_i(pattern).captures(text) {
        Some(m) => {
            let digits = m.get(1).map(|g| g.as_str()).unwrap_or("");
            digits.parse().unwrap_or_else(|_| panic!("invalid literal for int() with base 10: {}", base::str_repr(digits)))
        }
        None => default,
    }
}

pub fn name_of<C: CardOrName + ?Sized>(card: &C) -> String {
    let raw = match card.as_name() {
        Some(name) => name.to_string(),
        None => {
            let card = card.as_card();
            card["name"].or(&card["id"]).text()
        }
    };
    crate::re!(r"[^a-z' ]").replace_all(&raw.to_lowercase().replace('_', " "), "").replace('\'', "").trim().to_string()
}

pub fn is_curse(card: &Value) -> bool {
    matches!(base::str_of(&card["type"]).as_str(), "Curse" | "Status") || CURSE.contains_key(name_of(card).as_str())
}

/// HP given up: dear as a share of what is left, dearer in act 3, the end of the run if it is all of it.
pub fn hp_cost(loss: i64, ctx: &Ctx) -> f64 {
    if loss <= 0 {
        return 0.0;
    }
    if loss >= ctx.hp {
        return 1000.0;
    }
    12.0 * loss as f64 / ctx.hp.max(1) as f64 * (if ctx.act >= 3 { ACT3_HP } else { 1.0 })
}

pub fn heal(amount: i64, ctx: &Ctx) -> f64 {
    12.0 * amount.max(0).min((ctx.max_hp - ctx.hp).max(0)) as f64 / ctx.max_hp.max(1) as f64
}

pub fn max_hp_gain(n: i64) -> f64 {
    0.35 * n.min(17) as f64 + 0.2 * (n - 17).max(0) as f64
}

pub fn max_hp_loss(n: i64, ctx: &Ctx) -> f64 {
    let share = n as f64 / ctx.max_hp.max(1) as f64;
    1.5 + 60.0 * share + (if share >= 0.25 { 1000.0 } else { 0.0 })
}

pub fn _gold_curve(g: f64, rate: f64) -> f64 {
    rate * g.min(GOLD_FULL as f64) + rate / 2.0 * (g - GOLD_FULL as f64).max(0.0)
}

/// Gold gained (or paid, negative): what it adds to what we hold, by the shops left to spend it in.
pub fn gold(change: i64, ctx: &Ctx) -> f64 {
    let rate = GOLD_RATE.get(&ctx.act.min(3)).copied().unwrap_or(GOLD_RATE[&3]);
    let have = ctx.gold.max(0);
    _gold_curve((have + change).max(0) as f64, rate) - _gold_curve(have as f64, rate)
}

pub fn curse(name: &str) -> f64 {
    CURSE.get(name_of(name).as_str()).copied().unwrap_or(RANDOM_CURSE)
}

/// What a card adds to the deck (0 for an average one): its rating less what an average pick is worth.
pub fn card_worth<C: CardOrName + ?Sized>(card: &C) -> f64 {
    0.5 * (deck_value(&card.as_card()) - 3.0)
}

pub fn _removal_value(card: &Value) -> f64 {
    if is_curse(card) {
        let name = name_of(card);
        return if ETERNAL.contains(name.as_str()) { 0.0 } else { curse(&name) + 1.0 };
    }
    let name = card["name"].text();
    if is_basic(card) || is_basic(&json!({"name": name.trim_end_matches('+')})) {
        let up = name.ends_with('+') || card["is_upgraded"].truthy();
        let strike = name_of(card).contains("strike");
        return (if strike { 4.0 } else { 3.5 }) - (if up { 1.0 } else { 0.0 });
    }
    (3.0 - 0.5 * (deck_value(card) - 2.0).max(0.0)).max(0.5)
}

/// `worths[:n]` of the deck's removal values, the dearest first.
fn _dearest(ctx: &Ctx, n: i64) -> (Vec<f64>, usize) {
    let mut worths: Vec<f64> = ctx.deck.iter().map(_removal_value).collect();
    base::sort_by_key_rev(&mut worths, |w| *w);
    let len = worths.len();
    let end = if n < 0 { (len as i64 + n).max(0) as usize } else { (n as usize).min(len) };
    worths.truncate(end);
    (worths, len)
}

/// Removing the n cards the deck is best without (a curse first, then Strikes, then Defends).
pub fn removals(ctx: &Ctx, n: i64) -> f64 {
    let (worths, len) = _dearest(ctx, n);
    if len == 0 {
        return 3.5 * n as f64; // (nothing known of the deck: a Strike's worth)
    }
    base::fsum(worths) + 0.5 * (n - len as i64).max(0) as f64
}

/// Transforming the n cards we choose: each goes (as a removal) and a random card of ours comes.
pub fn transforms(ctx: &Ctx, n: i64) -> f64 {
    if ctx.deck.is_empty() {
        return 2.5 * n as f64;
    }
    base::fsum(_dearest(ctx, n).0.into_iter().map(|w| (w - 1.5).max(1.0)))
}

pub fn upgrades_left(ctx: &Ctx) -> i64 {
    ctx.deck.iter().filter(|c| !is_curse(c) && !c["name"].text().ends_with('+') && !c["is_upgraded"].truthy()).count() as i64
}

pub fn random_upgrades(n: i64, ctx: &Ctx) -> f64 {
    let left = if ctx.deck.is_empty() { 10 } else { upgrades_left(ctx) };
    RANDOM_UPGRADE * n.min(left) as f64
}

/// Potions on a belt with room for them (the rest are mostly lost: at best one pushes out a weaker one we hold).
pub fn potions_gained(n: i64, ctx: &Ctx, each: f64) -> f64 {
    each * n.min(ctx.free_slots()) as f64 + 0.1 * (n - ctx.free_slots()).max(0) as f64
}

/// Card sorts in the deck: Strikes, Defends (basic), Powers, attacks, Block cards, big attacks.
pub fn counts(ctx: &Ctx) -> IndexMap<&'static str, i64> {
    let mut out = IndexMap::from([("strikes", 0), ("defends", 0), ("powers", 0), ("attacks", 0), ("blocks", 0), ("big_attacks", 0), ("basics", 0)]);
    for c in &ctx.deck {
        let (n, info) = (name_of(c), parse(c));
        let basic = is_basic(c) || is_basic(&json!({"name": c["name"].text().trim_end_matches('+')}));
        out["basics"] += (basic && !is_curse(c)) as i64;
        out["strikes"] += (basic && n.contains("strike")) as i64;
        out["defends"] += (basic && n.contains("defend")) as i64;
        out["powers"] += (base::str_of(&c["type"]) == "Power") as i64;
        out["attacks"] += (info.damage != 0) as i64;
        out["blocks"] += (info.block != 0) as i64;
        out["big_attacks"] += (info.total_damage() >= 10 && info.cost >= 1) as i64;
    }
    out
}

pub fn card_key<C: CardOrName + ?Sized>(card: &C) -> String {
    _card_key(&card.as_card())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games;

    fn ctx_of(v: &Value, cards: &[Value]) -> Ctx {
        Ctx {
            hp: v["hp"].as_i64().unwrap(),
            max_hp: v["max_hp"].as_i64().unwrap(),
            gold: v["gold"].as_i64().unwrap(),
            act: v["act"].as_i64().unwrap(),
            floor: v["floor"].as_i64().unwrap(),
            deck: v["deck"].list().iter().map(|n| cards[n.as_u64().unwrap() as usize]["c"].clone()).collect(),
            relics: v["relics"].list().to_vec(),
            potions: v["potions"].list().to_vec(),
            slots: v["slots"].as_i64().unwrap(),
            character: v["character"].s().into(),
            game: v["game"].s().into(),
            bled: v["bled"].as_i64().unwrap(),
            keys: v["keys"].as_array().cloned(),
        }
    }

    /// (the original's numbers as floats: `sum()` of nothing is the int 0 there.)
    fn floats(xs: &Value) -> Vec<f64> {
        xs.list().iter().map(|x| x.as_f64().unwrap()).collect()
    }

    #[test]
    fn golden() {
        let g: Value = serde_json::from_str(include_str!("../../../tests/golden/events_value.json")).unwrap();
        let mut checked = 0;
        let patterns: Vec<&str> = g["patterns"].list().iter().map(|p| p.s()).collect();
        for row in g["texts"].list() {
            let text = row["t"].s();
            assert_eq!(name_of(text), row["name"].s(), "name_of {text:?}");
            assert_eq!(name_of(&row["t"]), row["name"].s(), "name_of {text:?}");
            assert_eq!(curse(text), row["curse"].as_f64().unwrap(), "curse {text:?}");
            let nums: Vec<i64> = patterns.iter().flat_map(|p| [num(p, text, 0), num(p, text, 50)]).collect();
            let hits: Vec<Value> = patterns.iter().enumerate().filter(|(n, _)| nums[2 * n] != 0 || nums[2 * n + 1] != 50).map(|(n, _)| json!([n, nums[2 * n]])).collect();
            assert_eq!(json!(hits), row["nums"], "num {text:?}");
            checked += 3 + patterns.len();
        }
        for game in ["sts2", "sts1"] {
            let _game = games::playing(game);
            for row in g["cards"][game].list() {
                let card = &row["c"];
                let why = format!("{game} {card}");
                assert_eq!(name_of(card), row["name"].s(), "{why}");
                assert_eq!(is_curse(card), row["curse"].as_bool().unwrap(), "{why}");
                assert_eq!(card_worth(card), row["worth"].as_f64().unwrap(), "{why}");
                assert_eq!(_removal_value(card), row["removal"].as_f64().unwrap(), "{why}");
                assert_eq!(card_key(card), row["key"].s(), "{why}");
                checked += 5;
            }
            for row in g["names"][game].list() {
                let name = row[0].s();
                assert_eq!(card_worth(name), row[1].as_f64().unwrap(), "{game} card_worth {name:?}");
                assert_eq!(card_worth(&name.to_string()), row[1].as_f64().unwrap());
                assert_eq!(card_key(name), row[2].s(), "{game} card_key {name:?}");
                checked += 2;
            }
        }
        for row in g["ctxs"].list() {
            let game = if row["ctx"]["game"].s() == "sts1" { "sts1" } else { "sts2" };
            let ctx = ctx_of(&row["ctx"], g["cards"][game].list());
            let _game = games::playing(game);
            let why = format!("ctx {}", base::json_dumps(&row["ctx"]));
            assert_eq!(ctx.free_slots(), row["free_slots"].as_i64().unwrap(), "{why}");
            assert_eq!(ctx.fights_left(), row["fights_left"].as_f64().unwrap(), "{why}");
            let ns: Vec<i64> = g["ns"].list().iter().map(|n| n.as_i64().unwrap()).collect();
            let golds: Vec<i64> = g["golds"].list().iter().map(|n| n.as_i64().unwrap()).collect();
            assert_eq!(ns.iter().map(|n| hp_cost(*n, &ctx)).collect::<Vec<_>>(), floats(&row["hp_cost"]), "hp_cost {why}");
            assert_eq!(ns.iter().map(|n| heal(*n, &ctx)).collect::<Vec<_>>(), floats(&row["heal"]), "heal {why}");
            assert_eq!(ns.iter().map(|n| max_hp_loss(*n, &ctx)).collect::<Vec<_>>(), floats(&row["max_hp_loss"]), "max_hp_loss {why}");
            assert_eq!(golds.iter().map(|n| gold(*n, &ctx)).collect::<Vec<_>>(), floats(&row["gold"]), "gold {why}");
            let few = [0, 1, 2, 3, 5, 40];
            assert_eq!(few.iter().map(|n| removals(&ctx, *n)).collect::<Vec<_>>(), floats(&row["removals"]), "removals {why}");
            assert_eq!(few.iter().map(|n| transforms(&ctx, *n)).collect::<Vec<_>>(), floats(&row["transforms"]), "transforms {why}");
            assert_eq!(few.iter().map(|n| random_upgrades(*n, &ctx)).collect::<Vec<_>>(), floats(&row["random_upgrades"]), "{why}");
            assert_eq!(few.iter().flat_map(|n| [potions_gained(*n, &ctx, POTION), potions_gained(*n, &ctx, 2.25)]).collect::<Vec<_>>(), floats(&row["potions_gained"]), "{why}");
            assert_eq!(upgrades_left(&ctx), row["upgrades_left"].as_i64().unwrap(), "{why}");
            let counted: serde_json::Map<String, Value> = counts(&ctx).into_iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
            assert_eq!(base::json_dumps(&Value::Object(counted)), base::json_dumps(&row["counts"]), "{why}");
            checked += 2 * ns.len() + ns.len() + golds.len() + 5 * few.len() + 4;
        }
        for row in g["max_hp_gain"].list() {
            assert_eq!(max_hp_gain(row[0].as_i64().unwrap()), row[1].as_f64().unwrap());
            checked += 1;
        }
        for row in g["gold_curve"].list() {
            assert_eq!(_gold_curve(row[0].as_f64().unwrap(), row[1].as_f64().unwrap()), row[2].as_f64().unwrap());
            checked += 1;
        }
        assert!(checked > 20000, "{checked} golden cases");
    }
}
