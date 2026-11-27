//! What both games' content modules share: the map, the events, the shop and the card offers.

use std::collections::{BTreeSet, HashSet};
use std::rc::Rc;

use indexmap::IndexMap;
use serde_json::{json, Map, Value};

use crate::py::{self, JsonExt, Random};
use crate::run::strategy;
use crate::sim::cards::{Card, CardDef, ViewArgs};
use crate::sim::content::{Choice, Item, Nodes, Odds};
use crate::sim::effects::Powers;
use crate::sim::engine::Fight;
use crate::sim::run::{Potion, Run};

pub const ROWS: i64 = 15;
pub const COLS: i64 = 7;
pub const PATHS: i64 = 6;

/// An act's map as the bot reads it: (col, row) -> {col, row, type, children}, rows 1..rows, the boss above them
/// (`make_map(rng, weights, *, elite_from=5, treasure_row=8, rows=ROWS)`).
///
/// Six paths climb from the bottom, one column left, right or straight at a time; rooms are drawn by weight,
/// the first row is fights, `treasure_row` a chest and the last row rest sites; no elite or rest site before
/// `elite_from`, none twice in a row on a path, and no rest site just below the last row.
pub fn make_map(rng: &mut Random, weights: &IndexMap<String, f64>, elite_from: i64, treasure_row: i64, rows: i64) -> Nodes {
    type At = (i64, i64);
    let mut edges: IndexMap<At, BTreeSet<At>> = IndexMap::new();
    let mut starts: Vec<i64> = Vec::new();
    for p in 0..PATHS {
        let mut col = rng.randrange1(COLS);
        while p < 2 && starts.contains(&col) {
            col = rng.randrange1(COLS);
        }
        starts.push(col);
        for row in 1..rows {
            let nxt = (COLS - 1).min(0.max(col + *rng.choice(&[-1, 0, 1])));
            edges.entry((col, row)).or_default().insert((nxt, row + 1));
            col = nxt;
        }
        edges.entry((col, rows)).or_default();
    }
    let boss = (3, rows + 1);
    for kids in edges.iter_mut().filter(|(k, _)| k.1 == rows).map(|(_, kids)| kids) {
        *kids = BTreeSet::from([boss]);
    }
    let mut parents: IndexMap<At, Vec<At>> = IndexMap::new();
    for (k, kids) in &edges {
        for c in kids {
            parents.entry(*c).or_default().push(*k);
        }
    }
    let mut types: IndexMap<At, String> = IndexMap::new();
    for row in 1..=rows {
        let mut here: Vec<At> = edges.keys().filter(|k| k.1 == row).copied().collect();
        here.sort();
        for k in here {
            let t = if row == 1 {
                "Monster".to_string()
            } else if row == treasure_row {
                "Treasure".to_string()
            } else if row == rows {
                "RestSite".to_string()
            } else {
                let mut banned: HashSet<&str> = HashSet::new();
                for p in parents.get(&k).map(|p| p.as_slice()).unwrap_or(&[]) {
                    if let Some(t) = types.get(p) {
                        if matches!(t.as_str(), "Elite" | "Shop" | "RestSite") {
                            banned.insert(t.as_str());
                        }
                    }
                }
                if row < elite_from {
                    banned.insert("Elite");
                    banned.insert("RestSite");
                }
                if row == rows - 1 {
                    banned.insert("RestSite");
                }
                let pool: Vec<(String, f64)> = weights.iter().filter(|(t, _)| !banned.contains(t.as_str())).map(|(t, w)| (t.clone(), *w)).collect();
                _draw(rng, &pool)
            };
            types.insert(k, t);
        }
    }
    let mut nodes: Nodes = edges
        .iter()
        .map(|(k, kids)| (*k, json!({"col": k.0, "row": k.1, "type": types[k], "children": kids.iter().map(|c| json!([c.0, c.1])).collect::<Vec<_>>()})))
        .collect();
    nodes.insert(boss, json!({"col": boss.0, "row": boss.1, "type": "Boss", "children": []}));
    nodes
}

/// A key of `weights` drawn by weight.
pub fn _draw<K: Clone>(rng: &mut Random, weights: &[(K, f64)]) -> K {
    let mut r = rng.random() * py::fsum(weights.iter().map(|(_, w)| *w));
    for (k, w) in weights {
        r -= w;
        if r <= 0.0 {
            return k.clone();
        }
    }
    weights.first().expect("something to draw from").0.clone()
}

