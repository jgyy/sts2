//! Cards that pay off only on what other cards make (Poison, Shivs, Souls, cards Exhausted), and the cards that make it.

use serde_json::{json, Value};
use sts2bot::cards::deck_value;
use sts2bot::run::{decks, strategy};

use crate::helpers::approx;

fn silent(cid: &str, name: &str, ctype: &str, cost: i64, text: &str) -> Value {
    json!({"id": cid, "name": name, "type": ctype, "cost": cost, "rarity": "Uncommon", "description": text})
}

// Payoffs

#[test]
fn test_a_payoff_is_worth_little_until_the_deck_holds_what_feeds_it() {
    let bubble = silent("BUBBLE_BUBBLE", "Bubble Bubble", "Skill", 1, "If the enemy has Poison, apply 9 Poison.");
    let accuracy = silent("ACCURACY", "Accuracy", "Power", 1, "Shivs deal 4 additional damage.");
    let poison = silent("DEADLY_POISON", "Deadly Poison", "Skill", 1, "Apply 5 Poison.");
    let fumes = silent("NOXIOUS_FUMES", "Noxious Fumes", "Power", 1, "At the start of your turn, apply 2 Poison to ALL enemies.");
    let dance = silent("BLADE_DANCE", "Blade Dance", "Skill", 1, "Add 3 Shivs into your Hand. Exhaust.");
    let mut starter = vec![json!({"id": "STRIKE_SILENT", "name": "Strike", "type": "Attack", "cost": 1, "description": "Deal 6 damage."}); 5];
    starter.extend(vec![json!({"id": "DEFEND_SILENT", "name": "Defend", "type": "Skill", "cost": 1, "description": "Gain 5 Block."}); 5]);
    let plus = |more: &[&Value]| -> Vec<Value> { starter.iter().cloned().chain(more.iter().map(|c| (*c).clone())).collect() };
    let fit = |c: &Value, deck: &[Value]| strategy::deck_fit(c, deck, Some(2), 3);
    let worth = |c: &Value, deck: &[Value]| deck_value(c) + fit(c, deck);
    // Nothing to feed it: Bubble Bubble does nothing (and its 9 Poison is no hit for the lean to damage).
    assert!(worth(&bubble, &starter) < 0.4 * worth(&bubble, &plus(&[&fumes])));
    assert!(worth(&bubble, &starter) < worth(&bubble, &plus(&[&poison])));
    assert!(worth(&bubble, &plus(&[&poison])) < worth(&bubble, &plus(&[&poison, &poison])));
    for fed in [vec![&fumes], vec![&poison, &poison]] {
        // fed in full: nothing taken off (a Power feeds it every turn, so one is enough)
        let whole = {
            let _off = decks::FEED_KEEP.set(1.0); // (the rule off)
            worth(&bubble, &plus(&fed))
        };
        approx(worth(&bubble, &plus(&fed)), whole);
    }
    assert!(worth(&accuracy, &starter) < 0.4 * worth(&accuracy, &plus(&[&dance, &dance])));
    approx(fit(&accuracy, &plus(&[&dance, &dance])), 0.0); // fed: its rating, no more
    // ... and once one is held, what feeds it is wanted, until there is enough of it.
    assert!(fit(&dance, &plus(&[&accuracy])) > fit(&dance, &starter) + 0.9);
    assert!(fit(&dance, &plus(&[&accuracy])) > fit(&dance, &plus(&[&accuracy, &dance])));
    approx(fit(&poison, &plus(&[&accuracy])), fit(&poison, &starter)); // not Accuracy's
    // A card that does something of its own is no payoff (Soul Storm deals its 9 whatever the Souls).
    let storm = silent("SOUL_STORM", "Soul Storm", "Attack", 1, "Deal 9 damage. Deals 4 additional damage for each Soul in your Exhaust Pile.");
    assert!(decks::pays_off(&storm).is_none());
}
