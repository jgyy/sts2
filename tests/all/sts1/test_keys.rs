//! The first game: the numbers the game works out for a card in hand, its powers in the planner's words, the three keys
//! that open act 4 (games/sts1/keys.rs), and fights the first game's own words once misled the planner in.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use serde_json::{json, Value};
use sts2bot::bot::{Bot, BotArgs};
use sts2bot::cards;
use sts2bot::client::{Game, GameUnavailable};
use sts2bot::fight::bossfight::BossWatch;
use sts2bot::fight::combat::Plan;
use sts2bot::fight::{battle, closing, combat};
use sts2bot::fightrecord::FightWatch;
use sts2bot::games::sts1::client::{Prefs, Sts1Client, Wire};
use sts2bot::games::sts1::screens::Screens;
use sts2bot::games::sts1::{keys, text, tiers};
use sts2bot::logging::{self, Level};
use sts2bot::py::JsonExt;
use sts2bot::run::events::{self, EventOptionValueArgs};
use sts2bot::run::strategy::{self, Memory};
use sts2bot::run::{crowd, decks};

use super::helpers::sts1_case;
use crate::fake_game::with;
use crate::helpers::{combat_state, enemy, DEFEND, STRIKE};

/// (the act's burning elite, on its last row: act 4 still possible)
fn AHEAD() -> Value {
    json!([{"x": 3, "y": 14, "symbol": "E", "emerald_key": true, "children": []}])
}

/// `_game(**kw)`: the keywords as a dict.
fn _game(kw: Value) -> Value {
    with(json!({"act": 3, "floor": 40, "current_hp": 70, "max_hp": 80, "final_act_available": true, "keys": {"ruby": false, "emerald": false, "sapphire": false},
                "map": AHEAD()}), kw)
}

/// Screens reading a message of the act 3 game above, with these fields changed.
fn screens(game: Value) -> Screens {
    let mut s = Screens::new();
    s.raw = Rc::new(json!({"in_game": true, "game_state": _game(game)}));
    s
}

/// `s.raw["game_state"]`, to change.
fn game_state(s: &mut Screens) -> &mut Value {
    &mut Rc::make_mut(&mut s.raw)["game_state"]
}

/// A card in hand, with the numbers the game worked out for it (damage, block and their printed base).
/// (The Python's defaults: cost 1, upgrades 0; `numbers`: its other keywords as a dict.)
fn live(i: i64, cid: &str, ctype: &str, cost: i64, upgrades: i64, numbers: Value) -> Value {
    Screens::new().card(&with(json!({"id": cid, "name": cid, "type": ctype, "cost": cost, "upgrades": upgrades}), numbers), Some(i), true)
}

/// `[x[key] for x in items]`.
fn each<'a>(items: &'a Value, key: &str) -> Vec<&'a Value> {
    items.list().iter().map(|x| &x[key]).collect()
}

fn card_index(action: &Option<sts2bot::Action>) -> i64 {
    action.as_ref().expect("a card to play").1["card_index"].as_i64().expect("KeyError: 'card_index'")
}

// class LiveNumbers

#[test]
fn test_a_hand_card_reads_the_numbers_the_game_worked_out() {
    let _case = sts1_case();
    // Weak and 5 Strength: the printed 6 is not what the Strike deals; Frail and Dexterity move a Defend's Block. Without the
    // mod's numbers the printed text stands, and a Skill's own numbers are not the card's damage (Flame Barrier's 4 back).
    let barrier = json!({"block": 15, "base_block": 12, "damage": -1, "base_damage": -1});
    for (cid, ctype, numbers, says) in [("Strike_R", "ATTACK", json!({"damage": 4, "base_damage": 6}), "Deal 4 damage"), ("Defend_R", "SKILL", json!({"block": 3, "base_block": 5}), "Gain 3 Block"),
                                        ("Strike_R", "ATTACK", json!({}), "Deal 6 damage"), ("Flame Barrier", "SKILL", barrier.clone(), "Gain 15 Block"),
                                        ("Flame Barrier", "SKILL", barrier.clone(), "deal 4 damage back")] {
        let card = live(0, cid, ctype, 1, 0, numbers);
        assert!(card["description"].s().contains(says), "{cid} says={says}: {}", card["description"]);
    }
    let wave = live(0, "Iron Wave", "ATTACK", 1, 1, json!({"damage": 9, "base_damage": 7, "block": 10, "base_block": 7}))["description"].clone();
    assert_eq!(wave, "Gain 10 Block. Deal 9 damage.");
}

