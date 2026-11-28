//! The first Slay the Spire: the state as the bot reads it, the commands its actions become, whole runs.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use serde_json::{json, Value};
use sts2bot::bot::BotArgs;
use sts2bot::cards;
use sts2bot::fight::combat;
use sts2bot::games;
use sts2bot::games::sts1::client::{Prefs, Sts1Client};
use sts2bot::games::sts1::screens::Screens;
use sts2bot::games::sts1::{host, speedmod, text, tiers};
use sts2bot::base::JsonExt;
use sts2bot::run::{crowd, potions, strategy};
use sts2bot::supervisor;

use super::fake_comm::FakeComm;
use super::helpers::{direct, direct_with, sts1_case};
use crate::helpers::make_bot_kw;

fn shared(game: FakeComm) -> Rc<RefCell<FakeComm>> {
    Rc::new(RefCell::new(game))
}

/// `make_bot(client, character="IRONCLAD")`.
fn ironclad() -> BotArgs {
    BotArgs { character: "IRONCLAD".to_string(), ..Default::default() }
}

/// The fake (seed 1) with an Ironclad run begun under the client, and a fight in `room` if given.
fn started(room: Option<&str>) -> (Rc<RefCell<FakeComm>>, Sts1Client) {
    let game = shared(FakeComm::seeded(1));
    let client = direct(&game);
    game.borrow_mut()._start("IRONCLAD", 0);
    if let Some(room) = room {
        game.borrow_mut()._fight(room);
    }
    (game, client)
}

/// host.read_prefs over a save folder of these files (a dict written as JSON, a str as it is).
fn read_saves(files: &[(&str, Value)]) -> Value {
    let d = tempfile::tempdir().unwrap();
    for (name, body) in files {
        std::fs::write(d.path().join(name), match body {
            Value::String(text) => text.clone(),
            other => other.to_string(),
        })
        .unwrap();
    }
    host::read_prefs(Some(d.path()))
}

/// `json.loads(path.read_text())`.
fn read_json(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// `[x[key] for x in items]`.
fn each<'a>(items: &'a Value, key: &str) -> Vec<&'a Value> {
    items.list().iter().map(|x| &x[key]).collect()
}

// class WholeRuns

#[test]
fn test_the_bot_plays_runs_start_to_finish() {
    let _case = sts1_case();
    let game = shared(FakeComm::seeded(3));
    let client = Rc::new(RefCell::new(direct_with(&game, Prefs::Dict(json!({"max_ascension": {"IRONCLAD": 2}})))));
    let mut bot = make_bot_kw(client, ironclad());
    bot.run(Some(2)).unwrap();
    assert_eq!(bot.runs_finished, 2);
    assert_eq!(game.borrow().runs, 2);
    assert_eq!(bot.wins, 2); // (the boss is beaten: the fake's game over says victory)
    let sent = game.borrow().sent.clone();
    assert!(sent.iter().any(|s| s == "start IRONCLAD 2")); // at the highest ascension the save files open
    assert!(sent.iter().any(|s| s.starts_with("play ")));
    assert!(sent.iter().any(|s| s == "end"));
    assert!(sent.iter().any(|s| s == "choose shop")); // the merchant's shelf is opened ...
    assert!(sent.iter().any(|s| s == "leave")); // ... and left
    assert!(sent.iter().all(|s| !s.starts_with("play ") || [2, 3].contains(&s.split_whitespace().count())));
    assert!(!bot.mem.character_selected); // (reset for the next run)
}

#[test]
fn test_a_skipped_card_reward_is_not_opened_again() {
    let _case = sts1_case();
    // CommunicationMod names a reward's kind "reward_type", and the game keeps a skipped card reward on the list:
    // the first live run opened it, skipped it and opened it again hundreds of times a second.
    let (game, mut client) = started(None);
    {
        let mut g = game.borrow_mut();
        (g.screen, g.phase, g.rewards) = ("COMBAT_REWARD".to_string(), Some("COMPLETE".to_string()), vec![json!({"reward_type": "GOLD", "gold": 15}), json!({"reward_type": "CARD"})]);
    }
    assert_eq!(each(&client.state().unwrap()["rewards"]["items"], "type"), ["gold", "card"]);
    let mut bot = make_bot_kw(Rc::new(RefCell::new(client)), ironclad());
    {
        let _bar = strategy::with_reward_bar_hook(|_size, _act| 99.0); // (nothing on offer is good enough)
        for _ in 0..8 {
            bot.step().unwrap();
        }
    }
    assert_eq!(game.borrow().sent.iter().filter(|c| *c == "skip").count(), 1);
    assert!(game.borrow().sent.iter().any(|c| c == "proceed")); // (and on to the map)
}

