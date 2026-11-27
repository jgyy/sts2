//! Powers that pay every turn of a long fight, at a card reward: energy (decks.ENERGY_POWER; scripts/demonfight.rs cards)
//! and damage or Doom (decks.SCALE_POWER).

use serde_json::{json, Value};
use sts2bot::cards::deck_value;
use sts2bot::learn;
use sts2bot::run::decks;
use sts2bot::run::strategy::{self, Memory};

use crate::fake_game::with;

/// (The Python's default: rarity "Common".)
fn c(cid: &str, name: &str, ctype: &str, cost: i64, text: &str, rarity: &str) -> Value {
    json!({"id": cid, "name": name, "type": ctype, "cost": cost.to_string(), "rarity": rarity, "description": text})
}

const E: &str = "[necrobinder_energy_icon.png]";

fn STRIKE() -> Value {
    c("STRIKE_NECROBINDER", "Strike", "Attack", 1, "Deal 6 damage.", "Basic")
}
fn DEFEND() -> Value {
    c("DEFEND_NECROBINDER", "Defend", "Skill", 1, "Gain 5 Block.", "Basic")
}
fn BODYGUARD() -> Value {
    c("BODYGUARD", "Bodyguard", "Skill", 1, "Summon 5.", "Basic")
}
fn UNLEASH() -> Value {
    c("UNLEASH", "Unleash", "Attack", 1, "Osty deals 6 damage. Deals additional damage equal to Osty's current HP.", "Basic")
}
fn POKE() -> Value {
    c("POKE", "Poke", "Attack", 0, "Osty deals 6 damage.", "Common")
}
fn REAVE() -> Value {
    c("REAVE", "Reave", "Attack", 1, "Deal 10 damage. Add a Soul into your Draw Pile.", "Common")
}
fn GRAVE_WARDEN() -> Value {
    c("GRAVE_WARDEN", "Grave Warden", "Skill", 1, "Gain 8 Block. Add a Soul into your Draw Pile.", "Common")
}
fn SCOURGE() -> Value {
    c("SCOURGE", "Scourge", "Skill", 1, "Apply 13 Doom. Draw 1 card.", "Common")
}
fn NEGATIVE_PULSE() -> Value {
    c("NEGATIVE_PULSE", "Negative Pulse", "Skill", 1, "Gain 5 Block. Apply 7 Doom to ALL enemies.", "Common")
}
fn BORROWED_TIME() -> Value {
    c("BORROWED_TIME", "Borrowed Time", "Skill", 1, &format!("Gain 4{E}. Cards cost an additional {E} this turn."), "Uncommon")
}
fn FRIENDSHIP() -> Value {
    c("FRIENDSHIP", "Friendship", "Power", 1, &format!("Lose 2 Strength. Gain {E} at the start of each turn."), "Uncommon")
}
fn DEMESNE() -> Value {
    c("DEMESNE", "Demesne", "Power", 3, &format!("Ethereal. At the start of your turn, gain {E} and draw 1 additional card."), "Rare")
}
fn PYRE() -> Value {
    c("PYRE", "Pyre", "Power", 2, "Gain [ironclad_energy_icon.png] at the start of each turn.", "Rare")
}
fn ORBIT() -> Value {
    c("ORBIT", "Orbit", "Power", 2, "Every 4[regent_energy_icon.png] you spend, gain [regent_energy_icon.png].", "Uncommon")
}
fn AUTOMATION() -> Value {
    c("AUTOMATION", "Automation", "Power", 1, &format!("Every 10 cards you draw, gain {E}."), "Uncommon")
}
fn NEUROSURGE() -> Value {
    c("NEUROSURGE", "Neurosurge", "Power", 0, &format!("Gain {E}{E}{E}. Draw 2 cards. At the start of your turn, apply 3 Doom to yourself."), "Rare")
}
fn WISP() -> Value {
    c("WISP", "Wisp", "Skill", 0, &format!("Gain {E}. Exhaust."), "Common")
}
fn INFLAME() -> Value {
    c("INFLAME", "Inflame", "Power", 1, "Gain 2 Strength.", "Uncommon")
}
fn COUNTDOWN() -> Value {
    c("COUNTDOWN", "Countdown", "Power", 1, "At the start of your turn, apply 6 Doom to a random enemy.", "Uncommon")
}
fn LETHALITY() -> Value {
    c("LETHALITY", "Lethality", "Power", 1, "Ethereal. The first Attack each turn deals 50% additional damage.", "Uncommon")
}
fn REAPER_FORM() -> Value {
    c("REAPER_FORM", "Reaper Form", "Power", 3, "Whenever Attacks deal damage, apply that much Doom.", "Rare")
}
fn CALCIFY() -> Value {
    c("CALCIFY", "Calcify", "Power", 1, "Osty's attacks deal 4 additional damage.", "Uncommon")
}
fn HAUNT() -> Value {
    c("HAUNT", "Haunt", "Power", 1, "Whenever you play a Soul, a random enemy loses 7 HP.", "Uncommon")
}
fn TIMES_UP() -> Value {
    c("TIMES_UP", "Time's Up", "Attack", 2, "Deal damage equal to the enemy's Doom.", "Uncommon")
}
fn BLIGHT_STRIKE() -> Value {
    c("BLIGHT_STRIKE", "Blight Strike", "Attack", 2, "Deal 8 damage. Apply Doom equal to damage dealt.", "Common")
}
fn OBLIVION() -> Value {
    c("OBLIVION", "Oblivion", "Skill", 0, "Whenever you play a card this turn, apply 3 Doom to the enemy.", "Rare")
}
fn SCULPTING_STRIKE() -> Value {
    c("SCULPTING_STRIKE", "Sculpting Strike", "Attack", 1, "Deal 9 damage. Add Ethereal to a card in your Hand.", "Common")
}
fn SHARED_FATE() -> Value {
    c("SHARED_FATE", "Shared Fate", "Skill", 0, "Lose 2 Strength. Enemy loses 2 Strength. Exhaust.", "Rare")
}
fn SOUL_STORM() -> Value {
    c("SOUL_STORM", "Soul Storm", "Attack", 1, "Deal 9 damage. Deals 4 additional damage for each Soul in your Exhaust Pile.", "Rare")
}
fn DEATHS_DOOR() -> Value {
    c("DEATHS_DOOR", "Death's Door", "Skill", 1, "Gain 6 Block. If you applied Doom this turn, gain Block 2 additional times.", "Uncommon")
}
fn PARSE() -> Value {
    c("PARSE", "Parse", "Skill", 1, "Ethereal. Draw 3 cards.", "Uncommon")
}

