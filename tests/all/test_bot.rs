use std::cell::RefCell;
use std::io::{Read, Write};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use indexmap::IndexMap;
use serde_json::{json, Value};
use sts2bot::bot::{Bot, BotArgs, BotError, Progress, SharedGame};
use sts2bot::cards::parse;
use sts2bot::client::{Client, Game, GameUnavailable, _dechunk};
use sts2bot::fight::combat::{self, SimulateArgs};
use sts2bot::logging::{self, Level, Record};
use sts2bot::py::JsonExt;
use sts2bot::run::events::EventOptionValueArgs;
use sts2bot::run::strategy::{self, Memory};
use sts2bot::run::decks;
use sts2bot::Action;

use crate::fake_game::{card, game_impl, with, FakeGame};
use crate::helpers::{combat_state, enemy, make_bot, make_bot_kw, FakeClock, DEFEND, STRIKE};

// TODO(supervisor): FastMode.test_sent_through_the_mod_and_tolerates_an_old_mod (supervisor.set_fast_mode)
// TODO(supervisor): CrashRecovery.test_a_crashed_child_is_not_counted_as_running (supervisor.game_running, supervisor._children)
// TODO(supervisor): CrashLoop.test_repeated_crashes_in_one_place_slow_the_bot_then_abandon_the_run (Supervisor._note_crash over a mock bot: `crashing`)
// TODO(supervisor): CrashLoop.test_while_there_is_something_else_to_try_the_run_is_not_given_up (the same)
// TODO(supervisor): SteamUpkeep.test_steam_is_started_before_the_game_when_it_is_not_running (supervisor.launch_game)
// TODO(supervisor): SteamUpkeep.test_under_systemd_the_game_gets_a_scope_of_its_own (supervisor._detached)
// TODO(supervisor): ModUpkeep.test_load_failure_is_read_from_the_game_log (supervisor.mod_load_error)
// TODO(supervisor): ModUpkeep.test_mod_is_stale_when_the_game_files_change (supervisor.mod_is_stale)
// TODO(supervisor): ModUpkeep.test_rebuilds_and_relaunches_when_the_game_rejects_the_mod (Supervisor.ensure_game)
// TODO(supervisor): ModUpkeep.test_rebuilds_before_launch_after_a_game_update (Supervisor.ensure_game)
// (No method of test_bot.py goes through the command line.)

const ICON: &str = "[ironclad_energy_icon.png]";

/// (The Python's defaults: hp 60, max_hp 80.)
fn event_value_at(title: &str, text: &str, hp: i64, max_hp: i64) -> f64 {
    strategy::_event_option_value(&json!({"title": title, "description": text}), hp, max_hp, EventOptionValueArgs::default())
}

fn event_value(title: &str, text: &str) -> f64 {
    event_value_at(title, text, 60, 80)
}

fn act(name: &str, params: Value) -> Action {
    (name.to_string(), params)
}

fn shared<G: Game + 'static>(game: G) -> Rc<RefCell<G>> {
    Rc::new(RefCell::new(game))
}

/// A bot whose waits take no time and leave its clock where it is. (`sleep`: the Python's default does nothing.)
fn quiet_bot(game: SharedGame, sleep: Option<Box<dyn Fn(f64)>>) -> Bot {
    Bot::new(game, BotArgs { clock: FakeClock::new().clock_fn(), sleep: Some(sleep.unwrap_or_else(|| Box::new(|_| {}))), ..Default::default() })
}

/// A fake game and its bot, stopped at the first fight and back on the main menu, as after a game restart.
fn restarted(game: FakeGame) -> (Rc<RefCell<FakeGame>>, Bot) {
    let game = game.shared();
    let mut bot = make_bot(game.clone());
    while game.borrow().screen != "combat" {
        bot.step().unwrap();
    }
    {
        let mut g = game.borrow_mut();
        (g.saved_screen, g.screen) = ("combat".to_string(), "main".to_string());
    }
    bot.forget(); // (as the supervisor's restart does: bot.run starts from a fresh look)
    (game, bot)
}

/// `with self.assertLogs(logger, level=...) as logs`: what `f` returned and the records at that level or above, as `logs.output`
/// has them (at least one, or the assertion fails).
fn assert_logs<T>(logger: &str, level: Level, f: impl FnOnce() -> T) -> (T, Vec<String>) {
    let (out, records) = logging::capture(f);
    let said: Vec<String> = records.iter().filter(|r: &&Record| r.level >= level && (r.logger == logger || r.logger.starts_with(&format!("{logger}.")))).map(|r| r.output()).collect();
    assert!(!said.is_empty(), "no logs of level {} or higher triggered on {}", level.name(), logger);
    (out, said)
}

/// A game that only shows its profile: `wins` on the win counter, one more from the `late`-th look on (0: never).
struct Wins {
    wins: i64,
    late: i64,
    asked: i64,
}

impl Wins {
    fn new(wins: i64, late: i64) -> Rc<RefCell<Wins>> {
        shared(Wins { wins, late, asked: 0 })
    }
}

impl Game for Wins {
    fn state(&mut self) -> Result<Value, GameUnavailable> {
        panic!("'Wins' object has no attribute 'state'")
    }
    fn act(&mut self, _action: &str, _params: &Value) -> Result<Value, GameUnavailable> {
        panic!("'Wins' object has no attribute 'act'")
    }
    fn profile(&mut self) -> Result<Value, GameUnavailable> {
        self.asked += 1;
        Ok(json!({"total_wins": self.wins + (self.late != 0 && self.asked >= self.late) as i64, "characters": []}))
    }
    fn card(&mut self, _card_id: &str) -> Result<Option<Value>, GameUnavailable> {
        Ok(None)
    }
    fn ping(&mut self) -> bool {
        true
    }
}

// FullLoop

/// While the travel animation plays the map still answers; wait it out.
#[test]
fn test_map_travel_is_not_clicked_twice() {
    #[derive(Default)]
    struct TravellingMap {
        clicks: i64,
        polls_left: i64,
    }
    impl TravellingMap {
        fn state(&mut self) -> Value {
            if self.clicks != 0 && self.polls_left <= 0 {
                return json!({"state_type": "event", "event": {"in_dialogue": true}});
            }
            self.polls_left -= 1;
            let opts = json!([{"index": 0, "col": self.clicks, "row": 1, "type": "Monster"}]);
            json!({"state_type": "map", "map": {"next_options": opts, "nodes": []}, "player": {"hp": 50, "max_hp": 80}})
        }
        fn act(&mut self, action: &str, _params: &Value) -> Value {
            if action == "choose_map_node" {
                (self.clicks, self.polls_left) = (self.clicks + 1, 5);
            }
            json!({"status": "ok"})
        }
    }
    game_impl!(TravellingMap);

    let game = shared(TravellingMap::default());
    make_bot(game.clone()).step().unwrap();
    assert_eq!(game.borrow().clicks, 1);
    assert_eq!(game.borrow_mut().state()["state_type"], "event");
}