#[test]
fn test_a_fights_first_look_waits_for_the_intents() {
    let _case = sts1_case();
    let (game, mut client) = started(Some("MonsterRoom"));
    game.borrow_mut().intent_pending = 2; // (the game sets them a few frames in)
    assert_eq!(client.state().unwrap()["battle"]["enemies"][0]["intents"][0]["type"], "Attack");
    let waits = || game.borrow().sent.iter().filter(|c| *c == "wait 10").count();
    assert_eq!(waits(), 2);
    client.state().unwrap();
    assert_eq!(waits(), 2); // (none once they are there)
}

#[test]
fn test_attacks_are_aimed_at_the_monster_by_its_place_in_the_list() {
    let _case = sts1_case();
    let (game, mut client) = started(Some("MonsterRoom"));
    game.borrow_mut().hand = vec![("Strike_R", "Strike", "ATTACK", 1, true)];
    let state = client.state().unwrap();
    assert_eq!(state["state_type"], "monster");
    let enemy = &state["battle"]["enemies"][0];
    assert_eq!(enemy["entity_id"], "JAWWORM_0");
    assert_eq!(enemy["intents"], json!([{"type": "Attack", "label": "7"}]));
    let wanted = combat::actions(&state);
    assert_eq!(wanted[0], ("play_card".to_string(), json!({"card_index": 0, "target": "JAWWORM_0"})));
    assert_eq!(client.command(&wanted[0].0, &wanted[0].1).as_deref(), Some("play 1 0"));
    assert_eq!(client.act("end_turn", &json!({})).unwrap()["status"], "ok");
}

// class Translation

#[test]
fn test_cards_read_like_the_second_games() {
    let _case = sts1_case();
    let s = Screens::new();
    let info = cards::parse(&s.card(&json!({"id": "Bash", "name": "Bash", "type": "ATTACK", "cost": 2, "has_target": true, "rarity": "BASIC", "upgrades": 0}), None, false));
    assert_eq!((info.damage, info.vulnerable, info.cost), (8, 2, 2));
    let plus = cards::parse(&s.card(&json!({"id": "Bash", "name": "Bash+", "type": "ATTACK", "cost": 2, "has_target": true, "upgrades": 1}), None, false));
    assert_eq!((plus.damage, plus.vulnerable), (10, 3));
    // Ids that are older names find their text, and energy is read.
    let sneaky = s.card(&json!({"id": "Underhanded Strike", "name": "Sneaky Strike", "type": "ATTACK", "cost": 2, "has_target": true}), None, false);
    assert_eq!(cards::parse(&sneaky).damage, 12);
    assert_eq!(cards::parse(&s.card(&json!({"id": "Seeing Red", "name": "Seeing Red", "type": "SKILL", "cost": 1}), None, false)).energy_gain, 2);
    let whirl = s.card(&json!({"id": "Whirlwind", "name": "Whirlwind", "type": "ATTACK", "cost": -1}), None, false);
    assert_eq!((&whirl["cost"], &whirl["target_type"]), (&json!("X"), &json!("AllEnemies")));
}

#[test]
fn test_every_table_entry_expands_to_plain_text() {
    let _case = sts1_case();
    for (key, raw) in text::CARDS().iter() {
        for up in [false, true] {
            let out = text::expand(raw, up);
            assert!(!out.contains("||"), "{key}");
            assert!(!sts2bot::re!(r"\d+\(\d+\)").is_match(&out), "{key}");
        }
    }
}

#[test]
fn test_the_boss_closes_every_road_on_the_map() {
    let _case = sts1_case();
    let road = json!([{"x": 1, "y": 13, "symbol": "R", "children": [{"x": 1, "y": 14}]}, {"x": 1, "y": 14, "symbol": "M", "children": [{"x": 3, "y": 16}]}]);
    let st = Screens::new().translate(&json!({"in_game": true, "available_commands": ["choose"], "game_state": {
        "screen_type": "MAP", "act": 1, "floor": 14, "current_hp": 50, "max_hp": 80, "gold": 0, "map": road,
        "screen_state": {"next_nodes": [{"x": 1, "y": 14, "symbol": "M"}], "boss_available": false}}}));
    assert_eq!(st["map"]["next_options"][0]["leads_to"][0]["type"], "Boss");
    assert_eq!(st["map"]["nodes"].list().iter().filter(|n| n["type"] == "Boss").count(), 1);
}