#[test]
fn test_heavy_blade_is_one_hit() {
    let _case = sts1_case();
    // Read as "14 damage 5 times" it was a 70-damage card for 2 energy, in hand and at every reward.
    let info = cards::parse(&live(0, "Heavy Blade", "ATTACK", 1, 1, json!({"damage": 24, "base_damage": 14})));
    assert_eq!((info.damage, info.hits), (24, 1));
    let printed = cards::parse(&Screens::new().card(&json!({"id": "Heavy Blade", "name": "Heavy Blade", "type": "ATTACK", "upgrades": 1, "cost": 2}), None, false));
    assert_eq!((printed.damage, printed.hits), (14, 1));
}

// class Tiers

#[test]
fn test_the_first_game_s_cards_have_tiers_of_their_own() {
    let _case = sts1_case();
    assert_eq!(tiers::TIER().keys().filter(|k| !text::CARDS().contains_key(k.as_str())).collect::<Vec<_>>(), Vec::<&String>::new()); // (every name is a card the bot can read)
    assert_eq!(crowd::rating("UNDERHANDED_STRIKE"), Some(tiers::C)); // (by the game's id: Sneaky Strike)
    assert!(crowd::rating("ACROBATICS").unwrap() > crowd::rating("POISONED_STAB").unwrap());
    assert!(crowd::rating("STRIKE_R").is_none());
}

#[test]
fn test_a_card_that_can_only_be_played_sometimes_is_not_free_damage() {
    let _case = sts1_case();
    // Clash topped the first rewards at 9.7: 14 damage for 0 energy on paper, dead beside Defends.
    let clash = Screens::new().card(&json!({"id": "Clash", "name": "Clash", "type": "ATTACK", "rarity": "COMMON", "cost": 0}), None, false);
    let clap = Screens::new().card(&json!({"id": "Thunderclap", "name": "Thunderclap", "type": "ATTACK", "rarity": "COMMON", "cost": 1}), None, false);
    assert!(cards::deck_value(&clash) < cards::deck_value(&clap) + 0.5);
}

// class Powers

#[test]
fn test_first_game_powers_in_the_planner_s_words() {
    let _case = sts1_case();
    let read = Screens::powers(&json!({"powers": [{"id": "Anger", "amount": 2}, {"id": "Hex", "amount": 1}, {"id": "Flight", "amount": 3},
                                                   {"id": "Sharp Hide", "amount": 3}, {"id": "Generic Strength Up Power", "amount": 3}]}));
    assert_eq!(read.iter().map(|p| &p["id"]).collect::<Vec<_>>(), ["ENRAGE_POWER", "HEX_DAZED_POWER", "FLUTTER_POWER", "THORNS_POWER", "RITUAL_POWER"]);
}

#[test]
fn test_a_darkling_comes_back_on_half_its_hp() {
    let _case = sts1_case();
    let read = Screens::powers(&json!({"max_hp": 55, "powers": [{"id": "Life Link", "amount": -1}]}));
    assert_eq!(read.len(), 1);
    let link = &read[0];
    assert_eq!((&link["id"], &link["amount"]), (&json!("REATTACH_POWER"), &json!(27)));
}

// class Keys

