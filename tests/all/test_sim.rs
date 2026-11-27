//! The model-run simulator (sim/): every first-game card plays without error, and a fight of each game runs to its end.

use std::rc::Rc;

use serde_json::{json, Value};
use sts2bot::games;
use sts2bot::learn;
use sts2bot::logging;
use sts2bot::py::{self, JsonExt};
use sts2bot::sim::cards::Card;
use sts2bot::sim::content::{self, sts1, sts2, Content};
use sts2bot::sim::engine::Fight;
use sts2bot::sim::run::Run;

/// Puts the game and the log back when dropped (the Python's setUp and tearDown).
struct Scene(games::Playing);

impl Drop for Scene {
    fn drop(&mut self) {
        logging::disable(None);
    }
}

fn _content(game: &str) -> (Scene, Rc<Content>) {
    logging::disable(Some(logging::Level::Critical));
    learn::update_knobs(&learn::DEFAULTS.iter().map(|(k, v)| (k.to_string(), *v)).collect());
    let scene = Scene(games::playing(game));
    (scene, content::content(game))
}

fn pile(cards: &[Card]) -> String {
    cards.iter().map(|c| format!("{}{}:{}:{}", c.key(), if c.up { "+" } else { "" }, c.cost(), c.bonus)).collect::<Vec<_>>().join(" ")
}

/// The fight as tests/golden/sim_sts1_cards.json keeps it (and one draw of its random generator).
fn snap(f: &mut Fight) -> Value {
    let pairs = |d: &indexmap::IndexMap<String, i64>| d.iter().map(|(k, v)| json!([k, v])).collect::<Vec<_>>();
    let foes: Vec<Value> = f.foes.iter().map(|x| json!([x.eid, x.hp, x.max_hp, x.block, pairs(&x.powers), x.move_name, x.gone, x.half_dead])).collect();
    json!({"hp": f.run.hp, "max_hp": f.run.max_hp, "gold": f.run.gold, "block": f.block, "energy": f.energy, "pw": pairs(&f.pw), "temp": pairs(&f.temp),
           "hand": pile(&f.hand), "draw": pile(&f.draw), "discard": pile(&f.discard), "exhausted": pile(&f.exhausted),
           "orbs": f.orbs.iter().map(|o| json!([o.kind, o.dark])).collect::<Vec<_>>(), "slots": f.orb_slots, "stance": f.stance, "mantra": f.mantra,
           "foes": foes, "won": f.won, "lost": f.lost, "potions": f.run.potions.iter().map(|p| p.as_ref().map(|p| p.key.clone())).collect::<Vec<_>>(),
           "turn_cards": f.turn_cards, "round": f.round, "rng": f.run.rng.random()})
}

fn jaw_worm_and_cultist(g: &Rc<Content>, character: &str, seed: i64) -> Fight {
    let mut run = Run::new(g, character, seed, None, 0);
    let foes = vec![sts1::_new("JAW_WORM", 0, &mut run, "monster", None), sts1::_new("CULTIST", 1, &mut run, "monster", None)];
    Fight::new(run, foes, "monster")
}

#[test]
fn test_every_first_game_card_plays() {
    let (_scene, sts1) = _content("sts1");
    for d in sts1.CARDS.values() {
        for up in [false, true] {
            let mut f = jaw_worm_and_cultist(&sts1, "IRONCLAD", 1);
            f.start();
            f.player_turn_start();
            f.hand.push(Card::new(d, up));
            f.energy = 5;
            if f.playable(f.hand.last().unwrap()) {
                let target = f.foes[0].eid.clone();
                f.play(f.hand.len() as i64 - 1, Some(&target));
            }
            f.state();
            f.player_turn_end();
            f.enemies_turn();
        }
    }
}