fn STARTER() -> Vec<Value> {
    [vec![STRIKE(); 4], vec![DEFEND(); 4], vec![BODYGUARD(), UNLEASH()]].concat()
}

/// 13 cards; hits Strike 6 x4, Unleash 6, Reave 10, Poke 6
fn PLAIN() -> Vec<Value> {
    [STARTER(), vec![REAVE(), POKE(), GRAVE_WARDEN()]].concat()
}

/// (one of the two live settings; at it our own table says Friendship 4.5, Grave Warden 6.0)
fn CROWD() -> learn::KnobGuard {
    learn::with_knobs(&[("crowd", 0.3)])
}

fn approx(a: f64, b: f64) {
    assert!((a - b).abs() < 5e-8, "{a} != {b}"); // (assertAlmostEqual: 7 places)
}

/// The card a reward is answered with (None: skipped). (The Python's default: act 1.)
fn chosen(offer: &[Value], deck: &[Value], act: i64) -> Option<String> {
    let mut mem = Memory::new();
    (mem.deck, mem.deck_size) = (deck.to_vec(), deck.len() as i64);
    let cards: Vec<Value> = offer.iter().enumerate().map(|(i, x)| with(x.clone(), json!({"index": i}))).collect();
    let state = json!({"state_type": "card_reward", "run": {"act": act}, "player": {"character": "The Necrobinder"}, "card_reward": {"cards": cards}});
    let first = strategy::card_reward(&state, &mut mem).remove(0);
    if first.0 == "skip_card_reward" {
        None
    } else {
        Some(offer[first.1["card_index"].as_i64().unwrap() as usize]["name"].as_str().unwrap().to_string())
    }
}