#[test]
fn test_recall_in_act_3_with_hp_to_spare_and_no_key_while_act_4_is_shut() {
    let _case = sts1_case();
    assert_eq!(keys::missing(&_game(json!({"final_act_available": false}))), BTreeSet::new());
    // Not before the heal (40 HP), but on the last rest site (floor 49) there is no other chance (once the Emerald is held: nothing
    // but the boss is left on the map after it).
    let emerald = json!({"ruby": false, "emerald": true, "sapphire": false});
    for (game, wanted) in [(json!({"final_act_available": false}), false), (json!({}), true), (json!({"current_hp": 40}), false), (json!({"current_hp": 37, "floor": 49, "keys": emerald}), true),
                           (json!({"current_hp": 35, "floor": 49, "keys": emerald}), false), (json!({"current_hp": 37, "floor": 49}), false), (json!({"act": 2}), false), (json!({"keys": {"ruby": true}}), false)] {
        assert_eq!(keys::wants_ruby(&_game(game.clone())), wanted, "{game}");
    }
}

#[test]
fn test_the_rest_site_shows_recall_only_when_it_is_wanted() {
    let _case = sts1_case();
    let mut s = screens(json!({"screen_type": "REST", "screen_state": {"rest_options": ["rest", "smith", "recall"]}}));
    assert_eq!(each(&s._rest()["rest_site"]["options"], "id"), ["recall"]);
    game_state(&mut s)["current_hp"] = json!(30); // (act 3 and hurt: beside the rest, which the rest site takes first)
    assert_eq!(each(&s._rest()["rest_site"]["options"], "id"), ["rest", "smith", "recall"]);
    game_state(&mut s)["keys"]["ruby"] = json!(true);
    assert_eq!(each(&s._rest()["rest_site"]["options"], "id"), ["rest", "smith"]);
    game_state(&mut s)["keys"] = json!({});
    game_state(&mut s)["final_act_available"] = json!(false);
    assert_eq!(each(&s._rest()["rest_site"]["options"], "id"), ["rest", "smith"]);
}

#[test]
fn test_recall_before_act_3_where_an_upgrade_would_have_been() {
    let _case = sts1_case();
    let opts = || json!({"options": [{"index": 0, "id": "rest"}, {"index": 1, "id": "smith"}, {"index": 2, "id": "recall"}]});
    let hale = json!({"state_type": "rest_site", "run": {"act": 1}, "player": {"hp": 75, "max_hp": 80}, "rest_site": opts()});
    assert_eq!(strategy::rest_site(&hale, &mut Memory::new())[0], ("choose_rest_option".to_string(), json!({"index": 2})));
    let hurt = with(hale, json!({"player": {"hp": 30, "max_hp": 80}}));
    assert_eq!(strategy::rest_site(&hurt, &mut Memory::new())[0], ("choose_rest_option".to_string(), json!({"index": 0})));
}

#[test]
fn test_the_act_3_chest_gives_the_sapphire_key_over_its_relic() {
    let _case = sts1_case();
    let relic = json!({"id": "Anchor", "name": "Anchor"});
    let rewards = json!([{"reward_type": "GOLD", "gold": 30}, {"reward_type": "RELIC", "relic": relic}, {"reward_type": "SAPPHIRE_KEY", "link": relic}]);
    let mut s = screens(json!({"screen_type": "COMBAT_REWARD", "screen_state": {"rewards": rewards}}));
    assert_eq!(each(&s._combat_reward()["rewards"]["items"], "type"), ["gold", "sapphire_key"]);
    game_state(&mut s)["act"] = json!(2);
    assert_eq!(each(&s._combat_reward()["rewards"]["items"], "type"), ["gold", "relic"]);
}

