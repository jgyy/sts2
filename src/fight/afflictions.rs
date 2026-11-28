//! The Knowledge Demon's offers: which affliction to carry for the rest of the fight, a limit (Mind Rot, Sloth, Waste Away) or a burn
//! (Disintegration). Which costs more depends on the deck: priced by the cards played each turn so far, and the energy left unspent.

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use serde_json::{json, Value};

use super::battle::power_amount;
use super::{combat, triggers};
use crate::cards::{_card_key, parse};
use crate::logging::Logger;
use crate::base::{self, JsonExt};

static LOG: Logger = Logger::new("sts2bot.afflictions");

/// HP a turn that one card a turn we may not play comes to (the fight is that much longer)
pub const CARD_LOST: f64 = 7.5;
/// cards a turn Sloth has to cost before the burn is the cheaper price (perfect play: scripts/demon/offers.rs)
pub const SLOTH_MARGIN: f64 = 2.0;
/// share of recent turns that ended with energy unspent, from which an energy less costs next to nothing
pub const IDLE_ENOUGH: f64 = 0.5;

/// Cards a turn that three a turn would have stopped, over the turns after the first (a turn counts for three at most).
/// (`turns`: `[cards played, energy left]` for each turn.)
pub fn lost_to_sloth(turns: &[Value]) -> f64 {
    let later: Vec<i64> = turns.iter().skip(1).map(|t| t[0].int()).collect();
    if later.len() >= 2 {
        later.iter().map(|&n| 3.min(0.max(n - 3))).sum::<i64>() as f64 / later.len() as f64
    } else {
        0.0
    }
}

/// Share of the last four turns that ended with energy we could not spend (None: too few turns seen).
pub fn idle_share(turns: &[Value]) -> Option<f64> {
    let recent: Vec<i64> = turns[turns.len().saturating_sub(4)..].iter().map(|t| t[1].int()).collect();
    if recent.len() >= 2 {
        Some(recent.iter().filter(|&&left| left >= 1).count() as f64 / recent.len() as f64)
    } else {
        None
    }
}

/// `[c] + [o for o in ordered if o is not c]`.
fn first(ordered: &[Value], c: usize) -> Vec<Value> {
    std::iter::once(&ordered[c]).chain(ordered.iter().enumerate().filter(|(n, _)| *n != c).map(|(_, o)| o)).cloned().collect()
}

/// `ordered` with the affliction to take first. `state["_turns"]` is (cards played, energy left) for each turn so far.
pub fn order(ordered: &[Value], state: &Value) -> Vec<Value> {
    let mut keys: IndexMap<String, usize> = IndexMap::new();
    for (n, c) in ordered.iter().enumerate() {
        keys.insert(_card_key(c), n);
    }
    let (burn, drain, sloth) = (keys.get("DISINTEGRATION").copied(), keys.get("WASTE_AWAY").copied(), keys.get("SLOTH").copied());
    let Some(burn) = burn else { return ordered.to_vec() };
    if drain.is_none() && sloth.is_none() {
        return ordered.to_vec();
    }
    // (The choice screen comes without the enemies: no rule here can ask for the Demon's HP.)
    let player = &state["player"];
    let hp = player["hp"].int();
    let turns = state["_turns"].list();
    let per_turn = match crate::re!(r"(?i)take (\d+) damage").captures(&ordered[burn]["description"].text()) {
        Some(m) => m[1].parse::<i64>().unwrap_or(0),
        None => 8,
    };
    let burning = 0.max(power_amount(player, "DISINTEGRATION")); // a burn taken earlier: the new one comes on top
    if let Some(sloth) = sloth {
        // The burn only for a deck that has been playing well over three cards a turn, with the HP to pay for it.
        let lost = lost_to_sloth(turns);
        let take_burn = lost >= SLOTH_MARGIN && CARD_LOST * lost > per_turn as f64 && hp > 5 * per_turn && burning == 0;
        crate::log_debug!(
            LOG,
            "affliction: cards a turn {}, Sloth would stop {:.2} a turn: {}",
            base::repr_of(&Value::Array(turns.iter().map(|t| t[0].clone()).collect())),
            lost,
            if take_burn { "the burn" } else { "Sloth" }
        );
        return first(ordered, if take_burn { burn } else { sloth });
    }
    // An energy less against the burn: a deck that spends all its energy takes the burn, unless it is a second one or would finish us in two turns.
    let idle = idle_share(turns);
    let spare = burning > 0 || idle.is_some_and(|idle| idle >= IDLE_ENOUGH);
    let take_burn = hp > 2 * (per_turn + burning) && !spare;
    crate::log_debug!(
        LOG,
        "affliction: energy left at the end of each turn {}: {}",
        base::repr_of(&Value::Array(turns.iter().map(|t| t[1].clone()).collect())),
        if take_burn { "the burn" } else { "Waste Away" }
    );
    first(ordered, if take_burn { burn } else { drain.expect("Waste Away is on offer") })
}