#[test]
fn test_a_potion_is_not_one_of_the_cards_a_turn_allows() {
    let mut bot = make_bot(FakeGame::default().shared());
    let mut state = combat_state!([STRIKE(0), DEFEND(1)], [enemy!("KNOWLEDGE_DEMON_0", 200, attack = 30)], hp = 38);
    state["player"]["status"] = json!([{"id": "SLOTH_POWER", "amount": 0}]); // (the number is the game's count of cards so far)
    bot._combat(&state);
    bot._after_success(&state, "use_potion", &json!({"slot": 0}));
    bot._after_success(&state, "play_card", &json!({"card_index": 0}));
    bot._after_success(&state, "play_card", &json!({"card_index": 0}));
    state["player"]["status"][0]["amount"] = json!(2);
    bot._played_from = Value::Null; // (the hand as it was when the last card left it: not what this test is about)
    assert_eq!(bot._combat(&state)[0].0, "play_card"); // the third card of three, potion or no potion
    bot._after_success(&state, "play_card", &json!({"card_index": 0}));
    state["player"]["status"][0]["amount"] = json!(3);
    bot._played_from = Value::Null;
    assert_eq!(bot._combat(&state), vec![]); // (one more look before ending a turn with energy left)
    assert_eq!(bot._combat(&state)[0].0, "end_turn");
}

#[test]
fn test_a_choice_in_a_fight_is_told_how_its_turns_went() {
    // (The Knowledge Demon's offers: what Sloth or an energy less would cost shows in the turns played so far.)
    let mut bot = make_bot(FakeGame::default().shared());
    let mut state = combat_state!([STRIKE(0), DEFEND(1)], [enemy!("KNOWLEDGE_DEMON_0", 300, attack = 17)], kind = "boss");
    for (rnd, (cards_played, left)) in [(4, 0), (5, 2)].into_iter().enumerate() {
        (state["battle"]["round"], state["player"]["energy"]) = (json!(rnd + 1), json!(3));
        bot._combat(&state);
        for _ in 0..cards_played {
            bot._after_success(&state, "play_card", &json!({"card_index": 0}));
        }
        (state["player"]["energy"], bot._played_from) = (json!(left), Value::Null);
        bot._combat(&state);
    }
    let seen: Rc<RefCell<Value>> = Rc::new(RefCell::new(json!({})));
    let offer = json!({"state_type": "card_select", "run": state["run"], "player": state["player"], "card_select": {"screen_type": "choose", "cards": []}});
    {
        let noted = seen.clone();
        let _patch = strategy::with_card_select_hook(move |s, _mem| {
            let merged = with(noted.borrow().clone(), s.clone());
            *noted.borrow_mut() = merged;
            vec![]
        });
        bot.candidates(&offer);
        assert_eq!(seen.borrow()["_turns"], json!([[4, 0], [5, 2]]));
        *seen.borrow_mut() = json!({});
        bot.candidates(&with(offer.clone(), json!({"run": {"act": 1, "floor": 9}}))); // some other room's screen: no fight to speak of
        assert!(!seen.borrow().has("_turns"));
    }
    state["run"] = json!({"act": 1, "floor": 4, "ascension": 0}); // the next fight starts from nothing
    state["battle"]["round"] = json!(1);
    bot._combat(&state);
    assert_eq!(bot._turns, vec![]);
}

#[test]
fn test_what_each_enemy_has_done_so_far_is_remembered_for_the_fight() {
    let threat = |rows: [(&str, (i64, i64, f64, i64)); 2]| -> IndexMap<String, (i64, i64, f64, i64)> { rows.into_iter().map(|(k, v)| (k.to_string(), v)).collect() };
    let mut bot = make_bot(FakeGame::default().shared());
    let mut state = combat_state!([STRIKE(0)], [enemy!("BUG_0", 46, attack = 15), enemy!("BEETLE_0", 86)]);
    bot._combat(&state);
    bot._combat(&state); // the same turn seen again counts once
    assert_eq!(bot._threat, threat([("BUG_0", (15, 1, 15.0, 1)), ("BEETLE_0", (0, 1, 0.0, 0))]));
    state["battle"]["round"] = json!(2);
    state["battle"]["enemies"] = json!([enemy!("BUG_0", 46), enemy!("BEETLE_0", 86, attack = 16)]);
    bot._combat(&state);
    assert_eq!(bot._threat, threat([("BUG_0", (15, 2, 15.0, 1)), ("BEETLE_0", (16, 2, 16.0, 1))])); // (damage, rounds, blows, turns attacked)
    state["run"]["floor"] = json!(4); // the next fight starts from nothing
    state["battle"]["round"] = json!(1);
    bot._combat(&state);
    assert_eq!(bot._threat, threat([("BUG_0", (0, 1, 0.0, 0)), ("BEETLE_0", (16, 1, 16.0, 1))]));
}

/// The shop said yes to a potion a relic forbids; the bot must leave in the end.
#[test]
fn test_a_purchase_that_never_happens_is_given_up() {
    #[derive(Default)]
    struct DeafShop {
        buys: i64,
        left: bool,
    }
    impl DeafShop {
        fn state(&mut self) -> Value {
            if self.left {
                return json!({"state_type": "map", "map": {"next_options": [], "nodes": []}});
            }
            let items = json!([{"index": 0, "category": "relic", "price": 50, "is_stocked": true, "can_afford": true}]);
            json!({"state_type": "shop", "shop": {"items": items, "can_proceed": true}, "player": {"hp": 50, "max_hp": 80, "gold": 300}, "run": {"act": 1, "floor": 5}})
        }
        fn act(&mut self, action: &str, _params: &Value) -> Value {
            self.buys += (action == "shop_purchase") as i64;
            self.left = self.left || action == "proceed";
            json!({"status": "ok"})
        }
    }
    game_impl!(DeafShop);

    let game = shared(DeafShop::default());
    let mut bot = make_bot(game.clone());
    for _ in 0..40 {
        bot.step().unwrap();
        if game.borrow().left {
            break;
        }
    }
    assert!(game.borrow().left);
    assert!(game.borrow().buys <= 2);
}