#[test]
fn test_the_burning_elite_is_worth_a_detour() {
    let _case = sts1_case();
    let mut s = screens(json!({"act": 1, "screen_type": "MAP", "screen_state": {"next_nodes": [{"x": 0, "y": 0}, {"x": 2, "y": 0}]}, "map": [
        {"x": 0, "y": 0, "symbol": "M", "children": [{"x": 0, "y": 1}]}, {"x": 2, "y": 0, "symbol": "M", "children": [{"x": 2, "y": 1}]},
        {"x": 0, "y": 1, "symbol": "E", "children": []}, {"x": 2, "y": 1, "symbol": "E", "emerald_key": true, "children": []}]}));
    let map = s._map();
    let node = |col: i64, row: i64| map["map"]["nodes"].list().iter().rfind(|n| n["col"] == col && n["row"] == row).expect("KeyError")["type"].clone();
    assert_eq!((node(0, 2), node(2, 2)), (json!("Elite"), json!("EmeraldElite")));
    let plain = strategy::path_value(&[("Monster", Some(9)), ("Elite", Some(10))], 70.0, 80, 99, 1, 1.0);
    let burning = strategy::path_value(&[("Monster", Some(9)), ("EmeraldElite", Some(10))], 70.0, 80, 99, 1, 1.0);
    assert!(((burning - plain) - strategy::EMERALD_WORTH[&1]).abs() < 5e-8, "{burning} - {plain}");
    game_state(&mut s)["keys"]["emerald"] = json!(true);
    assert!(!each(&s._map()["map"]["nodes"], "type").contains(&&json!("EmeraldElite")));
}

/// A game that knows only its profile.
struct Profile;

impl Game for Profile {
    fn state(&mut self) -> Result<Value, GameUnavailable> {
        panic!("'Profile' object has no attribute 'state'")
    }
    fn act(&mut self, _action: &str, _params: &Value) -> Result<Value, GameUnavailable> {
        panic!("'Profile' object has no attribute 'act'")
    }
    fn profile(&mut self) -> Result<Value, GameUnavailable> {
        Ok(json!({"total_wins": 0, "characters": []}))
    }
    fn card(&mut self, _card_id: &str) -> Result<Option<Value>, GameUnavailable> {
        Ok(None)
    }
    fn ping(&mut self) -> bool {
        true
    }
}

#[test]
fn test_the_keys_held_are_logged_and_kept_with_the_run() {
    let _case = sts1_case();
    assert_eq!(screens(json!({"keys": {"ruby": true, "emerald": false, "sapphire": true}}))._run()["keys"], json!(["ruby", "sapphire"]));
    assert!(screens(json!({"final_act_available": false}))._run().get("keys").is_none()); // (act 4 shut: nothing to collect)

    let d = tempfile::tempdir().unwrap();
    let mut bot = Bot::new(Rc::new(RefCell::new(Profile)), BotArgs { clock: Some(Box::new(|| 0.0)), sleep: Some(Box::new(|_| {})), log_dir: Some(d.path().to_path_buf()), ..Default::default() });
    let run = json!({"act": 1, "floor": 6, "keys": []});
    // (`with self.assertLogs("sts2bot", level="INFO") as logs`)
    let ((), records) = logging::capture(|| {
        bot._note_state(&json!({"state_type": "rest_site", "run": run, "player": {"hp": 70, "max_hp": 80}}));
        bot._note_state(&json!({"state_type": "map", "run": with(run.clone(), json!({"keys": ["ruby"]})), "player": {"hp": 70, "max_hp": 80}}));
        bot._note_state(&json!({"state_type": "game_over", "run": with(run.clone(), json!({"floor": 20, "keys": ["ruby"]})), "player": {"hp": 0, "max_hp": 80}}));
    });
    let output: Vec<String> = records.iter().filter(|r| r.level >= Level::Info && (r.logger == "sts2bot" || r.logger.starts_with("sts2bot."))).map(|r| r.output()).collect();
    assert!(!output.is_empty(), "no logs of level INFO or higher triggered on sts2bot");
    assert_eq!(output.iter().filter(|line| line.contains("keys")).collect::<Vec<_>>(), ["INFO:sts2bot:keys: ruby"]);
    let kept: Value = serde_json::from_str(&std::fs::read_to_string(d.path().join("runs.jsonl")).unwrap()).unwrap();
    assert_eq!(kept["keys"], json!(["ruby"]));
}

// class RunEnd