/// Each card of the first game played once (plain and upgraded) by its own character, as the Python model plays it:
/// what the bot is shown, the fight after the play, and the fight as the next turn begins.
#[test]
fn test_every_first_game_card_does_what_the_pythons_does() {
    let (_scene, sts1) = _content("sts1");
    let golden: Value = serde_json::from_str(include_str!("../golden/sim_sts1_cards.json")).unwrap();
    let mut checked = 0;
    for case in golden.list() {
        let why = format!("{}{} played by {}", case["key"].s(), if case["up"].truthy() { "+" } else { "" }, case["who"].s());
        let mut f = jaw_worm_and_cultist(&sts1, case["who"].s(), case["seed"].int());
        f.start();
        f.player_turn_start();
        f.hand.push(Card::new(&sts1.CARDS[case["key"].s()], case["up"].truthy()));
        f.energy = 5;
        let can = f.playable(f.hand.last().unwrap());
        let target = f.foes[0].eid.clone();
        let played = can && f.play(f.hand.len() as i64 - 1, Some(&target));
        assert_eq!((can, played), (case["can"].truthy(), case["played"].truthy()), "{why}");
        let state = f.state();
        let seen = json!([
            state["player"]["status"].list().iter().map(|s| format!("{}={}", s["id"].s(), s["amount"])).collect::<Vec<_>>().join(" "),
            state["player"]["hand"].list().iter().map(|c| format!("{}:{}:{}", c["id"].s(), c["cost"].s(), c["can_play"].truthy() as i64)).collect::<Vec<_>>().join(" "),
            state["battle"]["enemies"].list().iter().map(|e| format!("{}:{}", e["entity_id"].s(), e["intents"].list().iter().map(|i| format!("{}{}", i["type"].s(), i["label"].s())).collect::<Vec<_>>().join(","))).collect::<Vec<_>>().join(" "),
        ]);
        assert_eq!(py::json_dumps(&seen), py::json_dumps(&case["seen"]), "{why}: what the bot sees");
        assert_eq!(py::json_dumps(&snap(&mut f)), py::json_dumps(&case["first"]), "{why}: after the play");
        f.player_turn_end();
        f.enemies_turn();
        snap(&mut f); // (the Python took a snapshot here too: one draw of the generator)
        if !(f.won || f.lost) {
            f.player_turn_start();
        }
        assert_eq!(py::json_dumps(&snap(&mut f)), py::json_dumps(&case["next"]), "{why}: as the next turn begins");
        checked += 1;
    }
    assert!(checked > 700, "{checked} golden cases");
}

#[test]
fn test_a_fight_of_each_game_ends() {
    for (game, character, foes) in [("sts1", "WATCHER", ["CULTIST"]), ("sts2", "REGENT", ["NIBBIT"])] {
        let (_scene, g) = _content(game);
        let mut run = Run::new(&g, character, 3, None, 0);
        run.act_id = "OVERGROWTH".to_string();
        run.note_deck();
        let foes = foes.iter().enumerate().map(|(n, k)| if game == "sts2" { sts2::_new(k, n as i64, &mut run, 0, false, None) } else { sts1::_new(k, n as i64, &mut run, "monster", None) }).collect();
        let mut f = Fight::new(run, foes, "monster");
        f.run_fight();
        assert!(f.won || f.lost);
        assert!(f.round > 0);
    }
}

fn assert_almost_equal(a: f64, b: f64) {
    assert!((a - b).abs() < 5e-8, "{a} != {b}");
}

#[test]
fn test_each_game_keeps_its_own_ascension_rules() {
    {
        let (_scene, g) = _content("sts2");
        let run = Run::new(&g, "IRONCLAD", 1, None, 7);
        assert_eq!(run.hp, run.max_hp); // (a damaged start is the first game's A6)
        assert_eq!((g.boss_heal)(&run), 0.0); // (the next act's Ancient heals: act_start)
        let rare = |odds: &sts2bot::sim::content::Odds| odds["monster"].iter().find(|(r, _)| r == "Rare").unwrap().1;
        assert_almost_equal(rare(&sts2::SCARCE_ODDS()), rare(&sts2::CARD_ODDS()) / 2.0); // A7: half the rares ...
        assert_almost_equal(py::fsum(sts2::SCARCE_ODDS()["elite"].iter().map(|(_r, p)| *p)), 1.0); // ... the rest commons
    }
    let (_scene, sts1) = _content("sts1");
    let run = Run::new(&sts1, "IRONCLAD", 1, None, 6);
    assert_eq!(run.hp, py::trunc(run.max_hp as f64 * 0.9));
    assert_eq!((sts1.boss_heal)(&run), 0.75);
}

/// A run's line as the card-table tests write it.
fn table_run(floor: i64, picks: Value, lean: f64) -> Value {
    json!({"game": "sts2", "character": "SILENT", "floor": floor, "won": false, "picks": picks, "knobs": {}, "model_lean": lean})
}

fn lines(runs: &[Value]) -> String {
    runs.iter().map(|r| py::json_dumps(r) + "\n").collect()
}

fn sim_main(args: &[&str]) -> i32 {
    let _playing = games::playing("sts2");
    sts2bot::sim::cli::main(args.iter().map(|a| a.to_string()).collect())
}