/// The game resets its own time scale in boss fights; set once, x3 was gone for the session.
#[test]
fn test_the_game_speed_is_set_again_as_the_fight_goes_on() {
    #[derive(Default)]
    struct Idle {
        scales: Vec<Value>,
    }
    impl Idle {
        fn state(&mut self) -> Value {
            json!({"state_type": "unknown"})
        }
        fn act(&mut self, action: &str, params: &Value) -> Value {
            if action == "set_time_scale" {
                self.scales.push(params["scale"].clone());
            }
            json!({"status": "ok"})
        }
    }
    game_impl!(Idle);

    let game = shared(Idle::default());
    let mut bot = make_bot(game.clone());
    bot.time_scale = 3.0;
    for _ in 0..60 {
        bot.step().unwrap();
    }
    assert!(game.borrow().scales.len() > 1);
    assert!(game.borrow().scales.iter().all(|s| *s == json!(3.0)));
}

#[test]
fn test_plays_several_runs_back_to_back() {
    let game = FakeGame::seeded(1, 4).shared();
    let d = tempfile::tempdir().unwrap();
    let mut bot = make_bot_kw(game.clone(), BotArgs { log_dir: Some(d.path().to_path_buf()), ..Default::default() });
    bot.run(Some(3)).unwrap();
    let lines: Vec<Value> = std::fs::read_to_string(d.path().join("runs.jsonl")).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
    assert_eq!(bot.runs_finished, 3);
    let game = game.borrow();
    assert_eq!(game.runs_started, 3);
    assert_eq!(game.actions.last().unwrap(), &act("menu_select", json!({"option": "main_menu"})));
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|line| line["result"] == "win" && line["floor"].int() >= 4));
    assert_eq!(game.screen, "main");
    for name in ["play_card", "end_turn", "claim_reward", "select_card_reward", "choose_map_node", "choose_rest_option"] {
        assert!(game.actions.iter().any(|(a, _)| a == name), "{name}");
    }
}

#[test]
fn test_skipped_card_reward_does_not_loop() {
    let game = FakeGame::seeded(2, 3).shared();
    {
        let _bar = decks::SKIP_THRESHOLD.set(99.0); // (the bar for a card reward: skip everything)
        make_bot(game.clone()).run(Some(1)).unwrap();
    }
    let game = game.borrow();
    let skips = game.actions.iter().filter(|(a, _)| a == "skip_card_reward").count();
    let offered = game.actions.iter().filter(|a| **a == act("claim_reward", json!({"index": 1}))).count();
    assert!(skips >= 1);
    assert_eq!(skips, offered); // each card reward opened once, never re-opened
    assert_eq!(game.deck.len(), 10);
}

#[test]
fn test_loses_a_run_and_starts_again() {
    let mut bot = make_bot(FakeGame::new(3, 50, 70, 22).shared());
    bot.run(Some(2)).unwrap();
    assert_eq!((bot.runs_finished, bot.wins), (2, 0));
}

#[test]
fn test_resumes_a_saved_run_from_the_menu() {
    let (game, mut bot) = restarted(FakeGame::seeded(4, 3));
    bot.run(Some(1)).unwrap();
    assert_eq!(game.borrow().runs_started, 1);
    assert!(game.borrow().actions.contains(&act("menu_select", json!({"option": "continue"}))));
}

#[test]
fn test_abandons_when_told_to() {
    let (game, mut bot) = restarted(FakeGame::seeded(5, 4));
    bot.abandon_next = true;
    for _ in 0..6 {
        bot.step().unwrap();
    }
    assert!(!bot.abandon_next);
    assert_eq!(game.borrow().runs_started, 2);
}

#[test]
fn test_raises_stuck_when_nothing_works() {
    #[derive(Default)]
    struct Frozen {
        actions: Vec<(String, Value)>,
    }
    impl Frozen {
        fn state(&mut self) -> Value {
            json!({"state_type": "overlay", "run": {"act": 1, "floor": 2}, "player": {"hp": 5, "max_hp": 80}, "overlay": {"screen_type": "NWeird"}})
        }
        fn act(&mut self, action: &str, p: &Value) -> Value {
            self.actions.push((action.to_string(), p.clone()));
            json!({"status": "error", "error": "nope"})
        }
    }
    game_impl!(Frozen);

    let mut bot = make_bot_kw(shared(Frozen::default()), BotArgs { stuck_timeout: 120.0, ..Default::default() });
    let mut raised = None;
    for _ in 0..10_000 {
        if let Err(e) = bot.step() {
            raised = Some(e);
            break;
        }
    }
    assert!(matches!(raised, Some(BotError::Stuck(_))), "{raised:?}");
}

// Pace
// 2026-11-25: a fight round took about 1 s, of which the enemy's turn 0.15 s; the bot's own waits were the rest.

#[test]
fn test_a_fight_with_nothing_to_send_is_looked_at_again_soon() {
    struct EnemyTurn(Value);
    impl EnemyTurn {
        fn state(&mut self) -> Value {
            self.0.clone()
        }
        fn act(&mut self, _action: &str, _params: &Value) -> Value {
            panic!("'Game' object has no attribute 'act'")
        }
    }
    game_impl!(EnemyTurn);

    let enemy_turn = with(
        combat_state!([STRIKE(0)], [enemy!("JAW_WORM_0", 40, attack = 11)]),
        json!({"battle": {"round": 2, "turn": "enemy", "is_play_phase": false, "enemies": [enemy!("JAW_WORM_0", 40, attack = 11)]}}),
    );
    let slept: Rc<RefCell<Vec<f64>>> = Rc::new(RefCell::new(Vec::new()));
    let noted = slept.clone();
    let mut bot = Bot::new(shared(EnemyTurn(enemy_turn)), BotArgs { clock: FakeClock::new().clock_fn(), sleep: Some(Box::new(move |s| noted.borrow_mut().push(s))), ..Default::default() });
    bot.step().unwrap();
    assert_eq!(*slept.borrow(), vec![bot.poll.min(bot.fight_poll)]);
    assert!(bot.fight_poll < bot.poll);
}