pub fn rarity(rng: &mut Random, kind: &str, odds: &Odds) -> String {
    let mut r = rng.random();
    for (name, p) in &odds[kind] {
        if r < *p {
            return name.clone();
        }
        r -= p;
    }
    odds[kind].last().expect("odds").0.clone()
}

/// n different cards of a character's pool for a card reward (`offer(run, pool, n, rare_odds, kind, upgrade_chance=0.0)`).
pub fn offer(run: &mut Run, pool: &[Rc<CardDef>], n: i64, rare_odds: &Odds, kind: &str, upgrade_chance: f64) -> Vec<Card> {
    let mut out: Vec<Card> = Vec::new();
    for _ in 0..n {
        let want = if kind == "boss" { "Rare".to_string() } else { rarity(&mut run.rng, kind, rare_odds) };
        let mut choices: Vec<&Rc<CardDef>> = pool.iter().filter(|d| d.rarity == want && !out.iter().any(|c| c.key() == d.key)).collect();
        if choices.is_empty() {
            choices = pool.iter().filter(|d| !out.iter().any(|c| c.key() == d.key)).collect();
        }
        let d = *run.rng.choice(&choices);
        let up = want != "Rare" && run.rng.random() < upgrade_chance;
        out.push(Card::new(d, up));
    }
    out
}

/// One of these cards at random into the hand, free to play this turn.
pub fn free_card(f: &mut Fight, choices: &[Rc<CardDef>]) {
    if !choices.is_empty() {
        let mut c = Card::new(f.run.rng.choice(choices), false);
        c.cost_turn = Some(0);
        f.add_card(c, "hand");
    }
}