fn name(card: &str) -> Option<String> {
    Some(card.to_string())
}

/// STARTER + [REAVE, SCULPTING_STRIKE, POKE, GRAVE_WARDEN, NEGATIVE_PULSE, SCOURGE] * times
fn grown(times: usize) -> Vec<Value> {
    [STARTER(), vec![vec![REAVE(), SCULPTING_STRIKE(), POKE(), GRAVE_WARDEN(), NEGATIVE_PULSE(), SCOURGE()]; times].concat()].concat()
}

// TurnEnergy

#[test]
fn test_what_a_power_gives_every_turn_and_energy_once_is_not_energy_every_turn() {
    for (card, energy) in [(FRIENDSHIP(), 1.0), (DEMESNE(), 1.0), (PYRE(), 1.0), (NEUROSURGE(), 0.0), (WISP(), 0.0), (BORROWED_TIME(), 0.0), (INFLAME(), 0.0), (STRIKE(), 0.0)] {
        assert_eq!(decks::turn_energy(&card), energy, "{}", card["name"]);
    }
    approx(decks::turn_energy(&ORBIT()), 0.75);
    approx(decks::turn_energy(&AUTOMATION()), 0.55);
}

// EnergyAtAReward (under CROWD)

#[test]
fn test_friendship_over_the_cards_that_used_to_beat_it() {
    let _crowd = CROWD();
    // Grave Warden 6.0 and Scourge 7.3 were taken over Friendship 5.0-5.1 in a deck of 16.
    let deck = grown(1);
    assert_eq!(chosen(&[GRAVE_WARDEN(), FRIENDSHIP(), BORROWED_TIME()], &deck, 1), name("Friendship"));
    assert_eq!(chosen(&[SCOURGE(), FRIENDSHIP(), NEGATIVE_PULSE()], &deck, 1), name("Friendship"));
}

#[test]
fn test_demesne_is_no_longer_skipped() {
    let _crowd = CROWD();
    // Seen: "deck 22, need 5.0: Demesne 4.1, Negative Pulse 4.0, Sculpting Strike 2.5" and nothing taken.
    let deck = grown(2);
    assert_eq!(chosen(&[DEMESNE(), NEGATIVE_PULSE(), SCULPTING_STRIKE()], &deck, 2), name("Demesne"));
}

#[test]
fn test_the_second_is_worth_less_and_more_energy_less_again() {
    let _crowd = CROWD();
    let first = decks::deck_fit(&DEMESNE(), &PLAIN(), Some(2), 3);
    approx(first, decks::ENERGY_POWER);
    approx(decks::deck_fit(&DEMESNE(), &[PLAIN(), vec![PYRE()]].concat(), Some(2), 3), first / 2.0);
    approx(decks::deck_fit(&DEMESNE(), &PLAIN(), Some(2), 4), first / 2.0); // a relic's energy counts too
}

#[test]
fn test_friendship_costs_more_where_every_hit_takes_strength() {
    let _crowd = CROWD();
    let osty = [STARTER()[4..].to_vec(), vec![POKE(); 4]].concat(); // Defends, Bodyguard, Unleash and Pokes: nothing that takes Strength
    let hitters = [STARTER(), vec![REAVE(); 4]].concat();
    {
        let _hitters = decks::with_hitters_hook(|_| 99); // (the lean to attacks is not what is tested)
        assert!(decks::deck_fit(&FRIENDSHIP(), &osty, Some(2), 3) > decks::deck_fit(&FRIENDSHIP(), &hitters, Some(2), 3) + 1.0);
    }
    assert!(decks::deck_fit(&FRIENDSHIP(), &[hitters, vec![FRIENDSHIP()]].concat(), Some(2), 3) < 0.0); // a second one: less Strength still
}

// TurnDamage