#[test]
fn test_potions_that_must_stay_on_the_belt_are_not_offered() {
    let _case = sts1_case();
    let mut s = Screens::new();
    s.raw = Rc::new(json!({"game_state": {"potions": [{"id": "FairyPotion", "name": "Fairy in a Bottle", "can_use": true, "requires_target": false},
                                                      {"id": "Potion Slot", "name": "Potion Slot"}, {"id": "Fire Potion", "name": "Fire Potion", "can_use": true, "requires_target": true}]}}));
    let belt = s.potions();
    assert_eq!(belt.iter().map(|p| &p["slot"]).collect::<Vec<_>>(), [0, 2]);
    assert!(!belt[0]["can_use_in_combat"].truthy());
    assert_eq!((&belt[1]["target_type"], &belt[1]["description"]), (&json!("AnyEnemy"), &json!("Deal 20 damage.")));
}

#[test]
fn test_powers_take_the_names_the_planner_knows() {
    let _case = sts1_case();
    let m = json!({"powers": [{"id": "Weakened", "amount": 2}, {"id": "Plated Armor", "amount": 4}, {"id": "Vulnerable", "amount": 1}]});
    assert_eq!(Screens::powers(&m).iter().map(|p| &p["id"]).collect::<Vec<_>>(), ["WEAK_POWER", "PLATING_POWER", "VULNERABLE_POWER"]);
}

#[test]
fn test_menu_is_the_bots_own_until_the_run_starts() {
    let _case = sts1_case();
    let game = shared(FakeComm::default());
    let mut client = direct_with(&game, Prefs::Dict(json!({"max_ascension": {"DEFECT": 4}})));
    assert_eq!(client.state().unwrap()["options"], json!(["singleplayer"]));
    assert_eq!(client.act("menu_select", &json!({"option": "singleplayer"})).unwrap()["status"], "ok");
    assert_eq!(client.act("menu_select", &json!({"option": "DEFECT"})).unwrap()["status"], "ok");
    assert_eq!(client.state().unwrap()["ascension"], 4);
    assert_eq!(client.act("menu_select", &json!({"option": "confirm"})).unwrap()["status"], "ok");
    assert_eq!(game.borrow().sent.last().unwrap(), "start DEFECT 4");
    assert_eq!(client.state().unwrap()["state_type"], "map");
}

// class GameSwitch

#[test]
fn test_the_second_games_ratings_stay_out_of_the_first() {
    let _case = sts1_case();
    assert_eq!(crowd::rating("INFLAME"), Some(tiers::TIER()["INFLAME"])); // (the first game's own tiers: games/sts1/tiers.rs)
    assert!(crowd::rating("CRIMSON_MANTLE").is_none()); // (a Slay the Spire 2 card)
    assert!(!cards::is_known(&json!({"id": "INFLAME", "name": "Inflame"})));
    assert_eq!(potions::boss_floors(), potions::STS1_BOSS_FLOORS);
    games::select("sts2").unwrap();
    assert!(crowd::rating("INFLAME").is_some());
    assert_eq!(potions::boss_floors(), potions::BOSS_FLOORS);
}

#[test]
fn test_unknown_game_is_refused() {
    let _case = sts1_case();
    assert!(games::select("sts3").is_err());
}

// class SaveFilesAndSetup

#[test]
fn test_ascension_and_wins_come_from_the_save_files() {
    let _case = sts1_case();
    let prefs = read_saves(&[
        ("STSDataVagabond", json!({"WIN_COUNT": "3", "ASCENSION_LEVEL": "6"})),
        ("STSDataTheSilent", json!({"WIN_COUNT": "0", "ASCENSION_LEVEL": "1"})),
        ("STSDataDefect", json!("not json")),
        ("STSUnlocks", json!({"The Silent": "2", "Defect": "1", "GUARDIAN": "2"})),
        ("STSPlayer", json!({"IRONCLAD_WIN": "true"})),
    ]); // (Vagabond: the Ironclad)
    assert_eq!(prefs["wins"], 3);
    assert_eq!(prefs["max_ascension"], json!({"IRONCLAD": 6, "THE_SILENT": 0}));
    assert_eq!(prefs["locked"], json!(["WATCHER"]));
}