/// Attack, Skill and Power Potions: a card of that type of the character's; the others: a colorless card.
pub fn potion_card(f: &mut Fight, pot: &Potion) {
    let kind = match pot.key.as_str() {
        "ATTACK_POTION" => Some("Attack"),
        "SKILL_POTION" => Some("Skill"),
        "POWER_POTION" => Some("Power"),
        _ => None,
    };
    let from = if kind.is_some() { (f.game.pool)(&f.run.character) } else { (f.game.colorless)() };
    let choices: Vec<Rc<CardDef>> = from.iter().filter(|d| kind.is_none_or(|k| d.r#type == k)).cloned().collect();
    free_card(f, &choices);
}

/// Upgrade n cards of the deck at random among those `test` lets through (`upgrade_random(run, test, n=2)`).
pub fn upgrade_random(run: &mut Run, test: impl Fn(&Card) -> bool, n: usize) {
    let fresh: Vec<usize> = (0..run.deck.len()).filter(|i| !run.deck[*i].up && test(&run.deck[*i])).collect();
    let k = n.min(fresh.len());
    for i in run.rng.sample(&fresh, k) {
        run.deck[i].up = true;
    }
}

// ------------------------------------------------------------------ events
//
// An event here is a set of options written the way the games word them, so that the bot's event handler
// (crate::run::events) can price them, each with what it does. Gold, HP and the odds scale a little by act.

pub fn _transform(run: &mut Run) {
    if let Some(c) = run.choose_from_deck("transform", "Choose a card to Transform.", None) {
        run.remove_card(&c);
        run.log_add("removals", -1);
        let pool = (run.game.pool)(&run.character);
        let d = run.rng.choice(&pool).clone();
        run.deck.push(Card::new(&d, false));
    }
}

/// What an option of an event does to (run, act).
pub type Effect = fn(&mut Run, i64);

fn _relic(r: &mut Run) {
    let rid = (r.game.random_relic)(r, false);
    r.gain_relic(&rid);
}

fn _reward(r: &mut Run, kind: &str) {
    let offered = (r.game.card_choices)(r, kind);
    r.card_reward(offered, true);
}

/// (weight, [(title, description, effect(run, act))...]): a "Leave" option is always added
#[allow(non_snake_case)]
pub fn EVENTS() -> Vec<(i64, Vec<(&'static str, &'static str, Effect)>)> {
    vec![
        (3, vec![("Take", "Lose {hp} HP. Obtain a random Relic.", |r, a| {
            r.lose_hp(r.max_hp / 8 + 2 * a);
            _relic(r);
        })]),
        (3, vec![("Gold", "Gain {gold} Gold.", |r, a| r.gold = r.gold + 50 + 25 * a)]),
        (3, vec![
            ("Purify", "Remove a card from your Deck.", |r, _a| {
                r.remove_one();
            }),
            ("Upgrade", "Upgrade a card.", |r, _a| {
                r.upgrade_one();
            }),
        ]),
        (2, vec![("Trade", "Lose {hp} HP. Gain {gold2} Gold.", |r, a| {
            r.lose_hp(r.max_hp / 10 + a);
            r.gold = r.gold + 75 + 25 * a;
        })]),
        (2, vec![("Pray", "Become Cursed - Regret. Obtain a random Relic.", |r, _a| {
            let curse = (r.game.random_curse)(r);
            r.deck.push(curse);
            _relic(r);
        })]),
        (2, vec![
            ("Rest", "Heal {heal} HP.", |r, _a| r.hp = r.max_hp.min(r.hp + r.max_hp / 3)),
            ("Grow", "Gain 5 Max HP.", |r, _a| {
                r.max_hp += 5;
                r.hp += 5;
            }),
        ]),
        (3, vec![("Learn", "Choose a card to add to your Deck.", |r, _a| _reward(r, "monster"))]),
        (1, vec![("Study", "Lose {mhp} Max HP. Choose a Rare card to add to your Deck.", |r, _a| {
            r.max_hp -= 5;
            r.hp = r.hp.min(r.max_hp);
            _reward(r, "boss");
        })]),
        (2, vec![("Change", "Transform a card.", |r, _a| _transform(r))]),
        (1, vec![("Buy", "Pay {cost} Gold. Obtain a random Relic.", |r, a| {
            r.gold = r.gold - 100 - 25 * a;
            _relic(r);
        })]),
        (2, vec![("Smith", "Upgrade 2 random cards.", |r, _a| upgrade_random(r, |c| !matches!(c.r#type(), "Status" | "Curse"), 2))]),
        (2, vec![]), // nothing worth having (the event is passed by)
    ]
}

/// The bot's choice among the options of an event the model made up (the index of the option).
pub fn event_state(run: &Run, event_id: &str, event_name: &str, options: &[Choice], lone_is_proceed: bool) -> Value {
    let shown: Vec<Value> = options
        .iter()
        .enumerate()
        .map(|(i, (t, d, _f))| json!({"index": i, "title": t, "description": d, "is_locked": false, "is_proceed": lone_is_proceed && options.len() == 1}))
        .collect();
    run.screen("event", ("event", json!({"event_id": event_id, "event_name": event_name, "in_dialogue": false, "options": shown})))
}

pub fn event(run: &mut Run) {
    let act = run.act;
    let events = EVENTS();
    let weights: Vec<(usize, f64)> = events.iter().enumerate().map(|(i, (w, _o))| (i, *w as f64)).collect();
    let (_w, opts) = &events[_draw(&mut run.rng, &weights)];
    let nums = [("{hp}", run.max_hp / 8 + 2 * act), ("{gold}", 50 + 25 * act), ("{gold2}", 75 + 25 * act), ("{heal}", run.max_hp / 3), ("{mhp}", 5), ("{cost}", 100 + 25 * act)];
    let cost = 100 + 25 * act;
    let mut options: Vec<Choice> = opts
        .iter()
        .map(|(t, d, effect)| {
            let mut text = d.to_string();
            for (name, n) in &nums {
                text = text.replace(name, &n.to_string());
            }
            let effect = *effect;
            (t.to_string(), text, Some(Rc::new(move |r: &mut Run, a: i64| effect(r, a)) as Rc<dyn Fn(&mut Run, i64)>))
        })
        .collect();
    if options.iter().any(|(_t, d, _f)| d.contains("Pay")) && run.gold < cost {
        options = Vec::new();
    }
    options.push(("Leave".to_string(), "Leave.".to_string(), None));
    // (A lone option is the event's way out, "is_proceed" in the bot's words; with others beside it, it is one of them.)
    let state = event_state(run, "SIM_EVENT", "Event", &options, true);
    let mut choice = options.len() - 1;
    for (action, params) in strategy::event(&state, &mut run.mem) {
        if action == "choose_event_option" {
            choice = params["index"].as_i64().expect("an index") as usize;
            break;
        }
    }
    let (title, _d, effect) = &options[choice];
    run.digest.push(format!("V{}", title.chars().take(2).collect::<String>()));
    if let Some(effect) = effect {
        effect(run, act);
    }
}

// ------------------------------------------------------------------ shops

/// `shop_stock(run, prices, colorless_mult=1.2)`: `prices` by rarity (Common, Uncommon, Rare).
pub fn shop_stock(run: &mut Run, prices: &[(&str, i64)], colorless_mult: f64) -> Vec<Item> {
    let g = run.game.clone();
    let price_of = |rarity: &str| prices.iter().find(|(r, _)| *r == rarity).unwrap_or_else(|| panic!("KeyError: {rarity}")).1 as f64;
    let mut items: Vec<Item> = Vec::new();
    if run.has("MEAL_TICKET") {
        run.hp = run.max_hp.min(run.hp + 15);
    }

    fn add(items: &mut Vec<Item>, cat: &str, price: f64, fields: Vec<(String, Value)>) -> usize {
        let mut v = Map::new();
        v.insert("index".to_string(), json!(items.len()));
        v.insert("category".to_string(), json!(cat));
        v.insert("price".to_string(), json!(py::trunc(price)));
        v.insert("cost".to_string(), json!(py::trunc(price)));
        for (k, x) in fields {
            v.insert(k, x);
        }
        items.push(Item { v, ..Default::default() });
        items.len() - 1
    }

    fn card_fields(c: &Card) -> Vec<(String, Value)> {
        match c.view(ViewArgs::default()) {
            Value::Object(view) => view.into_iter().map(|(k, v)| (format!("card_{k}"), v)).collect(),
            _ => vec![],
        }
    }

    let own = (g.pool)(&run.character);
    let sale = run.rng.randrange1(5);
    for (n, kind) in ["Attack", "Attack", "Skill", "Skill", "Power"].into_iter().enumerate() {
        let choices: Vec<&Rc<CardDef>> = own.iter().filter(|d| d.r#type == kind && !items.iter().any(|i| i.v.get("card_id").is_some_and(|id| id.s() == d.key))).collect();
        let c = Card::new(*run.rng.choice(&choices), false);
        let price = price_of(&c.d.rarity) * run.rng.uniform(0.9, 1.1) * if n as i64 == sale { 0.5 } else { 1.0 };
        let at = add(&mut items, "card", price, card_fields(&c));
        items[at].card = Some(c);
    }
    let colorless = (g.colorless)();
    for rarity in ["Uncommon", "Rare"] {
        let choices: Vec<&Rc<CardDef>> = colorless.iter().filter(|d| d.rarity == rarity).collect();
        let c = Card::new(*run.rng.choice(&choices), false);
        let price = price_of(rarity) * colorless_mult * run.rng.uniform(0.9, 1.1);
        let at = add(&mut items, "card", price, card_fields(&c));
        items[at].card = Some(c);
    }
    for _ in 0..3 {
        let rid = (g.random_relic)(run, false);
        if items.iter().any(|i| i.v.get("relic_id").is_some_and(|id| id.s() == rid)) {
            continue;
        }
        let tier = if g.COMMON.contains(&rid) {
            150
        } else if g.UNCOMMON.contains(&rid) {
            250
        } else {
            300
        };
        let price = tier as f64 * run.rng.uniform(0.95, 1.05);
        let fields = vec![("relic_id".to_string(), json!(rid)), ("relic_name".to_string(), json!(py::title(&rid.replace('_', " ")))), ("description".to_string(), json!((g.relic_text)(&rid)))];
        let at = add(&mut items, "relic", price, fields);
        items[at].relic = Some(rid);
    }
    for _ in 0..3 {
        let p = (g.random_potion)(run);
        let price = *run.rng.choice(&[50, 75, 100]) as f64 * run.rng.uniform(0.95, 1.05);
        let fields = vec![("potion_id".to_string(), json!(p.key)), ("potion_name".to_string(), json!(p.name)), ("description".to_string(), json!(p.text))];
        let at = add(&mut items, "potion", price, fields);
        items[at].potion = Some(p);
    }
    let removal = if run.has("SMILING_MASK") { 50 } else { 75 + 25 * run.counters.val("removals") };
    add(&mut items, "card_removal", removal as f64, vec![]);
    items
}