#[test]
fn test_the_card_table_learns_only_from_runs_that_did_not_use_it() {
    let mut runs: Vec<Value> = (0..5).map(|_| table_run(30, json!([[2, "FOOTWORK"]]), 0.0)).collect();
    runs.extend((0..5).map(|_| table_run(20, json!([[2, "DEFLECT"]]), 0.0)));
    runs.extend((0..50).map(|_| table_run(48, json!([[2, "DEFLECT"]]), 1.0))); // (played with the table: left out)
    let d = tempfile::tempdir().unwrap();
    let (src, out) = (d.path().join("runs.jsonl"), d.path().join("cards.json"));
    std::fs::write(&src, lines(&runs)).unwrap();
    assert_eq!(sim_main(&["prior", src.to_str().unwrap(), "--out", out.to_str().unwrap(), "--prior", "10"]), 0);
    let table: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    let table = &table["sts2"]["SILENT"];
    // Par is 25: Footwork's five picks 5 floors over it, Deflect's 5 under, each shrunk to 5/15 of that.
    assert_almost_equal(table["FOOTWORK"].as_f64().unwrap(), py::round_to(25.0 / 15.0, 2));
    assert_almost_equal(table["DEFLECT"].as_f64().unwrap(), py::round_to(-25.0 / 15.0, 2));
}

#[test]
fn test_the_card_table_can_learn_on_top_of_itself() {
    let lean = learn::GAME_DEFAULTS["sts2"]["model_lean"];
    let mut runs: Vec<Value> = (0..5).map(|_| table_run(30, json!([[2, "FOOTWORK"]]), lean)).collect();
    runs.extend((0..5).map(|_| table_run(20, json!([[2, "DEFLECT"]]), lean)));
    runs.extend((0..50).map(|_| table_run(48, json!([[2, "DEFLECT"]]), 0.0))); // (not played with the table at the game's lean: left out)
    let d = tempfile::tempdir().unwrap();
    let (src, base, out) = (d.path().join("runs.jsonl"), d.path().join("base.json"), d.path().join("cards.json"));
    std::fs::write(&src, lines(&runs)).unwrap();
    std::fs::write(&base, py::json_dumps(&json!({"sts2": {"SILENT": {"FOOTWORK": 1.0, "DEFLECT": -2.5, "NOXIOUS_FUMES": 0.5}}, "sts1": {"IRONCLAD": {"BASH": 1.0}}}))).unwrap();
    assert_eq!(sim_main(&["prior", src.to_str().unwrap(), "--out", out.to_str().unwrap(), "--prior", "10", "--onto", base.to_str().unwrap()]), 0);
    let table: Value = serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
    // What the runs still say (as above, +-25/15) is added to the base, within --most; cards and games without runs keep theirs.
    assert_eq!(table["sts2"]["SILENT"], json!({"FOOTWORK": py::round_to(1.0 + 25.0 / 15.0, 2), "DEFLECT": -3.0, "NOXIOUS_FUMES": 0.5}));
    assert_eq!(table["sts1"], json!({"IRONCLAD": {"BASH": 1.0}}));
}

#[test]
fn test_the_owl_magistrate_lands() {
    let (_scene, g) = _content("sts2");
    let mut run = Run::new(&g, "IRONCLAD", 1, None, 0);
    run.act_id = "GLORY".to_string();
    run.note_deck();
    let owl = sts2::_new("OWL_MAGISTRATE", 0, &mut run, 1, false, None); // (the Python passes "monster" for the slot: not the first)
    let mut f = Fight::new(run, vec![owl], "monster");
    f.start();
    let mut seen = Vec::new();
    for _ in 0..4 {
        f.player_turn_start();
        f.player_turn_end();
        f.enemies_turn();
        let owl = &f.foes[0];
        seen.push((owl.history.last().unwrap().clone(), owl.powers.get("SOAR").is_some_and(|n| *n != 0)));
    }
    // Half damage from attacks while it flies, the round after Judicial Flight; Verdict brings it down.
    let want = [("MAGISTRATE_SCRUTINY", false), ("PECK_ASSAULT", false), ("JUDICIAL_FLIGHT", true), ("VERDICT", false)];
    assert_eq!(seen, want.iter().map(|(m, s)| (m.to_string(), *s)).collect::<Vec<_>>());
}

#[test]
fn test_a_new_axebot_starts_afresh() {
    let (_scene, g) = _content("sts2");
    let mut run = Run::new(&g, "IRONCLAD", 1, None, 0);
    run.act_id = "GLORY".to_string();
    run.note_deck();
    let bot = sts2::_new("AXEBOT", 0, &mut run, 1, false, None);
    let mut f = Fight::new(run, vec![bot], "monster");
    f.start();
    f.foes[0].powers.insert("STRENGTH".to_string(), 6);
    f.foes[0].powers.insert("VULNERABLE".to_string(), 2);
    let hp = f.foes[0].max_hp;
    f.kill(0);
    let bot = &f.foes[0];
    assert!(bot.alive());
    assert_eq!(bot.powers, indexmap::IndexMap::from([("STOCK".to_string(), 1)])); // (Stock 2 at the start: one replacement used)
    assert_eq!(bot.max_hp, hp + 10);
}

