//! The model-run simulator (sim/): every first-game card plays without error, and a fight of each game runs to its end.

use std::rc::Rc;

use serde_json::{json, Value};
use sts2bot::games;
use sts2bot::learn;
use sts2bot::logging;
use sts2bot::py::{self, JsonExt};
use sts2bot::sim::cards::Card;
use sts2bot::sim::content::{self, sts1, Content};
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
    // TODO(sts2 content): ("sts2", "REGENT", ["NIBBIT"], 0)
    for (game, character, foes, kind) in [("sts1", "WATCHER", ["CULTIST"], "monster")] {
        let (_scene, g) = _content(game);
        let mut run = Run::new(&g, character, 3, None, 0);
        run.act_id = "OVERGROWTH".to_string();
        run.note_deck();
        let foes = foes.iter().enumerate().map(|(n, k)| sts1::_new(k, n as i64, &mut run, kind, None)).collect();
        let mut f = Fight::new(run, foes, "monster");
        f.run_fight();
        assert!(f.won || f.lost);
        assert!(f.round > 0);
    }
}

#[test]
fn test_each_game_keeps_its_own_ascension_rules() {
    // TODO(sts2 content): the sts2 half (no damaged start at A7, boss_heal 0.0, SCARCE_ODDS half the rares and the rest commons)
    let (_scene, sts1) = _content("sts1");
    let run = Run::new(&sts1, "IRONCLAD", 1, None, 6);
    assert_eq!(run.hp, py::trunc(run.max_hp as f64 * 0.9));
    assert_eq!((sts1.boss_heal)(&run), 0.75);
}

// TODO(sts2 content): test_the_card_table_learns_only_from_runs_that_did_not_use_it (needs `sim prior`: the rest of sim/cli.rs)
// TODO(sts2 content): test_the_card_table_can_learn_on_top_of_itself (needs `sim prior`)
// TODO(sts2 content): test_the_owl_magistrate_lands
// TODO(sts2 content): test_a_new_axebot_starts_afresh
// TODO(sts2 content): test_grand_finale_waits_for_an_empty_draw_pile

#[test]
fn test_what_a_card_text_says_beyond_the_parse_happens() {
    // (2026-11-26: Bone Shards left Osty alive, Fight Me gave the enemy nothing, statuses and copies were never added, Hyperbeam
    // cost no Focus; the first game's Reprogram gave its Strength and Dexterity twice.)
    fn played(game: &str, character: &str, key: &str, setup: impl Fn(&mut Fight)) -> Fight {
        let (_scene, g) = _content(game);
        let mut run = Run::new(&g, character, 1, None, 0);
        run.act_id = "OVERGROWTH".to_string();
        run.note_deck();
        let foe = sts1::_new("JAW_WORM", 0, &mut run, "monster", None); // TODO(sts2 content): mod._new("NIBBIT", 0, run, 0) for sts2
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

    // TODO(sts2 content): BONE_SHARDS (twice), FIGHT_ME, FIGHT_THROUGH, ADAPTIVE_STRIKE, HYPERBEAM, DREDGE, THRASH, FOREGONE_CONCLUSION
    let reprogram = played("sts1", "DEFECT", "REPROGRAM", |_| {});
    assert_eq!((reprogram.strength(), reprogram.dexterity(), reprogram.focus()), (1, 1, -1));
}