#[test]
fn test_the_state_the_wait_ended_on_is_the_next_steps() {
    let game = FakeGame::seeded(6, 4).shared(); // (its `asked` counts the looks at the state)
    let mut bot = make_bot(game.clone());
    while game.borrow().screen != "combat" {
        bot.step().unwrap();
    }
    let before = game.borrow().asked;
    bot.step().unwrap(); // a card played: the wait asks until it shows ...
    let waited = game.borrow().asked - before;
    bot.step().unwrap(); // ... and the next play starts from what it saw
    assert_eq!(game.borrow().asked - before, waited + 1); // (its own wait's one look, no fresh one before it)
}

// LiveGameLessons
// Things the first sessions against the real game got wrong.

#[test]
fn test_card_text_is_read_as_the_game_means_it() {
    // Energy is written as icons; what a card does later is not done now; Weak and Vulnerable can come together.
    for (c, fields, expected) in [
        (card(0, "OFFERING", "Offering", "Skill", 0, &format!("Lose 6 HP. Gain {ICON}{ICON}. Draw 3 cards. Exhaust."), "None"), vec!["energy_gain", "draw", "hp_loss"], json!([2, 3, 6])),
        (card(0, "JUGGERNAUT", "Juggernaut", "Power", 2, "Whenever you gain Block, deal 6 damage to a random enemy.", "None"), vec!["damage"], json!([0])),
        (card(0, "FLAME_BARRIER", "Flame Barrier", "Skill", 2, "Gain 12 Block. Whenever you are attacked this turn, deal 4 damage back.", "None"), vec!["block", "damage"], json!([12, 0])),
        (card(0, "THE_BOMB", "The Bomb", "Skill", 2, "At the end of 3 turns, deal 40 damage to ALL enemies.", "None"), vec!["damage"], json!([0])),
        (card(0, "SHOCKWAVE", "Shockwave", "Skill", 2, "Apply 3 Weak and Vulnerable to ALL enemies. Exhaust.", "None"), vec!["weak", "vulnerable", "aoe"], json!([3, 3, true])),
    ] {
        let info = parse(&c).to_json();
        assert_eq!(Value::Array(fields.iter().map(|f| info[*f].clone()).collect()), expected, "{}", c["name"]);
    }
}

#[test]
fn test_strength_this_turn_adds_to_later_attacks() {
    let setup = card(0, "SETUP_STRIKE", "Setup Strike", "Attack", 1, "Deal 7 damage. Gain 3 Strength this turn.", "AnyEnemy");
    let foes = combat::read_enemies(&combat_state!([], [enemy!("A", 100)]));
    let ctx = || SimulateArgs { energy: 3, stars: 0, hp: 80, max_hp: 80, block: 0, ..Default::default() };
    let with_setup = combat::simulate(&[parse(&setup), parse(&STRIKE(1)), parse(&STRIKE(2))], &foes, ctx()).unwrap().score;
    let plain = combat::simulate(&[parse(&STRIKE(0)), parse(&STRIKE(1)), parse(&STRIKE(2))], &foes, ctx()).unwrap().score;
    assert!(((with_setup - plain) - (1 + 3 + 3) as f64).abs() < 5e-8);
}

#[test]
fn test_demon_form_is_played_early_in_a_boss_fight() {
    let demon = card(0, "DEMON_FORM", "Demon Form", "Power", 3, "At the start of your turn, gain 3 Strength.", "None");
    let state = combat_state!([demon, STRIKE(1), STRIKE(2), STRIKE(3)], [enemy!("BOSS", 250)], kind = "boss");
    assert_eq!(combat::choose_card(&state, None), Some(act("play_card", json!({"card_index": 0}))));
}

#[test]
fn test_sapping_strength_counts_as_defence() {
    let shackles = card(0, "DARK_SHACKLES", "Dark Shackles", "Skill", 0, "Enemy loses 9 Strength this turn. Exhaust.", "AnyEnemy");
    let mut foe = enemy!("A", 100);
    foe["intents"] = json!([{"type": "Attack", "label": "10x3"}]);
    assert_eq!(combat::choose_card(&combat_state!([shackles], [foe], energy = 0, hp = 20), None), Some(act("play_card", json!({"card_index": 0, "target": "A"}))));
}

#[test]
fn test_never_trades_max_hp_away() {
    let best = |options: &[(&str, &str)]| -> Value {
        let opts: Vec<Value> = options.iter().enumerate().map(|(i, (t, d))| json!({"index": i, "title": t, "description": d})).collect();
        let state = json!({"state_type": "event", "event": {"options": opts}, "player": {"hp": 70, "max_hp": 80}});
        strategy::event(&state, &mut Memory::new())[0].1["index"].clone()
    };
    assert_eq!(best(&[("Lose Everything", "???"), ("Give Up", "Stop reading the text and leave.")]), 1);
    assert_eq!(best(&[("Continue", "Lose 24 Max HP. Upgrade 2 random cards."), ("Give Up", "Leave.")]), 1);
    assert_eq!(best(&[("Decipher", "Lose 3 Max HP. Upgrade a random card."), ("Give Up", "Leave.")]), 0);
}

#[test]
fn test_an_event_that_keeps_costing_hp_is_left_after_a_few_bites() {
    // Slippery Bridge: "Hold On" costs 3, 4, 5, ... HP each time. Priced one bite at a time it always looked cheaper than
    // losing a card, and one run lost 42 HP there.
    let (mut mem, mut hp, mut picks) = (Memory::new(), 71, Vec::new());
    for loss in [3, 4, 5] {
        let opts = json!([{"index": 0, "title": "Overcome", "description": "Shining Strike is removed from your Deck."},
                          {"index": 1, "title": "Hold On", "description": format!("Lose {loss} HP. The card in the above option is randomized.")}]);
        let state = json!({"state_type": "event", "event": {"event_id": "SLIPPERY_BRIDGE", "options": opts}, "player": {"hp": hp, "max_hp": 75}});
        picks.push(strategy::event(&state, &mut mem)[0].1["index"].int());
        if *picks.last().unwrap() == 1 {
            hp -= loss;
        }
    }
    assert_eq!(picks, vec![1, 1, 0]);
}

#[test]
fn test_bridge_event_gives_up_a_starter_card_not_a_good_one() {
    let hold = event_value("Hold On", "Lose 5 HP. The card in the above option is randomized.");
    assert!(event_value("Overcome", "Defend is removed from your Deck.") > hold);
    assert!(event_value("Overcome", "Demon Form+ is removed from your Deck.") < hold);
}