#[test]
fn test_a_first_win_opens_ascension_1() {
    let _case = sts1_case();
    // The game keeps no ASCENSION_LEVEL until the first level-up and reads it as 1 once a character has won.
    let prefs = read_saves(&[
        ("STSDataVagabond", json!({"WIN_COUNT": "1", "WIN_STREAK": "0"})),
        ("STSDataTheSilent", json!({"WIN_STREAK": "0"})),
        ("STSPlayer", json!({"IRONCLAD_WIN": "true"})),
    ]);
    assert_eq!(prefs["max_ascension"], json!({"IRONCLAD": 1, "THE_SILENT": 0}));
}

#[test]
fn test_a_win_act_4_does_not_know_of_leaves_the_character_on_0() {
    let _case = sts1_case();
    // (A win from before the bot's mod set no DEFECT_WIN: the rotation plays the Defect until it wins again.)
    let prefs = read_saves(&[("STSDataDefect", json!({"WIN_COUNT": "1"})), ("STSDataWatcher", json!({"WIN_STREAK": "0"})), ("STSPlayer", json!({"IRONCLAD_WIN": "true"}))]);
    assert_eq!(prefs["max_ascension"], json!({"DEFECT": 0, "WATCHER": 0}));
}

#[test]
fn test_a_new_profile_has_only_the_ironclad() {
    let _case = sts1_case();
    let prefs = read_saves(&[]);
    assert_eq!(prefs, json!({"max_ascension": {}, "wins": 0, "locked": ["THE_SILENT", "DEFECT", "WATCHER"]}));
    let mut client = direct_with(&shared(FakeComm::default()), Prefs::Dict(prefs));
    assert_eq!(each(&client.profile().unwrap()["characters"], "id"), ["IRONCLAD"]);
    client.act("menu_select", &json!({"option": "singleplayer"})).unwrap();
    assert_eq!(client.act("menu_select", &json!({"option": "DEFECT"})).unwrap()["status"], "error"); // (a locked one is not picked)
}

#[test]
fn test_the_save_files_are_read_again_before_each_run() {
    let _case = sts1_case();
    let seen = Rc::new(RefCell::new(json!({"max_ascension": {"IRONCLAD": 0}, "wins": 0, "locked": ["THE_SILENT", "DEFECT", "WATCHER"]})));
    let read = seen.clone();
    let mut client = direct_with(&shared(FakeComm::default()), Prefs::Read(Box::new(move || read.borrow().clone())));
    assert_eq!(client.profile().unwrap()["characters"], json!([{"id": "IRONCLAD", "max_ascension": 0}]));
    *seen.borrow_mut() = json!({"max_ascension": {"IRONCLAD": 1}, "wins": 1, "locked": ["DEFECT", "WATCHER"]}); // a win: A1, and the Silent
    assert_eq!(client.profile().unwrap()["characters"], json!([{"id": "IRONCLAD", "max_ascension": 1}, {"id": "THE_SILENT", "max_ascension": 0}]));
    assert_eq!(client.profile().unwrap()["total_wins"], 1);
}

#[test]
fn test_the_game_is_set_to_full_screen_and_fast_mode_before_it_opens() {
    let _case = sts1_case();
    let d = tempfile::tempdir().unwrap();
    let (game, prefs) = (d.path().to_path_buf(), d.path().join("preferences"));
    let display = || std::fs::read_to_string(game.join("info.displayconfig")).unwrap().split_whitespace().map(|w| w.to_string()).collect::<Vec<_>>();
    std::fs::write(game.join("info.displayconfig"), "2560\n1600\n60\nfalse\nfalse\ntrue\n").unwrap();
    let changed = host::prepare_game(true, Some(&prefs), Some(&game));
    assert_eq!(display(), ["2560", "1600", &host::FRAME_CAP.to_string(), "false", "true", "false"]);
    assert_eq!(read_json(&prefs.join("STSGameplaySettings"))["Fast Mode"], "true");
    let tips = read_json(&prefs.join("STSTips"));
    assert_eq!((&tips["INTENT_TIP"], &tips["CARD_REWARD_TIP"], &tips["NO_FTUE"]), (&json!("true"), &json!("true"), &json!("true")));
    let slots = read_json(&prefs.join("STSSaveSlots"));
    assert_eq!((&slots["DEFAULT_SLOT"], &slots["PROFILE_NAME"]), (&json!("0"), &json!(host::PROFILE_NAME))); // (a profile, as the game's name screen makes)
    let sound = read_json(&prefs.join("STSSound"));
    assert_eq!((&sound["Master Volume"], &sound["Music Volume"], &sound["Sound Volume"], &sound["Mute in Bg"]), (&json!("1.0"), &json!("1.0"), &json!("1.0"), &json!("true")));
    assert_eq!(changed.len(), 5);
    assert_eq!(host::prepare_game(true, Some(&prefs), Some(&game)), Vec::<String>::new()); // (once set, left alone)
    host::prepare_game(false, Some(&prefs), Some(&game));
    assert_eq!(display()[3..5], ["false", "false"]); // --windowed
}