#[test]
fn test_a_finished_room_is_not_the_end_of_the_run() {
    let _case = sts1_case();
    // After the act 3 boss CommunicationMod says COMPLETE ("proceed" goes on to the victory room): read as game over,
    // the first win was written down as a loss on floor 50 and a win on floor 51.
    let mut s = Screens::new();
    let raw = json!({"in_game": true, "available_commands": ["proceed"], "game_state": with(_game(json!({"floor": 50})), json!({"screen_type": "COMPLETE", "room_phase": "COMPLETE"}))});
    s.raw = Rc::new(raw.clone());
    assert_eq!(s.translate(&raw)["state_type"], "overlay");
}

#[test]
fn test_a_victory_screen_does_not_lose_the_last_boss() {
    let _case = sts1_case();
    let boss = json!({"state_type": "boss", "run": {"act": 3, "floor": 50}, "battle": {"round": 5, "enemies": [{"entity_id": "DONU_0", "hp": 5, "max_hp": 250}]},
                      "player": {"hp": 33, "max_hp": 87}});
    let won = json!({"state_type": "game_over", "run": {"act": 3, "floor": 51}, "player": {"hp": 33}, "game_over": {"victory": true}});
    let (mut bosses, mut fights) = (BossWatch::new(), FightWatch::new(None));
    bosses.update(&boss, Some("boss"));
    fights.update(&boss, Some("boss"));
    assert!(bosses.update(&won, Some("game_over")).expect("the boss fight's line").contains(" won;"));
    assert!(!fights.update(&won, Some("game_over")).expect("the fight's record")["died"].truthy());
}

// class Scry

/// What the Scry test's client and the test share.
struct ScryTable {
    hand: Vec<Value>,
    picked: BTreeSet<String>,
    sent: Vec<String>,
}

impl ScryTable {
    fn grid(&self) -> Value {
        let ss = json!({"cards": self.hand, "selected_cards": self.hand.iter().filter(|c| self.picked.contains(c["uuid"].s())).collect::<Vec<_>>(), "any_number": true, "num_cards": 3});
        json!({"in_game": true, "available_commands": ["choose", "confirm"],
               "game_state": {"screen_type": "GRID", "current_action": "ScryAction", "room_phase": "COMBAT", "screen_state": ss}})
    }
}

/// (Python's `class Fake(Sts1Client)`: `_message` and `send` are its own.)
struct Fake(Rc<RefCell<ScryTable>>);

impl Wire for Fake {
    fn _message(&mut self) -> Result<Value, GameUnavailable> {
        Ok(self.0.borrow().grid())
    }

    fn send(&mut self, command: &str) -> Result<Value, GameUnavailable> {
        let mut t = self.0.borrow_mut();
        t.sent.push(command.to_string());
        if command == "confirm" {
            return Ok(json!({"status": "ok", "message": {"in_game": true, "available_commands": ["play", "end"], "game_state": {"screen_type": "NONE"}}}));
        }
        let at: usize = command.split_whitespace().nth(1).unwrap().parse().unwrap();
        let uuid = t.hand[at]["uuid"].s().to_string();
        if !t.picked.remove(&uuid) {
            t.picked.insert(uuid);
        }
        Ok(json!({"status": "ok", "message": t.grid()}))
    }
}

#[test]
fn test_scry_discards_status_and_curse_cards_and_confirms() {
    let _case = sts1_case();
    // The Watcher's first run: the bot clicked Scry's three cards on and off for good.
    let hand = vec![json!({"name": "Defend", "uuid": "a", "type": "SKILL"}), json!({"name": "Dazed", "uuid": "b", "type": "STATUS"}), json!({"name": "Strike", "uuid": "c", "type": "ATTACK"})];
    let table = Rc::new(RefCell::new(ScryTable { hand, picked: BTreeSet::from(["a".to_string()]), sent: Vec::new() })); // (a Defend already clicked on, as the stuck run had)
    Sts1Client::with_wire(Box::new(Fake(table.clone())), Prefs::None).state().unwrap();
    assert_eq!(table.borrow().sent, ["choose 0", "choose 1", "confirm"]);
    assert_eq!(table.borrow().picked, BTreeSet::from(["b".to_string()]));
}