#[test]
fn test_one_good_potion_beats_three_foul_ones() {
    assert!(event_value("Ransack", "Procure 1 random Uncommon Potion.") > event_value("Grab Potions", "Procure 3 Foul Potions."));
}

#[test]
fn test_the_lantern_key_is_given_back_for_gold() {
    assert!(event_value("Return the Key", "Gain 100 Gold.") > event_value("Keep the Key", "Fight to obtain the Key."));
}

#[test]
fn test_an_event_never_doubles_the_deck_or_hands_out_a_curse_as_a_relic() {
    // Reflections: "Receive Bad Luck" was scored as a relic and "Duplicate your Deck" went unpenalised; one run doubled its deck to 71.
    let shatter = event_value_at("Shatter", "Duplicate your Deck. Receive Bad Luck.", 69, 105);
    let mirror = event_value_at("Touch a Mirror", "Downgrade 2 random cards. Upgrade 4 random cards.", 69, 105);
    assert!(shatter < 0.0);
    assert!(mirror > shatter);
    assert!(event_value_at("Obtain the Spoils Map", "A relic.", 69, 105) > 3.0); // a real relic by name still counts
}

#[test]
fn test_a_curse_costs_what_it_does_and_a_relic_is_worth_what_it_gives() {
    // Clumsy is one dead draw a fight: a relic is worth it (refused 45 times in 45 for 6 HP and 52 Gold).
    assert!(event_value("That", "Add Clumsy to your Deck. Obtain a random Relic.") > event_value("This", "Lose 6 HP. Gain 52 Gold."));
    assert!(event_value("Resist", "Remove 2 cards from your Deck. Add Normality to your Deck.") < 0.0);
    // Royal Poison takes 4 HP at the start of every combat: not for a heal we hardly need, only for our life.
    let tea = |hp: i64| event_value_at("Enjoy Your Tea", "Obtain Royal Poison. Heal to full HP.", hp, 80);
    let fight = |hp: i64| event_value_at("Pick a Fight", "Lose 11 HP. Obtain a random Relic.", hp, 80);
    assert!(tea(70) < fight(70));
    assert!(tea(12) > fight(12));
    // The Sword of Stone needs five elites killed, and the bot meets about one a run.
    assert!(event_value_at("Grab the Sword", "Obtain the Sword of Stone.", 80, 80) < event_value_at("Dive into the Water", "Gain 115 Gold. Lose 7 HP.", 80, 80));
    // The Punch Off fight (two constructs that took 16, 37 and 44 HP, one a death) costs its HP: a relic with an Injury (Nab)
    // is taken instead, even at 60 of 80.
    assert!(event_value("I Can Take Them", "Fight them for Greater Rewards.") < event_value("Nab", "Add Injury to your Deck. Obtain a random Relic."));
}

#[test]
fn test_a_heal_gold_and_max_hp_are_worth_their_amounts() {
    let value = |text: &str, hp: i64| event_value_at("", text, hp, 80);
    assert!(value("Heal 21 HP.", 50) > value("Heal 21 HP.", 75));
    assert!((value("Heal to full HP.", 80) - value("Leave.", 60)).abs() < 5e-8);
    assert!(value("Gain 311 Gold.", 60) > value("Gain 62 Gold.", 60) + 2.0);
    assert!(value("Heal 10 HP.", 53) > value("Gain 2 Max HP. Take 3 damage.", 53));
    assert!(value("Gain 10 Max HP.", 60) > value("Gain 2 Max HP.", 60));
}

#[test]
fn test_picks_the_mildest_boss_affliction() {
    let cards_ = json!([card(0, "DISINTEGRATION", "Disintegration", "Status", 0, "At the end of your turn, take 6 damage.", "None"),
                        card(1, "SLOTH", "Sloth", "Status", 0, "You cannot play more than 3 cards each turn.", "None"), card(2, "WASTE_AWAY", "Waste Away", "Status", 0, "Gain 1 less energy per turn.", "None")]);
    let state = json!({"state_type": "card_select", "card_select": {"screen_type": "choose", "prompt": "Choose a card.", "cards": cards_}});
    assert_eq!(strategy::card_select(&state, &mut Memory::new())[0], act("select_card", json!({"index": 1})));
}

fn odd_card() -> Value {
    with(card(0, "NEW_THING", "New Thing", "Skill", 1, "Do something the bot has never seen.", "None"), json!({"rarity": "Rare"}))
}

#[test]
fn test_unreadable_cards_are_not_taken_for_their_rarity() {
    let state = json!({"state_type": "card_reward", "run": {"act": 2}, "card_reward": {"cards": [odd_card()], "can_skip": true}});
    assert_eq!(strategy::card_reward(&state, &mut Memory::new())[0].0, "skip_card_reward");
}

#[test]
fn test_a_card_offered_in_a_fight_is_always_taken_and_is_no_pick() {
    // (2026-11-25: an Attack Potion on round 1 of Donu and Deca, its three cards skipped as a reward below the bar.)
    let odd = odd_card();
    let state = json!({"state_type": "card_reward", "run": {"act": 2}, "card_reward": {"cards": [odd], "can_skip": true, "in_combat": true}});
    assert_eq!(strategy::card_reward(&state, &mut Memory::new())[0], act("select_card_reward", json!({"card_index": 0})));
    let mut bot = make_bot(FakeGame::default().shared());
    let mut shown = state.clone();
    shown["card_reward"]["cards"] = json!([with(odd, json!({"index": 0}))]);
    bot._after_success(&shown, "select_card_reward", &json!({"card_index": 0}));
    assert_eq!((bot.mem.deck.clone(), bot.mem.picks.clone()), (vec![], vec![]));
}

#[test]
fn test_a_burrowed_enemy_is_dug_out_instead_of_blocked_forever() {
    let burrowed = json!([{"id": "BURROWED_POWER", "amount": 1}]);
    let hand = [DEFEND(0), DEFEND(1), DEFEND(2), card(3, "BASH", "Bash", "Attack", 2, "Deal 11 damage. Apply 2 Vulnerable.", "AnyEnemy")];
    let mut state = combat_state!(hand.clone(), [enemy!("T", 62, attack = 24, block = 20, status = burrowed.clone())], hp = 57);
    state["battle"]["round"] = json!(25);
    assert_eq!(combat::choose_card(&state, None), Some(act("play_card", json!({"card_index": 3, "target": "T"}))));
    // Breaking the last of its Block stuns it, so that comes before blocking.
    let state = combat_state!(hand, [enemy!("T", 62, attack = 24, block = 9, status = burrowed)], hp = 57);
    assert_eq!(combat::choose_card(&state, None), Some(act("play_card", json!({"card_index": 3, "target": "T"}))));
}