#[test]
fn test_grand_finale_waits_for_an_empty_draw_pile() {
    // (Until 2026-11-26 no "Can only be played if" held in Slay the Spire 2's model: 60 damage to ALL enemies whenever drawn.)
    let (_scene, g) = _content("sts2");
    let mut run = Run::new(&g, "SILENT", 1, None, 0);
    run.act_id = "OVERGROWTH".to_string();
    run.note_deck();
    let foe = sts2::_new("NIBBIT", 0, &mut run, 0, false, None);
    let mut f = Fight::new(run, vec![foe], "monster");
    f.start();
    f.player_turn_start();
    f.hand.push(Card::new(&g.CARDS["GRAND_FINALE"], false));
    assert!(!f.draw.is_empty());
    assert!(!f.playable(f.hand.last().unwrap()));
    f.draw.clear();
    assert!(f.playable(f.hand.last().unwrap()));
}

#[test]
fn test_what_a_card_text_says_beyond_the_parse_happens() {
    // (2026-11-26: Bone Shards left Osty alive, Fight Me gave the enemy nothing, statuses and copies were never added, Hyperbeam
    // cost no Focus; the first game's Reprogram gave its Strength and Dexterity twice.)
    fn played(game: &str, character: &str, key: &str, setup: impl Fn(&mut Fight)) -> Fight {
        let (_scene, g) = _content(game);
        let mut run = Run::new(&g, character, 1, None, 0);
        run.act_id = "OVERGROWTH".to_string();
        run.note_deck();
        let foe = if game == "sts2" { sts2::_new("NIBBIT", 0, &mut run, 0, false, None) } else { sts1::_new("JAW_WORM", 0, &mut run, "monster", None) };
        let mut f = Fight::new(run, vec![foe], "monster");
        f.start();
        f.player_turn_start();
        f.energy = 6;
        setup(&mut f);
        f.hand.push(Card::new(&g.CARDS[key], false));
        let target = f.foes[0].eid.clone();
        f.play(f.hand.len() as i64 - 1, Some(&target));
        f
    }

    assert_eq!(played("sts2", "NECROBINDER", "BONE_SHARDS", |f| f.osty = 5).osty, 0);
    assert_eq!(played("sts2", "NECROBINDER", "BONE_SHARDS", |f| f.osty = 0).block, 0); // ("If Osty is alive")
    assert_eq!(played("sts2", "IRONCLAD", "FIGHT_ME", |_| {}).foes[0].powers.get("STRENGTH"), Some(&1));
    assert_eq!(played("sts2", "DEFECT", "FIGHT_THROUGH", |_| {}).discard.iter().filter(|c| c.key() == "WOUND").count(), 2);
    let mut twins: Vec<i64> = played("sts2", "DEFECT", "ADAPTIVE_STRIKE", |_| {}).discard.iter().filter(|c| c.key() == "ADAPTIVE_STRIKE").map(|c| c.cost()).collect();
    twins.sort();
    assert_eq!(twins, [0, 2]); // (the card itself, and its copy for nothing)
    let mut beam = played("sts2", "DEFECT", "HYPERBEAM", |f| f.channel("LIGHTNING"));
    assert_eq!((beam.focus(), beam.orb_passive(&beam.orbs[0])), (-3, 0));
    beam.player_turn_end();
    assert_eq!(beam.focus(), 0); // ("this turn")
    let stocked = |f: &mut Fight| {
        for k in ["STRIKE_NECROBINDER", "DEFEND_NECROBINDER", "UNLEASH"] {
            let c = Card::new(&f.game.CARDS[k], false);
            f.discard.push(c);
        }
    };
    let dredged = played("sts2", "NECROBINDER", "DREDGE", stocked);
    assert_eq!((dredged.hand.len(), dredged.discard.len()), (8, 0)); // (Dredge itself is Exhausted)
    let thrash = played("sts2", "IRONCLAD", "THRASH", |_| {});
    assert_eq!((thrash.hand.len(), thrash.exhausted.len()), (4, 1)); // (a Strike of the five Exhausted)
    let mut foregone = played("sts2", "REGENT", "FOREGONE_CONCLUSION", |_| {});
    foregone.player_turn_end();
    foregone.enemies_turn();
    foregone.player_turn_start();
    assert_eq!(foregone.hand.len(), 7);
    let reprogram = played("sts1", "DEFECT", "REPROGRAM", |_| {});
    assert_eq!((reprogram.strength(), reprogram.dexterity(), reprogram.focus()), (1, 1, -1));
}