// class EnergyLoss

#[test]
fn test_a_power_that_takes_energy_every_turn_is_priced_for_it() {
    let _case = sts1_case();
    // Fasting read as 7.8 (3 Strength, 3 Dexterity), and the Watcher's first run held two: 1 energy a turn.
    let fasting = Screens::new().card(&json!({"id": "Fasting2", "name": "Fasting", "type": "POWER", "rarity": "UNCOMMON", "cost": 2}), None, false);
    let strike = Screens::new().card(&json!({"id": "Strike_P", "name": "Strike", "type": "ATTACK", "rarity": "BASIC", "cost": 1}), None, false);
    let strikes = vec![strike; 10];
    let first = cards::deck_value(&fasting) + decks::deck_fit(&fasting, &strikes, Some(1), 3);
    let held: Vec<Value> = strikes.iter().cloned().chain([fasting.clone()]).collect();
    let second = cards::deck_value(&fasting) + decks::deck_fit(&fasting, &held, Some(1), 3);
    assert!(first < cards::deck_value(&fasting) - 3.0);
    assert!(second < 0.0);
}

#[test]
fn test_a_second_fasting_is_not_played() {
    let _case = sts1_case();
    // Three Fasting+ left the Watcher 1 energy a turn, dead to a Chosen from full HP. One already in play: the second is an
    // energy of what is left, and the Strikes are worth more.
    let fasting = Screens::new().card(&json!({"id": "Fasting2", "name": "Fasting", "type": "POWER", "rarity": "UNCOMMON", "cost": 2, "upgrades": 1}), Some(0), false);
    assert_eq!(cards::parse(&fasting).energy_drain, 1);
    let mut s = combat_state!([fasting, STRIKE(1), STRIKE(2)], [enemy!("CHOSEN_0", 98, attack = "5x2")], energy = 2, kind = "elite");
    s["player"]["status"] = json!(Screens::powers(&json!({"powers": [{"id": "EnergyDownPower", "amount": 1}, {"id": "Strength", "amount": 4}]})));
    assert_eq!(s["player"]["status"][0]["id"], "ENERGY_DOWN_POWER");
    assert_ne!(card_index(&combat::choose_card(&s, None)), 0);
}

// class Curses

#[test]
fn test_curses_that_hurt_at_the_end_of_the_turn_are_seen() {
    let _case = sts1_case();
    // In the first game's words ("At the end of your turn, lose HP equal to ...") Regret cost 3-5 HP a turn the plans
    // counted as nothing; Decay and Burn too.
    let hand: Vec<Value> = [("Regret", "CURSE"), ("Decay", "CURSE"), ("Burn", "STATUS"), ("Strike_R", "ATTACK")]
        .iter()
        .enumerate()
        .map(|(n, (cid, t))| Screens::new().card(&json!({"id": cid, "name": cid, "type": t, "cost": -2}), Some(n as i64), false))
        .collect();
    assert_eq!(battle::hand_burns(&json!({"hand": hand})), [(0, -1, false), (1, 2, true), (2, 2, true)]);
}

// class TimeEater

#[test]
fn test_the_twelfth_card_ends_the_turn() {
    let _case = sts1_case();
    // 11 cards played, 30 HP, a 31 hit coming, 4 energy: Cold Snap was the 12th card and the last.
    let s = combat_state!([STRIKE(0), DEFEND(1), STRIKE(2)], [enemy!("TIMEEATER_0", 183, attack = 31, status = json!([{"id": "TIME_WARP_POWER", "amount": 11}]))],
                          energy = 4, hp = 30, block = 4, kind = "boss");
    assert_eq!(battle::cards_left_this_turn(&s), Some(1));
    assert_eq!(card_index(&combat::choose_card(&s, None)), 1);
}