#[test]
fn test_escapes_the_sandpit_before_it_closes() {
    let escape = card(0, "FRANTIC_ESCAPE", "Frantic Escape", "Status", 1, "Get farther away. Increase Sandpit by 1. Increase the cost of this card by 1.", "None");
    let boss = enemy!("INS", 200, attack = 20, status = json!([{"id": "SANDPIT_POWER", "amount": 1}]));
    assert_eq!(combat::choose_card(&combat_state!([escape, DEFEND(1), DEFEND(2), DEFEND(3)], [boss], kind = "boss"), None), Some(act("play_card", json!({"card_index": 0}))));
}

#[test]
fn test_kills_the_minion_that_does_all_the_damage() {
    let minion = enemy!("TORCH", 199, attack = 28, status = json!([{"id": "MINION_POWER", "amount": 1}]));
    let state = combat_state!([STRIKE(0)], [minion, enemy!("QUEEN", 400)], kind = "boss", energy = 1);
    assert_eq!(combat::choose_card(&state, None), Some(act("play_card", json!({"card_index": 0, "target": "TORCH"}))));
    // Small fry beside a leader that fights for itself are still ignored.
    let weak = enemy!("FOLLOWER", 59, attack = 5, status = json!([{"id": "MINION_POWER", "amount": 1}]));
    let state = combat_state!([STRIKE(0)], [weak, enemy!("PRIEST", 190, attack = 8)], kind = "boss", energy = 1);
    assert_eq!(combat::choose_card(&state, None).unwrap().1["target"], "PRIEST");
}

/// Surrounded: the enemy behind us hits 50% harder; aiming a card at it turns us round.
#[test]
fn test_turns_to_face_the_enemy_about_to_hit_hard() {
    let crusher = enemy!("CRUSHER", 200, attack = 6, status = json!([{"id": "BACK_ATTACK_LEFT_POWER", "amount": 1}]));
    let rocket = enemy!("ROCKET", 150, attack = 27, status = json!([{"id": "BACK_ATTACK_RIGHT_POWER", "amount": 1}]));
    let mut state = combat_state!([STRIKE(0)], [crusher, rocket], kind = "boss", energy = 1, hp = 40);
    state["player"]["status"] = json!([{"id": "SURROUNDED_POWER", "amount": 1}]);
    state["_facing"] = json!("CRUSHER"); // so Rocket's 27 is its attack from behind
    assert_eq!(combat::choose_card(&state, None), Some(act("play_card", json!({"card_index": 0, "target": "ROCKET"}))));
    assert_eq!(combat::read_enemies(&state).iter().map(|e| (e.entity_id.to_string(), e.behind)).collect::<Vec<_>>(), vec![("CRUSHER".to_string(), false), ("ROCKET".to_string(), true)]);
}

#[test]
fn test_hard_to_kill_caps_each_hit() {
    let big = card(0, "BLUDGEON", "Bludgeon", "Attack", 3, "Deal 32 damage.", "AnyEnemy");
    let twin = card(1, "TWIN_STRIKE", "Twin Strike", "Attack", 1, "Deal 5 damage twice.", "AnyEnemy");
    let state = combat_state!([big, twin, STRIKE(2), STRIKE(3)], [enemy!("EXO", 28, status = json!([{"id": "HARD_TO_KILL_POWER", "amount": 9}]))]);
    assert_ne!(combat::choose_card(&state, None).unwrap().1["card_index"], 0);
}

#[test]
fn test_block_potion_waits_for_a_big_hit() {
    let potion = json!({"slot": 0, "id": "BLOCK_POTION", "name": "Block Potion", "description": "Gain 12 Block.", "target_type": "Self"});
    let calm = combat_state!([STRIKE(0)], [enemy!("BOSS", 300)], kind = "boss", potions = [potion.clone()]);
    assert!(!combat::actions(&calm).iter().any(|(a, _)| a == "use_potion"));
    let storm = combat_state!([STRIKE(0)], [enemy!("BOSS", 300, attack = 30)], kind = "boss", potions = [potion]);
    assert_eq!(combat::actions(&storm)[0], act("use_potion", json!({"slot": 0})));
}

#[test]
fn test_only_one_bound_card_is_planned_per_turn() {
    let bound = json!({"keywords": [{"name": "Bound", "description": "Only 1 Bound card can be played each turn."}]});
    let hand = [
        with(card(0, "DEFEND_R", "Defend", "Skill", 1, "Gain 5 Block.", "Self"), bound.clone()),
        with(card(1, "DEFEND_R", "Defend", "Skill", 1, "Gain 5 Block.", "Self"), bound.clone()),
        with(card(2, "IMPERVIOUS", "Impervious", "Skill", 2, "Gain 30 Block. Exhaust.", "Self"), bound),
    ];
    // Without the rule the plan would be Defend + Impervious; the first Defend played would then lock Impervious out and the hit would land.
    assert_eq!(combat::choose_card(&combat_state!(hand, [enemy!("A", 200, attack = 30)], hp = 40), None), Some(act("play_card", json!({"card_index": 2}))));
}

#[test]
fn test_two_cards_together_kill_the_reviving_attacker() {
    let ghost = enemy!("PARAFRIGHT", 21, attack = 16, status = json!([{"id": "ILLUSION_POWER", "amount": 1}, {"id": "MINION_POWER", "amount": 1}]));
    let bash = card(0, "BASH", "Bash", "Attack", 2, "Deal 8 damage. Apply 2 Vulnerable.", "AnyEnemy");
    let twin = card(1, "TWIN_STRIKE", "Twin Strike+", "Attack", 1, "Deal 7 damage twice.", "AnyEnemy");
    let (action, params) = combat::choose_card(&combat_state!([bash, twin, DEFEND(2)], [ghost, enemy!("OBSCURA", 101, attack = 10)], hp = 18), None).unwrap();
    assert_eq!((action.as_str(), &params["target"]), ("play_card", &json!("PARAFRIGHT")));
}