// Cards a fight offers ("Choose 1 of 3 ... It's free to play this turn."): ranked by what each does for this turn's plan.
/// The target a card offered on this screen would have in the hand (the screen does not say).
pub fn _aimed(card: &Value) -> &'static str {
    let text = card["description"].text();
    if crate::re!(r"(?i)\ball\s+enemies\b").is_match(&text) {
        return "AllEnemies";
    }
    if crate::re!(r"(?i)random\s+enemy").is_match(&text) {
        return "RandomEnemy";
    }
    if base::str_of(&card["type"]) == "Attack" || crate::re!(r"(?i)\bapply\b|\benemy\b").is_match(&text) {
        "AnyEnemy"
    } else {
        "Self"
    }
}

/// What tells one offer screen from the next: the names on offer, the hand's, the energy and the round.
pub type Seen = (Vec<String>, Vec<String>, Value, Value);

thread_local! {
    /// the last offer screen ranked, and the card put first
    pub static _OFFER: RefCell<(Option<Seen>, usize)> = const { RefCell::new((None, 0)) };
    static OFFERED: RefCell<Option<Rc<dyn Fn(&[Value], &Value) -> Vec<Value>>>> = const { RefCell::new(None) };
}

/// `afflictions._OFFER[:] = [None, 0]`: forget the last offer screen ranked.
pub fn _OFFER_reset() {
    _OFFER.with(|o| *o.borrow_mut() = (None, 0));
}

/// planner points by which an offered card has to beat the one the ratings put first
pub const OFFER_MARGIN: f64 = 5.0;

/// A card offered here that the planner can read.
pub fn _read(card: &Value) -> bool {
    let mut aimed = card.clone();
    aimed["target_type"] = json!(_aimed(card));
    let info = parse(&aimed);
    info.known || info.r#type == "Power" || triggers::READ.contains(&info.key.as_str())
}

/// A card that may not be playable at all this turn ("Can only be played if ...").
pub fn _iffy(card: &Value) -> bool {
    crate::re!(r"(?i)can\s+only\s+be\s+played").is_match(&card["description"].text())
}

/// Puts the earlier `offered` back when dropped.
#[must_use = "the stand-in ends when this is dropped"]
pub struct OfferedGuard(Option<Rc<dyn Fn(&[Value], &Value) -> Vec<Value>>>);

impl Drop for OfferedGuard {
    fn drop(&mut self) {
        OFFERED.with(|h| *h.borrow_mut() = self.0.take());
    }
}

/// `mock.patch.object(afflictions, "offered", ...)`: `f` answers every `afflictions::offered` on this thread until the guard is dropped.
/// A spy (`wraps=afflictions.offered`) notes its arguments and calls `afflictions::_offered`, the ranking itself.
pub fn with_offered_hook(f: impl Fn(&[Value], &Value) -> Vec<Value> + 'static) -> OfferedGuard {
    OfferedGuard(OFFERED.with(|h| h.borrow_mut().replace(Rc::new(f))))
}

/// `ordered` with the card this turn makes most of first, by the planner's best line with it free in the hand (`fight`: the state before;
/// null for none). (Answered by a test's stand-in while one is in place: `with_offered_hook`.)
pub fn offered(ordered: &[Value], fight: &Value) -> Vec<Value> {
    match OFFERED.with(|h| h.borrow().clone()) {
        Some(hook) => hook(ordered, fight),
        None => _offered(ordered, fight),
    }
}