// class PaidHp

#[test]
fn test_no_card_that_costs_the_last_hp_even_with_regen_to_come() {
    let _case = sts1_case();
    // On 5 HP with Regen 3 the planner read Offering+'s 6 as 3 and played it into a stunned boss: dead.
    let hand = [live(0, "Spot Weakness", "SKILL", 1, 1, json!({})), live(1, "Wound", "STATUS", -2, 0, json!({})), live(2, "Defend_R", "SKILL", 1, 1, json!({"block": 10, "base_block": 8})),
                live(3, "Bash", "ATTACK", 2, 1, json!({"damage": 21, "base_damage": 10})), live(4, "Offering", "SKILL", 0, 1, json!({}))];
    let mut boss = enemy!("BRONZEAUTOMATON_1", 75, status = json!([{"id": "ARTIFACT_POWER", "amount": 1}, {"id": "STRENGTH_POWER", "amount": 6}]));
    boss["intents"] = json!([{"type": "Stun", "label": ""}]);
    let mut s = combat_state!(hand, [boss], energy = 2, hp = 5, kind = "boss");
    s["player"]["draw_pile_count"] = json!(10);
    s["player"]["status"] = json!([{"id": "REGEN_POWER", "amount": 3}, {"id": "STRENGTH_POWER", "amount": 11}]);
    s["battle"]["round"] = json!(7);
    assert_ne!(card_index(&combat::choose_card(&s, None)), 4);
}

#[test]
fn test_an_event_curse_in_the_first_game_s_words_costs_something() {
    let _case = sts1_case();
    // "Become Cursed - Regret." cost nothing: Golden Shrine's Desecrate (275 Gold and Regret) beat Pray.
    let value = |title: &str, text: &str| events::_event_option_value(&json!({"title": title, "description": text}), 60, 80, EventOptionValueArgs { act: Some(1), ..Default::default() });
    assert!(value("Desecrate", "Gain 275 Gold. Become Cursed - Regret.") < value("Pray", "Gain 100 Gold."));
    assert!(value("Rummage", "Obtain a special Relic. Become Cursed - Pain.") < value("Forge", "Upgrade a card in your deck."));
}

// class WrithingMass

fn mass_state(hp: i64) -> Value {
    let hand = [live(0, "Defend_P", "SKILL", 1, 1, json!({"block": 8, "base_block": 8})), live(1, "CrushJoints", "ATTACK", 0, 1, json!({"damage": 10, "base_damage": 10})),
                live(2, "Consecrate", "ATTACK", 0, 1, json!({"damage": 8, "base_damage": 8})), live(3, "Eruption", "ATTACK", 2, 0, json!({"damage": 9, "base_damage": 9})),
                live(4, "ThirdEye", "SKILL", 1, 0, json!({"block": 7, "base_block": 7}))];
    let mut mass = enemy!("WRITHINGMASS_0", 81, status = Screens::powers(&json!({"powers": [{"id": "Malleable", "amount": 3}, {"id": "Compulsive", "amount": -1}]})));
    mass["intents"] = json!([{"type": "Debuff", "label": ""}]);
    let mut s = combat_state!(hand, [mass], energy = 4, hp = hp, max_hp = 89);
    s["battle"]["round"] = json!(4);
    s
}

#[test]
fn test_a_hit_gives_it_a_new_intent() {
    let _case = sts1_case();
    // On 12 HP, its intent a Debuff, the Watcher entered Wrath and hit it; it came round to 14 x 3.
    let action = combat::choose_card(&mass_state(12), None);
    assert!(match &action {
        None => true,
        Some((name, params)) => name != "play_card" || [0, 4].contains(&params["card_index"].as_i64().expect("KeyError: 'card_index'")),
    });
    assert_eq!(card_index(&combat::choose_card(&mass_state(60), None)), 1); // (with HP to spare it is still to be killed)
}