#[test]
fn test_the_bot_s_old_profile_name_becomes_the_user_s() {
    let _case = sts1_case();
    let d = tempfile::tempdir().unwrap();
    let (game, prefs) = (d.path().to_path_buf(), d.path().join("preferences"));
    std::fs::create_dir(&prefs).unwrap();
    let old = json!({"DEFAULT_SLOT": "0", "PROFILE_NAME": "sts2bot", "COMPLETION": "37.7", "PLAYTIME": "180208"});
    for name in ["STSSaveSlots", "STSSaveSlots.backUp"] {
        std::fs::write(prefs.join(name), old.to_string()).unwrap();
    }
    let changed = host::prepare_game(true, Some(&prefs), Some(&game));
    for name in ["STSSaveSlots", "STSSaveSlots.backUp"] {
        let slots = read_json(&prefs.join(name));
        assert_eq!((&slots["PROFILE_NAME"], &slots["PLAYTIME"]), (&json!("jegoh"), &json!("180208")));
    }
    assert!(changed.iter().any(|c| c == "profile renamed 'sts2bot' -> 'jegoh'"), "{changed:?}");
    let mut someone = old.clone();
    someone["PROFILE_NAME"] = json!("Someone");
    std::fs::write(prefs.join("STSSaveSlots"), someone.to_string()).unwrap();
    host::prepare_game(true, Some(&prefs), Some(&game));
    assert_eq!(read_json(&prefs.join("STSSaveSlots"))["PROFILE_NAME"], "Someone"); // (a name the user chose stays)
}

#[test]
fn test_the_game_is_launched_with_its_clock_sped_up_and_its_output_kept() {
    let _case = sts1_case();
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().to_path_buf();
    // (`mock.patch.dict("os.environ", ...)`: the environment as this thread's supervisor sees it, not the process's)
    let mut env = supervisor::environ();
    env.insert("STS1_GAME_DIR".to_string(), dir.to_string_lossy().into_owned());
    env.insert("STS1_PREFS_DIR".to_string(), dir.join("preferences").to_string_lossy().into_owned());
    // What `_spawn` was called with: argv, and env (None: no `env` keyword).
    type Spawned = Option<(Vec<String>, Option<indexmap::IndexMap<String, String>>)>;
    let spawned: Rc<RefCell<Spawned>> = Rc::new(RefCell::new(None));
    let seen = spawned.clone();
    let _p = supervisor::patch(|s| {
        s.environ = Some(env);
        s.set_spawn(move |argv, kw| {
            *seen.borrow_mut() = Some((argv.to_vec(), kw.env));
            Ok(())
        });
    });
    {
        let _ensure = speedmod::with_ensure_hook(|_, _| true);
        host::Sts1Host::new(Some(dir.clone()), true, 3.0).launch(&format!("java -jar mts.jar --skip-launcher --mods {}", host::MODS));
    }
    let (argv, env) = spawned.borrow_mut().take().unwrap();
    assert!(argv.contains(&format!("{},{}", host::MODS, speedmod::MOD_ID)), "{argv:?}");
    assert_eq!(env.unwrap()[speedmod::ENV], "3");
    assert!(dir.join("game.log").exists());
    {
        let _ensure = speedmod::with_ensure_hook(|_, _| false);
        host::Sts1Host::new(Some(dir.clone()), true, 3.0).launch(&format!("java -jar mts.jar --mods {}", host::MODS));
    }
    let (_argv, env) = spawned.borrow_mut().take().unwrap();
    assert!(env.is_none()); // (no mod: the game as it is)
    assert!(dir.join("game.log.1").exists()); // (the launch before)
}

#[test]
fn test_communication_mod_is_pointed_at_the_relay() {
    let _case = sts1_case();
    let d = tempfile::tempdir().unwrap();
    let written = std::fs::read_to_string(host::write_comm_config(15599, Some(&d.path().join("CommunicationMod").join("config.properties"))).unwrap()).unwrap();
    assert!(written.contains(" relay --port 15599"), "{written}");
    assert!(written.contains("runAtGameStart=true"));
    assert!(written.contains("verbose=false")); // (every message echoed into game.log otherwise)
}