/// `offered` itself, whatever stands in for it.
pub fn _offered(ordered: &[Value], fight: &Value) -> Vec<Value> {
    if !fight.truthy() || ordered.len() < 2 || ordered.iter().any(|c| !["Attack", "Skill", "Power"].contains(&base::str_of(&c["type"]).as_str())) {
        return ordered.to_vec();
    }
    let player = &fight["player"];
    let hand = player["hand"].list();
    let seen: Seen = (
        ordered.iter().map(|c| base::str_of(&c["name"])).collect(),
        hand.iter().map(|c| base::str_of(&c["name"])).collect(),
        player["energy"].clone(),
        fight["battle"]["round"].clone(),
    );
    let last = _OFFER.with(|o| {
        let o = o.borrow();
        (o.0.as_ref() == Some(&seen)).then_some(o.1)
    });
    if let Some(last) = last {
        // (the screen is asked about on every poll while it is open)
        return first(ordered, last);
    }
    let mut scores: Vec<f64> = Vec::new();
    for c in ordered {
        let mut plan = combat::Plan::default();
        let mut free = if c.is_object() { c.clone() } else { json!({}) };
        free["index"] = json!(hand.len());
        free["cost"] = json!("0");
        free["target_type"] = json!(_aimed(c));
        free["can_play"] = json!(true);
        free["unplayable_reason"] = Value::Null;
        let mut with_it = if player.is_object() { player.clone() } else { json!({}) };
        let mut cards = hand.to_vec();
        cards.push(free);
        with_it["hand"] = Value::Array(cards);
        let mut state = fight.clone();
        state["player"] = with_it;
        combat::choose_card(&state, Some(&mut plan));
        scores.push(plan.score.unwrap_or(f64::NEG_INFINITY));
    }
    // Only a card the planner can read and surely play, by a clear margin: the rating behind the order also counts later draws of it.
    let fit: Vec<usize> = (0..ordered.len()).filter(|&n| _read(&ordered[n]) && !_iffy(&ordered[n])).collect();
    // (nothing to set an unread card against)
    let top = if _iffy(&ordered[0]) {
        f64::NEG_INFINITY
    } else if _read(&ordered[0]) {
        scores[0]
    } else {
        f64::INFINITY
    };
    let best = base::max_by_key(fit, |&n| (scores[n], -(n as i64))).unwrap_or(0);
    let best = if scores[best] >= top + OFFER_MARGIN { best } else { 0 };
    _OFFER.with(|o| *o.borrow_mut() = (Some(seen), best));
    crate::log_debug!(LOG, "offer: {}", ordered.iter().zip(&scores).map(|(c, s)| format!("{} {:.1}", base::str_of(&c["name"]), s)).collect::<Vec<_>>().join(", "));
    first(ordered, best)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fight::combat::testing::{rows, Golden};
    use std::cell::Cell;

    fn golden(text: &str) -> Golden {
        Golden {
            atoms: rows(&[text], "atoms"),
            cards: Vec::new(),
            knobs: rows(&[text], "knobs").iter().map(|k| k.entries().map(|(name, x)| (name.clone(), x.float())).collect()).collect(),
        }
    }

    /// What this module logged, as the cases have it: `[level, message]`.
    fn lines(records: &[crate::logging::Record]) -> Value {
        records.iter().filter(|r| r.logger == "sts2bot.afflictions").map(|r| json!([r.level.name(), r.message])).collect()
    }

    fn picked(ordered: &[Value], places: &Value) -> Vec<Value> {
        places.list().iter().map(|i| ordered[i.int() as usize].clone()).collect()
    }

    fn offer_json() -> Value {
        _OFFER.with(|o| {
            let o = o.borrow();
            json!([o.0.as_ref().map(|s| json!([s.0, s.1, s.2, s.3])), o.1])
        })
    }

    #[test]
    fn golden_turns_and_order() {
        let text = include_str!("../../tests/golden/fight2/small.json");
        let g = golden(text);
        let (lost, idle, order_cases) = (rows(&[text], "lost"), rows(&[text], "idle"), rows(&[text], "order"));
        assert!(lost.len() >= 100 && idle.len() >= 90 && order_cases.len() >= 400);
        for rec in &lost {
            assert_eq!(json!(lost_to_sloth(rec["turns"].list())), rec["r"], "lost_to_sloth {}", rec["turns"]);
        }
        for rec in &idle {
            assert_eq!(json!(idle_share(rec["turns"].list())), rec["r"], "idle_share {}", rec["turns"]);
        }
        let mut moved = 0;
        for (n, rec) in order_cases.iter().enumerate() {
            let _env = g.env(&rec["env"]);
            let ordered = rec["ordered"].list();
            let (got, logged) = crate::logging::capture(|| order(ordered, &rec["s"]));
            assert_eq!(got, picked(ordered, &rec["r"]), "order case {n}: {rec}");
            assert_eq!(lines(&logged), rec["log"], "order case {n}");
            moved += (rec["r"][0].int() != 0) as usize;
        }
        assert!(moved > 150, "{moved} offers reordered");
    }

    #[test]
    fn golden_cards() {
        let cases = rows(&[include_str!("../../tests/golden/fight2/cards0.json"), include_str!("../../tests/golden/fight2/cards1.json")], "card");
        assert!(cases.len() >= 6000);
        let mut bad = Vec::new();
        for (n, rec) in cases.iter().enumerate() {
            let _game = crate::games::playing(rec["g"].s());
            let c = &rec["c"];
            if json!([_aimed(c), _read(c), _iffy(c)]) != json!([rec["aimed"], rec["read"], rec["iffy"]]) {
                bad.push(n);
            }
        }
        assert!(bad.is_empty(), "{} of {} cards differ: {:?}", bad.len(), cases.len(), &bad[..bad.len().min(20)]);
    }

    #[test]
    fn golden_offered() {
        let texts = [include_str!("../../tests/golden/fight2/offered0.json"), include_str!("../../tests/golden/fight2/offered1.json")];
        let (mut total, mut bad, mut moved) = (0, Vec::new(), 0);
        for text in texts {
            let g = golden(text);
            for (n, rec) in rows(&[text], "offered").iter().enumerate() {
                total += 1;
                let _env = g.env(&rec["env"]);
                let fight = if rec["fight"].is_object() { g.state(&rec["fight"]) } else { rec["fight"].clone() };
                let ordered = rec["ordered"].list();
                let calls = Rc::new(Cell::new(0));
                let spy = calls.clone();
                let _hook = combat::with_choose_card_hook(move |s, plan| {
                    spy.set(spy.get() + 1);
                    combat::_choose_card(s, plan)
                });
                _OFFER_reset();
                let (got, logged) = crate::logging::capture(|| offered(ordered, &fight));
                let mut same = got == picked(ordered, &rec["r"]) && lines(&logged) == rec["log"] && calls.get() == rec["calls"].int() && offer_json() == rec["offer"];
                // the same screen asked about again
                let (again, logged2) = crate::logging::capture(|| offered(ordered, &fight));
                same = same && again == picked(ordered, &rec["r2"]) && lines(&logged2) == rec["log2"] && (rec["offer"][0].is_null() || calls.get() == rec["calls"].int());
                // the same cards the other way round: another screen
                let back: Vec<Value> = ordered.iter().rev().cloned().collect();
                same = same && offered(&back, &fight) == picked(ordered, &rec["r3"]) && offer_json() == rec["offer3"];
                if !same {
                    if bad.len() < 5 {
                        eprintln!(
                            "case {n}: got {:?} want {} in {} plans, want {}\n  log {} want {}\n  offer {} want {}",
                            got.iter().map(|c| c["name"].s()).collect::<Vec<_>>(),
                            rec["r"],
                            calls.get(),
                            rec["calls"],
                            lines(&logged),
                            rec["log"],
                            offer_json(),
                            rec["offer3"]
                        );
                    }
                    bad.push(n);
                }
                moved += (rec["r"][0].int() != 0) as usize;
            }
        }
        assert!(total >= 500, "{total} cases");
        assert!(bad.is_empty(), "{} of {total} cases differ: {:?}", bad.len(), &bad[..bad.len().min(40)]);
        assert!(moved > 100, "{moved} offers reordered");
    }

    #[test]
    fn a_stand_in_answers_offered() {
        let seen = Rc::new(Cell::new(0));
        let spy = seen.clone();
        let cards = [json!({"name": "A", "type": "Skill"}), json!({"name": "B", "type": "Skill"})];
        {
            let _hook = with_offered_hook(move |ordered, fight| {
                spy.set(spy.get() + 1 + fight.is_null() as i32 * 10);
                _offered(ordered, fight)
            });
            assert_eq!(offered(&cards, &Value::Null), cards);
        }
        assert_eq!(offered(&cards, &json!({})), cards);
        assert_eq!(seen.get(), 11);
        assert_eq!((CARD_LOST, SLOTH_MARGIN, IDLE_ENOUGH, OFFER_MARGIN), (7.5, 2.0, 0.5, 5.0));
    }
}