#[test]
fn test_a_win_is_read_from_the_games_own_counter() {
    let game = Wins::new(4, 0);
    let mut bot = quiet_bot(game.clone(), None);
    bot._refresh_ascensions(); // run start: remembers the counter
    let over = json!({"state_type": "game_over", "run": {"act": 3, "floor": 48}, "player": {"hp": 0, "max_hp": 80}});
    game.borrow_mut().wins = 5;
    let ((), logs) = assert_logs("sts2bot", Level::Info, || bot._record_run(&over, false));
    assert!(logs[0].contains("win"));
    assert_eq!(bot.wins, 1);
    let ((), logs) = assert_logs("sts2bot", Level::Info, || bot._record_run(&with(over.clone(), json!({"player": {"hp": 12, "max_hp": 80}})), false)); // counter unchanged: a loss
    assert!(logs[0].contains("loss"));
}

#[test]
fn test_a_win_the_game_has_not_written_down_yet_is_waited_for() {
    let mut bot = quiet_bot(Wins::new(2, 5), None);
    bot._refresh_ascensions();
    let ((), logs) = assert_logs("sts2bot", Level::Info, || bot._record_run(&json!({"state_type": "game_over", "run": {"act": 3, "floor": 48}, "player": {"hp": 3, "max_hp": 83}}), false));
    assert!(logs.last().unwrap().contains("win"));
    assert_eq!(bot.wins, 1);
}

#[test]
fn test_a_death_straight_out_of_a_fight_is_not_waited_on() {
    // Every act 3 death waited 4 s for a win that could not come (4.1 s from the last play; wins take 1-2 s).
    let (game, slept) = (Wins::new(2, 0), Rc::new(RefCell::new(Vec::<f64>::new())));
    let noting = || -> Option<Box<dyn Fn(f64)>> {
        let slept = slept.clone();
        Some(Box::new(move |s| slept.borrow_mut().push(s)))
    };
    let mut bot = quiet_bot(game.clone(), noting());
    bot._refresh_ascensions();
    let over = json!({"state_type": "game_over", "run": {"act": 3, "floor": 48}, "player": {"hp": 0, "max_hp": 80}});
    bot._note_state(&json!({"state_type": "boss", "run": {"act": 3, "floor": 48}, "player": {"hp": 4, "max_hp": 80}, "battle": {"round": 9, "enemies": []}}));
    let ((), logs) = assert_logs("sts2bot", Level::Info, || bot._note_state(&over));
    assert!(logs.iter().any(|line| line.contains("run 1 over: loss")), "{logs:?}");
    assert_eq!(*slept.borrow(), Vec::<f64>::new()); // no waiting on the counter
    // ... while one that leaves its fight first (a won last boss) is still waited for.
    let mut bot = quiet_bot(game, noting());
    bot._refresh_ascensions();
    bot._note_state(&json!({"state_type": "rewards", "run": {"act": 3, "floor": 48}, "player": {"hp": 30, "max_hp": 80}}));
    assert_logs("sts2bot", Level::Info, || bot._note_state(&over));
    assert_eq!(slept.borrow().len(), 8);
}

#[test]
fn test_with_the_timeline_complete_a_death_skips_the_summary() {
    let over = json!({"state_type": "game_over", "game_over": {"options": ["continue", "force_main_menu"]}});
    let epochs: IndexMap<String, String> = [("NEOW_EPOCH", "Revealed"), ("FOOL1_EPOCH", "Revealed")].into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    let mut mem = Memory { epochs, last_result: "loss".to_string(), ..Memory::new() };
    assert_eq!(strategy::game_over(&over, &mut mem), vec![act("menu_select", json!({"option": "force_main_menu"}))]);
    mem.last_result = "win".to_string();
    assert_eq!(strategy::game_over(&over, &mut mem), vec![act("menu_select", json!({"option": "continue"}))]); // (a win: the summary)
    mem.last_result = "loss".to_string();
    mem.epochs.insert("FOOL2_EPOCH".to_string(), "NotObtained".to_string());
    assert_eq!(strategy::game_over(&over, &mut mem), vec![act("menu_select", json!({"option": "continue"}))]); // (score still buys epochs)
}

#[test]
fn test_a_bot_started_mid_run_still_sees_that_run_won() {
    let game = Wins::new(3, 0);
    let mut bot = quiet_bot(game.clone(), None);
    bot._note_state(&json!({"state_type": "map", "run": {"act": 3, "floor": 40}, "player": {"hp": 50, "max_hp": 80}}));
    game.borrow_mut().wins = 4;
    let ((), logs) = assert_logs("sts2bot", Level::Info, || bot._note_state(&json!({"state_type": "game_over", "run": {"act": 3, "floor": 48}, "player": {"hp": 0, "max_hp": 80}})));
    assert!(logs.iter().any(|line| line.contains("run 1 over: win")), "{logs:?}");
}

#[test]
fn test_reveals_unlocks_on_the_timeline_before_starting_a_run() {
    let mut mem = Memory::new();
    let main = json!({"state_type": "menu", "menu_screen": "main", "options": ["timeline", "settings", "quit"], "pending_epoch_ids": ["NEOW_EPOCH"]});
    assert_eq!(strategy::menu(&main, &mut mem, "auto")[0], act("menu_select", json!({"option": "timeline"})));
    let timeline = json!({"state_type": "menu", "menu_screen": "timeline", "options": [{"name": "advance", "enabled": true}, {"name": "back", "enabled": true}]});
    assert_eq!(strategy::menu(&timeline, &mut mem, "auto")[0], act("menu_select", json!({"option": "advance"})));

    struct TimelineGame(Value);
    impl TimelineGame {
        fn state(&mut self) -> Value {
            self.0.clone()
        }
        fn act(&mut self, _action: &str, _params: &Value) -> Value {
            json!({"status": "ok", "message": "No more epochs to advance", "done": true})
        }
    }
    game_impl!(TimelineGame);

    let mut bot = make_bot(shared(TimelineGame(timeline.clone())));
    for _ in 0..3 {
        bot.step().unwrap(); // the mod says nothing is left, three times running ...
    }
    assert_eq!(strategy::menu(&timeline, &mut bot.mem, "auto")[0], act("menu_select", json!({"option": "back"})));
    // ... so back at the menu the bot plays instead of going round again.
    let main = json!({"state_type": "menu", "menu_screen": "main", "options": ["singleplayer", "timeline", "quit"]});
    assert_eq!(strategy::menu(&main, &mut bot.mem, "auto")[0], act("menu_select", json!({"option": "singleplayer"})));
    // A run in progress comes first; the Timeline can wait until it is over.
    let busy = json!({"state_type": "menu", "menu_screen": "main", "options": ["continue", "abandon_run", "timeline"], "pending_epoch_ids": ["SILENT2_EPOCH"]});
    assert_eq!(strategy::menu(&busy, &mut Memory::new(), "auto")[0], act("menu_select", json!({"option": "continue"})));
}