#[test]
fn test_on_low_hp_a_turn_it_attacks_anyway_is_not_a_reason_to_stop() {
    let _case = sts1_case();
    // 2026-11-25 18:46: on 21 HP every hit was priced at its worst (32), so the bot only blocked: 11 rounds, the Mass 160 -> 120.
    let mut s = mass_state(21);
    s["battle"]["enemies"][0]["intents"] = json!([{"type": "Attack", "label": "10x3"}]);
    let mut plan = Plan::default();
    combat::choose_card(&s, Some(&mut plan));
    assert!(plan.line.expect("KeyError: 'line'").iter().any(|c| c.r#type == "Attack"));
    assert_eq!(battle::reacted(30, 20, 32), 20); // (a new draw instead of the 30 shown)
    assert_eq!(battle::reacted(0, 20, 32), 32); // (a quiet turn still stays quiet)
    assert_eq!(battle::reacted(32, 20, 0), 20); // (above 40 HP: the mean in place of whatever was shown)
}

// class AwakenedOne

#[test]
fn test_its_first_death_is_not_the_end() {
    let _case = sts1_case();
    let status = Screens::powers(&json!({"powers": [{"id": "Unawakened", "amount": -1}, {"id": "Curiosity", "amount": 1}]}));
    let e = json!({"entity_id": "AWAKENEDONE_2", "hp": 20, "max_hp": 300, "block": 0, "status": status, "intents": [{"type": "Attack", "label": "16"}]});
    let read = battle::read_enemies(&json!({"battle": {"enemies": [e], "round": 9}, "player": {"hp": 50}, "state_type": "boss"}));
    assert_eq!(read.len(), 1);
    let boss = &read[0];
    assert_eq!((boss.respawns, boss.curious), (1, 1));
    assert_eq!(closing::discount(30.0, 10, 50, &[boss]), 1.0); // (20 HP is not "dying next turn": 300 more come after)
}

// class NoWayToAct4

#[test]
fn test_without_the_emerald_and_its_elite_behind_us_the_chest_keeps_its_relic() {
    let _case = sts1_case();
    let relic = json!({"id": "Anchor", "name": "Anchor"});
    let rewards = json!([{"reward_type": "RELIC", "relic": relic}, {"reward_type": "SAPPHIRE_KEY", "link": relic}]);
    let behind = json!([{"x": 1, "y": 3, "symbol": "E", "emerald_key": true, "children": []}]);
    let ahead = json!([{"x": 1, "y": 12, "symbol": "E", "emerald_key": true, "children": []}]);
    for (nodes, held, want) in [(&behind, false, ["relic"]), (&ahead, false, ["sapphire_key"]), (&behind, true, ["sapphire_key"])] {
        let mut s = screens(json!({"floor": 43, "map": nodes, "keys": {"ruby": true, "emerald": held, "sapphire": false}, "screen_type": "COMBAT_REWARD", "screen_state": {"rewards": rewards}}));
        assert_eq!(each(&s._combat_reward()["rewards"]["items"], "type"), want, "ahead={} emerald={held}", std::ptr::eq(nodes, &ahead));
    }
    assert!(keys::emerald_ahead(&_game(json!({"act": 2, "floor": 30})))); // (another act, another burning elite)
}

#[test]
fn test_nor_does_an_act_3_rest_site_give_up_its_rest_for_the_ruby() {
    let _case = sts1_case();
    let rest = json!({"rest_options": ["rest", "smith", "recall"]});
    let behind = json!([{"x": 1, "y": 3, "symbol": "E", "emerald_key": true, "children": []}]);
    let mut s = screens(json!({"floor": 47, "current_hp": 75, "map": behind, "screen_type": "REST", "screen_state": rest}));
    assert_eq!(each(&s._rest()["rest_site"]["options"], "id"), ["rest", "smith"]);
    game_state(&mut s)["map"] = json!([with(behind[0].clone(), json!({"y": 13}))]);
    assert_eq!(each(&s._rest()["rest_site"]["options"], "id"), ["recall"]);
}