#[test]
fn test_what_a_power_adds_every_turn() {
    assert_eq!(decks::turn_damage(&COUNTDOWN(), &PLAIN()), 6.0);
    let nine = COUNTDOWN()["description"].as_str().unwrap().replace('6', "9");
    assert_eq!(decks::turn_damage(&with(COUNTDOWN(), json!({"description": nine})), &PLAIN()), 9.0);
    approx(decks::turn_damage(&LETHALITY(), &PLAIN()), 0.5 * 46.0 / 7.0); // half the deck's average hit
    approx(decks::turn_damage(&REAPER_FORM(), &PLAIN()), 4.0 * 46.0 / 13.0); // four plays a turn of what the deck hits for
    approx(decks::turn_damage(&CALCIFY(), &PLAIN()), 4.0 * 4.0 * 2.0 / 13.0); // Unleash and Poke
    approx(decks::turn_damage(&HAUNT(), &PLAIN()), 7.0 * 4.0 * 2.0 / 13.0); // Reave and Grave Warden make a Soul each
    for card in [NEUROSURGE(), INFLAME(), FRIENDSHIP(), DEMESNE(), SCOURGE(), REAVE()] {
        // nothing for what deals no damage every turn (Neurosurge's Doom is on us)
        assert_eq!(decks::turn_damage(&card, &STARTER()), 0.0, "{}", card["name"]);
    }
}

#[test]
fn test_capped() {
    let heavy = vec![c("BIG", "Big", "Attack", 1, "Deal 40 damage.", "Common"); 10];
    approx(decks::deck_fit(&REAPER_FORM(), &heavy, Some(2), 3) - decks::deck_fit(&with(REAPER_FORM(), json!({"description": "Nothing."})), &heavy, Some(2), 3), decks::SCALE_CAP);
}

// DamageAtAReward (under CROWD)

/// STARTER + [REAVE, SCULPTING_STRIKE, POKE, GRAVE_WARDEN, NEGATIVE_PULSE, SCOURGE, REAVE, SCULPTING_STRIKE]
fn DECK() -> Vec<Value> {
    [grown(1), vec![REAVE(), SCULPTING_STRIKE()]].concat()
}

#[test]
fn test_reaper_form_over_the_cards_that_beat_it() {
    let _crowd = CROWD();
    // Seen: "Shared Fate 4.3, Soul Storm 3.9, Reaper Form 3.7" and "Soul Storm 7.1, Undeath 5.1, Reaper Form 4.9".
    assert_eq!(chosen(&[SHARED_FATE(), SOUL_STORM(), REAPER_FORM()], &DECK(), 2), name("Reaper Form"));
}

#[test]
fn test_countdown_is_no_longer_skipped() {
    let _crowd = CROWD();
    // Seen: "deck 24, need 4.5: Death's Door 4.4, Parse 3.7, Countdown 3.6" and nothing taken.
    let deck = [DECK(), vec![vec![DEFEND(), STRIKE()]; 4].concat(), vec![REAVE(); 2]].concat();
    assert_eq!(chosen(&[DEATHS_DOOR(), PARSE(), COUNTDOWN()], &deck, 2), name("Countdown"));
}

// TimesUpNeedsDoom (under CROWD)
// Time's Up deals the Doom an enemy already has: in 20 of 63 fights the Necrobinder held it, the deck had no Doom card.

#[test]
fn test_what_puts_doom_on_an_enemy() {
    let _crowd = CROWD();
    for card in [SCOURGE(), COUNTDOWN(), REAPER_FORM(), BLIGHT_STRIKE(), NEGATIVE_PULSE(), OBLIVION()] {
        assert!(decks::makes_doom(&card), "{}", card["name"]);
    }
    for card in [NEUROSURGE(), STRIKE(), REAVE(), TIMES_UP()] {
        // (Neurosurge's 3 Doom is on the Necrobinder)
        assert!(!decks::makes_doom(&card), "{}", card["name"]);
    }
}

#[test]
fn test_no_doom_in_the_deck_means_no_value_and_a_doom_source_keeps_its_rating() {
    let _crowd = CROWD();
    for card in [TIMES_UP(), with(TIMES_UP(), json!({"id": "TIME_S_UP"}))] {
        approx(deck_value(&card) + decks::deck_fit(&card, &PLAIN(), Some(2), 3), 0.0);
    }
    for source in [OBLIVION(), SCOURGE()] {
        assert!(deck_value(&TIMES_UP()) + decks::deck_fit(&TIMES_UP(), &[PLAIN(), vec![source.clone()]].concat(), Some(2), 3) > 1.0, "{}", source["name"]);
    }
}