/// The first click on a "choose 1 of 3" screen is often ignored by the game.
#[test]
fn test_choose_screen_click_is_retried_quickly() {
    #[derive(Default)]
    struct ChooseGame {
        clicks: i64,
    }
    impl ChooseGame {
        fn state(&mut self) -> Value {
            let done = self.clicks >= 2;
            json!({"state_type": if done { "map" } else { "card_select" }, "card_select": {"screen_type": "choose", "prompt": "Choose a card.", "cards": if done { vec![] } else { vec![STRIKE(0)] }}})
        }
        fn act(&mut self, _action: &str, _params: &Value) -> Value {
            self.clicks += 1;
            json!({"status": "ok", "message": "Choosing card"})
        }
    }
    game_impl!(ChooseGame);

    let (game, clock) = (shared(ChooseGame::default()), FakeClock::new());
    let mut bot = Bot::new(game.clone(), BotArgs { clock: clock.clock_fn(), sleep: clock.sleep_fn(), ..Default::default() });
    bot.step().unwrap();
    bot.step().unwrap();
    assert_eq!(game.borrow().clicks, 2);
    assert!(clock.t.get() < 1.5);
}

// CrashLoop

#[test]
fn test_what_was_asked_when_the_game_died_is_not_asked_again_in_that_room() {
    /// A shop with two things to buy, where buying the first kills the game.
    #[derive(Default)]
    struct Shop {
        actions: Vec<(String, Value)>,
    }
    impl Shop {
        fn state(&mut self) -> Value {
            let items: Vec<Value> = [0, 1].iter().map(|i| json!({"index": i, "category": "relic", "price": 50, "is_stocked": true, "can_afford": true})).collect();
            json!({"state_type": "shop", "run": {"act": 3, "floor": 46}, "player": {"hp": 50, "max_hp": 80, "gold": 300, "potions": []},
                   "shop": {"items": items, "can_proceed": true}})
        }
        fn act(&mut self, action: &str, params: &Value) -> Value {
            self.actions.push((action.to_string(), params.clone()));
            json!({"status": "ok", "message": "ok"})
        }
    }
    game_impl!(Shop);

    let game = shared(Shop::default());
    let mut bot = make_bot(game.clone());
    bot.step().unwrap();
    let first = game.borrow().actions.last().unwrap().clone();
    assert_eq!(first.0, "shop_purchase");
    let (crashed, _) = assert_logs("sts2bot", Level::Warning, || bot.note_crash());
    assert!(crashed); // the game died: that purchase is the suspect
    assert!(!bot.note_crash()); // (nothing more was asked since)
    bot._strikes.clear();
    bot.step().unwrap();
    assert_ne!(game.borrow().actions.last().unwrap(), &first); // back in the same shop, it buys the other thing
    // Slow play is for the room that crashed, not for the ones after it.
    (bot.careful_until, bot.careful_at) = (bot.clock() + 120.0, Some(Progress(0, Some((Some(3), Some(46))))));
    bot._last_where = Some((Some(3), Some(46)));
    assert!(bot._careful());
    bot._last_where = Some((Some(3), Some(47)));
    assert!(!bot._careful());
}

// HttpClient
// The real Client against a local HTTP server that fronts the fake game.

/// One request off the socket: (method, path, body).
fn read_request(sock: &mut std::net::TcpStream) -> Option<(String, String, Vec<u8>)> {
    let mut raw = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        match sock.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => raw.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&raw[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut first = lines.next()?.split(' ');
    let (method, path) = (first.next()?.to_string(), first.next()?.to_string());
    let length: usize = lines.filter_map(|l| l.split_once(':')).find(|(k, _)| k.eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.trim().parse().ok()).unwrap_or(0);
    let mut body = raw[head_end + 4..].to_vec();
    while body.len() < length {
        match sock.read(&mut chunk) {
            Ok(0) | Err(_) => return None,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
        }
    }
    Some((method, path, body))
}

#[test]
fn test_round_trip_and_errors() {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    let server = std::thread::spawn(move || {
        let mut game = FakeGame::seeded(6, 2);
        for sock in listener.incoming() {
            if stopping.load(Ordering::SeqCst) {
                break;
            }
            let Ok(mut sock) = sock else { continue };
            let Some((method, path, body)) = read_request(&mut sock) else { continue };
            let (code, answer) = if method == "GET" {
                (200, if path.starts_with("/api") { game.state() } else { json!({"status": "ok"}) })
            } else {
                let mut body: Value = serde_json::from_slice(&body).unwrap();
                let action = body.as_object_mut().unwrap().shift_remove("action").unwrap();
                let result = game.act(action.as_str().unwrap(), &body);
                (if result["status"] == "ok" { 200 } else { 400 }, result)
            };
            let raw = sts2bot::py::json_dumps(&answer).into_bytes();
            let _ = sock.write_all(format!("HTTP/1.0 {} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", code, raw.len()).as_bytes());
            let _ = sock.write_all(&raw);
        }
    });
    let client = Client::on_port(port);
    let played = std::panic::catch_unwind(|| {
        assert!(client.ping());
        assert_eq!(client.state().unwrap()["state_type"], "menu");
        assert_eq!(client.act("end_turn", &json!({})).unwrap()["status"], "error");
        let mut bot = Bot::new(shared(client.clone()), BotArgs { poll: 0.0, settle: 0.05, ..Default::default() });
        bot.run(Some(1)).unwrap();
        assert_eq!(bot.runs_finished, 1);
    });
    // (server.shutdown(); server.server_close())
    stop.store(true, Ordering::SeqCst);
    let _ = std::net::TcpStream::connect(("127.0.0.1", port));
    server.join().unwrap();
    if let Err(e) = played {
        std::panic::resume_unwind(e);
    }
    assert!(client.state().is_err()); // (GameUnavailable)
    assert!(!client.ping());
}

#[test]
fn test_chunked_answers_are_read_whole() {
    assert_eq!(_dechunk(b"4\r\n{\"a\"\r\n5;x=1\r\n: 12}\r\n0\r\n\r\n").unwrap(), b"{\"a\": 12}".to_vec());
}
